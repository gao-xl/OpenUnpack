import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { defaultSettings, type AppSnapshot } from '../src/types';
const native = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn() }));
vi.mock('@tauri-apps/api/core', () => ({ invoke: native.invoke }));
vi.mock('@tauri-apps/api/event', () => ({ listen: native.listen }));
vi.mock('../src/i18n', () => ({ t: (text: string) => text }));
const live: AppSnapshot = { jobs: [], passwords: [], settings: { ...defaultSettings }, running: false, paused: false, vaultError: null, dataDirectory: '', engineAvailable: true, recoverableCount: 0, cacheUsedBytes: 0, sessionId: 'synthetic-live', revision: '7' };
const response = (data: unknown, status = 200) => ({ ok: status >= 200 && status < 300, status, json: async () => status < 300 ? { requestId: 'fixture', data, meta: { apiVersion: 1 } } : { requestId: 'fixture', error: data } }) as Response;
beforeEach(() => {
 vi.resetModules(); vi.resetAllMocks(); localStorage.clear(); sessionStorage.clear();
 document.querySelector('meta[name="openunpack-web-console"]')?.remove();
 delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
 vi.stubGlobal('fetch', vi.fn());
});
afterEach(() => { vi.useRealTimers(); vi.unstubAllGlobals(); delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__; document.querySelector('meta[name="openunpack-web-console"]')?.remove(); localStorage.clear(); sessionStorage.clear(); });
function desktop() { Object.defineProperty(window, '__TAURI_INTERNALS__', { value: {}, configurable: true }); }
it('unconfigured browser previews stay offline; only the service marker opts into a web console', async () => {
 const first = await import('../src/api');
 expect(first.api.isDaemonMode()).toBe(false);
 expect((await first.api.snapshot()).sessionId).toBe('preview');
 expect(fetch).not.toHaveBeenCalled();
 document.head.insertAdjacentHTML('beforeend', '<meta name="openunpack-web-console" content="1">');
 vi.resetModules();
 const consoleApi = (await import('../src/api')).api;
 expect(consoleApi.isDaemonMode()).toBe(true);
 expect(consoleApi.getDaemonConnection().url).toBe(window.location.origin);
});
it('daemon calls unwrap the envelope and keep fetch redirects, credentials and time bounded', async () => {
 desktop(); const { api } = await import('../src/api');
 api.setDaemonConnection('http://127.0.0.1:47980', 'synthetic-key');
 vi.mocked(fetch).mockResolvedValue(response(live));
 expect(await api.snapshot()).toEqual(live);
 expect(fetch).toHaveBeenCalledWith('http://127.0.0.1:47980/api/v1/commands', expect.objectContaining({ redirect: 'error', credentials: 'omit', cache: 'no-store', signal: expect.any(AbortSignal), body: JSON.stringify({ command: 'queue_snapshot' }) }));
 expect(localStorage.getItem('openunpack_daemon_token')).toBeNull();
 expect(sessionStorage.getItem('openunpack_daemon_token')).toBe('synthetic-key');
});
it('malformed daemon snapshots fail before they can reach the mounted UI', async () => {
 desktop(); const { api } = await import('../src/api'); api.setDaemonConnection('http://127.0.0.1:47980', 'synthetic-key');
 vi.mocked(fetch).mockResolvedValue(response({ ...live, jobs: 'invalid' }));
 await expect(api.snapshot()).rejects.toThrow('数据格式无效');
 vi.mocked(fetch).mockResolvedValue({ ok: true, json: async () => live } as Response);
 await expect(api.snapshot()).rejects.toThrow('数据格式无效');
});
it('daemon errors preserve the nested explanation and redact an echoed authorization key', async () => {
 desktop(); const { api } = await import('../src/api'); api.setDaemonConnection('http://127.0.0.1:47980', 'synthetic-key');
 vi.mocked(fetch).mockResolvedValue(response({ message: 'permission denied: synthetic-key' }, 403));
 await expect(api.history()).rejects.toThrow('permission denied: [REDACTED]');
});
it('daemon snapshot updates use the supported queue snapshot command instead of an unavailable delta endpoint', async () => {
 desktop(); const { api } = await import('../src/api'); api.setDaemonConnection('http://127.0.0.1:47980', 'synthetic-key');
 vi.mocked(fetch).mockResolvedValue(response(live));
 expect(await api.snapshotUpdate('6')).toEqual({ sessionId: live.sessionId, revision: live.revision, full: live, delta: null });
 expect(JSON.parse(vi.mocked(fetch).mock.calls[0][1]?.body as string).command).toBe('queue_snapshot');
});
it('native API administration and language do not get sent to the connected daemon', async () => {
 desktop(); const { api } = await import('../src/api'); api.setDaemonConnection('http://127.0.0.1:47980', 'synthetic-key');
 native.invoke.mockResolvedValue(undefined);
 await api.apiStatus(); await api.apiConfigure({ enabled: false, port: 47980 }); await api.apiOpenDocs(); await api.apiRevokeClient('fixture-client'); await api.setLanguage('ja-JP');
 expect(native.invoke.mock.calls.map(call => call[0])).toEqual(['api_status', 'api_configure', 'api_open_docs', 'api_revoke_client', 'set_ui_language']);
 expect(fetch).not.toHaveBeenCalled();
 await expect(api.executePostAction('shutdown')).rejects.toThrow('不能对本机执行');
});
it('a new endpoint with an empty key never inherits a previous endpoint credential', async () => {
 desktop(); const { api } = await import('../src/api'); api.setDaemonConnection('http://127.0.0.1:47980', 'first-synthetic-key');
 api.setDaemonConnection('http://localhost:47981', '');
 vi.mocked(fetch).mockResolvedValue(response([])); await api.history();
 expect(vi.mocked(fetch).mock.calls[0][1]?.headers).not.toHaveProperty('Authorization');
 expect(api.getDaemonConnection().token).toBe('');
 expect(sessionStorage.getItem('openunpack_daemon_token_url')).toBe('http://localhost:47981');
});
it('only an explicitly supported root origin can receive a daemon credential', async () => {
 desktop(); const { api } = await import('../src/api');
 for (const url of ['http://user:pass@127.0.0.1:47980', 'http://127.0.0.1:47980/api/v1', 'http://127.0.0.1:47980?key=secret', 'http://127.0.0.1:47980/#fragment', 'http://127.0.0.1.evil.test:47980', 'https://remote.example']) {
  expect(() => api.setDaemonConnection(url, 'synthetic-key')).toThrow();
 }
 expect(fetch).not.toHaveBeenCalled();
 vi.resetModules(); delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
 const browserApi = (await import('../src/api')).api;
 expect(() => browserApi.setDaemonConnection('https://remote.example', 'synthetic-key')).toThrow('同源');
});
it('invalid legacy connection settings are discarded without contacting their origin', async () => {
 desktop(); localStorage.setItem('openunpack_daemon_url', 'http://untrusted.example'); localStorage.setItem('openunpack_daemon_token', 'synthetic-legacy');
 const { api } = await import('../src/api');
 expect(api.isDaemonMode()).toBe(false); expect(localStorage.getItem('openunpack_daemon_token')).toBeNull(); expect(fetch).not.toHaveBeenCalled();
});
it('a connection is committed only after compatibility and authenticated capability checks succeed', async () => {
 desktop(); const { api } = await import('../src/api'); api.setDaemonConnection('http://127.0.0.1:47980', 'old-key');
 vi.mocked(fetch).mockResolvedValueOnce(response({ service: 'OpenUnpack', apiVersion: 1 })).mockResolvedValueOnce(response({ message: 'bad key' }, 401));
 await expect(api.connectDaemon('http://localhost:47981', 'new-key')).rejects.toThrow('bad key');
 expect(api.getDaemonConnection()).toEqual({ url: 'http://127.0.0.1:47980', token: 'old-key' });
 vi.mocked(fetch).mockResolvedValueOnce(response({ service: 'OpenUnpack', apiVersion: 1 })).mockResolvedValueOnce(response({ apiVersion: 1, commands: ['queue_snapshot'] }));
 await api.connectDaemon('http://localhost:47981', 'new-key');
 expect(api.getDaemonConnection()).toEqual({ url: 'http://localhost:47981', token: 'new-key' });
 expect(vi.mocked(fetch).mock.calls[0][1]?.headers).not.toHaveProperty('Authorization');
 await expect(api.connectDaemon('http://localhost:47981', '')).rejects.toThrow('请填写');
});
class Socket {
 static opened: Socket[] = [];
 url: string;
 close = vi.fn();
 onopen = () => {};
 onmessage = (_event: MessageEvent) => {};
 onclose = () => {};
 onerror = () => {};
 constructor(url: string) { this.url = url; Socket.opened.push(this); }
}
it('web subscriptions mint a one-time ticket and never expose the bearer in the websocket URL', async () => {
 desktop(); const { api } = await import('../src/api'); api.setDaemonConnection('http://127.0.0.1:47980', 'synthetic-key');
 Socket.opened = []; vi.stubGlobal('WebSocket', Socket);
 vi.mocked(fetch).mockResolvedValue(response({ ticket: 'one-time-synthetic-ticket', expiresInSeconds: 15 }));
 const stop = await api.subscribe(vi.fn());
 await vi.waitFor(() => expect(Socket.opened).toHaveLength(1));
 expect(Socket.opened[0].url).toBe('ws://127.0.0.1:47980/api/v1/ws?ticket=one-time-synthetic-ticket');
 expect(Socket.opened[0].url).not.toContain('synthetic-key');
 expect(vi.mocked(fetch).mock.calls[0][0]).toBe('http://127.0.0.1:47980/api/v1/ws-ticket');
 stop(); expect(Socket.opened[0].close).toHaveBeenCalledTimes(1);
});
it('unsubscribing during ticket minting suppresses a late websocket and snapshot callback', async () => {
 desktop(); const { api } = await import('../src/api'); api.setDaemonConnection('http://127.0.0.1:47980', 'synthetic-key');
 Socket.opened = []; vi.stubGlobal('WebSocket', Socket);
 let resolve!: (value: Response) => void;
 vi.mocked(fetch).mockReturnValue(new Promise<Response>(yes => { resolve = yes; }));
 const receive = vi.fn(); const stop = await api.subscribe(receive); stop();
 resolve(response({ ticket: 'late-ticket', expiresInSeconds: 15 }));
 await Promise.resolve(); await Promise.resolve(); await Promise.resolve();
 expect(Socket.opened).toHaveLength(0); expect(receive).not.toHaveBeenCalled();
});

it('disconnect retires an in-flight daemon snapshot and ticket before a new native subscription', async () => {
 vi.useFakeTimers(); desktop(); native.listen.mockResolvedValue(() => {});
 native.invoke.mockResolvedValue(live);
 let finishSnapshot!: (value: Response) => void; let finishTicket!: (value: Response) => void;
 vi.mocked(fetch).mockImplementation(async (input) => {
  const url = typeof input === 'string' ? input : input instanceof URL ? input.href : input.url;
  return url.endsWith('/ws-ticket')
   ? new Promise<Response>(done => { finishTicket = done; })
   : new Promise<Response>(done => { finishSnapshot = done; });
 });
 const { api } = await import('../src/api'); api.setDaemonConnection('http://127.0.0.1:39111', 'synthetic-key');
 const receiveOld = vi.fn(); const oldError = vi.fn(); const stopOld = await api.subscribe(receiveOld, oldError);
 await vi.advanceTimersByTimeAsync(150); api.setDaemonConnection(null, null);
 finishSnapshot(response(live)); finishTicket(response({ ticket: 'retired-ticket', expiresInSeconds: 15 }));
 await vi.advanceTimersByTimeAsync(300);
 expect(receiveOld).not.toHaveBeenCalled(); expect(oldError).not.toHaveBeenCalled();
 const receiveNative = vi.fn(); const stopNative = await api.subscribe(receiveNative);
 await vi.advanceTimersByTimeAsync(150); expect(receiveNative).toHaveBeenCalledExactlyOnceWith(live);
 stopOld(); stopNative();
});
