import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { AppSnapshot } from '../src/types';
const native = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn(), unlisten: vi.fn(), event: (() => {}) as (event?: { payload: { sessionId: string; revision: string } }) => void }));
vi.mock('@tauri-apps/api/core', () => ({ invoke: native.invoke }));
vi.mock('@tauri-apps/api/event', () => ({ listen: native.listen }));

beforeEach(() => {
  vi.resetModules(); vi.resetAllMocks(); vi.useFakeTimers(); localStorage.clear(); sessionStorage.clear();
  Object.defineProperty(window, '__TAURI_INTERNALS__', { value: {}, configurable: true });
  native.listen.mockImplementation(async (_event, callback) => { native.event = callback; return native.unlisten; });
});
afterEach(() => { vi.useRealTimers(); delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__; });
const state = (number: number) => ({ jobs: [], cacheUsedBytes: number, sessionId: 'fixture', revision: String(number) }) as unknown as AppSnapshot;

describe('桌面实时状态接收', () => {
  it('日常快照读取有限队列，完整快照仅由显式兼容入口调用', async () => {
    native.invoke.mockResolvedValue(state(1));
    const { api } = await import('../src/api');
    await api.snapshot();
    expect(native.invoke).toHaveBeenLastCalledWith('queue_snapshot', undefined);
    await api.fullSnapshot();
    expect(native.invoke).toHaveBeenLastCalledWith('snapshot', undefined);
  });
  it('输出规则试算和取消保留独立请求身份', async () => {
    native.invoke.mockResolvedValue(undefined);
    const { api } = await import('../src/api');
    await api.previewExclusions(['temp/'], ['temp'], 'output', 'rules-1');
    await api.cancelPreviewExclusions('rules-1');
    expect(native.invoke).toHaveBeenNthCalledWith(1, 'preview_exclusions', { paths: ['temp/'], patterns: ['temp'], scope: 'output', requestId: 'rules-1' });
    expect(native.invoke).toHaveBeenNthCalledWith(2, 'cancel_preview_exclusions', { requestId: 'rules-1' });
  });
  it('归档检查与取消传递同一请求身份，密码空格完整保留', async () => {
    native.invoke.mockResolvedValue(undefined);
    const { api } = await import('../src/api');
    await api.inspectArchive('C:\\fixtures\\secret.zip', '  exact secret  ', 'inspection-1');
    await api.cancelInspectArchive('inspection-1');
    expect(native.invoke).toHaveBeenNthCalledWith(1, 'inspect_archive', {
      path: 'C:\\fixtures\\secret.zip', password: '  exact secret  ', inspectId: 'inspection-1',
    });
    expect(native.invoke).toHaveBeenNthCalledWith(2, 'cancel_inspect_archive', { inspectId: 'inspection-1' });
  });
  it('注册监听期间丢失的通知由注册后的快照补齐', async () => {
    let registered!: (cleanup: () => void) => void;
    native.listen.mockReturnValue(new Promise<() => void>(yes => { registered = yes; }));
    native.invoke.mockResolvedValue(state(3));
    const { api } = await import('../src/api'); const receive = vi.fn();
    const subscription = api.subscribe(receive);
    registered(native.unlisten);
    const stop = await subscription;
    await vi.advanceTimersByTimeAsync(150);
    expect(receive).toHaveBeenCalledExactlyOnceWith(state(3));
    stop();
  });
  it('新失效版本到达后不展示仍在读取的旧快照', async () => {
    let resolve!: (value: AppSnapshot) => void;
    native.invoke.mockReturnValueOnce(new Promise<AppSnapshot>(yes => { resolve = yes; })).mockResolvedValue(state(2));
    const { api } = await import('../src/api'); const receive = vi.fn();
    const stop = await api.subscribe(receive);
    await vi.advanceTimersByTimeAsync(150);
    native.event({ payload: { sessionId: 'fixture', revision: '2' } });
    resolve(state(1));
    await vi.advanceTimersByTimeAsync(300);
    expect(receive).toHaveBeenCalledExactlyOnceWith(state(2));
    stop();
  });
  it('更新中到来的事件合并后，后续事件仍能继续刷新', async () => {
    let resolve!: (value: AppSnapshot) => void;
    native.invoke.mockReturnValueOnce(new Promise<AppSnapshot>(yes => { resolve = yes; })).mockResolvedValueOnce(state(2)).mockResolvedValue(state(3));
    const { api } = await import('../src/api'); const receive = vi.fn();
    const stop = await api.subscribe(receive);
    native.event(); await vi.advanceTimersByTimeAsync(150);
    expect(native.invoke).toHaveBeenCalledTimes(1);
    native.event(); await vi.advanceTimersByTimeAsync(150);
    expect(native.invoke).toHaveBeenCalledTimes(1);
    resolve(state(1)); await vi.advanceTimersByTimeAsync(150);
    expect(native.invoke).toHaveBeenCalledTimes(2);
    native.event(); await vi.advanceTimersByTimeAsync(150);
    expect(native.invoke).toHaveBeenCalledTimes(3);
    expect(receive.mock.calls.map(call => call[0].cacheUsedBytes)).toEqual([1, 2, 3]);
    stop(); native.event(); await vi.advanceTimersByTimeAsync(300);
    expect(native.invoke).toHaveBeenCalledTimes(3);
    expect(native.unlisten).toHaveBeenCalledOnce();
  });
  it('卸载后迟到状态不更新界面，挂起定时器也清除', async () => {
    let resolve!: (value: AppSnapshot) => void;
    native.invoke.mockReturnValueOnce(new Promise<AppSnapshot>(yes => { resolve = yes; }));
    const { api } = await import('../src/api'); const receive = vi.fn();
    const stop = await api.subscribe(receive);
    native.event(); await vi.advanceTimersByTimeAsync(150);
    stop(); resolve(state(1)); await vi.advanceTimersByTimeAsync(500);
    expect(receive).not.toHaveBeenCalled();
    expect(native.unlisten).toHaveBeenCalledOnce();
  });
  it('快照失败明确提示，后续事件仍可恢复读取', async () => {
    native.invoke.mockRejectedValueOnce('synthetic state failure').mockResolvedValueOnce(state(2));
    const { api } = await import('../src/api'); const receive = vi.fn(); const onError = vi.fn();
    const stop = await api.subscribe(receive, onError);
    native.event(); await vi.advanceTimersByTimeAsync(150);
    expect(onError).toHaveBeenCalledExactlyOnceWith('当前状态读取未成功，请刷新后核对任务；停止入口仍可使用。');
    native.event(); await vi.advanceTimersByTimeAsync(150);
    expect(receive).toHaveBeenCalledExactlyOnceWith(state(2)); stop();
  });
});

it('a native snapshot completing after daemon selection is ignored before the page reloads', async () => {
 let complete!: (value: AppSnapshot) => void;
 native.invoke.mockReturnValueOnce(new Promise<AppSnapshot>(done => { complete = done; }));
 const { api } = await import('../src/api'); const receive = vi.fn(); const onError = vi.fn();
 const stop = await api.subscribe(receive, onError);
 await vi.advanceTimersByTimeAsync(150);
 api.setDaemonConnection('http://127.0.0.1:39111', 'synthetic-key');
 complete(state(1)); await vi.advanceTimersByTimeAsync(300);
 native.event({ payload: { sessionId: 'fixture', revision: '2' } });
 await vi.advanceTimersByTimeAsync(300);
 expect(receive).not.toHaveBeenCalled(); expect(onError).not.toHaveBeenCalled();
 expect(native.invoke).toHaveBeenCalledTimes(1); stop();
});
it('a native read failure after daemon selection cannot display a stale connection error', async () => {
 let fail!: (reason: unknown) => void;
 native.invoke.mockReturnValueOnce(new Promise((_yes, no) => { fail = no; }));
 const { api } = await import('../src/api'); const receive = vi.fn(); const onError = vi.fn();
 const stop = await api.subscribe(receive, onError); await vi.advanceTimersByTimeAsync(150);
 api.setDaemonConnection('http://127.0.0.1:39111', 'synthetic-key');
 fail('old native failure'); await vi.advanceTimersByTimeAsync(300);
 expect(receive).not.toHaveBeenCalled(); expect(onError).not.toHaveBeenCalled(); stop();
});
