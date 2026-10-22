import { act, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { App as AntApp, ConfigProvider } from 'antd';
import { beforeEach, expect, it, vi } from 'vitest';
import { JobDetails } from '../src/components/shared';
import { defaultSettings, type JobView } from '../src/types';
const bridge = vi.hoisted(() => ({ isDaemonMode: vi.fn(), getConnectionVersion: vi.fn(), submitInput: vi.fn(), savePassword: vi.fn(), retry: vi.fn() }));
vi.mock('../src/api', () => ({ isDesktop: false, api: bridge }));
const job: JobView = { id: 'encrypted', source: { path: 'C:\\approved\\encrypted.zip', displayName: 'encrypted.zip', volumes: [], size: 1, missingVolume: false }, options: { ...defaultSettings }, status: 'running', stage: 'needsPassword', progress: 0, message: '', attempts: 1, outputPath: null, verificationScope: '', createdAt: '', finishedAt: null, workspace: null, nodes: [], stageTimes: {}, transferProgress: null };
const run = async (work: () => Promise<unknown>) => { await work(); };
function show(inputDisabled?: boolean) { return render(<ConfigProvider button={{ autoInsertSpace: false }} theme={{ token: { motion: false } }}><AntApp><JobDetails job={job} close={() => {}} run={run} operationsDisabled inputDisabled={inputDisabled} /></AntApp></ConfigProvider>); }
async function passwordDialog() {
 // AntD deliberately shares test-id across dialogs in NODE_ENV=test; locate the live input owner.
 const input = await screen.findByLabelText('即时解压密码');
 const dialog = input.closest('[role="dialog"]');
 if (!(dialog instanceof HTMLElement)) throw new Error('password input must belong to a dialog');
 expect(within(dialog).getByText('输入解压密码 — encrypted.zip')).toBeTruthy();
 return dialog;
}
beforeEach(() => { vi.resetAllMocks(); bridge.isDaemonMode.mockReturnValue(true); bridge.getConnectionVersion.mockReturnValue(0); bridge.submitInput.mockResolvedValue({ success: true, jobId: job.id, inputType: 'password', resumedInFlight: true }); bridge.savePassword.mockResolvedValue({ id: 'saved' }); });
it('daemon details submit an in-flight password with exact whitespace and no default vault write', async () => {
 const user = userEvent.setup(); show();
 await user.click(screen.getByRole('button', { name: '提交密码' }));
 const dialog = await passwordDialog();
 expect(within(dialog).getByRole('checkbox')).toHaveProperty('checked', false);
 await user.type(within(dialog).getByLabelText('即时解压密码'), '  exact secret  ');
 await user.click(within(dialog).getByRole('button', { name: '提交密码' }));
 await waitFor(() => expect(bridge.submitInput).toHaveBeenCalledExactlyOnceWith('encrypted', 'password', '  exact secret  '));
 expect(bridge.savePassword).not.toHaveBeenCalled(); expect(bridge.retry).not.toHaveBeenCalled();
});
it('explicit vault opt-in saves before submitting the same daemon input', async () => {
 const user = userEvent.setup(); show(false);
 await user.click(screen.getByRole('button', { name: '提交密码' }));
 const dialog = await passwordDialog();
 await user.type(within(dialog).getByLabelText('即时解压密码'), 'opt-in secret');
 await user.click(within(dialog).getByRole('checkbox'));
 await user.click(within(dialog).getByRole('button', { name: '提交密码' }));
 await waitFor(() => expect(bridge.submitInput).toHaveBeenCalledExactlyOnceWith('encrypted', 'password', 'opt-in secret'));
 expect(bridge.savePassword).toHaveBeenCalledExactlyOnceWith(expect.objectContaining({ secret: 'opt-in secret' }));
 expect(bridge.savePassword.mock.invocationCallOrder[0]).toBeLessThan(bridge.submitInput.mock.invocationCallOrder[0]);
 expect(bridge.retry).not.toHaveBeenCalled();
});
it('closing a daemon draft during an opted-in vault save prevents a late password submission', async () => {
 let complete!: (value: unknown) => void;
 const pending = new Promise(done => { complete = done; }); bridge.savePassword.mockReturnValueOnce(pending);
 const user = userEvent.setup(); show(false);
 await user.click(screen.getByRole('button', { name: '提交密码' }));
 const dialog = await passwordDialog();
 await user.type(within(dialog).getByLabelText('即时解压密码'), 'cancelled secret');
 await user.click(within(dialog).getByRole('checkbox'));
 await user.click(within(dialog).getByRole('button', { name: '提交密码' }));
 await waitFor(() => expect(bridge.savePassword).toHaveBeenCalledOnce());
 await user.click(within(dialog).getByRole('button', { name: '取消' }));
 await act(async () => { complete({ id: 'saved' }); await pending; });
 expect(bridge.submitInput).not.toHaveBeenCalled(); expect(bridge.retry).not.toHaveBeenCalled();
});
it('an explicit deletion/input lock still disables the live password entry', () => {
 show(true); expect(screen.getByRole('button', { name: '提交密码' })).toHaveProperty('disabled', true);
});

it('a vault response from the previous connection cannot forward the password to a newly selected endpoint', async () => {
 let complete!: (value: unknown) => void; const pending = new Promise(done => { complete = done; });
 bridge.savePassword.mockReturnValueOnce(pending); const user = userEvent.setup(); show(false);
 await user.click(screen.getByRole('button', { name: '提交密码' }));
 const dialog = await passwordDialog();
 await user.type(within(dialog).getByLabelText('即时解压密码'), 'endpoint-bound secret');
 await user.click(within(dialog).getByRole('checkbox'));
 await user.click(within(dialog).getByRole('button', { name: '提交密码' }));
 await waitFor(() => expect(bridge.savePassword).toHaveBeenCalledOnce());
 bridge.getConnectionVersion.mockReturnValue(1);
 await act(async () => { complete({ id: 'saved' }); await pending; });
 expect(bridge.submitInput).not.toHaveBeenCalled(); expect(bridge.retry).not.toHaveBeenCalled();
});
