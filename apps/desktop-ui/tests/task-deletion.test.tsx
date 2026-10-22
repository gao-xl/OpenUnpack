import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { act, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { App as AntApp, ConfigProvider } from 'antd';
import zhCN from 'antd/locale/zh_CN';
import QueuePage from '../src/components/QueuePage';
import HistoryPage from '../src/components/HistoryPage';
import { defaultSettings } from '../src/types';
import type { AppSnapshot, JobView } from '../src/types';
import { historyPageResult } from './history-fixture';

const bridge = vi.hoisted(() => ({
  onDrop: vi.fn(), remove: vi.fn(), historyPage: vi.fn(), jobDetails: vi.fn(), cancel: vi.fn(), retry: vi.fn(),
  recover: vi.fn(), openPath: vi.fn(), clearHistory: vi.fn(), clearCompleted: vi.fn(),
}));
vi.mock('../src/api', () => ({ isDesktop: true, api: bridge }));

const base: AppSnapshot = { jobs: [], passwords: [], settings: { ...defaultSettings }, running: false, paused: false, vaultError: null, dataDirectory: 'C:\\fixtures\\isolated', engineAvailable: true, recoverableCount: 0, cacheUsedBytes: 0, sessionId: 'delete-fixture', revision: '1' };
const errors: string[] = [];
const run = async (work: () => Promise<unknown>) => {
  try { await work(); } catch (error) { errors.push(error instanceof Error ? error.message : String(error)); }
};
const renderPage = (page: React.ReactNode) => render(<ConfigProvider locale={zhCN} button={{ autoInsertSpace: false }} theme={{ token: { motion: false } }}><AntApp>{page}</AntApp></ConfigProvider>);
const refresh = vi.fn(async () => {});
function job(id: string, overrides: Partial<JobView> = {}): JobView {
  return { id, source: { path: `C:\\fixtures\\${id}.zip`, displayName: `${id}.zip`, volumes: [], size: 1024, missingVolume: false }, options: { ...defaultSettings }, status: 'queued', stage: 'queued', progress: 0, message: '', attempts: 0, outputPath: null, verificationScope: '', createdAt: '', finishedAt: null, workspace: null, stageTimes: {}, transferProgress: null, nodes: [], ...overrides };
}
function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}

beforeEach(() => {
  vi.resetAllMocks(); errors.length = 0;
  // These cases assert explicit delete reloads; polling is covered separately.
  vi.spyOn(document, 'hidden', 'get').mockReturnValue(true);
  bridge.onDrop.mockResolvedValue(() => {});
  bridge.remove.mockResolvedValue(undefined);
  bridge.historyPage.mockResolvedValue(historyPageResult([]));
  bridge.jobDetails.mockResolvedValue(null);
});
afterEach(() => { vi.restoreAllMocks(); });

describe('删除任务确认与异步结果', () => {
  it('单项删除明确说明保留源包与发布结果，取消不调用删除，确认才提交', async () => {
    const user = userEvent.setup();
    renderPage(<QueuePage snapshot={{ ...base, jobs: [job('queued')] }} run={run} refresh={refresh} />);
    await user.click(screen.getByRole('button', { name: '删除任务 queued.zip' }));
    const dialog = screen.getByRole('dialog');
    expect(within(dialog).getByText('原始压缩包和已经发布到目标目录的结果会保留。')).toBeTruthy();
    expect(within(dialog).getByText(/运行中的任务将先停止/)).toBeTruthy();
    expect(bridge.remove).not.toHaveBeenCalled();
    await user.click(within(dialog).getByRole('button', { name: '取消' }));
    expect(bridge.remove).not.toHaveBeenCalled();
    await user.click(screen.getByRole('button', { name: '删除任务 queued.zip' }));
    await user.click(within(screen.getByRole('dialog')).getByRole('button', { name: '删除任务' }));
    await waitFor(() => expect(bridge.remove).toHaveBeenCalledExactlyOnceWith('queued'));
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull());
  });

  it('其它任务运行时仍能删除等待任务或带临时缓存的失败任务', async () => {
    const user = userEvent.setup();
    renderPage(<QueuePage snapshot={{ ...base, running: true, jobs: [job('active', { status: 'running' }), job('waiting'), job('failed', { status: 'failed', workspace: 'C:\\fixtures\\cache\\owned' })] }} run={run} refresh={refresh} />);
    for (const id of ['waiting', 'failed']) {
      const button = screen.getByRole('button', { name: `删除任务 ${id}.zip` }) as HTMLButtonElement;
      expect(button.disabled).toBe(false);
      await user.click(button);
      await user.click(within(screen.getByRole('dialog')).getByRole('button', { name: '删除任务' }));
      await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull());
    }
    expect(bridge.remove.mock.calls.map(call => call[0])).toEqual(['waiting', 'failed']);
    expect(bridge.cancel).not.toHaveBeenCalled();
  });

  it('运行任务删除必须等待后端完成，期间显示正在删除并阻止重复操作', async () => {
    const user = userEvent.setup(); const pending = deferred<void>();
    bridge.remove.mockReturnValueOnce(pending.promise);
    renderPage(<QueuePage snapshot={{ ...base, running: true, jobs: [job('active', { status: 'running', stage: 'extracting' })] }} run={run} refresh={refresh} />);
    const button = screen.getByRole('button', { name: '删除任务 active.zip' }) as HTMLButtonElement;
    await user.click(button);
    await user.click(within(screen.getByRole('dialog')).getByRole('button', { name: '删除任务' }));
    const dialog = screen.getByRole('dialog');
    expect(within(dialog).getByRole('status').textContent).toContain('正在等待任务停止并清理缓存');
    expect((within(dialog).getByRole('button', { name: '取消' }) as HTMLButtonElement).disabled).toBe(true);
    expect(button.disabled).toBe(true);
    await user.click(within(dialog).getByRole('button', { name: '正在删除' }));
    expect(bridge.remove).toHaveBeenCalledExactlyOnceWith('active');
    expect(bridge.cancel).not.toHaveBeenCalled();
    await act(async () => { pending.resolve(); });
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull());
    expect(errors).toEqual([]);
  });

  it('批量删除继续处理失败后的任务，成功项取消选择，失败项和原因保留并可单独重试', async () => {
    const user = userEvent.setup();
    bridge.remove.mockRejectedValueOnce(new Error('临时缓存仍被占用')).mockResolvedValueOnce(undefined).mockRejectedValueOnce(new Error('发布结果尚未核对'));
    renderPage(<QueuePage snapshot={{ ...base, running: true, jobs: [job('first', { status: 'failed', workspace: 'C:\\fixtures\\cache\\first' }), job('second'), job('third')] }} run={run} refresh={refresh} />);
    for (const id of ['first', 'second', 'third']) await user.click(screen.getByRole('checkbox', { name: `选择任务 ${id}.zip` }));
    await user.click(screen.getByRole('button', { name: '删除所选任务' }));
    expect(screen.getByText('删除 3 个任务？')).toBeTruthy();
    await user.click(within(screen.getByRole('dialog')).getByRole('button', { name: '删除任务' }));
    await screen.findByText('这些任务未删除，可重试或取消');
    expect(bridge.remove.mock.calls.map(call => call[0])).toEqual(['first', 'second', 'third']);
    expect(screen.getByText('first.zip：临时缓存仍被占用')).toBeTruthy();
    expect(screen.getByText('third.zip：发布结果尚未核对')).toBeTruthy();
    expect((screen.getByRole('checkbox', { name: '选择任务 first.zip', hidden: true }) as HTMLInputElement).checked).toBe(true);
    expect((screen.getByRole('checkbox', { name: '选择任务 second.zip', hidden: true }) as HTMLInputElement).checked).toBe(false);
    expect((screen.getByRole('checkbox', { name: '选择任务 third.zip', hidden: true }) as HTMLInputElement).checked).toBe(true);
    expect(errors).toHaveLength(1);
    await user.click(within(screen.getByRole('dialog')).getByRole('button', { name: '重试删除' }));
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull());
    expect(bridge.remove.mock.calls.map(call => call[0])).toEqual(['first', 'second', 'third', 'first', 'third']);
    expect((screen.getByRole('button', { name: '删除所选任务' }) as HTMLButtonElement).disabled).toBe(true);
  });

  it('首项仍在停止时立即提交后续选中任务，乱序返回按任务保留失败选择与原因', async () => {
    const user = userEvent.setup(); const first = deferred<void>(); const second = deferred<void>();
    bridge.remove.mockImplementation((id: string) => id === 'active' ? first.promise : id === 'queued' ? second.promise : Promise.resolve());
    renderPage(<QueuePage snapshot={{ ...base, running: true, jobs: [job('active', { status: 'running' }), job('queued'), job('done')] }} run={run} refresh={refresh} />);
    for (const id of ['active', 'queued', 'done']) await user.click(screen.getByRole('checkbox', { name: `选择任务 ${id}.zip` }));
    await user.click(screen.getByRole('button', { name: '删除所选任务' }));
    await user.click(within(screen.getByRole('dialog')).getByRole('button', { name: '删除任务' }));
    await waitFor(() => expect(bridge.remove.mock.calls.map(call => call[0])).toEqual(['active', 'queued', 'done']));
    expect(within(screen.getByRole('dialog')).getByRole('status')).toBeTruthy();
    await act(async () => { second.reject(new Error('queued 缓存清理失败')); });
    expect((within(screen.getByRole('dialog')).getByRole('button', { name: '取消' }) as HTMLButtonElement).disabled).toBe(true);
    await act(async () => { first.resolve(); });
    await screen.findByText('queued.zip：queued 缓存清理失败');
    expect((screen.getByRole('checkbox', { name: '选择任务 active.zip', hidden: true }) as HTMLInputElement).checked).toBe(false);
    expect((screen.getByRole('checkbox', { name: '选择任务 queued.zip', hidden: true }) as HTMLInputElement).checked).toBe(true);
    expect((screen.getByRole('checkbox', { name: '选择任务 done.zip', hidden: true }) as HTMLInputElement).checked).toBe(false);
    expect(errors).toHaveLength(1);
    expect(errors[0]).toContain('queued.zip');
    expect(errors[0]).not.toContain('active.zip');
  });

  it('待核对或正在停止的任务显示明确不能删除的原因', () => {
    renderPage(<QueuePage snapshot={{ ...base, jobs: [job('pending', { status: 'publishPending' }), job('stopping', { status: 'stopping' })] }} run={run} refresh={refresh} />);
    const pending = screen.getByRole('button', { name: '删除任务 pending.zip' }) as HTMLButtonElement;
    const stopping = screen.getByRole('button', { name: '删除任务 stopping.zip' }) as HTMLButtonElement;
    expect(pending.disabled).toBe(true); expect(pending.title).toContain('发布结果尚未核对');
    expect(stopping.disabled).toBe(true); expect(stopping.title).toContain('等待后台进程停止');
    expect(bridge.remove).not.toHaveBeenCalled();
  });

  it('已完成批量删除必须确认，覆盖缓存待清理任务并明确移除历史而保留文件', async () => {
    const user = userEvent.setup();
    renderPage(<QueuePage snapshot={{ ...base, jobs: [job('done', { status: 'completed', outputPath: 'C:\\fixtures\\output\\done' }), job('cache', { status: 'cleanupPending', workspace: 'C:\\fixtures\\cache\\owned' }), job('waiting')] }} run={run} refresh={refresh} />);
    await user.click(screen.getByRole('button', { name: '删除当前显示的已结束任务' }));
    expect(screen.getByText('任务记录将同时从队列和处理记录中移除。')).toBeTruthy();
    expect(screen.getByText('原始压缩包和已经发布到目标目录的结果会保留。')).toBeTruthy();
    expect(bridge.remove).not.toHaveBeenCalled();
    await user.click(within(screen.getByRole('dialog')).getByRole('button', { name: '取消' }));
    expect(bridge.remove).not.toHaveBeenCalled();
    await user.click(screen.getByRole('button', { name: '删除当前显示的已结束任务' }));
    await user.click(within(screen.getByRole('dialog')).getByRole('button', { name: '删除任务' }));
    await waitFor(() => expect(bridge.remove.mock.calls.map(call => call[0])).toEqual(['done', 'cache']));
    expect(bridge.clearCompleted).not.toHaveBeenCalled();
  });

  it('停止中的尝试删除不承诺强行终止，未确认退出时保留任务并显示失败', async () => {
    const user = userEvent.setup();
    bridge.remove.mockRejectedValueOnce(new Error('后台进程退出尚未确认，请稍后重试'));
    renderPage(<QueuePage snapshot={{ ...base, jobs: [job('stop', { status: 'stopping' })] }} run={run} refresh={refresh} />);
    expect(screen.queryByRole('button', { name: '强行终止' })).toBeNull();
    const check = screen.getByRole('button', { name: '尝试删除' }) as HTMLButtonElement;
    expect(check.title).toContain('退出尚未确认时保留任务');
    await user.click(check);
    await user.click(within(screen.getByRole('dialog')).getByRole('button', { name: '删除任务' }));
    await screen.findByText('stop.zip：后台进程退出尚未确认，请稍后重试');
    expect(bridge.remove).toHaveBeenCalledExactlyOnceWith('stop');
    expect(screen.getByRole('dialog')).toBeTruthy();
    expect(screen.getAllByText('stop.zip')).toHaveLength(2);
  });
});

describe('历史任务删除', () => {
  it('全局队列运行时允许删除带缓存的失败记录，成功后重载历史与刷新队列', async () => {
    const user = userEvent.setup();
    bridge.historyPage.mockResolvedValueOnce(historyPageResult([job('history', { status: 'failed', workspace: 'C:\\fixtures\\cache\\history' })])).mockResolvedValue(historyPageResult([]));
    renderPage(<HistoryPage snapshot={{ ...base, running: true }} run={run} refresh={refresh} />);
    const button = await screen.findByRole('button', { name: '删除任务 history.zip' }) as HTMLButtonElement;
    expect(button.disabled).toBe(false);
    await user.click(button);
    await user.click(within(screen.getByRole('dialog')).getByRole('button', { name: '删除任务' }));
    await waitFor(() => expect(bridge.remove).toHaveBeenCalledExactlyOnceWith('history'));
    await waitFor(() => expect(bridge.historyPage).toHaveBeenCalledTimes(2));
    expect(refresh).toHaveBeenCalledTimes(1);
    expect(screen.queryByText('history.zip')).toBeNull();
    expect(bridge.recover).not.toHaveBeenCalled();
  });

  it('删除失败保留历史记录和确认框，不能显示删除成功或提前刷新', async () => {
    const user = userEvent.setup();
    bridge.historyPage.mockResolvedValue(historyPageResult([job('held', { status: 'cancelled', workspace: 'C:\\fixtures\\cache\\held' })]));
    bridge.remove.mockRejectedValueOnce(new Error('未确认后台进程退出'));
    renderPage(<HistoryPage snapshot={base} run={run} refresh={refresh} />);
    await user.click(await screen.findByRole('button', { name: '删除任务 held.zip' }));
    await user.click(within(screen.getByRole('dialog')).getByRole('button', { name: '删除任务' }));
    await screen.findByText('held.zip：未确认后台进程退出');
    expect(screen.getByRole('dialog')).toBeTruthy();
    expect(screen.getAllByText('held.zip')).toHaveLength(2);
    expect(bridge.historyPage).toHaveBeenCalledTimes(1);
    expect(refresh).not.toHaveBeenCalled();
    expect(errors[0]).toContain('未确认后台进程退出');
    await user.click(within(screen.getByRole('dialog')).getByRole('button', { name: '取消' }));
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull());
    expect(screen.getByText('held.zip')).toBeTruthy();
  });

  it('历史多选删除保留失败选项，成功记录移除后重新载入历史', async () => {
    const user = userEvent.setup(); const held = job('held', { status: 'failed' });
    bridge.historyPage.mockResolvedValueOnce(historyPageResult([held, job('removed', { status: 'completed' })])).mockResolvedValue(historyPageResult([held]));
    bridge.remove.mockRejectedValueOnce(new Error('缓存目录仍被占用')).mockResolvedValueOnce(undefined);
    renderPage(<HistoryPage snapshot={base} run={run} refresh={refresh} />);
    await screen.findByRole('checkbox', { name: '选择任务 held.zip' });
    for (const id of ['held', 'removed']) await user.click(screen.getByRole('checkbox', { name: `选择任务 ${id}.zip` }));
    await user.click(screen.getByRole('button', { name: '删除所选任务' }));
    await user.click(within(screen.getByRole('dialog')).getByRole('button', { name: '删除任务' }));
    await screen.findByText('held.zip：缓存目录仍被占用');
    await waitFor(() => expect(bridge.historyPage).toHaveBeenCalledTimes(2));
    expect(bridge.remove.mock.calls.map(call => call[0])).toEqual(['held', 'removed']);
    expect((screen.getByRole('checkbox', { name: '选择任务 held.zip', hidden: true }) as HTMLInputElement).checked).toBe(true);
    expect(screen.queryByRole('checkbox', { name: '选择任务 removed.zip', hidden: true })).toBeNull();
    expect(refresh).toHaveBeenCalledTimes(1);
  });

  it('删除当前详情任务后关闭详情且删除期间禁用详情内重试', async () => {
    const user = userEvent.setup(); const pending = deferred<void>();
    bridge.jobDetails.mockResolvedValue(job('detail', { status: 'failed' }));
    bridge.historyPage.mockResolvedValueOnce(historyPageResult([job('detail', { status: 'failed' })])).mockResolvedValue(historyPageResult([]));
    bridge.remove.mockReturnValueOnce(pending.promise);
    renderPage(<HistoryPage snapshot={base} run={run} refresh={refresh} />);
    const deleteButton = await screen.findByRole('button', { name: '删除任务 detail.zip' });
    await user.click(screen.getByText('详情', { selector: 'span' }));
    const retry = await screen.findByRole('button', { name: '按当前设置重试任务' }) as HTMLButtonElement;
    expect(retry.disabled).toBe(false);
    await user.click(deleteButton);
    const deletionDialog = screen.getByText('运行中的任务将先停止，确认后台进程停止后再删除任务记录，并清理本应用的临时缓存。').closest('[role="dialog"]') as HTMLElement;
    await user.click(within(deletionDialog).getByRole('button', { name: '删除任务' }));
    expect(retry.disabled).toBe(true);
    await act(async () => { pending.resolve(); });
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull());
    expect(screen.queryByText('detail.zip')).toBeNull();
    expect(bridge.retry).not.toHaveBeenCalled();
  });
});
