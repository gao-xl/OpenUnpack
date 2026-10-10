import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { App as AntApp, Alert, Button, Empty, Input, InputNumber, Modal, Progress, Select, Space, Switch, Table, Tag, Typography } from 'antd';
import { FolderAddOutlined, FileAddOutlined, PlayCircleOutlined, PauseCircleOutlined, StopOutlined, SettingOutlined, DeleteOutlined } from '@ant-design/icons';
import type { ColumnsType } from 'antd/es/table';
import type { SharedProps } from '../App';
import { api, isDesktop } from '../api';
import type { JobView, ScanResult, Settings, SourceGroup } from '../types';
import { stageLabels } from '../types';
import { formatBytes, JobDetails, percent, retryable, StatusTag, terminal } from './shared';
import SourceCleanupSelect from './SourceCleanupSelect';

export default function QueuePage({ snapshot, run }: SharedProps) {
  const { modal } = AntApp.useApp();
  const [query, setQuery] = useState('');
  const [filter, setFilter] = useState('all');
  const [selected, setSelected] = useState<string[]>([]);
  const [detail, setDetail] = useState<string>();
  const [scan, setScan] = useState<ScanResult>();
  const [sources, setSources] = useState<string[]>([]);
  const [options, setOptions] = useState<Settings>(snapshot.settings);
  const [showOptions, setShowOptions] = useState(false);
  const [scanning, setScanning] = useState(false);
  const [scanStopping, setScanStopping] = useState(false);
  const activeScan = useRef<{ id: string; cancelled: boolean } | null>(null);
  const settingsRef = useRef(snapshot.settings);
  settingsRef.current = snapshot.settings;
  const addPaths = useCallback((paths: string[]) => {
    if (!paths.length || activeScan.current) return;
    const current = { id: crypto.randomUUID(), cancelled: false };
    activeScan.current = current;
    setScanning(true); setScanStopping(false); setScan(undefined);
    void run(async () => {
      try {
        const result = await api.scan(paths, current.id);
        if (activeScan.current === current && !current.cancelled) {
          setScan(result);
          setSources(result.sources.filter(source => !source.missingVolume).map(source => source.path));
          setOptions({ ...settingsRef.current });
        }
      } catch (error) { if (!current.cancelled) throw error; }
      finally { if (activeScan.current === current) { activeScan.current = null; setScanning(false); setScanStopping(false); } }
    });
  }, [run]);
  const cancelScan = () => {
    const current = activeScan.current;
    if (!current || current.cancelled) return;
    current.cancelled = true; setScanStopping(true);
    void run(() => api.cancelScan(current.id));
  };
  useEffect(() => {
    let disposed = false;
    let stop: (() => void) | undefined;
    void api.onDrop(addPaths).then(cleanup => disposed ? cleanup() : (stop = cleanup)).catch(() => {});
    return () => { disposed = true; stop?.(); const current = activeScan.current; if (current) { current.cancelled = true; void api.cancelScan(current.id).catch(() => {}); activeScan.current = null; } };
  }, [addPaths]);
  const pick = (folder: boolean) => void run(async () => {
    const paths = await api.pickFiles(folder);
    if (!paths.length) return;
    addPaths(paths);
  });
  useEffect(() => { setSelected(previous => previous.filter(id => snapshot.jobs.some(job => job.id === id))); }, [snapshot.jobs]);
  const counts = useMemo(() => ({ total: snapshot.jobs.length, running: snapshot.jobs.filter(job => job.status === 'running').length, waiting: snapshot.jobs.filter(job => ['queued', 'paused', 'waitingResource'].includes(job.status)).length, completed: snapshot.jobs.filter(job => ['completed', 'cleanupPending'].includes(job.status)).length, errors: snapshot.jobs.filter(retryable).length }), [snapshot.jobs]);
  const jobs = snapshot.jobs.filter(job => (!query || `${job.source.displayName} ${job.source.path}`.toLowerCase().includes(query.toLowerCase())) && (filter === 'all' || (filter === 'waiting' ? ['queued', 'paused', 'waitingResource'].includes(job.status) : filter === 'failed' ? retryable(job) : job.status === filter)));
  const selectJobs = snapshot.jobs.filter(job => selected.includes(job.id));
  const operateSelected = (operation: (job: JobView) => Promise<unknown>) => void run(async () => { for (const job of selectJobs) await operation(job); setSelected([]); });
  const cancel = (ids: string[]) => modal.confirm({ title: '停止所选任务？', content: '将请求停止当前作业。原始压缩包始终保留，现有输出会按恢复规则处理。', okText: '停止任务', cancelText: '返回', onOk: () => run(async () => { for (const id of ids) await api.cancel(id); setSelected([]); }) });
  const columns: ColumnsType<JobView> = [
    { title: '归档文件', key: 'name', width: 265, render: (_, job) => <><Button type="link" style={{ padding: 0 }} onClick={() => setDetail(job.id)}>{job.source.displayName}</Button><div className="muted">{formatBytes(job.source.size)}{job.source.volumes.length > 1 ? ` · ${job.source.volumes.length} 个分卷` : ''}</div></> },
    { title: '状态', key: 'status', width: 140, render: (_, job) => <StatusTag status={job.status} /> },
    { title: '当前阶段', key: 'stage', width: 210, render: (_, job) => <><span>{stageLabels[job.stage] ?? job.stage}</span><Progress size="small" percent={percent(job.progress)} status={job.status === 'failed' ? 'exception' : undefined} /></> },
    { title: '密码范围', key: 'password', width: 145, render: (_, job) => job.options.passwordGroup || '全部启用密码' },
    { title: '输出 / 提示', key: 'message', width: 240, ellipsis: true, render: (_, job) => job.message || job.outputPath || job.options.outputRoot || '原始归档所在目录' },
    { title: '操作', key: 'actions', width: 160, fixed: 'right', render: (_, job) => <Space size={0}><Button size="small" type="link" onClick={() => setDetail(job.id)}>详情</Button>{retryable(job) && <Button size="small" type="link" disabled={!isDesktop || snapshot.running} onClick={() => void run(() => api.retry(job.id))}>重试</Button>}{['running', 'queued', 'paused'].includes(job.status) && <Button size="small" type="link" danger disabled={!isDesktop} onClick={() => cancel([job.id])}>停止</Button>}{job.outputPath && ['completed', 'cleanupPending', 'partial'].includes(job.status) && <Button size="small" type="link" disabled={!isDesktop} onClick={() => void run(() => api.openPath(job.outputPath!))}>打开</Button>}</Space> },
  ];
  const sourceColumns: ColumnsType<SourceGroup> = [{ title: '归档', key: 'source', render: (_, source) => <><strong>{source.displayName}</strong><div className="muted" style={{ wordBreak: 'break-all' }}>{source.path}</div></> }, { title: '大小', dataIndex: 'size', render: formatBytes }, { title: '分卷', key: 'volumes', render: (_, source) => source.missingVolume ? <Tag color="error">缺少分卷</Tag> : `${source.volumes.length || 1} 卷` }];
  const groups = [...new Set(snapshot.passwords.flatMap(password => password.groups))];
  return <>
    <div className="page-header"><div><Typography.Title level={3}>批量解压</Typography.Title><Typography.Paragraph type="secondary">添加归档或文件夹，选择密码范围与处理策略。默认保留源包；在结果核对完成后发布。</Typography.Paragraph></div><Space><Button aria-label="添加文件" icon={<FileAddOutlined aria-hidden />} disabled={!isDesktop || scanning} onClick={() => pick(false)}>添加文件</Button><Button aria-label="添加文件夹" type="primary" icon={<FolderAddOutlined aria-hidden />} disabled={!isDesktop || scanning} onClick={() => pick(true)}>添加文件夹</Button></Space></div>
    <div className="summary-grid">{[['全部任务', counts.total], ['处理中', counts.running], ['等待处理', counts.waiting], ['已完成', counts.completed], ['需要处理', counts.errors]].map(([name, count]) => <div className="summary-card" key={name}><span className="muted">{name}</span><strong>{count}</strong></div>)}</div>
    {!snapshot.engineAvailable && isDesktop && <Alert showIcon type="warning" title="解压引擎尚未就绪" description="请确认官方 7-Zip 引擎与作业进程已安装。可管理队列与密码库，开始前需恢复引擎。" style={{ marginBottom: 16 }} />}
    <div className="section-card">
      <div className="toolbar"><Space wrap><Button type="primary" icon={<PlayCircleOutlined />} disabled={!isDesktop || !snapshot.engineAvailable || snapshot.running || !snapshot.jobs.some(job => ['queued', 'paused'].includes(job.status))} onClick={() => void run(() => api.start())}>{snapshot.paused ? '继续队列' : '开始队列'}</Button><Button icon={<PauseCircleOutlined />} disabled={!isDesktop || !snapshot.running || snapshot.paused} onClick={() => void run(() => api.pause())}>暂停新任务</Button><Button icon={<StopOutlined />} disabled={!isDesktop || !selected.length} onClick={() => cancel(selected)}>停止所选</Button><Button disabled={!isDesktop || snapshot.running || !selectJobs.some(retryable)} onClick={() => operateSelected(job => retryable(job) ? api.retry(job.id) : Promise.resolve())}>重试所选</Button><Button icon={<DeleteOutlined />} disabled={!isDesktop || !selectJobs.length || selectJobs.some(job => !terminal(job) || !!job.workspace)} onClick={() => operateSelected(job => api.remove(job.id))}>移除记录</Button></Space><Button disabled={!isDesktop || !counts.completed} onClick={() => void run(() => api.clearCompleted())}>清除已完成</Button></div>
      <div className="toolbar"><Space><Input.Search aria-label="搜索归档" placeholder="搜索文件名或来源路径" allowClear value={query} onChange={event => setQuery(event.target.value)} style={{ width: 290 }} /><Select aria-label="任务状态筛选" value={filter} onChange={setFilter} style={{ width: 150 }} options={[{ value: 'all', label: '全部状态' }, { value: 'running', label: '处理中' }, { value: 'waiting', label: '等待处理' }, { value: 'failed', label: '需要处理' }, { value: 'completed', label: '已完成' }]} /></Space><Typography.Text type="secondary">SSD {snapshot.settings.cacheEnabled ? '启用' : '关闭'} · 内包 {snapshot.settings.expandNested ? `共 ${snapshot.settings.maxDepth} 层` : '保留'} · {snapshot.settings.verification === 'strict' ? '目标回读' : '标准验证'}</Typography.Text></div>
      <Table<JobView> rowKey="id" columns={columns} dataSource={jobs} virtual scroll={{ x: 1160, y: 390 }} pagination={false} rowSelection={{ selectedRowKeys: selected, onChange: keys => setSelected(keys.map(String)) }} locale={{ emptyText: <Empty description={isDesktop ? '拖入归档文件，或使用上方按钮添加文件与文件夹' : '真实空队列 · 打开桌面版后可添加文件'} /> }} />
    </div>
    <Typography.Paragraph type="secondary" style={{ marginTop: 12 }}>初版按一个根任务顺序处理；暂停会阻止启动下一项，停止会请求当前作业退出。进度与完成状态由本地核心报告。</Typography.Paragraph>
    <Modal title="正在扫描来源" open={scanning} closable={false} destroyOnHidden footer={<Button danger disabled={scanStopping} onClick={cancelScan}>{scanStopping ? '等待扫描停止' : '取消扫描'}</Button>}>
      <Typography.Paragraph>正在查找支持的归档并归组分卷。扫描不会验证归档内容是否完整，也不会修改源文件。</Typography.Paragraph>
      <Typography.Text type="secondary">{scanStopping ? '取消请求已发出，等待读取结束。' : `扫描规则：${snapshot.settings.recursiveScan ? '包含子目录' : '当前目录'}，${snapshot.settings.inputExclusions.length} 条输入排除规则。`}</Typography.Text>
    </Modal>
    <Modal title="检查扫描结果" open={!!scan} onCancel={() => setScan(undefined)} width={820} okText={`添加 ${sources.length} 项`} cancelText="返回" okButtonProps={{ disabled: !isDesktop || !sources.length }} onOk={() => void run(async () => { if (scan) await api.addJobs(scan.sources.filter(source => sources.includes(source.path)), { ...options }); setScan(undefined); })}>
      <Alert type="info" showIcon title={`发现 ${scan?.sources.length ?? 0} 个归档任务`} description="这是扫描发现结果。分卷命名连续不代表内容已经校验；缺卷项需补齐来源后再次扫描。下方展开每项可查看实际归组的卷路径。" style={{ marginBottom: 12 }} />
      {!!scan?.notices.length && <><Typography.Title level={5}>扫描跳过与访问提示（{scan.notices.length} 项）</Typography.Title><Table size="small" rowKey={(notice, index) => `${index}-${notice.path}`} dataSource={scan.notices} pagination={{ pageSize: 4 }} columns={[{ title: '路径', dataIndex: 'path', ellipsis: true }, { title: '提示', dataIndex: 'message' }]} /></>}
      <Table<SourceGroup> rowKey="path" size="small" columns={sourceColumns} dataSource={scan?.sources} expandable={{ expandedRowRender: source => <ul>{source.volumes.map(path => <li key={path} style={{ wordBreak: 'break-all' }}>{path}</li>)}</ul> }} rowSelection={{ selectedRowKeys: sources, onChange: keys => setSources(keys.map(String)), getCheckboxProps: source => ({ disabled: source.missingVolume }) }} pagination={{ pageSize: 8 }} />
      <div className="toolbar"><Typography.Text type="secondary">输出：{options.outputRoot || '源归档所在目录'} · {options.cacheEnabled ? 'SSD 工作目录' : '目标卷临时目录'} · {options.expandNested ? `共 ${options.maxDepth} 层展开` : '保留内包'}</Typography.Text><Button icon={<SettingOutlined />} onClick={() => setShowOptions(true)}>本批次选项</Button></div>
    </Modal>
    <Modal title="本批次处理选项" open={showOptions} onCancel={() => setShowOptions(false)} onOk={() => setShowOptions(false)} okText="使用这些选项" cancelText="返回">
      <Typography.Paragraph type="secondary">仅影响本批次；加入队列后设置会冻结。</Typography.Paragraph>
      <label className="setting-row"><span>输出根目录</span><Space.Compact style={{ width: '100%' }}><Input readOnly aria-label="本批次输出目录" value={options.outputRoot} placeholder="留空在源归档旁创建结果目录" /><Button disabled={!isDesktop} onClick={() => void run(async () => { const path = await api.pickDirectory(); if (path) setOptions({ ...options, outputRoot: path }); })}>选择</Button><Button onClick={() => setOptions({ ...options, outputRoot: '' })}>清空</Button></Space.Compact></label>
      <div className="setting-row"><span>同名目标</span><Select style={{ width: 220 }} value={options.conflictPolicy} onChange={conflictPolicy => setOptions({ ...options, conflictPolicy })} options={[{ value: 'rename', label: '新建名称，保留已有内容' }, { value: 'skip', label: '跳过该任务' }]} /></div>
      <div className="setting-row"><span>密码分组</span><Select value={options.passwordGroup} style={{ width: 220 }} options={[{ value: '', label: '全部启用密码' }, ...groups.map(group => ({ value: group, label: group }))]} onChange={passwordGroup => setOptions({ ...options, passwordGroup })} /></div>
      <div className="setting-row"><span>展开内包</span><Space><Switch checked={options.expandNested} onChange={expandNested => setOptions({ ...options, expandNested })} /><InputNumber aria-label="总展开层数（含外包）" min={1} max={8} disabled={!options.expandNested} value={options.maxDepth} onChange={value => setOptions({ ...options, maxDepth: value ?? 2 })} /></Space></div>
      <div className="setting-row"><span>SSD 工作目录</span><Switch checked={options.cacheEnabled} onChange={cacheEnabled => setOptions({ ...options, cacheEnabled })} /></div>
      {options.cacheEnabled && <Alert type={options.cacheRoot ? 'info' : 'warning'} title={options.cacheRoot || '先在设置中选择 SSD 工作目录'} showIcon />}
      <div className="setting-row"><span>智能去套娃</span><Switch checked={options.unwrapSingleRoot} onChange={unwrapSingleRoot => setOptions({ ...options, unwrapSingleRoot })} /></div>
      <div className="setting-row"><span>验证方式</span><Select value={options.verification} options={[{ value: 'standard', label: '标准流程验证' }, { value: 'strict', label: '完整目标回读' }]} onChange={verification => setOptions({ ...options, verification })} style={{ width: 220 }} /></div>
      <div className="setting-row">
        <span>完成后源包</span>
        <SourceCleanupSelect value={options.sourceCleanup} onChange={sourceCleanup => setOptions({ ...options, sourceCleanup })} disabled={!isDesktop} />
      </div>
    </Modal>
    <JobDetails job={snapshot.jobs.find(job => job.id === detail)} close={() => setDetail(undefined)} run={run} operationsDisabled={snapshot.running} />
  </>;
}
