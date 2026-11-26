import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, screen, act } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { App as AntApp, ConfigProvider } from 'antd';
import zhCN from 'antd/locale/zh_CN';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import {
  SkinProvider,
  useSkin,
  SKIN_PRESETS,
  SKIN_TOKENS,
  type SkinId,
  type SkinContextValue,
} from '../src/theme';
import QueuePage from '../src/components/QueuePage';
import VaultPage from '../src/components/VaultPage';
import HistoryPage from '../src/components/HistoryPage';
import SettingsPage from '../src/components/SettingsPage';
import { defaultSettings, type AppSnapshot, type JobView } from '../src/types';

const bridge = vi.hoisted(() => ({
  isDaemonMode: vi.fn(() => false),
  saveSettings: vi.fn(),
  pickDirectory: vi.fn(),
  pickFiles: vi.fn(),
  savePassword: vi.fn(),
  deletePassword: vi.fn(),
  historyPage: vi.fn().mockResolvedValue({ items: [], total: 0 }),
  retry: vi.fn(),
  clearHistory: vi.fn(),
  onDrop: vi.fn(() => Promise.resolve(() => {})),
}));

vi.mock('../src/api', () => ({
  isDesktop: true,
  api: bridge,
}));

const mockJob: JobView = {
  id: 'qa-job-1',
  source: {
    path: 'C:\\data\\test-archive.zip',
    displayName: 'test-archive.zip',
    volumes: [],
    size: 20971520,
    missingVolume: false,
  },
  options: { ...defaultSettings },
  status: 'running',
  stage: 'extracting',
  progress: 0.72,
  message: 'Extracting payload...',
  attempts: 1,
  outputPath: 'C:\\output\\test',
  verificationScope: 'standard',
  createdAt: '2026-11-02T08:00:00Z',
  finishedAt: null,
  workspace: null,
  nodes: [],
  stageTimes: {},
  transferProgress: {
    completedBytes: 15099494,
    totalBytes: 20971520,
    bytesPerSecond: 10485760,
    remainingSeconds: 1,
    updatedAt: '2026-11-02T08:00:00Z',
  },
};

const mockSnapshot: AppSnapshot = {
  jobs: [mockJob],
  passwords: [
    {
      id: 'pwd-1',
      name: 'Primary Archive Key',
      groups: ['Work'],
      notes: 'Encrypted',
      enabled: true,
      successCount: 3,
      lastSuccess: '2026-11-02',
      revision: 1,
    },
    {
      id: 'pwd-2',
      name: 'Legacy Zip Key',
      groups: ['Personal'],
      notes: '',
      enabled: false,
      successCount: 0,
      lastSuccess: null,
      revision: 1,
    },
  ],
  settings: { ...defaultSettings },
  running: true,
  paused: false,
  vaultError: null,
  dataDirectory: 'C:\\data',
  engineAvailable: true,
  recoverableCount: 0,
  cacheUsedBytes: 52428800,
  sessionId: 'test-session',
  revision: '1',
};

const run = async (work: () => Promise<unknown>) => {
  await work();
};
const refresh = async () => {};

function SkinTestInspector({
  onInspect,
}: {
  onInspect: (state: ReturnType<typeof useSkin>) => void;
}) {
  const skin = useSkin();
  onInspect(skin);
  return (
    <div data-testid="skin-inspector">
      <span data-testid="current-skin">{skin.skin}</span>
      <span data-testid="current-theme-mode">{skin.themeMode}</span>
      <span data-testid="is-dark">{skin.isDark ? 'dark' : 'light'}</span>
      <span data-testid="reduced-motion">{skin.reducedMotion ? 'reduced' : 'normal'}</span>
    </div>
  );
}

describe('QA Cross-Skin Full Regression & A11y Suite', () => {
  beforeEach(() => {
    vi.resetAllMocks();
    localStorage.clear();
    document.documentElement.removeAttribute('data-skin');
    document.documentElement.removeAttribute('data-theme');
    document.body.className = '';
    bridge.historyPage.mockResolvedValue({ items: [], total: 0 });
  });

  afterEach(() => {
    localStorage.clear();
  });

  describe('1. Skin Engine Token & Preset Integrity Matrix', () => {
    const skinIds: SkinId[] = ['win11', 'win98', 'modern', 'cyberpunk'];

    it('all 4 skins exist in SKIN_PRESETS with complete metadata', () => {
      expect(SKIN_PRESETS).toHaveLength(4);
      for (const id of skinIds) {
        const preset = SKIN_PRESETS.find((p) => p.id === id);
        expect(preset).toBeDefined();
        expect(preset?.name).toBeTruthy();
        expect(preset?.nameEn).toBeTruthy();
        expect(preset?.description).toBeTruthy();
        expect(preset?.preview.accentColor).toMatch(/^#[0-9a-fA-F]{6}$/);
      }
    });

    it('all 4 skins define both light and dark tokens with valid colors and radius specs', () => {
      for (const id of skinIds) {
        const tokens = SKIN_TOKENS[id];
        expect(tokens).toBeDefined();
        expect(tokens.light).toBeDefined();
        expect(tokens.dark).toBeDefined();

        if (id === 'win98') {
          expect(tokens.light.borderRadius).toBe(0);
          expect(tokens.dark.borderRadius).toBe(0);
          expect(tokens.light.wireframe).toBe(true);
          expect(tokens.dark.wireframe).toBe(true);
        } else if (id === 'cyberpunk') {
          expect(tokens.light.colorPrimary).toBe('#ff0055');
          expect(tokens.dark.colorPrimary).toBe('#00f0ff');
        } else if (id === 'win11') {
          expect(tokens.light.borderRadius).toBe(6);
          expect(tokens.light.colorPrimary).toBe('#0067c0');
        } else if (id === 'modern') {
          expect(tokens.light.colorPrimary).toBe('#4f46e5');
        }
      }
    });
  });

  describe('2. Accessibility, Reduced-Motion & High-Contrast Gate', () => {
    it('styles.css contains robust prefers-reduced-motion definitions', () => {
      const css = ['tokens.css', 'layout.css', 'skins.css']
        .map((f) => readFileSync(resolve(__dirname, '../src/styles', f), 'utf-8'))
        .join('\n');
      expect(css).toContain('@media (prefers-reduced-motion: reduce)');
      expect(css).toContain('animation: none !important');
      expect(css).toContain('transition: none !important');
      expect(css).toContain('transform: none !important');
    });

    it('styles.css contains forced-colors high contrast definitions for Windows HC mode', () => {
      const css = ['tokens.css', 'layout.css', 'skins.css']
        .map((f) => readFileSync(resolve(__dirname, '../src/styles', f), 'utf-8'))
        .join('\n');
      expect(css).toContain('@media (forced-colors: active)');
      expect(css).toContain('ButtonBorder');
      expect(css).toContain('Highlight');
    });

    it('win98 skin enforces reducedMotion=true automatically', () => {
      let state: SkinContextValue | null = null;
      render(
        <SkinProvider initialSkin="win98" initialThemeMode="light">
          <SkinTestInspector
            onInspect={(s) => {
              state = s;
            }}
          />
        </SkinProvider>,
      );
      const inspected = state as SkinContextValue | null;
      expect(inspected?.reducedMotion).toBe(true);
      expect(screen.getByTestId('reduced-motion').textContent).toBe('reduced');
    });

    it('DOM elements receive proper data-skin and data-theme attributes across mode switches', () => {
      let state: SkinContextValue | null = null;
      render(
        <SkinProvider initialSkin="cyberpunk" initialThemeMode="dark">
          <SkinTestInspector
            onInspect={(s) => {
              state = s;
            }}
          />
        </SkinProvider>,
      );

      expect(document.documentElement.getAttribute('data-skin')).toBe('cyberpunk');
      expect(document.documentElement.getAttribute('data-theme')).toBe('dark');
      expect(document.body.classList.contains('skin-cyberpunk')).toBe(true);
      expect(document.body.classList.contains('theme-dark')).toBe(true);

      act(() => {
        const inspected = state as SkinContextValue | null;
        inspected?.setSkin('modern');
        inspected?.setThemeMode('light');
      });

      expect(document.documentElement.getAttribute('data-skin')).toBe('modern');
      expect(document.documentElement.getAttribute('data-theme')).toBe('light');
      expect(document.body.classList.contains('skin-modern')).toBe(true);
      expect(document.body.classList.contains('skin-cyberpunk')).toBe(false);
      expect(document.body.classList.contains('theme-light')).toBe(true);
      expect(document.body.classList.contains('theme-dark')).toBe(false);
    });
  });

  describe('3. Multi-Page Layout & View Switcher Stability Across All Skins', () => {
    const skinList: SkinId[] = ['win11', 'win98', 'modern', 'cyberpunk'];

    for (const skin of skinList) {
      it('renders QueuePage in both Table and Kanban modes under skin ' + skin, async () => {
        const user = userEvent.setup();
        const { unmount } = render(
          <SkinProvider initialSkin={skin} initialThemeMode="light">
            <ConfigProvider
              locale={zhCN}
              button={{ autoInsertSpace: false }}
              theme={{ token: { motion: false } }}
            >
              <AntApp>
                <QueuePage snapshot={mockSnapshot} refresh={refresh} run={run} />
              </AntApp>
            </ConfigProvider>
          </SkinProvider>,
        );

        expect(screen.getByText('表格视图')).toBeTruthy();
        expect(screen.getByText('卡片看板')).toBeTruthy();
        expect(screen.getByRole('table')).toBeTruthy();

        await user.click(screen.getByText('卡片看板'));
        expect(screen.queryByRole('table')).toBeNull();
        expect(screen.getAllByText('test-archive.zip').length).toBeGreaterThan(0);

        unmount();
      });

      it(
        'renders VaultPage in both Card and Table modes with telemetry summary under skin ' + skin,
        async () => {
          const user = userEvent.setup();
          const { unmount } = render(
            <SkinProvider initialSkin={skin} initialThemeMode="dark">
              <ConfigProvider
                locale={zhCN}
                button={{ autoInsertSpace: false }}
                theme={{ token: { motion: false } }}
              >
                <AntApp>
                  <VaultPage snapshot={mockSnapshot} refresh={refresh} run={run} />
                </AntApp>
              </ConfigProvider>
            </SkinProvider>,
          );

          expect(screen.getByText('密码总数')).toBeTruthy();
          expect(screen.getByText('启用状态')).toBeTruthy();
          expect(screen.getByRole('table')).toBeTruthy();

          await user.click(screen.getByText('卡片看板'));
          expect(screen.queryByRole('table')).toBeNull();
          expect(screen.getByText('Primary Archive Key')).toBeTruthy();
          expect(screen.getByText('Legacy Zip Key')).toBeTruthy();

          unmount();
        },
      );

      it('renders HistoryPage with metrics bar and pagination under skin ' + skin, () => {
        const { unmount } = render(
          <SkinProvider initialSkin={skin} initialThemeMode="light">
            <ConfigProvider
              locale={zhCN}
              button={{ autoInsertSpace: false }}
              theme={{ token: { motion: false } }}
            >
              <AntApp>
                <HistoryPage snapshot={mockSnapshot} refresh={refresh} run={run} />
              </AntApp>
            </ConfigProvider>
          </SkinProvider>,
        );

        expect(screen.getByText('最近记录总数')).toBeTruthy();
        expect(screen.getByText('已完成')).toBeTruthy();
        expect(screen.getByText('处理失败')).toBeTruthy();

        unmount();
      });
    }

    it('renders SettingsPage skin selector and interactive skin cards properly', async () => {
      const user = userEvent.setup();
      render(
        <SkinProvider initialSkin="win11" initialThemeMode="light">
          <ConfigProvider
            locale={zhCN}
            button={{ autoInsertSpace: false }}
            theme={{ token: { motion: false } }}
          >
            <AntApp>
              <SettingsPage snapshot={mockSnapshot} refresh={refresh} run={run} />
            </AntApp>
          </ConfigProvider>
        </SkinProvider>,
      );

      await user.click(screen.getByRole('tab', { name: /界面与备份/ }));

      expect(screen.getByRole('combobox', { name: '界面皮肤' })).toBeTruthy();
      expect(screen.getByText('皮肤预设与实时预览')).toBeTruthy();
      expect(screen.getAllByText('Win11 亚克力流体').length).toBeGreaterThan(0);
      expect(screen.getAllByText('Win98 复古经典').length).toBeGreaterThan(0);
      expect(screen.getAllByText('现代极简 (Modern Minimal)').length).toBeGreaterThan(0);
      expect(screen.getAllByText('赛博霓虹 (Cyberpunk)').length).toBeGreaterThan(0);

      const cyberCard = screen.getByRole('button', { name: /赛博霓虹/i });
      await user.click(cyberCard);
      expect(document.documentElement.getAttribute('data-skin')).toBe('cyberpunk');
    });
  });
});
