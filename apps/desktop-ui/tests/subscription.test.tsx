import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { AppSnapshot } from '../src/types';
const native = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn(), unlisten: vi.fn(), event: (() => {}) as () => void }));
vi.mock('@tauri-apps/api/core', () => ({ invoke: native.invoke }));
vi.mock('@tauri-apps/api/event', () => ({ listen: native.listen }));

beforeEach(() => {
  vi.resetModules(); vi.resetAllMocks(); vi.useFakeTimers();
  Object.defineProperty(window, '__TAURI_INTERNALS__', { value: {}, configurable: true });
  native.listen.mockImplementation(async (_event, callback) => { native.event = callback; return native.unlisten; });
});
afterEach(() => { vi.useRealTimers(); delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__; });
const state = (number: number) => ({ jobs: [], cacheUsedBytes: number }) as unknown as AppSnapshot;

describe('桌面实时状态接收', () => {
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
