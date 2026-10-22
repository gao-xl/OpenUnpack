import { describe, expect, it, vi } from 'vitest';
const invoke = vi.hoisted(() => vi.fn());
vi.mock('@tauri-apps/api/core', () => ({ invoke }));
import { api, isDesktop } from '../src/api';
import type { ToolboxOptions } from '../src/types';

describe('本地目录整理工具箱 IPC 契约与安全隔离', () => {
  it('非桌面环境下预览与执行操作被严格拦截，杜绝越权调用', async () => {
    expect(isDesktop).toBe(false);
    const opts: ToolboxOptions = {
      kind: 'normalizeExtensions',
      sourceDirectory: 'C:\\Users\\Public\\Downloads',
      targetDirectory: null,
      recursive: true,
      removeEmptyFolders: false,
      conflictStrategy: 'rename',
      renameRule: null,
    };
    await expect(api.previewToolbox(opts)).rejects.toThrow('界面预览');
    await expect(
      api.executeToolbox({ options: opts, approvedPaths: ['C:\\file.tar.gz.tar'], token: null })
    ).rejects.toThrow('界面预览');
    expect(invoke).not.toHaveBeenCalled();
  });
});
