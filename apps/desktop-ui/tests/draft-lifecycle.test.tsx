import { act, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { App as AntApp, ConfigProvider } from 'antd';
import zhCN from 'antd/locale/zh_CN';
import { beforeEach, expect, it, vi } from 'vitest';
import QueuePage from '../src/components/QueuePage';
import VaultPage from '../src/components/VaultPage';
import { JobDetails } from '../src/components/shared';
import { defaultSettings, type AppSnapshot, type JobView } from '../src/types';

const bridge = vi.hoisted(() => ({
  onDrop: vi.fn(), savePassword: vi.fn(), retry: vi.fn(), confirmImport: vi.fn(),
  previewImport: vi.fn(), cancelInspectArchive: vi.fn(), inspectArchive: vi.fn(),
}));
vi.mock('../src/api', () => ({ isDesktop: true, api: bridge }));

const job: JobView = {
  id: 'old-job', source: { path: 'C:\\fixtures\\old.zip', displayName: 'old.zip', volumes: [], size: 1, missingVolume: false },
  options: { ...defaultSettings }, status: 'needsPassword', stage: 'probing', progress: 0, message: '', attempts: 1,
  outputPath: null, verificationScope: '', createdAt: '', finishedAt: null, workspace: null, nodes: [], stageTimes: {}, transferProgress: null,
};
const snapshot: AppSnapshot = {
  jobs: [job], passwords: [], settings: { ...defaultSettings }, running: false, paused: false, vaultError: null,
  dataDirectory: 'C:\\fixtures', engineAvailable: true, recoverableCount: 0, cacheUsedBytes: 0, sessionId: 'drafts', revision: '1',
};
const run = async (work: () => Promise<unknown>) => { await work(); };
const refresh = async () => {};
function wrap(element: React.ReactNode) {
  return <ConfigProvider locale={zhCN} button={{ autoInsertSpace: false }} theme={{ token: { motion: false } }}><AntApp>{element}</AntApp></ConfigProvider>;
}
function deferred() {
  let resolve!: () => void;
  const promise = new Promise<void>(done => { resolve = done; });
  return { promise, resolve };
}
beforeEach(() => {
  vi.resetAllMocks();
  bridge.onDrop.mockResolvedValue(() => {});
  bridge.retry.mockResolvedValue(undefined);
  bridge.cancelInspectArchive.mockResolvedValue(undefined);
});

it('queue password draft closes when its owning task disappears', async () => {
  const user = userEvent.setup();
  const view = render(wrap(<QueuePage snapshot={snapshot} run={run} refresh={refresh} />));
  await user.click(screen.getByRole('button', { name: '输入密码' }));
  await user.type(screen.getByLabelText('即时解压密码'), 'old-secret');
  view.rerender(wrap(<QueuePage snapshot={{ ...snapshot, jobs: [] }} run={run} refresh={refresh} />));
  await waitFor(() => expect(screen.queryByLabelText('即时解压密码')).toBeNull());
});

it('an old queue password save cannot close a newer task draft or retry after cancellation', async () => {
  const user = userEvent.setup();
  const pending = deferred();
  bridge.savePassword.mockReturnValueOnce(pending.promise);
  const second = { ...job, id: 'new-job', source: { ...job.source, displayName: 'new.zip', path: 'C:\\fixtures\\new.zip' } };
  render(wrap(<QueuePage snapshot={{ ...snapshot, jobs: [job, second] }} run={run} refresh={refresh} />));
  await user.click(screen.getAllByRole('button', { name: '输入密码' })[0]);
  await user.type(screen.getByLabelText('即时解压密码'), 'old-secret');
  await user.click(screen.getByRole('button', { name: '解密并重试' }));
  await waitFor(() => expect(bridge.savePassword).toHaveBeenCalledOnce());
  await user.click(within(screen.getByRole('dialog')).getByRole('button', { name: '取消' }));
  await user.click(screen.getAllByRole('button', { name: '输入密码' })[1]);
  await user.type(screen.getByLabelText('即时解压密码'), 'new-secret');
  await act(async () => { pending.resolve(); await pending.promise; });
  expect(screen.getByLabelText('即时解压密码')).toHaveProperty('value', 'new-secret');
  expect(bridge.retry).not.toHaveBeenCalled();
});

it('task details discard the password draft when switching tasks', async () => {
  const user = userEvent.setup();
  const view = render(wrap(<JobDetails job={job} close={() => {}} run={run} />));
  await user.click(screen.getByRole('button', { name: '输入密码并重试' }));
  await user.type(screen.getByLabelText('即时解压密码'), 'old-secret');
  view.rerender(wrap(<JobDetails job={{ ...job, id: 'new-job' }} close={() => {}} run={run} />));
  await waitFor(() => expect(screen.queryByLabelText('即时解压密码')).toBeNull());
});

it('closing task details during a save prevents a late retry and preserves the next draft', async () => {
  const user = userEvent.setup();
  const pending = deferred();
  bridge.savePassword.mockReturnValueOnce(pending.promise);
  const view = render(wrap(<JobDetails job={job} close={() => {}} run={run} />));
  await user.click(screen.getByRole('button', { name: '输入密码并重试' }));
  await user.type(screen.getByLabelText('即时解压密码'), 'old-secret');
  await user.click(screen.getByRole('button', { name: '解密并重试' }));
  await waitFor(() => expect(bridge.savePassword).toHaveBeenCalledOnce());
  view.rerender(wrap(<JobDetails job={undefined} close={() => {}} run={run} />));
  view.rerender(wrap(<JobDetails job={{ ...job, id: 'new-task' }} close={() => {}} run={run} />));
  await user.click(screen.getByRole('button', { name: '输入密码并重试' }));
  await user.type(screen.getByLabelText('即时解压密码'), 'new-secret');
  await act(async () => { pending.resolve(); await pending.promise; });
  expect(bridge.retry).not.toHaveBeenCalled();
  expect(screen.getByLabelText('即时解压密码')).toHaveProperty('value', 'new-secret');
});

it('an old vault save cannot erase a newly opened password draft', async () => {
  const user = userEvent.setup();
  const pending = deferred();
  bridge.savePassword.mockReturnValueOnce(pending.promise);
  render(wrap(<VaultPage snapshot={snapshot} run={run} refresh={refresh} />));
  await user.click(screen.getByRole('button', { name: /添加密码/ }));
  await user.type(screen.getByLabelText('名称 / 标识'), 'old-name');
  await user.type(screen.getByLabelText('密码明文'), 'old-secret');
  await user.click(screen.getByRole('button', { name: '保存' }));
  await waitFor(() => expect(bridge.savePassword).toHaveBeenCalledOnce());
  await user.click(within(screen.getByRole('dialog')).getByRole('button', { name: '取消' }));
  await user.click(screen.getByRole('button', { name: /添加密码/ }));
  await user.type(screen.getByLabelText('名称 / 标识'), 'new-name');
  await user.type(screen.getByLabelText('密码明文'), 'new-secret');
  await act(async () => { pending.resolve(); await pending.promise; });
  expect(screen.getByLabelText('名称 / 标识')).toHaveProperty('value', 'new-name');
  expect(screen.getByLabelText('密码明文')).toHaveProperty('value', 'new-secret');
});

it('editing import text during confirmation preserves the newer draft', async () => {
  const user = userEvent.setup();
  const pending = deferred();
  bridge.confirmImport.mockReturnValueOnce(pending.promise);
  bridge.previewImport.mockResolvedValue({ token: 'old-token', newCount: 1, duplicateCount: 0, errorCount: 0, rows: [] });
  render(wrap(<VaultPage snapshot={snapshot} run={run} refresh={refresh} />));
  await user.click(screen.getByRole('button', { name: /批量导入/ }));
  await user.type(screen.getByLabelText('待导入密码文本'), 'old-secret');
  await user.click(screen.getByRole('button', { name: '生成导入预览' }));
  await user.click(await screen.findByRole('button', { name: '确认导入 (1 新增)' }));
  await waitFor(() => expect(bridge.confirmImport).toHaveBeenCalledOnce());
  await user.clear(screen.getByLabelText('待导入密码文本'));
  await user.type(screen.getByLabelText('待导入密码文本'), 'new-secret');
  await act(async () => { pending.resolve(); await pending.promise; });
  expect(screen.getByLabelText('待导入密码文本')).toHaveProperty('value', 'new-secret');
});
