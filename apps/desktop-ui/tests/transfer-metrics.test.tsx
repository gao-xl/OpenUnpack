import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { act, cleanup, render, screen, within } from '@testing-library/react';
import { App as AntApp, ConfigProvider } from 'antd';
import zhCN from 'antd/locale/zh_CN';
import TransferMetrics, { formatRemainingTime, formatTransferRate } from '../src/components/TransferMetrics';
import QueuePage from '../src/components/QueuePage';
import { JobDetails } from '../src/components/shared';
import { defaultSettings } from '../src/types';
import type { AppSnapshot, JobView, TransferProgress } from '../src/types';

vi.mock('../src/api', () => ({ isDesktop: true, api: { onDrop: vi.fn().mockResolvedValue(() => {}), cancelScan: vi.fn() } }));
const now = Date.parse('2026-10-11T12:00:00Z');
const observed: TransferProgress = { completedBytes: 8 * 1024 ** 2, totalBytes: 128 * 1024 ** 2, bytesPerSecond: 2 * 1024 ** 2, remainingSeconds: 60, updatedAt: new Date(now).toISOString() };
const fixture: JobView = {
  id: 'speed-fixture', source: { path: 'C:\\fixtures\\outer.zip', displayName: 'outer.zip', volumes: [], size: 1024, missingVolume: false },
  options: { ...defaultSettings }, status: 'running', stage: 'extracting', progress: 0.0625, message: '', attempts: 1, outputPath: null,
  verificationScope: '', createdAt: '2026-10-11T11:59:00Z', finishedAt: null, workspace: null, nodes: [], stageTimes: {}, transferProgress: observed,
};
const job = (changes: Partial<JobView> = {}, sample: Partial<TransferProgress> = {}): JobView => ({ ...fixture, ...changes, transferProgress: changes.transferProgress === null ? null : { ...observed, ...sample } });
const run = async (work: () => Promise<unknown>) => { await work(); };
const refresh = async () => {};
const renderPage = (element: React.ReactNode) => render(<ConfigProvider locale={zhCN} theme={{ token: { motion: false } }}><AntApp>{element}</AntApp></ConfigProvider>);

beforeEach(() => { vi.useFakeTimers(); vi.setSystemTime(now); });
afterEach(() => { cleanup(); vi.useRealTimers(); });

describe('实时指标格式', () => {
  it('以 1024 为单位显示真实输出/传输速度，避免将小于 1 字节的值格式化为旧的 0', () => {
    expect(formatTransferRate(512)).toBe('512 B/s');
    expect(formatTransferRate(1024)).toBe('1.0 KiB/s');
    expect(formatTransferRate(3 * 1024 ** 2)).toBe('3.0 MiB/s');
    expect(formatTransferRate(0.01)).toBe('<0.1 B/s');
    expect(formatTransferRate(0)).toBe('—');
    expect(formatTransferRate(Infinity)).toBe('—');
  });
  it('预计时间向上取整并用简洁的中文表达，拒绝非法值', () => {
    expect(formatRemainingTime(0)).toBe('0 秒');
    expect(formatRemainingTime(1.01)).toBe('2 秒');
    expect(formatRemainingTime(61.2)).toBe('1 分 2 秒');
    expect(formatRemainingTime(3_661)).toBe('1 小时 1 分');
    expect(formatRemainingTime(90_000)).toBe('1 天 1 小时');
    expect(formatRemainingTime(100 * 86_400)).toBe('超过 99 天');
    expect(formatRemainingTime(-1)).toBe('—');
    expect(formatRemainingTime(NaN)).toBe('—');
  });
});

describe('实际进度与阶段范围', () => {
  it.each([
    ['extracting', '输出速度', '预计本阶段剩余'],
    ['nested', '输出速度', '预计当前内包剩余'],
    ['caching', '传输速度', '预计本阶段剩余'],
    ['copying', '传输速度', '预计本阶段剩余'],
    ['verifying', '回读速度', '预计本阶段剩余'],
  ])('%s 使用当前阶段真实计量并明确估计范围', (stage, label, scope) => {
    render(<TransferMetrics job={job({ stage })} />);
    expect(screen.getByText(`${label}：2.0 MiB/s`)).toBeTruthy();
    expect(screen.getByText(`${scope}：约 1 分 0 秒`)).toBeTruthy();
    expect(screen.getByRole('group', { name: '实时速度与预计剩余时间' }).hasAttribute('aria-live')).toBe(false);
  });
  it('没有样本、零输出或尚未测速时说明正在估算，不展示没有依据的 ETA', () => {
    const page = render(<TransferMetrics job={job({ transferProgress: null })} />);
    expect(screen.getByText('输出速度：正在估算')).toBeTruthy();
    page.rerender(<TransferMetrics job={job({}, { completedBytes: 0 })} />);
    expect(screen.getByText('预计本阶段剩余：正在估算')).toBeTruthy();
    expect(screen.queryByText(/MiB\/s/)).toBeNull();
    page.rerender(<TransferMetrics job={job({}, { bytesPerSecond: null })} />);
    expect(screen.getByText('输出速度：正在估算')).toBeTruthy();
    page.rerender(<TransferMetrics job={job({}, { bytesPerSecond: 0 })} />);
    expect(screen.getByText('预计本阶段剩余：正在估算')).toBeTruthy();
  });
  it('未知总量仍可显示实际速度，但不推算剩余时间', () => {
    render(<TransferMetrics job={job({}, { totalBytes: 0, remainingSeconds: 120 })} />);
    expect(screen.getByText('输出速度：2.0 MiB/s')).toBeTruthy();
    expect(screen.getByText('预计本阶段剩余：暂无法估算')).toBeTruthy();
  });
  it.each(['completed', 'failed', 'cancelled', 'queued', 'paused', 'stopping', 'needsPassword', 'waitingResource', 'interrupted', 'publishPending', 'cleanupPending', 'partial', 'skipped'])('%s 不遗留速度与剩余时间', status => {
    render(<TransferMetrics job={job({ status })} />);
    expect(screen.queryByRole('group')).toBeNull();
    expect(screen.queryByText(/MiB\/s/)).toBeNull();
  });
  it('非传输阶段不沿用前一阶段数字，阶段改变后正确展示新样本', () => {
    const page = render(<TransferMetrics job={job({ stage: 'planning' })} />);
    expect(screen.getByText('本阶段暂无法估算速度与剩余时间')).toBeTruthy();
    expect(screen.queryByText(/MiB\/s/)).toBeNull();
    page.rerender(<TransferMetrics job={job({ stage: 'copying' }, { bytesPerSecond: 4 * 1024 ** 2, remainingSeconds: 30 })} />);
    expect(screen.getByText('传输速度：4.0 MiB/s')).toBeTruthy();
    expect(screen.getByText('预计本阶段剩余：约 30 秒')).toBeTruthy();
  });
});

describe('采样有效期与无效数据', () => {
  it('没有快照更新时恰好 5 秒降级；失效计时器不会虚构速度或倒计时', () => {
    render(<TransferMetrics job={job()} />);
    act(() => { vi.advanceTimersByTime(4_999); });
    expect(screen.getByText('输出速度：2.0 MiB/s')).toBeTruthy();
    expect(screen.getByText('预计本阶段剩余：约 1 分 0 秒')).toBeTruthy();
    act(() => { vi.advanceTimersByTime(1); });
    expect(screen.getByText('输出速度：等待新进度')).toBeTruthy();
    expect(screen.getByText('预计本阶段剩余：等待新进度')).toBeTruthy();
    expect(screen.queryByText(/MiB\/s/)).toBeNull();
  });
  it('新进度替换过期计时器，失效后仍能恢复真实数值', () => {
    const page = render(<TransferMetrics job={job()} />);
    act(() => { vi.advanceTimersByTime(4_000); });
    const updatedAt = new Date(Date.now()).toISOString();
    page.rerender(<TransferMetrics job={job({}, { updatedAt, bytesPerSecond: 3 * 1024 ** 2, remainingSeconds: 40 })} />);
    act(() => { vi.advanceTimersByTime(1_000); });
    expect(screen.getByText('输出速度：3.0 MiB/s')).toBeTruthy();
    act(() => { vi.advanceTimersByTime(4_000); });
    expect(screen.getByText('输出速度：等待新进度')).toBeTruthy();
    page.rerender(<TransferMetrics job={job({}, { updatedAt: new Date(Date.now()).toISOString() })} />);
    expect(screen.getByText('输出速度：2.0 MiB/s')).toBeTruthy();
  });
  it('墙钟回拨不会撤销计时器到期的失效状态，新采样才恢复速度和 ETA', () => {
    const page = render(<TransferMetrics job={job()} />);
    act(() => { vi.advanceTimersByTime(4_000); });
    vi.setSystemTime(now + 1_000);
    act(() => { vi.advanceTimersByTime(1_000); });
    // Only two wall-clock seconds have passed, but five timer seconds have elapsed.
    expect(Date.now()).toBe(now + 2_000);
    expect(screen.getByText('输出速度：等待新进度')).toBeTruthy();
    expect(screen.getByText('预计本阶段剩余：等待新进度')).toBeTruthy();
    page.rerender(<TransferMetrics job={job({}, { updatedAt: observed.updatedAt })} />);
    expect(screen.queryByText(/MiB\/s/)).toBeNull();
    page.rerender(<TransferMetrics job={job({}, { updatedAt: new Date(Date.now()).toISOString(), bytesPerSecond: 3 * 1024 ** 2, remainingSeconds: 40 })} />);
    expect(screen.getByText('输出速度：3.0 MiB/s')).toBeTruthy();
    expect(screen.getByText('预计本阶段剩余：约 40 秒')).toBeTruthy();
  });
  it.each(['bad timestamp', '2026-10-11', '2026-02-30T12:00:00Z', '2026-10-10T24:00:00Z', '2026-10-11T12:00:00.001Z', '2026-10-11T11:59:55Z'])('拒绝无效、未来或陈旧的时间戳：%s', updatedAt => {
    render(<TransferMetrics job={job({}, { updatedAt })} />);
    expect(screen.getByText('输出速度：等待新进度')).toBeTruthy();
    expect(screen.getByText('预计本阶段剩余：等待新进度')).toBeTruthy();
  });
  it.each<Partial<TransferProgress>>([
    { completedBytes: -1 }, { completedBytes: 0.5 }, { completedBytes: Number.MAX_SAFE_INTEGER + 1 },
    { totalBytes: Infinity }, { totalBytes: -1 }, { totalBytes: 1 }, { bytesPerSecond: NaN },
    { bytesPerSecond: Infinity }, { bytesPerSecond: -1 }, { bytesPerSecond: Number.MAX_SAFE_INTEGER + 1 },
    { remainingSeconds: -1 }, { remainingSeconds: Infinity }, { remainingSeconds: Number.MAX_SAFE_INTEGER + 1 },
  ])('安全数值校验失败时不显示旧值：%j', sample => {
    render(<TransferMetrics job={job({}, sample)} />);
    expect(screen.getByText('输出速度：等待新进度')).toBeTruthy();
    expect(screen.queryByText(/MiB\/s/)).toBeNull();
    expect(screen.queryByText(/约 .*秒/)).toBeNull();
  });
  it('卸载后清理唯一的失效计时器', () => {
    const page = render(<TransferMetrics job={job()} />);
    expect(vi.getTimerCount()).toBe(1);
    page.unmount();
    expect(vi.getTimerCount()).toBe(0);
  });
});

describe('队列与任务详情采用一致的真实指标', () => {
  it('全局暂停仅阻止新任务；仍在执行的 Worker 在队列和详情中都显示新进度', () => {
    const nested = job({ stage: 'nested' });
    const snapshot: AppSnapshot = { jobs: [nested], passwords: [], settings: { ...defaultSettings }, running: true, paused: true, vaultError: null, dataDirectory: 'C:\\fixtures\\data', engineAvailable: true, recoverableCount: 0, cacheUsedBytes: 0, sessionId: 'fixture', revision: '1' };
    renderPage(<><QueuePage snapshot={snapshot} run={run} refresh={refresh} /><JobDetails job={nested} run={run} close={() => {}} /></>);
    const metrics = screen.getAllByRole('group', { name: '实时速度与预计剩余时间' });
    expect(metrics).toHaveLength(2);
    for (const group of metrics) {
      expect(within(group).getByText('输出速度：2.0 MiB/s')).toBeTruthy();
      expect(within(group).getByText('预计当前内包剩余：约 1 分 0 秒')).toBeTruthy();
    }
    expect(screen.getAllByRole('progressbar').some(bar => bar.getAttribute('aria-valuenow') === '6')).toBe(true);
    act(() => { vi.advanceTimersByTime(5_000); });
    expect(screen.getAllByText('输出速度：等待新进度')).toHaveLength(2);
  });
  it('任务完成后队列与详情不遗留实时速度，仍显示原有阶段百分比', () => {
    const finished = job({ status: 'completed', stage: 'finished', progress: 1 });
    const snapshot: AppSnapshot = { jobs: [finished], passwords: [], settings: { ...defaultSettings }, running: false, paused: false, vaultError: null, dataDirectory: '', engineAvailable: true, recoverableCount: 0, cacheUsedBytes: 0, sessionId: 'fixture', revision: '2' };
    renderPage(<><QueuePage snapshot={snapshot} run={run} refresh={refresh} /><JobDetails job={finished} run={run} close={() => {}} /></>);
    expect(screen.queryByRole('group', { name: '实时速度与预计剩余时间' })).toBeNull();
    expect(screen.queryByText(/MiB\/s/)).toBeNull();
    expect(screen.getAllByRole('progressbar').some(bar => bar.getAttribute('aria-valuenow') === '100')).toBe(true);
  });
});
