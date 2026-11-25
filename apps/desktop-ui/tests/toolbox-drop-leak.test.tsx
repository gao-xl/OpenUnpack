import { render, waitFor, act } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { App as AntApp, ConfigProvider } from 'antd';
import { beforeEach, expect, it, vi } from 'vitest';
import ToolboxPage from '../src/components/ToolboxPage';
import type { AppSnapshot } from '../src/types';
import { defaultSettings } from '../src/types';

let unlistenMock = vi.fn();
let dropCallback: ((paths: string[]) => void) | null = null;

const bridge = vi.hoisted(() => ({
  pickFiles: vi.fn(),
  pickDirectory: vi.fn(),
  previewToolbox: vi.fn(),
  executeToolbox: vi.fn(),
  fixArchiveExtensions: vi.fn(),
  openPath: vi.fn(),
  onDrop: vi.fn(),
}));

vi.mock('../src/api', () => ({
  api: bridge,
  isDesktop: true,
}));

const snapshot: AppSnapshot = {
  jobs: [],
  passwords: [],
  settings: { ...defaultSettings },
  running: false,
  paused: false,
  vaultError: null,
  dataDirectory: '',
  engineAvailable: true,
  recoverableCount: 0,
  cacheUsedBytes: 0,
  sessionId: 'fixture',
  revision: '1',
};

const emptyRun = async (work: () => Promise<unknown>) => {
  await work();
};

beforeEach(() => {
  vi.resetAllMocks();
  unlistenMock = vi.fn();
  dropCallback = null;
  bridge.onDrop.mockImplementation((cb: (paths: string[]) => void) => {
    dropCallback = cb;
    return Promise.resolve(unlistenMock);
  });
});

const renderPage = (initialTab: 'autoFix' | 'normalizeExtensions' = 'autoFix') =>
  render(
    <ConfigProvider button={{ autoInsertSpace: false }} theme={{ token: { motion: false } }}>
      <AntApp>
        <ToolboxPage
          snapshot={snapshot}
          refresh={async () => {}}
          run={emptyRun}
          initialTab={initialTab}
        />
      </AntApp>
    </ConfigProvider>,
  );

it('ToolboxPage 卸载时彻底注销拖拽监听器，杜绝事件与内存泄漏', async () => {
  const { unmount } = renderPage('autoFix');

  await waitFor(() => {
    expect(bridge.onDrop).toHaveBeenCalledTimes(1);
  });

  unmount();

  // 验证卸载后必须调用 unlisten 清理
  expect(unlistenMock).toHaveBeenCalledTimes(1);
});

it('ToolboxPage 在 onDrop 尚未完成注册前卸载，待解析到达后立即执行 cleanup 释放', async () => {
  let pendingResolve!: (cleanup: () => void) => void;
  bridge.onDrop.mockReturnValue(
    new Promise<() => void>((resolve) => {
      pendingResolve = resolve;
    }),
  );

  const { unmount } = renderPage('autoFix');
  expect(bridge.onDrop).toHaveBeenCalledTimes(1);

  // 组件在异步注册就绪前即卸载
  unmount();
  expect(unlistenMock).not.toHaveBeenCalled();

  // 稍后异步 IPC 注册完成返回 unlisten
  await act(async () => {
    pendingResolve(unlistenMock);
  });

  // 必须由于 disposed 立即被调用释放，防止在 Tauri 后台常驻泄漏
  expect(unlistenMock).toHaveBeenCalledTimes(1);
});

it('卸载后即便有排队的拖拽事件到达，也不会引起无效状态更新', async () => {
  const { unmount } = renderPage('autoFix');

  await waitFor(() => {
    expect(dropCallback).not.toBeNull();
  });

  unmount();

  // 卸载后触发 drop 回调，不应抛出异常或泄露
  expect(() => {
    dropCallback?.(['C:\\test\\file.zip']);
  }).not.toThrow();
});
it('在工具箱各 Tab 间切换时，不反复注销与重复注册，保持单一安全监听', async () => {
  const user = userEvent.setup();
  const { screen } = await import('@testing-library/react');
  renderPage('normalizeExtensions');

  await waitFor(() => {
    expect(bridge.onDrop).toHaveBeenCalledTimes(1);
  });

  const autoFixTab = screen.getByText(/压缩包后缀自动修复/);
  await user.click(autoFixTab);

  expect(bridge.onDrop).toHaveBeenCalledTimes(1);
  expect(unlistenMock).not.toHaveBeenCalled();
});
