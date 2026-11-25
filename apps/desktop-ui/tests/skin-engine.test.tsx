import { describe, it, expect, beforeEach, afterEach } from 'vitest';
import { render, screen, act } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import {
  SKIN_PRESETS,
  SKIN_STORAGE_KEY,
  THEME_MODE_STORAGE_KEY,
  DEFAULT_SKIN,
  getAppTheme,
  SkinProvider,
  useSkin,
} from '../src/theme';

describe('Skin Engine: Presets & Tokens', () => {
  it('registers all four presets with comprehensive metadata', () => {
    const ids = SKIN_PRESETS.map((p) => p.id);
    expect(ids).toContain('win11');
    expect(ids).toContain('win98');
    expect(ids).toContain('modern');
    expect(ids).toContain('cyberpunk');
    expect(SKIN_PRESETS.length).toBe(4);
    expect(DEFAULT_SKIN).toBe('win11');

    for (const preset of SKIN_PRESETS) {
      expect(preset.name).toBeTruthy();
      expect(preset.nameEn).toBeTruthy();
      expect(preset.description).toBeTruthy();
      expect(preset.tags.length).toBeGreaterThan(0);
      expect(preset.preview.accentColor).toMatch(/^#[0-9a-fA-F]{6}$/);
      expect(typeof preset.preview.borderRadius).toBe('number');
    }
  });

  it('generates distinct ThemeConfig for Win11 Fluent', () => {
    const lightTheme = getAppTheme('win11', false);
    const darkTheme = getAppTheme('win11', true);

    expect(lightTheme.token?.borderRadius).toBe(6);
    expect(lightTheme.token?.wireframe).toBe(false);
    expect(lightTheme.token?.colorPrimary).toBe('#0067c0');

    expect(darkTheme.token?.colorPrimary).toBe('#4cc2ff');
    expect(darkTheme.token?.colorBgLayout).toBe('#1c1c1c');
  });

  it('generates distinct ThemeConfig for Win98 Classic with zero radius and disabled motion', () => {
    const lightTheme = getAppTheme('win98', false);
    const darkTheme = getAppTheme('win98', true);

    expect(lightTheme.token?.borderRadius).toBe(0);
    expect(lightTheme.token?.borderRadiusSM).toBe(0);
    expect(lightTheme.token?.borderRadiusLG).toBe(0);
    expect(lightTheme.token?.wireframe).toBe(true);
    expect(lightTheme.token?.motion).toBe(false);
    expect(lightTheme.token?.colorPrimary).toBe('#000080');
    expect(lightTheme.components?.Button?.borderRadius).toBe(0);

    expect(darkTheme.token?.borderRadius).toBe(0);
    expect(darkTheme.token?.motion).toBe(false);
    expect(darkTheme.token?.colorPrimary).toBe('#3a75c4');
  });

  it('generates distinct ThemeConfig for Modern Minimal and Cyberpunk', () => {
    const modern = getAppTheme('modern', false);
    expect(modern.token?.borderRadius).toBe(6);
    expect(modern.token?.colorPrimary).toBe('#4f46e5');

    const cyberDark = getAppTheme('cyberpunk', true);
    expect(cyberDark.token?.borderRadius).toBe(2);
    expect(cyberDark.token?.colorPrimary).toBe('#00f0ff');
    expect(cyberDark.token?.colorBgLayout).toBe('#08090e');
  });

  it('maintains backward compatibility with getAppTheme(isDark, reducedMotion)', () => {
    const fallbackLight = getAppTheme(false);
    const fallbackDark = getAppTheme(true, true);

    expect(fallbackLight.token?.colorPrimary).toBe('#0067c0');
    expect(fallbackLight.token?.motion).toBe(true);

    expect(fallbackDark.token?.colorPrimary).toBe('#4cc2ff');
    expect(fallbackDark.token?.motion).toBe(false);
  });
});

describe('Skin Engine: SkinProvider & useSkin', () => {
  beforeEach(() => {
    localStorage.clear();
    document.documentElement.removeAttribute('data-skin');
    document.documentElement.removeAttribute('data-theme');
    document.body.removeAttribute('data-skin');
    document.body.removeAttribute('data-theme');
  });

  afterEach(() => {
    localStorage.clear();
  });

  function Consumer() {
    const {
      skin,
      themeMode,
      effectiveSkin,
      effectiveThemeMode,
      isDark,
      reducedMotion,
      setSkin,
      setThemeMode,
      previewSkin,
    } = useSkin();

    return (
      <div>
        <div data-testid="current-skin">{skin}</div>
        <div data-testid="effective-skin">{effectiveSkin}</div>
        <div data-testid="current-mode">{themeMode}</div>
        <div data-testid="effective-mode">{effectiveThemeMode}</div>
        <div data-testid="is-dark">{isDark ? 'dark' : 'light'}</div>
        <div data-testid="reduced-motion">{reducedMotion ? 'true' : 'false'}</div>
        <div data-testid="preview-skin">{previewSkin ?? 'none'}</div>
        <button onClick={() => setSkin('cyberpunk')}>Set Cyberpunk</button>
        <button onClick={() => setSkin('win98')}>Set Win98</button>
        <button onClick={() => setThemeMode('dark')}>Set Dark Mode</button>
        <button onClick={() => setThemeMode('light')}>Set Light Mode</button>
      </div>
    );
  }

  it('initializes with default skin and system mode, and updates DOM attributes', () => {
    render(
      <SkinProvider>
        <Consumer />
      </SkinProvider>,
    );

    expect(screen.getByTestId('current-skin').textContent).toBe('win11');
    expect(screen.getByTestId('current-mode').textContent).toBe('system');
    expect(document.body.getAttribute('data-skin')).toBe('win11');
    expect(document.documentElement.getAttribute('data-skin')).toBe('win11');
    expect(document.body.classList.contains('skin-win11')).toBe(true);
  });

  it('persists skin changes to localStorage and synchronizes DOM attributes', async () => {
    const user = userEvent.setup();
    render(
      <SkinProvider>
        <Consumer />
      </SkinProvider>,
    );

    await user.click(screen.getByText('Set Cyberpunk'));
    expect(screen.getByTestId('current-skin').textContent).toBe('cyberpunk');
    expect(localStorage.getItem(SKIN_STORAGE_KEY)).toBe('cyberpunk');
    expect(document.body.getAttribute('data-skin')).toBe('cyberpunk');
    expect(document.documentElement.getAttribute('data-skin')).toBe('cyberpunk');
    expect(document.body.classList.contains('skin-cyberpunk')).toBe(true);
  });

  it('persists themeMode changes and synchronizes data-theme attribute', async () => {
    const user = userEvent.setup();
    render(
      <SkinProvider>
        <Consumer />
      </SkinProvider>,
    );

    await user.click(screen.getByText('Set Dark Mode'));
    expect(screen.getByTestId('current-mode').textContent).toBe('dark');
    expect(screen.getByTestId('is-dark').textContent).toBe('dark');
    expect(localStorage.getItem(THEME_MODE_STORAGE_KEY)).toBe('dark');
    expect(document.body.getAttribute('data-theme')).toBe('dark');
    expect(document.documentElement.getAttribute('data-theme')).toBe('dark');

    await user.click(screen.getByText('Set Light Mode'));
    expect(screen.getByTestId('current-mode').textContent).toBe('light');
    expect(screen.getByTestId('is-dark').textContent).toBe('light');
    expect(localStorage.getItem(THEME_MODE_STORAGE_KEY)).toBe('light');
    expect(document.body.getAttribute('data-theme')).toBe('light');
  });

  it('handles live skin and theme preview events without overwriting stored preference', () => {
    render(
      <SkinProvider>
        <Consumer />
      </SkinProvider>,
    );

    act(() => {
      window.dispatchEvent(
        new CustomEvent('openunpack:skin-preview', { detail: { skin: 'win98' } }),
      );
    });

    expect(screen.getByTestId('current-skin').textContent).toBe('win11');
    expect(screen.getByTestId('effective-skin').textContent).toBe('win98');
    expect(document.body.getAttribute('data-skin')).toBe('win98');
    expect(localStorage.getItem(SKIN_STORAGE_KEY)).toBeNull();

    act(() => {
      window.dispatchEvent(new CustomEvent('openunpack:skin-preview', { detail: { skin: null } }));
    });

    expect(screen.getByTestId('effective-skin').textContent).toBe('win11');
    expect(document.body.getAttribute('data-skin')).toBe('win11');
  });

  it('enforces reduced motion automatically on Win98 skin', async () => {
    const user = userEvent.setup();
    render(
      <SkinProvider>
        <Consumer />
      </SkinProvider>,
    );

    expect(screen.getByTestId('reduced-motion').textContent).toBe('false');

    await user.click(screen.getByText('Set Win98'));
    expect(screen.getByTestId('effective-skin').textContent).toBe('win98');
    expect(screen.getByTestId('reduced-motion').textContent).toBe('true');
  });

  it('restores stored values from localStorage upon mount', () => {
    localStorage.setItem(SKIN_STORAGE_KEY, 'modern');
    localStorage.setItem(THEME_MODE_STORAGE_KEY, 'dark');

    render(
      <SkinProvider>
        <Consumer />
      </SkinProvider>,
    );

    expect(screen.getByTestId('current-skin').textContent).toBe('modern');
    expect(screen.getByTestId('current-mode').textContent).toBe('dark');
    expect(screen.getByTestId('is-dark').textContent).toBe('dark');
    expect(document.body.getAttribute('data-skin')).toBe('modern');
    expect(document.body.getAttribute('data-theme')).toBe('dark');
  });
});
