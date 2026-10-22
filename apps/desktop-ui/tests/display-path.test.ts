import { describe, expect, it } from 'vitest';
import { displayPath } from '../src/components/shared';

describe('displayPath 规范路径显示（仅界面渲染，不回写 IPC）', () => {
  it('去除扩展长度前缀并保留中文、空格与常规路径', () => {
    expect(displayPath('\\\\?\\C:\\Users\\im\\下载 目录\\包 一.zip')).toBe('C:\\Users\\im\\下载 目录\\包 一.zip');
    expect(displayPath('\\\\?\\D:\\分卷\\split.7z.001')).toBe('D:\\分卷\\split.7z.001');
  });
  it('UNC 扩展前缀转换为常规 UNC 形式', () => {
    expect(displayPath('\\\\?\\UNC\\server\\share\\a.zip')).toBe('\\\\server\\share\\a.zip');
  });
  it('常规路径、空值与未知形式保持不变', () => {
    expect(displayPath('C:\\already\\normal.zip')).toBe('C:\\already\\normal.zip');
    expect(displayPath('\\\\server\\share\\a.zip')).toBe('\\\\server\\share\\a.zip');
    expect(displayPath('')).toBe('');
    expect(displayPath(null)).toBe('');
    expect(displayPath(undefined)).toBe('');
    expect(displayPath('相对\\路径\\a.zip')).toBe('相对\\路径\\a.zip');
  });
});
