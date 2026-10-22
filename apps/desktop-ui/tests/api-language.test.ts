import { afterEach, beforeEach, expect, test, vi } from 'vitest';

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock('@tauri-apps/api/core', () => ({ invoke }));
vi.mock('../src/i18n', () => ({ t: (text: string) => text }));

beforeEach(() => {
  vi.resetModules();
  invoke.mockReset();
});
afterEach(() => vi.unstubAllGlobals());

test('language changes preserve selection order and recover after a failed update', async () => {
  vi.stubGlobal('window', { __TAURI_INTERNALS__: {} });
  let finishFirst!: () => void;
  invoke.mockImplementationOnce(() => new Promise<void>(resolve => { finishFirst = resolve; }))
    .mockRejectedValueOnce('native update failed')
    .mockResolvedValueOnce(undefined);
  const { api } = await import('../src/api');
  const first = api.setLanguage('zh-CN');
  const second = api.setLanguage('en-US');
  const third = api.setLanguage('ja-JP');
  const secondFailure = expect(second).rejects.toThrow('native update failed');
  await vi.waitFor(() => expect(invoke).toHaveBeenCalledTimes(1));
  finishFirst();
  await first;
  await secondFailure;
  await third;
  expect(invoke.mock.calls).toEqual([
    ['set_ui_language', { language: 'zh-CN' }],
    ['set_ui_language', { language: 'en-US' }],
    ['set_ui_language', { language: 'ja-JP' }],
  ]);
});

test('browser preview skips native language updates', async () => {
  vi.stubGlobal('window', {});
  const { api } = await import('../src/api');
  await api.setLanguage('ja-JP');
  expect(invoke).not.toHaveBeenCalled();
});
