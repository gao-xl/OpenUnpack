import { beforeEach, describe, expect, it, vi } from 'vitest';
import { render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { App as AntApp, ConfigProvider } from 'antd';
import zhCN from 'antd/locale/zh_CN';
import BulkPasswordBar from '../src/components/BulkPasswordBar';
import SettingsPage from '../src/components/SettingsPage';
import VaultPage from '../src/components/VaultPage';
import QueuePage from '../src/components/QueuePage';
import { JobDetails } from '../src/components/shared';
import RulePreview from '../src/components/RulePreview';
import SourceCleanupSelect from '../src/components/SourceCleanupSelect';
import InterfaceBoundary from '../src/components/InterfaceBoundary';
import HistoryPage from '../src/components/HistoryPage';
import { defaultSettings } from '../src/types';
import type { AppSnapshot, JobView, ImportPreview, ScanResult } from '../src/types';
import { historyPageResult } from './history-fixture';

const bridge = vi.hoisted(() => ({
  snapshot: vi.fn(), scan: vi.fn(), cancelScan: vi.fn(), pickFiles: vi.fn(), pickDirectory: vi.fn(), onDrop: vi.fn(),
  addJobs: vi.fn(), start: vi.fn(), pause: vi.fn(), cancel: vi.fn(), retry: vi.fn(), retryNode: vi.fn(), remove: vi.fn(), clearCompleted: vi.fn(),
  copyPassword: vi.fn(), revealPassword: vi.fn(), savePassword: vi.fn(), bulkPasswordAction: vi.fn(), deletePassword: vi.fn(),
  pickOpenBackup: vi.fn(), previewRestoreBackup: vi.fn(), confirmRestoreBackup: vi.fn(), pickSaveBackup: vi.fn(), exportBackup: vi.fn(), saveSettings: vi.fn(),
  previewExclusions: vi.fn(), cancelPreviewExclusions: vi.fn(), openPath: vi.fn(), recover: vi.fn(), historyPage: vi.fn(), jobDetails: vi.fn(), clearHistory: vi.fn(),
  getShellContextMenuStatus: vi.fn().mockResolvedValue(false), setShellContextMenuStatus: vi.fn().mockResolvedValue(true), executePostAction: vi.fn().mockResolvedValue(undefined),
}));
vi.mock('../src/api', () => ({ isDesktop: true, api: bridge }));

const snapshot: AppSnapshot = { jobs: [], passwords: [{ id: 'fixture-pw', name: '测试条目', groups: ['测试组'], notes: '', enabled: true, revision: 1, successCount: 0, lastSuccess: null }], settings: { ...defaultSettings }, running: false, paused: false, vaultError: null, dataDirectory: 'C:\\fixtures\\isolated', engineAvailable: true, recoverableCount: 0, cacheUsedBytes: 0, sessionId: 'fixture', revision: '1' };
const run = async (work: () => Promise<unknown>) => { await work(); };
const refresh = async () => {};
const renderPage = (element: React.ReactNode) => render(<ConfigProvider locale={zhCN} button={{ autoInsertSpace: false }} theme={{ token: { motion: false } }}><AntApp>{element}</AntApp></ConfigProvider>);
const preview: ImportPreview = { token: 'fixture-preview-token', newCount: 1, duplicateCount: 0, errorCount: 0, rows: [{ line: 1, name: '备份测试名称', group: '测试组', status: 'new', message: '' }] };
function deferred<T>() { let resolve!: (value: T) => void; let reject!: (reason: unknown) => void; const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; }); return { promise, resolve, reject }; }

beforeEach(() => {
  vi.resetAllMocks();
  bridge.onDrop.mockResolvedValue(() => {});
  bridge.pickFiles.mockResolvedValue(['C:\\fixtures\\a.zip']);
  bridge.pickOpenBackup.mockResolvedValue('C:\\fixtures\\backup.ouvault');
  bridge.previewRestoreBackup.mockResolvedValue(preview);
  bridge.confirmRestoreBackup.mockResolvedValue({ added: 1, duplicates: 0 });
  bridge.cancelPreviewExclusions.mockResolvedValue(undefined);
  bridge.getShellContextMenuStatus.mockResolvedValue(false);
  bridge.setShellContextMenuStatus.mockResolvedValue(true);
  bridge.executePostAction.mockResolvedValue(undefined);
});

describe('暂停与重试入口回归', () => {
  const job: JobView = { id: 'queue-fixture', source: { path: 'C:\\fixtures\\plain.zip', displayName: 'plain.zip', volumes: [], size: 1, missingVolume: false }, options: { ...defaultSettings }, status: 'queued', stage: 'queued', progress: 0, message: '', attempts: 0, outputPath: null, verificationScope: '', createdAt: '', finishedAt: null, workspace: null, stageTimes: {}, transferProgress: null, nodes: [] };
  it('暂停中的活动队列允许继续，继续后不重复启动', async () => {
    const user = userEvent.setup();
    const paused = { ...snapshot, running: true, paused: true, jobs: [job] };
    const view = renderPage(<QueuePage snapshot={paused} run={run} refresh={refresh} />);
    const resume = screen.getAllByRole('button', { name: /继续队列/ })[0] as HTMLButtonElement;
    expect(resume.disabled).toBe(false);
    await user.click(resume);
    expect(bridge.start).toHaveBeenCalledTimes(1);
    view.rerender(<ConfigProvider locale={zhCN}><AntApp><QueuePage snapshot={{ ...paused, paused: false }} run={run} refresh={refresh} /></AntApp></ConfigProvider>);
    expect((screen.getByRole('button', { name: /开始队列/ }) as HTMLButtonElement).disabled).toBe(true);
  });
  it('外包失败显示任务重试，节点重试仅用于失败或需密码的内包', () => {
    const failed = { ...job, status: 'needsPassword', nodes: [{ id: 'outer', parentId: null, name: 'plain.zip', depth: 0, status: 'needsPassword', stage: 'probe', progress: 0, attempts: 1, message: '', passwordLabel: null }, { id: 'inner', parentId: 'outer', name: 'inner.zip', depth: 1, status: 'interrupted', stage: 'probe', progress: 0, attempts: 1, message: '', passwordLabel: null }] };
    renderPage(<JobDetails job={failed} close={() => {}} run={run} />);
    expect(screen.getByRole('button', { name: '按当前设置重试任务' })).toBeTruthy();
    expect(screen.queryByRole('button', { name: '重试此节点' })).toBeNull();
  });
});

describe('显式密码操作', () => {
  it('查看密码失败后明确显示失败并保留关闭入口', async () => {
    const user = userEvent.setup();
    bridge.revealPassword.mockRejectedValueOnce(new Error('synthetic read failure'));
    const failedRun = async (work: () => Promise<unknown>) => { try { await work(); } catch { /* App.run shows the global error. */ } };
    renderPage(<VaultPage snapshot={snapshot} refresh={refresh} run={failedRun} />);
    await user.click(screen.getByRole('button', { name: '查看密码' }));
    expect(await screen.findByText('密码读取失败，请关闭后重试。')).toBeTruthy();
    expect(screen.queryByText('正在读取安全密码...')).toBeNull();
    await user.click(screen.getByText('关闭', { selector: 'span' }));
    await waitFor(() => expect(screen.queryByText('密码读取失败，请关闭后重试。')).toBeNull());
  });
  it('预览或没有选择时不能调用批量操作', () => {
    const onAction = vi.fn();
    renderPage(<BulkPasswordBar selectedCount={2} disabled busy={false} groups={[]} onAction={onAction} onClear={() => {}} />);
    expect((screen.getByRole('button', { name: '启用所选密码' }) as HTMLButtonElement).disabled).toBe(true);
    expect((screen.getByRole('button', { name: '删除所选密码' }) as HTMLButtonElement).disabled).toBe(true);
    expect(onAction).not.toHaveBeenCalled();
  });
  it('批量删除必须确认，取消不删除', async () => {
    const user = userEvent.setup(); const onAction = vi.fn();
    renderPage(<BulkPasswordBar selectedCount={2} disabled={false} busy={false} groups={[]} onAction={onAction} onClear={() => {}} />);
    await user.click(screen.getByRole('button', { name: '删除所选密码' }));
    expect(onAction).not.toHaveBeenCalled();
    await user.click(await screen.findByRole('button', { name: '取消' }));
    expect(onAction).not.toHaveBeenCalled();
    await user.click(screen.getByRole('button', { name: '删除所选密码' }));
    await user.click(await screen.findByRole('button', { name: '删除' }));
    expect(onAction).toHaveBeenCalledExactlyOnceWith('delete');
  });
  it('清空分组明确确认后才提交，密码复制走原生端口且不读取明文密码', async () => {
    const user = userEvent.setup(); const onAction = vi.fn();
    const first = renderPage(<BulkPasswordBar selectedCount={1} disabled={false} busy={false} groups={[]} onAction={onAction} onClear={() => {}} />);
    await user.click(screen.getByRole('button', { name: '清空所选密码的分组' }));
    expect(onAction).not.toHaveBeenCalled();
    await user.click(screen.getByRole('button', { name: '清空分组' }));
    expect(onAction).toHaveBeenCalledExactlyOnceWith('setGroups', []);
    first.unmount();
    renderPage(<VaultPage snapshot={snapshot} refresh={refresh} run={run} />);
    await user.click(screen.getByRole('button', { name: '复制 测试条目 密码' }));
    await waitFor(() => expect(bridge.copyPassword).toHaveBeenCalledExactlyOnceWith('fixture-pw'));
    expect(bridge.revealPassword).not.toHaveBeenCalled();
    expect(await screen.findByText(/密码已复制，剪贴板将在 30 秒后自动清除/)).toBeTruthy();
  });
});

describe('备份恢复必须经过预览', () => {
  it('首次只解密预览，再明确确认才合并；预览不显示密码', async () => {
    const user = userEvent.setup();
    renderPage(<SettingsPage snapshot={snapshot} refresh={refresh} run={run} />);
    await user.click(screen.getByRole('tab', { name: /界面与备份/ }));
    await user.click(screen.getByRole('button', { name: '恢复备份并合并' }));
    await user.type(await screen.findByLabelText('备份密码'), 'fixture-backup-pass');
    await user.click(screen.getByRole('button', { name: '解密并预览' }));
    expect(await screen.findByText('备份测试名称')).toBeTruthy();
    expect(bridge.confirmRestoreBackup).not.toHaveBeenCalled();
    await user.click(screen.getByRole('button', { name: '确认合并' }));
    await waitFor(() => expect(bridge.confirmRestoreBackup).toHaveBeenCalledExactlyOnceWith('C:\\fixtures\\backup.ouvault', 'fixture-backup-pass', preview.token));
  });
  it('密码改变时废弃旧预览，关闭后迟到的预览不会重新打开', async () => {
    const user = userEvent.setup();
    renderPage(<SettingsPage snapshot={snapshot} refresh={refresh} run={run} />);
    await user.click(screen.getByRole('tab', { name: /界面与备份/ }));
    await user.click(screen.getByRole('button', { name: '恢复备份并合并' }));
    await user.type(await screen.findByLabelText('备份密码'), 'fixture-backup-pass');
    await user.click(screen.getByRole('button', { name: '解密并预览' }));
    await screen.findByText('备份测试名称');
    await user.clear(screen.getByLabelText('备份密码'));
    await user.type(screen.getByLabelText('备份密码'), 'changed-backup-pass');
    expect(screen.queryByRole('button', { name: '确认合并' })).toBeNull();
    const pending = deferred<ImportPreview>(); bridge.previewRestoreBackup.mockReturnValueOnce(pending.promise);
    await user.click(screen.getByRole('button', { name: '解密并预览' }));
    await user.click(screen.getByRole('button', { name: '取消' }));
    pending.resolve(preview);
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull());
    expect(bridge.confirmRestoreBackup).not.toHaveBeenCalled();
  });
});

describe('队列停止与节点操作', () => {
  it('正在扫描时可独立取消，迟到结果不添加任务', async () => {
    const user = userEvent.setup(); const pending = deferred<{ sources: []; notices: [] }>();
    bridge.scan.mockReturnValueOnce(pending.promise);
    renderPage(<QueuePage snapshot={snapshot} refresh={refresh} run={run} />);
    await user.click(screen.getByRole('button', { name: /添加文件$/ }));
    await waitFor(() => expect(bridge.scan).toHaveBeenCalledOnce());
    const requestId = bridge.scan.mock.calls[0][1];
    await user.click(screen.getByRole('button', { name: '取消扫描' }));
    expect(bridge.cancelScan).toHaveBeenCalledExactlyOnceWith(requestId);
    pending.resolve({ sources: [], notices: [] });
    await waitFor(() => expect(screen.queryByText('正在扫描来源')).toBeNull());
    expect(screen.queryByText('检查扫描结果')).toBeNull();
    expect(bridge.addJobs).not.toHaveBeenCalled();
  });
  it('允许重试失败内包；部分交付只有用户确认后提交', async () => {
    const user = userEvent.setup();
    const job: JobView = { id: 'fixture-job', source: { path: 'C:\\fixtures\\outer.zip', displayName: 'outer.zip', volumes: [], size: 1, missingVolume: false }, options: { ...defaultSettings }, status: 'needsPassword', stage: 'nested', progress: 0.5, message: '内包需要密码', attempts: 1, outputPath: null, verificationScope: '', createdAt: '', finishedAt: null, workspace: 'C:\\fixtures\\work', stageTimes: {}, transferProgress: null, nodes: [{ id: 'outer', parentId: null, name: 'outer.zip', depth: 0, status: 'completed', stage: 'finished', progress: 1, attempts: 1, message: '', passwordLabel: null }, { id: 'inner', parentId: 'outer', name: 'inner.7z', depth: 1, status: 'needsPassword', stage: 'nested', progress: 0, attempts: 1, message: '', passwordLabel: null }] };
    renderPage(<JobDetails job={job} close={() => {}} run={run} />);
    await user.click(screen.getByRole('button', { name: '重试此节点' }));
    expect(bridge.retryNode).toHaveBeenCalledExactlyOnceWith(job.id, 'inner');
    await user.click(screen.getByRole('button', { name: '交付部分结果' }));
    expect(bridge.retry).not.toHaveBeenCalled();
    const explanation = await screen.findByText(/将交付已完成内容并保留未展开的内包/);
    const confirmation = explanation.closest('[role="dialog"]') as HTMLElement;
    expect(confirmation).not.toBeNull();
    await user.click(within(confirmation).getByRole('button', { name: '交付部分结果' }));
    expect(bridge.retry).toHaveBeenCalledExactlyOnceWith(job.id, true);
  });
});

describe('批量解压 ZIP 与 7z 以及实时进度与百分比展示', () => {
  it('支持批量扫描与添加 ZIP/7z 归档任务，并调用 addJobs 与 start 启动队列', async () => {
    const user = userEvent.setup();
    const scanResult: ScanResult = {
      sources: [
        { path: 'C:\\fixtures\\archive.zip', displayName: 'archive.zip', volumes: ['C:\\fixtures\\archive.zip'], size: 1024 * 1024, missingVolume: false },
        { path: 'C:\\fixtures\\package.7z', displayName: 'package.7z', volumes: ['C:\\fixtures\\package.7z'], size: 2 * 1024 * 1024, missingVolume: false },
        { path: 'C:\\fixtures\\broken.7z.001', displayName: 'broken.7z.001', volumes: ['C:\\fixtures\\broken.7z.001'], size: 512, missingVolume: true },
      ],
      notices: [],
    };
    bridge.pickFiles.mockResolvedValueOnce(['C:\\fixtures\\archive.zip', 'C:\\fixtures\\package.7z', 'C:\\fixtures\\broken.7z.001']);
    bridge.scan.mockResolvedValueOnce(scanResult);
    bridge.addJobs.mockResolvedValueOnce([]);

    renderPage(<QueuePage snapshot={snapshot} refresh={refresh} run={run} />);
    await user.click(screen.getByRole('button', { name: /添加文件$/ }));
    await waitFor(() => expect(bridge.scan).toHaveBeenCalledOnce());

    expect(await screen.findByText('检查扫描结果')).toBeTruthy();
    expect(screen.getByText('archive.zip')).toBeTruthy();
    expect(screen.getByText('package.7z')).toBeTruthy();
    expect(screen.getByText('broken.7z.001')).toBeTruthy();
    expect(screen.getByText('缺少分卷')).toBeTruthy();

    const addButton = screen.getByRole('button', { name: '添加 2 项' });
    expect(addButton).toBeTruthy();
    await user.click(addButton);

    await waitFor(() => expect(bridge.addJobs).toHaveBeenCalledOnce());
    const addedSources = bridge.addJobs.mock.calls[0][0];
    expect(addedSources).toHaveLength(2);
    expect(addedSources.map((s: { displayName: string }) => s.displayName)).toEqual(['archive.zip', 'package.7z']);
  });

  it('展示批量解压任务列表、实时进度条及百分比计算，并支持启动开始队列', async () => {
    const user = userEvent.setup();
    const activeSnapshot: AppSnapshot = {
      ...snapshot,
      engineAvailable: true,
      running: false,
      jobs: [
        {
          id: 'job-zip',
          source: { path: 'C:\\fixtures\\archive.zip', displayName: 'archive.zip', volumes: ['C:\\fixtures\\archive.zip'], size: 1024 * 1024, missingVolume: false },
          options: { ...defaultSettings },
          status: 'running',
          stage: 'extracting',
          progress: 0.654, // 应渲染为 65%
          message: '',
          attempts: 1,
          outputPath: null,
          verificationScope: 'standard',
          createdAt: '2026-10-10 10:00:00',
          finishedAt: null,
          workspace: 'C:\\fixtures\\work1',
          nodes: [
            { id: 'node-zip-1', parentId: null, name: 'archive.zip', depth: 0, status: 'running', stage: 'extracting', progress: 0.654, attempts: 1, message: '', passwordLabel: null },
          ],
          stageTimes: {}, transferProgress: null,
        },
        {
          id: 'job-7z',
          source: { path: 'C:\\fixtures\\package.7z', displayName: 'package.7z', volumes: ['C:\\fixtures\\package.7z'], size: 5 * 1024 * 1024, missingVolume: false },
          options: { ...defaultSettings },
          status: 'queued',
          stage: 'queued',
          progress: 0,
          message: '',
          attempts: 0,
          outputPath: null,
          verificationScope: 'standard',
          createdAt: '2026-10-10 10:01:00',
          finishedAt: null,
          workspace: null,
          nodes: [],
          stageTimes: {}, transferProgress: null,
        },
      ],
    };

    renderPage(<QueuePage snapshot={activeSnapshot} refresh={refresh} run={run} />);

    expect(screen.getByText('archive.zip')).toBeTruthy();
    expect(screen.getByText('package.7z')).toBeTruthy();

    expect(screen.getByText('外层解压')).toBeTruthy();
    expect(screen.getByText('等待')).toBeTruthy();
    const progressBars = screen.getAllByRole('progressbar');
    expect(progressBars.length).toBeGreaterThanOrEqual(2);
    expect(progressBars.some(bar => bar.getAttribute('aria-valuenow') === '65')).toBe(true);

    const startButton = screen.getByRole('button', { name: /开始队列/ });
    expect((startButton as HTMLButtonElement).disabled).toBe(false);
    await user.click(startButton);
    expect(bridge.start).toHaveBeenCalledOnce();
  });

  it('任务详情抽屉中准确显示内外层节点的实时进度与百分比计算', async () => {
    const jobWithNodes: JobView = {
      id: 'job-nested',
      source: { path: 'C:\\fixtures\\outer.zip', displayName: 'outer.zip', volumes: [], size: 10485760, missingVolume: false },
      options: { ...defaultSettings },
      status: 'running',
      stage: 'nested',
      progress: 0.728, // 73%
      message: '正在解压内部 7z 压缩包',
      attempts: 1,
      outputPath: null,
      verificationScope: 'standard',
      createdAt: '2026-10-10 10:00:00',
      finishedAt: null,
      workspace: 'C:\\fixtures\\work-nested',
      nodes: [
        { id: 'node-outer', parentId: null, name: 'outer.zip', depth: 0, status: 'completed', stage: 'finished', progress: 1.0, attempts: 1, message: '', passwordLabel: null },
        { id: 'node-inner', parentId: 'node-outer', name: 'inner.7z', depth: 1, status: 'running', stage: 'extracting', progress: 0.456, attempts: 1, message: '', passwordLabel: null },
      ],
      stageTimes: {}, transferProgress: null,
    };

    renderPage(<JobDetails job={jobWithNodes} close={() => {}} run={run} />);

    const progressBars = screen.getAllByRole('progressbar');
    expect(progressBars.some(bar => bar.getAttribute('aria-valuenow') === '73')).toBe(true);

    expect(screen.getAllByText(/outer\.zip/).length).toBeGreaterThanOrEqual(2);
    expect(screen.getByText('inner.7z')).toBeTruthy();
    expect(screen.getByText(/完成 · 100%/)).toBeTruthy();
    expect(screen.getByText(/外层解压 · 46%/)).toBeTruthy();
  });
});

describe('规则与高级动作', () => {
  it('规则改变后丢弃迟到试算结果，不冒充已读归档内容', async () => {
    const user = userEvent.setup(); const pending = deferred<{ path: string; matched: boolean }[]>();
    bridge.previewExclusions.mockReturnValueOnce(pending.promise);
    const page = renderPage(<RulePreview inputPatterns={['*.url']} outputPatterns={[]} run={run} />);
    await user.type(screen.getByLabelText('待试算的相对路径'), '资料/说明.url');
    await user.click(screen.getByRole('button', { name: '预览匹配' }));
    expect(bridge.previewExclusions).toHaveBeenCalledExactlyOnceWith(['资料/说明.url'], ['*.url'], 'input', expect.any(String));
    page.rerender(<ConfigProvider locale={zhCN} button={{ autoInsertSpace: false }} theme={{ token: { motion: false } }}><AntApp><RulePreview inputPatterns={['*.bak']} outputPatterns={[]} run={run} /></AntApp></ConfigProvider>);
    expect((screen.getByLabelText('待试算的相对路径') as HTMLTextAreaElement).value).toBe('资料/说明.url');
    pending.resolve([{ path: '资料/说明.url', matched: true }]);
    await waitFor(() => expect((screen.getByRole('button', { name: '预览匹配' }) as HTMLButtonElement).disabled).toBe(false));
    expect(screen.queryByText('命中：将排除')).toBeNull();
    expect(screen.getByText(/仅预览规则匹配效果/)).toBeTruthy();
  });
  it('输出试算传递明确目录条目和范围，并在切换范围后清空旧结果', async () => {
    const user = userEvent.setup();
    bridge.previewExclusions.mockResolvedValueOnce([{ path: 'temp/', matched: true }, { path: 'temp/file.bin', matched: true }]);
    renderPage(<RulePreview inputPatterns={['*.url']} outputPatterns={['temp']} run={run} />);
    await user.click(screen.getByRole('combobox', { name: '规则试算范围' }));
    await user.click(await screen.findByText('归档输出规则'));
    expect(screen.getByText(/目录请以/)).toBeTruthy();
    await user.type(screen.getByLabelText('待试算的相对路径'), 'temp/\ntemp/file.bin');
    await user.click(screen.getByRole('button', { name: '预览匹配' }));
    expect(bridge.previewExclusions).toHaveBeenCalledExactlyOnceWith(['temp/', 'temp/file.bin'], ['temp'], 'output', expect.any(String));
    await waitFor(() => expect(screen.getAllByText('命中：将排除')).toHaveLength(2));
    await user.click(screen.getByRole('combobox', { name: '规则试算范围' }));
    await user.click(await screen.findByText('扫描输入规则'));
    expect(screen.queryByText('命中：将排除')).toBeNull();
  });
  it('取消输出规则试算仅针对原请求，后续试算与迟到结果互不干扰', async () => {
    const user = userEvent.setup(); const pending = deferred<{ path: string; matched: boolean }[]>();
    bridge.previewExclusions.mockReturnValueOnce(pending.promise).mockResolvedValueOnce([{ path: 'new.bin', matched: false }]);
    renderPage(<RulePreview inputPatterns={[]} outputPatterns={['temp']} run={run} />);
    await user.click(screen.getByRole('combobox', { name: '规则试算范围' }));
    await user.click(await screen.findByText('归档输出规则'));
    await user.type(screen.getByLabelText('待试算的相对路径'), 'old.bin');
    await user.click(screen.getByRole('button', { name: '预览匹配' }));
    const oldId = bridge.previewExclusions.mock.calls[0][3];
    await user.click(screen.getByRole('button', { name: '停止试算' }));
    expect(bridge.cancelPreviewExclusions).toHaveBeenCalledExactlyOnceWith(oldId);
    await user.clear(screen.getByLabelText('待试算的相对路径'));
    await user.type(screen.getByLabelText('待试算的相对路径'), 'new.bin');
    await user.click(screen.getByRole('button', { name: '预览匹配' }));
    await screen.findByRole('cell', { name: 'new.bin' });
    const newId = bridge.previewExclusions.mock.calls[1][3];
    expect(oldId).not.toBe(newId);
    pending.resolve([{ path: 'old.bin', matched: true }]);
    await waitFor(() => expect(screen.queryByRole('cell', { name: 'old.bin' })).toBeNull());
    expect(bridge.cancelPreviewExclusions).not.toHaveBeenCalledWith(newId);
  });
  it('用户主动停止输出试算后，已取消请求不显示成新的操作失败', async () => {
    const user = userEvent.setup(); const pending = deferred<{ path: string; matched: boolean }[]>();
    const reported: unknown[] = [];
    const reportingRun = async (work: () => Promise<unknown>) => { try { await work(); } catch (error) { reported.push(error); } };
    bridge.previewExclusions.mockReturnValueOnce(pending.promise);
    bridge.cancelPreviewExclusions.mockImplementation(async () => { pending.reject(new Error('Cancelled')); });
    renderPage(<RulePreview inputPatterns={[]} outputPatterns={[]} run={reportingRun} />);
    await user.click(screen.getByRole('combobox', { name: '规则试算范围' }));
    await user.click(await screen.findByText('归档输出规则'));
    await user.type(screen.getByLabelText('待试算的相对路径'), 'example.bin');
    await user.click(screen.getByRole('button', { name: '预览匹配' }));
    await user.click(screen.getByRole('button', { name: '停止试算' }));
    await waitFor(() => expect((screen.getByRole('button', { name: '预览匹配' }) as HTMLButtonElement).disabled).toBe(false));
    expect(reported).toEqual([]);
  });
  it.each([
    ['ASCII 512', 'a'.repeat(512), true],
    ['中文 512字节', '中'.repeat(170) + 'aa', true],
    ['中文 513字节', '中'.repeat(171), false],
    ['Emoji 513字节', '😀'.repeat(128) + 'a', false],
  ] as const)('保存规则校验 UTF-8 长度：%s', async (_, pattern, accepted) => {
    const user = userEvent.setup();
    const settings = { ...defaultSettings, inputExclusions: [pattern], outputExclusions: [pattern] };
    renderPage(<SettingsPage snapshot={{ ...snapshot, settings }} refresh={refresh} run={run} />);
    await user.click(screen.getByRole('button', { name: /保存设置$/ }));
    if (accepted) {
      expect(bridge.saveSettings).toHaveBeenCalledExactlyOnceWith(settings);
    } else {
      expect(bridge.saveSettings).not.toHaveBeenCalled();
      expect(await screen.findByText('单条排除规则不可超过 512 UTF-8 字节')).toBeTruthy();
    }
  });
  it('源包直接删除策略必须明确确认后才改变', async () => {
    const user = userEvent.setup(); const onChange = vi.fn();
    renderPage(<SourceCleanupSelect value="keep" onChange={onChange} />);
    await user.click(screen.getByRole('combobox', { name: '完整交付后源压缩包处理' }));
    await user.click(await screen.findByText('完整交付后直接删除'));
    expect(onChange).not.toHaveBeenCalled();
    await user.click(await screen.findByRole('button', { name: '启用直接删除' }));
    expect(onChange).toHaveBeenCalledExactlyOnceWith('delete');
  });
  it('取消直接删除保留原策略，选择回收站无需永久删除确认', async () => {
    const user = userEvent.setup(); const onChange = vi.fn();
    renderPage(<SourceCleanupSelect value="keep" onChange={onChange} />);
    await user.click(screen.getByRole('combobox', { name: '完整交付后源压缩包处理' }));
    await user.click(await screen.findByText('完整交付后直接删除'));
    await user.click(await screen.findByRole('button', { name: '继续保留' }));
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull());
    expect(onChange).not.toHaveBeenCalled();
    await user.click(screen.getByRole('combobox', { name: '完整交付后源压缩包处理' }));
    await user.click(await screen.findByText('完整交付后移入回收站'));
    expect(onChange).toHaveBeenCalledExactlyOnceWith('recycle');
    expect(screen.queryByRole('dialog')).toBeNull();
  });
  it('设置说明区分外层源包和成功内包，保存所选清理策略', async () => {
    const user = userEvent.setup();
    renderPage(<SettingsPage snapshot={snapshot} refresh={refresh} run={run} />);
    await user.click(screen.getByRole('tab', { name: /完成后动作/ }));
    expect(screen.getByText(/原始外层压缩包及全部分卷会保留/)).toBeTruthy();
    expect(screen.getByText(/多层解压后，结果中不保留已成功展开的内包及分卷/)).toBeTruthy();
    await user.click(screen.getByRole('combobox', { name: '完整交付后源压缩包处理' }));
    await user.click(await screen.findByText('完整交付后移入回收站'));
    expect(screen.getByText(/任一层失败、取消、达到层数限制/)).toBeTruthy();
    await user.click(screen.getByRole('tab', { name: /处理策略/ }));
    await user.click(screen.getByRole('switch', { name: '自动展开内包' }));
    await user.click(screen.getByRole('tab', { name: /完成后动作/ }));
    expect(screen.getByText(/未启用多层解压，压缩包中的内包作为普通文件保留/)).toBeTruthy();
    await user.click(screen.getByRole('button', { name: /保存设置$/ }));
    expect(bridge.saveSettings).toHaveBeenCalledExactlyOnceWith({ ...defaultSettings, sourceCleanup: 'recycle', expandNested: false });
  });
  it('本批次确认直接删除后，将多层解压和清理策略一起提交给新任务', async () => {
    const user = userEvent.setup();
    const source = { path: 'C:\\fixtures\\outer.zip', displayName: 'outer.zip', volumes: ['C:\\fixtures\\outer.zip'], size: 1024, missingVolume: false };
    bridge.scan.mockResolvedValueOnce({ sources: [source], notices: [] });
    bridge.addJobs.mockResolvedValueOnce([]);
    renderPage(<QueuePage snapshot={snapshot} refresh={refresh} run={run} />);
    await user.click(screen.getByRole('button', { name: /添加文件$/ }));
    await user.click(await screen.findByRole('button', { name: '本批次选项' }));
    expect(screen.getByText(/原始外层压缩包及全部分卷会保留/)).toBeTruthy();
    await user.click(screen.getByRole('combobox', { name: '完整交付后源压缩包处理' }));
    await user.click(await screen.findByText('完整交付后直接删除'));
    await user.click(await screen.findByRole('button', { name: '启用直接删除' }));
    expect(screen.getByText(/完整解压、发布并校验通过后，将原始外层压缩包及全部分卷直接删除/)).toBeTruthy();
    expect(screen.getByText(/多层解压后，结果中不保留已成功展开的内包及分卷/)).toBeTruthy();
    expect(bridge.addJobs).not.toHaveBeenCalled();
    await user.click(screen.getByRole('button', { name: '添加 1 项' }));
    await waitFor(() => expect(bridge.addJobs).toHaveBeenCalledExactlyOnceWith([source], { ...defaultSettings, sourceCleanup: 'delete' }));
    expect(snapshot.settings.sourceCleanup).toBe('keep');
  });
  it('界面崩溃后仍保留独立停止入口，显示停止请求而非伪称已取消', async () => {
    const user = userEvent.setup();
    const quiet = vi.spyOn(console, 'error').mockImplementation(() => {});
    const Broken = () => { throw new Error('synthetic render fault'); };
    try {
      renderPage(<InterfaceBoundary><Broken /></InterfaceBoundary>);
      expect(screen.getByText('任务可能仍在后台运行')).toBeTruthy();
      expect(bridge.cancel).not.toHaveBeenCalled();
      await user.click(screen.getByRole('button', { name: '请求停止后台任务' }));
      expect(bridge.cancel).toHaveBeenCalledExactlyOnceWith();
      expect(await screen.findByText(/停止请求已发送/)).toBeTruthy();
    } finally { quiet.mockRestore(); }
  });

  it('HistoryPage 只读加载历史不触发外部 refresh，搜索支持过滤', async () => {
    const user = userEvent.setup();
    const mockRefresh = vi.fn();
    const historyJob: JobView = {
      id: 'hist-1',
      source: { path: 'C:\\fixtures\\done.zip', displayName: 'done.zip', volumes: [], size: 1024, missingVolume: false },
      options: { ...defaultSettings },
      status: 'completed',
      stage: 'finished',
      progress: 1,
      message: '解压成功',
      attempts: 1,
      outputPath: 'C:\\fixtures\\done',
      verificationScope: 'standard',
      createdAt: '2026-10-10 10:00:00',
      finishedAt: '2026-10-10 10:01:00',
      workspace: null,
      nodes: [],
      stageTimes: {}, transferProgress: null,
    };
    bridge.historyPage.mockImplementation(query => Promise.resolve(historyPageResult([historyJob], query)));
    renderPage(<HistoryPage snapshot={snapshot} refresh={mockRefresh} run={run} />);

    await waitFor(() => expect(bridge.historyPage).toHaveBeenCalled());
    expect(mockRefresh).not.toHaveBeenCalled();
    expect(await screen.findByText('done.zip')).toBeTruthy();

    const searchInput = screen.getByPlaceholderText('搜索文件路径或名称');
    await user.type(searchInput, 'non-existent');
    await waitFor(() => expect(screen.queryByText('done.zip')).toBeNull());

    await user.clear(searchInput);
    await waitFor(() => expect(screen.getByText('done.zip')).toBeTruthy());
  });

  it('设置页面保留默认单任务并支持保存五个活跃任务上限', async () => {
    const user = userEvent.setup();
    const mockRefresh = vi.fn();
    renderPage(<SettingsPage snapshot={snapshot} refresh={mockRefresh} run={run} />);

    await user.click(screen.getByRole('tab', { name: /性能与缓存/ }));
    expect(screen.getByText('并发解压任务数')).toBeTruthy();
    expect(screen.getByText(/同时最多 5 个任务/)).toBeTruthy();

    const input = screen.getByLabelText('并发解压任务数数值');
    expect(input).toBeTruthy();
    expect((input as HTMLInputElement).value).toBe('1');
    expect(input.getAttribute('aria-valuemax')).toBe('5');

    await user.clear(input);
    await user.type(input, '5');

    await user.click(screen.getByRole('button', { name: /保存设置/ }));
    await waitFor(() => {
      expect(bridge.saveSettings).toHaveBeenCalledWith(
        expect.objectContaining({ concurrency: 5 })
      );
    });
  });
  it('缓存关闭保留直写，启用后可明确选择总是中转并保存策略', async () => {
    const user = userEvent.setup();
    renderPage(<SettingsPage snapshot={snapshot} refresh={refresh} run={run} />);
    await user.click(screen.getByRole('tab', { name: /性能与缓存/ }));
    const selector = screen.getByRole('combobox', { name: '缓存选择策略' });
    expect(selector.getAttribute('disabled')).not.toBeNull();
    await user.click(screen.getByRole('switch', { name: '启用缓存工作目录' }));
    await user.click(selector);
    await user.click(await screen.findByText('总是使用中转缓存'));
    await user.click(screen.getByRole('button', { name: /保存设置/ }));
    await waitFor(() => expect(bridge.saveSettings).toHaveBeenCalledWith(expect.objectContaining({ cacheEnabled: true, cachePolicy: 'always' })));
  });

  it('设置页面展示并支持配置引擎线程数与优先级等新参数', async () => {
    const user = userEvent.setup();
    const mockRefresh = vi.fn();
    renderPage(<SettingsPage snapshot={snapshot} refresh={mockRefresh} run={run} />);

    await user.click(screen.getByRole('tab', { name: /性能与缓存/ }));
    expect(screen.getByText('引擎工作线程数')).toBeTruthy();
    expect(screen.getByText('工作进程优先级')).toBeTruthy();
    expect(screen.getByText(/0 表示按并发任务数分配核心/)).toBeTruthy();

    const threadInput = screen.getByLabelText('引擎工作线程数数值');
    expect(threadInput).toBeTruthy();
    expect((threadInput as HTMLInputElement).value).toBe('0');

    await user.clear(threadInput);
    await user.type(threadInput, '8');

    await user.click(screen.getByRole('button', { name: /保存设置/ }));
    await waitFor(() => {
      expect(bridge.saveSettings).toHaveBeenCalledWith(
        expect.objectContaining({
          engineThreads: 8,
          workerPriority: 'belowNormal',
          salvageCorrupted: true,
          autoFixEncoding: true,
          cleanEmptyDirs: true,
        })
      );
    });
  });
});
