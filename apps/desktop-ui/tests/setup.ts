import { vi } from 'vitest';
import '@testing-library/react';

Object.defineProperty(window, 'matchMedia', {
  writable: true,
  value: vi.fn((query: string) => ({
    matches: false,
    media: query,
    onchange: null,
    addListener() {},
    removeListener() {},
    addEventListener() {},
    removeEventListener() {},
    dispatchEvent() { return false; }
  }))
});
globalThis.ResizeObserver = class { observe() {} unobserve() {} disconnect() {} };
HTMLElement.prototype.scrollIntoView = () => {};
const getStyle = window.getComputedStyle;
window.getComputedStyle = element => getStyle(element);
