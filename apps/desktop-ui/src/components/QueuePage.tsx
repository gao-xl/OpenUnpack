import { t } from '../i18n';
import { useCallback, useDeferredValue, useEffect, useMemo, useRef, useState } from 'react';
import { App as AntApp, Alert, Button, Checkbox, Input, InputNumber, Modal, Progress, Select, Space, Switch, Table, Tag, Typography } from 'antd';
import { FolderAddOutlined, FileAddOutlined, PlayCircleOutlined, PauseCircleOutlined, StopOutlined, SettingOutlined, DeleteOutlined } from '@ant-design/icons';
import type { ColumnsType } from 'antd/es/table';
import type { SharedProps } from '../App';
import { api, isDesktop } from '../api';
import type { JobView, ScanResult, Settings, SourceGroup } from '../types';
import { stageLabels } from '../types';
import { formatBytes, displayPath, JobDetails, percent, retryable, StatusTag } from './shared';
import SourceCleanupSelect, { SourceCleanupScope } from './SourceCleanupSelect';
import TransferMetrics from './TransferMetrics';
import { taskDeleteBlockedReason, useTaskDeletion } from './TaskDeleteDialog';

export default function QueuePage({ snapshot, run, openHistory }: SharedProps & { openHistory?: () => void }) {
  const { modal } = AntApp.useApp();
  const daemon = api.isDaemonMode?.() === true;
  const connected = isDesktop || daemon;
  const nativeEngine = isDesktop && !daemon;
  const [serverPath, setServerPath] = useState('');
  const [query, setQuery] = useState('');
  const deferredQuery = useDeferredValue(query);
  const [filter, setFilter] = useState('all');
  const [selected, setSelected] = useState<string[]>([]);
  const selectedIds = useMemo(() => new Set(selected), [selected]);
  const [detail, setDetail] = useState<string>();
  const deletion = useTaskDeletion(run, ids => {
    const removedIds = new Set(ids);
    setSelected(previous => previous.filter(id => !removedIds.has(id)));
    setDetail(previous => previous && removedIds.has(previous) ? undefined : previous);
  });
  const [scan, setScan] = useState<ScanResult>();
  const [sources, setSources] = useState<string[]>([]);
  const selectedSourcePaths = useMemo(() => new Set(sources), [sources]);
  const [options, setOptions] = useState<Settings>(snapshot.settings);
  const [showOptions, setShowOptions] = useState(false);
  const [scanning, setScanning] = useState(false);
  const [scanStopping, setScanStopping] = useState(false);
  const [passwordJob, setPasswordJob] = useState<JobView | null>(null);
  const [inputPassword, setInputPassword] = useState('');
  const [saveToVault, setSaveToVault] = useState(!daemon);
  const [submittingPassword, setSubmittingPassword] = useState(false);
  const passwordDraftEpoch = useRef(0);
  const passwordRequestEpoch = useRef<number | null>(null);
  const closePassword = useCallback(() => {
    passwordDraftEpoch.current += 1;
    passwordRequestEpoch.current = null;
    setPasswordJob(null);
    setInputPassword('');
    setSubmittingPassword(false);
  }, []);
  useEffect(() => () => { passwordDraftEpoch.current += 1; }, []);
  const passwordOwnerPresent = !passwordJob || snapshot.jobs.some(job => job.id === passwordJob.id);
  useEffect(() => {
    if (passwordOwnerPresent) return;
    const epoch = ++passwordDraftEpoch.current;
    queueMicrotask(() => { if (passwordDraftEpoch.current === epoch) closePassword(); });
  }, [passwordOwnerPresent, closePassword]);
  const [addingJobs, setAddingJobs] = useState(false);
  const addBusyRef = useRef(false);
  const scanEpochRef = useRef(0);
  const activeScan = useRef<{ id: string; cancelled: boolean } | null>(null);
  const settingsRef = useRef(snapshot.settings);
  useEffect(() => {
    settingsRef.current = snapshot.settings;
  }, [snapshot.settings]);
  const addPaths = useCallback((paths: string[]) => {
    if (!paths.length || activeScan.current || addBusyRef.current) return;
    const current = { id: crypto.randomUUID(), cancelled: false };
    activeScan.current = current;
    scanEpochRef.current += 1;
    setScanning(true); setScanStopping(false); setScan(undefined);
    void run(async () => {
      try {
        const result = await api.scan(paths, current.id);
        if (activeScan.current === current && !current.cancelled) {
          scanEpochRef.current += 1;
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
  useEffect(() => {
    const existingIds = new Set(snapshot.jobs.map(job => job.id));
    queueMicrotask(() => {
      setSelected(previous => {
        const next = previous.filter(id => existingIds.has(id));
        return next.length === previous.length ? previous : next;
      });
    });
  }, [snapshot.jobs]);
  const counts = useMemo(() => ({ total: snapshot.jobs.length, running: snapshot.jobs.filter(job => job.status === 'running').length, waiting: snapshot.jobs.filter(job => ['queued', 'paused', 'waitingResource'].includes(job.status)).length, completed: snapshot.jobs.filter(job => ['completed', 'cleanupPending'].includes(job.status)).length, errors: snapshot.jobs.filter(retryable).length }), [snapshot.jobs]);
  const jobs = useMemo(() => {
    const normalizedQuery = deferredQuery.trim().toLowerCase();
    return snapshot.jobs.filter(job => (!normalizedQuery || `${job.source.displayName} ${job.source.path}`.toLowerCase().includes(normalizedQuery)) && (filter === 'all' || (filter === 'waiting' ? ['queued', 'paused', 'waitingResource'].includes(job.status) : filter === 'failed' ? retryable(job) : job.status === filter)));
  }, [snapshot.jobs, deferredQuery, filter]);
  const selectJobs = useMemo(() => snapshot.jobs.filter(job => selectedIds.has(job.id)), [snapshot.jobs, selectedIds]);
  const selectedStartIds = selectJobs.filter(job => ['queued', 'waitingResource'].includes(job.status) || (daemon && snapshot.paused && job.status === 'running')).map(job => job.id);
  const startDisabled = !connected || deletion.busy || !snapshot.engineAvailable || (snapshot.running && !snapshot.paused) || (daemon ? !selectedStartIds.length : !snapshot.jobs.some(job => ['queued', 'paused'].includes(job.status)));
  const startQueue = () => void run(() => daemon ? api.startSelected(selectedStartIds) : api.start());
  const completedJobs = snapshot.jobs.filter(job => ['completed', 'partial', 'salvaged', 'skipped', 'cleanupPending'].includes(job.status));
  const operateSelected = (operation: (job: JobView) => Promise<unknown>) => void run(async () => { for (const job of selectJobs) await operation(job); setSelected([]); });
  const cancel = (ids: string[]) => modal.confirm({ title: t("停止所选任务？"), content: t("将请求停止当前作业。原始压缩包始终保留，现有输出会按恢复规则处理。"), okText: t("停止任务"), cancelText: t("返回"), onOk: () => run(async () => { for (const id of ids) await api.cancel(id); setSelected([]); }) });

  const getExtensionBadge = (displayName: string) => {
    const parts = displayName.split('.');
    let ext = parts.length > 1 ? parts.pop()?.toUpperCase() || 'ZIP' : 'ZIP';
    if (ext.length > 4) ext = ext.slice(0, 4);
    if (ext === '7Z' || /^\d{3}$/.test(ext)) {
      return { label: ext, bg: 'var(--ou-primary-bg)', text: 'var(--ou-primary)', border: 'var(--ou-primary-border)' };
    }
    if (ext === 'RAR') {
      return { label: 'RAR', bg: 'var(--ou-success-bg)', text: 'var(--ou-success)', border: 'var(--ou-success-border)' };
    }
    if (['TAR', 'GZ', 'BZ2', 'XZ'].includes(ext)) {
      return { label: ext, bg: 'var(--ou-accent-bg)', text: 'var(--ou-accent)', border: 'var(--ou-accent-border)' };
    }
    return { label: ext, bg: 'var(--ou-warning-bg)', text: 'var(--ou-warning)', border: 'var(--ou-warning-border)' };
  };

  const columns: ColumnsType<JobView> = [
    {
      title: t("归档文件"),
      key: 'name',
      width: 275,
      render: (_, job) => {
        const badge = getExtensionBadge(job.source.displayName);
        return (
          <div style={{ display: 'flex', alignItems: 'flex-start', gap: 8 }}>
            <span style={{
              fontSize: 10,
              fontFamily: 'var(--ou-font-mono)',
              fontWeight: 600,
              padding: '1px 5px',
              borderRadius: 4,
              backgroundColor: badge.bg,
              color: badge.text,
              border: `0.5px solid ${badge.border}`,
              lineHeight: '14px',
              marginTop: 2,
              flexShrink: 0,
            }}>
              {badge.label}
            </span>
            <div style={{ minWidth: 0, flex: 1 }}>
              <Button
                type="link"
                style={{
                  padding: 0,
                  fontWeight: 500,
                  fontSize: 13,
                  height: 'auto',
                  textAlign: 'left',
                  whiteSpace: 'normal',
                  wordBreak: 'break-all',
                }}
                onClick={() => setDetail(job.id)}
              >
                {job.source.displayName}
              </Button>
              <div className="muted mono-text" style={{ fontSize: 11, marginTop: 2 }}>
                {formatBytes(job.source.size)}
                {job.source.volumes.length > 1 ? t(" · {p0} 个分卷", { p0: job.source.volumes.length }) : ''}
              </div>
            </div>
          </div>
        );
      },
    },
    { title: t("状态"), key: 'status', width: 140, render: (_, job) => <StatusTag status={job.status} /> },
    { title: t("当前阶段"), key: 'stage', width: 250, render: (_, job) => <><span>{t(stageLabels[job.stage] ?? job.stage)}</span><Progress size="small" percent={percent(job.progress)} status={job.status === 'failed' ? 'exception' : undefined} /><TransferMetrics job={job} /></> },
    { title: t("密码范围"), key: 'password', width: 145, render: (_, job) => job.options.passwordGroup || t("全部启用密码") },
    { title: t("输出 / 提示"), key: 'message', width: 240, ellipsis: true, render: (_, job) => job.message || displayPath(job.outputPath) || displayPath(job.options.outputRoot) || t("原始归档所在目录") },
    { title: t("操作"), key: 'actions', width: 245, fixed: 'right', render: (_, job) => <Space size={0} wrap>
      <Button size="small" type="link" onClick={() => setDetail(job.id)}>{t("详情")}</Button>
      {(job.status === 'needsPassword' || (daemon && job.status === 'running' && job.stage === 'needsPassword')) && (
        <Button
          size="small"
          type="link"
          style={{ color: 'var(--ou-warning)', fontWeight: 600, padding: '0 4px' }}
          disabled={!connected || deletion.busy}
          onClick={() => {
            passwordDraftEpoch.current += 1;
            passwordRequestEpoch.current = null;
            setPasswordJob(job);
            setInputPassword('');
            setSaveToVault(!daemon);
            setSubmittingPassword(false);
          }}
        >{t("输入密码")}</Button>
      )}
      {retryable(job) && job.status !== 'needsPassword' && (
        <>
          <Button size="small" type="link" disabled={!connected || snapshot.running || deletion.busy} onClick={() => void run(() => api.retry(job.id))}>{t("重试")}</Button>
          <Button
            size="small"
            type="link"
            style={{ padding: '0 4px' }}
            disabled={!connected || snapshot.running || deletion.busy}
            onClick={() => {
              passwordDraftEpoch.current += 1;
              passwordRequestEpoch.current = null;
              setPasswordJob(job);
              setInputPassword('');
              setSaveToVault(false);
              setSubmittingPassword(false);
            }}
          >{t("指定密码重试")}</Button>
        </>
      )}
      {['running', 'queued', 'paused'].includes(job.status) && <Button size="small" type="link" danger disabled={!connected || deletion.busy} onClick={() => void cancel([job.id])}>{t("停止")}</Button>}
      {job.status === 'stopping' && (
        <Button
          size="small"
          type="link"
          danger
          style={{ padding: '0 4px' }}
          disabled={!connected || deletion.busy}
          title={t("退出尚未确认时保留任务和临时缓存；停止完成或重启后处理恢复记录")}
          onClick={() => deletion.request([job])}
        >{t("尝试删除")}</Button>
      )}
      {job.outputPath && ['completed', 'cleanupPending', 'partial', 'salvaged'].includes(job.status) && <Button size="small" type="link" disabled={!nativeEngine} onClick={() => void run(() => api.openPath(job.outputPath!))}>{t("打开")}</Button>}
      <Button size="small" type="link" danger aria-label={t("删除任务 {p0}", { p0: job.source.displayName })} title={taskDeleteBlockedReason(job)} disabled={!connected || deletion.busy || !!taskDeleteBlockedReason(job)} loading={deletion.deletingIds.includes(job.id)} onClick={() => deletion.request([job])}>{deletion.deletingIds.includes(job.id) ? t("正在删除") : t("删除任务")}</Button>
    </Space> },
  ];
  const sourceColumns: ColumnsType<SourceGroup> = [{ title: t("归档"), key: 'source', render: (_, source) => <><strong>{source.displayName}</strong><div className="muted" style={{ wordBreak: 'break-all' }}>{displayPath(source.path)}</div></> }, { title: t("大小"), dataIndex: 'size', render: formatBytes }, { title: t("分卷"), key: 'volumes', render: (_, source) => source.missingVolume ? <Tag color="error">{t("缺少分卷")}</Tag> : t("{p0} 卷", { p0: source.volumes.length || 1 }) }];
  const groups = [...new Set(snapshot.passwords.flatMap(password => password.groups))];
  const totalSelectedVolumes = useMemo(() => {
    if (!scan) return 0;
    return scan.sources
      .filter(source => selectedSourcePaths.has(source.path))
      .reduce((sum, s) => sum + (s.volumes.length || 1), 0);
  }, [scan, selectedSourcePaths]);

  return <>
    <div className="page-header"><div><Typography.Title level={3}>{t("批量解压")}</Typography.Title><Typography.Paragraph type="secondary">{t("添加压缩文件或文件夹进行批量解压，支持分卷识别与密码匹配。")}</Typography.Paragraph></div><Space><Button aria-label={t("添加文件")} icon={<FileAddOutlined aria-hidden />} disabled={!nativeEngine || scanning} onClick={() => pick(false)}>{t("添加文件")}</Button><Button aria-label={t("添加文件夹")} type="primary" icon={<FolderAddOutlined aria-hidden />} disabled={!nativeEngine || scanning} onClick={() => pick(true)}>{t("添加文件夹")}</Button></Space></div>
    <div className="telemetry-bar">
      <div className="telemetry-card">
        <span className="telemetry-label">{t("当前显示")}</span>
        <span className="telemetry-value">{counts.total}</span>
      </div>
      <div className="telemetry-card">
        <span className="telemetry-label">{t("处理中")}</span>
        <span className="telemetry-value" style={{ color: counts.running > 0 ? 'var(--ou-primary)' : undefined }}>
          {counts.running}
        </span>
      </div>
      <div className="telemetry-card">
        <span className="telemetry-label">{t("等待处理")}</span>
        <span className="telemetry-value">{counts.waiting}</span>
      </div>
      <div className="telemetry-card">
        <span className="telemetry-label">{t("显示的已完成")}</span>
        <span className="telemetry-value" style={{ color: counts.completed > 0 ? 'var(--ou-success)' : undefined }}>
          {counts.completed}
        </span>
      </div>
      <div className="telemetry-card">
        <span className="telemetry-label">{t("显示的待处理")}</span>
        <span className="telemetry-value" style={{ color: counts.errors > 0 ? 'var(--ou-warning)' : undefined }}>
          {counts.errors}
        </span>
      </div>
    </div>
    <Alert type="info" showIcon title={t("显示全部待处理、待核对任务和最近 20 条其他已结束任务")} description={t("保留临时目录的任务始终显示。完整处理记录可在“处理记录”中按页查看；原始任务记录不会因队列显示范围而删除。")} action={openHistory ? <Button onClick={openHistory}>{t("查看处理记录")}</Button> : undefined} style={{ marginBottom: 16 }} />
    {daemon && <Space.Compact style={{ width: '100%', marginBottom: 16 }}><Input.Search aria-label={t('服务端归档路径')} value={serverPath} onChange={event => setServerPath(event.target.value)} placeholder={t('填写已批准目录内的服务端文件或文件夹路径')} enterButton={t('扫描路径')} disabled={scanning || deletion.busy} loading={scanning} onSearch={value => { if (value.trim()) addPaths([value.trim()]); }} /></Space.Compact>}
    {!snapshot.engineAvailable && connected && <Alert showIcon type="warning" title={t("解压引擎尚未就绪")} description={t("请确认官方 7-Zip 引擎与作业进程已安装。可管理队列与密码库，开始前需恢复引擎。")} style={{ marginBottom: 16 }} />}
    {snapshot.paused && snapshot.jobs.some(job => (daemon ? ['queued', 'waitingResource', 'running'] : ['queued', 'paused', 'waitingResource']).includes(job.status)) && (
      <Alert
        showIcon
        type="warning"
        title={t("队列当前处于暂停状态")}
        description={daemon ? t("请选择要继续的任务，再点击“继续所选”。") : t("新添加的任务已进入等待队列，请点击“继续队列”开始解压。")}
        action={<Button size="small" type="primary" aria-label={daemon ? t("横幅继续所选") : t("横幅继续队列")} disabled={startDisabled} onClick={startQueue}>{daemon ? t("继续所选") : t("继续队列")}</Button>}
        style={{ marginBottom: 16 }}
      />
    )}
    <div className="section-card">
      <div className="toolbar"><Space wrap><Button type="primary" icon={<PlayCircleOutlined aria-hidden />} disabled={startDisabled} onClick={startQueue}>{daemon ? (snapshot.paused ? t("继续所选") : t("开始所选")) : (snapshot.paused ? t("继续队列") : t("开始队列"))}</Button><Button icon={<PauseCircleOutlined aria-hidden />} disabled={!connected || !snapshot.running || snapshot.paused} onClick={() => void run(() => api.pause())}>{t("暂停新任务")}</Button><Button icon={<StopOutlined aria-hidden />} disabled={!connected || deletion.busy || !selected.length} onClick={() => void cancel(selected)}>{t("停止所选")}</Button><Button disabled={!connected || deletion.busy || snapshot.running || !selectJobs.some(retryable)} onClick={() => operateSelected(job => retryable(job) ? api.retry(job.id) : Promise.resolve())}>{t("重试所选")}</Button><Button icon={<DeleteOutlined aria-hidden />} aria-label={t("删除所选任务")} loading={deletion.busy} disabled={!connected || deletion.busy || !selectJobs.length} onClick={() => deletion.request(selectJobs)}>{t("删除所选任务")}</Button></Space><Button danger disabled={!connected || deletion.busy || !completedJobs.length} title={t("删除已完成任务记录并清理临时缓存，原包与已发布结果保留")} onClick={() => deletion.request(completedJobs)}>{t("删除当前显示的已结束任务")}</Button></div>
      <div className="toolbar"><Space><Input.Search aria-label={t("搜索归档")} placeholder={t("搜索文件名或来源路径")} allowClear value={query} onChange={event => setQuery(event.target.value)} style={{ width: 290 }} /><Select aria-label={t("任务状态筛选")} value={filter} onChange={setFilter} style={{ width: 150 }} options={[{ value: 'all', label: t("全部状态") }, { value: 'running', label: t("处理中") }, { value: 'waiting', label: t("等待处理") }, { value: 'failed', label: t("需要处理") }, { value: 'completed', label: t("已完成") }]} /></Space><Typography.Text type="secondary">SSD {snapshot.settings.cacheEnabled ? t("启用") : t("关闭")}{t("· 内包")}{snapshot.settings.expandNested ? t("共 {p0} 层", { p0: snapshot.settings.maxDepth }) : t("保留")} · {snapshot.settings.verification === 'strict' ? t("目标回读") : t("标准验证")}</Typography.Text></div>
      <Table<JobView>
        rowKey="id"
        columns={columns}
        dataSource={jobs}
        virtual
        scroll={{ x: 1275, y: 390 }}
        pagination={false}
        rowSelection={{ selectedRowKeys: selected, onChange: keys => setSelected(keys.map(String)), getCheckboxProps: job => ({ disabled: deletion.busy, 'aria-label': t("选择任务 {p0}", { p0: job.source.displayName }) }) }}
        locale={{
          emptyText: (
            <div
              className="dropzone-container"
              style={{ margin: '20px auto', maxWidth: 440, cursor: isDesktop ? 'pointer' : 'default' }}
              onClick={() => { if (isDesktop && !scanning) pick(false); }}
              role="region"
              aria-label={isDesktop ? t("暂无解压任务，拖入压缩文件或使用上方按钮添加") : t("暂无解压任务")}
            >
              <FileAddOutlined className="dropzone-icon" />
              <div className="dropzone-title">
                {isDesktop ? t("暂无解压任务，拖入压缩文件或使用上方按钮添加") : t("暂无解压任务")}
              </div>
              <div className="dropzone-sub">
                {t("添加压缩文件或文件夹进行批量解压，支持分卷识别与密码匹配。")}
              </div>
            </div>
          ),
        }}
      />
    </div>
    <Modal title={t("正在扫描来源")} open={scanning} closable={false} destroyOnHidden footer={<Button danger disabled={scanStopping} onClick={cancelScan}>{scanStopping ? t("等待扫描停止") : t("取消扫描")}</Button>}>
      <Typography.Paragraph>{t("正在查找支持的归档并归组分卷。扫描不会验证归档内容是否完整，也不会修改源文件。")}</Typography.Paragraph>
      <Typography.Text type="secondary">{scanStopping ? t("取消请求已发出，等待读取结束。") : t("扫描规则：{p0}，{p1} 条输入排除规则。", { p0: snapshot.settings.recursiveScan ? '包含子目录' : '当前目录', p1: snapshot.settings.inputExclusions.length })}</Typography.Text>
    </Modal>
    <Modal
      title={t("检查扫描结果")}
      open={!!scan}
      confirmLoading={addingJobs}
      cancelButtonProps={{ disabled: addingJobs }}
      onCancel={() => {
        if (addingJobs || addBusyRef.current) return;
        scanEpochRef.current += 1;
        setScan(undefined);
        setShowOptions(false);
      }}
      width={840}
      okText={t("添加 {p0} 项", { p0: sources.length })}
      cancelText={t("返回")}
      okButtonProps={{ disabled: !connected || !sources.length || addingJobs }}
      onOk={() => void run(async () => {
        if (!scan || addBusyRef.current) return;
        addBusyRef.current = true;
        setAddingJobs(true);
        const epoch = scanEpochRef.current;
        const frozenSources = scan.sources.filter(source => selectedSourcePaths.has(source.path));
        const frozenOptions = { ...options };
        try {
          const added = await api.addJobs(frozenSources, frozenOptions);
          const autoStart = frozenOptions.autoStartOnAdd ?? snapshot.settings.autoStartOnAdd ?? true;
          if (autoStart && !snapshot.paused && snapshot.engineAvailable) {
            if (daemon) { if (added.length) await api.startSelected(added.map(job => job.id)); } else await api.start();
          }
          if (scanEpochRef.current === epoch) {
            setScan(undefined);
            setShowOptions(false);
          }
        } finally {
          addBusyRef.current = false;
          setAddingJobs(false);
        }
      })}
    >
      <Alert type="info" showIcon title={t("发现 {p0} 个归档任务", { p0: scan?.sources.length ?? 0 })} description={t("已按分卷自动关联。若提示缺少分卷，请补齐对应文件后再行解压；展开条目可查看具体分卷路径。")} style={{ marginBottom: 12 }} />
      {!!scan?.notices.length && <><Typography.Title level={5}>{t("扫描跳过与访问提示（{count} 项）", { count: scan.notices.length })}</Typography.Title><Table size="small" rowKey={(notice, index) => `${index}-${notice.path}`} dataSource={scan.notices} pagination={{ pageSize: 4 }} columns={[{ title: t("路径"), dataIndex: 'path', ellipsis: true }, { title: t("提示"), dataIndex: 'message' }]} /></>}
      <Table<SourceGroup> rowKey="path" size="small" columns={sourceColumns} dataSource={scan?.sources} expandable={{ expandedRowRender: source => <ul>{source.volumes.map(path => <li key={path} style={{ wordBreak: 'break-all' }}>{displayPath(path)}</li>)}</ul> }} rowSelection={{ selectedRowKeys: sources, onChange: keys => setSources(keys.map(String)), getCheckboxProps: source => ({ disabled: source.missingVolume }) }} pagination={{ pageSize: 8 }} />
      <div className="toolbar" style={{ marginTop: 12 }}>
        <Typography.Text type="secondary">{t("输出：")}{displayPath(options.outputRoot) || t("源归档所在目录")} · {options.cacheEnabled ? t("SSD 工作目录") : t("目标卷临时目录")} · {options.expandNested ? t("共 {p0} 层展开", { p0: options.maxDepth }) : t("保留内包")}</Typography.Text>
        <Button icon={<SettingOutlined aria-hidden />} onClick={() => setShowOptions(!showOptions)}>{showOptions ? t("收起批次选项") : t("本批次选项")}</Button>
      </div>
      {showOptions && (
        <div style={{ marginTop: 12, padding: 16, background: 'var(--ou-bg-card)', border: '1px solid var(--ou-border-color)', borderRadius: 8 }}>
          <Typography.Title level={5} style={{ marginTop: 0 }}>{t("本批次处理选项")}</Typography.Title>
          <Typography.Paragraph type="secondary" style={{ fontSize: 12 }}>{t("仅影响本批次；加入队列后设置会冻结。")}</Typography.Paragraph>
          <label className="setting-row"><span>{t("输出根目录")}</span><Space.Compact style={{ width: '100%' }}><Input readOnly={!daemon} onChange={event => setOptions(prev => ({ ...prev, outputRoot: event.target.value }))} aria-label={t("本批次输出目录")} value={displayPath(options.outputRoot)} placeholder={t("留空在源归档旁创建结果目录")} /><Button disabled={!nativeEngine} onClick={() => { const epoch = scanEpochRef.current; void run(async () => { const path = await api.pickDirectory(); if (path && scanEpochRef.current === epoch) setOptions(prev => ({ ...prev, outputRoot: path })); }); }}>{t("选择")}</Button><Button onClick={() => setOptions(prev => ({ ...prev, outputRoot: '' }))}>{t("清空")}</Button></Space.Compact></label>
          <div className="setting-row"><span>{t("同名目标")}</span><Select style={{ width: 220 }} value={options.conflictPolicy} onChange={conflictPolicy => setOptions(prev => ({ ...prev, conflictPolicy }))} options={[{ value: 'rename', label: t("新建名称，保留已有内容") }, { value: 'skip', label: t("跳过该任务") }]} /></div>
          <div className="setting-row"><span>{t("密码分组")}</span><Select value={options.passwordGroup} style={{ width: 220 }} options={[{ value: '', label: t("全部启用密码") }, ...groups.map(group => ({ value: group, label: group }))]} onChange={passwordGroup => setOptions(prev => ({ ...prev, passwordGroup }))} /></div>
          <div className="setting-row"><span>{t("展开内包")}</span><Space><Switch checked={options.expandNested} onChange={expandNested => setOptions(prev => ({ ...prev, expandNested }))} /><InputNumber aria-label={t("总展开层数（含外包）")} min={1} max={8} disabled={!options.expandNested} value={options.maxDepth} onChange={value => setOptions(prev => ({ ...prev, maxDepth: value ?? 2 }))} /></Space></div>
          <div className="setting-row"><span>{t("SSD 工作目录")}</span><Switch checked={options.cacheEnabled} onChange={cacheEnabled => setOptions(prev => ({ ...prev, cacheEnabled }))} /></div>
          {options.cacheEnabled && <Alert type={options.cacheRoot ? 'info' : 'warning'} title={displayPath(options.cacheRoot) || t("先在设置中选择 SSD 工作目录")} showIcon style={{ marginBottom: 8 }} />}
          <div className="setting-row">
            <span>{t("智能去套娃")}</span>
            <Space direction="vertical" align="end">
              <Switch checked={options.unwrapSingleRoot} onChange={unwrapSingleRoot => setOptions(prev => ({ ...prev, unwrapSingleRoot }))} />
            </Space>
          </div>
          <Typography.Text type="secondary" style={{ fontSize: 12, display: 'block', marginTop: -4, marginBottom: 8 }}>{t("开启后若压缩包内仅含单个顶层目录，将自动剥离顶层外壳，避免生成同名嵌套文件夹。")}</Typography.Text>
          <div className="setting-row"><span>{t("验证方式")}</span><Select value={options.verification} options={[{ value: 'standard', label: t("标准流程验证") }, { value: 'strict', label: t("完整目标回读") }]} onChange={verification => setOptions(prev => ({ ...prev, verification }))} style={{ width: 220 }} /></div>
          <div className="setting-row">
            <span>{t("完成后源包")}</span>
            <SourceCleanupSelect value={options.sourceCleanup} onChange={sourceCleanup => setOptions(prev => ({ ...prev, sourceCleanup }))} disabled={!connected} />
          </div>
          <SourceCleanupScope value={options.sourceCleanup} expandNested={options.expandNested} />
          {options.sourceCleanup !== 'keep' && (
            <Alert
              type="warning"
              showIcon
              style={{ marginTop: 8 }}
              title={t("源文件清理范围提示")}
              description={t("解压完全成功并校验通过后，将清理当前所选 {p0} 个任务的全部源文件（共 {p1} 个分卷文件）。若解压失败则绝不处理源文件。", { p0: sources.length, p1: totalSelectedVolumes })}
            />
          )}
          <div className="setting-row" style={{ marginTop: 8 }}>
            <span>{t("添加后自动开始")}</span>
            <Switch checked={options.autoStartOnAdd} onChange={autoStartOnAdd => setOptions(prev => ({ ...prev, autoStartOnAdd }))} />
          </div>
        </div>
      )}
    </Modal>
    <Modal
      title={t("输入解压密码 — {p0}", { p0: passwordJob?.source.displayName ?? '' })}
      open={!!passwordJob && passwordOwnerPresent}
      onCancel={closePassword}
      destroyOnHidden
      
      okText={daemon ? t("提交密码") : t("解密并重试")}
      cancelText={t("取消")}
      okButtonProps={{ loading: submittingPassword, disabled: !inputPassword }}
      onOk={() => void run(async () => {
        if (!passwordJob || !inputPassword) return;
        const epoch = passwordDraftEpoch.current;
        const connectionVersion = api.getConnectionVersion?.() ?? 0;
        if (passwordRequestEpoch.current === epoch) return;
        passwordRequestEpoch.current = epoch;
        setSubmittingPassword(true);
        try {
          const secret = inputPassword;
          if (saveToVault) {
            await api.savePassword({
              name: t("{p0} 密码", { p0: passwordJob.source.displayName }),
              secret,
              groups: passwordJob.options.passwordGroup ? [passwordJob.options.passwordGroup] : [],
              notes: t("解压即时录入"),
              enabled: true,
            });
            if (passwordDraftEpoch.current !== epoch || connectionVersion !== (api.getConnectionVersion?.() ?? 0)) return;
          }
          if (daemon) await api.submitInput(passwordJob.id, 'password', secret);
          else if (saveToVault) await api.retry(passwordJob.id);
          else await api.retry(passwordJob.id, false, secret);
          if (passwordDraftEpoch.current === epoch) closePassword();
        } finally {
          if (passwordDraftEpoch.current === epoch) {
            passwordRequestEpoch.current = null;
            setSubmittingPassword(false);
          }
        }
      })}
    >
      <Typography.Paragraph>{daemon ? t("密码将仅提交给当前服务任务；勾选保存后才会写入服务端密码库。") : t("该压缩文件受密码保护。请输入解压密码，输入后将立即重新调度此任务。")}</Typography.Paragraph>
      <Space orientation="vertical" style={{ width: '100%' }}>
        <Input.Password
          autoFocus
          aria-label={t("即时解压密码")}
          placeholder={t("输入解压密码")}
          value={inputPassword}
          onChange={e => setInputPassword(e.target.value)}
        />
        <Checkbox checked={saveToVault} onChange={e => setSaveToVault(e.target.checked)}>{daemon ? t("同时保存到服务端加密密码库（需要密码库写入权限）") : t("同时保存到本地加密密码库（推荐，后续同批次或分卷可自动匹配）")}</Checkbox>
      </Space>
    </Modal>
    <JobDetails job={snapshot.jobs.find(job => job.id === detail)} close={() => setDetail(undefined)} run={run} operationsDisabled={snapshot.running || deletion.busy} inputDisabled={deletion.busy || (snapshot.running && !daemon)} />
    {deletion.dialog}
  </>;
}
