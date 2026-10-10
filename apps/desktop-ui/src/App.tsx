import { useState, useEffect, useRef, useCallback, useMemo } from 'react';
import { App as AntApp, ConfigProvider, Layout, Menu, Alert, Spin, Button, Tag, theme } from 'antd';
import {
  FileZipOutlined,
  KeyOutlined,
  HistoryOutlined,
  SettingOutlined,
  ReloadOutlined,
} from '@ant-design/icons';
import { api, isDesktop } from './api';
import type { AppSnapshot } from './types';
import { defaultSettings } from './types';
import zhCN from 'antd/locale/zh_CN';
import { formatBytes } from './components/shared';
import QueuePage from './components/QueuePage';
import VaultPage from './components/VaultPage';
import HistoryPage from './components/HistoryPage';
import SettingsPage from './components/SettingsPage';

const { Header, Sider, Content, Footer } = Layout;

export interface SharedProps {
  snapshot: AppSnapshot;
  refresh: () => Promise<void>;
  run: (work: () => Promise<unknown>) => Promise<void>;
}

const initialSnapshot: AppSnapshot = {
  jobs: [],
  passwords: [],
  settings: defaultSettings,
  running: false,
  paused: false,
  vaultError: null,
  dataDirectory: '',
  engineAvailable: false,
  recoverableCount: 0,
  cacheUsedBytes: 0,
};

const menuItems = [
  { key: 'queue', icon: <FileZipOutlined aria-hidden />, label: '批量解压' },
  { key: 'vault', icon: <KeyOutlined aria-hidden />, label: '密码库' },
  { key: 'history', icon: <HistoryOutlined aria-hidden />, label: '处理记录' },
  { key: 'settings', icon: <SettingOutlined aria-hidden />, label: '设置与备份' },
];

const titleMap: Record<string, string> = {
  queue: '批量解压',
  vault: '密码库',
  history: '处理记录',
  settings: '设置与备份',
};

export default function App() {
  const [snapshot, setSnapshot] = useState<AppSnapshot>(initialSnapshot);
  const [selectedKey, setSelectedKey] = useState<string>('queue');
  const [initialLoading, setInitialLoading] = useState(true);
  const [localError, setLocalError] = useState<string | null>(null);
  const busyRef = useRef(0);
  const refreshGeneration = useRef(0);
  const [busy, setBusy] = useState(false);
  const [compact, setCompact] = useState(() => window.innerWidth <= 1050);
  useEffect(() => {
    const media = window.matchMedia('(max-width: 1050px)');
    const changed = () => setCompact(media.matches);
    media.addEventListener('change', changed);
    return () => media.removeEventListener('change', changed);
  }, []);

  const [systemDark, setSystemDark] = useState<boolean>(
    () => typeof window !== 'undefined' && !!window.matchMedia?.('(prefers-color-scheme: dark)').matches
  );

  useEffect(() => {
    if (typeof window === 'undefined' || !window.matchMedia) return;
    const media = window.matchMedia('(prefers-color-scheme: dark)');
    const handler = (e: MediaQueryListEvent) => setSystemDark(e.matches);
    media.addEventListener('change', handler);
    return () => media.removeEventListener('change', handler);
  }, []);

  const isDark = snapshot.settings.theme === 'dark' || (snapshot.settings.theme !== 'light' && systemDark);

  useEffect(() => {
    const mode = isDark ? 'dark' : 'light';
    document.body.setAttribute('data-theme', mode);
    return () => {
      document.body.removeAttribute('data-theme');
    };
  }, [isDark]);

  const refresh = useCallback(async () => {
    const generation = ++refreshGeneration.current;
    try {
      const next = await api.snapshot();
      if (generation === refreshGeneration.current) setSnapshot(next);
    } catch (err: unknown) {
      if (err instanceof Error) setLocalError(err.message);
    }
  }, []);

  const run = useCallback(
    async (work: () => Promise<unknown>): Promise<void> => {
      // A file picker or scan must not prevent an independent cancellation.
      busyRef.current += 1;
      setBusy(true);
      try {
        await work();
      } catch (err: unknown) {
        setLocalError(err instanceof Error ? err.message : String(err));
      } finally {
        await refresh();
        busyRef.current = Math.max(0, busyRef.current - 1);
        setBusy(busyRef.current > 0);
      }
    },
    [refresh]
  );

  useEffect(() => {
    let unmounted = false;
    let unsub: (() => void) | undefined;

    void refresh().finally(() => {
      if (!unmounted) setInitialLoading(false);
    });

    void api.subscribe((next) => {
      if (!unmounted) { refreshGeneration.current += 1; setSnapshot(next); }
    }, message => { if (!unmounted) setLocalError(message); }).then((cleanup) => {
      if (unmounted) cleanup();
      else unsub = cleanup;
    }).catch(() => { if (!unmounted) setLocalError('状态订阅未建立，请使用刷新检查任务状态。'); });

    return () => {
      unmounted = true;
      if (unsub) unsub();
    };
  }, [refresh]);

  const runningCount = useMemo(
    () => snapshot.jobs.filter((j) => j.status === 'running').length,
    [snapshot.jobs]
  );

  const sharedProps: SharedProps = { snapshot, refresh, run };

  const renderPage = () => {
    switch (selectedKey) {
      case 'vault':
        return <VaultPage {...sharedProps} />;
      case 'history':
        return <HistoryPage {...sharedProps} />;
      case 'settings':
        return <SettingsPage {...sharedProps} />;
      case 'queue':
      default:
        return <QueuePage {...sharedProps} />;
    }
  };

  return (
    <ConfigProvider locale={zhCN} button={{ autoInsertSpace: false }} theme={{ algorithm: isDark ? theme.darkAlgorithm : theme.defaultAlgorithm }}>
      <AntApp>
      <Layout className="app-shell" style={{ minHeight: '100vh' }}>
        <Sider className="sidebar" width={216} collapsed={compact} collapsedWidth={72} theme="dark">
          <div className="brand">
            <span className="brand-mark">OU</span>
            <span className="brand-name">OpenUnpack</span>
          </div>
          <Menu
            mode="inline"
            theme="dark"
            selectedKeys={[selectedKey]}
            items={menuItems}
            onClick={({ key }) => setSelectedKey(key)}
          />
        </Sider>
        <Layout className="main-layout">
          <Header className="header-bar">
            <h2>{titleMap[selectedKey] || 'OpenUnpack'}</h2>
            <div className="header-actions">
              {snapshot.running ? (
                <Tag color="processing">处理中 ({runningCount})</Tag>
              ) : snapshot.paused ? (
                <Tag color="warning">已暂停</Tag>
              ) : (
                <Tag color="default">就绪</Tag>
              )}
              <Button aria-label="刷新状态" icon={<ReloadOutlined aria-hidden />} onClick={() => void refresh()}>
                刷新
              </Button>
            </div>
          </Header>
          <Content className="page-content" style={{ padding: 24, overflow: 'auto' }}>
            {!isDesktop && (
              <Alert
                title="浏览器预览模式"
                description="解压引擎与本地密码库需在桌面客户端中运行。"
                type="info"
                showIcon
                closable
                style={{ marginBottom: 16 }}
              />
            )}
            {snapshot.vaultError && (
              <Alert
                title="密码库状态提示"
                description={snapshot.vaultError}
                type="warning"
                showIcon
                style={{ marginBottom: 16 }}
              />
            )}
            {localError && (
              <Alert
                title="操作提示"
                description={localError}
                type="error"
                showIcon
                closable
                onClose={() => setLocalError(null)}
                style={{ marginBottom: 16 }}
              />
            )}
            {initialLoading ? (
              <div style={{ textAlign: 'center', padding: '80px 0' }}>
                <Spin size="large" />
              </div>
            ) : (
              <div aria-busy={busy}>{renderPage()}</div>
            )}
          </Content>
          <Footer className="bottom-bar">
            <span>缓存占用: {formatBytes(snapshot.cacheUsedBytes)}</span>
            <span>数据目录: {snapshot.dataDirectory || '默认'}</span>
            <span>任务总数: {snapshot.jobs.length} 项</span>
            {snapshot.recoverableCount > 0 && <span>可恢复: {snapshot.recoverableCount}</span>}
          </Footer>
        </Layout>
      </Layout>
      </AntApp>
    </ConfigProvider>
  );
}
