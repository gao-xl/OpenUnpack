import { describe, expect, it, vi } from 'vitest';
const invoke = vi.hoisted(() => vi.fn());
vi.mock('@tauri-apps/api/core', () => ({ invoke }));
import { api, isDesktop } from '../src/api';

describe('浏览器预览与本地能力分离', () => {
  it('预览不生成任务、口令、引擎状态或缓存性能数据', async () => {
    expect(isDesktop).toBe(false);
    const snapshot = await api.snapshot();
    expect(snapshot.jobs).toEqual([]);
    expect(snapshot.passwords).toEqual([]);
    expect(snapshot.engineAvailable).toBe(false);
    expect(snapshot.cacheUsedBytes).toBe(0);
    expect(snapshot.dataDirectory).toBe('桌面版启动后显示');
    expect(invoke).not.toHaveBeenCalled();
  });
  it('浏览器中的扫描、复制和合并会明确拒绝，完全不调用原生IPC', async () => {
    await expect(api.scan(['C:\\fixtures\\a.zip'])).rejects.toThrow('界面预览');
    await expect(api.copyPassword('fixture-pw')).rejects.toThrow('界面预览');
    await expect(api.confirmRestoreBackup('C:\\fixtures\\backup', 'fixture passphrase', 'fixture-token')).rejects.toThrow('界面预览');
    expect(invoke).not.toHaveBeenCalled();
  });
});
