import { act, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { App as AntApp, ConfigProvider } from 'antd';
import { beforeEach, expect, it, vi } from 'vitest';
import ToolboxPage from '../src/components/ToolboxPage';
import type { AppSnapshot, ToolboxPreviewResult } from '../src/types';
import { defaultSettings } from '../src/types';

const bridge = vi.hoisted(() => ({ pickDirectory: vi.fn(), previewToolbox: vi.fn(), executeToolbox: vi.fn() }));
vi.mock('../src/api', () => ({ api: bridge, isDesktop: true }));
const snapshot: AppSnapshot = { jobs: [], passwords: [], settings: { ...defaultSettings }, running: false, paused: false, vaultError: null, dataDirectory: '', engineAvailable: true, recoverableCount: 0, cacheUsedBytes: 0, sessionId: 'fixture', revision: '1' };
const emptyRun = async (work: () => Promise<unknown>) => { await work(); };
function preview(name: string): ToolboxPreviewResult {
  return { totalScanned: 1, totalPlanned: 1, conflictsCount: 0, token: name + '-token', plans: [{ sourcePath: 'C:\\fixtures\\' + name, relativePath: name, proposedName: name + '.zip', proposedPath: 'C:\\fixtures\\' + name + '.zip', size: 1, action: 'rename', hasConflict: false, conflictReason: null }] };
}
beforeEach(() => { vi.resetAllMocks(); bridge.pickDirectory.mockResolvedValue('C:\\fixtures'); });
const renderPage = () => render(<ConfigProvider button={{ autoInsertSpace: false }} theme={{ token: { motion: false } }}><AntApp><ToolboxPage snapshot={snapshot} refresh={async () => {}} run={emptyRun} /></AntApp></ConfigProvider>);

it('更改选项解除旧分析的加载状态，迟到结果不能替换新预览', async () => {
  const user = userEvent.setup();
  let resolve!: (value: ToolboxPreviewResult) => void;
  bridge.previewToolbox.mockReturnValueOnce(new Promise<ToolboxPreviewResult>(yes => { resolve = yes; })).mockResolvedValueOnce(preview('new-result'));
  renderPage();
  await user.click(screen.getByRole('button', { name: /选择文件夹$/ }));
  const analyze = screen.getByRole('button', { name: /分析并预览改动/ });
  await user.click(analyze);
  await waitFor(() => expect(bridge.previewToolbox).toHaveBeenCalledTimes(1));
  await user.click(screen.getByText('跳过冲突文件'));
  await user.click(analyze);
  await waitFor(() => expect(bridge.previewToolbox).toHaveBeenCalledTimes(2));
  await screen.findByText('new-result');
  await act(async () => { resolve(preview('old-result')); });
  expect(screen.queryByText('old-result')).toBeNull();
  expect(screen.getByText('new-result')).toBeTruthy();
  expect(bridge.executeToolbox).not.toHaveBeenCalled();
});

it('已分析的预览随选项改变立即失效，旧审批不能执行新选项', async () => {
  const user = userEvent.setup(); bridge.previewToolbox.mockResolvedValue(preview('approved-result'));
  renderPage();
  await user.click(screen.getByRole('button', { name: /选择文件夹$/ }));
  await user.click(screen.getByRole('button', { name: /分析并预览改动/ }));
  await screen.findByText('approved-result');
  await user.click(screen.getByText('跳过冲突文件'));
  expect(screen.queryByText('approved-result')).toBeNull();
  expect(bridge.executeToolbox).not.toHaveBeenCalled();
});
