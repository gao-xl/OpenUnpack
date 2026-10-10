import React, { useState, useEffect, useRef, useMemo } from 'react';
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
  const [search, setSearch] = useState('');
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

  const [revealTarget, setRevealTarget] = useState<PasswordView | null>(null);
  const [revealedSecret, setRevealedSecret] = useState<string | null>(null);
  const [revealSeconds, setRevealSeconds] = useState(20);
  const revealEpochRef = useRef(0);

  const [isImportOpen, setIsImportOpen] = useState(false);
  const [importFormat, setImportFormat] = useState<'txt' | 'tsv'>('txt');
  const [importGroup, setImportGroup] = useState('');
  const [importEncoding, setImportEncoding] = useState('utf-8');
  const [importText, setImportText] = useState('');
  const [preview, setPreview] = useState<ImportPreview | null>(null);
  const previewEpochRef = useRef(0);

  useEffect(() => {
    setSelected(previous => previous.filter(id => snapshot.passwords.some(password => password.id === id)));
  }, [snapshot.passwords]);
  useEffect(() => {
    const hide = () => setSecretVisible(false);
    window.addEventListener('blur', hide);
    return () => {
      revealEpochRef.current += 1;
      previewEpochRef.current += 1;
      window.removeEventListener('blur', hide);
    };
  }, []);
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
    setCopyNotice('口令已复制。30 秒后仅在剪贴板仍是本次复制内容时清除。Windows 剪贴板历史可能保留独立副本。');
  });

  const closeReveal = () => {
    revealEpochRef.current += 1;
    setRevealTarget(null);
    setRevealedSecret(null);
  };

  useEffect(() => {
    if (!revealTarget) return;
    setRevealSeconds(20);
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
    if (!isDesktop) return;
    const epoch = ++revealEpochRef.current;
    setRevealTarget(item);
    setRevealedSecret(null);
    void run(async () => {
      const secret = await api.revealPassword(item.id);
      if (revealEpochRef.current === epoch) setRevealedSecret(secret);
    });
  };

  const openAdd = () => {
    setEditingItem(null);
    form.resetFields();
    form.setFieldsValue({ enabled: true, groups: [] });
    setIsEditOpen(true);
    setSecretVisible(false);
  };

  const openEdit = (item: PasswordView) => {
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
    const values = await form.validateFields().catch(() => null);
    if (!values) return;
    savingRef.current = true; setSaving(true);
    try { await run(async () => {
      await api.savePassword({
        id: editingItem?.id,
        name: values.name.trim(),
        groups: values.groups || [],
        notes: values.notes || '',
        enabled: values.enabled ?? true,
        secret: values.secret ? values.secret : undefined,
      });
      setIsEditOpen(false);
      form.resetFields();
    }); } finally { savingRef.current = false; setSaving(false); }
  };

  const handleToggle = (item: PasswordView, enabled: boolean) => {
    if (savingRef.current) return;
    savingRef.current = true; setSaving(true);
    void run(() => api.savePassword({
      id: item.id, name: item.name, groups: item.groups, notes: item.notes, enabled,
    })).finally(() => { savingRef.current = false; setSaving(false); });
  };

  const clearImport = () => {
    previewEpochRef.current += 1;
    setIsImportOpen(false);
    setImportText('');
    setImportGroup('');
    setPreview(null);
  };

  const handlePickFile = async () => {
    if (!isDesktop) return;
    const epoch = ++previewEpochRef.current;
    await run(async () => {
      const paths = await api.pickFiles(false);
      if (paths && paths[0]) {
        const content = await api.readImportFile(paths[0], importEncoding);
        if (epoch === previewEpochRef.current) { setImportText(content); setPreview(null); }
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
    savingRef.current = true; setSaving(true);
    try { await run(async () => { await api.confirmImport(importText, importGroup, importFormat, preview.token); clearImport(); }); }
    finally { savingRef.current = false; setSaving(false); }
  };

  const filteredPasswords = useMemo(() => {
    const q = search.trim().toLowerCase();
    return snapshot.passwords.filter(
      (p) => (!groupFilter || p.groups.includes(groupFilter)) && (enabledFilter === 'all' || p.enabled === (enabledFilter === 'enabled')) && (!q || p.name.toLowerCase().includes(q)
        || p.groups.some((g) => g.toLowerCase().includes(q))
        || (p.notes && p.notes.toLowerCase().includes(q))),
    );
  }, [snapshot.passwords, search, groupFilter, enabledFilter]);

  const columns = [
    {
      title: '状态',
      dataIndex: 'enabled',
      key: 'enabled',
      width: 80,
      render: (enabled: boolean, r: PasswordView) => (
        <Switch size="small" disabled={!isDesktop || saving || !!snapshot.vaultError} checked={enabled} onChange={(val) => handleToggle(r, val)} aria-label={`切换 ${r.name} 启用状态`} />
      ),
    },
    { title: '名称/标识', dataIndex: 'name', key: 'name', ellipsis: true },
    {
      title: '分组',
      dataIndex: 'groups',
      key: 'groups',
      render: (groups: string[]) => groups.map((g) => <Tag key={g}>{g}</Tag>),
    },
    { title: '备注', dataIndex: 'notes', key: 'notes', ellipsis: true, render: (t: string) => t || '-' },
    { title: '成功次数', dataIndex: 'successCount', key: 'successCount', width: 90 },
    {
      title: '上次使用',
      dataIndex: 'lastSuccess',
      key: 'lastSuccess',
      width: 170,
      render: (d: string | null) => (d ? new Date(d).toLocaleString() : '-'),
    },
    {
      title: '操作',
      key: 'action',
      width: 175,
      render: (_: unknown, r: PasswordView) => (
        <Space size="small">
          <Button size="small" icon={<CopyOutlined />} disabled={!isDesktop || !!snapshot.vaultError} onClick={() => copyPassword(r)} aria-label={`复制 ${r.name} 口令`} />
          <Button
            size="small"
            icon={<EyeOutlined />}
            disabled={!isDesktop || !!snapshot.vaultError}
            onClick={() => handleReveal(r)}
            aria-label="查看密码"
          />
          <Button size="small" disabled={!isDesktop || saving || !!snapshot.vaultError} icon={<EditOutlined />} onClick={() => openEdit(r)} aria-label={`编辑 ${r.name}`} />
          <Popconfirm disabled={!isDesktop || saving || !!snapshot.vaultError} title="确认删除该密码条目吗？" onConfirm={() => void run(() => api.deletePassword(r.id))}>
            <Button size="small" disabled={!isDesktop || saving || !!snapshot.vaultError} danger icon={<DeleteOutlined />} aria-label={`删除 ${r.name}`} />
          </Popconfirm>
        </Space>
      ),
    },
  ];

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 16 }}>
      <div className="page-header"><Typography.Title level={3}>密码库</Typography.Title><Text type="secondary">按分组管理候选密码。列表与处理记录只显示标识，密码由当前 Windows 账户保护。</Text></div>
      <div style={{ display: 'flex', justifyContent: 'space-between', gap: 12, flexWrap: 'wrap' }}>
        <Input
          prefix={<SearchOutlined />}
          placeholder="按名称、分组或备注搜索..."
          value={search}
          onChange={(e) => setSearch(e.target.value)}
          style={{ maxWidth: 360 }}
          allowClear
          aria-label="搜索密码条目"
        />
        <Space>
          <Select aria-label="密码分组筛选" value={groupFilter} onChange={setGroupFilter} style={{ width: 150 }} options={[{ value: '', label: '全部分组' }, ...[...new Set(snapshot.passwords.flatMap(password => password.groups))].map(group => ({ value: group, label: group }))]} />
          <Select aria-label="密码启用状态筛选" value={enabledFilter} onChange={setEnabledFilter} style={{ width: 120 }} options={[{ value: 'all', label: '全部状态' }, { value: 'enabled', label: '已启用' }, { value: 'disabled', label: '已停用' }]} />
          <Button disabled={!isDesktop || !!snapshot.vaultError} icon={<ImportOutlined />} onClick={() => setIsImportOpen(true)}>批量导入</Button>
          <Button disabled={!isDesktop || !!snapshot.vaultError} type="primary" icon={<PlusOutlined />} onClick={openAdd}>添加密码</Button>
        </Space>
      </div>

      {copyNotice && <Alert type="info" showIcon closable title="临时剪贴板" description={copyNotice} onClose={() => setCopyNotice('')} />}
      <BulkPasswordBar selectedCount={selected.length} disabled={!isDesktop || !!snapshot.vaultError} busy={saving} groups={[...new Set(snapshot.passwords.flatMap(password => password.groups))]} onAction={bulkAction} onClear={() => setSelected([])} />

      <Table<PasswordView>
        rowKey="id"
        columns={columns}
        dataSource={filteredPasswords}
        pagination={{ pageSize: 15, showTotal: (t) => `共 ${t} 条` }}
        scroll={{ x: 1050 }}
        size="middle"
        rowSelection={{ selectedRowKeys: selected, onChange: keys => setSelected(keys.map(String)), getCheckboxProps: () => ({ disabled: !isDesktop || saving || !!snapshot.vaultError }) }}
      />

      <Modal
        title={editingItem ? '编辑密码条目' : '新建密码条目'}
        open={isEditOpen}
        onOk={handleSave}
        confirmLoading={saving}
        okText="保存"
        cancelText="取消"
        onCancel={() => { setIsEditOpen(false); form.resetFields(); }}
        destroyOnHidden
      >
        <Form form={form} layout="vertical">
          <Form.Item name="name" label="名称 / 标识" rules={[{ required: true, message: '请输入密码名称或标识' }]}>
            <Input placeholder="例如：日常解压密码" maxLength={100} />
          </Form.Item>
          <Form.Item
            name="secret"
            label="密码明文"
            rules={[...(editingItem ? [] : [{ required: true, message: '请输入密码' }]), { validator: (_rule, value: string | undefined) => value && /[\0\r\n]/.test(value) ? Promise.reject(new Error('口令不支持 NUL、换行或回车。')) : Promise.resolve() }]}
            extra={`${editingItem ? '留空保持现有口令。' : ''}首尾空格与大小写会完整保留；仅支持单行口令。`}
          >
            <Input.Password placeholder="密码明文，保存后加密存储" visibilityToggle={{ visible: secretVisible, onVisibleChange: setSecretVisible }} autoComplete="new-password" />
          </Form.Item>
          <Form.Item name="groups" label="分组标签">
            <Select mode="tags" placeholder="选择或输入标签后回车" tokenSeparators={[',']} options={[...new Set(snapshot.passwords.flatMap(password => password.groups))].map(group => ({ value: group, label: group }))} />
          </Form.Item>
          <Form.Item name="notes" label="备注信息">
            <Input.TextArea rows={2} placeholder="来源说明或其他备注" maxLength={500} />
          </Form.Item>
          <Form.Item name="enabled" label="启用状态" valuePropName="checked">
            <Switch checkedChildren="已启用" unCheckedChildren="已停用" />
          </Form.Item>
        </Form>
      </Modal>

      <Modal
        title={`查看密码 - ${revealTarget?.name ?? ''}`}
        open={Boolean(revealTarget)}
        onCancel={closeReveal}
        footer={[<Button key="close" onClick={closeReveal}>关闭</Button>]}
        destroyOnHidden
      >
        <Alert
          type="warning"
          showIcon
          title="安全性提示"
          description={`密码将在 ${revealSeconds} 秒后隐藏。窗口失焦或隐藏时立即清空显示。`}
          style={{ marginBottom: 16 }}
        />
        <div style={{ textAlign: 'center', padding: '12px 0' }}>
          {revealedSecret !== null ? (
            <Paragraph
              style={{ fontSize: 18, fontFamily: 'monospace', margin: 0 }}
            >
              <Text code strong style={{ fontSize: 16 }}>{revealedSecret}</Text>
            </Paragraph>
          ) : (
            <Text type="secondary">正在读取安全密码...</Text>
          )}
        </div>
      </Modal>

      <Modal
        title="批量导入密码"
        open={isImportOpen}
        width={720}
        destroyOnHidden
        onCancel={clearImport}
        footer={[
          <Button key="cancel" onClick={clearImport}>取消</Button>,
          <Button
            key="confirm"
            type="primary"
            disabled={!isDesktop || !preview?.token || preview.errorCount > 0 || saving}
            loading={saving}
            onClick={handleConfirmImport}
          >
            确认导入 {preview ? `(${preview.newCount} 新增)` : ''}
          </Button>,
        ]}
      >
        <Space orientation="vertical" style={{ width: '100%' }} size="middle">
          <Space wrap>
            <span>格式：</span>
            <Select
              aria-label="导入文本格式"
              value={importFormat}
              onChange={(val) => { previewEpochRef.current += 1; setImportFormat(val); setPreview(null); }}
              options={[
                { label: 'TXT：每行一个密码', value: 'txt' },
                { label: 'TSV：名称、密码、分组、备注', value: 'tsv' },
              ]}
              style={{ width: 220 }}
            />
            <span>默认分组：</span>
            <Input
              aria-label="导入默认分组"
              value={importGroup}
              placeholder="默认分组（可选）"
              onChange={(e) => { previewEpochRef.current += 1; setImportGroup(e.target.value); setPreview(null); }}
              style={{ width: 140 }}
            />
            <span>读取前选择编码：</span>
            <Select
              aria-label="导入文件编码"
              value={importEncoding}
              onChange={setImportEncoding}
              options={[{ label: 'UTF-8', value: 'utf-8' }, { label: 'GB18030', value: 'gb18030' }, { label: 'UTF-16LE', value: 'utf-16le' }]}
              style={{ width: 110 }}
            />
            <Button icon={<FolderOpenOutlined />} disabled={!isDesktop} onClick={handlePickFile}>选择文件</Button>
          </Space>

          <Input.TextArea
            aria-label="待导入密码文本"
            rows={5}
            placeholder="粘贴待导入密码内容..."
            value={importText}
            onChange={(e) => { previewEpochRef.current += 1; setImportText(e.target.value); setPreview(null); }}
          />
          <Alert type="info" showIcon title="口令按原文保存" description={importFormat === 'txt' ? 'TXT 一行一条，空行跳过；首尾空格与大小写完整保留。选择文件前指定编码；非 UTF-8 不会自动猜测。' : 'TSV 每行四列，使用 Tab 分隔：名称、口令、分组、备注；分组可用逗号分隔。文件不包含标题行。'} />

          <Button type="dashed" block onClick={handlePreviewImport} disabled={!isDesktop || !importText.length}>
            生成导入预览
          </Button>

          {preview && (
            <Card size="small" title="预览结果">
              <Space style={{ marginBottom: 8 }}>
                <Tag color="green">新增: {preview.newCount}</Tag>
                <Tag color="orange">重复: {preview.duplicateCount}</Tag>
                <Tag color={preview.errorCount > 0 ? 'red' : 'default'}>错误: {preview.errorCount}</Tag>
              </Space>
              {preview.errorCount > 0 && (
                <Alert
                  type="error"
                  title="存在解析错误条目，必须先纠正或清理错误行方可执行导入。"
                  style={{ marginBottom: 8 }}
                />
              )}
              <Table
                rowKey={(r) => `${r.line}-${r.name}`}
                dataSource={preview.rows}
                size="small"
                pagination={{ pageSize: 5 }}
                columns={[
                  { title: '行号', dataIndex: 'line', key: 'line', width: 60 },
                  { title: '标识/名称', dataIndex: 'name', key: 'name' },
                  { title: '分组', dataIndex: 'group', key: 'group' },
                  { title: '状态', dataIndex: 'status', key: 'status', width: 80 },
                  { title: '说明', dataIndex: 'message', key: 'message' },
                ]}
              />
            </Card>
          )}
        </Space>
      </Modal>
    </div>
  );
}
