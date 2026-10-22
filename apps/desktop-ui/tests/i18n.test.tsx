/// <reference types="vite/client" />
import { afterEach, describe, expect, it, vi } from 'vitest';
import { act, render, screen } from '@testing-library/react';
import { getLanguage, languageStorageKey, messages, readLanguage, setLanguage, t, useLanguage } from '../src/i18n';
import ts from 'typescript';

afterEach(() => { act(() => setLanguage('zh-CN')); localStorage.removeItem(languageStorageKey); });

describe('interface languages', () => {
  it('switches mounted text, persists the preference and sets the document language', () => {
    function Label() { const { language } = useLanguage(); return <span lang={language}>{t('批量解压')}</span>; }
    render(<Label />);
    expect(screen.getByText('批量解压')).toBeTruthy();
    act(() => setLanguage('en-US'));
    expect(screen.getByText('Batch extraction')).toBeTruthy();
    expect(localStorage.getItem(languageStorageKey)).toBe('en-US');
    expect(readLanguage()).toBe('en-US');
    act(() => setLanguage('ja-JP'));
    expect(screen.getByText('一括解凍')).toBeTruthy();
    expect(document.documentElement.lang).toBe('ja-JP');
  });

  it('keeps interpolation values intact, including Chinese filenames and placeholder-like text', () => {
    setLanguage('en-US');
    const path = 'C:\\中文目录\\{count}.zip';
    expect(t('数据目录: {path}', { path })).toBe(`Data directory: ${path}`);
    expect(t('unknown diagnostic')).toBe('unknown diagnostic');
  });

  it('falls back safely for invalid preferences or unavailable storage', () => {
    localStorage.setItem(languageStorageKey, 'invalid');
    expect(readLanguage()).toBe('zh-CN');
    const spy = vi.spyOn(Storage.prototype, 'setItem').mockImplementation(() => { throw new Error('denied'); });
    try { setLanguage('ja-JP'); expect(getLanguage()).toBe('ja-JP'); } finally { spy.mockRestore(); }
  });

  it('follows changes from another window', () => {
    localStorage.setItem(languageStorageKey, 'en-US');
    window.dispatchEvent(new StorageEvent('storage', { key: languageStorageKey }));
    expect(getLanguage()).toBe('en-US');
  });

  it('has English and Japanese translations for every literal translation call', () => {
    const missing = new Set<string>();
    const sources = import.meta.glob<string>('../src/**/*.{ts,tsx}', { query: '?raw', import: 'default', eager: true });
      for (const [path, text] of Object.entries(sources)) {
        const source = ts.createSourceFile(path, text, ts.ScriptTarget.Latest, true);
        function visit(node: ts.Node) {
          if (ts.isCallExpression(node) && ts.isIdentifier(node.expression) && node.expression.text === 't') {
            const key = node.arguments[0];
            if (key && ts.isStringLiteralLike(key)) {
              const pair = messages[key.text];
              if (!pair?.[0] || !pair[1]) missing.add(key.text);
              if (pair) for (const text of pair) {
                expect([...text.matchAll(/\{\w+\}/g)].map(m => m[0]).sort(), key.text)
                  .toEqual([...key.text.matchAll(/\{\w+\}/g)].map(m => m[0]).sort());
              }
            }
          }
          ts.forEachChild(node, visit);
        }
        visit(source);
      }
    expect([...missing]).toEqual([]);
  });
});
