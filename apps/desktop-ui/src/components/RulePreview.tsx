import { useEffect, useRef, useState } from 'react';
import { Alert, Button, Input, Select, Space, Table, Tag, Typography } from 'antd';
import { api, isDesktop } from '../api';

export default function RulePreview({ inputPatterns, outputPatterns, run }: { inputPatterns: string[]; outputPatterns: string[]; run: (work: () => Promise<unknown>) => Promise<void> }) {
  const [scope, setScope] = useState('input');
  const [text, setText] = useState('');
  const [rows, setRows] = useState<{ path: string; matched: boolean }[] | null>(null);
  const [busy, setBusy] = useState(false);
  const epoch = useRef(0);
  const patterns = scope === 'input' ? inputPatterns : outputPatterns;
  const patternKey = JSON.stringify(patterns);
  useEffect(() => { epoch.current += 1; setRows(null); }, [scope, patternKey]);
  useEffect(() => () => { epoch.current += 1; }, []);
  const preview = () => {
    if (busy) return;
    const generation = ++epoch.current;
    setBusy(true);
    void run(async () => {
      const result = await api.previewExclusions(text.split(/\r?\n/).filter(path => path.length > 0), patterns);
      if (generation === epoch.current) setRows(result);
    }).finally(() => setBusy(false));
  };
  return <div className="rule-preview">
    <Typography.Title level={5}>匹配规则试算</Typography.Title>
    <Alert type="info" showIcon title="试算你提供的相对路径" description="这是当前规则的匹配预览，不读取或声称已经列出压缩包内容。扫描提示与任务详情会记录实际过滤范围。" />
    <Space wrap style={{ margin: '12px 0' }}><Select aria-label="规则试算范围" value={scope} onChange={setScope} options={[{ value: 'input', label: '扫描输入规则' }, { value: 'output', label: '归档输出规则' }]} /><Typography.Text type="secondary">当前 {patterns.length} 条规则</Typography.Text></Space>
    <Input.TextArea aria-label="待试算的相对路径" rows={4} maxLength={65536} placeholder="每行一个相对路径，例如 资料/说明.url；路径中的空格保留" value={text} onChange={event => { epoch.current += 1; setText(event.target.value); setRows(null); }} />
    <Button disabled={!isDesktop || !text.length || busy} loading={busy} onClick={preview} style={{ margin: '12px 0' }}>预览匹配</Button>
    {rows && <Table rowKey={(row, index) => `${index}-${row.path}`} dataSource={rows} size="small" pagination={{ pageSize: 6 }} columns={[{ title: '相对路径', dataIndex: 'path' }, { title: '当前规则', key: 'matched', render: (_, row) => <Tag color={row.matched ? 'warning' : 'default'}>{row.matched ? '命中：将排除' : '未命中：保留'}</Tag> }]} />}
  </div>;
}
