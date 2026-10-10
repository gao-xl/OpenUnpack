import { beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
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
import { defaultSettings } from '../src/types';
import type { AppSnapshot, JobView, ImportPreview } from '../src/types';

const bridge = vi.hoisted(() => ({
  snapshot: vi.fn(), scan: vi.fn(), cancelScan: vi.fn(), pickFiles: vi.fn(), pickDirectory: vi.fn(), onDrop: vi.fn(),
  addJobs: vi.fn(), start: vi.fn(), pause: vi.fn(), cancel: vi.fn(), retry: vi.fn(), retryNode: vi.fn(), remove: vi.fn(), clearCompleted: vi.fn(),
  copyPassword: vi.fn(), revealPassword: vi.fn(), savePassword: vi.fn(), bulkPasswordAction: vi.fn(), deletePassword: vi.fn(),
  pickOpenBackup: vi.fn(), previewRestoreBackup: vi.fn(), confirmRestoreBackup: vi.fn(), pickSaveBackup: vi.fn(), exportBackup: vi.fn(), saveSettings: vi.fn(),
  previewExclusions: vi.fn(), openPath: vi.fn(), recover: vi.fn(),
}));
vi.mock('../src/api', () => ({ isDesktop: true, api: bridge }));

const snapshot: AppSnapshot = { jobs: [], passwords: [{ id: 'fixture-pw', name: '测试条目', groups: ['测试组'], notes: '', enabled: true, revision: 1, successCount: 0, lastSuccess: null }], settings: { ...defaultSettings }, running: false, paused: false, vaultError: null, dataDirectory: 'C:\\fixtures\\isolated', engineAvailable: true, recoverableCount: 0, cacheUsedBytes: 0 };
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
});

describe('显式密码操作', () => {
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
    await user.click(await screen.findByRole('button', { name: '删除', exact: true }));
    expect(onAction).toHaveBeenCalledExactlyOnceWith('delete');
  });
  it('清空分组明确确认后才提交，密码复制走原生端口且不读取口令', async () => {
    const user = userEvent.setup(); const onAction = vi.fn();
    const first = renderPage(<BulkPasswordBar selectedCount={1} disabled={false} busy={false} groups={[]} onAction={onAction} onClear={() => {}} />);
    await user.click(screen.getByRole('button', { name: '清空所选密码的分组' }));
    expect(onAction).not.toHaveBeenCalled();
    await user.click(screen.getByRole('button', { name: '清空分组', exact: true }));
    expect(onAction).toHaveBeenCalledExactlyOnceWith('setGroups', []);
    first.unmount();
    renderPage(<VaultPage snapshot={snapshot} refresh={refresh} run={run} />);
    await user.click(screen.getByRole('button', { name: '复制 测试条目 口令' }));
    await waitFor(() => expect(bridge.copyPassword).toHaveBeenCalledExactlyOnceWith('fixture-pw'));
    expect(bridge.revealPassword).not.toHaveBeenCalled();
    expect(await screen.findByText(/30 秒后仅在剪贴板仍是本次复制内容/)).toBeTruthy();
  });
});

describe('备份恢复必须经过预览', () => {
  it('首次只解密预览，再明确确认才合并；预览不显示口令', async () => {
    const user = userEvent.setup();
    renderPage(<SettingsPage snapshot={snapshot} refresh={refresh} run={run} />);
    await user.click(screen.getByRole('tab', { name: /界面与备份/ }));
    await user.click(screen.getByRole('button', { name: '恢复备份并合并' }));
    await user.type(await screen.findByLabelText('备份口令'), 'fixture-backup-pass');
    await user.click(screen.getByRole('button', { name: '解密并预览' }));
    expect(await screen.findByText('备份测试名称')).toBeTruthy();
    expect(bridge.confirmRestoreBackup).not.toHaveBeenCalled();
    await user.click(screen.getByRole('button', { name: '确认合并' }));
    await waitFor(() => expect(bridge.confirmRestoreBackup).toHaveBeenCalledExactlyOnceWith('C:\\fixtures\\backup.ouvault', 'fixture-backup-pass', preview.token));
  });
  it('口令改变时废弃旧预览，关闭后迟到的预览不会重新打开', async () => {
    const user = userEvent.setup();
    renderPage(<SettingsPage snapshot={snapshot} refresh={refresh} run={run} />);
    await user.click(screen.getByRole('tab', { name: /界面与备份/ }));
    await user.click(screen.getByRole('button', { name: '恢复备份并合并' }));
    await user.type(await screen.findByLabelText('备份口令'), 'fixture-backup-pass');
    await user.click(screen.getByRole('button', { name: '解密并预览' }));
    await screen.findByText('备份测试名称');
    await user.clear(screen.getByLabelText('备份口令'));
    await user.type(screen.getByLabelText('备份口令'), 'changed-backup-pass');
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
    await user.click(screen.getByRole('button', { name: /添加文件$/, exact: false }));
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
    const job: JobView = { id: 'fixture-job', source: { path: 'C:\\fixtures\\outer.zip', displayName: 'outer.zip', volumes: [], size: 1, missingVolume: false }, options: { ...defaultSettings }, status: 'needsPassword', stage: 'nested', progress: 0.5, message: '内包需要密码', attempts: 1, outputPath: null, verificationScope: '', createdAt: '', finishedAt: null, workspace: 'C:\\fixtures\\work', stageTimes: {}, nodes: [{ id: 'outer', parentId: null, name: 'outer.zip', depth: 0, status: 'completed', stage: 'finished', progress: 1, attempts: 1, message: '', passwordLabel: null }, { id: 'inner', parentId: 'outer', name: 'inner.7z', depth: 1, status: 'needsPassword', stage: 'nested', progress: 0, attempts: 1, message: '', passwordLabel: null }] };
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

describe('规则与高级动作', () => {
  it('规则改变后丢弃迟到试算结果，不冒充已读归档内容', async () => {
    const user = userEvent.setup(); const pending = deferred<{ path: string; matched: boolean }[]>();
    bridge.previewExclusions.mockReturnValueOnce(pending.promise);
    const page = renderPage(<RulePreview inputPatterns={['*.url']} outputPatterns={[]} run={run} />);
    await user.type(screen.getByLabelText('待试算的相对路径'), '资料/说明.url');
    await user.click(screen.getByRole('button', { name: '预览匹配' }));
    expect(bridge.previewExclusions).toHaveBeenCalledExactlyOnceWith(['资料/说明.url'], ['*.url']);
    page.rerender(<ConfigProvider locale={zhCN} button={{ autoInsertSpace: false }} theme={{ token: { motion: false } }}><AntApp><RulePreview inputPatterns={['*.bak']} outputPatterns={[]} run={run} /></AntApp></ConfigProvider>);
    expect((screen.getByLabelText('待试算的相对路径') as HTMLTextAreaElement).value).toBe('资料/说明.url');
    pending.resolve([{ path: '资料/说明.url', matched: true }]);
    await waitFor(() => expect((screen.getByRole('button', { name: '预览匹配' }) as HTMLButtonElement).disabled).toBe(false));
    expect(screen.queryByText('命中：将排除')).toBeNull();
    expect(screen.getByText(/这是当前规则的匹配预览/)).toBeTruthy();
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
});
