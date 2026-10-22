import { afterEach, expect, it } from 'vitest';
import { act, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import App from '../src/App';
import { languageStorageKey, setLanguage } from '../src/i18n';

afterEach(() => { act(() => setLanguage('zh-CN')); localStorage.removeItem(languageStorageKey); });

it('switches all navigation and the mounted settings page without losing an unsaved draft', async () => {
  const user = userEvent.setup();
  render(<App />);
  await user.click(await screen.findByRole('menuitem', { name: '设置与备份' }));
  const output = await screen.findByLabelText('默认输出根目录');
  // The read-only directory picker is not available in browser preview. Use the
  // history limit instead to verify that language changes preserve drafts.
  expect(output).toBeTruthy();
  await user.click(screen.getByRole('tab', { name: /界面与备份/ }));
  const historyLimit = screen.getByRole('spinbutton', { name: '历史记录上限' });
  await user.clear(historyLimit);
  await user.type(historyLimit, '777');
  await user.click(screen.getByRole('combobox', { name: '界面语言' }));
  await user.click(screen.getByText('English'));
  expect(await screen.findByRole('menuitem', { name: 'Settings & backup' })).toBeTruthy();
  expect(screen.getByRole('spinbutton', { name: 'History limit' })).toHaveProperty('value', '777');
  act(() => setLanguage('ja-JP'));
  expect(screen.getByRole('menuitem', { name: '設定とバックアップ' })).toBeTruthy();
  expect(screen.getByRole('spinbutton', { name: '履歴の上限' })).toHaveProperty('value', '777');
});
