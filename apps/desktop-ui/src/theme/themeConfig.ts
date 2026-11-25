import type { ThemeConfig } from 'antd';
import { theme } from 'antd';
import type { SkinId } from './types';
import { SKIN_TOKENS, DEFAULT_SKIN } from './presets';

export function getAppTheme(skin: SkinId, isDark: boolean, reducedMotion?: boolean): ThemeConfig;
export function getAppTheme(isDark: boolean, reducedMotion?: boolean): ThemeConfig;
export function getAppTheme(arg1: SkinId | boolean, arg2?: boolean, arg3?: boolean): ThemeConfig {
  let skin: SkinId = DEFAULT_SKIN;
  let isDark = false;
  let reducedMotion = false;

  if (typeof arg1 === 'string') {
    skin = arg1;
    isDark = !!arg2;
    reducedMotion = !!arg3;
  } else {
    skin = DEFAULT_SKIN;
    isDark = !!arg1;
    reducedMotion = !!arg2;
  }

  const skinTokensSet = SKIN_TOKENS[skin] ?? SKIN_TOKENS[DEFAULT_SKIN];
  const t = isDark ? skinTokensSet.dark : skinTokensSet.light;

  // Win98 authentic feel: turn off animations
  const effectiveMotion = skin === 'win98' ? false : !reducedMotion;

  const baseConfig: ThemeConfig = {
    algorithm: isDark ? theme.darkAlgorithm : theme.defaultAlgorithm,
    token: {
      motion: effectiveMotion,
      fontFamily: t.fontFamily,
      colorPrimary: t.colorPrimary,
      colorPrimaryHover: t.colorPrimaryHover,
      colorPrimaryActive: t.colorPrimaryActive,
      colorSuccess: t.colorSuccess,
      colorWarning: t.colorWarning,
      colorError: t.colorError,
      colorInfo: t.colorInfo,

      colorBgLayout: t.colorBgLayout,
      colorBgContainer: t.colorBgContainer,
      colorBgElevated: t.colorBgElevated,
      colorBgSpotlight: t.colorBgSpotlight,

      colorText: t.colorText,
      colorTextSecondary: t.colorTextSecondary,
      colorTextTertiary: t.colorTextTertiary,
      colorTextQuaternary: t.colorTextQuaternary,

      colorBorder: t.colorBorder,
      colorBorderSecondary: t.colorBorderSecondary,

      borderRadius: t.borderRadius,
      borderRadiusSM: t.borderRadiusSM,
      borderRadiusLG: t.borderRadiusLG,
      fontSize: t.fontSize,
      controlHeight: t.controlHeight,
      controlHeightSM: t.controlHeightSM,
      controlHeightLG: t.controlHeightLG,
      wireframe: t.wireframe,
    },
    components: {
      Button: {
        controlHeight: t.controlHeight,
        controlHeightSM: t.controlHeightSM,
        controlHeightLG: t.controlHeightLG,
        paddingInline: skin === 'win98' ? 14 : 12,
        borderRadius: t.borderRadius,
        defaultBorderColor:
          skin === 'win98' ? (isDark ? '#505560' : '#808080') : isDark ? '#334155' : '#cbd5e1',
        defaultHoverBorderColor: t.colorPrimaryHover,
      },
      Table: {
        headerBg:
          skin === 'win98'
            ? isDark
              ? '#242830'
              : '#d4d0c8'
            : skin === 'cyberpunk'
              ? isDark
                ? '#141824'
                : '#eaedf5'
              : isDark
                ? '#161f30'
                : '#f1f5f9',
        headerColor: isDark ? '#cbd5e1' : '#334155',
        headerSplitColor: skin === 'win98' ? (isDark ? '#383c44' : '#808080') : 'transparent',
        rowHoverBg:
          skin === 'cyberpunk'
            ? isDark
              ? 'rgba(0, 240, 255, 0.08)'
              : 'rgba(255, 0, 85, 0.06)'
            : isDark
              ? 'rgba(59, 130, 246, 0.08)'
              : 'rgba(37, 99, 235, 0.04)',
        borderColor: t.colorBorder,
        cellPaddingBlock: skin === 'win98' ? 6 : 10,
        cellPaddingBlockSM: 4,
        cellPaddingInline: skin === 'win98' ? 8 : 12,
        cellPaddingInlineSM: 6,
      },
      Menu: {
        itemBorderRadius: t.borderRadius,
        itemMarginInline: skin === 'win98' ? 2 : 8,
        itemMarginBlock: skin === 'win98' ? 2 : 4,
        itemHeight: skin === 'win98' ? 30 : 36,
        activeBarBorderWidth: 0,
        darkItemBg: 'transparent',
        darkSubMenuItemBg: 'transparent',
        darkItemSelectedBg:
          skin === 'win98'
            ? '#000080'
            : skin === 'cyberpunk'
              ? 'rgba(0, 240, 255, 0.16)'
              : 'rgba(59, 130, 246, 0.15)',
        darkItemSelectedColor: skin === 'cyberpunk' ? '#00f0ff' : '#ffffff',
        itemSelectedBg:
          skin === 'win98'
            ? '#000080'
            : skin === 'cyberpunk'
              ? 'rgba(255, 0, 85, 0.12)'
              : 'rgba(37, 99, 235, 0.08)',
        itemSelectedColor: skin === 'win98' ? '#ffffff' : t.colorPrimary,
      },
      Card: {
        borderRadiusLG: t.borderRadiusLG,
        colorBorderSecondary: t.colorBorderSecondary,
      },
      Progress: {
        remainingColor: isDark ? t.colorBorder : '#e2e8f0',
        lineBorderRadius: skin === 'win98' || skin === 'cyberpunk' ? 0 : 3,
      },
      Tag: {
        borderRadiusSM: t.borderRadiusSM,
        defaultBg: isDark ? t.colorBgElevated : '#f1f5f9',
        defaultColor: isDark ? '#cbd5e1' : '#475569',
      },
      Input: {
        activeBorderColor: t.colorPrimary,
        hoverBorderColor: t.colorPrimaryHover,
        borderRadius: t.borderRadius,
      },
      Alert: {
        borderRadiusLG: t.borderRadius,
      },
    },
  };

  return baseConfig;
}
