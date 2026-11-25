/**
 * 快照游标：客户端侧的增量订阅状态机。
 *
 * 与 Rust 侧 `protocol::desktop::{SnapshotDelta, SnapshotUpdate}` 语义一一对应，
 * 负责全量/增量帧的接收顺序校验、会话切换与修订号单调性保障。
 */
import type { AppSnapshot, SnapshotDelta, SnapshotUpdate, StateInvalidated } from './types';

/**
 * 校验对象是否为携带合法会话 ID 与十进制修订号的状态载荷。
 * 修订号以字符串传输（避免 JS Number 精度丢失），且不得超过 u64 上限。
 */
export function validVersion(value: unknown): value is StateInvalidated {
  if (!value || typeof value !== 'object') return false;
  const version = value as StateInvalidated;
  return (
    typeof version.sessionId === 'string' &&
    version.sessionId.length > 0 &&
    typeof version.revision === 'string' &&
    /^(0|[1-9]\d*)$/.test(version.revision) &&
    BigInt(version.revision) <= 18446744073709551615n
  );
}

/**
 * 将增量 delta 应用到基线快照，返回新快照。
 * 顺序：先剔除移除项 → 同 ID 变更覆盖 → 追加基线中不存在的新实体；
 * settings 与运行时状态为空时沿用基线。
 */
export function applySnapshotDelta(
  base: AppSnapshot,
  delta: SnapshotDelta,
  sessionId: string,
  revision: string,
): AppSnapshot {
  const removedJobs = new Set(delta.jobsRemoved);
  const changedJobsMap = new Map(delta.jobsChanged.map((j) => [j.id, j]));
  const updatedJobs = base.jobs
    .filter((j) => !removedJobs.has(j.id))
    .map((j) => changedJobsMap.get(j.id) ?? j);
  const existingJobIds = new Set(updatedJobs.map((j) => j.id));
  for (const changed of delta.jobsChanged) {
    if (!existingJobIds.has(changed.id)) {
      updatedJobs.push(changed);
      existingJobIds.add(changed.id);
    }
  }

  const removedPwds = new Set(delta.passwordsRemoved);
  const changedPwdsMap = new Map(delta.passwordsChanged.map((p) => [p.id, p]));
  const updatedPasswords = base.passwords
    .filter((p) => !removedPwds.has(p.id))
    .map((p) => changedPwdsMap.get(p.id) ?? p);
  const existingPwdIds = new Set(updatedPasswords.map((p) => p.id));
  for (const changed of delta.passwordsChanged) {
    if (!existingPwdIds.has(changed.id)) {
      updatedPasswords.push(changed);
      existingPwdIds.add(changed.id);
    }
  }

  const settings = delta.settings ? { ...delta.settings } : base.settings;
  const state = delta.state;

  return {
    jobs: updatedJobs,
    passwords: updatedPasswords,
    settings,
    running: state ? state.running : base.running,
    paused: state ? state.paused : base.paused,
    vaultError: state ? state.vaultError : base.vaultError,
    dataDirectory: state ? state.dataDirectory : base.dataDirectory,
    engineAvailable: state ? state.engineAvailable : base.engineAvailable,
    recoverableCount: state ? state.recoverableCount : base.recoverableCount,
    cacheUsedBytes: state ? state.cacheUsedBytes : base.cacheUsedBytes,
    sessionId,
    revision,
  };
}

/**
 * 订阅游标：跟踪最新接受的快照与其会话/修订号。
 *
 * 保证同一会话内修订号严格递增（重复或回放的帧被丢弃），
 * 服务端重启换会话后旧会话立即退役。
 */
export class SnapshotCursor {
  private session = '';
  private revision = -1n;
  /** 已退役的会话 ID：服务端重启后不再接受其历史帧。 */
  private retired = new Set<string>();
  private lastSnapshot: AppSnapshot | null = null;

  /** 当前接受的修订号（字符串形式）；尚无快照时为 null。 */
  get currentRevision(): string | null {
    return this.revision >= 0n ? this.revision.toString() : null;
  }

  get currentSession(): string | null {
    return this.session || null;
  }

  /** 最近一次接受的完整快照。 */
  get last(): AppSnapshot | null {
    return this.lastSnapshot;
  }

  /** 尝试接受一帧全量快照；重复/回退/退役会话的帧被拒绝。 */
  accept(next: AppSnapshot): boolean {
    if (!validVersion(next) || this.retired.has(next.sessionId)) return false;
    const revision = BigInt(next.revision);
    if (next.sessionId === this.session && revision <= this.revision) return false;
    if (this.session && next.sessionId !== this.session) this.retired.add(this.session);
    this.session = next.sessionId;
    this.revision = revision;
    this.lastSnapshot = next;
    return true;
  }

  /**
   * 应用一帧订阅更新（full 或 delta），返回最新快照；
   * 帧不合法、跨会话或修订号回退时返回 null（调用方应丢弃）。
   */
  applyUpdate(update: SnapshotUpdate): AppSnapshot | null {
    if (!update || !validVersion(update)) return null;

    if (update.full) {
      if (this.accept(update.full)) {
        return this.lastSnapshot;
      }
      return null;
    }

    if (update.delta) {
      if (!this.lastSnapshot || this.session !== update.sessionId) {
        return null;
      }
      const synthesized = applySnapshotDelta(
        this.lastSnapshot,
        update.delta,
        update.sessionId,
        update.revision,
      );
      if (this.accept(synthesized)) {
        return this.lastSnapshot;
      }
      return null;
    }

    return null;
  }
}
