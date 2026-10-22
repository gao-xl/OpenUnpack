import { act, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { App as AntApp, ConfigProvider } from 'antd';
import zhCN from 'antd/locale/zh_CN';
import { beforeEach, expect, it, vi } from 'vitest';
import ApiSettingsPanel from '../src/components/ApiSettingsPanel';
import SettingsPage from '../src/components/SettingsPage';
import type { ApiClientView, ApiPermission, CreatedApiClient, LocalApiStatus } from '../src/local-api-types';
import { defaultSettings, type AppSnapshot } from '../src/types';

const bridge = vi.hoisted(() => ({
  desktop: true, apiStatus: vi.fn(), apiConfigure: vi.fn(), apiCreateClient: vi.fn(),
  apiRevokeClient: vi.fn(), apiOpenDocs: vi.fn(), pickDirectory: vi.fn(), saveSettings: vi.fn(),
  getDaemonConnection: vi.fn(), connectDaemon: vi.fn(), verifyDaemonConnection: vi.fn(), setDaemonConnection: vi.fn(),
}));
vi.mock('../src/api', () => ({ get isDesktop() { return bridge.desktop; }, api: bridge }));

const permissions: ApiPermission[] = [
  { scope: 'jobs.read', label: '读取任务', sensitive: false },
  { scope: 'jobs.write', label: '管理任务', sensitive: false },
  { scope: 'files.read', label: '读取文件', sensitive: false },
  { scope: 'files.write', label: '写入文件', sensitive: false },
  { scope: 'files.delete', label: '删除文件', sensitive: true },
  { scope: 'vault.read', label: '读取密码列表', sensitive: false },
  { scope: 'vault.write', label: '管理密码', sensitive: false },
  { scope: 'vault.reveal', label: '读取密码明文', sensitive: true },
  { scope: 'settings.read', label: '读取设置', sensitive: false },
  { scope: 'settings.write', label: '修改设置', sensitive: false },
  { scope: 'desktop.interactive', label: '桌面交互', sensitive: true },
];
const client: ApiClientView = { id: 'owned-client', name: '下载助手', scopes: ['jobs.read'], readRoots: [], writeRoots: [], createdAt: '1' };
const status = (overrides: Partial<LocalApiStatus> = {}): LocalApiStatus => ({
  settings: { enabled: false, port: 47980 }, running: false, baseUrl: null, docsUrl: null,
  error: null, clients: [], permissions, ...overrides,
});
const enabledStatus = () => status({ settings: { enabled: true, port: 48123 }, running: true, baseUrl: 'http://127.0.0.1:48123/api/v1', docsUrl: 'http://127.0.0.1:48123/docs' });
const snapshot: AppSnapshot = {
  jobs: [], passwords: [], settings: { ...defaultSettings }, running: false, paused: false,
  vaultError: null, dataDirectory: 'C:\\fixtures', engineAvailable: true, recoverableCount: 0,
  cacheUsedBytes: 0, sessionId: 'api-ui-test', revision: '1',
};
const run = async (work: () => Promise<unknown>) => { await work(); };
function wrap(element: React.ReactNode) {
  return <ConfigProvider locale={zhCN} button={{ autoInsertSpace: false }} theme={{ token: { motion: false } }}><AntApp>{element}</AntApp></ConfigProvider>;
}
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>(done => { resolve = done; });
  return { promise, resolve };
}
async function openDraft(user: ReturnType<typeof userEvent.setup>) {
  await waitFor(() => expect(screen.getByRole('button', { name: '添加应用' })).toHaveProperty('disabled', false));
  await user.click(screen.getByRole('button', { name: '添加应用' }));
  await user.type(screen.getByLabelText('API 应用名称'), ' 下载助手 ');
}

beforeEach(() => {
  vi.restoreAllMocks();
  vi.resetAllMocks();
  bridge.desktop = true;
  bridge.getDaemonConnection.mockReturnValue({ url: null, token: null });
  bridge.connectDaemon.mockResolvedValue(undefined);
  bridge.verifyDaemonConnection.mockImplementation(async (url: string, token: string) => ({ url, token }));
  bridge.apiStatus.mockResolvedValue(status());
  bridge.apiConfigure.mockResolvedValue(enabledStatus());
  bridge.apiOpenDocs.mockResolvedValue(undefined);
  bridge.apiRevokeClient.mockResolvedValue(undefined);
});

it('settings exposes a lazy local API tab and saves its connection independently', async () => {
  const user = userEvent.setup();
  render(wrap(<SettingsPage snapshot={snapshot} refresh={async () => {}} run={run} />));
  expect(bridge.apiStatus).not.toHaveBeenCalled();
  await user.click(screen.getByRole('tab', { name: /本机 API/ }));
  await waitFor(() => expect(screen.getByRole('switch', { name: '启用本机 API' })).toHaveProperty('disabled', false));
  await user.click(screen.getByRole('switch', { name: '启用本机 API' }));
  await user.clear(screen.getByRole('spinbutton', { name: '本机 API 端口' }));
  expect(screen.getByRole('button', { name: '保存连接设置' })).toHaveProperty('disabled', true);
  await user.type(screen.getByRole('spinbutton', { name: '本机 API 端口' }), '48123');
  bridge.apiStatus.mockResolvedValue(enabledStatus());
  await user.click(screen.getByRole('button', { name: '保存连接设置' }));
  await waitFor(() => expect(bridge.apiConfigure).toHaveBeenCalledExactlyOnceWith({ enabled: true, port: 48123 }));
  expect(bridge.saveSettings).not.toHaveBeenCalled();
  expect(await screen.findByText('http://127.0.0.1:48123/api/v1')).toBeTruthy();
  await waitFor(() => expect(screen.getByRole('button', { name: '打开接口文档' })).toHaveProperty('disabled', false));
  await user.click(screen.getByRole('button', { name: '打开接口文档' }));
  expect(bridge.apiOpenDocs).toHaveBeenCalledOnce();
});

it('offers explicit permission presets and retains only chosen picker grants', async () => {
  const user = userEvent.setup();
  bridge.pickDirectory.mockResolvedValueOnce('C:\\fixtures\\read').mockResolvedValueOnce('C:\\fixtures\\read').mockResolvedValueOnce('D:\\output');
  bridge.apiCreateClient.mockResolvedValue({ client, token: 'synthetic-once' });
  render(wrap(<ApiSettingsPanel run={run} />));
  await openDraft(user);
  await user.click(screen.getByRole('button', { name: '解压自动化' }));
  expect(screen.queryByText('包含敏感操作')).toBeNull();
  await user.click(screen.getByRole('button', { name: '全部功能' }));
  expect(screen.getByText('包含敏感操作')).toBeTruthy();
  await user.click(screen.getByRole('button', { name: '只读任务' }));
  expect(screen.queryByText('包含敏感操作')).toBeNull();
  expect(screen.getByRole('checkbox', { name: '读取任务' })).toHaveProperty('checked', true);
  expect(screen.getByRole('checkbox', { name: '读取密码明文' })).toHaveProperty('checked', false);
  await user.click(screen.getByRole('button', { name: '批准可读目录' }));
  await user.click(screen.getByRole('button', { name: '批准可读目录' }));
  expect(screen.getAllByText('可读：C:\\fixtures\\read')).toHaveLength(1);
  await user.click(screen.getByRole('button', { name: '批准可写目录' }));
  const readGrant = screen.getByText('可读：C:\\fixtures\\read').parentElement!;
  await user.click(within(readGrant).getByRole('button', { name: '移除' }));
  await user.click(screen.getByRole('button', { name: '生成密钥' }));
  await waitFor(() => expect(bridge.apiCreateClient).toHaveBeenCalledExactlyOnceWith({ name: '下载助手', scopes: ['jobs.read'], readRoots: [], writeRoots: ['D:\\output'] }));
});

it('validates the backend name byte limit and requires at least one permission', async () => {
  const user = userEvent.setup();
  render(wrap(<ApiSettingsPanel run={run} />));
  await openDraft(user);
  await user.clear(screen.getByLabelText('API 应用名称'));
  await user.type(screen.getByLabelText('API 应用名称'), '名'.repeat(43));
  expect(screen.getByRole('button', { name: '生成密钥' })).toHaveProperty('disabled', true);
  await user.clear(screen.getByLabelText('API 应用名称'));
  await user.type(screen.getByLabelText('API 应用名称'), '合适名称');
  await user.click(screen.getByRole('checkbox', { name: '读取任务' }));
  expect(screen.getByRole('button', { name: '生成密钥' })).toHaveProperty('disabled', true);
  expect(bridge.apiCreateClient).not.toHaveBeenCalled();
});

it('creates one key while pending, clears its modal and never persists it to browser storage', async () => {
  const user = userEvent.setup();
  const pending = deferred<CreatedApiClient>();
  bridge.apiCreateClient.mockReturnValueOnce(pending.promise);
  const persisted = vi.spyOn(Storage.prototype, 'setItem');
  render(wrap(<ApiSettingsPanel run={run} />));
  await openDraft(user);
  const generate = screen.getByRole('button', { name: '生成密钥' });
  await user.dblClick(generate);
  expect(bridge.apiCreateClient).toHaveBeenCalledOnce();
  expect(generate).toHaveProperty('disabled', true);
  bridge.apiStatus.mockResolvedValue(status({ clients: [client] }));
  await act(async () => { pending.resolve({ client, token: 'synthetic-once-only-key' }); await pending.promise; });
  expect(await screen.findByLabelText('新 API 应用密钥')).toHaveProperty('value', 'synthetic-once-only-key');
  expect(screen.getByLabelText('新 API 应用密钥')).toHaveProperty('type', 'password');
  expect(screen.queryByText('synthetic-once-only-key')).toBeNull();
  await user.click(screen.getByRole('button', { name: '已保存，关闭' }));
  await waitFor(() => expect(screen.queryByLabelText('新 API 应用密钥')).toBeNull());
  await user.click(screen.getByRole('button', { name: '添加应用' }));
  expect(screen.getByLabelText('API 应用名称')).toHaveProperty('value', '');
  expect(screen.queryByDisplayValue('synthetic-once-only-key')).toBeNull();
  expect(persisted).not.toHaveBeenCalled();
});

it('shows creation failure in its dialog and preserves the draft without an unhandled rejection', async () => {
  const user = userEvent.setup();
  bridge.apiCreateClient.mockRejectedValueOnce(new Error('授权保存失败，原配置已保留。'));
  render(wrap(<ApiSettingsPanel run={run} />));
  await openDraft(user);
  await user.click(screen.getByRole('button', { name: '生成密钥' }));
  expect(await within(screen.getByRole('dialog')).findByText('授权保存失败，原配置已保留。')).toBeTruthy();
  expect(screen.getByLabelText('API 应用名称')).toHaveProperty('value', ' 下载助手 ');
  expect(screen.queryByLabelText('新 API 应用密钥')).toBeNull();
  await waitFor(() => expect(screen.getByRole('button', { name: '生成密钥' })).toHaveProperty('disabled', false));
});

it('does not let an older status request overwrite the newly saved connection', async () => {
  const user = userEvent.setup();
  const oldRefresh = deferred<LocalApiStatus>();
  bridge.apiStatus.mockResolvedValueOnce(status()).mockReturnValueOnce(oldRefresh.promise).mockResolvedValue(enabledStatus());
  render(wrap(<ApiSettingsPanel run={run} />));
  await waitFor(() => expect(screen.getByRole('button', { name: '保存连接设置' })).toHaveProperty('disabled', false));
  await user.click(screen.getByRole('button', { name: '刷新连接状态' }));
  await user.click(screen.getByRole('switch', { name: '启用本机 API' }));
  await user.clear(screen.getByRole('spinbutton', { name: '本机 API 端口' }));
  await user.type(screen.getByRole('spinbutton', { name: '本机 API 端口' }), '48123');
  await user.click(screen.getByRole('button', { name: '保存连接设置' }));
  expect(await screen.findByText('运行中')).toBeTruthy();
  await act(async () => { oldRefresh.resolve(status()); await oldRefresh.promise; });
  expect(screen.getByText('运行中')).toBeTruthy();
  expect(screen.getByText('http://127.0.0.1:48123/api/v1')).toBeTruthy();
});

it('revokes only after confirmation and refreshes the client list', async () => {
  const user = userEvent.setup();
  bridge.apiStatus.mockResolvedValue(status({ clients: [client] }));
  render(wrap(<ApiSettingsPanel run={run} />));
  await user.click(await screen.findByRole('button', { name: '撤销授权' }));
  expect(bridge.apiRevokeClient).not.toHaveBeenCalled();
  await user.click(screen.getByRole('button', { name: '取消' }));
  expect(bridge.apiRevokeClient).not.toHaveBeenCalled();
  await user.click(screen.getByRole('button', { name: '撤销授权' }));
  bridge.apiStatus.mockResolvedValue(status());
  await user.click(screen.getByRole('button', { name: '确定' }));
  await waitFor(() => expect(bridge.apiRevokeClient).toHaveBeenCalledExactlyOnceWith(client.id));
  await waitFor(() => expect(screen.queryByRole('button', { name: '撤销授权' })).toBeNull());
});

it('drops a late create response after unmount and never shows it in a new panel', async () => {
  const user = userEvent.setup();
  const pending = deferred<CreatedApiClient>();
  bridge.apiCreateClient.mockReturnValueOnce(pending.promise);
  const view = render(wrap(<ApiSettingsPanel run={run} />));
  await openDraft(user);
  await user.click(screen.getByRole('button', { name: '生成密钥' }));
  view.unmount();
  render(wrap(<ApiSettingsPanel run={run} />));
  await act(async () => { pending.resolve({ client, token: 'synthetic-late-key' }); await pending.promise; });
  expect(screen.queryByLabelText('新 API 应用密钥')).toBeNull();
  expect(screen.queryByDisplayValue('synthetic-late-key')).toBeNull();
});

it('clears a one-time key when leaving the API tab and does not reveal it on return', async () => {
  const user = userEvent.setup();
  bridge.apiCreateClient.mockResolvedValue({ client, token: 'synthetic-tab-key' });
  render(wrap(<SettingsPage snapshot={snapshot} refresh={async () => {}} run={run} />));
  await user.click(screen.getByRole('tab', { name: /本机 API/ }));
  await openDraft(user);
  await user.click(screen.getByRole('button', { name: '生成密钥' }));
  await screen.findByLabelText('新 API 应用密钥');
  // Tab changes can also be driven programmatically while the modal is open.
  act(() => { screen.getByRole('tab', { name: /输出管理/ }).click(); });
  await waitFor(() => expect(screen.queryByLabelText('新 API 应用密钥')).toBeNull());
  await user.click(screen.getByRole('tab', { name: /本机 API/ }));
  expect(screen.queryByDisplayValue('synthetic-tab-key')).toBeNull();
});

it('allows status retry after failure and keeps browser previews read-only', async () => {
  const user = userEvent.setup();
  bridge.apiStatus.mockRejectedValueOnce(new Error('读取连接失败。')).mockResolvedValue(status());
  const view = render(wrap(<ApiSettingsPanel run={run} />));
  expect(await screen.findByText('读取连接失败。')).toBeTruthy();
  expect(screen.getByRole('switch', { name: '启用本机 API' })).toHaveProperty('disabled', true);
  await user.click(screen.getByRole('button', { name: '刷新连接状态' }));
  await waitFor(() => expect(screen.getByRole('button', { name: '添加应用' })).toHaveProperty('disabled', false));
  view.unmount();
  bridge.desktop = false;
  bridge.apiStatus.mockClear();
  render(wrap(<ApiSettingsPanel run={run} />));
  expect(bridge.apiStatus).not.toHaveBeenCalled();
  for (const name of ['添加应用', '保存连接设置', '打开接口文档', '刷新连接状态'])
    expect(screen.getByRole('button', { name })).toHaveProperty('disabled', true);
});


it('does not announce a daemon connection before authentication succeeds and reports a failed attempt', async () => {
 const user = userEvent.setup(); bridge.verifyDaemonConnection.mockRejectedValueOnce(new Error('synthetic authentication failed'));
 render(wrap(<ApiSettingsPanel run={run} />));
 await user.type(screen.getByLabelText('守护进程地址'), 'http://127.0.0.1:47980');
 await user.type(screen.getByLabelText('API 密钥 (Token)'), 'synthetic-key');
 await user.click(screen.getByRole('button', { name: '连接守护进程' }));
 expect(await screen.findByText('synthetic authentication failed')).toBeTruthy();
 expect(screen.getByText('未连接')).toBeTruthy(); expect(screen.queryByText('已连接')).toBeNull();
 expect(bridge.verifyDaemonConnection).toHaveBeenCalledExactlyOnceWith('http://127.0.0.1:47980', 'synthetic-key');
 expect(bridge.setDaemonConnection).not.toHaveBeenCalled();
});
it('editing the daemon endpoint clears the previous credential instead of forwarding it to the new endpoint', async () => {
 const user = userEvent.setup(); bridge.getDaemonConnection.mockReturnValue({ url: 'http://127.0.0.1:47980', token: 'previous-synthetic-key' });
 render(wrap(<ApiSettingsPanel run={run} />));
 expect(screen.getByLabelText('API 密钥 (Token)')).toHaveProperty('value', 'previous-synthetic-key');
 await user.clear(screen.getByLabelText('守护进程地址'));
 await user.type(screen.getByLabelText('守护进程地址'), 'http://localhost:47981');
 expect(screen.getByLabelText('API 密钥 (Token)')).toHaveProperty('value', '');
 expect(bridge.connectDaemon).not.toHaveBeenCalled();
});
it('a pending daemon verification is single-flight and a late success after unmount does not activate a new panel', async () => {
 const user = userEvent.setup(); const pending = deferred<{ url: string; token: string }>(); bridge.verifyDaemonConnection.mockReturnValueOnce(pending.promise);
 const first = render(wrap(<ApiSettingsPanel run={run} />));
 await user.type(screen.getByLabelText('守护进程地址'), 'http://127.0.0.1:47980');
 await user.type(screen.getByLabelText('API 密钥 (Token)'), 'synthetic-key');
 await user.dblClick(screen.getByRole('button', { name: '连接守护进程' }));
 expect(bridge.verifyDaemonConnection).toHaveBeenCalledTimes(1); first.unmount();
 await act(async () => { pending.resolve({ url: 'http://127.0.0.1:47980', token: 'synthetic-key' }); });
 expect(bridge.setDaemonConnection).not.toHaveBeenCalled();
 render(wrap(<ApiSettingsPanel run={run} />));
 expect(screen.getByText('未连接')).toBeTruthy(); expect(screen.queryByText('已连接')).toBeNull();
});
