import { listen } from '@tauri-apps/api/event';
import { t } from '../i18n';
import { SnapshotCursor, validVersion } from '../snapshot-cursor';
import type { AppSnapshot, SnapshotUpdate, StateInvalidated } from '../types';
import {
  call,
  daemonRequest,
  getActiveDaemon,
  getConnectionVersion,
  isDesktop,
  onConnectionChange,
  preview,
} from './connection';
import { checkResult, isRecord } from './guards';

export interface JobProgressUpdatePayload {
  jobId: string;
  stage?: string;
  progress?: number;
  completedBytes?: number;
  totalBytes?: number;
  currentFile?: string;
  bytesPerSecond?: number;
  remainingSeconds?: number;
  [key: string]: unknown;
}
/** 任务等待密码输入的事件载荷（对应协议 `NeedPasswordEvent`）。 */
export interface NeedPasswordEventPayload {
  jobId: string;
  archivePath?: string;
  [key: string]: unknown;
}
/** 任务进度监听器集合（Tauri 事件与守护进程推送共用分发）。 */
const progressListeners = new Set<(update: JobProgressUpdatePayload) => void>();
const needPasswordListeners = new Set<(event: NeedPasswordEventPayload) => void>();

/**
 * 原生 Tauri 事件监听器生命周期管理。
 *
 * 原生监听只在「桌面 + 非守护进程」模式有意义；守护进程模式由 `subscribe()`
 * 的 WebSocket 推送事件。使用单一监听器与代际令牌消除重复注册和切模式漏听问题。
 */
function createNativeEventBridge<T>(
  eventName: string,
  hasListeners: () => boolean,
  dispatch: (payload: T) => void,
) {
  let nativeUnlisten: (() => void) | null = null;
  let nativeSetup: Promise<void> | null = null;
  let listenerGeneration = 0;

  return () => {
    listenerGeneration += 1;
    const generation = listenerGeneration;
    if (!isDesktop || getActiveDaemon() || !hasListeners()) {
      const cleanup = nativeUnlisten;
      nativeUnlisten = null;
      nativeSetup = null;
      cleanup?.();
      return;
    }
    if (nativeUnlisten || nativeSetup) return;
    nativeSetup = (async () => {
      try {
        const unlisten = await listen<T>(eventName, (event) => {
          if (!event?.payload) return;
          dispatch(event.payload);
        });
        if (generation === listenerGeneration && isDesktop && !getActiveDaemon() && hasListeners())
          nativeUnlisten = unlisten;
        else unlisten();
      } catch {
        /* 监听器不可用；快照仍是权威来源。 */
      } finally {
        if (generation === listenerGeneration) nativeSetup = null;
      }
    })();
  };
}

const syncProgressListener = createNativeEventBridge<JobProgressUpdatePayload>(
  'job-progress',
  () => progressListeners.size > 0,
  (payload) => {
    progressListeners.forEach((listener) => listener(payload));
  },
);

const syncNeedPasswordListener = createNativeEventBridge<NeedPasswordEventPayload>(
  'need-password',
  () => needPasswordListeners.size > 0,
  (payload) => {
    needPasswordListeners.forEach((listener) => listener(payload));
  },
);

onConnectionChange(() => {
  syncProgressListener();
  syncNeedPasswordListener();
});

export function onProgress(callback: (update: JobProgressUpdatePayload) => void) {
  progressListeners.add(callback);
  syncProgressListener();
  return () => {
    progressListeners.delete(callback);
    syncProgressListener();
  };
}

export function onNeedPassword(callback: (event: NeedPasswordEventPayload) => void) {
  needPasswordListeners.add(callback);
  syncNeedPasswordListener();
  return () => {
    needPasswordListeners.delete(callback);
    syncNeedPasswordListener();
  };
}

async function fullSnapshotForUpdate(): Promise<AppSnapshot> {
  return isDesktop || getActiveDaemon() ? call<AppSnapshot>('snapshot') : preview;
}
async function snapshotUpdateForSub(sinceRevision?: string | null): Promise<SnapshotUpdate> {
  return call<SnapshotUpdate>('snapshot_update', { sinceRevision: sinceRevision ?? null });
}

export async function subscribe(
  callback: (snapshot: AppSnapshot) => void,
  onError?: (message: string) => void,
): Promise<() => void> {
  const activeDaemon = getActiveDaemon();
  if (activeDaemon) {
    const connection = { ...activeDaemon };
    const epoch = getConnectionVersion();
    const abort = new AbortController();
    let disposed = false,
      busy = false,
      pending = false;
    let ws: WebSocket | undefined;
    let timer: ReturnType<typeof setTimeout> | undefined;
    let reconnect: ReturnType<typeof setTimeout> | undefined;
    const current = () => !disposed && epoch === getConnectionVersion();
    const schedule = () => {
      if (!current() || timer !== undefined) return;
      timer = setTimeout(() => {
        timer = undefined;
        void update();
      }, 150);
    };
    const update = async () => {
      if (!current()) return;
      if (busy) {
        pending = true;
        return;
      }
      busy = true;
      try {
        const result = await daemonRequest(
          connection,
          '/api/v1/commands',
          'POST',
          { command: 'queue_snapshot' },
          abort.signal,
        );
        checkResult('queue_snapshot', result);
        if (current()) callback(result as AppSnapshot);
      } catch {
        if (current()) onError?.(t('当前状态读取未成功，请刷新后核对任务；停止入口仍可使用。'));
      } finally {
        busy = false;
        if (pending && current()) {
          pending = false;
          schedule();
        }
      }
    };
    const connect = async () => {
      if (!current()) return;
      try {
        const value = await daemonRequest(
          connection,
          '/api/v1/ws-ticket',
          'POST',
          undefined,
          abort.signal,
        );
        if (!current()) return;
        if (
          !isRecord(value) ||
          typeof value.ticket !== 'string' ||
          !value.ticket ||
          value.ticket.length > 256
        )
          throw new Error('Invalid ticket');
        const socketUrl = new URL('/api/v1/ws', connection.url);
        socketUrl.protocol = socketUrl.protocol === 'https:' ? 'wss:' : 'ws:';
        socketUrl.searchParams.set('ticket', value.ticket);
        ws = new WebSocket(socketUrl.toString());
        ws.onopen = schedule;
        ws.onmessage = (event) => {
          if (!current()) return;
          try {
            const parsed: unknown = JSON.parse(typeof event.data === 'string' ? event.data : '');
            if (!isRecord(parsed)) return;
            if (parsed.event === 'state_changed' || parsed.event === 'ready') schedule();
            else if (isRecord(parsed.data) && typeof parsed.data.jobId === 'string') {
              if (parsed.event === 'job_progress')
                progressListeners.forEach((listener) =>
                  listener(parsed.data as JobProgressUpdatePayload),
                );
              if (parsed.event === 'need_password')
                needPasswordListeners.forEach((listener) =>
                  listener(parsed.data as NeedPasswordEventPayload),
                );
            }
          } catch {
            /* Ignore malformed optional events; snapshots remain authoritative. */
          }
        };
        ws.onclose = () => {
          if (current())
            reconnect = setTimeout(() => {
              void connect();
            }, 2000);
        };
        ws.onerror = () => ws?.close();
      } catch {
        if (current()) {
          onError?.(t('状态订阅未建立，请使用刷新检查任务状态。'));
          reconnect = setTimeout(() => {
            void connect();
          }, 5000);
        }
      }
    };
    void connect();
    schedule();
    return () => {
      disposed = true;
      abort.abort();
      if (timer) clearTimeout(timer);
      if (reconnect) clearTimeout(reconnect);
      ws?.close();
    };
  }
  if (!isDesktop) return () => {};
  const nativeEpoch = getConnectionVersion();
  let disposed = false,
    timer: ReturnType<typeof setTimeout> | undefined,
    busy = false,
    pending = false;
  const currentNative = () => !disposed && nativeEpoch === getConnectionVersion();
  const cursor = new SnapshotCursor();
  let wanted: StateInvalidated | undefined;
  const retiredSessions = new Set<string>();
  let staleReads = 0;
  const schedule = () => {
    if (!currentNative() || timer !== undefined) return;
    timer = setTimeout(() => {
      timer = undefined;
      void update();
    }, 150);
  };
  const update = async () => {
    if (!currentNative()) return;
    if (busy) {
      pending = true;
      return;
    }
    busy = true;
    try {
      const updateResult = await snapshotUpdateForSub(cursor.currentRevision);
      if (!currentNative()) return;
      let s: AppSnapshot | null = null;
      if (updateResult && 'jobs' in updateResult) {
        if (cursor.accept(updateResult as unknown as AppSnapshot))
          s = updateResult as unknown as AppSnapshot;
      } else {
        s = cursor.applyUpdate(updateResult);
        if (!s) {
          const full = await fullSnapshotForUpdate();
          if (cursor.accept(full)) s = full;
        }
      }
      if (currentNative() && s) {
        if (!validVersion(s)) throw new Error('Invalid snapshot version');
        if (
          wanted &&
          (s.sessionId !== wanted.sessionId || BigInt(s.revision) < BigInt(wanted.revision))
        ) {
          if (++staleReads <= 2) pending = true;
          else onError?.(t('当前状态读取未成功，请刷新后核对任务；停止入口仍可使用。'));
        } else {
          staleReads = 0;
          callback(s);
        }
      }
    } catch {
      if (currentNative()) onError?.(t('当前状态读取未成功，请刷新后核对任务；停止入口仍可使用。'));
    } finally {
      busy = false;
      if (pending && currentNative()) {
        pending = false;
        schedule();
      }
    }
  };
  const unlisten = await listen<StateInvalidated>('state-changed', (event) => {
    if (!currentNative()) return;
    if (!validVersion(event?.payload)) {
      schedule();
      return;
    }
    const next = event.payload;
    if (retiredSessions.has(next.sessionId)) return;
    if (wanted?.sessionId === next.sessionId && BigInt(next.revision) <= BigInt(wanted.revision))
      return;
    if (wanted && wanted.sessionId !== next.sessionId) retiredSessions.add(wanted.sessionId);
    wanted = next;
    staleReads = 0;
    schedule();
  });
  schedule(); // Close the initial snapshot/listener-registration gap.
  return () => {
    disposed = true;
    if (timer) clearTimeout(timer);
    unlisten();
  };
}
