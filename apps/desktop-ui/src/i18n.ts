import { useSyncExternalStore } from 'react';
import { pageMessages } from './locales/pages';
import { settingsMessages } from './locales/settings';
import { apiMessages } from './locales/api';
import { commonMessages } from './locales/common';

export type Language = 'zh-CN' | 'en-US' | 'ja-JP';
export const languageStorageKey = 'openunpack.language';
export const messages: Record<string, [string, string]> = {
  ...pageMessages, ...settingsMessages, ...apiMessages, ...commonMessages,
};
const listeners = new Set<() => void>();
function isLanguage(value: unknown): value is Language {
  return value === 'zh-CN' || value === 'en-US' || value === 'ja-JP';
}
export function readLanguage(): Language {
  try {
    const saved = localStorage.getItem(languageStorageKey);
    if (isLanguage(saved)) return saved;
  } catch { /* Language switching also works when storage is unavailable. */ }
  return 'zh-CN';
}
let language = readLanguage();
function updateDocument() {
  if (typeof document !== 'undefined') document.documentElement.lang = language;
}
updateDocument();
export function getLanguage(): Language { return language; }
export function setLanguage(next: Language) {
  if (!isLanguage(next)) return;
  try { localStorage.setItem(languageStorageKey, next); } catch { /* Keep the session selection. */ }
  if (language === next) return;
  language = next;
  updateDocument();
  listeners.forEach(listener => listener());
}
if (typeof window !== 'undefined') {
  window.addEventListener('storage', event => {
    if (event.key !== languageStorageKey && event.key !== null) return;
    const next = readLanguage();
    if (language === next) return;
    language = next;
    updateDocument();
    listeners.forEach(listener => listener());
  });
}
function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => { listeners.delete(listener); };
}
export function useLanguage() {
  const current = useSyncExternalStore(subscribe, getLanguage, (): Language => 'zh-CN');
  return { language: current, setLanguage };
}
/** Only call for application text, never for file names, passwords or user data. */
export function t(source: string, values?: Record<string, string | number>): string {
  const translated = language === 'zh-CN' ? source : messages[source]?.[language === 'en-US' ? 0 : 1] ?? source;
  return values ? translated.replace(/\{(\w+)\}/g, (token, key: string) =>
    Object.prototype.hasOwnProperty.call(values, key) ? String(values[key]) : token) : translated;
}
