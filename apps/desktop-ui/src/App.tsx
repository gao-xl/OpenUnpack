import { useState, useEffect, useRef, useCallback, useMemo } from 'react';
import { App as AntApp, ConfigProvider, Layout, Menu, Alert, Spin, Button, Tag, Modal, Typography } from 'antd';
import {
  FileZipOutlined,
  KeyOutlined,
  HistoryOutlined,
  SettingOutlined,
  ReloadOutlined,
  ToolOutlined,
  CheckCircleOutlined,
  CloseCircleOutlined,
} from '@ant-design/icons';
import { api, isDesktop } from './api';
import { humanizeVaultError } from './vaultError';
import type { AppSnapshot } from './types';
import { defaultSettings } from './types';
import { SnapshotCursor } from './snapshot-cursor';
import zhCN from 'antd/locale/zh_CN';
import enUS from 'antd/locale/en_US';
import jaJP from 'antd/locale/ja_JP';
import { t, useLanguage } from './i18n';
import { formatBytes } from './components/shared';
import { getAppTheme } from './theme/themeConfig';
import QueuePage from './components/QueuePage';
import VaultPage from './components/VaultPage';
import HistoryPage from './components/HistoryPage';
import SettingsPage from './components/SettingsPage';
import ToolboxPage from './components/ToolboxPage';

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
  sessionId: 'initial',
  revision: '0',
};

const menuItems = [
  { key: 'queue', icon: <FileZipOutlined aria-hidden />, label: '批量解压' },
  { key: 'toolbox', icon: <ToolOutlined aria-hidden />, label: '目录工具箱' },
  { key: 'vault', icon: <KeyOutlined aria-hidden />, label: '密码库' },
  { key: 'history', icon: <HistoryOutlined aria-hidden />, label: '处理记录' },
  { key: 'settings', icon: <SettingOutlined aria-hidden />, label: '设置与备份' },
];

const titleMap: Record<string, string> = {
  queue: '批量解压',
  toolbox: '目录工具箱',
  vault: '密码库',
  history: '处理记录',
  settings: '设置与备份',
};

export default function App() {
  const { language } = useLanguage();
  const daemon = api.isDaemonMode?.() === true;
  const [snapshot, setSnapshot] = useState<AppSnapshot>(initialSnapshot);
  const [selectedKey, setSelectedKey] = useState<string>('queue');
  const [initialLoading, setInitialLoading] = useState(true);
  const [localError, setLocalError] = useState<string | null>(null);
  useEffect(() => {
    if (isDesktop) void api.setLanguage(language).catch((error: unknown) => {
      setLocalError(error instanceof Error ? error.message : String(error));
    });
  }, [language]);
  const busyRef = useRef(0);
  const refreshGeneration = useRef(0);
  const snapshotCursor = useRef(new SnapshotCursor());
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
    const connectionVersion = api.getConnectionVersion?.() ?? 0;
    try {
      const next = await api.snapshot();
      if (connectionVersion === (api.getConnectionVersion?.() ?? 0) && generation === refreshGeneration.current && snapshotCursor.current.accept(next)) setSnapshot(next);
    } catch (err: unknown) {
      if (connectionVersion === (api.getConnectionVersion?.() ?? 0) && err instanceof Error) setLocalError(err.message);
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
        const message = err instanceof Error ? err.message : String(err);
        setLocalError(humanizeVaultError(message, t) ?? message);
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

    queueMicrotask(() => {
      void refresh().finally(() => {
        if (!unmounted) setInitialLoading(false);
      });
    });

    void api.subscribe((next) => {
      if (!unmounted && snapshotCursor.current.accept(next)) { refreshGeneration.current += 1; setSnapshot(next); }
    }, message => { if (!unmounted) setLocalError(message); }).then((cleanup) => {
      if (unmounted) cleanup();
      else unsub = cleanup;
    }).catch(() => { if (!unmounted) setLocalError(t('状态订阅未建立，请使用刷新检查任务状态。')); });

    return () => {
      unmounted = true;
      if (unsub) unsub();
    };
  }, [refresh]);

  const [countdownState, setCountdownState] = useState<{ action: string; seconds: number } | null>(null);
  const prevRunningRef = useRef(false);

  useEffect(() => {
    const wasRunning = prevRunningRef.current;
    prevRunningRef.current = snapshot.running;

    if (wasRunning && !snapshot.running && isDesktop && !daemon) {
      const allSettled = snapshot.jobs.length > 0 && snapshot.jobs.every(j =>
        ['completed', 'partial', 'salvaged', 'failed', 'cancelled', 'skipped'].includes(j.status)
      );
      const anyCancelled = snapshot.jobs.some(j => j.status === 'cancelled');
      if (allSettled) {
        const action = snapshot.settings.postExtractionAction;
        if (action === 'openOutput') {
          const lastOutput = [...snapshot.jobs]
            .filter(j => j.outputPath)
            .sort((a, b) => (b.finishedAt ?? b.createdAt).localeCompare(a.finishedAt ?? a.createdAt))[0]?.outputPath
            || snapshot.settings.outputRoot;
          if (lastOutput) {
            void api.executePostAction('openOutput', lastOutput).catch((err: unknown) =>
              setLocalError(err instanceof Error ? err.message : String(err)));
          }
        } else if ((action === 'sleep' || action === 'shutdown') && !anyCancelled) {
          // Cancelling the batch means "stop", not "finished": never shut the
          // machine down because the user aborted every remaining job.
          setTimeout(() => {
            setCountdownState({ action, seconds: 30 });
          }, 0);
        }
      }
    }
  }, [snapshot.running, snapshot.jobs, snapshot.settings.postExtractionAction, snapshot.settings.outputRoot, daemon]);

  useEffect(() => {
    if (!countdownState) return;
    const timer = setTimeout(() => {
      setCountdownState(prev => {
        if (!prev) return null;
        if (prev.seconds <= 1) {
          void api.executePostAction(prev.action).catch((err: unknown) =>
            setLocalError(err instanceof Error ? err.message : String(err)));
          return null;
        }
        return { ...prev, seconds: prev.seconds - 1 };
      });
    }, 1000);
    return () => clearTimeout(timer);
  }, [countdownState]);

  const runningCount = useMemo(
    () => snapshot.jobs.filter((j) => j.status === 'running').length,
    [snapshot.jobs]
  );

  const sharedProps: SharedProps = { snapshot, refresh, run };

  const renderPage = () => {
    switch (selectedKey) {
      case 'toolbox':
        return <ToolboxPage {...sharedProps} />;
      case 'vault':
        return <VaultPage {...sharedProps} />;
      case 'history':
        return <HistoryPage {...sharedProps} />;
      case 'settings':
        return <SettingsPage {...sharedProps} />;
      case 'queue':
      default:
        return <QueuePage {...sharedProps} openHistory={() => setSelectedKey('history')} />;
    }
  };

  return (
    <ConfigProvider locale={{ 'zh-CN': zhCN, 'en-US': enUS, 'ja-JP': jaJP }[language]} button={{ autoInsertSpace: false }} theme={getAppTheme(isDark)}>
      <AntApp>
      <Layout className="app-shell" style={{ minHeight: '100vh' }}>
        <Sider className="sidebar" width={200} collapsed={compact} collapsedWidth={64} theme={isDark ? 'dark' : 'light'}>
          <div className="brand">
            <span className="brand-mark">OU</span>
            <span className="brand-name">OpenUnpack</span>
          </div>
          <Menu
            mode="inline"
            theme={isDark ? 'dark' : 'light'}
            selectedKeys={[selectedKey]}
            items={menuItems.map(item => ({
              ...item,
              label: (
                <span style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', width: '100%' }}>
                  <span>{t(item.label)}</span>
                  {item.key === 'queue' && runningCount > 0 && (
                    <span style={{
                      backgroundColor: 'var(--ou-primary)',
                      color: 'var(--ou-text-white)',
                      fontSize: 10,
                      fontWeight: 600,
                      borderRadius: 10,
                      padding: '1px 6px',
                      lineHeight: '14px',
                    }}>
                      {runningCount}
                    </span>
                  )}
                </span>
              ),
            }))}
            onClick={({ key }) => setSelectedKey(key)}
          />
          {!compact && (
            <div className="sidebar-telemetry">
              <div className="sidebar-telemetry-item">
                <span className="mono-text" style={{ fontSize: 11 }}>
                  {t('缓存占用: {size}', { size: formatBytes(snapshot.cacheUsedBytes) })}
                </span>
                <span style={{ display: 'inline-flex', alignItems: 'center', gap: 4 }}>
                  {snapshot.engineAvailable ? (
                    <CheckCircleOutlined style={{ color: 'var(--ou-primary)', fontSize: 11 }} />
                  ) : (
                    <CloseCircleOutlined style={{ color: 'var(--ou-danger)', fontSize: 11 }} />
                  )}
                  <span style={{ fontSize: 11 }}>{snapshot.engineAvailable ? t('就绪') : t('失败')}</span>
                </span>
              </div>
            </div>
          )}
        </Sider>
        <Layout className="main-layout">
          <Header className="header-bar">
            <h2>{t(titleMap[selectedKey] || 'OpenUnpack')}</h2>
            <div className="header-actions">
              {snapshot.running ? (
                <Tag color="processing">{t('处理中 ({count})', { count: runningCount })}</Tag>
              ) : snapshot.paused ? (
                <Tag color="warning">{t('已暂停')}</Tag>
              ) : (
                <Tag color="default">{t('就绪')}</Tag>
              )}
              <Button aria-label={t('刷新状态')} icon={<ReloadOutlined aria-hidden />} onClick={() => void refresh()}>
                {t('刷新')}
              </Button>
            </div>
          </Header>
          <Content className="page-content" style={{ padding: 20, overflow: 'auto' }}>
            {!isDesktop && !daemon && (
              <Alert
                title={t('浏览器预览模式')}
                description={t('解压引擎与本地密码库需在桌面客户端中运行。')}
                type="info"
                showIcon
                closable
                style={{ marginBottom: 16 }}
              />
            )}
            {snapshot.vaultError && (
              <Alert
                title={t('密码库状态提示')}
                description={humanizeVaultError(snapshot.vaultError, t)}
                type="warning"
                showIcon
                style={{ marginBottom: 16 }}
              />
            )}
            {localError && (
              <Alert
                title={t('操作提示')}
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
            <span>{t('缓存占用: {size}', { size: formatBytes(snapshot.cacheUsedBytes) })}</span>
            <span>{t('数据目录: {path}', { path: snapshot.dataDirectory || t('默认') })}</span>
            <span>{t('任务总数: {count} 项', { count: snapshot.jobs.length })}</span>
            {snapshot.recoverableCount > 0 && <span>{t('可恢复: {count}', { count: snapshot.recoverableCount })}</span>}
          </Footer>
        </Layout>
      </Layout>
      <Modal
        open={!!countdownState}
        title={countdownState?.action === 'shutdown' ? t('任务完成，即将自动关机') : t('任务完成，即将进入休眠')}
        footer={[
          <Button key="cancel" onClick={() => setCountdownState(null)}>
            {t('取消操作')}
          </Button>,
          <Button
            key="confirm"
            type="primary"
            danger={countdownState?.action === 'shutdown'}
            onClick={() => {
              const act = countdownState!.action;
              setCountdownState(null);
              void api.executePostAction(act).catch((err: unknown) =>
                setLocalError(err instanceof Error ? err.message : String(err)));
            }}
          >
            {t('立即执行')}
          </Button>,
        ]}
        closable={false}
      >
        <div style={{ textAlign: 'center', padding: '24px 0' }}>
          <Typography.Title level={2} style={{ margin: 0, color: 'var(--ou-primary)' }}>
            {countdownState?.seconds}s
          </Typography.Title>
          <Typography.Paragraph type="secondary" style={{ marginTop: 12 }}>
            {countdownState?.action === 'shutdown'
              ? t('所有解压任务已完成。计算机将在倒计时结束后自动关机，按“取消操作”可终止。')
              : t('所有解压任务已完成。计算机将在倒计时结束后进入休眠，按“取消操作”可终止。')}
          </Typography.Paragraph>
        </div>
      </Modal>
      </AntApp>
    </ConfigProvider>
  );
}
