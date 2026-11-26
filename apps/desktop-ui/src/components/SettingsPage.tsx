import { t, useLanguage } from '../i18n';
import React, { useState, useEffect, useMemo, useRef } from 'react';
import { App as AntApp, Card, Tabs, Input, Button, Alert, Tag } from 'antd';
import {
  SaveOutlined,
  SafetyCertificateOutlined,
  ThunderboltOutlined,
  ClearOutlined,
} from '@ant-design/icons';
import type { AppSnapshot, Settings, ImportPreview } from '../types';
import { api, isDesktop } from '../api';
import { isExclusionPatternWithinLimit } from '../exclusion-rules';
import { useSkin, type SkinId } from '../theme';
import SettingsBackupModal from './SettingsBackupModal';

import { useBasicSettingsTabs, type SettingsTabContext } from './SettingsTabsBasic';
import { useSystemSettingsTabs, SETTING_TAB_KEYWORDS } from './SettingsTabsSystem';
interface Props {
  snapshot: AppSnapshot;
  refresh: () => Promise<void>;
  run: (work: () => Promise<unknown>) => Promise<void>;
}

export default function SettingsPage({ snapshot, refresh, run }: Props) {
  const { message } = AntApp.useApp();
  const daemon = api.isDaemonMode?.() === true;
  const connected = isDesktop || daemon;
  const nativeEngine = isDesktop && !daemon;
  const { language, setLanguage } = useLanguage();
  const { skin, setSkin, presets } = useSkin();
  const [draftSkin, setDraftSkin] = useState<SkinId>(() => skin);
  const [draft, setDraft] = useState<Settings>(() => ({ ...snapshot.settings }));
  const [savedJson, setSavedJson] = useState(() => JSON.stringify(snapshot.settings));
  const [inputExclusionsText, setInputExclusionsText] = useState(() =>
    (snapshot.settings.inputExclusions || []).join('\n'),
  );
  const [outputExclusionsText, setOutputExclusionsText] = useState(() =>
    (snapshot.settings.outputExclusions || []).join('\n'),
  );
  const [saving, setSaving] = useState(false);
  const [modalMode, setModalMode] = useState<'export' | 'restore' | null>(null);
  const [modalPath, setModalPath] = useState('');
  const [passphrase, setPassphrase] = useState('');
  const [confirmPass, setConfirmPass] = useState('');
  const [modalBusy, setModalBusy] = useState(false);
  const [restorePreview, setRestorePreview] = useState<ImportPreview | null>(null);
  const [shellMenuEnabled, setShellMenuEnabled] = useState(false);
  const [shellMenuBusy, setShellMenuBusy] = useState(false);
  const [activeTabKey, setActiveTabKey] = useState<string>('output');
  const [searchQuery, setSearchQuery] = useState('');
  const modalEpoch = useRef(0);
  const busyRef = useRef(false);
  const dirtyRef = useRef(false);

  useEffect(() => {
    if (nativeEngine && typeof api.getShellContextMenuStatus === 'function') {
      Promise.resolve(api.getShellContextMenuStatus())
        .then((val) => {
          if (typeof val === 'boolean') setShellMenuEnabled(val);
        })
        .catch(() => {});
    }
  }, [nativeEngine]);

  useEffect(() => {
    const next = JSON.stringify(snapshot.settings);
    if (next !== savedJson) {
      queueMicrotask(() => {
        setSavedJson(next);
        if (!dirtyRef.current) {
          setDraft({ ...snapshot.settings });
          setInputExclusionsText((snapshot.settings.inputExclusions || []).join('\n'));
          setOutputExclusionsText((snapshot.settings.outputExclusions || []).join('\n'));
        }
      });
    }
  }, [snapshot.settings, savedJson]);

  useEffect(() => {
    return () => {
      window.dispatchEvent(
        new CustomEvent('openunpack:theme-preview', { detail: { theme: null } }),
      );
      window.dispatchEvent(new CustomEvent('openunpack:skin-preview', { detail: { skin: null } }));
    };
  }, []);

  const handleSkinChange = (nextSkin: SkinId) => {
    setDraftSkin(nextSkin);
    window.dispatchEvent(
      new CustomEvent('openunpack:skin-preview', { detail: { skin: nextSkin } }),
    );
  };

  const update = <K extends keyof Settings>(key: K, val: Settings[K]) => {
    dirtyRef.current = true;
    setDraft((p) => ({ ...p, [key]: val }));
  };

  const passwordGroups = useMemo(() => {
    const s = new Set<string>();
    snapshot.passwords?.forEach((p) => p.groups?.forEach((g) => g && s.add(g)));
    return Array.from(s);
  }, [snapshot.passwords]);

  const handleSave = async () => {
    if (saving) return;
    const parsedInput = inputExclusionsText
      .split('\n')
      .map((s) => s.trim())
      .filter(Boolean);
    const parsedOutput = outputExclusionsText
      .split('\n')
      .map((s) => s.trim())
      .filter(Boolean);
    if (parsedInput.length > 100 || parsedOutput.length > 100) {
      message.error(t('排除规则数量不可超过 100 条'));
      return;
    }
    if ([...parsedInput, ...parsedOutput].some((p) => !isExclusionPatternWithinLimit(p))) {
      message.error(t('单条排除规则不可超过 512 UTF-8 字节'));
      return;
    }
    const nextDraft: Settings = {
      ...draft,
      inputExclusions: parsedInput,
      outputExclusions: parsedOutput,
    };
    setSaving(true);
    try {
      await run(async () => {
        await api.saveSettings(nextDraft);
        dirtyRef.current = false;
        setSavedJson(JSON.stringify(nextDraft));
        setSkin(draftSkin);
        window.dispatchEvent(
          new CustomEvent('openunpack:theme-preview', { detail: { theme: null } }),
        );
        window.dispatchEvent(
          new CustomEvent('openunpack:skin-preview', { detail: { skin: null } }),
        );
        message.success(t('设置已保存'));
        await refresh();
      });
    } finally {
      setSaving(false);
    }
  };

  const handlePickOutput = async () => {
    await run(async () => {
      const dir = await api.pickDirectory();
      if (dir) update('outputRoot', dir);
    });
  };

  const handlePickCache = async () => {
    await run(async () => {
      const dir = await api.pickDirectory();
      if (dir) update('cacheRoot', dir);
    });
  };

  const startExport = async () => {
    const epoch = ++modalEpoch.current;
    await run(async () => {
      const p = await api.pickSaveBackup();
      if (p && modalEpoch.current === epoch) {
        setRestorePreview(null);
        setModalPath(p);
        setPassphrase('');
        setConfirmPass('');
        setModalMode('export');
      }
    });
  };

  const startRestore = async () => {
    const epoch = ++modalEpoch.current;
    await run(async () => {
      const p = await api.pickOpenBackup();
      if (p && modalEpoch.current === epoch) {
        setRestorePreview(null);
        setModalPath(p);
        setPassphrase('');
        setConfirmPass('');
        setModalMode('restore');
      }
    });
  };

  const closeModal = () => {
    modalEpoch.current += 1;
    setModalMode(null);
    setModalPath('');
    setPassphrase('');
    setConfirmPass('');
    setModalBusy(false);
    setRestorePreview(null);
  };
  useEffect(
    () => () => {
      modalEpoch.current += 1;
    },
    [],
  );

  const submitModal = async () => {
    if (busyRef.current) return;
    busyRef.current = true;
    const epoch = modalEpoch.current;
    setModalBusy(true);
    try {
      if (modalMode === 'export') {
        await run(async () => {
          await api.exportBackup(modalPath, passphrase);
          message.success(t('备份已成功导出'));
          if (epoch === modalEpoch.current) closeModal();
        });
      } else if (modalMode === 'restore') {
        await run(async () => {
          if (!restorePreview) {
            const preview = await api.previewRestoreBackup(modalPath, passphrase);
            if (epoch === modalEpoch.current) setRestorePreview(preview);
          } else {
            const res = await api.confirmRestoreBackup(modalPath, passphrase, restorePreview.token);
            message.success(
              t('备份已合并：新增 {value0} 条，重复 {value1} 条', {
                value0: res.added,
                value1: res.duplicates,
              }),
            );
            await refresh();
            if (epoch === modalEpoch.current) closeModal();
          }
        });
      }
    } finally {
      busyRef.current = false;
      setModalBusy(false);
    }
  };

  const tabCtx: SettingsTabContext = {
    snapshot,
    draft,
    daemon,
    connected,
    nativeEngine,
    passwordGroups,
    inputExclusionsText,
    outputExclusionsText,
    draftSkin,
    presets,
    language,
    shellMenuEnabled,
    shellMenuBusy,
    update,
    setInputExclusionsText,
    setOutputExclusionsText,
    handlePickOutput,
    handlePickCache,
    handleSkinChange,
    setLanguage,
    setShellMenuEnabled,
    setShellMenuBusy,
    startExport,
    startRestore,
    run,
    message,
  };
  const tabItems = [...useBasicSettingsTabs(tabCtx), ...useSystemSettingsTabs(tabCtx)];

  const matchingTabs = useMemo(() => {
    const q = searchQuery.trim().toLowerCase();
    if (!q) return new Set<string>();
    const matched = new Set<string>();
    for (const [key, words] of Object.entries(SETTING_TAB_KEYWORDS)) {
      if (words.some((w) => w.includes(q) || q.includes(w))) matched.add(key);
    }
    return matched;
  }, [searchQuery]);

  const handleSearch = (q: string) => {
    setSearchQuery(q);
    const trimmed = q.trim().toLowerCase();
    if (!trimmed) return;
    for (const [key, words] of Object.entries(SETTING_TAB_KEYWORDS)) {
      if (words.some((w) => w.includes(trimmed) || trimmed.includes(w))) {
        setActiveTabKey(key);
        break;
      }
    }
  };

  const decoratedTabs = tabItems.map((item) => {
    const isMatched = matchingTabs.has(item.key);
    return {
      ...item,
      label: isMatched ? (
        <span style={{ fontWeight: 600, color: 'var(--ou-primary)' }}>
          {item.label}
          <Tag color="processing" style={{ marginLeft: 6, fontSize: 10, padding: '0 4px' }}>
            {t('匹配')}
          </Tag>
        </span>
      ) : (
        item.label
      ),
    };
  });

  return (
    <div style={{ maxWidth: 940, margin: '0 auto', paddingBottom: 24 }}>
      <div className="page-header" style={{ marginBottom: 14 }}>
        <div>
          <h2 className="page-title">{t('参数设置')}</h2>
          <p className="page-description">
            {t('配置解压策略、密码组、缓存目录及密码库备份与恢复')}
          </p>
        </div>
        <Button
          type="primary"
          icon={<SaveOutlined />}
          loading={saving}
          disabled={!connected}
          onClick={() => void handleSave()}
        >
          {t('保存设置')}
        </Button>
      </div>

      {snapshot.running && (
        <Alert
          title={t('提示：当前任务使用已冻结设置，新任务使用保存值。')}
          type="warning"
          showIcon
          style={{ marginBottom: 14 }}
        />
      )}

      <div style={{ display: 'flex', gap: 10, alignItems: 'center', marginBottom: 12 }}>
        <Input.Search
          aria-label={t('搜索设置项')}
          placeholder={t('搜索设置项，例如：密码、输出、删除源包、线程、缓存、乱码...')}
          allowClear
          value={searchQuery}
          onChange={(e) => handleSearch(e.target.value)}
          style={{ flex: 1 }}
        />
      </div>
      <div className="settings-presets-bar">
        <span style={{ fontSize: 12, color: 'var(--ou-text-muted)', fontWeight: 500 }}>
          {t('快速推荐预设：')}
        </span>
        <Button
          size="small"
          icon={<ThunderboltOutlined />}
          onClick={() => {
            dirtyRef.current = true;
            setDraft((p) => ({
              ...p,
              concurrency: 3,
              verification: 'standard',
              autoStartOnAdd: true,
              salvageCorrupted: true,
              cacheEnabled: true,
            }));
            message.success(t('已应用「极速解压」预设配置，请点击右上角保存。'));
          }}
        >
          {t('🚀 极速解压')}
        </Button>
        <Button
          size="small"
          icon={<SafetyCertificateOutlined />}
          onClick={() => {
            dirtyRef.current = true;
            setDraft((p) => ({
              ...p,
              concurrency: 1,
              verification: 'strict',
              salvageCorrupted: true,
              sourceCleanup: 'keep',
            }));
            message.success(t('已应用「安全保真」预设配置，请点击右上角保存。'));
          }}
        >
          {t('🛡️ 安全保真')}
        </Button>
        <Button
          size="small"
          icon={<ClearOutlined />}
          onClick={() => {
            dirtyRef.current = true;
            setDraft((p) => ({
              ...p,
              unwrapSingleRoot: true,
              cleanEmptyDirs: true,
              autoFixEncoding: true,
              sourceCleanup: 'recycle',
            }));
            message.success(t('已应用「省空间与整理」预设配置，请点击右上角保存。'));
          }}
        >
          {t('🧹 省空间整理')}
        </Button>
      </div>

      <Card
        style={{ borderRadius: 'var(--ou-radius)', border: '1px solid var(--ou-border-color)' }}
      >
        <Tabs activeKey={activeTabKey} onChange={setActiveTabKey} items={decoratedTabs} />
      </Card>

      <SettingsBackupModal
        modalMode={modalMode}
        modalPath={modalPath}
        passphrase={passphrase}
        confirmPass={confirmPass}
        modalBusy={modalBusy}
        restorePreview={restorePreview}
        onChangePassphrase={(nextPassphrase) => {
          modalEpoch.current += 1;
          setRestorePreview(null);
          setPassphrase(nextPassphrase);
        }}
        onChangeConfirmPass={setConfirmPass}
        onSubmit={() => void submitModal()}
        onCancel={closeModal}
      />
    </div>
  );
}
