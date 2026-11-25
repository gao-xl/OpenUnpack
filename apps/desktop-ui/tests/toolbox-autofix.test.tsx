import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { App as AntApp, ConfigProvider } from 'antd';
import { beforeEach, expect, it, vi } from 'vitest';
import ToolboxPage from '../src/components/ToolboxPage';
import type { AppSnapshot, ArchiveExtensionFixItem } from '../src/types';
import { defaultSettings } from '../src/types';

const bridge = vi.hoisted(() => ({
  pickFiles: vi.fn(),
  pickDirectory: vi.fn(),
  previewToolbox: vi.fn(),
  executeToolbox: vi.fn(),
  fixArchiveExtensions: vi.fn(),
  openPath: vi.fn(),
  onDrop: vi.fn().mockResolvedValue(() => {}),
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
  bridge.onDrop.mockResolvedValue(() => {});
});

const renderPage = () =>
  render(
    <ConfigProvider button={{ autoInsertSpace: false }} theme={{ token: { motion: false } }}>
      <AntApp>
        <ToolboxPage
          snapshot={snapshot}
          refresh={async () => {}}
          run={emptyRun}
          initialTab="autoFix"
        />
      </AntApp>
    </ConfigProvider>,
  );

it('压缩包后缀自动修复：能够选择文件并一键执行物理重命名修复', async () => {
  const user = userEvent.setup();
  const mockResults: ArchiveExtensionFixItem[] = [
    {
      originalPath: 'C:\\downloads\\movie.mp4',
      proposedName: 'movie.zip',
      newPath: 'C:\\downloads\\movie.zip',
      status: 'renamed',
      message: '已成功修改后缀为 movie.zip',
    },
    {
      originalPath: 'C:\\downloads\\data.7z',
      proposedName: 'data.7z',
      newPath: 'C:\\downloads\\data.7z',
      status: 'already_correct',
      message: '魔数检测有效（SevenZip），后缀已正确无需修改',
    },
  ];

  bridge.pickFiles.mockResolvedValue(['C:\\downloads\\movie.mp4', 'C:\\downloads\\data.7z']);
  bridge.fixArchiveExtensions.mockResolvedValue(mockResults);

  renderPage();

  // 点击选择文件
  const pickBtn = screen.getByRole('button', { name: /选择文件 \(支持多选\)/ });
  await user.click(pickBtn);

  // 验证待处理列表出现已选文件
  expect(await screen.findByText('movie.mp4')).toBeTruthy();
  expect(screen.getByText('data.7z')).toBeTruthy();

  // 点击开始自动修复后缀按钮
  const fixBtn = screen.getByRole('button', { name: /开始自动修复后缀 \(物理重命名\)/ });
  await user.click(fixBtn);

  // 校验 IPC 调用
  await waitFor(() => {
    expect(bridge.fixArchiveExtensions).toHaveBeenCalledWith([
      'C:\\downloads\\movie.mp4',
      'C:\\downloads\\data.7z',
    ]);
  });

  // 验证结果呈现
  expect(await screen.findByText('movie.zip')).toBeTruthy();
  expect(screen.getByText('已修改后缀')).toBeTruthy();
  expect(screen.getByText('无需修改')).toBeTruthy();

  // 点击打开目录
  const openButtons = screen.getAllByRole('button', { name: /打开目录/ });
  expect(openButtons.length).toBeGreaterThan(0);
  await user.click(openButtons[0]);
  expect(bridge.openPath).toHaveBeenCalledWith('C:\\downloads\\movie.zip');
});

it('压缩包后缀自动修复：清空按钮能正常清空待处理列表', async () => {
  const user = userEvent.setup();
  bridge.pickFiles.mockResolvedValue(['C:\\downloads\\corrupted.mp4']);
  renderPage();

  const pickBtn = screen.getByRole('button', { name: /选择文件 \(支持多选\)/ });
  await user.click(pickBtn);
  expect(await screen.findByText('corrupted.mp4')).toBeTruthy();

  const clearBtn = screen.getByRole('button', { name: /清空待处理列表/ });
  await user.click(clearBtn);

  expect(screen.queryByText('corrupted.mp4')).toBeNull();
});
