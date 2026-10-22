import { t } from '../i18n';
import { useState, useEffect, useMemo, useRef, useDeferredValue, useCallback } from 'react';
import { App as AntApp, Table, Button, Space, Tag, Input, Select, Popconfirm, Card, Alert } from 'antd';
import { ReloadOutlined, DeleteOutlined, FolderOpenOutlined, EyeOutlined } from '@ant-design/icons';
import { api, isDesktop } from '../api';
import type { AppSnapshot, HistoryPageQuery, JobHistoryPage, JobSummary, JobView } from '../types';
import { statusLabels } from '../types';
import { JobDetails, displayPath, verificationLabel, terminal, StatusTag, ArchiveFormatBadge } from './shared';
import { taskDeleteBlockedReason, useTaskDeletion } from './TaskDeleteDialog';

function previewPage(snapshot: AppSnapshot, query: HistoryPageQuery): JobHistoryPage {
  const history = snapshot.jobs.filter(terminal).sort((a, b) => b.createdAt.localeCompare(a.createdAt) || b.id.localeCompare(a.id)).slice(0, snapshot.settings.historyLimit);
  const search = query.search.trim().toLowerCase();
  const filtered = history.filter(job => (query.status === 'all' || job.status === query.status) && (!search || job.source.displayName.toLowerCase().includes(search) || job.source.path.toLowerCase().includes(search)));
  const offset = Math.min(query.offset, Math.floor(Math.max(0, filtered.length - 1) / query.limit) * query.limit);
  return { items: filtered.slice(offset, offset + query.limit), offset, limit: query.limit, filteredTotal: filtered.length, total: history.length,
    completed: history.filter(job => job.status === 'completed').length, failed: history.filter(job => job.status === 'failed').length,
    recoverable: history.filter(job => ['needsPassword', 'failed', 'cancelled', 'interrupted', 'publishPending'].includes(job.status)).length,
    clearAllowed: false, sessionId: snapshot.sessionId, revision: snapshot.revision };
}

export default function HistoryPage({ snapshot, refresh, run }: { snapshot: AppSnapshot; refresh: () => Promise<void>; run: (work: () => Promise<unknown>) => Promise<void> }) {
  const { message } = AntApp.useApp();
  const daemon = api.isDaemonMode?.() === true;
  const connected = isDesktop || daemon;
  const nativeEngine = isDesktop && !daemon;
  const [page, setPage] = useState<JobHistoryPage | null>(null);
  const [offset, setOffset] = useState(0);
  const [pageSize, setPageSize] = useState(10);
  const [search, setSearch] = useState('');
  const deferredSearch = useDeferredValue(search);
  const [statusFilter, setStatusFilter] = useState('all');
  const [selectedJob, setSelectedJob] = useState<JobView | null>(null);
  const [detailError, setDetailError] = useState<string | null>(null);
  const [selected, setSelected] = useState<string[]>([]);
  const selectedIds = useMemo(() => new Set(selected), [selected]);
  const [loadError, setLoadError] = useState<string | null>(null);
  const snapshotRef = useRef(snapshot);
  const queryRef = useRef<HistoryPageQuery>({ offset: 0, limit: 10, status: 'all', search: '' });
  const requestSeq = useRef(0);
  const loadPromise = useRef<Promise<void> | null>(null);
  const mounted = useRef(false);
  const selectedJobId = useRef<string | null>(null);
  const detailSeq = useRef(0);
  const detailsPending = useRef(0);

  useEffect(() => { snapshotRef.current = snapshot; }, [snapshot]);
  const closeDetails = useCallback(() => {
    selectedJobId.current = null;
    detailSeq.current += 1;
    setSelectedJob(null);
    setDetailError(null);
  }, []);
  const loadDetails = useCallback(async (id: string, background = false) => {
    if (background && detailsPending.current > 0) return;
    selectedJobId.current = id;
    const seq = ++detailSeq.current;
    detailsPending.current += 1;
    try {
      const detail = connected ? await api.jobDetails(id) : snapshotRef.current.jobs.find(job => job.id === id) ?? null;
      if (!mounted.current || seq !== detailSeq.current || selectedJobId.current !== id) return;
      setSelectedJob(detail);
      setDetailError(null);
      if (!detail) selectedJobId.current = null;
    } catch (error) {
      if (mounted.current && seq === detailSeq.current) setDetailError(error instanceof Error ? error.message : t("读取任务详情失败，请重试。"));
    } finally {
      detailsPending.current -= 1;
    }
  }, [connected]);

  // Serialize queries; timer refreshes skip any read already in flight.
  const loadHistory = useCallback((): Promise<void> => {
    requestSeq.current += 1;
    if (loadPromise.current) return loadPromise.current;
    const operation = async () => {
      let readAgain = true;
      while (mounted.current && readAgain) {
        const seq = requestSeq.current;
        const query = queryRef.current;
        try {
          const next = connected ? await api.historyPage(query) : previewPage(snapshotRef.current, query);
          if (mounted.current && seq === requestSeq.current) {
            setPage(next);
            setOffset(next.offset);
            setLoadError(null);
            const visible = new Set(next.items.map(job => job.id));
            setSelected(previous => previous.filter(id => visible.has(id)));
            if (selectedJobId.current) void loadDetails(selectedJobId.current, true);
          }
        } catch (error) {
          if (mounted.current && seq === requestSeq.current) setLoadError(error instanceof Error ? error.message : t("读取历史记录失败"));
        }
        readAgain = seq !== requestSeq.current;
      }
    };
    const promise = operation().finally(() => { if (loadPromise.current === promise) loadPromise.current = null; });
    loadPromise.current = promise;
    return promise;
  }, [loadDetails, connected]);

  useEffect(() => {
    mounted.current = true;
    return () => { mounted.current = false; requestSeq.current += 1; detailSeq.current += 1; };
  }, []);
  useEffect(() => {
    queryRef.current = { offset, limit: pageSize, status: statusFilter, search: deferredSearch };
    queueMicrotask(() => { if (mounted.current) void loadHistory(); });
  }, [offset, pageSize, statusFilter, deferredSearch, snapshot.sessionId, snapshot.settings.historyLimit, loadHistory]);
  useEffect(() => {
    const poll = () => { if (!document.hidden && !loadPromise.current) void loadHistory(); };
    const timer = setInterval(poll, 2000);
    document.addEventListener('visibilitychange', poll);
    return () => { clearInterval(timer); document.removeEventListener('visibilitychange', poll); };
  }, [loadHistory]);

  const historyList = page?.items ?? [];
  const deletion = useTaskDeletion(run, async ids => {
    const removedIds = new Set(ids);
    setPage(previous => previous ? { ...previous, items: previous.items.filter(job => !removedIds.has(job.id)) } : previous);
    setSelected(previous => previous.filter(id => !removedIds.has(id)));
    if (selectedJobId.current && removedIds.has(selectedJobId.current)) closeDetails();
    await loadHistory();
    await refresh();
  });
  const liveDetail = selectedJob ? snapshot.jobs.find(job => job.id === selectedJob.id) ?? selectedJob : undefined;
  const disableClear = !connected || deletion.busy || snapshot.running || !page?.clearAllowed;
  const columns = [
    {
      title: t("来源名称与路径"),
      key: 'source',
      render: (_: unknown, job: JobSummary) => (
        <div style={{ display: 'flex', alignItems: 'flex-start', gap: 8 }}>
          <ArchiveFormatBadge name={job.source.displayName} />
          <div style={{ minWidth: 0, flex: 1 }}>
            <div style={{ fontWeight: 600, fontSize: 13, wordBreak: 'break-all', color: 'var(--ou-text-title)' }}>
              {job.source.displayName}
            </div>
            <div className="muted mono-text" style={{ fontSize: 11, wordBreak: 'break-all', marginTop: 2 }}>
              {displayPath(job.source.path)}
            </div>
          </div>
        </div>
      ),
    },
    {
      title: t("状态"),
      key: 'status',
      width: 130,
      render: (_: unknown, job: JobSummary) => <StatusTag status={job.status} />,
    },
    {
      title: t("校验方式"),
      key: 'verification',
      width: 170,
      render: (_: unknown, job: JobSummary) => (
        <Tag style={{ borderRadius: 4, fontSize: 12 }}>{verificationLabel(job.verificationScope)}</Tag>
      ),
    },
    {
      title: t("完成时间"),
      key: 'finishedAt',
      width: 180,
      render: (_: unknown, job: JobSummary) => (
        <span className="tabular-nums mono-text" style={{ fontSize: 12, color: 'var(--ou-text-body)' }}>
          {job.finishedAt || job.createdAt || '-'}
        </span>
      ),
    },
    {
      title: t("操作"),
      key: 'action',
      width: 300,
      render: (_: unknown, job: JobSummary) => <Space size="small" wrap>
        <Button size="small" icon={<EyeOutlined aria-hidden />} onClick={() => void loadDetails(job.id)}>{t("详情")}</Button>
        {job.outputPath && <Button size="small" icon={<FolderOpenOutlined aria-hidden />} disabled={!nativeEngine} onClick={() => void run(() => api.openPath(job.outputPath!))}>{t("打开结果")}</Button>}
        {job.status === 'publishPending' && <Space wrap>
          <Button size="small" type="primary" ghost disabled={!connected || snapshot.running || deletion.busy} onClick={() => void run(async () => { await api.recover(job.id, 'verify'); await loadHistory(); })}>{t("回读并核对")}</Button>
          <Button size="small" disabled={!connected || snapshot.running || deletion.busy} onClick={() => void run(async () => { await api.recover(job.id, 'retry'); await loadHistory(); })}>{t("重试写回")}</Button>
        </Space>}
        {['failed', 'cancelled', 'needsPassword', 'interrupted'].includes(job.status) && <Button size="small" disabled={!connected || snapshot.running || deletion.busy} onClick={() => void run(async () => { await api.retry(job.id); await loadHistory(); })}>{t("重试")}</Button>}
        {job.workspace && ['failed', 'needsPassword', 'interrupted', 'cancelled', 'cleanupPending'].includes(job.status) && <Popconfirm title={t("清理保留的临时结果？")} description={t("仅清理本应用标记的工作目录与临时目录，已发布结果与原始压缩包不会因此删除。")} onConfirm={() => void run(async () => { await api.recover(job.id, 'cleanup'); await loadHistory(); })}><Button disabled={!connected || snapshot.running || deletion.busy} size="small">{t("清理临时目录")}</Button></Popconfirm>}
        <Button size="small" danger aria-label={t("删除任务 ") + job.source.displayName} title={taskDeleteBlockedReason(job)} disabled={!connected || deletion.busy || !!taskDeleteBlockedReason(job)} loading={deletion.deletingIds.includes(job.id)} onClick={() => deletion.request([job])}>{deletion.deletingIds.includes(job.id) ? t("正在删除") : t("删除任务")}</Button>
      </Space>
    },
  ];

  return (
    <div style={{ padding: 16, display: 'flex', flexDirection: 'column', gap: 14 }}>
      <div className="page-header">
        <div>
          <h2 className="page-title">{t("处理记录")}</h2>
          <p className="page-description">
            {t("按页查看最近 {count} 条处理记录，显示上限可在设置中调整。详情在打开时读取。", { count: snapshot.settings.historyLimit })}
          </p>
        </div>
      </div>

      <div className="telemetry-bar">
        <div className="telemetry-card">
          <span className="telemetry-label">{t("最近记录总数")}</span>
          <span className="telemetry-value tabular-nums">{page?.total ?? 0}</span>
        </div>
        <div className="telemetry-card">
          <span className="telemetry-label">{t("已完成")}</span>
          <span className="telemetry-value tabular-nums" style={{ color: 'var(--ou-success)' }}>{page?.completed ?? 0}</span>
        </div>
        <div className="telemetry-card">
          <span className="telemetry-label">{t("处理失败")}</span>
          <span className="telemetry-value tabular-nums" style={{ color: (page?.failed ?? 0) > 0 ? 'var(--ou-danger)' : undefined }}>{page?.failed ?? 0}</span>
        </div>
        <div className="telemetry-card">
          <span className="telemetry-label">{t("可恢复/待核对")}</span>
          <span className="telemetry-value tabular-nums" style={{ color: (page?.recoverable ?? 0) > 0 ? 'var(--ou-warning)' : undefined }}>{page?.recoverable ?? 0}</span>
        </div>
      </div>

      {loadError && <Alert type="warning" showIcon title={t("读取历史记录失败")} description={<Space wrap><span>{loadError}</span>{page && <span>{t("（已保留显示此前读取的记录）")}</span>}<Button size="small" type="primary" ghost onClick={() => void loadHistory()}>{t("重试读取")}</Button></Space>} />}
      {detailError && <Alert type="warning" showIcon title={t("读取任务详情失败")} description={detailError} action={<Button onClick={() => { if (selectedJobId.current) void loadDetails(selectedJobId.current); }}>{t("重试详情")}</Button>} />}

      <Card size="small" style={{ borderRadius: 'var(--ou-radius)', border: '1px solid var(--ou-border-color)' }}>
        <div className="toolbar" style={{ marginBottom: 12 }}>
          <Space wrap>
            <Input.Search
              placeholder={t("搜索文件路径或名称")}
              allowClear
              style={{ width: 260 }}
              value={search}
              onChange={event => { setSearch(event.target.value); setOffset(0); setSelected([]); }}
            />
            <Select
              style={{ width: 150 }}
              value={statusFilter}
              onChange={value => { setStatusFilter(value); setOffset(0); setSelected([]); }}
              options={[{ label: t("全部状态"), value: 'all' }, ...Object.entries(statusLabels).map(([value, label]) => ({ label: t(label), value }))]}
            />
          </Space>
          <Space wrap>
            <Button
              danger
              icon={<DeleteOutlined aria-hidden />}
              aria-label={t("删除所选任务")}
              loading={deletion.busy}
              disabled={!connected || deletion.busy || !selected.length}
              onClick={() => deletion.request(historyList.filter(job => selectedIds.has(job.id)))}
            >
              {t("删除所选任务")}
            </Button>
            <Button icon={<ReloadOutlined aria-hidden />} onClick={() => void loadHistory()}>{t("刷新记录")}</Button>
            <Popconfirm
              title={t("清理历史记录")}
              description={t("仅清除历史任务记录，已提取/生成的文件保留，确定清空吗？")}
              disabled={disableClear}
              onConfirm={() => void run(async () => { await api.clearHistory(); await loadHistory(); await refresh(); message.success(t("历史记录已清空")); })}
            >
              <Button danger icon={<DeleteOutlined aria-hidden />} disabled={disableClear}>{t("清空历史记录")}</Button>
            </Popconfirm>
          </Space>
        </div>

        <Table<JobSummary>
          rowKey="id"
          columns={columns}
          dataSource={historyList}
          size="small"
          scroll={{ x: 1100 }}
          rowSelection={{
            selectedRowKeys: selected,
            onChange: keys => setSelected(keys.map(String)),
            getCheckboxProps: job => ({ disabled: deletion.busy, 'aria-label': t("选择任务 ") + job.source.displayName }),
          }}
          pagination={{
            current: Math.floor(offset / pageSize) + 1,
            pageSize,
            total: page?.filteredTotal ?? 0,
            showSizeChanger: true,
            pageSizeOptions: [10, 20, 50, 100],
            onChange: (number, size) => { setPageSize(size); setOffset(size === pageSize ? (number - 1) * size : 0); setSelected([]); },
          }}
          locale={{ emptyText: t("暂无历史记录") }}
        />
      </Card>

      <JobDetails job={liveDetail} close={closeDetails} run={run} operationsDisabled={snapshot.running || deletion.busy} />
      {deletion.dialog}
    </div>
  );
}
