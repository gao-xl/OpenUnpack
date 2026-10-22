import { t, getLanguage } from '../i18n';
import { useState, useEffect, useRef, useMemo, useDeferredValue } from 'react';
import {
  Table, Button, Input, Modal, Form, Select, Switch, Space, Tag, Popconfirm, Typography, Alert, Card,
} from 'antd';
import {
  PlusOutlined, ImportOutlined, EyeOutlined, EditOutlined, DeleteOutlined, CopyOutlined,
  SearchOutlined, FolderOpenOutlined,
} from '@ant-design/icons';
import type { AppSnapshot, PasswordView, PasswordInput, ImportPreview } from '../types';
import { api, isDesktop } from '../api';
import BulkPasswordBar from './BulkPasswordBar';

const { Text, Paragraph } = Typography;

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
    const existingIds = new Set(snapshot.passwords.map(password => password.id));
    queueMicrotask(() => {
      setSelected(previous => {
        const next = previous.filter(id => existingIds.has(id));
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
    savingRef.current = true; setSaving(true);
    void run(async () => {
      await api.bulkPasswordAction(selected, action === 'setGroups' ? 'group' : action, groups);
      setSelected([]);
    }).finally(() => { savingRef.current = false; setSaving(false); });
  };
  const copyPassword = (item: PasswordView) => void run(async () => {
    await api.copyPassword(item.id);
    setCopyNotice(t("密码已复制，剪贴板将在 30 秒后自动清除（已排除系统历史与云同步；第三方工具可能不受约束）。"));
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
    const countdown = setInterval(() => setRevealSeconds(seconds => Math.max(0, seconds - 1)), 1000);
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
        if (revealEpochRef.current === epoch) setRevealError(t("密码读取失败，请关闭后重试。"));
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
    savingRef.current = true; setSaving(true);
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
    }); } finally { savingRef.current = false; setSaving(false); }
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
    savingRef.current = true; setSaving(true);
    void run(() => api.savePassword({
      id: item.id, name: item.name, groups: item.groups, notes: item.notes, enabled,
    })).finally(() => { savingRef.current = false; setSaving(false); });
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
        if (epoch === previewEpochRef.current) { importDraftEpochRef.current += 1; setImportText(content); setPreview(null); }
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
    savingRef.current = true; setSaving(true);
    try {
      await run(async () => {
        await api.confirmImport(importText, importGroup, importFormat, preview.token);
        if (importDraftEpochRef.current === draftEpoch) clearImport();
      });
    } finally { savingRef.current = false; setSaving(false); }
  };

  const filteredPasswords = useMemo(() => {
    const q = deferredSearch.trim().toLowerCase();
    return snapshot.passwords.filter(
      (p) => (!groupFilter || p.groups.includes(groupFilter)) && (enabledFilter === 'all' || p.enabled === (enabledFilter === 'enabled')) && (!q || p.name.toLowerCase().includes(q)
        || p.groups.some((g) => g.toLowerCase().includes(q))
        || (p.notes && p.notes.toLowerCase().includes(q))),
    );
  }, [snapshot.passwords, deferredSearch, groupFilter, enabledFilter]);

  const columns = [
    {
      title: t("状态"),
      dataIndex: 'enabled',
      key: 'enabled',
      width: 80,
      render: (enabled: boolean, r: PasswordView) => (
        <Switch size="small" disabled={!connected || saving || !!snapshot.vaultError} checked={enabled} onChange={(val) => handleToggle(r, val)} aria-label={t("切换 {p0} 启用状态", { p0: r.name })} />
      ),
    },
    { title: t("名称/标识"), dataIndex: 'name', key: 'name', ellipsis: true },
    {
      title: t("分组"),
      dataIndex: 'groups',
      key: 'groups',
      render: (groups: string[]) => groups.map((g) => <Tag key={g}>{g}</Tag>),
    },
    { title: t("备注"), dataIndex: 'notes', key: 'notes', ellipsis: true, render: (t: string) => t || '-' },
    {
      title: t("成功次数"),
      dataIndex: 'successCount',
      key: 'successCount',
      width: 90,
      render: (count: number) => <span className="tabular-nums mono-text">{count}</span>,
    },
    {
      title: t("上次使用"),
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
      title: t("操作"),
      key: 'action',
      width: 175,
      render: (_: unknown, r: PasswordView) => (
        <Space size="small">
          <Button size="small" icon={<CopyOutlined />} disabled={!nativeEngine || !!snapshot.vaultError} onClick={() => copyPassword(r)} aria-label={t("复制 {p0} 密码", { p0: r.name })} />
          <Button
            size="small"
            icon={<EyeOutlined />}
            disabled={!connected || !!snapshot.vaultError}
            onClick={() => handleReveal(r)}
            aria-label={t("查看密码")}
          />
          <Button size="small" disabled={!connected || saving || !!snapshot.vaultError} icon={<EditOutlined />} onClick={() => openEdit(r)} aria-label={t("编辑 {p0}", { p0: r.name })} />
          <Popconfirm disabled={!connected || saving || !!snapshot.vaultError} title={t("确认删除该密码条目吗？")} onConfirm={() => void run(() => api.deletePassword(r.id))}>
            <Button size="small" disabled={!connected || saving || !!snapshot.vaultError} danger icon={<DeleteOutlined />} aria-label={t("删除 {p0}", { p0: r.name })} />
          </Popconfirm>
        </Space>
      ),
    },
  ];

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 14, padding: 16 }}>
      <div className="page-header">
        <div>
          <h2 className="page-title">{t("密码库")}</h2>
          <p className="page-description">
            {t("按分组管理候选密码。列表与处理记录只显示标识，密码由当前 Windows 账户保护。")}
          </p>
        </div>
      </div>
      <div className="toolbar">
        <Input
          prefix={<SearchOutlined />}
          placeholder={t("按名称、分组或备注搜索...")}
          value={search}
          onChange={(e) => setSearch(e.target.value)}
          style={{ maxWidth: 360 }}
          allowClear
          aria-label={t("搜索密码条目")}
        />
        <Space wrap>
          <Select aria-label={t("密码分组筛选")} value={groupFilter} onChange={setGroupFilter} style={{ width: 150 }} options={[{ value: '', label: t("全部分组") }, ...[...new Set(snapshot.passwords.flatMap(password => password.groups))].map(group => ({ value: group, label: group }))]} />
          <Select aria-label={t("密码启用状态筛选")} value={enabledFilter} onChange={setEnabledFilter} style={{ width: 120 }} options={[{ value: 'all', label: t("全部状态") }, { value: 'enabled', label: t("已启用") }, { value: 'disabled', label: t("已停用") }]} />
          <Button disabled={!connected || !!snapshot.vaultError} icon={<ImportOutlined />} onClick={() => setIsImportOpen(true)}>{t("批量导入")}</Button>
          <Button disabled={!connected || !!snapshot.vaultError} type="primary" icon={<PlusOutlined />} onClick={openAdd}>{t("添加密码")}</Button>
        </Space>
      </div>

      {copyNotice && <Alert type="info" showIcon closable title={t("临时剪贴板")} description={copyNotice} onClose={() => setCopyNotice('')} />}
      <BulkPasswordBar selectedCount={selected.length} disabled={!connected || !!snapshot.vaultError} busy={saving} groups={[...new Set(snapshot.passwords.flatMap(password => password.groups))]} onAction={bulkAction} onClear={() => setSelected([])} />

      <Table<PasswordView>
        rowKey="id"
        columns={columns}
        dataSource={filteredPasswords}
        pagination={{ pageSize: 15, showTotal: (total) => t("共 {p0} 条", { p0: total }) }}
        scroll={{ x: 1050 }}
        size="small"
        rowSelection={{ selectedRowKeys: selected, onChange: keys => setSelected(keys.map(String)), getCheckboxProps: () => ({ disabled: !isDesktop || saving || !!snapshot.vaultError }) }}
      />

      <Modal
        title={editingItem ? t("编辑密码条目") : t("新建密码条目")}
        open={isEditOpen}
        onOk={() => void handleSave()}
        confirmLoading={saving}
        okText={t("保存")}
        cancelText={t("取消")}
        onCancel={clearEdit}
        destroyOnHidden
      >
        <Form form={form} layout="vertical" preserve={false} onValuesChange={() => { editDraftEpochRef.current += 1; }}>
          <Form.Item name="name" label={t("名称 / 标识")} rules={[{ required: true, message: t("请输入密码名称或标识") }]}>
            <Input placeholder={t("例如：日常解压密码")} maxLength={100} />
          </Form.Item>
          <Form.Item
            name="secret"
            label={t("密码明文")}
            rules={[...(editingItem ? [] : [{ required: true, message: t("请输入密码") }]), { validator: (_rule, value: string | undefined) => value && /[\0\r\n]/.test(value) ? Promise.reject(new Error(t("密码不支持 NUL、换行或回车。"))) : Promise.resolve() }]}
            extra={t("{p0}首尾空格与大小写会完整保留；仅支持单行密码。", { p0: editingItem ? t("留空保持原密码。") : '' })}
          >
            <Input.Password placeholder={t("密码明文，保存后加密存储")} visibilityToggle={{ visible: secretVisible, onVisibleChange: setSecretVisible }} autoComplete="new-password" />
          </Form.Item>
          <Form.Item name="groups" label={t("分组标签")}>
            <Select mode="tags" placeholder={t("选择或输入标签后回车")} tokenSeparators={[',']} options={[...new Set(snapshot.passwords.flatMap(password => password.groups))].map(group => ({ value: group, label: group }))} />
          </Form.Item>
          <Form.Item name="notes" label={t("备注信息")}>
            <Input.TextArea rows={2} placeholder={t("来源说明或其他备注")} maxLength={500} />
          </Form.Item>
          <Form.Item name="enabled" label={t("启用状态")} valuePropName="checked">
            <Switch checkedChildren={t("已启用")} unCheckedChildren={t("已停用")} />
          </Form.Item>
        </Form>
      </Modal>

      <Modal
        title={t("查看密码 - {p0}", { p0: revealTarget?.name ?? '' })}
        open={Boolean(revealTarget)}
        onCancel={closeReveal}
        footer={[<Button key="close" onClick={closeReveal}>{t("关闭")}</Button>]}
        destroyOnHidden
      >
        <Alert
          type="warning"
          showIcon
          title={t("安全性提示")}
          description={t("密码将在 {p0} 秒后隐藏。窗口失焦或隐藏时立即清空显示。", { p0: revealSeconds })}
          style={{ marginBottom: 16 }}
        />
        <div style={{ textAlign: 'center', padding: '12px 0' }}>
          {revealedSecret !== null ? (
            <Paragraph
              style={{ fontSize: 18, fontFamily: 'monospace', margin: 0 }}
            >
              <Text code strong style={{ fontSize: 16 }}>{revealedSecret}</Text>
            </Paragraph>
          ) : revealError ? <Text type="danger">{revealError}</Text> : (
            <Text type="secondary">{t("正在读取安全密码...")}</Text>
          )}
        </div>
      </Modal>

      <Modal
        title={t("批量导入密码")}
        open={isImportOpen}
        width={720}
        destroyOnHidden
        onCancel={clearImport}
        footer={[
          <Button key="cancel" onClick={clearImport}>{t("取消")}</Button>,
          <Button
            key="confirm"
            type="primary"
            disabled={!connected || !preview?.token || preview.errorCount > 0 || saving}
            loading={saving}
            onClick={() => void handleConfirmImport()}
          >{preview ? t("确认导入 ({count} 新增)", { count: preview.newCount }) : t("确认导入")}
          </Button>,
        ]}
      >
        <Space orientation="vertical" style={{ width: '100%' }} size="middle">
          <Space wrap>
            <span>{t("格式：")}</span>
            <Select
              aria-label={t("导入文本格式")}
              value={importFormat}
              onChange={(val) => { importDraftEpochRef.current += 1; previewEpochRef.current += 1; setImportFormat(val); setPreview(null); }}
              options={[
                { label: t("TXT：每行一个密码"), value: 'txt' },
                { label: t("TSV：名称、密码、分组、备注"), value: 'tsv' },
                { label: t("CSV：逗号分隔 / 自定义列映射"), value: 'csv' },
              ]}
              style={{ width: 240 }}
            />
            <span>{t("默认分组：")}</span>
            <Input
              aria-label={t("导入默认分组")}
              value={importGroup}
              placeholder={t("默认分组（可选）")}
              onChange={(e) => { importDraftEpochRef.current += 1; previewEpochRef.current += 1; setImportGroup(e.target.value); setPreview(null); }}
              style={{ width: 140 }}
            />
            <span>{t("读取前选择编码：")}</span>
            <Select
              aria-label={t("导入文件编码")}
              value={importEncoding}
              onChange={value => { importDraftEpochRef.current += 1; previewEpochRef.current += 1; setImportEncoding(value); setPreview(null); }}
              options={[{ label: 'UTF-8', value: 'utf-8' }, { label: 'GB18030', value: 'gb18030' }, { label: 'UTF-16LE', value: 'utf-16le' }]}
              style={{ width: 110 }}
            />
            <Button icon={<FolderOpenOutlined />} disabled={!nativeEngine} onClick={() => void handlePickFile()}>{t("选择文件")}</Button>
          </Space>

          <Input.TextArea
            aria-label={t("待导入密码文本")}
            rows={5}
            placeholder={t("粘贴待导入密码内容...")}
            value={importText}
            onChange={(e) => { importDraftEpochRef.current += 1; previewEpochRef.current += 1; setImportText(e.target.value); setPreview(null); }}
          />
          <Alert
            type="info"
            showIcon
            title={t("密码按原样保存")}
            description={
              importFormat === 'txt'
                ? t("TXT 格式每行一个密码，空行自动忽略；首尾空格与大小写完整保留。")
                : importFormat === 'tsv'
                ? t("TSV 格式每行四列，使用 Tab 分隔：名称、密码、分组、备注；分组可用逗号分隔。无需包含表头。")
                : t("CSV 格式支持逗号分隔、带双引号转义；支持首行表头自动识别密码列，或按 [名称, 密码, 分组, 备注] 顺序排列。")
            }
          />

          <Button type="dashed" block onClick={handlePreviewImport} disabled={!connected || !importText.length}>{t("生成导入预览")}</Button>

          {preview && (
            <Card size="small" title={t("预览结果")}>
              <Space style={{ marginBottom: 8 }}>
                <Tag color="green">{t("新增: {count}", { count: preview.newCount })}</Tag>
                <Tag color="orange">{t("重复: {count}", { count: preview.duplicateCount })}</Tag>
                <Tag color={preview.errorCount > 0 ? 'red' : 'default'}>{t("错误: {count}", { count: preview.errorCount })}</Tag>
              </Space>
              {preview.errorCount > 0 && (
                <Alert
                  type="error"
                  title={t("存在解析错误条目，必须先纠正或清理错误行方可执行导入。")}
                  style={{ marginBottom: 8 }}
                />
              )}
              <Table
                rowKey={(r) => `${r.line}-${r.name}`}
                dataSource={preview.rows}
                size="small"
                pagination={{ pageSize: 5 }}
                columns={[
                  { title: t("行号"), dataIndex: 'line', key: 'line', width: 60 },
                  { title: t("标识/名称"), dataIndex: 'name', key: 'name' },
                  { title: t("分组"), dataIndex: 'group', key: 'group' },
                  { title: t("状态"), dataIndex: 'status', key: 'status', width: 80 },
                  { title: t("说明"), dataIndex: 'message', key: 'message' },
                ]}
              />
            </Card>
          )}
        </Space>
      </Modal>
    </div>
  );
}
