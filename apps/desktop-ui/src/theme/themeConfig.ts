import type { ThemeConfig } from 'antd';
import { theme } from 'antd';

export function getAppTheme(isDark: boolean): ThemeConfig {
  return {
    algorithm: isDark ? theme.darkAlgorithm : theme.defaultAlgorithm,
    token: {
      fontFamily: "'Segoe UI Variable', 'Segoe UI', 'Microsoft YaHei UI', -apple-system, BlinkMacSystemFont, system-ui, sans-serif",
      colorPrimary: isDark ? '#3b82f6' : '#2563eb',
      colorPrimaryHover: isDark ? '#60a5fa' : '#3b82f6',
      colorSuccess: isDark ? '#22c55e' : '#16a34a',
      colorWarning: isDark ? '#f59e0b' : '#d97706',
      colorError: isDark ? '#ef4444' : '#dc2626',
      colorInfo: isDark ? '#38bdf8' : '#0284c7',

      // 背景层次
      colorBgLayout: isDark ? '#0b0f19' : '#f8fafc',
      colorBgContainer: isDark ? '#111827' : '#ffffff',
      colorBgElevated: isDark ? '#1a2234' : '#ffffff',
      colorBgSpotlight: isDark ? '#1f293d' : '#0f172a',

      // 文字层次
      colorText: isDark ? '#f1f5f9' : '#0f172a',
      colorTextSecondary: isDark ? '#94a3b8' : '#475569',
      colorTextTertiary: isDark ? '#64748b' : '#94a3b8',
      colorTextQuaternary: isDark ? '#475569' : '#cbd5e1',

      // 边框层次
      colorBorder: isDark ? '#1e293b' : '#e2e8f0',
      colorBorderSecondary: isDark ? '#172033' : '#edf2f7',

      // 几何与排版
      borderRadius: 6,
      borderRadiusSM: 4,
      borderRadiusLG: 8,
      fontSize: 13,
      controlHeight: 32,
      controlHeightSM: 26,
      controlHeightLG: 38,
      wireframe: false,
    },
    components: {
      Button: {
        controlHeight: 32,
        controlHeightSM: 26,
        paddingInline: 12,
        borderRadius: 6,
        defaultBorderColor: isDark ? '#334155' : '#cbd5e1',
        defaultHoverBorderColor: isDark ? '#3b82f6' : '#2563eb',
      },
      Table: {
        headerBg: isDark ? '#161f30' : '#f1f5f9',
        headerColor: isDark ? '#cbd5e1' : '#475569',
        headerSplitColor: 'transparent',
        rowHoverBg: isDark ? 'rgba(59, 130, 246, 0.08)' : 'rgba(37, 99, 235, 0.04)',
        borderColor: isDark ? '#1e293b' : '#f1f5f9',
        cellPaddingBlock: 10,
        cellPaddingBlockSM: 6,
        cellPaddingInline: 12,
        cellPaddingInlineSM: 8,
      },
      Menu: {
        itemBorderRadius: 6,
        itemMarginInline: 8,
        itemMarginBlock: 4,
        itemHeight: 36,
        activeBarBorderWidth: 0,
        darkItemBg: 'transparent',
        darkSubMenuItemBg: 'transparent',
        darkItemSelectedBg: 'rgba(59, 130, 246, 0.15)',
        darkItemSelectedColor: '#ffffff',
        itemSelectedBg: 'rgba(37, 99, 235, 0.08)',
        itemSelectedColor: '#2563eb',
      },
      Card: {
        borderRadiusLG: 8,
        colorBorderSecondary: isDark ? '#1e293b' : '#e2e8f0',
      },
      Progress: {
        remainingColor: isDark ? '#1e293b' : '#e2e8f0',
        lineBorderRadius: 3,
      },
      Tag: {
        borderRadiusSM: 4,
        defaultBg: isDark ? '#1e293b' : '#f1f5f9',
        defaultColor: isDark ? '#cbd5e1' : '#475569',
      },
      Input: {
        activeBorderColor: '#3b82f6',
        hoverBorderColor: '#60a5fa',
        borderRadius: 6,
      },
      Alert: {
        borderRadiusLG: 6,
      },
    },
  };
}
