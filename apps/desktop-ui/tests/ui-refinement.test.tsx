import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, act } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { App as AntApp, ConfigProvider } from 'antd';
import zhCN from 'antd/locale/zh_CN';
import SettingsPage from '../src/components/SettingsPage';
import QueuePage from '../src/components/QueuePage';
import VaultPage from '../src/components/VaultPage';
import HistoryPage from '../src/components/HistoryPage';
import { SkinProvider, SKIN_PRESETS } from '../src/theme';
import { defaultSettings, type AppSnapshot, type JobView } from '../src/types';

const bridge = vi.hoisted(() => ({
  isDaemonMode: vi.fn(() => false),
  saveSettings: vi.fn(),
  pickDirectory: vi.fn(),
  pickFiles: vi.fn(),
  savePassword: vi.fn(),
  deletePassword: vi.fn(),
  historyPage: vi.fn(),
  retry: vi.fn(),
  clearHistory: vi.fn(),
  onDrop: vi.fn(() => Promise.resolve(() => {})),
}));

vi.mock('../src/api', () => ({
  isDesktop: true,
  api: bridge,
}));

const mockJob: JobView = {
  id: 'job-1',
  source: {
    path: 'C:\\data\\archive.zip',
    displayName: 'archive.zip',
    volumes: [],
    size: 10485760,
    missingVolume: false,
  },
  options: { ...defaultSettings },
  status: 'running',
  stage: 'extracting',
  progress: 0.65,
  message: '',
  attempts: 1,
  outputPath: 'C:\\output\\archive',
  verificationScope: 'standard',
  createdAt: '2026-11-02T08:00:00Z',
  finishedAt: null,
  workspace: null,
  nodes: [],
  stageTimes: {},
  transferProgress: {
    completedBytes: 6815744,
    totalBytes: 10485760,
    bytesPerSecond: 5242880,
    remainingSeconds: 1,
    updatedAt: '2026-11-02T08:00:00Z',
  },
};

const mockSnapshot: AppSnapshot = {
  jobs: [mockJob],
  passwords: [
    {
      id: 'pwd-1',
      name: 'Work Archive',
      groups: ['Work'],
      notes: 'Daily backup',
      enabled: true,
      successCount: 0,
      lastSuccess: null,
      revision: 1,
    },
    {
      id: 'pwd-2',
      name: 'Personal Files',
      groups: ['Personal'],
      notes: '',
      enabled: false,
      successCount: 0,
      lastSuccess: null,
      revision: 1,
    },
  ],
  settings: { ...defaultSettings },
  running: true,
  paused: false,
  vaultError: null,
  dataDirectory: 'C:\\data',
  engineAvailable: true,
  recoverableCount: 0,
  cacheUsedBytes: 52428800,
  sessionId: 'test-session',
  revision: '1',
};

const run = async (work: () => Promise<unknown>) => {
  await work();
};
const refresh = async () => {};

function wrap(ui: React.ReactNode) {
  return (
    <SkinProvider>
      <ConfigProvider
        locale={zhCN}
        button={{ autoInsertSpace: false }}
        theme={{ token: { motion: false } }}
      >
        <AntApp>{ui}</AntApp>
      </ConfigProvider>
    </SkinProvider>
  );
}

beforeEach(() => {
  vi.resetAllMocks();
  localStorage.clear();
});

describe('UI Refinement: SettingsPage Skin Selector', () => {
  it('renders skin selector and all four preset cards with preview metadata', async () => {
    const user = userEvent.setup();
    render(wrap(<SettingsPage snapshot={mockSnapshot} refresh={refresh} run={run} />));

    await user.click(screen.getByRole('tab', { name: /界面与备份/ }));

    expect(screen.getByRole('combobox', { name: '界面皮肤' })).toBeTruthy();
    expect(screen.getByText('皮肤预设与实时预览')).toBeTruthy();

    for (const preset of SKIN_PRESETS) {
      expect(screen.getAllByText(preset.name).length).toBeGreaterThan(0);
    }
  });

  it('triggers live preview event when a skin card is clicked', async () => {
    const user = userEvent.setup();
    const previewSpy = vi.fn();
    window.addEventListener('openunpack:skin-preview', previewSpy);

    render(wrap(<SettingsPage snapshot={mockSnapshot} refresh={refresh} run={run} />));
    await user.click(screen.getByRole('tab', { name: /界面与备份/ }));

    const cyberpunkCard = screen.getByRole('button', { name: /赛博霓虹/i });
    await user.click(cyberpunkCard);

    expect(previewSpy).toHaveBeenCalled();
    const lastCall = previewSpy.mock.calls[previewSpy.mock.calls.length - 1][0] as CustomEvent;
    expect(lastCall.detail?.skin).toBe('cyberpunk');

    window.removeEventListener('openunpack:skin-preview', previewSpy);
  });
});

describe('UI Refinement: QueuePage Polymorphic Layout', () => {
  it('toggles between Table view and Card view', async () => {
    const user = userEvent.setup();
    render(wrap(<QueuePage snapshot={mockSnapshot} refresh={refresh} run={run} />));

    expect(screen.getByText('表格视图')).toBeTruthy();
    expect(screen.getByText('卡片看板')).toBeTruthy();

    expect(screen.getByRole('table')).toBeTruthy();

    await user.click(screen.getByText('卡片看板'));

    expect(screen.queryByRole('table')).toBeNull();
    expect(screen.getAllByText('archive.zip').length).toBeGreaterThan(0);
    expect(screen.getByText('ZIP')).toBeTruthy();
  });
});

describe('UI Refinement: VaultPage Polymorphic Layout', () => {
  it('renders stats telemetry and supports Card view', async () => {
    const user = userEvent.setup();
    render(wrap(<VaultPage snapshot={mockSnapshot} refresh={refresh} run={run} />));

    expect(screen.getByText('密码总数')).toBeTruthy();
    expect(screen.getByText('启用状态')).toBeTruthy();

    expect(screen.getByRole('table')).toBeTruthy();

    await user.click(screen.getByText('卡片看板'));

    expect(screen.queryByRole('table')).toBeNull();
    expect(screen.getByText('Work Archive')).toBeTruthy();
    expect(screen.getByText('Personal Files')).toBeTruthy();
  });
});

describe('UI Refinement: HistoryPage Polymorphic Layout', () => {
  it('renders history telemetry bar and supports Card view', async () => {
    bridge.historyPage.mockResolvedValue({
      items: [
        {
          id: 'hist-1',
          source: {
            path: 'C:\\data\\done.zip',
            displayName: 'done.zip',
            size: 1024,
            volumes: [],
            missingVolume: false,
          },
          status: 'completed',
          outputPath: 'C:\\output\\done',
          verificationScope: 'standard',
          createdAt: '2026-11-02T08:00:00Z',
          finishedAt: '2026-11-02T08:01:00Z',
          message: '',
          attempts: 1,
        },
      ],
      offset: 0,
      limit: 10,
      filteredTotal: 1,
      total: 1,
      completed: 1,
      failed: 0,
      recoverable: 0,
      clearAllowed: true,
      sessionId: 'test',
      revision: '1',
    });

    const user = userEvent.setup();
    render(wrap(<HistoryPage snapshot={mockSnapshot} refresh={refresh} run={run} />));

    expect(screen.getByText('最近记录总数')).toBeTruthy();
    expect(screen.getByText('已完成')).toBeTruthy();

    await act(async () => {
      await new Promise((r) => setTimeout(r, 50));
    });

    await user.click(screen.getByText('卡片看板'));
    expect(screen.queryByRole('table')).toBeNull();
  });
});
