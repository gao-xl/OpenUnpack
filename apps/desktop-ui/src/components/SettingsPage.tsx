import { t, useLanguage } from '../i18n';
import React, { useState, useEffect, useMemo, useRef } from 'react';
import { App as AntApp, Card, Tabs, Form, Input, InputNumber, Slider, Button, Switch, Select, Radio, Modal, Alert, Space, Typography, Table, Empty, List } from 'antd';
import { FolderOpenOutlined, SaveOutlined, ExportOutlined, ImportOutlined, SettingOutlined, SafetyCertificateOutlined, ThunderboltOutlined, FilterOutlined, BellOutlined, AppstoreOutlined, ClearOutlined, EyeOutlined, FolderAddOutlined, DeleteOutlined, FolderOutlined } from '@ant-design/icons';
import type { AppSnapshot, Settings, ImportPreview } from '../types';
import { MAX_CONCURRENT_JOBS } from '../types';
import { api, isDesktop } from '../api';
import SourceCleanupSelect, { SourceCleanupScope } from './SourceCleanupSelect';
import RulePreview from './RulePreview';
import ApiSettingsPanel from './ApiSettingsPanel';
import { displayPath } from './shared';
import { isExclusionPatternWithinLimit } from '../exclusion-rules';

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
  const daemon = api.isDaemonMode?.() === true;
  const connected = isDesktop || daemon;
  const nativeEngine = isDesktop && !daemon;
  const { language, setLanguage } = useLanguage();
  const [draft, setDraft] = useState<Settings>(() => ({ ...snapshot.settings }));
  const [savedJson, setSavedJson] = useState(() => JSON.stringify(snapshot.settings));
  const [saving, setSaving] = useState(false);
  const [modalMode, setModalMode] = useState<'export' | 'restore' | null>(null);
  const [modalPath, setModalPath] = useState('');
  const [passphrase, setPassphrase] = useState('');
  const [confirmPass, setConfirmPass] = useState('');
  const [modalBusy, setModalBusy] = useState(false);
  const [restorePreview, setRestorePreview] = useState<ImportPreview | null>(null);
  const [shellMenuEnabled, setShellMenuEnabled] = useState(false);
  const [shellMenuBusy, setShellMenuBusy] = useState(false);
  const modalEpoch = useRef(0);
  const busyRef = useRef(false);

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
        setDraft({ ...snapshot.settings });
        setSavedJson(next);
      });
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
    if (draft.inputExclusions.length > 100 || draft.outputExclusions.length > 100) {
      message.error(t("排除规则数量不可超过 100 条"));
      return;
    }
    if ([...draft.inputExclusions, ...draft.outputExclusions].some((p) => !isExclusionPatternWithinLimit(p))) {
      message.error(t("单条排除规则不可超过 512 UTF-8 字节"));
      return;
    }
    setSaving(true);
    try {
      await run(async () => {
        await api.saveSettings(draft);
        setSavedJson(JSON.stringify(draft));
        message.success(t("设置已保存"));
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
    const epoch = ++modalEpoch.current;
    await run(async () => { const p = await api.pickSaveBackup(); if (p && modalEpoch.current === epoch) { setRestorePreview(null); setModalPath(p); setPassphrase(''); setConfirmPass(''); setModalMode('export'); } });
  };

  const startRestore = async () => {
    const epoch = ++modalEpoch.current;
    await run(async () => { const p = await api.pickOpenBackup(); if (p && modalEpoch.current === epoch) { setRestorePreview(null); setModalPath(p); setPassphrase(''); setConfirmPass(''); setModalMode('restore'); } });
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
          message.success(t("备份已成功导出"));
          if (epoch === modalEpoch.current) closeModal();
        });
      } else if (modalMode === 'restore') {
        await run(async () => {
          if (!restorePreview) {
            const preview = await api.previewRestoreBackup(modalPath, passphrase);
            if (epoch === modalEpoch.current) setRestorePreview(preview);
          } else {
            const res = await api.confirmRestoreBackup(modalPath, passphrase, restorePreview.token);
            message.success(t("备份已合并：新增 {value0} 条，重复 {value1} 条", { value0: res.added, value1: res.duplicates }));
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
      label: <span><SettingOutlined /> {t("输出管理")}</span>,
      children: (
        <Form layout="vertical">
          <Form.Item label={t("输出根目录")} help={t("留空时在原归档旁创建独立结果目录，避免文件散落；现有文件不会覆盖。")}>
            <Space.Compact style={{ width: '100%' }}>
              <Input aria-label={t("默认输出根目录")} value={displayPath(draft.outputRoot)} readOnly={!daemon} onChange={event => update('outputRoot', event.target.value)} placeholder={t("原文件所在目录（默认）")} />
              <Button icon={<FolderOpenOutlined />} disabled={!nativeEngine} onClick={() => void handlePickOutput()}>{t("选择目录")}</Button>
              <Button icon={<ClearOutlined />} onClick={() => update('outputRoot', '')}>{t("清空")}</Button>
            </Space.Compact>
          </Form.Item>
          <Form.Item label={t("同名冲突策略")}>
            <Select aria-label={t("同名冲突策略")} value={draft.conflictPolicy} onChange={(v) => update('conflictPolicy', v)} options={[{ value: 'rename', label: t("自动重命名") }, { value: 'skip', label: t("跳过") }]} />
          </Form.Item>
          <Form.Item label={t("智能去套娃（目录精简）")}>
            <Space>
              <Switch aria-label={t("整理唯一顶层目录")} checked={draft.unwrapSingleRoot} onChange={(v) => update('unwrapSingleRoot', v)} />
              <span>{t("当解压结果顶层仅含唯一一个文件夹时，自动剥离多余的顶层外壳目录")}</span>
            </Space>
          </Form.Item>
          <Form.Item label={t("清理空目录")}>
            <Space>
              <Switch aria-label={t("清理空目录")} checked={draft.cleanEmptyDirs} onChange={(v) => update('cleanEmptyDirs', v)} />
              <span>{t("解压完成后自动清理结果中的各级空目录")}</span>
            </Space>
          </Form.Item>
          <Alert title={t("保护说明")} description={t("避免解压出双层同名嵌套目录，同时防止无根目录的散落文件污染上级文件夹。")} type="info" showIcon />
        </Form>
      ),
    },
    {
      key: 'processing',
      label: <span><SafetyCertificateOutlined /> {t("处理策略")}</span>,
      children: (
        <Form layout="vertical">
          <Form.Item label={t("嵌套解压")}>
            <Space>
              <Switch aria-label={t("自动展开内包")} checked={draft.expandNested} onChange={(v) => update('expandNested', v)} />
              <span>{t("展开内层嵌套压缩包")}</span>
              {draft.expandNested && (
                <Space style={{ marginLeft: 16 }}>
                  <span>{t("总展开层数（含外包）：")}</span>
                  <InputNumber aria-label={t("总展开层数")} min={1} max={8} value={draft.maxDepth} onChange={(v) => update('maxDepth', v ?? 2)} />
                </Space>
              )}
            </Space>
          </Form.Item>
          <Form.Item label={t("优先密码分组")}>
            <Select aria-label={t("默认密码分组")} allowClear placeholder={t("全部可用密码")} value={draft.passwordGroup || undefined} onChange={(v) => update('passwordGroup', v || '')} options={passwordGroups.map((g) => ({ value: g, label: g }))} />
          </Form.Item>
          <Form.Item label={t("校验级别")}>
            <Radio.Group aria-label={t("写回验证模式")} value={draft.verification} onChange={(e) => update('verification', e.target.value as string)}>
              <Radio value="standard">{t("标准（流式摘要+引擎完整性）")}</Radio>
              <Radio value="strict">{t("完整校验（回读目标目录，更耗时）")}</Radio>
            </Radio.Group>
          </Form.Item>
          <Form.Item label={t("添加后自动解压")}>
            <Space>
              <Switch aria-label={t("添加后自动解压")} checked={draft.autoStartOnAdd} onChange={(v) => update('autoStartOnAdd', v)} />
              <span>{t("添加文件或确认批次后自动开始队列，无需手动点击“开始队列”")}</span>
            </Space>
          </Form.Item>
          <Form.Item label={t("损坏包抢救模式")}>
            <Space>
              <Switch aria-label={t("损坏包抢救提取")} checked={draft.salvageCorrupted} onChange={(v) => update('salvageCorrupted', v)} />
              <span>{t("遇到 CRC 错误或数据损坏时尽可能提取完好文件，严禁删除源压缩包")}</span>
            </Space>
          </Form.Item>
          <Form.Item label={t("文件名乱码修复")}>
            <Space>
              <Switch aria-label={t("自动识别并修复文件名乱码")} checked={draft.autoFixEncoding} onChange={(v) => update('autoFixEncoding', v)} />
              <span>{t("针对传统非 UTF-8 编码的 ZIP 归档（GBK/Shift-JIS/Big5）自动推断并修复乱码")}</span>
            </Space>
          </Form.Item>
        </Form>
      ),
    },
    {
      key: 'performance',
      label: <span><ThunderboltOutlined /> {t("性能与缓存")}</span>,
      children: (
        <Form layout="vertical">
          <Form.Item label={t("缓存开关")}>
            <Space size="large">
              <Space><Switch aria-label={t("启用缓存工作目录")} checked={draft.cacheEnabled} onChange={(v) => update('cacheEnabled', v)} /><span>{t("启用中转缓存")}</span></Space>
              <Space><Switch aria-label={t("缓存输入源包")} disabled={!draft.cacheEnabled} checked={draft.cacheInput} onChange={(v) => update('cacheInput', v)} /><span>{t("使用中转缓存时复制输入")}</span></Space>
              <Space><Switch aria-label={t("小文件优先处理")} checked={draft.smallFirst} onChange={(v) => update('smallFirst', v)} /><span>{t("小文件优先处理")}</span></Space>
            </Space>
          </Form.Item>
          <Form.Item label={t("缓存选择策略")} help={t("关闭缓存即直接解压；自动模式在缺少收益证据时优先直写。中转缓存不保证更快。")}>
            <Select aria-label={t("缓存选择策略")} disabled={!draft.cacheEnabled} value={draft.cachePolicy} onChange={(value) => update('cachePolicy', value)} options={[{ value: 'auto', label: t("自动选择（推荐）") }, { value: 'always', label: t("总是使用中转缓存") }]} style={{ width: 280 }} />
          </Form.Item>
          <Form.Item
            label={t("并发解压任务数")}
            help={t("同时最多 {value0} 个任务，默认 1；机械硬盘建议设为 1，SSD 可按实际负载增加", { value0: MAX_CONCURRENT_JOBS })}
          >
            <Space size="large" align="center" style={{ width: '100%', maxWidth: 380 }}>
              <Slider
                aria-label={t("并发解压任务数")}
                min={1}
                max={MAX_CONCURRENT_JOBS}
                marks={{ 1: '1', 2: '2', 3: '3', 4: '4', 5: '5' }}
                step={1}
                value={draft.concurrency ?? 1}
                onChange={(v) => update('concurrency', v)}
                style={{ flex: 1, minWidth: 200 }}
              />
              <InputNumber
                aria-label={t("并发解压任务数数值")}
                min={1}
                max={MAX_CONCURRENT_JOBS}
                value={draft.concurrency ?? 1}
                onChange={(v) => update('concurrency', Math.min(MAX_CONCURRENT_JOBS, Math.max(1, v ?? 1)))}
              />
            </Space>
          </Form.Item>
          <Form.Item
            label={t("引擎工作线程数")}
            help={t("7-Zip 解压线程数，0 表示按并发任务数分配核心（每任务至少 1 个）；手动设置的线程数保持不变")}
          >
            <Space size="large" align="center" style={{ width: '100%', maxWidth: 380 }}>
              <Slider
                aria-label={t("引擎工作线程数")}
                min={0}
                max={32}
                marks={{ 0: t("自动"), 4: '4', 8: '8', 16: '16', 32: '32' }}
                step={1}
                value={draft.engineThreads ?? 0}
                onChange={(v) => update('engineThreads', v)}
                style={{ flex: 1, minWidth: 200 }}
              />
              <InputNumber
                aria-label={t("引擎工作线程数数值")}
                min={0}
                max={64}
                value={draft.engineThreads ?? 0}
                onChange={(v) => update('engineThreads', Math.min(64, Math.max(0, v ?? 0)))}
              />
            </Space>
          </Form.Item>
          <Form.Item label={t("工作进程优先级")} help={t("调整后台解压进程的 CPU 抢占优先级，避免系统卡顿")}>
            <Select
              aria-label={t("工作进程优先级")}
              value={draft.workerPriority || 'belowNormal'}
              onChange={(v) => update('workerPriority', v)}
              style={{ width: 260 }}
              options={[
                { value: 'belowNormal', label: t("低于正常（推荐，解压不卡前台）") },
                { value: 'idle', label: t("空闲（后台静默解压，资源最低消耗）") },
                { value: 'normal', label: t("正常（与其他应用同级竞争 CPU）") },
              ]}
            />
          </Form.Item>
          <Form.Item label={t("SSD 工作目录")} help={t("启用中转缓存时必须设置工作目录，建议选择 SSD 所在分区。")}>
            <Space.Compact style={{ width: '100%' }}>
              <Input aria-label={t("SSD 工作目录")} value={displayPath(draft.cacheRoot)} readOnly={!daemon} onChange={event => update('cacheRoot', event.target.value)} placeholder={t("请选择固态硬盘上的工作目录")} />
              <Button icon={<FolderOpenOutlined />} disabled={!nativeEngine} onClick={() => void handlePickCache()}>{t("选择目录")}</Button>
              <Button icon={<ClearOutlined />} onClick={() => update('cacheRoot', '')}>{t("清空")}</Button>
            </Space.Compact>
          </Form.Item>
          <Space wrap size="large">
            <Form.Item label={t("缓存配额 (GiB)")}><InputNumber aria-label={t("缓存配额 GiB")} min={1} max={4096} value={Math.round(draft.cacheQuotaBytes / GB)} onChange={(v) => update('cacheQuotaBytes', Math.max(1, v ?? 20) * GB)} /></Form.Item>
            <Form.Item label={t("最小剩余磁盘 (GiB)")}><InputNumber aria-label={t("最小剩余空间 GiB")} min={0.5} max={1024} step={0.5} value={Number((draft.minFreeBytes / GB).toFixed(1))} onChange={(v) => update('minFreeBytes', Math.max(0.5, v ?? 1) * GB)} /></Form.Item>
            <Form.Item label={t("根任务累计解压上限 (GiB)")}><InputNumber aria-label={t("根任务累计解压上限 GiB")} min={1} max={8192} value={Math.round(draft.maxExpandedBytes / GB)} onChange={(v) => update('maxExpandedBytes', Math.max(1, v ?? 100) * GB)} /></Form.Item>
            <Form.Item label={t("最大文件条数")}><InputNumber aria-label={t("最大文件条数")} min={100} max={100000} step={1000} value={draft.maxEntries} onChange={(v) => update('maxEntries', Math.max(100, v ?? 100000))} /></Form.Item>
          </Space>
        </Form>
      ),
    },
    {
      key: 'exclusion',
      label: <span><FilterOutlined /> {t("筛选排除")}</span>,
      children: (
        <Form layout="vertical">
          <Form.Item label={t("扫描范围")}><Space><Switch aria-label={t("递归扫描子目录")} checked={draft.recursiveScan} onChange={(v) => update('recursiveScan', v)} /><span>{t("递归扫描子目录")}</span></Space></Form.Item>
          <Form.Item label={t("输入排除规则（每行一条）")} help={t("最多 100 条规则，单条不超过 512 UTF-8 字节")}><TextArea aria-label={t("扫描输入排除规则")} rows={3} value={draft.inputExclusions.join('\n')} onChange={(e) => update('inputExclusions', e.target.value.split('\n').map((s) => s.trim()).filter(Boolean))} placeholder={t("例如: *.tmp")} /></Form.Item>
          <Form.Item
            help={t("最多 100 条规则，单条不超过 512 UTF-8 字节")}
            label={
              <Space>
                <span>{t("输出排除规则（每行一条）")}</span>
                <Button
                  size="small"
                  type="link"
                  aria-label={t("添加 URL、快捷方式及系统文件规则")}
                  onClick={() => {
                    const presets = ['*.url', '*.lnk', '__MACOSX/*', '.DS_Store', 'Thumbs.db', 'desktop.ini'];
                    const merged = Array.from(new Set([...draft.outputExclusions, ...presets]));
                    update('outputExclusions', merged);
                  }}
                >
                  {t("添加 URL、快捷方式及系统文件规则")}</Button>
              </Space>
            }
          >
            <TextArea aria-label={t("归档输出排除规则")} rows={3} value={draft.outputExclusions.join('\n')} onChange={(e) => update('outputExclusions', e.target.value.split('\n').map((s) => s.trim()).filter(Boolean))} placeholder={t("例如: *.bak")} />
          </Form.Item>
          <Alert title={t("规则范围")} description={t("支持通配符（* 匹配任意字符，? 匹配单字符），不区分大小写。输入规则过滤要添加的压缩包，输出规则过滤解压生成的文件（排除的内层压缩包不会继续展开）。")} type="warning" showIcon />
          <RulePreview inputPatterns={draft.inputExclusions} outputPatterns={draft.outputExclusions} run={run} />
        </Form>
      ),
    },
    {
      key: 'watch',
      label: <span><EyeOutlined /> {t("自动监听")}</span>,
      children: (
        <Form layout="vertical">
          <Form.Item
            label={t("自动监听目录（下载与自动入队）")}
            help={t("监听指定文件夹（如下载目录），当压缩包下载落盘且写入完成时，自动添加至待解压队列并匹配密码")}
          >
            <div style={{ marginBottom: 12 }}>
              <Button
                type="dashed"
                icon={<FolderAddOutlined />}
                disabled={!nativeEngine}
                onClick={() => {
                  void (async () => {
                    await run(async () => {
                      const dir = await api.pickDirectory();
                      if (dir) {
                        const current = draft.watchedFolders || [];
                        if (!current.includes(dir)) {
                          update('watchedFolders', [...current, dir]);
                        }
                      }
                    });
                  })();
                }}
              >
                {t("添加监听文件夹")}
              </Button>
            </div>
            {(draft.watchedFolders || []).length === 0 ? (
              <Empty
                image={Empty.PRESENTED_IMAGE_SIMPLE}
                description={t("暂未添加监听文件夹。添加后即可在后台自动监听新下载的压缩包。")}
              />
            ) : (
              <List
                size="small"
                bordered
                dataSource={draft.watchedFolders || []}
                renderItem={(folder) => (
                  <List.Item
                    actions={[
                      <Button
                        key="delete"
                        disabled={!nativeEngine}
                        type="text"
                        danger
                        icon={<DeleteOutlined />}
                        onClick={() => {
                          update(
                            'watchedFolders',
                            (draft.watchedFolders || []).filter((f) => f !== folder)
                          );
                        }}
                      >
                        {t("移除")}
                      </Button>,
                    ]}
                  >
                    <Space>
                      <FolderOutlined style={{ color: 'var(--ou-info)' }} />
                      <Text code style={{ wordBreak: 'break-all' }}>
                        {displayPath(folder)}
                      </Text>
                    </Space>
                  </List.Item>
                )}
              />
            )}
          </Form.Item>
          <Alert
            title={t("防抢读稳定机制说明")}
            description={t("针对浏览器（如 Chrome/Edge）、下载工具（IDM 等）分块下载特性，系统采用排他读锁探测与尺寸稳定窗口判定，仅在文件完全落盘并释放写入锁后才会自动入队，确保解压完整可靠。")}
            type="info"
            showIcon
            style={{ marginTop: 16 }}
          />
        </Form>
      ),
    },
    {
      key: 'completion',
      label: <span><BellOutlined /> {t("完成后动作")}</span>,
      children: (
        <Form layout="vertical">
          <Form.Item label={t("源压缩包处理")} help={<SourceCleanupScope value={draft.sourceCleanup} expandNested={draft.expandNested} />}>
            <SourceCleanupSelect value={draft.sourceCleanup} onChange={v => update('sourceCleanup', v)} disabled={!connected} />
          </Form.Item>
          <Alert
            title={t("安全保护规则")}
            description={t("默认保留原始压缩包。选择清理时，仅在解压完全成功且校验通过后才会处理源文件；解压未完成或失败时始终保留。")}
            type="info"
            showIcon
            style={{ marginBottom: 16 }}
          />
          <Form.Item label={t("任务完成音效")}>
            <Space>
              <Switch aria-label={t("处理结束提示音")} checked={draft.completionSound} onChange={(v) => update('completionSound', v)} />
              <span>{t("任务交付后播放系统提示音")}</span>
            </Space>
          </Form.Item>
          <Form.Item
            label={t("全部队列完成后动作")}
            help={t("当整个任务队列全部处理完成且无人值守时执行的后续动作；休眠和关机将提前展示 30 秒带音效的倒计时窗口，可随时取消。")}
          >
            <Select
              aria-label={t("全部队列完成后动作")}
              disabled={!nativeEngine}
              value={draft.postExtractionAction || 'none'}
              onChange={(v) => update('postExtractionAction', v)}
              style={{ width: 300 }}
              options={[
                { value: 'none', label: t("不执行任何操作（默认）") },
                { value: 'openOutput', label: t("打开最终解压输出目录") },
                { value: 'sleep', label: t("进入系统睡眠/休眠（带30秒倒计时）") },
                { value: 'shutdown', label: t("自动关闭计算机（带30秒倒计时）") },
              ]}
            />
          </Form.Item>
        </Form>
      ),
    },
    {
      key: 'localapi',
      label: <span><AppstoreOutlined /> {t("本机 API")}</span>,
      destroyOnHidden: true,
      children: <ApiSettingsPanel run={run} />,
    },
    {
      key: 'interfacebackup',
      label: <span><AppstoreOutlined /> {t("界面与备份")}</span>,
      children: (
        <Form layout="vertical">
          <Space wrap size="large">
            <Form.Item label={t("界面语言")}><Select aria-label={t("界面语言")} value={language} onChange={setLanguage} style={{ width: 170 }} options={[{value: "zh-CN", label: "简体中文"}, {value: "en-US", label: "English"}, {value: "ja-JP", label: "日本語"}]} /></Form.Item>
            <Form.Item label={t("界面主题")}><Select aria-label={t("界面主题")} value={draft.theme} onChange={(v) => update('theme', v)} style={{ width: 130 }} options={[{ value: 'light', label: t("浅色") }, { value: 'dark', label: t("深色") }, { value: 'system', label: t("跟随系统") }]} /></Form.Item>
            <Form.Item label={t("历史记录上限")}><InputNumber aria-label={t("历史记录上限")} min={50} max={5000} value={draft.historyLimit} onChange={(v) => update('historyLimit', v ?? 500)} /></Form.Item>
            <Form.Item label={t("窗口置顶")}><Switch aria-label={t("窗口置顶")} checked={draft.alwaysOnTop} onChange={(v) => update('alwaysOnTop', v)} /></Form.Item>
            <Form.Item label={t("最小化到托盘")}><Switch aria-label={t("最小化到托盘")} checked={draft.minimizeToTray} onChange={(v) => update('minimizeToTray', v)} /></Form.Item>
          </Space>
          <Alert style={{ marginBottom: 16 }} title={t("安全存储")} description={t("本地密码库由当前 Windows 账户凭据自动加密保护。")} type="success" />
          <Form.Item label={t("本机数据目录")}><Space wrap><Text code style={{ wordBreak: 'break-all' }}>{snapshot.dataDirectory || t("桌面版启动后显示")}</Text><Button disabled={!nativeEngine || !snapshot.dataDirectory} onClick={() => void run(() => api.openPath(snapshot.dataDirectory))}>{t("打开数据目录")}</Button></Space></Form.Item>
          <Form.Item label={t("解压引擎")}><Text type={snapshot.engineAvailable ? 'success' : 'warning'}>{snapshot.engineAvailable ? t("本地引擎和作业进程已就绪") : t("尚未就绪，请检查安装包中的 worker 与 7-Zip 文件")}</Text></Form.Item>
          <Form.Item
            label={t("资源管理器右键菜单集成")}
            help={t("在 Windows 资源管理器右键点击压缩包时显示“使用 OpenUnpack 解压”，一键入队")}
          >
            <Space>
              <Switch
                aria-label={t("资源管理器右键菜单集成")}
                checked={shellMenuEnabled}
                loading={shellMenuBusy}
                disabled={!nativeEngine}
                onChange={(checked) => {
                  void (async () => {
                    setShellMenuBusy(true);
                    try {
                      const res = await api.setShellContextMenuStatus(checked);
                      setShellMenuEnabled(res);
                      message.success(checked ? t("已添加资源管理器右键菜单") : t("已移除资源管理器右键菜单"));
                    } catch (err: unknown) {
                      message.error(String(err));
                    } finally {
                      setShellMenuBusy(false);
                    }
                  })();
                }}
              />
              <span>{shellMenuEnabled ? t("已启用右键快捷解压") : t("未启用")}</span>
            </Space>
          </Form.Item>
          <Form.Item label={t("密码库备份与恢复")}>
            <Space>
              <Button aria-label={t("导出加密备份")} icon={<ExportOutlined aria-hidden />} disabled={!nativeEngine} onClick={() => void startExport()}>{t("导出加密备份")}</Button>
              <Button aria-label={t("恢复备份并合并")} icon={<ImportOutlined aria-hidden />} disabled={!nativeEngine} onClick={() => void startRestore()}>{t("恢复备份并合并")}</Button>
            </Space>
          </Form.Item>
        </Form>
      ),
    },
  ];

  return (
    <div style={{ maxWidth: 940, margin: '0 auto', padding: '16px 20px' }}>
      <div className="page-header" style={{ marginBottom: 14 }}>
        <div>
          <h2 className="page-title">{t("参数设置")}</h2>
          <p className="page-description">
            {t("配置解压策略、密码组、缓存目录及密码库备份与恢复")}
          </p>
        </div>
        <Button type="primary" icon={<SaveOutlined />} loading={saving} disabled={!connected} onClick={() => void handleSave()}>{t("保存设置")}</Button>
      </div>

      {snapshot.running && <Alert title={t("提示：当前任务使用已冻结设置，新任务使用保存值。")} type="warning" showIcon style={{ marginBottom: 14 }} />}
      <Card style={{ borderRadius: 'var(--ou-radius)', border: '1px solid var(--ou-border-color)' }}>
        <Tabs items={tabItems} />
      </Card>

      <Modal
        title={modalMode === 'export' ? t("导出密码库备份") : t("恢复密码库备份")}
        open={modalMode !== null}
        onCancel={closeModal}
        destroyOnHidden
        okText={modalMode === 'export' ? t("导出加密备份") : restorePreview ? t("确认合并") : t("解密并预览")}
        cancelText={t("取消")}
        okButtonProps={{ loading: modalBusy, disabled: Array.from(passphrase).length < 8 || (modalMode === 'export' && passphrase !== confirmPass) || !!restorePreview?.errorCount }}
        onOk={() => void submitModal()}
      >
        <Space orientation="vertical" style={{ width: '100%' }}>
          <div><Text type="secondary">{t("文件路径：")}</Text><Text code style={{ wordBreak: 'break-all' }}>{modalPath}</Text></div>
          <Alert type="info" title={modalMode === 'restore' ? t("恢复备份说明") : t("导出备份说明")} description={modalMode === 'restore' ? t("解密并校验条目后可预览内容，确认后合并到当前库。重复条目将保留名称和备注并合并分组。密码错误或文件损坏不会影响当前密码库。") : t("备份文件将使用你设置的独立密码加密，可用于在其他电脑或系统中恢复。请妥善保存该密码。")} />
          <div><Text>{t("备份密码（至少 8 位）：")}</Text><Input.Password aria-label={t("备份密码")} autoComplete="new-password" disabled={modalBusy} value={passphrase} onChange={(e) => { modalEpoch.current += 1; setRestorePreview(null); setPassphrase(e.target.value); }} placeholder={t("至少 8 位密码")} /></div>
          {modalMode === 'export' && <div><Text>{t("确认密码：")}</Text><Input.Password value={confirmPass} onChange={(e) => setConfirmPass(e.target.value)} placeholder={t("请再次输入密码")} /></div>}
          {restorePreview && <><Space wrap><Text strong>{t("备份预览")}</Text><Text>{t("新增 {count} 条", { count: restorePreview.newCount })}</Text><Text>{t("重复 {count} 条", { count: restorePreview.duplicateCount })}</Text><Text type={restorePreview.errorCount ? 'danger' : undefined}>{t("错误 {count} 条", { count: restorePreview.errorCount })}</Text></Space><Table size="small" rowKey={row => `${row.line}-${row.name}`} dataSource={restorePreview.rows} pagination={{ pageSize: 5 }} columns={[{ title: t("条目"), dataIndex: 'line' }, { title: t("名称"), dataIndex: 'name' }, { title: t("分组"), dataIndex: 'group' }, { title: t("状态"), dataIndex: 'status' }, { title: t("说明"), dataIndex: 'message' }]} /></>}
        </Space>
      </Modal>
    </div>
  );
}
