import { beforeEach, expect, it, vi } from 'vitest';
import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { App as AntApp, ConfigProvider } from 'antd';
import QueuePage from '../src/components/QueuePage';
import { defaultSettings, type AppSnapshot, type JobView } from '../src/types';
const bridge = vi.hoisted(() => ({ isDaemonMode: vi.fn(), startSelected: vi.fn(), start: vi.fn(), pause: vi.fn(), cancel: vi.fn(), retry: vi.fn(), remove: vi.fn(), onDrop: vi.fn(), cancelScan: vi.fn(), submitInput: vi.fn(), savePassword: vi.fn() }));
vi.mock('../src/api', () => ({ isDesktop: false, api: bridge }));
function job(id: string, status = 'queued'): JobView { return { id, source: { path: 'C:\\approved\\' + id + '.zip', displayName: id + '.zip', size: 1, volumes: [], missingVolume: false }, options: { ...defaultSettings }, status, stage: 'queued', progress: 0, message: '', attempts: 0, outputPath: null, verificationScope: '', createdAt: '2026-10-20', finishedAt: null, workspace: null, nodes: [], stageTimes: {}, transferProgress: null }; }
const snapshot: AppSnapshot = { jobs: [job('queued-a'), job('waiting-b', 'waitingResource'), job('done', 'completed')], passwords: [], settings: { ...defaultSettings }, running: false, paused: false, vaultError: null, dataDirectory: '', engineAvailable: true, recoverableCount: 0, cacheUsedBytes: 0, sessionId: 'daemon-test', revision: '1' };
const run = async (work: () => Promise<unknown>) => { await work(); };
beforeEach(() => { vi.resetAllMocks(); bridge.isDaemonMode.mockReturnValue(true); bridge.onDrop.mockResolvedValue(() => {}); bridge.startSelected.mockResolvedValue(undefined); bridge.cancel.mockResolvedValue(undefined); bridge.submitInput.mockResolvedValue(undefined); });
function show(state = snapshot) { return render(<ConfigProvider button={{ autoInsertSpace: false }} theme={{ token: { motion: false } }}><AntApp><QueuePage snapshot={state} refresh={async () => {}} run={run} /></AntApp></ConfigProvider>); }
it('browser daemon queue controls call selected-job API while local file pickers stay disabled', async () => {
 const user = userEvent.setup(); show();
 expect(screen.getByRole('button', { name: '添加文件' })).toHaveProperty('disabled', true);
 expect(screen.getByRole('button', { name: '添加文件夹' })).toHaveProperty('disabled', true);
 expect(screen.getByLabelText('服务端归档路径')).toBeTruthy();
 const start = screen.getByRole('button', { name: '开始所选' }); expect(start).toHaveProperty('disabled', true);
 await user.click(screen.getByRole('checkbox', { name: '选择任务 queued-a.zip' }));
 await user.click(start);
 expect(bridge.startSelected).toHaveBeenCalledExactlyOnceWith(['queued-a']);
 expect(bridge.start).not.toHaveBeenCalled();
});
it('browser daemon cancellation remains per selected job and never becomes a global cancel', async () => {
 const user = userEvent.setup(); show();
 await user.click(screen.getByRole('checkbox', { name: '选择任务 queued-a.zip' }));
 await user.click(screen.getByRole('button', { name: '停止所选' }));
 await user.click(await screen.findByRole('button', { name: '停止任务' }));
 await waitFor(() => expect(bridge.cancel).toHaveBeenCalledExactlyOnceWith('queued-a'));
});
it('an offline browser preview leaves task mutation controls disabled', () => {
 bridge.isDaemonMode.mockReturnValue(false); show();
 expect(screen.getByRole('button', { name: '开始队列' })).toHaveProperty('disabled', true);
 expect(screen.queryByLabelText('服务端归档路径')).toBeNull();
 expect(bridge.startSelected).not.toHaveBeenCalled();
});

it('a paused daemon banner resumes only explicitly selected queued or waiting-resource jobs', async () => {
 const user = userEvent.setup(); show({ ...snapshot, running: true, paused: true });
 const resume = screen.getByRole('button', { name: '横幅继续所选' });
 expect(resume).toHaveProperty('disabled', true);
 await user.click(screen.getByRole('checkbox', { name: '选择任务 waiting-b.zip' }));
 await user.click(screen.getByRole('checkbox', { name: '选择任务 done.zip' }));
 await user.click(resume);
 expect(bridge.startSelected).toHaveBeenCalledExactlyOnceWith(['waiting-b']);
 expect(bridge.start).not.toHaveBeenCalled();
});
it('a running daemon job waiting for a password receives inline input without a vault write or retry', async () => {
 const waiting = { ...job('encrypted', 'running'), stage: 'needsPassword' };
 const user = userEvent.setup(); show({ ...snapshot, jobs: [waiting], running: true });
 await user.click(screen.getByRole('button', { name: '输入密码' }));
 expect(screen.getByRole('checkbox', { name: '同时保存到服务端加密密码库（需要密码库写入权限）' })).toHaveProperty('checked', false);
 await user.type(screen.getByLabelText('即时解压密码'), 'synthetic-inline-password');
 await user.click(screen.getByRole('button', { name: '提交密码' }));
 await waitFor(() => expect(bridge.submitInput).toHaveBeenCalledExactlyOnceWith('encrypted', 'password', 'synthetic-inline-password'));
 expect(bridge.savePassword).not.toHaveBeenCalled(); expect(bridge.retry).not.toHaveBeenCalled();
});

it('a paused daemon with only active jobs resumes through an explicitly selected running anchor', async () => {
 const user = userEvent.setup(); show({ ...snapshot, jobs: [job('running-anchor', 'running'), job('unrelated-active', 'running')], running: true, paused: true });
 const resume = screen.getByRole('button', { name: '继续所选' });
 const bannerResume = screen.getByRole('button', { name: '横幅继续所选' });
 expect(resume).toHaveProperty('disabled', true); expect(bannerResume).toHaveProperty('disabled', true);
 await user.click(screen.getByRole('checkbox', { name: '选择任务 running-anchor.zip' }));
 expect(resume).toHaveProperty('disabled', false); expect(bannerResume).toHaveProperty('disabled', false);
 await user.click(resume);
 expect(bridge.startSelected).toHaveBeenCalledExactlyOnceWith(['running-anchor']);
 expect(bridge.start).not.toHaveBeenCalled();
});
