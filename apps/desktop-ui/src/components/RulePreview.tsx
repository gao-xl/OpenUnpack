import { t } from '../i18n';
import { useCallback, useEffect, useRef, useState } from 'react';
import { Alert, Button, Input, Select, Space, Table, Tag, Typography } from 'antd';
import { api, isDesktop } from '../api';
import { isExclusionPatternWithinLimit } from '../exclusion-rules';

export default function RulePreview({ inputPatterns, outputPatterns, run }: { inputPatterns: string[]; outputPatterns: string[]; run: (work: () => Promise<unknown>) => Promise<void> }) {
  const connected = isDesktop || api.isDaemonMode?.() === true;
  const [scope, setScope] = useState<'input' | 'output'>('input');
  const [text, setText] = useState('');
  const [rows, setRows] = useState<{ key: string; path: string; matched: boolean }[] | null>(null);
  const [busy, setBusy] = useState(false);
  const epoch = useRef(0);
  const active = useRef<{ id: string; scope: 'input' | 'output' } | null>(null);
  const mounted = useRef(false);
  const [cancelError, setCancelError] = useState<string | null>(null);
  const cancelActive = useCallback(() => {
    const request = active.current;
    active.current = null;
    if (request?.scope === 'output') void api.cancelPreviewExclusions(request.id).catch(() => {
      if (mounted.current) setCancelError(t("停止规则试算未确认，请稍后核对后台状态。"));
    });
  }, []);
  const patterns = scope === 'input' ? inputPatterns : outputPatterns;
  const patternKey = JSON.stringify(patterns);
  useEffect(() => {
    const generation = ++epoch.current;
    cancelActive();
    queueMicrotask(() => { if (mounted.current && epoch.current === generation) { setRows(null); setBusy(false); } });
  }, [scope, patternKey, cancelActive]);
  useEffect(() => {
    mounted.current = true;
    return () => { mounted.current = false; epoch.current += 1; cancelActive(); };
  }, [cancelActive]);
  const preview = () => {
    if (busy) return;
    const generation = ++epoch.current;
    const request = { id: crypto.randomUUID(), scope };
    active.current = request;
    setBusy(true);
    setCancelError(null);
    void run(async () => {
      if (patterns.length > 100 || patterns.some(pattern => !isExclusionPatternWithinLimit(pattern))) {
        throw new Error(t("最多 100 条排除规则，单条不超过 512 UTF-8 字节。"));
      }
      try {
        const result = await api.previewExclusions(text.split(/\r?\n/).filter(path => path.length > 0), patterns, scope, request.id);
        if (generation === epoch.current) setRows(result.map((row, index) => ({ ...row, key: String(index) + '-' + row.path })));
      } catch (error) {
        if (generation === epoch.current) throw error;
      }
    }).finally(() => {
      if (active.current === request) active.current = null;
      if (mounted.current && generation === epoch.current) setBusy(false);
    }).catch(() => { /* App.run reports the operation error. */ });
  };
  return <div className="rule-preview">
    <Typography.Title level={5}>{t("匹配规则试算")}</Typography.Title>
    <Alert type="info" showIcon title={t("规则匹配预览")} description={scope === 'output'
      ? t("仅预览规则匹配效果，不会实际解压文件。按归档条目试算：目录请以 / 或 \\ 结尾，并与子文件一起列出；命中的目录会排除其后代。未列出的目录不推测为归档目录条目。")
      : t("仅预览规则匹配效果，不会实际解压文件。输入范围试算单个选择的文件或目录自身；批量扫描还会跳过被排除的父目录，其子文件不会入队。")} />
    <Space wrap style={{ margin: '12px 0' }}><Select aria-label={t("规则试算范围")} value={scope} onChange={setScope} options={[{ value: 'input', label: t("扫描输入规则") }, { value: 'output', label: t("归档输出规则") }]} /><Typography.Text type="secondary">{t("当前 {count} 条规则", {count: patterns.length})}</Typography.Text></Space>
    <Input.TextArea aria-label={t("待试算的相对路径")} rows={4} maxLength={65536} placeholder={scope === 'output' ? t("每行一个归档条目，例如 temp/ 和 temp/文件.bin；目录行以 / 结尾，路径中的空格保留") : t("每行一个相对路径，例如 资料/说明.url；路径中的空格保留")} value={text} onChange={event => { epoch.current += 1; cancelActive(); setBusy(false); setText(event.target.value); setRows(null); }} />
    <Button disabled={!connected || !text.length || busy} loading={busy} onClick={preview} style={{ margin: '12px 0' }}>{t("预览匹配")}</Button>
    {busy && <Space><Typography.Text>{t("等待可用资源或试算中…")}</Typography.Text><Button onClick={() => { epoch.current += 1; cancelActive(); setBusy(false); }}>{t("停止试算")}</Button></Space>}
    {cancelError && <Alert type="warning" showIcon title={cancelError} />}
    {rows && <Table rowKey="key" dataSource={rows} size="small" pagination={{ pageSize: 6 }} columns={[{ title: t("相对路径"), dataIndex: 'path' }, { title: t("当前规则"), key: 'matched', render: (_, row) => <Tag color={row.matched ? 'warning' : 'default'}>{row.matched ? t("命中：将排除") : t("未命中：保留")}</Tag> }]} />}
  </div>;
}
