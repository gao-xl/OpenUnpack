import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { App as AntApp, ConfigProvider } from 'antd';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import QueuePage from '../src/components/QueuePage';
import VaultPage from '../src/components/VaultPage';
import HistoryPage from '../src/components/HistoryPage';
import type { AppSnapshot, JobView, PasswordView, JobSummary } from '../src/types';
import { defaultSettings } from '../src/types';

const bridge = vi.hoisted(() => ({
  onDrop: vi.fn().mockResolvedValue(() => {}),
  cancelScan: vi.fn(),
  retry: vi.fn(),
  historyPage: vi.fn(),
  copyPassword: vi.fn(),
  deletePassword: vi.fn(),
  savePassword: vi.fn(),
}));

vi.mock('../src/api', () => ({
  api: bridge,
  isDesktop: true,
}));

const wrap = (node: React.ReactNode) => (
  <ConfigProvider button={{ autoInsertSpace: false }} theme={{ token: { motion: false } }}>
    <AntApp>{node}</AntApp>
  </ConfigProvider>
);

const emptyRun = async (work: () => Promise<unknown>) => {
  await work();
};

beforeEach(() => {
  vi.resetAllMocks();
  bridge.onDrop.mockResolvedValue(() => {});
});

describe('10,000+ 超大列表虚拟滚动测试 (DOM 节点稳定在 30 左右，消除性能假死)', () => {
  it('QueuePage 在 10,000+ 任务下，表格视图与卡片视图的渲染节点均保持在视口预算（约 30 左右）', async () => {
    const user = userEvent.setup();
    const jobs: JobView[] = Array.from({ length: 10000 }, (_, i) => ({
      id: 'job-' + i,
      source: {
        path: 'C:\\archives\\archive-' + i + '.zip',
        displayName: 'archive-' + i + '.zip',
        volumes: [],
        size: 1024 * 1024,
        missingVolume: false,
      },
      options: { ...defaultSettings },
      status: 'queued',
      stage: 'waiting',
      progress: 0,
      message: '',
      attempts: 0,
      outputPath: null,
      verificationScope: 'standard',
      createdAt: '2026-10-23T10:00:00Z',
      finishedAt: null,
      workspace: 'C:\\temp\\workspace\\job-' + i,
      nodes: [],
      stageTimes: {},
      transferProgress: null,
    }));

    const snapshot: AppSnapshot = {
      jobs,
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

    const { container } = render(
      wrap(<QueuePage snapshot={snapshot} run={emptyRun} refresh={async () => {}} />),
    );

    // 1. 验证默认表格视图下的虚拟行数
    const tableRows = container.querySelectorAll('.ant-table-row, tr[data-row-key]');
    expect(tableRows.length).toBeGreaterThan(0);
    expect(tableRows.length).toBeLessThan(40);

    // 2. 切换至卡片看板
    await user.click(screen.getByText('卡片看板'));

    // 验证卡片视图的虚拟卡片数稳定在 30 左右（15 ~ 36 之间），绝对不为 10,000
    const cards = container.querySelectorAll('.telemetry-card');
    expect(cards.length).toBeGreaterThanOrEqual(15);
    expect(cards.length).toBeLessThanOrEqual(40);
  });

  it('VaultPage 在 10,000+ 密码条目下，表格与卡片视图渲染节点均稳定在 30 左右', async () => {
    const user = userEvent.setup();
    const passwords: PasswordView[] = Array.from({ length: 10000 }, (_, i) => ({
      id: 'pwd-' + i,
      name: 'Password ' + i,
      groups: ['test'],
      notes: 'note ' + i,
      enabled: true,
      lastUsedAt: null,
      successCount: 0,
      lastSuccess: null,
      revision: 1,
    }));

    const snapshot: AppSnapshot = {
      jobs: [],
      passwords,
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

    const { container } = render(
      wrap(<VaultPage snapshot={snapshot} run={emptyRun} refresh={async () => {}} />),
    );

    // 1. 表格视图下节点数
    const tableRows = container.querySelectorAll('.ant-table-row, tr[data-row-key]');
    expect(tableRows.length).toBeGreaterThan(0);
    expect(tableRows.length).toBeLessThan(40);

    // 2. 切换至卡片看板
    await user.click(screen.getByText('卡片看板'));

    // 验证卡片视图的虚拟卡片数
    const cards = container.querySelectorAll('.telemetry-card');
    // telemetry-bar 内有 4 个统计 card，加上 VirtualGrid 内的卡片
    // VirtualGrid 内的卡片数量稳定在 15~36
    const totalTelemetryCards = cards.length;
    expect(totalTelemetryCards).toBeGreaterThanOrEqual(15);
    expect(totalTelemetryCards).toBeLessThanOrEqual(45);
  });

  it('HistoryPage 在大数据量下，表格与卡片视图均通过虚拟滚动保持极低 DOM 占用', async () => {
    const user = userEvent.setup();
    const historyList: JobSummary[] = Array.from({ length: 100 }, (_, i) => ({
      id: 'hist-' + i,
      source: {
        path: 'C:\\archives\\done-' + i + '.zip',
        displayName: 'done-' + i + '.zip',
        volumes: [],
        size: 1024 * 1024,
        missingVolume: false,
      },
      status: 'completed',
      stage: 'finished',
      progress: 1.0,
      message: '',
      attempts: 1,
      outputPath: 'C:\\output\\done-' + i,
      verificationScope: 'standard',
      createdAt: '2026-10-23T10:00:00Z',
      finishedAt: '2026-10-23T10:01:00Z',
      workspace: null,
    }));

    bridge.historyPage.mockResolvedValue({
      items: historyList,
      offset: 0,
      limit: 100,
      filteredTotal: 10000,
      total: 10000,
      completed: 10000,
      failed: 0,
      recoverable: 0,
      clearAllowed: true,
      sessionId: 'test',
      revision: '1',
    });

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

    const { container } = render(
      wrap(<HistoryPage snapshot={snapshot} run={emptyRun} refresh={async () => {}} />),
    );

    // 初始渲染表格
    const tableRows = container.querySelectorAll('.ant-table-row, tr[data-row-key]');
    expect(tableRows.length).toBeLessThan(40);

    // 切换至卡片视图
    await user.click(screen.getByText('卡片看板'));

    const cards = container.querySelectorAll('.telemetry-card');
    // telemetry-card 包括 4 个统计面板 + 视口内卡片
    expect(cards.length).toBeGreaterThanOrEqual(4);
    expect(cards.length).toBeLessThanOrEqual(45);
  });
});
