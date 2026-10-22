import { act, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { App as AntApp, ConfigProvider } from 'antd';
import zhCN from 'antd/locale/zh_CN';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import HistoryPage from '../src/components/HistoryPage';
import type { AppSnapshot, JobHistoryPage, JobView } from '../src/types';
import { defaultSettings } from '../src/types';
import { historyPageResult } from './history-fixture';

const bridge = vi.hoisted(() => ({ historyPage: vi.fn(), history: vi.fn(), jobDetails: vi.fn(), remove: vi.fn(), retry: vi.fn(), recover: vi.fn(), openPath: vi.fn(), clearHistory: vi.fn() }));
vi.mock('../src/api', () => ({ api: bridge, isDesktop: true }));
const base: AppSnapshot = { jobs: [], passwords: [], settings: { ...defaultSettings }, running: false, paused: false, vaultError: null, dataDirectory: '', engineAvailable: true, recoverableCount: 0, cacheUsedBytes: 0, sessionId: 'fixture', revision: '1' };
const run = async (work: () => Promise<unknown>) => { await work(); };
const refresh = vi.fn(async () => {});
function job(id: string): JobView {
  return { id, source: { path: 'C:\\fixtures\\' + id + '.zip', displayName: id + '.zip', volumes: [], size: 1, missingVolume: false }, options: { ...defaultSettings }, status: 'completed', stage: 'finished', progress: 1, message: '', attempts: 1, outputPath: null, verificationScope: '', createdAt: '', finishedAt: null, workspace: null, nodes: [], stageTimes: {}, transferProgress: null };
}
function deferred<T>() { let resolve!: (value: T) => void; let reject!: (reason: unknown) => void; const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; }); return { promise, resolve, reject }; }
const view = (snapshot = base) => <ConfigProvider locale={zhCN} button={{ autoInsertSpace: false }} theme={{ token: { motion: false } }}><AntApp><HistoryPage snapshot={snapshot} refresh={refresh} run={run} /></AntApp></ConfigProvider>;
beforeEach(() => { vi.resetAllMocks(); bridge.jobDetails.mockResolvedValue(null); bridge.remove.mockResolvedValue(undefined); });
afterEach(() => { vi.restoreAllMocks(); });

it('只读取当前页，翻页和搜索交给后端，旧全量历史接口不调用', async () => {
  const user = userEvent.setup();
  const jobs = Array.from({ length: 21 }, (_, index) => job('record-' + String(index).padStart(2, '0')));
  bridge.historyPage.mockImplementation(query => Promise.resolve(historyPageResult(jobs, query)));
  render(view());
  await screen.findByText('record-00.zip');
  expect(screen.queryByText('record-10.zip')).toBeNull();
  await user.click(screen.getByTitle('下一页'));
  await screen.findByText('record-10.zip');
  expect(screen.queryByText('record-00.zip')).toBeNull();
  expect(bridge.historyPage).toHaveBeenLastCalledWith({ offset: 10, limit: 10, status: 'all', search: '' });
  await user.type(screen.getByPlaceholderText('搜索文件路径或名称'), 'record-20');
  await screen.findByText('record-20.zip');
  expect(bridge.historyPage).toHaveBeenLastCalledWith({ offset: 0, limit: 10, status: 'all', search: 'record-20' });
  expect(bridge.history).not.toHaveBeenCalled();
  expect(bridge.jobDetails).not.toHaveBeenCalled();
});

it('过滤条件变化时串行读取，丢弃旧响应并查询最新条件', async () => {
  const user = userEvent.setup(); const pending = deferred<JobHistoryPage>();
  bridge.historyPage.mockReturnValueOnce(pending.promise).mockResolvedValue(historyPageResult([job('new')]));
  render(view());
  await waitFor(() => expect(bridge.historyPage).toHaveBeenCalledTimes(1));
  await user.type(screen.getByPlaceholderText('搜索文件路径或名称'), 'new');
  expect(bridge.historyPage).toHaveBeenCalledTimes(1);
  await act(async () => { pending.resolve(historyPageResult([job('old')])); await pending.promise; });
  await screen.findByText('new.zip');
  expect(screen.queryByText('old.zip')).toBeNull();
  expect(bridge.historyPage).toHaveBeenCalledTimes(2);
  expect(bridge.historyPage).toHaveBeenLastCalledWith(expect.objectContaining({ search: 'new' }));
});

it('页读取失败保留先前结果并提供重试入口', async () => {
  const user = userEvent.setup();
  bridge.historyPage.mockResolvedValueOnce(historyPageResult([job('kept')])).mockRejectedValueOnce(new Error('存储暂时不可读'));
  render(view()); await screen.findByText('kept.zip');
  await user.click(screen.getByRole('button', { name: '刷新记录' }));
  await screen.findByText('存储暂时不可读');
  expect(screen.getByText('kept.zip')).toBeTruthy();
  expect(screen.getByRole('button', { name: '重试读取' })).toBeTruthy();
  expect(refresh).not.toHaveBeenCalled();
});

it('详情按需读取，迟到详情不能覆盖新选择', async () => {
  const user = userEvent.setup(); const pending = deferred<JobView | null>();
  bridge.historyPage.mockResolvedValue(historyPageResult([job('first'), job('second')]));
  bridge.jobDetails.mockReturnValueOnce(pending.promise).mockResolvedValueOnce(job('second'));
  render(view()); await screen.findByText('first.zip');
  await user.click(screen.getAllByRole('button', { name: '详情' })[0]);
  await user.click(screen.getAllByRole('button', { name: '详情' })[1]);
  await waitFor(() => expect(bridge.jobDetails).toHaveBeenCalledTimes(2));
  const drawer = await screen.findByRole('dialog');
  expect(within(drawer).getByText('second.zip', { selector: '.ant-drawer-title' })).toBeTruthy();
  await act(async () => { pending.resolve(job('first')); await pending.promise; });
  expect(within(drawer).queryByText('first.zip', { selector: '.ant-drawer-title' })).toBeNull();
});

it('当前页没有保护项时仍遵守后端的全历史清空保护', async () => {
  bridge.historyPage.mockResolvedValue({ ...historyPageResult([job('plain')]), clearAllowed: false });
  render(view()); await screen.findByText('plain.zip');
  expect((screen.getByRole('button', { name: '清空历史记录' }) as HTMLButtonElement).disabled).toBe(true);
});

it('发布待核对保留回读和重试写回入口，不能直接删除或清空', async () => {
  const user = userEvent.setup();
  const pending = { ...job('pending-publication'), status: 'publishPending', workspace: 'C:\\fixtures\\retained' };
  bridge.historyPage.mockResolvedValue(historyPageResult([pending]));
  bridge.recover.mockResolvedValue(undefined);
  render(view()); await screen.findByText('pending-publication.zip');
  expect((screen.getByRole('button', { name: '删除任务 pending-publication.zip' }) as HTMLButtonElement).disabled).toBe(true);
  expect((screen.getByRole('button', { name: '清空历史记录' }) as HTMLButtonElement).disabled).toBe(true);
  await user.click(screen.getByRole('button', { name: '回读并核对' }));
  await waitFor(() => expect(bridge.recover).toHaveBeenCalledWith('pending-publication', 'verify'));
  await user.click(screen.getByRole('button', { name: '重试写回' }));
  await waitFor(() => expect(bridge.recover).toHaveBeenCalledWith('pending-publication', 'retry'));
  expect(bridge.retry).not.toHaveBeenCalled();
});

it('慢详情读取期间后台页刷新不会使它不断过期或重复排队', async () => {
  const user = userEvent.setup(); let poll!: () => void;
  const original = globalThis.setInterval;
  vi.spyOn(globalThis, 'setInterval').mockImplementation(((handler: TimerHandler, timeout?: number, ...args: unknown[]) => {
    if (timeout === 2000 && typeof handler === 'function') {
      poll = handler as () => void;
      // Dispatch polls explicitly: wall-clock ticks during a slow CI render
      // must not introduce extra queries or synthetic deletion responses.
      return original(() => {}, timeout, ...args);
    }
    return original(handler, timeout, ...args);
  }) as typeof setInterval);
  const pending = deferred<JobView | null>();
  bridge.historyPage.mockResolvedValue(historyPageResult([job('slow')]));
  bridge.jobDetails.mockReturnValueOnce(pending.promise).mockResolvedValue(job('slow'));
  render(view()); await screen.findByText('slow.zip');
  await user.click(screen.getByRole('button', { name: '详情' }));
  await act(async () => { poll(); });
  expect(bridge.historyPage).toHaveBeenCalledTimes(2);
  expect(bridge.jobDetails).toHaveBeenCalledTimes(1);
  await act(async () => { pending.resolve(job('slow')); await pending.promise; });
  expect(await screen.findByRole('dialog')).toBeTruthy();
});

it('隐藏页面停止轮询，已有页查询期间轮询不产生重叠请求', async () => {
  let poll!: () => void;
  const original = globalThis.setInterval;
  vi.spyOn(globalThis, 'setInterval').mockImplementation(((handler: TimerHandler, timeout?: number, ...args: unknown[]) => {
    if (timeout === 2000 && typeof handler === 'function') {
      poll = handler as () => void;
      return original(() => {}, timeout, ...args);
    }
    return original(handler, timeout, ...args);
  }) as typeof setInterval);
  const pending = deferred<JobHistoryPage>();
  bridge.historyPage.mockReturnValueOnce(pending.promise).mockResolvedValue(historyPageResult([job('poll')]));
  const mounted = render(view());
  await waitFor(() => expect(bridge.historyPage).toHaveBeenCalledTimes(1));
  act(() => poll()); expect(bridge.historyPage).toHaveBeenCalledTimes(1);
  await act(async () => { pending.resolve(historyPageResult([job('initial')])); await pending.promise; });
  const hidden = vi.spyOn(document, 'hidden', 'get').mockReturnValue(true);
  act(() => poll()); expect(bridge.historyPage).toHaveBeenCalledTimes(1);
  hidden.mockReturnValue(false);
  await act(async () => { poll(); });
  await screen.findByText('poll.zip');
  expect(bridge.historyPage).toHaveBeenCalledTimes(2);
  mounted.unmount();
});
