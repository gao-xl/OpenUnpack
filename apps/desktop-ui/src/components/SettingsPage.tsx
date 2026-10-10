import React, { useState, useEffect, useMemo, useRef } from 'react';
import { App as AntApp, Card, Tabs, Form, Input, InputNumber, Button, Switch, Select, Radio, Modal, Alert, Space, Typography, Table } from 'antd';
import { FolderOpenOutlined, SaveOutlined, ExportOutlined, ImportOutlined, SettingOutlined, SafetyCertificateOutlined, ThunderboltOutlined, FilterOutlined, BellOutlined, AppstoreOutlined, ClearOutlined } from '@ant-design/icons';
import type { AppSnapshot, Settings, ImportPreview } from '../types';
import { api, isDesktop } from '../api';
import SourceCleanupSelect from './SourceCleanupSelect';
import RulePreview from './RulePreview';

const { Text } = Typography;
const { TextArea } = Input;
const GB = 1024 ** 3;

interface Props {
  snapshot: AppSnapshot;
  refresh: () => Promise<void>;
  run: (work: () => Promise<unknown>) => Promise<void>;
}

export default function SettingsPage({ snapshot, refresh, run }: Props) {
  const { message } = AntApp.useApp();
  const [draft, setDraft] = useState<Settings>(() => ({ ...snapshot.settings }));
  const [savedJson, setSavedJson] = useState(() => JSON.stringify(snapshot.settings));
  const [saving, setSaving] = useState(false);
  const [modalMode, setModalMode] = useState<'export' | 'restore' | null>(null);
  const [modalPath, setModalPath] = useState('');
  const [passphrase, setPassphrase] = useState('');
  const [confirmPass, setConfirmPass] = useState('');
  const [modalBusy, setModalBusy] = useState(false);
  const [restorePreview, setRestorePreview] = useState<ImportPreview | null>(null);
  const modalEpoch = useRef(0);
  const busyRef = useRef(false);

  useEffect(() => {
    const next = JSON.stringify(snapshot.settings);
    if (next !== savedJson) {
      setDraft({ ...snapshot.settings });
      setSavedJson(next);
    }
  }, [snapshot.settings, savedJson]);

  const update = <K extends keyof Settings>(key: K, val: Settings[K]) => setDraft((p) => ({ ...p, [key]: val }));

  const passwordGroups = useMemo(() => {
    const s = new Set<string>();
    snapshot.passwords?.forEach((p) => p.groups?.forEach((g) => g && s.add(g)));
    return Array.from(s);
  }, [snapshot.passwords]);

  const handleSave = async () => {
    if (saving) return;
    setSaving(true);
    try {
      await run(async () => {
        await api.saveSettings(draft);
        setSavedJson(JSON.stringify(draft));
        message.success('设置已保存');
        await refresh();
      });
    } finally {
      setSaving(false);
    }
  };

  const handlePickOutput = async () => {
    await run(async () => { const dir = await api.pickDirectory(); if (dir) update('outputRoot', dir); });
  };

  const handlePickCache = async () => {
    await run(async () => { const dir = await api.pickDirectory(); if (dir) update('cacheRoot', dir); });
  };

  const startExport = async () => {
    await run(async () => { const p = await api.pickSaveBackup(); if (p) { modalEpoch.current += 1; setRestorePreview(null); setModalPath(p); setPassphrase(''); setConfirmPass(''); setModalMode('export'); } });
  };

  const startRestore = async () => {
    await run(async () => { const p = await api.pickOpenBackup(); if (p) { modalEpoch.current += 1; setRestorePreview(null); setModalPath(p); setPassphrase(''); setConfirmPass(''); setModalMode('restore'); } });
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
  useEffect(() => () => { modalEpoch.current += 1; }, []);

  const submitModal = async () => {
    if (busyRef.current) return;
    busyRef.current = true;
    const epoch = modalEpoch.current;
    setModalBusy(true);
    try {
      if (modalMode === 'export') {
        await run(async () => {
          await api.exportBackup(modalPath, passphrase);
          message.success('备份已成功导出');
          if (epoch === modalEpoch.current) closeModal();
        });
      } else if (modalMode === 'restore') {
        await run(async () => {
          if (!restorePreview) {
            const preview = await api.previewRestoreBackup(modalPath, passphrase);
            if (epoch === modalEpoch.current) setRestorePreview(preview);
          } else {
            const res = await api.confirmRestoreBackup(modalPath, passphrase, restorePreview.token);
            message.success(`备份已合并：新增 ${res.added} 条，重复 ${res.duplicates} 条`);
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

  const tabItems = [
    {
      key: 'output',
      label: <span><SettingOutlined /> 输出管理</span>,
      children: (
        <Form layout="vertical">
          <Form.Item label="输出根目录" help="留空时在原归档旁创建独立结果目录，避免文件散落；现有文件不会覆盖。">
            <Space.Compact style={{ width: '100%' }}>
              <Input aria-label="默认输出根目录" value={draft.outputRoot} readOnly placeholder="原文件所在目录（默认）" />
              <Button icon={<FolderOpenOutlined />} disabled={!isDesktop} onClick={() => void handlePickOutput()}>选择目录</Button>
              <Button icon={<ClearOutlined />} onClick={() => update('outputRoot', '')}>清空</Button>
            </Space.Compact>
          </Form.Item>
          <Form.Item label="同名冲突策略">
            <Select aria-label="同名冲突策略" value={draft.conflictPolicy} onChange={(v) => update('conflictPolicy', v)} options={[{ value: 'rename', label: '自动重命名' }, { value: 'skip', label: '跳过' }]} />
          </Form.Item>
          <Form.Item label="智能去套娃（目录精简）">
            <Space>
              <Switch aria-label="整理唯一顶层目录" checked={draft.unwrapSingleRoot} onChange={(v) => update('unwrapSingleRoot', v)} />
              <span>当解压结果顶层仅含唯一一个文件夹时，自动剥离多余的顶层外壳目录</span>
            </Space>
          </Form.Item>
          <Alert title="保护说明" description="避免解压出双层同名嵌套目录，同时防止无根目录的散落文件污染上级文件夹。" type="info" showIcon />
        </Form>
      ),
    },
    {
      key: 'processing',
      label: <span><SafetyCertificateOutlined /> 处理策略</span>,
      children: (
        <Form layout="vertical">
          <Form.Item label="嵌套解压">
            <Space>
              <Switch aria-label="自动展开内包" checked={draft.expandNested} onChange={(v) => update('expandNested', v)} />
              <span>展开内层嵌套压缩包</span>
              {draft.expandNested && (
                <Space style={{ marginLeft: 16 }}>
                  <span>总展开层数（含外包）：</span>
                  <InputNumber aria-label="总展开层数" min={1} max={8} value={draft.maxDepth} onChange={(v) => update('maxDepth', v ?? 2)} />
                </Space>
              )}
            </Space>
          </Form.Item>
          <Form.Item label="优先密码分组">
            <Select aria-label="默认密码分组" allowClear placeholder="全部可用密码" value={draft.passwordGroup || undefined} onChange={(v) => update('passwordGroup', v || '')} options={passwordGroups.map((g) => ({ value: g, label: g }))} />
          </Form.Item>
          <Form.Item label="校验级别">
            <Radio.Group aria-label="写回验证模式" value={draft.verification} onChange={(e) => update('verification', e.target.value)}>
              <Radio value="standard">标准（流式摘要+引擎完整性）</Radio>
              <Radio value="strict">完整（增加目标临时目录回读，不证明断电持久性）</Radio>
            </Radio.Group>
          </Form.Item>
        </Form>
      ),
    },
    {
      key: 'performance',
      label: <span><ThunderboltOutlined /> 性能与缓存</span>,
      children: (
        <Form layout="vertical">
          <Form.Item label="缓存开关">
            <Space size="large">
              <Space><Switch aria-label="启用缓存工作目录" checked={draft.cacheEnabled} onChange={(v) => update('cacheEnabled', v)} /><span>启用中转缓存</span></Space>
              <Space><Switch aria-label="缓存输入源包" disabled={!draft.cacheEnabled} checked={draft.cacheInput} onChange={(v) => update('cacheInput', v)} /><span>缓存输入（机械硬盘推荐）</span></Space>
              <Space><Switch aria-label="小文件优先处理" checked={draft.smallFirst} onChange={(v) => update('smallFirst', v)} /><span>小文件优先处理</span></Space>
            </Space>
          </Form.Item>
          <Form.Item label="SSD 工作目录" help="启用缓存后必须选择工作目录；初版同时执行一个根任务。">
            <Space.Compact style={{ width: '100%' }}>
              <Input aria-label="SSD 工作目录" value={draft.cacheRoot} readOnly placeholder="请选择固态硬盘上的工作目录" />
              <Button icon={<FolderOpenOutlined />} disabled={!isDesktop} onClick={() => void handlePickCache()}>选择目录</Button>
              <Button icon={<ClearOutlined />} onClick={() => update('cacheRoot', '')}>清空</Button>
            </Space.Compact>
          </Form.Item>
          <Space wrap size="large">
            <Form.Item label="缓存配额 (GiB)"><InputNumber aria-label="缓存配额 GiB" min={1} max={4096} value={Math.round(draft.cacheQuotaBytes / GB)} onChange={(v) => update('cacheQuotaBytes', Math.max(1, v ?? 20) * GB)} /></Form.Item>
            <Form.Item label="最小剩余磁盘 (GiB)"><InputNumber aria-label="最小剩余空间 GiB" min={0.5} max={1024} step={0.5} value={Number((draft.minFreeBytes / GB).toFixed(1))} onChange={(v) => update('minFreeBytes', Math.max(0.5, v ?? 1) * GB)} /></Form.Item>
            <Form.Item label="根任务累计解压上限 (GiB)"><InputNumber aria-label="根任务累计解压上限 GiB" min={1} max={8192} value={Math.round(draft.maxExpandedBytes / GB)} onChange={(v) => update('maxExpandedBytes', Math.max(1, v ?? 100) * GB)} /></Form.Item>
            <Form.Item label="最大文件条数"><InputNumber aria-label="最大文件条数" min={100} max={100000} step={1000} value={draft.maxEntries} onChange={(v) => update('maxEntries', Math.max(100, v ?? 100000))} /></Form.Item>
          </Space>
        </Form>
      ),
    },
    {
      key: 'exclusion',
      label: <span><FilterOutlined /> 筛选排除</span>,
      children: (
        <Form layout="vertical">
          <Form.Item label="扫描范围"><Space><Switch aria-label="递归扫描子目录" checked={draft.recursiveScan} onChange={(v) => update('recursiveScan', v)} /><span>递归扫描子目录</span></Space></Form.Item>
          <Form.Item label="输入排除规则（每行一条）"><TextArea aria-label="扫描输入排除规则" rows={3} value={draft.inputExclusions.join('\n')} onChange={(e) => update('inputExclusions', e.target.value.split('\n').map((s) => s.trim()).filter(Boolean))} placeholder="例如: *.tmp" /></Form.Item>
          <Form.Item
            label={
              <Space>
                <span>输出排除规则（每行一条）</span>
                <Button
                  size="small"
                  type="link"
                  aria-label="添加 URL、快捷方式及系统文件规则"
                  onClick={() => {
                    const presets = ['*.url', '*.lnk', '__MACOSX/*', '.DS_Store', 'Thumbs.db', 'desktop.ini'];
                    const merged = Array.from(new Set([...draft.outputExclusions, ...presets]));
                    update('outputExclusions', merged);
                  }}
                >
                  添加 URL、快捷方式及系统文件规则
                </Button>
              </Space>
            }
          >
            <TextArea aria-label="归档输出排除规则" rows={3} value={draft.outputExclusions.join('\n')} onChange={(e) => update('outputExclusions', e.target.value.split('\n').map((s) => s.trim()).filter(Boolean))} placeholder="例如: *.bak" />
          </Form.Item>
          <Alert title="规则范围" description="扫描输入规则决定哪些归档进入队列；归档输出规则决定哪些条目交付。* 匹配任意字符，? 匹配一个字符。匹配相对路径或文件名，英文不区分大小写。没有默认隐藏规则；排除内包时该项不会展开或交付。" type="warning" showIcon />
          <RulePreview inputPatterns={draft.inputExclusions} outputPatterns={draft.outputExclusions} run={run} />
        </Form>
      ),
    },
    {
      key: 'completion',
      label: <span><BellOutlined /> 完成后动作</span>,
      children: (
        <Form layout="vertical">
          <Form.Item label="源压缩包处理">
            <SourceCleanupSelect value={draft.sourceCleanup} onChange={v => update('sourceCleanup', v)} disabled={!isDesktop} />
          </Form.Item>
          <Alert
            title="安全保护规则"
            description="源包保留是默认策略。选用清理时，需完整交付、未排除输出、来源未变化并再次核对结果；部分交付或核对失败保留源包。原始分卷清理失败会独立提示，不撤回已交付结果。"
            type="info"
            showIcon
            style={{ marginBottom: 16 }}
          />
          <Form.Item label="任务完成音效">
            <Space>
              <Switch aria-label="处理结束提示音" checked={draft.completionSound} onChange={(v) => update('completionSound', v)} />
              <span>任务交付后播放系统提示音</span>
            </Space>
          </Form.Item>
        </Form>
      ),
    },
    {
      key: 'interfacebackup',
      label: <span><AppstoreOutlined /> 界面与备份</span>,
      children: (
        <Form layout="vertical">
          <Space wrap size="large">
            <Form.Item label="界面主题"><Select aria-label="界面主题" value={draft.theme} onChange={(v) => update('theme', v)} style={{ width: 130 }} options={[{ value: 'light', label: '浅色' }, { value: 'dark', label: '深色' }, { value: 'system', label: '跟随系统' }]} /></Form.Item>
            <Form.Item label="历史记录上限"><InputNumber aria-label="历史记录上限" min={50} max={5000} value={draft.historyLimit} onChange={(v) => update('historyLimit', v ?? 500)} /></Form.Item>
            <Form.Item label="窗口置顶"><Switch aria-label="窗口置顶" checked={draft.alwaysOnTop} onChange={(v) => update('alwaysOnTop', v)} /></Form.Item>
            <Form.Item label="最小化到托盘"><Switch aria-label="最小化到托盘" checked={draft.minimizeToTray} onChange={(v) => update('minimizeToTray', v)} /></Form.Item>
          </Space>
          <Alert style={{ marginBottom: 16 }} title="凭证保护" description="主密码库由 Windows DPAPI 当前登录用户透明加解密保护。" type="success" />
          <Form.Item label="本机数据目录"><Space wrap><Text code style={{ wordBreak: 'break-all' }}>{snapshot.dataDirectory || '桌面版启动后显示'}</Text><Button disabled={!isDesktop || !snapshot.dataDirectory} onClick={() => void run(() => api.openPath(snapshot.dataDirectory))}>打开数据目录</Button></Space></Form.Item>
          <Form.Item label="解压引擎"><Text type={snapshot.engineAvailable ? 'success' : 'warning'}>{snapshot.engineAvailable ? '本地引擎和作业进程已就绪' : '尚未就绪，请检查安装包中的 worker 与 7-Zip 文件'}</Text></Form.Item>
          <Form.Item label="密码库灾备管理">
            <Space>
              <Button aria-label="导出加密备份" icon={<ExportOutlined aria-hidden />} disabled={!isDesktop} onClick={() => void startExport()}>导出加密备份</Button>
              <Button aria-label="恢复备份并合并" icon={<ImportOutlined aria-hidden />} disabled={!isDesktop} onClick={() => void startRestore()}>恢复备份并合并</Button>
            </Space>
          </Form.Item>
        </Form>
      ),
    },
  ];

  return (
    <div style={{ maxWidth: 900, margin: '0 auto', padding: '16px 20px' }}>
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 16 }}>
        <div><h2 style={{ margin: 0 }}>参数设置</h2><Text type="secondary">配置解压策略、密码组、缓存目录及密码库灾备</Text></div>
        <Button type="primary" icon={<SaveOutlined />} loading={saving} disabled={!isDesktop} onClick={() => void handleSave()}>保存设置</Button>
      </div>

      {snapshot.running && <Alert title="提示：当前任务使用已冻结设置，新任务使用保存值。" type="warning" showIcon style={{ marginBottom: 16 }} />}
      <Card><Tabs items={tabItems} /></Card>

      <Modal
        title={modalMode === 'export' ? '导出密码库备份' : '恢复密码库备份'}
        open={modalMode !== null}
        onCancel={closeModal}
        destroyOnHidden
        okText={modalMode === 'export' ? '导出加密备份' : restorePreview ? '确认合并' : '解密并预览'}
        cancelText="取消"
        okButtonProps={{ loading: modalBusy, disabled: Array.from(passphrase).length < 8 || (modalMode === 'export' && passphrase !== confirmPass) || !!restorePreview?.errorCount }}
        onOk={() => void submitModal()}
      >
        <Space orientation="vertical" style={{ width: '100%' }}>
          <div><Text type="secondary">文件路径：</Text><Text code style={{ wordBreak: 'break-all' }}>{modalPath}</Text></div>
          <Alert type="info" title={modalMode === 'restore' ? '恢复备份说明' : '导出备份说明'} description={modalMode === 'restore' ? '先解密与校验并查看条目预览，再由你确认合并。重复条目保留当前名称与备注并合并分组。口令错误、文件损坏或版本不兼容均不会改变当前库。' : '备份另用你设置的口令加密。请牢记口令；备份可用于其他 Windows 账户与电脑。'} />
          <div><Text>加解密口令（至少 8 字符）：</Text><Input.Password aria-label="备份口令" autoComplete="new-password" disabled={modalBusy} value={passphrase} onChange={(e) => { modalEpoch.current += 1; setRestorePreview(null); setPassphrase(e.target.value); }} placeholder="至少 8 字符口令" /></div>
          {modalMode === 'export' && <div><Text>确认口令：</Text><Input.Password value={confirmPass} onChange={(e) => setConfirmPass(e.target.value)} placeholder="请再次输入口令" /></div>}
          {restorePreview && <><Space wrap><Text strong>备份预览</Text><Text>新增 {restorePreview.newCount} 条</Text><Text>重复 {restorePreview.duplicateCount} 条</Text><Text type={restorePreview.errorCount ? 'danger' : undefined}>错误 {restorePreview.errorCount} 条</Text></Space><Table size="small" rowKey={row => `${row.line}-${row.name}`} dataSource={restorePreview.rows} pagination={{ pageSize: 5 }} columns={[{ title: '条目', dataIndex: 'line' }, { title: '名称', dataIndex: 'name' }, { title: '分组', dataIndex: 'group' }, { title: '状态', dataIndex: 'status' }, { title: '说明', dataIndex: 'message' }]} /></>}
        </Space>
      </Modal>
    </div>
  );
}
