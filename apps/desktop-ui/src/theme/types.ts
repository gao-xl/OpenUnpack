import type { ThemeConfig } from 'antd';

export type SkinId = 'win11' | 'win98' | 'modern' | 'cyberpunk';
export type ThemeMode = 'system' | 'light' | 'dark';

export interface SkinPreviewMeta {
  accentColor: string;
  bgLayout: string;
  bgContainer: string;
  borderRadius: number;
  wireframe?: boolean;
  fontFamily: string;
}

export interface SkinPreset {
  id: SkinId;
  name: string;
  nameEn: string;
  description: string;
  descriptionEn: string;
  tags: string[];
  preview: SkinPreviewMeta;
}

export interface SkinTokens {
  fontFamily: string;
  fontFamilyCode?: string;
  borderRadius: number;
  borderRadiusSM: number;
  borderRadiusLG: number;
  wireframe: boolean;

  colorPrimary: string;
  colorPrimaryHover: string;
  colorPrimaryActive: string;
  colorSuccess: string;
  colorWarning: string;
  colorError: string;
  colorInfo: string;

  colorBgLayout: string;
  colorBgContainer: string;
  colorBgElevated: string;
  colorBgSpotlight: string;

  colorText: string;
  colorTextSecondary: string;
  colorTextTertiary: string;
  colorTextQuaternary: string;

  colorBorder: string;
  colorBorderSecondary: string;

  controlHeight: number;
  controlHeightSM: number;
  controlHeightLG: number;
  fontSize: number;
}

export interface SkinContextValue {
  skin: SkinId;
  themeMode: ThemeMode;
  effectiveSkin: SkinId;
  effectiveThemeMode: ThemeMode;
  isDark: boolean;
  reducedMotion: boolean;
  setSkin: (skin: SkinId) => void;
  setThemeMode: (mode: ThemeMode) => void;
  previewSkin: SkinId | null;
  previewThemeMode: ThemeMode | null;
  setPreviewSkin: (skin: SkinId | null) => void;
  setPreviewThemeMode: (mode: ThemeMode | null) => void;
  presets: readonly SkinPreset[];
  themeConfig: ThemeConfig;
}
