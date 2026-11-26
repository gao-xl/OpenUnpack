import { invoke } from '@tauri-apps/api/core';
import { t } from '../i18n';
import type { AppSnapshot } from '../types';
import { defaultSettings } from '../types';
import {
  checkResult,
  daemonOnlyCommands,
  engineLocalCommands,
  isRecord,
  LONG_RUNNING_DAEMON_COMMANDS,
  nativeCommands,
} from './guards';

export const isDesktop = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
const connectionListeners = new Set<() => void>();
export function onConnectionChange(cb: () => void) {
  connectionListeners.add(cb);
}
/** 浏览器模式下的占位空快照（桌面未启动时的 UI 骨架数据）。 */
export const preview: AppSnapshot = {
  jobs: [],
  passwords: [],
  settings: { ...defaultSettings },
  running: false,
  paused: false,
  vaultError: null,
  get dataDirectory() {
    return t('桌面版启动后显示');
  },
  engineAvailable: false,
  recoverableCount: 0,
  cacheUsedBytes: 0,
  sessionId: 'preview',
  revision: '0',
};

export interface DaemonConnection {
  url: string;
  token: string;
}
const tokenKey = 'openunpack_daemon_token';
const tokenUrlKey = 'openunpack_daemon_token_url';
const urlKey = 'openunpack_daemon_url';
const webConsole =
  typeof document !== 'undefined' &&
  document.querySelector('meta[name="openunpack-web-console"]')?.getAttribute('content') === '1';
export function isLoopbackHost(hostname: string): boolean {
  return hostname === '127.0.0.1' || hostname === 'localhost';
}
/** 校验并归一化守护进程根地址。
 *
 * 桌面模式仅接受本机回环 HTTP；浏览器模式仅接受当前页面同源服务。
 * 拒绝携带凭据、路径、查询参数或片段的地址。 */
export function daemonUrl(raw: string): string {
  let parsed: URL;
  try {
    parsed = new URL(raw.trim());
  } catch {
    throw new Error(t('守护进程地址格式无效，请填写完整地址。'));
  }
  if (
    parsed.username ||
    parsed.password ||
    parsed.search ||
    parsed.hash ||
    parsed.pathname !== '/'
  ) {
    throw new Error(t('请填写服务根地址，不要包含账户、路径、查询参数或片段。'));
  }
  if (isDesktop) {
    if (parsed.protocol !== 'http:' || !isLoopbackHost(parsed.hostname))
      throw new Error(
        t('桌面连接仅支持本机 HTTP 服务；远端服务请在浏览器打开其同源 HTTPS 控制台。'),
      );
  } else if (
    !['http:', 'https:'].includes(parsed.protocol) ||
    parsed.origin !== window.location?.origin
  ) {
    throw new Error(t('浏览器仅能连接当前页面的同源服务，请直接打开目标服务的控制台。'));
  }
  return parsed.origin;
}
let activeDaemon: DaemonConnection | null = (() => {
  try {
    const savedUrl = localStorage.getItem(urlKey);
    const target = savedUrl
      ? daemonUrl(savedUrl)
      : webConsole
        ? daemonUrl(window.location.origin)
        : null;
    if (!target) return null;
    // A session credential is bound to the endpoint that the user approved.
    const savedTokenUrl = sessionStorage.getItem(tokenUrlKey);
    let token = savedTokenUrl === target ? sessionStorage.getItem(tokenKey) || '' : '';
    const legacy = localStorage.getItem(tokenKey);
    if (!token && legacy && savedUrl && daemonUrl(savedUrl) === target) {
      token = legacy;
      sessionStorage.setItem(tokenKey, token);
      sessionStorage.setItem(tokenUrlKey, target);
    }
    localStorage.removeItem(tokenKey);
    if (isDesktop && !webConsole && !token) {
      // Desktop mode must never cold-boot into a stale localStorage daemon URL once sessionStorage is empty.
      return null;
    }
    return { url: target, token };
  } catch {
    try {
      localStorage.removeItem(tokenKey);
      sessionStorage.removeItem(tokenKey);
      sessionStorage.removeItem(tokenUrlKey);
    } catch {
      /* No persistent credential fallback. */
    }
    return null;
  }
})();
let connectionEpoch = 0;

export async function daemonRequest(
  connection: DaemonConnection,
  endpoint: string,
  method: 'GET' | 'POST',
  body?: unknown,
  signal?: AbortSignal,
  timeoutMs?: number,
): Promise<unknown> {
  const controller = new AbortController();
  const cancel = () => controller.abort();
  if (signal?.aborted) controller.abort();
  else signal?.addEventListener('abort', cancel, { once: true });
  const timer = setTimeout(cancel, timeoutMs ?? 15000);
  try {
    const response = await fetch(connection.url + endpoint, {
      method,
      redirect: 'error',
      credentials: 'omit',
      cache: 'no-store',
      signal: controller.signal,
      headers: {
        ...(body === undefined ? {} : { 'Content-Type': 'application/json' }),
        ...(connection.token ? { Authorization: 'Bearer ' + connection.token } : {}),
      },
      body: body === undefined ? undefined : JSON.stringify(body),
    });
    const payload: unknown = await response.json().catch(() => null);
    if (!response.ok) {
      const detail =
        isRecord(payload) && isRecord(payload.error) && typeof payload.error.message === 'string'
          ? payload.error.message
          : 'HTTP ' + response.status;
      throw new Error(
        connection.token ? detail.replaceAll(connection.token, '[REDACTED]') : detail,
      );
    }
    if (!isRecord(payload) || !Object.prototype.hasOwnProperty.call(payload, 'data'))
      throw new Error(t('守护进程返回的数据格式无效，请核对服务版本。'));
    return payload.data;
  } catch (error) {
    if (controller.signal.aborted)
      throw new Error(t('守护进程请求已取消或超时，请核对任务状态后重试。'));
    if (error instanceof TypeError)
      throw new Error(t('无法连接守护进程，请核对地址、网络与服务状态。'));
    throw error;
  } finally {
    clearTimeout(timer);
    signal?.removeEventListener('abort', cancel);
  }
}
export function formatNativeError(error: unknown): string {
  if (typeof error === 'string') return error;
  if (error instanceof Error) return error.message;
  if (isRecord(error)) {
    if (typeof error.message === 'string') {
      if (typeof error.code === 'string' && !error.message.includes(error.code)) {
        return `${error.code}: ${error.message}`;
      }
      return error.message;
    }
    if (typeof error.code === 'string') return error.code;
    if (typeof error.error === 'string') return error.error;
    try {
      const json = JSON.stringify(error);
      if (json && json !== '{}') return json;
    } catch {
      /* ignore circular */
    }
  }
  return t('操作未完成，请查看任务状态后重试。');
}
export async function callNative<T>(name: string, args?: Record<string, unknown>): Promise<T> {
  if (daemonOnlyCommands.has(name))
    throw new Error(t('此功能仅在连接 OpenUnpack 守护进程时可用。'));
  if (!isDesktop)
    throw new Error(
      activeDaemon
        ? t('此操作需要本机桌面客户端。')
        : t('此页面是界面预览，请打开桌面版使用文件与密码功能。'),
    );
  if (activeDaemon && engineLocalCommands.has(name))
    throw new Error(t('连接守护进程时不能对本机执行此操作。'));
  try {
    return await invoke<T>(name, args);
  } catch (error) {
    throw new Error(formatNativeError(error));
  }
}
export function getActiveDaemon() {
  return activeDaemon;
}
export function getConnectionVersion() {
  return connectionEpoch;
}
export function getDaemonConnection() {
  return { url: activeDaemon?.url ?? null, token: activeDaemon?.token ?? null };
}
export function isDaemonMode() {
  return activeDaemon !== null;
}
export function isWebConsole() {
  return webConsole;
}

export async function callDaemon<T>(
  command: string,
  args?: Record<string, unknown>,
  signal?: AbortSignal,
): Promise<T> {
  if (!activeDaemon) throw new Error(t('未配置守护进程连接。'));
  const timeoutMs = LONG_RUNNING_DAEMON_COMMANDS.has(command) ? 120000 : 15000;
  const value = await daemonRequest(
    activeDaemon,
    '/api/v1/commands',
    'POST',
    { command, ...(args ? { args } : {}) },
    signal,
    timeoutMs,
  );
  checkResult(command, value);
  return value as T;
}
export async function call<T>(name: string, args?: Record<string, unknown>): Promise<T> {
  if (daemonOnlyCommands.has(name) && !activeDaemon) {
    throw new Error(t('此功能仅在连接 OpenUnpack 守护进程时可用。'));
  }
  if (nativeCommands.has(name)) return callNative<T>(name, args);
  if (activeDaemon) return callDaemon<T>(name, args);
  return callNative<T>(name, args);
}
export function setDaemonConnection(url: string | null, token: string | null) {
  const next = url ? { url: daemonUrl(url), token: (token || '').trim() } : null;
  connectionEpoch += 1;
  activeDaemon = next;
  connectionListeners.forEach((fn) => fn());
  try {
    localStorage.removeItem(tokenKey);
    if (next) {
      localStorage.setItem(urlKey, next.url);
      sessionStorage.setItem(tokenKey, next.token);
      sessionStorage.setItem(tokenUrlKey, next.url);
    } else {
      localStorage.removeItem(urlKey);
      sessionStorage.removeItem(tokenKey);
      sessionStorage.removeItem(tokenUrlKey);
    }
  } catch {
    /* Continue only in memory when storage is unavailable. */
  }
}
export async function verifyDaemonConnection(url: string, token: string) {
  const candidate = { url: daemonUrl(url), token: token.trim() };
  if (!candidate.token) throw new Error(t('请填写 API 应用密钥。'));
  const health = await daemonRequest({ ...candidate, token: '' }, '/api/v1/health', 'GET');
  if (!isRecord(health) || health.service !== 'OpenUnpack' || health.apiVersion !== 1)
    throw new Error(t('目标服务不是兼容的 OpenUnpack API。'));
  const capabilities = await daemonRequest(candidate, '/api/v1/capabilities', 'GET');
  if (
    !isRecord(capabilities) ||
    capabilities.apiVersion !== 1 ||
    !Array.isArray(capabilities.commands) ||
    !capabilities.commands.includes('queue_snapshot')
  )
    throw new Error(t('目标服务不是兼容的 OpenUnpack API。'));
  return candidate;
}
export async function connectDaemon(url: string, token: string) {
  const candidate = await verifyDaemonConnection(url, token);
  setDaemonConnection(candidate.url, candidate.token);
}
// Native UI language is local configuration even while viewing a daemon.
let languageUpdate: Promise<void> = Promise.resolve();
export function setLanguage(language: 'zh-CN' | 'en-US' | 'ja-JP'): Promise<void> {
  if (!isDesktop) return Promise.resolve();
  const result = languageUpdate.then(() => callNative<void>('set_ui_language', { language }));
  languageUpdate = result.catch(() => {});
  return result;
}
/** 高频进度心跳事件的载荷（对应协议 `JobProgressUpdate`）。 */
