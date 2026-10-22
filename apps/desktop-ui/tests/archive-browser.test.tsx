import { beforeEach, expect, it, vi } from 'vitest';
import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { App as AntApp, ConfigProvider } from 'antd';
import ArchiveBrowser from '../src/components/ArchiveBrowser';
import { JobDetails } from '../src/components/shared';
import { defaultSettings, type JobView } from '../src/types';

const bridge = vi.hoisted(() => ({ inspectArchive: vi.fn(), cancelInspectArchive: vi.fn(), savePassword: vi.fn(), retry: vi.fn(), addJobs: vi.fn(), pickDirectory: vi.fn(), extractSelective: vi.fn(), openPath: vi.fn(), isDaemonMode: vi.fn(), getConnectionVersion: vi.fn() }));
vi.mock('../src/api', () => ({ isDesktop: true, api: bridge }));

const job: JobView = {
  id: 'browse-fixture', source: { path: 'C:\\fixtures\\synthetic.zip', displayName: 'synthetic.zip', volumes: [], size: 1, missingVolume: false },
  options: { ...defaultSettings }, status: 'completed', stage: 'finished', progress: 1, message: '', attempts: 1, outputPath: null,
  verificationScope: '', createdAt: '', finishedAt: '', workspace: null, stageTimes: {}, transferProgress: null, nodes: [],
};

const inspection = {
  archivePath: 'C:\\fixtures\\synthetic.zip', totalEntries: 3, totalSize: 5 * 1024 * 1024, totalSizeKnown: true, encryptedEntries: 1, detectedEncoding: '',
  entries: [
    { path: 'folder', size: 0, sizeKnown: true, isDirectory: true, isEncrypted: false, rawPath: '' },
    { path: 'folder/a.txt', size: 1024 * 1024, sizeKnown: true, isDirectory: false, isEncrypted: false, rawPath: '' },
    { path: 'folder/b.txt', size: 4 * 1024 * 1024, sizeKnown: true, isDirectory: false, isEncrypted: true, rawPath: '' },
  ],
};

function wrap(element: React.ReactNode) {
  return <ConfigProvider button={{ autoInsertSpace: false }} theme={{ token: { motion: false } }}><AntApp>{element}</AntApp></ConfigProvider>;
}

beforeEach(() => {
  vi.resetAllMocks();
  bridge.cancelInspectArchive.mockResolvedValue(undefined);
  bridge.openPath.mockResolvedValue(undefined);
  bridge.isDaemonMode.mockReturnValue(false);
  bridge.getConnectionVersion.mockReturnValue(0);
});

it('renders the archive tree with totals and encrypted markers', async () => {
  bridge.inspectArchive.mockResolvedValue(inspection);
  render(wrap(<ArchiveBrowser job={job} close={() => {}} />));
  await waitFor(() => expect(bridge.inspectArchive).toHaveBeenCalledWith('C:\\fixtures\\synthetic.zip', null, expect.any(String)));
  expect(await screen.findByText('3 项')).toBeTruthy();
  expect(screen.getByText('共 5.0 MiB')).toBeTruthy();
  expect(screen.getByText('加密 1 项')).toBeTruthy();
  expect(screen.getByText('a.txt')).toBeTruthy();
  expect(screen.getByText('b.txt')).toBeTruthy();
  expect(screen.getByText('加密')).toBeTruthy();
});

it('search filters entries and keeps folder context', async () => {
  const user = userEvent.setup();
  bridge.inspectArchive.mockResolvedValue(inspection);
  render(wrap(<ArchiveBrowser job={job} close={() => {}} />));
  await screen.findByText('a.txt');
  await user.type(screen.getByRole('textbox', { name: '搜索归档条目' }), 'b.txt');
  await waitFor(() => expect(screen.queryByText('a.txt')).toBeNull());
  expect(screen.getByText('b.txt')).toBeTruthy();
  expect(screen.getByText('folder')).toBeTruthy();
  expect(screen.getByText('命中 1 / 3 项')).toBeTruthy();
});

it('select-all under a search sends only the matched entries and never their parent folder', async () => {
  const user = userEvent.setup();
  bridge.inspectArchive.mockResolvedValue(inspection);
  bridge.extractSelective.mockResolvedValue({ outputDirectory: 'C:////selected', extractedFiles: ['folder/a.txt'], totalBytes: 1024 });
  render(wrap(<ArchiveBrowser job={job} close={() => {}} />));
  await screen.findByText('a.txt');
  await user.type(screen.getByRole('textbox', { name: '搜索归档条目' }), 'a.txt');
  await waitFor(() => expect(screen.queryByText('b.txt')).toBeNull());
  await user.click(screen.getByRole('button', { name: '全选命中项' }));
  await user.click(screen.getByRole('button', { name: /提取所选条目/ }));
  await waitFor(() => expect(bridge.extractSelective).toHaveBeenCalledExactlyOnceWith(
    expect.objectContaining({ entryPaths: ['folder/a.txt'] }),
  ));
});

it('password-protected archives browse and extract with the entered password without saving it', async () => {
  const user = userEvent.setup();
  bridge.inspectArchive.mockRejectedValueOnce(new Error('NeedsPassword')).mockResolvedValueOnce(inspection);
  render(wrap(<ArchiveBrowser job={job} close={() => {}} />));
  expect(await screen.findByText('此归档需要密码才能列出内容')).toBeTruthy();
  await user.type(screen.getByLabelText('归档密码'), 'synthetic-secret');
  await user.click(screen.getByRole('button', { name: '使用该密码浏览' }));
  await screen.findByText('a.txt');
  expect(bridge.inspectArchive).toHaveBeenLastCalledWith('C:\\fixtures\\synthetic.zip', 'synthetic-secret', expect.any(String));
  bridge.extractSelective.mockResolvedValue({ outputDirectory: 'C:\\selected', extractedFiles: ['folder/a.txt'], totalBytes: 1024 });
  await user.click(screen.getByRole('button', { name: '全选' }));
  await user.click(screen.getByRole('button', { name: /提取所选条目/ }));
  await waitFor(() => expect(bridge.extractSelective).toHaveBeenCalledExactlyOnceWith(expect.objectContaining({ password: 'synthetic-secret' })));
  expect(bridge.savePassword).not.toHaveBeenCalled();
});

it('hard failures surface an actionable error without a tree', async () => {
  bridge.inspectArchive.mockRejectedValue(new Error('源压缩包不存在或不可读。'));
  render(wrap(<ArchiveBrowser job={job} close={() => {}} />));
  expect(await screen.findByText('无法读取归档内容')).toBeTruthy();
  expect(screen.getByText('源压缩包不存在或不可读。')).toBeTruthy();
  expect(screen.queryByText('a.txt')).toBeNull();
});

it('stale slow inspection response does not overwrite newer job inspection', async () => {
  let resolveSlow!: (val: typeof inspection) => void;
  const slowPromise = new Promise<typeof inspection>(resolve => { resolveSlow = resolve; });
  const secondInspection = {
    ...inspection,
    archivePath: 'C:\\fixtures\\second.zip',
    entries: [{ path: 'second.txt', size: 100, sizeKnown: true, isDirectory: false, isEncrypted: false, rawPath: '' }],
  };
  bridge.inspectArchive
    .mockImplementationOnce(() => slowPromise)
    .mockResolvedValueOnce(secondInspection);

  const { rerender } = render(wrap(<ArchiveBrowser job={job} close={() => {}} />));
  await waitFor(() => expect(bridge.inspectArchive).toHaveBeenCalledOnce());
  const secondJob: JobView = { ...job, id: 'second-job', source: { ...job.source, path: 'C:\\fixtures\\second.zip', displayName: 'second.zip' } };
  rerender(wrap(<ArchiveBrowser job={secondJob} close={() => {}} />));

  expect(await screen.findByText('second.txt')).toBeTruthy();
  // Now slow promise resolves from first job
  resolveSlow(inspection);
  await new Promise(r => setTimeout(r, 50));
  // Must still display second.txt and not revert to a.txt
  expect(screen.getByText('second.txt')).toBeTruthy();
  expect(screen.queryByText('a.txt')).toBeNull();
});

it('JobDetails exposes the browse entry and opens the browser', async () => {
  const user = userEvent.setup();
  bridge.inspectArchive.mockResolvedValue(inspection);
  render(wrap(<JobDetails job={job} close={() => {}} run={async () => {}} />));
  await user.click(screen.getByRole('button', { name: '浏览归档内容' }));
  await waitFor(() => expect(bridge.inspectArchive).toHaveBeenCalledWith('C:\\fixtures\\synthetic.zip', null, expect.any(String)));
  expect(await screen.findByText('a.txt')).toBeTruthy();
});

it('renders detected encoding tag and shows original filename when fixed', async () => {
  const gbkInspection = {
    archivePath: 'C:\\fixtures\\gbk.zip',
    totalEntries: 1,
    totalSize: 1024,
    totalSizeKnown: true,
    encryptedEntries: 0,
    detectedEncoding: 'GBK',
    entries: [
      { path: '说明文档.txt', size: 1024, sizeKnown: true, isDirectory: false, isEncrypted: false, rawPath: '˵ĵ.txt' },
    ],
  };
  bridge.inspectArchive.mockResolvedValue(gbkInspection);
  render(wrap(<ArchiveBrowser job={job} close={() => {}} />));
  expect(await screen.findByText('编码: GBK (自动修正)')).toBeTruthy();
  expect(screen.getByText('说明文档.txt')).toBeTruthy();
  expect(screen.getByText('(原: ˵ĵ.txt)')).toBeTruthy();
});

it('a late cancellation targets only the old inspection ID after switching tasks', async () => {
  let resolveCancel!: () => void;
  const pendingCancel = new Promise<void>(resolve => { resolveCancel = resolve; });
  bridge.cancelInspectArchive.mockReturnValueOnce(pendingCancel);
  bridge.inspectArchive.mockReturnValueOnce(new Promise(() => {})).mockResolvedValueOnce(inspection);
  const view = render(wrap(<ArchiveBrowser job={job} close={() => {}} />));
  await waitFor(() => expect(bridge.inspectArchive).toHaveBeenCalledOnce());
  const firstId = bridge.inspectArchive.mock.calls[0][2];
  view.rerender(wrap(<ArchiveBrowser job={{ ...job, id: 'second-job', source: { ...job.source, path: 'C:\\fixtures\\second.zip' } }} close={() => {}} />));
  await screen.findByText('a.txt');
  const secondId = bridge.inspectArchive.mock.calls[1][2];
  expect(firstId).not.toEqual(secondId);
  expect(bridge.cancelInspectArchive).toHaveBeenCalledExactlyOnceWith(firstId);
  await act(async () => { resolveCancel(); await pendingCancel; });
  expect(screen.getByText('a.txt')).toBeTruthy();
  expect(bridge.cancelInspectArchive).not.toHaveBeenCalledWith(secondId);
});

it('closing an active inspection cancels its ID and handles cancellation failure safely', async () => {
  const user = userEvent.setup();
  const close = vi.fn();
  bridge.inspectArchive.mockReturnValue(new Promise(() => {}));
  bridge.cancelInspectArchive.mockRejectedValue(new Error('cancel failed'));
  render(wrap(<ArchiveBrowser job={job} close={close} />));
  await waitFor(() => expect(bridge.inspectArchive).toHaveBeenCalledOnce());
  const inspectId = bridge.inspectArchive.mock.calls[0][2];
  await user.click(screen.getByRole('button', { name: '关闭' }));
  expect(bridge.cancelInspectArchive).toHaveBeenCalledWith(inspectId);
  expect(close).toHaveBeenCalledOnce();
  expect(await screen.findByText('停止归档检查未确认，请稍后核对后台状态。')).toBeTruthy();
});

it('unknown expanded sizes are displayed as unknown rather than zero bytes', async () => {
  bridge.inspectArchive.mockResolvedValue({
    ...inspection, totalSize: 0, totalSizeKnown: false,
    entries: [{ path: 'file.txt', size: 0, sizeKnown: false, isDirectory: false, isEncrypted: false, rawPath: '' }],
  });
  render(wrap(<ArchiveBrowser job={job} close={() => {}} />));
  expect(await screen.findByText('总展开大小未知')).toBeTruthy();
  expect(screen.getByText('大小未知')).toBeTruthy();
  expect(screen.queryByText('0 B')).toBeNull();
});

it('deep archive paths retain the final filename when rendering depth is bounded', async () => {
  const parts = [...Array.from({ length: 40 }, (_, index) => `directory${index}`), 'actual-file.txt'];
  bridge.inspectArchive.mockResolvedValue({
    ...inspection, entries: [{ path: parts.join('/'), size: 1, sizeKnown: true, isDirectory: false, isEncrypted: false, rawPath: '' }],
  });
  render(wrap(<ArchiveBrowser job={job} close={() => {}} />));
  await screen.findByText('directory0');
  const viewport = screen.getByRole('tree').querySelector('.ant-tree-list-holder');
  expect(viewport).toBeTruthy();
  fireEvent.scroll(viewport!, { target: { scrollTop: 600 } });
  expect(await screen.findByText(parts.slice(31).join('/'))).toBeTruthy();
});

it('extracts only the selected entries and opens the returned native output directory', async () => {
  const user = userEvent.setup();
  const close = vi.fn();
  bridge.inspectArchive.mockResolvedValue(inspection);
  const outputDirectory = 'C:\\approved-output\\selected';
  bridge.extractSelective.mockResolvedValue({ outputDirectory, extractedFiles: ['folder/a.txt', 'folder/b.txt'], totalBytes: 5 * 1024 * 1024 });
  render(wrap(<ArchiveBrowser job={{ ...job, options: { ...job.options, outputRoot: 'C:\\approved-output' } }} close={close} />));
  await screen.findByText('a.txt');

  await user.click(screen.getByRole('button', { name: '全选' }));
  expect(await screen.findByText(/已选 3 项/)).toBeTruthy();

  const extractBtn = screen.getByRole('button', { name: /提取所选条目/ });
  expect(extractBtn).toBeTruthy();
  await user.click(extractBtn);

  await waitFor(() => expect(bridge.extractSelective).toHaveBeenCalledExactlyOnceWith({
    archivePath: job.source.path,
    entryPaths: expect.arrayContaining(['folder', 'folder/a.txt', 'folder/b.txt']),
    destinationDir: 'C:\\approved-output', password: undefined, flatten: false,
  }));
  expect(bridge.extractSelective.mock.calls[0][0].entryPaths).toHaveLength(3);
  expect(await screen.findByText(`已成功提取 2 个文件至 ${outputDirectory}`)).toBeTruthy();
  expect(bridge.openPath).toHaveBeenCalledExactlyOnceWith(outputDirectory);
  expect(bridge.addJobs).not.toHaveBeenCalled();
  expect(bridge.savePassword).not.toHaveBeenCalled();
  expect(close).toHaveBeenCalledOnce();
});

it('uses the directory explicitly selected for extraction', async () => {
  const user = userEvent.setup();
  const close = vi.fn();
  bridge.inspectArchive.mockResolvedValue(inspection);
  bridge.pickDirectory.mockResolvedValue('C:\\chosen-output');
  bridge.extractSelective.mockResolvedValue({ outputDirectory: 'C:\\chosen-output\\selected', extractedFiles: ['folder/a.txt'], totalBytes: 1024 });
  render(wrap(<ArchiveBrowser job={job} close={close} />));
  await screen.findByText('a.txt');
  await user.click(screen.getByRole('button', { name: '全选' }));
  await user.click(screen.getByRole('button', { name: '提取所选至...' }));
  await waitFor(() => expect(close).toHaveBeenCalledOnce());
  expect(bridge.pickDirectory).toHaveBeenCalledOnce();
  expect(bridge.extractSelective).toHaveBeenCalledExactlyOnceWith(expect.objectContaining({ destinationDir: 'C:\\chosen-output' }));
  expect(bridge.addJobs).not.toHaveBeenCalled();
});

it('cancelling directory selection preserves the selected entries and submits no extraction', async () => {
  const user = userEvent.setup();
  const close = vi.fn();
  bridge.inspectArchive.mockResolvedValue(inspection);
  bridge.pickDirectory.mockResolvedValue(null);
  render(wrap(<ArchiveBrowser job={job} close={close} />));
  await screen.findByText('a.txt');
  await user.click(screen.getByRole('button', { name: '全选' }));
  await user.click(screen.getByRole('button', { name: '提取所选至...' }));
  await waitFor(() => expect(bridge.pickDirectory).toHaveBeenCalledOnce());
  expect(screen.getByText('已选 3 项')).toBeTruthy();
  await user.click(screen.getByRole('button', { name: '提取所选至...' }));
  expect(bridge.pickDirectory).toHaveBeenCalledTimes(2);
  expect(bridge.extractSelective).not.toHaveBeenCalled();
  expect(bridge.addJobs).not.toHaveBeenCalled();
  expect(bridge.openPath).not.toHaveBeenCalled();
  expect(close).not.toHaveBeenCalled();
});

it('shows extraction failures without closing or opening output and allows an explicit retry', async () => {
  const user = userEvent.setup();
  const close = vi.fn();
  bridge.inspectArchive.mockResolvedValue(inspection);
  bridge.pickDirectory.mockResolvedValue('C:\\retry-target');
  bridge.extractSelective.mockRejectedValueOnce(new Error('目标目录没有写权限。'));
  render(wrap(<ArchiveBrowser job={job} close={close} />));
  await screen.findByText('a.txt');
  await user.click(screen.getByRole('button', { name: '全选' }));
  await user.click(screen.getByRole('button', { name: '提取所选至...' }));
  expect(await screen.findByText('目标目录没有写权限。')).toBeTruthy();
  expect(screen.getByText('已选 3 项')).toBeTruthy();
  expect(close).not.toHaveBeenCalled();
  expect(bridge.openPath).not.toHaveBeenCalled();
  expect(bridge.addJobs).not.toHaveBeenCalled();
  bridge.extractSelective.mockResolvedValueOnce({ outputDirectory: 'C:\\selected', extractedFiles: ['folder/a.txt'], totalBytes: 1024 });
  await user.click(screen.getByRole('button', { name: /提取所选条目/ }));
  await waitFor(() => expect(close).toHaveBeenCalledOnce());
  expect(bridge.extractSelective).toHaveBeenCalledTimes(2);
  expect(bridge.extractSelective).toHaveBeenNthCalledWith(2, expect.objectContaining({ destinationDir: 'C:\\retry-target' }));
});

it.each([
  ['NeedsPassword: archive requires a password', '提取所选条目需要密码'],
  ['PasswordMismatchOrCorrupt: extraction could not verify the archive', '密码不匹配或归档已损坏'],
])('handles %s by retrying the same selection and destination with a temporary password', async (error, title) => {
  const user = userEvent.setup();
  const close = vi.fn();
  bridge.inspectArchive.mockResolvedValue(inspection);
  bridge.pickDirectory.mockResolvedValue('C:\\chosen-password-output');
  bridge.extractSelective.mockRejectedValueOnce(new Error(error));
  render(wrap(<ArchiveBrowser job={job} close={close} />));
  await screen.findByText('a.txt');
  await user.click(screen.getByRole('button', { name: '全选' }));
  await user.click(screen.getByRole('button', { name: '提取所选至...' }));
  expect(await screen.findByText(title)).toBeTruthy();
  expect(screen.getByText('已选 3 项')).toBeTruthy();
  expect(screen.getByText('输入密码后重试提取；若仍失败，请检查密码或归档完整性。密码仅用于本次提取，不会保存到密码库。')).toBeTruthy();
  expect(close).not.toHaveBeenCalled();
  expect(bridge.openPath).not.toHaveBeenCalled();
  const temporaryPassword = ' 临时密码 ';
  await user.type(screen.getByLabelText('所选条目的解压密码'), temporaryPassword);
  expect(screen.queryByRole('button', { name: '应用此密码并重试解压' })).toBeNull();
  bridge.extractSelective.mockResolvedValueOnce({ outputDirectory: 'C:\\chosen-password-output\\selected', extractedFiles: ['folder/a.txt'], totalBytes: 1024 });
  await user.click(screen.getByRole('button', { name: '使用该密码提取所选' }));
  await waitFor(() => expect(close).toHaveBeenCalledOnce());
  expect(bridge.extractSelective).toHaveBeenCalledTimes(2);
  expect(bridge.extractSelective).toHaveBeenNthCalledWith(2, {
    archivePath: job.source.path,
    entryPaths: expect.arrayContaining(['folder', 'folder/a.txt', 'folder/b.txt']),
    destinationDir: 'C:\\chosen-password-output', password: temporaryPassword, flatten: false,
  });
  expect(bridge.inspectArchive).toHaveBeenCalledOnce();
  expect(bridge.pickDirectory).toHaveBeenCalledOnce();
  expect(bridge.savePassword).not.toHaveBeenCalled();
  expect(bridge.addJobs).not.toHaveBeenCalled();
});

it('daemon extraction uses the job output directory without invoking local directory or shell operations', async () => {
  const user = userEvent.setup();
  const close = vi.fn();
  bridge.isDaemonMode.mockReturnValue(true);
  bridge.inspectArchive.mockResolvedValue(inspection);
  bridge.extractSelective.mockResolvedValue({ outputDirectory: 'D:\\service-output\\selected', extractedFiles: ['folder/a.txt'], totalBytes: 1024 });
  render(wrap(<ArchiveBrowser job={{ ...job, options: { ...job.options, outputRoot: 'D:\\service-output' } }} close={close} />));
  await screen.findByText('a.txt');
  expect(screen.queryByRole('button', { name: '提取所选至...' })).toBeNull();
  await user.click(screen.getByRole('button', { name: '全选' }));
  await user.click(screen.getByRole('button', { name: /提取所选条目/ }));
  await waitFor(() => expect(close).toHaveBeenCalledOnce());
  expect(bridge.extractSelective).toHaveBeenCalledExactlyOnceWith(expect.objectContaining({ destinationDir: 'D:\\service-output' }));
  expect(bridge.pickDirectory).not.toHaveBeenCalled();
  expect(bridge.openPath).not.toHaveBeenCalled();
  expect(bridge.addJobs).not.toHaveBeenCalled();
});

it('switching services retires a temporary extraction password even when the new job has the same identity', async () => {
  const user = userEvent.setup();
  const close = vi.fn();
  bridge.inspectArchive.mockResolvedValue(inspection);
  bridge.extractSelective.mockRejectedValueOnce(new Error('NeedsPassword: encrypted entry'));
  const view = render(wrap(<ArchiveBrowser job={job} close={close} />));
  await screen.findByText('a.txt');
  await user.click(screen.getByRole('button', { name: '全选' }));
  await user.click(screen.getByRole('button', { name: /提取所选条目/ }));
  await user.type(await screen.findByLabelText('所选条目的解压密码'), 'old-service-secret');
  bridge.getConnectionVersion.mockReturnValue(1);
  view.rerender(wrap(<ArchiveBrowser job={job} close={close} />));
  await screen.findByText('a.txt');
  expect(screen.queryByLabelText('所选条目的解压密码')).toBeNull();
  expect(screen.queryByText('已选 3 项')).toBeNull();
  bridge.extractSelective.mockResolvedValueOnce({ outputDirectory: 'C:\\new-service-output', extractedFiles: ['folder/a.txt'], totalBytes: 1024 });
  await user.click(screen.getByRole('button', { name: '全选' }));
  await user.click(screen.getByRole('button', { name: /提取所选条目/ }));
  await waitFor(() => expect(close).toHaveBeenCalledOnce());
  expect(bridge.extractSelective).toHaveBeenNthCalledWith(2, expect.objectContaining({ password: undefined }));
  expect(bridge.savePassword).not.toHaveBeenCalled();
});

it.each(['task', 'connection'] as const)('a late extraction response after switching %s cannot close the browser or open an old output', async changed => {
  const user = userEvent.setup();
  const close = vi.fn();
  let resolveExtract!: (result: { outputDirectory: string; extractedFiles: string[]; totalBytes: number }) => void;
  const pendingExtract = new Promise<{ outputDirectory: string; extractedFiles: string[]; totalBytes: number }>(resolve => { resolveExtract = resolve; });
  bridge.inspectArchive.mockResolvedValue(inspection);
  bridge.extractSelective.mockReturnValue(pendingExtract);
  const view = render(wrap(<ArchiveBrowser job={job} close={close} />));
  await screen.findByText('a.txt');
  await user.click(screen.getByRole('button', { name: '全选' }));
  await user.click(screen.getByRole('button', { name: /提取所选条目/ }));
  await waitFor(() => expect(bridge.extractSelective).toHaveBeenCalledOnce());
  if (changed === 'task') view.rerender(wrap(<ArchiveBrowser job={{ ...job, id: 'new-task' }} close={close} />));
  else bridge.getConnectionVersion.mockReturnValue(1);
  await screen.findByText('a.txt');
  await act(async () => { resolveExtract({ outputDirectory: 'C:\\old-output', extractedFiles: ['folder/a.txt'], totalBytes: 1024 }); await pendingExtract; });
  expect(screen.getByText('a.txt')).toBeTruthy();
  expect(close).not.toHaveBeenCalled();
  expect(bridge.openPath).not.toHaveBeenCalled();
});

it.each(['task', 'connection'] as const)('a late directory choice after switching %s cannot submit an extraction for the old task', async changed => {
  const user = userEvent.setup();
  const close = vi.fn();
  let resolvePick!: (path: string) => void;
  const pendingPick = new Promise<string>(resolve => { resolvePick = resolve; });
  bridge.inspectArchive.mockResolvedValue(inspection);
  bridge.pickDirectory.mockReturnValue(pendingPick);
  const view = render(wrap(<ArchiveBrowser job={job} close={close} />));
  await screen.findByText('a.txt');
  await user.click(screen.getByRole('button', { name: '全选' }));
  await user.click(screen.getByRole('button', { name: '提取所选至...' }));
  await waitFor(() => expect(bridge.pickDirectory).toHaveBeenCalledOnce());
  if (changed === 'task') view.rerender(wrap(<ArchiveBrowser job={{ ...job, id: 'new-task' }} close={close} />));
  else bridge.getConnectionVersion.mockReturnValue(1);
  await screen.findByText('a.txt');
  await act(async () => { resolvePick('C:\\old-choice'); await pendingPick; });
  expect(bridge.extractSelective).not.toHaveBeenCalled();
  expect(close).not.toHaveBeenCalled();
});

it('keeps extraction successful and explains when opening the native output folder fails', async () => {
  const user = userEvent.setup();
  const close = vi.fn();
  bridge.inspectArchive.mockResolvedValue(inspection);
  bridge.extractSelective.mockResolvedValue({ outputDirectory: 'C:\\selected', extractedFiles: ['folder/a.txt'], totalBytes: 1024 });
  bridge.openPath.mockRejectedValue(new Error('Explorer unavailable'));
  render(wrap(<ArchiveBrowser job={job} close={close} />));
  await screen.findByText('a.txt');
  await user.click(screen.getByRole('button', { name: '全选' }));
  await user.click(screen.getByRole('button', { name: /提取所选条目/ }));
  expect(await screen.findByText('提取已完成，但无法打开输出目录：Explorer unavailable')).toBeTruthy();
  expect(close).toHaveBeenCalledOnce();
  expect(bridge.extractSelective).toHaveBeenCalledOnce();
});

it('progress-only job updates do not restart inspection', async () => {
  bridge.inspectArchive.mockResolvedValue(inspection);
  const view = render(wrap(<ArchiveBrowser job={job} close={() => {}} />));
  await screen.findByText('a.txt');
  view.rerender(wrap(<ArchiveBrowser job={{ ...job, progress: 0.5, message: '新的进度' }} close={() => {}} />));
  expect(bridge.inspectArchive).toHaveBeenCalledOnce();
  expect(bridge.cancelInspectArchive).not.toHaveBeenCalled();
});

it('switching tasks during password application prevents a late retry or draft reset', async () => {
  const user = userEvent.setup();
  let resolveSave!: () => void;
  const pendingSave = new Promise<void>(resolve => { resolveSave = resolve; });
  bridge.inspectArchive.mockRejectedValueOnce(new Error('NeedsPassword')).mockResolvedValueOnce(inspection).mockRejectedValueOnce(new Error('NeedsPassword'));
  bridge.savePassword.mockReturnValueOnce(pendingSave);
  const view = render(wrap(<ArchiveBrowser job={job} close={() => {}} />));
  await screen.findByText('此归档需要密码才能列出内容');
  await user.type(screen.getByLabelText('归档密码'), 'old-secret');
  await user.click(screen.getByRole('button', { name: '使用该密码浏览' }));
  await screen.findByText('a.txt');
  await user.click(screen.getByRole('button', { name: '应用此密码并重试解压' }));
  await waitFor(() => expect(bridge.savePassword).toHaveBeenCalledOnce());
  view.rerender(wrap(<ArchiveBrowser job={{ ...job, id: 'new-task' }} close={() => {}} />));
  await screen.findByText('此归档需要密码才能列出内容');
  expect(screen.getByLabelText('归档密码')).toHaveProperty('value', '');
  await user.type(screen.getByLabelText('归档密码'), 'new-secret');
  await act(async () => { resolveSave(); await pendingSave; });
  expect(bridge.retry).not.toHaveBeenCalled();
  expect(screen.getByLabelText('归档密码')).toHaveProperty('value', 'new-secret');
});
