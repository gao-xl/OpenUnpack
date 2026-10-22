import { beforeEach, describe, expect, it, vi } from 'vitest';
import { render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { App as AntApp, ConfigProvider } from 'antd';
import zhCN from 'antd/locale/zh_CN';
import QueuePage from '../src/components/QueuePage';
import SettingsPage from '../src/components/SettingsPage';
import ArchiveBrowser from '../src/components/ArchiveBrowser';
import { defaultSettings } from '../src/types';
import type { AppSnapshot, JobView, ScanResult } from '../src/types';

const bridge = vi.hoisted(() => ({
  snapshot: vi.fn(), scan: vi.fn(), cancelScan: vi.fn(), pickFiles: vi.fn(), pickDirectory: vi.fn(), onDrop: vi.fn(),
  addJobs: vi.fn(), start: vi.fn(), pause: vi.fn(), cancel: vi.fn(), retry: vi.fn(), retryNode: vi.fn(), remove: vi.fn(), clearCompleted: vi.fn(),
  copyPassword: vi.fn(), revealPassword: vi.fn(), savePassword: vi.fn(), bulkPasswordAction: vi.fn(), deletePassword: vi.fn(),
  pickOpenBackup: vi.fn(), previewRestoreBackup: vi.fn(), confirmRestoreBackup: vi.fn(), pickSaveBackup: vi.fn(), exportBackup: vi.fn(), saveSettings: vi.fn(),
  previewExclusions: vi.fn(), openPath: vi.fn(), recover: vi.fn(), history: vi.fn(), clearHistory: vi.fn(), inspectArchive: vi.fn(), cancelInspectArchive: vi.fn(),
}));
vi.mock('../src/api', () => ({ isDesktop: true, api: bridge }));

const snapshot: AppSnapshot = {
  jobs: [],
  passwords: [{ id: 'pw-1', name: '默认组密码', groups: ['测试组'], notes: '', enabled: true, revision: 1, successCount: 0, lastSuccess: null }],
  settings: { ...defaultSettings, autoStartOnAdd: true },
  running: false,
  paused: false,
  vaultError: null,
  dataDirectory: 'C:\\fixtures\\isolated',
  engineAvailable: true,
  recoverableCount: 0,
  cacheUsedBytes: 0,
  sessionId: 'ux-session',
  revision: '1',
};

const run = async (work: () => Promise<unknown>) => { await work(); };
const refresh = async () => {};
const renderPage = (element: React.ReactNode) => render(
  <ConfigProvider locale={zhCN} button={{ autoInsertSpace: false }} theme={{ token: { motion: false } }}>
    <AntApp>{element}</AntApp>
  </ConfigProvider>
);

beforeEach(() => {
  vi.resetAllMocks();
  bridge.onDrop.mockResolvedValue(() => {});
  bridge.cancelInspectArchive.mockResolvedValue(undefined);
});

describe('UX 重构 6 项核心痛点验收', () => {
  it('痛点 1 & 2: 扫描确认后自动开始队列联动，且消除了第三层模态框轰炸（选项内嵌展开）', async () => {
    const user = userEvent.setup();
    const scanResult: ScanResult = {
      sources: [
        { path: 'C:\\fixtures\\pkg.zip', displayName: 'pkg.zip', volumes: ['C:\\fixtures\\pkg.zip'], size: 1024, missingVolume: false },
      ],
      notices: [],
    };
    bridge.pickFiles.mockResolvedValueOnce(['C:\\fixtures\\pkg.zip']);
    bridge.scan.mockResolvedValueOnce(scanResult);
    bridge.addJobs.mockResolvedValueOnce([]);
    bridge.start.mockResolvedValueOnce(undefined);

    renderPage(<QueuePage snapshot={snapshot} refresh={refresh} run={run} />);
    await user.click(screen.getByRole('button', { name: /添加文件$/ }));
    await waitFor(() => expect(bridge.scan).toHaveBeenCalledOnce());

    // 检查扫描结果弹窗打开
    expect(await screen.findByText('检查扫描结果')).toBeTruthy();
    // 展开内嵌批次选项（非第三层 Modal 弹窗）
    const optionsButton = screen.getByRole('button', { name: '本批次选项' });
    await user.click(optionsButton);
    expect(await screen.findByText('本批次处理选项')).toBeTruthy();
    expect(screen.getByText('智能去套娃')).toBeTruthy();
    expect(screen.getByText(/开启后若压缩包内仅含单个顶层目录/)).toBeTruthy();

    // 点击添加项，应触发 addJobs，随后由于 autoStartOnAdd=true 自动调用 api.start
    await user.click(screen.getByRole('button', { name: '添加 1 项' }));
    await waitFor(() => expect(bridge.addJobs).toHaveBeenCalledOnce());
    expect(bridge.start).toHaveBeenCalledOnce();
  });

  it('痛点 2: 设置页支持配置 autoStartOnAdd', async () => {
    const user = userEvent.setup();
    renderPage(<SettingsPage snapshot={snapshot} refresh={refresh} run={run} />);
    await user.click(screen.getByRole('tab', { name: /处理策略/ }));
    expect(await screen.findByText('添加后自动解压')).toBeTruthy();
    expect(screen.getByText(/添加文件或确认批次后自动开始队列/)).toBeTruthy();
  });

  it('痛点 3: 状态为 needsPassword 的任务提供即时输入密码通道并保存到密码库重试', async () => {
    const user = userEvent.setup();
    const pwdJob: JobView = {
      id: 'job-pwd',
      source: { path: 'C:\\fixtures\\secret.zip', displayName: 'secret.zip', volumes: [], size: 1024, missingVolume: false },
      options: { ...defaultSettings },
      status: 'needsPassword',
      stage: 'probing',
      progress: 0,
      message: '需要解压密码',
      attempts: 1,
      outputPath: null,
      verificationScope: 'standard',
      createdAt: '2026-10-10 10:00:00',
      finishedAt: null,
      workspace: null,
      nodes: [],
      stageTimes: {},
      transferProgress: null,
    };
    bridge.savePassword.mockResolvedValueOnce({ id: 'saved-id', name: 'secret.zip 密码' });
    bridge.retry.mockResolvedValueOnce(undefined);

    renderPage(<QueuePage snapshot={{ ...snapshot, jobs: [pwdJob] }} refresh={refresh} run={run} />);
    const pwdBtn = screen.getByRole('button', { name: '输入密码' });
    expect(pwdBtn).toBeTruthy();
    await user.click(pwdBtn);

    expect(await screen.findByText(/输入解压密码 — secret\.zip/)).toBeTruthy();
    const input = screen.getByLabelText('即时解压密码');
    await user.type(input, '123456');

    const submitBtn = screen.getByRole('button', { name: '解密并重试' });
    await user.click(submitBtn);

    await waitFor(() => expect(bridge.savePassword).toHaveBeenCalledWith(expect.objectContaining({
      secret: '123456',
      notes: '解压即时录入',
    })));
    expect(bridge.retry).toHaveBeenCalledWith('job-pwd');
  });

  it('痛点 3 补全: ArchiveBrowser 密码浏览成功后支持应用密码并重试解压', async () => {
    const user = userEvent.setup();
    const pwdJob: JobView = {
      id: 'job-browse',
      source: { path: 'C:\\fixtures\\secret2.zip', displayName: 'secret2.zip', volumes: [], size: 1024, missingVolume: false },
      options: { ...defaultSettings },
      status: 'needsPassword',
      stage: 'probing',
      progress: 0,
      message: '需要解压密码',
      attempts: 1,
      outputPath: null,
      verificationScope: 'standard',
      createdAt: '2026-10-10 10:00:00',
      finishedAt: null,
      workspace: null,
      nodes: [],
      stageTimes: {},
      transferProgress: null,
    };
    const inspection = {
      archivePath: 'C:\\fixtures\\secret2.zip',
      totalEntries: 1,
      totalSize: 512,
      totalSizeKnown: true,
      encryptedEntries: 1,
      entries: [{ path: 'file.txt', size: 512, sizeKnown: true, isDirectory: false, isEncrypted: true, rawPath: '' }],
    };
    bridge.inspectArchive
      .mockRejectedValueOnce(new Error('NeedsPassword'))
      .mockResolvedValueOnce(inspection);
    bridge.savePassword.mockResolvedValueOnce({ id: 'saved-id-2' });
    bridge.retry.mockResolvedValueOnce(undefined);

    renderPage(<ArchiveBrowser job={pwdJob} close={() => {}} />);
    expect(await screen.findByText('此归档需要密码才能列出内容')).toBeTruthy();
    await user.type(screen.getByLabelText('归档密码'), 'password999');
    await user.click(screen.getByRole('button', { name: '使用该密码浏览' }));

    expect(await screen.findByText('file.txt')).toBeTruthy();
    const applyRetryBtn = screen.getByRole('button', { name: '应用此密码并重试解压' });
    expect(applyRetryBtn).toBeTruthy();
    await user.click(applyRetryBtn);

    await waitFor(() => expect(bridge.savePassword).toHaveBeenCalledWith(expect.objectContaining({
      secret: 'password999',
    })));
    expect(bridge.retry).toHaveBeenCalledWith('job-browse');
  });

  it('痛点 4: 处于 stopping 状态的任务可尝试安全删除，不承诺强行终止', async () => {
    const user = userEvent.setup();
    const stoppingJob: JobView = {
      id: 'job-stopping',
      source: { path: 'C:\\fixtures\\stuck.zip', displayName: 'stuck.zip', volumes: [], size: 1024, missingVolume: false },
      options: { ...defaultSettings },
      status: 'stopping',
      stage: 'extracting',
      progress: 0.5,
      message: '正在停止',
      attempts: 1,
      outputPath: null,
      verificationScope: 'standard',
      createdAt: '2026-10-10 10:00:00',
      finishedAt: null,
      workspace: null,
      nodes: [],
      stageTimes: {},
      transferProgress: null,
    };
    bridge.remove.mockResolvedValueOnce(undefined);

    renderPage(<QueuePage snapshot={{ ...snapshot, jobs: [stoppingJob] }} refresh={refresh} run={run} />);
    const deleteBtn = screen.getByRole('button', { name: '尝试删除' }) as HTMLButtonElement;
    expect(deleteBtn.title).toContain('退出尚未确认时保留任务和临时缓存');
    expect(screen.queryByRole('button', { name: '强行终止' })).toBeNull();
    await user.click(deleteBtn);

    expect(await screen.findByText(/确认后台进程停止后再删除任务记录/)).toBeTruthy();
    const confirmBtn = within(screen.getByRole('dialog')).getByRole('button', { name: '删除任务' });
    await user.click(confirmBtn);

    await waitFor(() => expect(bridge.remove).toHaveBeenCalledWith('job-stopping'));
  });

  it('痛点 5 & 6: 队列工具栏准确说明删除已完成任务与暂停横幅提示', () => {
    const pausedWithWaiting = {
      ...snapshot,
      paused: true,
      jobs: [
        {
          id: 'job-waiting',
          source: { path: 'C:\\fixtures\\waiting.zip', displayName: 'waiting.zip', volumes: [], size: 1024, missingVolume: false },
          options: { ...defaultSettings },
          status: 'queued' as const,
          stage: 'queued' as const,
          progress: 0,
          message: '',
          attempts: 0,
          outputPath: null,
          verificationScope: 'standard',
          createdAt: '2026-10-10 10:00:00',
          finishedAt: null,
          workspace: null,
          nodes: [],
          stageTimes: {},
          transferProgress: null,
        },
      ],
    };

    renderPage(<QueuePage snapshot={pausedWithWaiting} refresh={refresh} run={run} />);
    // 痛点 5: 删除任务不会声称仍保留历史记录。
    const clearBtn = screen.getByRole('button', { name: '删除当前显示的已结束任务' });
    expect(clearBtn).toBeTruthy();
    expect(clearBtn.title).toContain('删除已完成任务记录并清理临时缓存，原包与已发布结果保留');

    // 暂停横幅提示
    expect(screen.getByText('队列当前处于暂停状态')).toBeTruthy();
    expect(screen.getByText(/新添加的任务已进入等待队列，请点击“继续队列”开始解压/)).toBeTruthy();
  });
});
