import { t, getLanguage } from '../i18n';
import { useState, useEffect, useRef, useMemo, useDeferredValue } from 'react';
import {
  Table,
  Button,
  Input,
  Form,
  Select,
  Switch,
  Space,
  Tag,
  Popconfirm,
  Alert,
  Segmented,
  Empty,
} from 'antd';
import {
  PlusOutlined,
  ImportOutlined,
  EyeOutlined,
  EditOutlined,
  DeleteOutlined,
  CopyOutlined,
  SearchOutlined,
  AppstoreOutlined,
  BarsOutlined,
} from '@ant-design/icons';
import type { AppSnapshot, PasswordView, PasswordInput, ImportPreview } from '../types';
import { api, isDesktop } from '../api';
import BulkPasswordBar from './BulkPasswordBar';
import VaultCardGrid from './VaultCardGrid';
import VaultModals from './VaultModals';
export interface VaultPageProps {
  snapshot: AppSnapshot;
  refresh: () => Promise<void>;
  run: (work: () => Promise<unknown>) => Promise<void>;
}

export default function VaultPage({ snapshot, run }: VaultPageProps) {
  const daemon = api.isDaemonMode?.() === true;
  const connected = isDesktop || daemon;
  const nativeEngine = isDesktop && !daemon;
  const [search, setSearch] = useState('');
  const deferredSearch = useDeferredValue(search);
  const [groupFilter, setGroupFilter] = useState('');
  const [enabledFilter, setEnabledFilter] = useState('all');
  const [saving, setSaving] = useState(false);
  const [selected, setSelected] = useState<string[]>([]);
  const [viewMode, setViewMode] = useState<'table' | 'cards'>('table');
  const [copyNotice, setCopyNotice] = useState('');
  const [secretVisible, setSecretVisible] = useState(false);
  const savingRef = useRef(false);
  const [isEditOpen, setIsEditOpen] = useState(false);
  const [editingItem, setEditingItem] = useState<PasswordView | null>(null);
  const [form] = Form.useForm<PasswordInput>();
  const editDraftEpochRef = useRef(0);

  const [revealTarget, setRevealTarget] = useState<PasswordView | null>(null);
  const [revealedSecret, setRevealedSecret] = useState<string | null>(null);
  const [revealError, setRevealError] = useState<string | null>(null);
  const [revealSeconds, setRevealSeconds] = useState(20);
  const revealEpochRef = useRef(0);

  const [isImportOpen, setIsImportOpen] = useState(false);
  const [importFormat, setImportFormat] = useState<'txt' | 'tsv' | 'csv'>('txt');
  const [importGroup, setImportGroup] = useState('');
  const [importEncoding, setImportEncoding] = useState('utf-8');
  const [importText, setImportText] = useState('');
  const [preview, setPreview] = useState<ImportPreview | null>(null);
  const previewEpochRef = useRef(0);
  // Bumped whenever the import modal is reset/closed; a late confirm may only
  // clear the draft it was started from, never a newer one.
  const importDraftEpochRef = useRef(0);

  useEffect(() => {
    const existingIds = new Set(snapshot.passwords.map((password) => password.id));
    queueMicrotask(() => {
      setSelected((previous) => {
        const next = previous.filter((id) => existingIds.has(id));
        return next.length === previous.length ? previous : next;
      });
    });
  }, [snapshot.passwords]);
  useEffect(() => {
    const hide = () => setSecretVisible(false);
    window.addEventListener('blur', hide);
    return () => {
      revealEpochRef.current += 1;
      previewEpochRef.current += 1;
      importDraftEpochRef.current += 1;
      editDraftEpochRef.current += 1;
      form.resetFields();
      window.removeEventListener('blur', hide);
    };
  }, [form]);
  const bulkAction = (action: 'enable' | 'disable' | 'delete' | 'setGroups', groups?: string[]) => {
    if (savingRef.current || !selected.length) return;
    savingRef.current = true;
    setSaving(true);
    void run(async () => {
      await api.bulkPasswordAction(selected, action === 'setGroups' ? 'group' : action, groups);
      setSelected([]);
    }).finally(() => {
      savingRef.current = false;
      setSaving(false);
    });
  };
  const copyPassword = (item: PasswordView) =>
    void run(async () => {
      await api.copyPassword(item.id);
      setCopyNotice(
        t(
          '密码已复制，剪贴板将在 30 秒后自动清除（已排除系统历史与云同步；第三方工具可能不受约束）。',
        ),
      );
    });

  const closeReveal = () => {
    revealEpochRef.current += 1;
    setRevealTarget(null);
    setRevealedSecret(null);
    setRevealError(null);
  };

  useEffect(() => {
    if (!revealTarget) return;
    queueMicrotask(() => setRevealSeconds(20));
    const countdown = setInterval(
      () => setRevealSeconds((seconds) => Math.max(0, seconds - 1)),
      1000,
    );
    const timer = setTimeout(closeReveal, 20000);
    const onBlurOrHide = () => closeReveal();
    window.addEventListener('blur', onBlurOrHide);
    document.addEventListener('visibilitychange', onBlurOrHide);
    return () => {
      clearTimeout(timer);
      clearInterval(countdown);
      window.removeEventListener('blur', onBlurOrHide);
      document.removeEventListener('visibilitychange', onBlurOrHide);
    };
  }, [revealTarget]);

  const handleReveal = (item: PasswordView) => {
    if (!connected) return;
    const epoch = ++revealEpochRef.current;
    setRevealTarget(item);
    setRevealedSecret(null);
    setRevealError(null);
    void run(async () => {
      try {
        const secret = await api.revealPassword(item.id);
        if (revealEpochRef.current === epoch) setRevealedSecret(secret);
      } catch (error) {
        if (revealEpochRef.current === epoch) setRevealError(t('密码读取失败，请关闭后重试。'));
        throw error;
      }
    });
  };

  const openAdd = () => {
    editDraftEpochRef.current += 1;
    setEditingItem(null);
    form.resetFields();
    form.setFieldsValue({ enabled: true, groups: [] });
    setIsEditOpen(true);
    setSecretVisible(false);
  };

  const openEdit = (item: PasswordView) => {
    editDraftEpochRef.current += 1;
    setEditingItem(item);
    form.resetFields();
    form.setFieldsValue({
      name: item.name,
      groups: item.groups,
      notes: item.notes,
      enabled: item.enabled,
      secret: '',
    });
    setIsEditOpen(true);
    setSecretVisible(false);
  };

  const handleSave = async () => {
    if (savingRef.current) return;
    const epoch = editDraftEpochRef.current;
    const editingId = editingItem?.id;
    savingRef.current = true;
    setSaving(true);
    try {
      const values = await form.validateFields().catch(() => null);
      if (!values || editDraftEpochRef.current !== epoch) return;
      await run(async () => {
        await api.savePassword({
          id: editingId,
          name: values.name.trim(),
          groups: values.groups || [],
          notes: values.notes || '',
          enabled: values.enabled ?? true,
          secret: values.secret ? values.secret : undefined,
        });
        if (editDraftEpochRef.current === epoch) clearEdit();
      });
    } finally {
      savingRef.current = false;
      setSaving(false);
    }
  };

  const clearEdit = () => {
    editDraftEpochRef.current += 1;
    setIsEditOpen(false);
    setEditingItem(null);
    setSecretVisible(false);
    form.resetFields();
  };

  const handleToggle = (item: PasswordView, enabled: boolean) => {
    if (savingRef.current) return;
    savingRef.current = true;
    setSaving(true);
    void run(() =>
      api.savePassword({
        id: item.id,
        name: item.name,
        groups: item.groups,
        notes: item.notes,
        enabled,
      }),
    ).finally(() => {
      savingRef.current = false;
      setSaving(false);
    });
  };

  const clearImport = () => {
    importDraftEpochRef.current += 1;
    previewEpochRef.current += 1;
    setIsImportOpen(false);
    setImportText('');
    setImportGroup('');
    setPreview(null);
  };

  const handlePickFile = async () => {
    if (!isDesktop) return;
    importDraftEpochRef.current += 1;
    const epoch = ++previewEpochRef.current;
    await run(async () => {
      const paths = await api.pickFiles(false);
      if (paths && paths[0]) {
        const content = await api.readImportFile(paths[0], importEncoding);
        if (epoch === previewEpochRef.current) {
          importDraftEpochRef.current += 1;
          setImportText(content);
          setPreview(null);
        }
      }
    });
  };

  const handlePreviewImport = () => {
    if (!importText.length) return;
    const epoch = ++previewEpochRef.current;
    const curText = importText;
    void run(async () => {
      const res = await api.previewImport(curText, importGroup, importFormat);
      if (previewEpochRef.current === epoch && importText === curText) setPreview(res);
    });
  };

  const handleConfirmImport = async () => {
    if (!preview?.token || preview.errorCount > 0) return;
    if (savingRef.current) return;
    const draftEpoch = importDraftEpochRef.current;
    savingRef.current = true;
    setSaving(true);
    try {
      await run(async () => {
        await api.confirmImport(importText, importGroup, importFormat, preview.token);
        if (importDraftEpochRef.current === draftEpoch) clearImport();
      });
    } finally {
      savingRef.current = false;
      setSaving(false);
    }
  };

  const filteredPasswords = useMemo(() => {
    const q = deferredSearch.trim().toLowerCase();
    return snapshot.passwords.filter(
      (p) =>
        (!groupFilter || p.groups.includes(groupFilter)) &&
        (enabledFilter === 'all' || p.enabled === (enabledFilter === 'enabled')) &&
        (!q ||
          p.name.toLowerCase().includes(q) ||
          p.groups.some((g) => g.toLowerCase().includes(q)) ||
          (p.notes && p.notes.toLowerCase().includes(q))),
    );
  }, [snapshot.passwords, deferredSearch, groupFilter, enabledFilter]);

  const columns = [
    {
      title: t('状态'),
      dataIndex: 'enabled',
      key: 'enabled',
      width: 80,
      render: (enabled: boolean, r: PasswordView) => (
        <Switch
          size="small"
          disabled={!connected || saving || !!snapshot.vaultError}
          checked={enabled}
          onChange={(val) => handleToggle(r, val)}
          aria-label={t('切换 {p0} 启用状态', { p0: r.name })}
        />
      ),
    },
    { title: t('名称/标识'), dataIndex: 'name', key: 'name', ellipsis: true },
    {
      title: t('分组'),
      dataIndex: 'groups',
      key: 'groups',
      render: (groups: string[]) => groups.map((g) => <Tag key={g}>{g}</Tag>),
    },
    {
      title: t('备注'),
      dataIndex: 'notes',
      key: 'notes',
      ellipsis: true,
      render: (t: string) => t || '-',
    },
    {
      title: t('成功次数'),
      dataIndex: 'successCount',
      key: 'successCount',
      width: 90,
      render: (count: number) => <span className="tabular-nums mono-text">{count}</span>,
    },
    {
      title: t('上次使用'),
      dataIndex: 'lastSuccess',
      key: 'lastSuccess',
      width: 170,
      render: (d: string | null) => (
        <span className="tabular-nums mono-text" style={{ fontSize: 12 }}>
          {d ? new Date(d).toLocaleString(getLanguage()) : '-'}
        </span>
      ),
    },
    {
      title: t('操作'),
      key: 'action',
      width: 175,
      render: (_: unknown, r: PasswordView) => (
        <Space size="small">
          <Button
            size="small"
            icon={<CopyOutlined />}
            disabled={!nativeEngine || !!snapshot.vaultError}
            onClick={() => copyPassword(r)}
            aria-label={t('复制 {p0} 密码', { p0: r.name })}
          />
          <Button
            size="small"
            icon={<EyeOutlined />}
            disabled={!connected || !!snapshot.vaultError}
            onClick={() => handleReveal(r)}
            aria-label={t('查看 {p0} 密码', { p0: r.name })}
          />
          <Button
            size="small"
            disabled={!connected || saving || !!snapshot.vaultError}
            icon={<EditOutlined />}
            onClick={() => openEdit(r)}
            aria-label={t('编辑 {p0}', { p0: r.name })}
          />
          <Popconfirm
            disabled={!connected || saving || !!snapshot.vaultError}
            title={t('确认删除该密码条目吗？')}
            onConfirm={() => void run(() => api.deletePassword(r.id))}
          >
            <Button
              size="small"
              disabled={!connected || saving || !!snapshot.vaultError}
              danger
              icon={<DeleteOutlined />}
              aria-label={t('删除 {p0}', { p0: r.name })}
            />
          </Popconfirm>
        </Space>
      ),
    },
  ];

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 14 }}>
      <div className="page-header">
        <div>
          <h2 className="page-title">{t('密码库')}</h2>
          <p className="page-description">
            {t('按分组管理候选密码。列表与处理记录只显示标识，密码由当前 Windows 账户保护。')}
          </p>
        </div>
      </div>
      <div className="telemetry-bar">
        <div className="telemetry-card">
          <span className="telemetry-label">{t('密码总数')}</span>
          <span className="telemetry-value tabular-nums">{snapshot.passwords.length}</span>
        </div>
        <div className="telemetry-card">
          <span className="telemetry-label">{t('启用状态')}</span>
          <span className="telemetry-value tabular-nums" style={{ color: 'var(--ou-success)' }}>
            {snapshot.passwords.filter((p) => p.enabled).length} / {snapshot.passwords.length}
          </span>
        </div>
        <div className="telemetry-card">
          <span className="telemetry-label">{t('分组数量')}</span>
          <span className="telemetry-value tabular-nums">
            {[...new Set(snapshot.passwords.flatMap((p) => p.groups))].length}
          </span>
        </div>
        <div className="telemetry-card">
          <span className="telemetry-label">{t('存储安全状态')}</span>
          <span
            style={{
              fontSize: 13,
              fontWeight: 600,
              color: snapshot.vaultError ? 'var(--ou-danger)' : 'var(--ou-success)',
              marginTop: 2,
            }}
          >
            {snapshot.vaultError ? t('需检查密码库') : t('Windows 账户加密保护')}
          </span>
        </div>
      </div>

      <div className="toolbar">
        <Input
          prefix={<SearchOutlined />}
          placeholder={t('按名称、分组或备注搜索...')}
          value={search}
          onChange={(e) => setSearch(e.target.value)}
          style={{ maxWidth: 360 }}
          allowClear
          aria-label={t('搜索密码条目')}
        />
        <Space wrap>
          <Select
            aria-label={t('密码分组筛选')}
            value={groupFilter}
            onChange={setGroupFilter}
            style={{ width: 150 }}
            options={[
              { value: '', label: t('全部分组') },
              ...[...new Set(snapshot.passwords.flatMap((password) => password.groups))].map(
                (group) => ({ value: group, label: group }),
              ),
            ]}
          />
          <Select
            aria-label={t('密码启用状态筛选')}
            value={enabledFilter}
            onChange={setEnabledFilter}
            style={{ width: 120 }}
            options={[
              { value: 'all', label: t('全部状态') },
              { value: 'enabled', label: t('已启用') },
              { value: 'disabled', label: t('已停用') },
            ]}
          />
          <Segmented
            value={viewMode}
            onChange={(v) => setViewMode(v as 'table' | 'cards')}
            options={[
              { value: 'table', icon: <BarsOutlined aria-hidden />, label: t('表格视图') },
              { value: 'cards', icon: <AppstoreOutlined aria-hidden />, label: t('卡片看板') },
            ]}
          />
          <Button
            disabled={!connected || !!snapshot.vaultError}
            icon={<ImportOutlined />}
            onClick={() => setIsImportOpen(true)}
          >
            {t('批量导入')}
          </Button>
          <Button
            disabled={!connected || !!snapshot.vaultError}
            type="primary"
            icon={<PlusOutlined />}
            onClick={openAdd}
          >
            {t('添加密码')}
          </Button>
        </Space>
      </div>

      {copyNotice && (
        <Alert
          type="info"
          showIcon
          closable
          title={t('临时剪贴板')}
          description={copyNotice}
          onClose={() => setCopyNotice('')}
        />
      )}
      <BulkPasswordBar
        selectedCount={selected.length}
        disabled={!connected || !!snapshot.vaultError}
        busy={saving}
        groups={[...new Set(snapshot.passwords.flatMap((password) => password.groups))]}
        onAction={bulkAction}
        onClear={() => setSelected([])}
      />

      {viewMode === 'table' ? (
        <Table<PasswordView>
          rowKey="id"
          columns={columns}
          dataSource={filteredPasswords}
          virtual
          pagination={{ pageSize: 15, showTotal: (total) => t('共 {p0} 条', { p0: total }) }}
          scroll={{ x: 1050, y: 460 }}
          size="small"
          rowSelection={{
            selectedRowKeys: selected,
            onChange: (keys) => setSelected(keys.map(String)),
            getCheckboxProps: () => ({ disabled: !connected || saving || !!snapshot.vaultError }),
          }}
        />
      ) : filteredPasswords.length === 0 ? (
        <Empty image={Empty.PRESENTED_IMAGE_SIMPLE} description={t('暂无匹配的密码条目')} />
      ) : (
        <VaultCardGrid
          items={filteredPasswords}
          selected={selected}
          onSelectChange={setSelected}
          connected={connected}
          nativeEngine={nativeEngine}
          saving={saving}
          vaultError={snapshot.vaultError}
          onToggle={handleToggle}
          onCopy={copyPassword}
          onReveal={handleReveal}
          onEdit={openEdit}
          run={run}
        />
      )}

      <VaultModals
        connected={connected}
        nativeEngine={nativeEngine}
        saving={saving}
        groups={[...new Set(snapshot.passwords.flatMap((password) => password.groups))]}
        isEditOpen={isEditOpen}
        editingItem={editingItem}
        form={form}
        secretVisible={secretVisible}
        onSecretVisibleChange={setSecretVisible}
        onEditValuesChange={() => {
          editDraftEpochRef.current += 1;
        }}
        onSaveEdit={handleSave}
        onCancelEdit={clearEdit}
        revealTarget={revealTarget}
        revealedSecret={revealedSecret}
        revealError={revealError}
        revealSeconds={revealSeconds}
        onCloseReveal={closeReveal}
        isImportOpen={isImportOpen}
        importFormat={importFormat}
        importGroup={importGroup}
        importEncoding={importEncoding}
        importText={importText}
        preview={preview}
        onImportFormatChange={(val) => {
          importDraftEpochRef.current += 1;
          previewEpochRef.current += 1;
          setImportFormat(val);
          setPreview(null);
        }}
        onImportGroupChange={(val) => {
          importDraftEpochRef.current += 1;
          previewEpochRef.current += 1;
          setImportGroup(val);
          setPreview(null);
        }}
        onImportEncodingChange={(val) => {
          importDraftEpochRef.current += 1;
          previewEpochRef.current += 1;
          setImportEncoding(val);
          setPreview(null);
        }}
        onImportTextChange={(val) => {
          importDraftEpochRef.current += 1;
          previewEpochRef.current += 1;
          setImportText(val);
          setPreview(null);
        }}
        onPickImportFile={handlePickFile}
        onPreviewImport={handlePreviewImport}
        onConfirmImport={handleConfirmImport}
        onCancelImport={clearImport}
      />
    </div>
  );
}
