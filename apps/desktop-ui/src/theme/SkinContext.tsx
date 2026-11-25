import React, { createContext, useContext, useState, useEffect, useMemo, useCallback } from 'react';
import type { SkinId, ThemeMode, SkinContextValue } from './types';
import { SKIN_PRESETS, SKIN_STORAGE_KEY, THEME_MODE_STORAGE_KEY, DEFAULT_SKIN } from './presets';
import { getAppTheme } from './themeConfig';

const SkinContext = createContext<SkinContextValue | null>(null);

function getStoredSkin(fallback: SkinId = DEFAULT_SKIN): SkinId {
  if (typeof window === 'undefined') return fallback;
  try {
    const item = localStorage.getItem(SKIN_STORAGE_KEY);
    if (item === 'win11' || item === 'win98' || item === 'modern' || item === 'cyberpunk') {
      return item;
    }
  } catch {
    // LocalStorage might be disabled or throw in restricted context
  }
  return fallback;
}

function getStoredThemeMode(fallback: ThemeMode = 'system'): ThemeMode {
  if (typeof window === 'undefined') return fallback;
  try {
    const item = localStorage.getItem(THEME_MODE_STORAGE_KEY);
    if (item === 'system' || item === 'light' || item === 'dark') {
      return item;
    }
  } catch {
    // LocalStorage might be disabled
  }
  return fallback;
}

export interface SkinProviderProps {
  initialSkin?: SkinId;
  initialThemeMode?: ThemeMode;
  children: React.ReactNode;
}

export function SkinProvider({ initialSkin, initialThemeMode, children }: SkinProviderProps) {
  const [skin, setSkinState] = useState<SkinId>(() => getStoredSkin(initialSkin ?? DEFAULT_SKIN));
  const [themeMode, setThemeModeState] = useState<ThemeMode>(() =>
    getStoredThemeMode(initialThemeMode ?? 'system'),
  );

  const [previewSkin, setPreviewSkin] = useState<SkinId | null>(null);
  const [previewThemeMode, setPreviewThemeMode] = useState<ThemeMode | null>(null);

  const [systemDark, setSystemDark] = useState<boolean>(
    () =>
      typeof window !== 'undefined' &&
      !!window.matchMedia?.('(prefers-color-scheme: dark)').matches,
  );
  const [systemReducedMotion, setSystemReducedMotion] = useState<boolean>(
    () =>
      typeof window !== 'undefined' &&
      !!window.matchMedia?.('(prefers-reduced-motion: reduce)').matches,
  );

  // System media query listeners
  useEffect(() => {
    if (typeof window === 'undefined' || !window.matchMedia) return;
    const media = window.matchMedia('(prefers-color-scheme: dark)');
    const handler = (e: MediaQueryListEvent) => setSystemDark(e.matches);
    media.addEventListener('change', handler);
    return () => media.removeEventListener('change', handler);
  }, []);

  useEffect(() => {
    if (typeof window === 'undefined' || !window.matchMedia) return;
    const media = window.matchMedia('(prefers-reduced-motion: reduce)');
    const handler = (e: MediaQueryListEvent) => setSystemReducedMotion(e.matches);
    media.addEventListener('change', handler);
    return () => media.removeEventListener('change', handler);
  }, []);

  // Listen for live preview events
  useEffect(() => {
    if (typeof window === 'undefined') return;

    const handleSkinPreview = (e: Event) => {
      const custom = e as CustomEvent<{ skin: SkinId | null }>;
      const next = custom.detail?.skin ?? null;
      setPreviewSkin(next);
    };

    const handleThemePreview = (e: Event) => {
      const custom = e as CustomEvent<{ theme: ThemeMode | null }>;
      const next = custom.detail?.theme ?? null;
      setPreviewThemeMode(next);
    };

    window.addEventListener('openunpack:skin-preview', handleSkinPreview);
    window.addEventListener('openunpack:theme-preview', handleThemePreview);

    return () => {
      window.removeEventListener('openunpack:skin-preview', handleSkinPreview);
      window.removeEventListener('openunpack:theme-preview', handleThemePreview);
    };
  }, []);

  // Cross-tab storage synchronization
  useEffect(() => {
    if (typeof window === 'undefined') return;
    const handleStorage = (e: StorageEvent) => {
      if (e.key === SKIN_STORAGE_KEY && e.newValue) {
        if (
          e.newValue === 'win11' ||
          e.newValue === 'win98' ||
          e.newValue === 'modern' ||
          e.newValue === 'cyberpunk'
        ) {
          setSkinState(e.newValue);
        }
      }
      if (e.key === THEME_MODE_STORAGE_KEY && e.newValue) {
        if (e.newValue === 'system' || e.newValue === 'light' || e.newValue === 'dark') {
          setThemeModeState(e.newValue);
        }
      }
    };
    window.addEventListener('storage', handleStorage);
    return () => window.removeEventListener('storage', handleStorage);
  }, []);

  const setSkin = useCallback((newSkin: SkinId) => {
    setSkinState(newSkin);
    try {
      localStorage.setItem(SKIN_STORAGE_KEY, newSkin);
    } catch {
      // Ignore storage errors
    }
  }, []);

  const setThemeMode = useCallback((newMode: ThemeMode) => {
    setThemeModeState(newMode);
    try {
      localStorage.setItem(THEME_MODE_STORAGE_KEY, newMode);
    } catch {
      // Ignore storage errors
    }
  }, []);

  const effectiveSkin = previewSkin ?? skin;
  const effectiveThemeMode = previewThemeMode ?? themeMode;
  const isDark = effectiveThemeMode === 'dark' || (effectiveThemeMode === 'system' && systemDark);
  const reducedMotion = systemReducedMotion || effectiveSkin === 'win98';

  // Synchronize DOM attributes on html and body
  useEffect(() => {
    if (typeof document === 'undefined') return;
    const mode = isDark ? 'dark' : 'light';

    // Set data attributes
    document.documentElement.setAttribute('data-skin', effectiveSkin);
    document.documentElement.setAttribute('data-theme', mode);
    document.body.setAttribute('data-skin', effectiveSkin);
    document.body.setAttribute('data-theme', mode);

    // Maintain skin-specific class list
    const skinClasses = ['skin-win11', 'skin-win98', 'skin-modern', 'skin-cyberpunk'];
    document.body.classList.remove(...skinClasses);
    document.body.classList.add(`skin-${effectiveSkin}`);

    document.body.classList.remove('theme-light', 'theme-dark');
    document.body.classList.add(`theme-${mode}`);
  }, [effectiveSkin, isDark]);

  const themeConfig = useMemo(
    () => getAppTheme(effectiveSkin, isDark, reducedMotion),
    [effectiveSkin, isDark, reducedMotion],
  );

  const value: SkinContextValue = useMemo(
    () => ({
      skin,
      themeMode,
      effectiveSkin,
      effectiveThemeMode,
      isDark,
      reducedMotion,
      setSkin,
      setThemeMode,
      previewSkin,
      previewThemeMode,
      setPreviewSkin,
      setPreviewThemeMode,
      presets: SKIN_PRESETS,
      themeConfig,
    }),
    [
      skin,
      themeMode,
      effectiveSkin,
      effectiveThemeMode,
      isDark,
      reducedMotion,
      setSkin,
      setThemeMode,
      previewSkin,
      previewThemeMode,
      themeConfig,
    ],
  );

  return <SkinContext.Provider value={value}>{children}</SkinContext.Provider>;
}

const defaultFallbackValue: SkinContextValue = {
  skin: DEFAULT_SKIN,
  themeMode: 'system',
  effectiveSkin: DEFAULT_SKIN,
  effectiveThemeMode: 'system',
  isDark: false,
  reducedMotion: false,
  setSkin: () => {},
  setThemeMode: () => {},
  previewSkin: null,
  previewThemeMode: null,
  setPreviewSkin: () => {},
  setPreviewThemeMode: () => {},
  presets: SKIN_PRESETS,
  themeConfig: getAppTheme(DEFAULT_SKIN, false, false),
};

export function useSkin(): SkinContextValue {
  const context = useContext(SkinContext);
  return context ?? defaultFallbackValue;
}
