import { t } from '../i18n';
import { useCallback, useDeferredValue, useEffect, useMemo, useRef, useState } from 'react';
import {
  App as AntApp,
  Alert,
  Button,
  Input,
  Modal,
  Select,
  Space,
  Table,
  Tag,
  Typography,
  Segmented,
} from 'antd';
import {
  FolderAddOutlined,
  FileAddOutlined,
  PlayCircleOutlined,
  PauseCircleOutlined,
  StopOutlined,
  DeleteOutlined,
  AppstoreOutlined,
  BarsOutlined,
} from '@ant-design/icons';
import type { SharedProps } from '../App';
import { api, isDesktop } from '../api';
import type { JobView, ScanResult, Settings } from '../types';
import { JobDetails, retryable } from './shared';
import { useTaskDeletion } from './TaskDeleteDialog';
import { BatchProgressCard } from './QueueProgressCards';
import QueuePasswordModal from './QueuePasswordModal';

import { useQueueJobColumns, createSourceReviewColumns } from './QueueJobColumns';
import QueueCardGrid from './QueueCardGrid';
import QueueSourceReviewModal from './QueueSourceReviewModal';
export default function QueuePage({
  snapshot,
  run,
  openHistory,
  externalOpenRequest,
  onConsumeExternalOpen,
}: SharedProps & {
  openHistory?: () => void;
  externalOpenRequest?: { seq: number; paths: string[] } | null;
  onConsumeExternalOpen?: () => void;
}) {
  const { modal, notification, message } = AntApp.useApp();
  const [hideScopeAlert, setHideScopeAlert] = useState(
    () => localStorage.getItem('openunpack_dismiss_scope_alert') === 'true',
  );
  const daemon = api.isDaemonMode?.() === true;
  const connected = isDesktop || daemon;
  const nativeEngine = isDesktop && !daemon;
  const [serverPath, setServerPath] = useState('');
  const [query, setQuery] = useState('');
  const deferredQuery = useDeferredValue(query);
  const [filter, setFilter] = useState('all');
  const [tableHeight, setTableHeight] = useState(() =>
    typeof window !== 'undefined' ? Math.max(380, window.innerHeight - 340) : 480,
  );
  useEffect(() => {
    const onResize = () => setTableHeight(Math.max(380, window.innerHeight - 340));
    window.addEventListener('resize', onResize);
    return () => window.removeEventListener('resize', onResize);
  }, []);
  const [rawSelected, setSelected] = useState<string[]>([]);
  const [viewMode, setViewMode] = useState<'table' | 'cards'>('table');
  const existingJobIds = useMemo(
    () => new Set(snapshot.jobs.map((job) => job.id)),
    [snapshot.jobs],
  );
  const selected = useMemo(
    () => rawSelected.filter((id) => existingJobIds.has(id)),
    [rawSelected, existingJobIds],
  );
  const selectedIds = useMemo(() => new Set(selected), [selected]);
  const [detail, setDetail] = useState<string>();
  const deletion = useTaskDeletion(run, (ids) => {
    const removedIds = new Set(ids);
    setSelected((previous) => previous.filter((id) => !removedIds.has(id)));
    setDetail((previous) => (previous && removedIds.has(previous) ? undefined : previous));
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
  const jobsRef = useRef(snapshot.jobs);
  const passwordJobRef = useRef(passwordJob);
  useEffect(() => {
    jobsRef.current = snapshot.jobs;
  }, [snapshot.jobs]);
  useEffect(() => {
    passwordJobRef.current = passwordJob;
  }, [passwordJob]);
  const closePassword = useCallback(() => {
    passwordDraftEpoch.current += 1;
    passwordRequestEpoch.current = null;
    setPasswordJob(null);
    setInputPassword('');
    setSubmittingPassword(false);
  }, []);
  useEffect(() => {
    const unlisten = api.onNeedPassword?.((event) => {
      if (!event?.jobId || passwordJobRef.current) return;
      const target = jobsRef.current.find((job) => job.id === event.jobId);
      if (!target) return;
      passwordDraftEpoch.current += 1;
      passwordRequestEpoch.current = null;
      setInputPassword('');
      setSubmittingPassword(false);
      setPasswordJob(target);
    });
    return () => unlisten?.();
  }, []);
  useEffect(
    () => () => {
      passwordDraftEpoch.current += 1;
    },
    [],
  );
  const passwordOwnerPresent =
    !passwordJob || snapshot.jobs.some((job) => job.id === passwordJob.id);
  useEffect(() => {
    if (passwordOwnerPresent) return;
    const epoch = ++passwordDraftEpoch.current;
    queueMicrotask(() => {
      if (passwordDraftEpoch.current === epoch) closePassword();
    });
  }, [passwordOwnerPresent, closePassword]);
  const [addingJobs, setAddingJobs] = useState(false);
  const addBusyRef = useRef(false);
  const scanEpochRef = useRef(0);
  const activeScan = useRef<{ id: string; cancelled: boolean } | null>(null);
  const settingsRef = useRef(snapshot.settings);
  useEffect(() => {
    settingsRef.current = snapshot.settings;
  }, [snapshot.settings]);
  const addPaths = useCallback(
    (paths: string[], isDrop = false) => {
      if (!paths.length || activeScan.current || addBusyRef.current) return;
      const current = { id: crypto.randomUUID(), cancelled: false };
      activeScan.current = current;
      scanEpochRef.current += 1;
      setScanning(true);
      setScanStopping(false);
      setScan(undefined);
      void run(async () => {
        try {
          const result = await api.scan(paths, current.id);
          if (activeScan.current === current && !current.cancelled) {
            scanEpochRef.current += 1;
            const validSources = result.sources.filter((source) => !source.missingVolume);
            const hasNotices = Boolean(result.notices && result.notices.length > 0);
            const hasMissing = result.sources.some((source) => source.missingVolume);

            // 极速黄金路径：仅当拖拽（onDrop）且无分卷缺失、无警告提示时直接入队，免弹窗阻断；手动点击按钮添加仍保留确认面板供调参
            if (isDrop && validSources.length > 0 && !hasNotices && !hasMissing) {
              const currentSettings = { ...settingsRef.current };
              const added = await api.addJobs(validSources, currentSettings);
              const autoStart =
                currentSettings.autoStartOnAdd ?? snapshot.settings.autoStartOnAdd ?? true;
              if (autoStart && !snapshot.paused && snapshot.engineAvailable) {
                if (daemon) {
                  if (added.length) await api.startSelected(added.map((j) => j.id));
                } else {
                  await api.start();
                }
              }
              notification.success({
                title: t('已添加并开始解压'),
                message: t('已添加并开始解压'),
                description: t('成功添加 {count} 项归档文件。', { count: validSources.length }),
                placement: 'bottomRight',
                duration: 3.5,
              });
              return;
            }

            if (result.sources.length === 0 && !hasNotices) {
              message.warning(t('未在所选路径中检测到可解压的归档文件。'));
              return;
            }

            setScan(result);
            setSources(validSources.map((source) => source.path));
            setOptions({ ...settingsRef.current });
          }
        } catch (error) {
          if (!current.cancelled) throw error;
        } finally {
          if (activeScan.current === current) {
            activeScan.current = null;
            setScanning(false);
            setScanStopping(false);
          }
        }
      });
    },
    [
      run,
      daemon,
      message,
      notification,
      snapshot.engineAvailable,
      snapshot.paused,
      snapshot.settings.autoStartOnAdd,
    ],
  );
  const submitInstantPassword = useCallback(() => {
    if (!passwordJob || !inputPassword || submittingPassword) return;
    const epoch = passwordDraftEpoch.current;
    const connectionVersion = api.getConnectionVersion?.() ?? 0;
    if (passwordRequestEpoch.current === epoch) return;
    passwordRequestEpoch.current = epoch;
    setSubmittingPassword(true);
    void run(async () => {
      try {
        const secret = inputPassword;
        if (saveToVault) {
          await api.savePassword({
            name: t('{p0} 密码', { p0: passwordJob.source.displayName }),
            secret,
            groups: passwordJob.options.passwordGroup ? [passwordJob.options.passwordGroup] : [],
            notes: t('解压即时录入'),
            enabled: true,
          });
          if (
            passwordDraftEpoch.current !== epoch ||
            connectionVersion !== (api.getConnectionVersion?.() ?? 0)
          )
            return;
        }
        if (daemon) {
          await api.submitInput(passwordJob.id, 'password', secret);
        } else if (passwordJob.status === 'running' && api.submitNativeInput) {
          const resumed = await api.submitNativeInput(passwordJob.id, secret);
          if (!resumed) {
            if (saveToVault) await api.retry(passwordJob.id);
            else await api.retry(passwordJob.id, false, secret);
          }
        } else if (saveToVault) {
          await api.retry(passwordJob.id);
        } else {
          await api.retry(passwordJob.id, false, secret);
        }
        if (passwordDraftEpoch.current === epoch) closePassword();
      } finally {
        if (passwordDraftEpoch.current === epoch) {
          passwordRequestEpoch.current = null;
          setSubmittingPassword(false);
        }
      }
    });
  }, [passwordJob, inputPassword, submittingPassword, saveToVault, daemon, run, closePassword]);

  const cancelScan = () => {
    const current = activeScan.current;
    if (!current || current.cancelled) return;
    current.cancelled = true;
    setScanStopping(true);
    void run(() => api.cancelScan(current.id));
  };
  useEffect(() => {
    let disposed = false;
    let stop: (() => void) | undefined;
    void api
      .onDrop((paths) => addPaths(paths, true))
      .then((cleanup) => (disposed ? cleanup() : (stop = cleanup)))
      .catch(() => {});
    return () => {
      disposed = true;
      stop?.();
      const current = activeScan.current;
      if (current) {
        current.cancelled = true;
        void api.cancelScan(current.id).catch(() => {});
        activeScan.current = null;
      }
    };
  }, [addPaths]);
  const consumedExternalSeq = useRef<number>(0);
  useEffect(() => {
    if (
      externalOpenRequest &&
      externalOpenRequest.paths.length > 0 &&
      externalOpenRequest.seq !== consumedExternalSeq.current
    ) {
      consumedExternalSeq.current = externalOpenRequest.seq;
      addPaths(externalOpenRequest.paths, true);
      onConsumeExternalOpen?.();
    }
  }, [externalOpenRequest, addPaths, onConsumeExternalOpen]);
  const pick = (folder: boolean) =>
    void run(async () => {
      const paths = await api.pickFiles(folder);
      if (!paths.length) return;
      addPaths(paths);
    });
  const counts = useMemo(
    () => ({
      total: snapshot.jobs.length,
      running: snapshot.jobs.filter((job) => job.status === 'running').length,
      waiting: snapshot.jobs.filter((job) =>
        ['queued', 'paused', 'waitingResource'].includes(job.status),
      ).length,
      completed: snapshot.jobs.filter((job) => ['completed', 'cleanupPending'].includes(job.status))
        .length,
      errors: snapshot.jobs.filter(retryable).length,
    }),
    [snapshot.jobs],
  );

  const jobs = useMemo(() => {
    const normalizedQuery = deferredQuery.trim().toLowerCase();
    return snapshot.jobs.filter(
      (job) =>
        (!normalizedQuery ||
          `${job.source.displayName} ${job.source.path}`.toLowerCase().includes(normalizedQuery)) &&
        (filter === 'all' ||
          (filter === 'waiting'
            ? ['queued', 'paused', 'waitingResource'].includes(job.status)
            : filter === 'failed'
              ? retryable(job)
              : job.status === filter)),
    );
  }, [snapshot.jobs, deferredQuery, filter]);
  const selectJobs = useMemo(
    () => snapshot.jobs.filter((job) => selectedIds.has(job.id)),
    [snapshot.jobs, selectedIds],
  );
  const selectedStartIds = selectJobs
    .filter(
      (job) =>
        ['queued', 'waitingResource'].includes(job.status) ||
        (daemon && snapshot.paused && job.status === 'running'),
    )
    .map((job) => job.id);
  const startDisabled =
    !connected ||
    deletion.busy ||
    !snapshot.engineAvailable ||
    (snapshot.running && !snapshot.paused) ||
    (daemon
      ? !selectedStartIds.length
      : !snapshot.jobs.some((job) => ['queued', 'paused'].includes(job.status)));
  const startQueue = () =>
    void run(() => (daemon ? api.startSelected(selectedStartIds) : api.start()));
  const completedJobs = snapshot.jobs.filter((job) =>
    ['completed', 'partial', 'salvaged', 'skipped', 'cleanupPending'].includes(job.status),
  );
  const operateSelected = (operation: (job: JobView) => Promise<unknown>) =>
    void run(async () => {
      await Promise.all(selectJobs.map((job) => operation(job)));
      setSelected([]);
    });
  const cancel = (ids: string[]): void => {
    void modal.confirm({
      title: t('停止所选任务？'),
      content: t('将请求停止当前作业。原始压缩包始终保留，现有输出会按恢复规则处理。'),
      okText: t('停止任务'),
      cancelText: t('返回'),
      onOk: () =>
        run(async () => {
          await Promise.all(ids.map((id) => api.cancel(id)));
          setSelected([]);
        }),
    });
  };

  const openPasswordPrompt = useCallback((job: JobView, saveDefault: boolean) => {
    passwordDraftEpoch.current += 1;
    passwordRequestEpoch.current = null;
    setPasswordJob(job);
    setInputPassword('');
    setSaveToVault(saveDefault);
    setSubmittingPassword(false);
  }, []);
  const columns = useQueueJobColumns({
    connected,
    nativeEngine,
    daemon,
    snapshotRunning: snapshot.running,
    deletionBusy: deletion.busy,
    deletingIds: deletion.deletingIds,
    onOpenDetail: setDetail,
    onPromptPassword: openPasswordPrompt,
    onCancel: cancel,
    onRequestDelete: (targets) => deletion.request(targets),
    run,
  });
  const sourceColumns = createSourceReviewColumns();
  const groups = [...new Set(snapshot.passwords.flatMap((password) => password.groups))];
  const totalSelectedVolumes = useMemo(() => {
    if (!scan) return 0;
    return scan.sources
      .filter((source) => selectedSourcePaths.has(source.path))
      .reduce((sum, s) => sum + (s.volumes.length || 1), 0);
  }, [scan, selectedSourcePaths]);

  return (
    <>
      <div className="page-header">
        <div>
          <h2 className="page-title">{t('批量解压')}</h2>
          <p className="page-description">
            {t('添加压缩文件或文件夹进行批量解压，支持分卷识别与密码匹配。')}
          </p>
        </div>
        <Space>
          <Button
            aria-label={t('添加文件')}
            icon={<FileAddOutlined aria-hidden />}
            disabled={!nativeEngine || scanning}
            onClick={() => pick(false)}
          >
            {t('添加文件')}
          </Button>
          <Button
            aria-label={t('添加文件夹')}
            type="primary"
            icon={<FolderAddOutlined aria-hidden />}
            disabled={!nativeEngine || scanning}
            onClick={() => pick(true)}
          >
            {t('添加文件夹')}
          </Button>
        </Space>
      </div>
      {counts.total > 0 && <BatchProgressCard snapshot={snapshot} counts={counts} />}
      {counts.total > 0 && (
        <div className="telemetry-bar-compact">
          <span className="telemetry-chip">
            {t('总任务')} <strong>{counts.total}</strong>
          </span>
          {counts.running > 0 && (
            <span className="telemetry-chip chip-running">
              <span className="ou-pulse-dot" style={{ width: 6, height: 6 }} />
              {t('处理中')} <strong>{counts.running}</strong>
            </span>
          )}
          {counts.waiting > 0 && (
            <span className="telemetry-chip">
              {t('等待中')} <strong>{counts.waiting}</strong>
            </span>
          )}
          {counts.completed > 0 && (
            <span className="telemetry-chip chip-completed">
              {t('已完成')} <strong>{counts.completed}</strong>
            </span>
          )}
          {counts.errors > 0 && (
            <span className="telemetry-chip chip-errors">
              {t('需处理')} <strong>{counts.errors}</strong>
            </span>
          )}
        </div>
      )}
      {!hideScopeAlert && (
        <Alert
          type="info"
          showIcon
          closable
          onClose={() => {
            setHideScopeAlert(true);
            localStorage.setItem('openunpack_dismiss_scope_alert', 'true');
          }}
          title={t('显示全部待处理、待核对任务和最近 20 条其他已结束任务')}
          description={t(
            '保留临时目录的任务始终显示。完整处理记录可在“处理记录”中按页查看；原始任务记录不会因队列显示范围而删除。',
          )}
          action={
            openHistory ? (
              <Button size="small" onClick={openHistory}>
                {t('查看处理记录')}
              </Button>
            ) : undefined
          }
          style={{ marginBottom: 14 }}
        />
      )}
      {daemon && (
        <Space.Compact style={{ width: '100%', marginBottom: 16 }}>
          <Input.Search
            aria-label={t('服务端归档路径')}
            value={serverPath}
            onChange={(event) => setServerPath(event.target.value)}
            placeholder={t('填写已批准目录内的服务端文件或文件夹路径')}
            enterButton={t('扫描路径')}
            disabled={scanning || deletion.busy}
            loading={scanning}
            onSearch={(value) => {
              if (value.trim()) addPaths([value.trim()]);
            }}
          />
        </Space.Compact>
      )}
      {!snapshot.engineAvailable && connected && (
        <Alert
          showIcon
          type="warning"
          title={t('解压引擎尚未就绪')}
          description={t(
            '请确认官方 7-Zip 引擎与作业进程已安装。可管理队列与密码库，开始前需恢复引擎。',
          )}
          style={{ marginBottom: 16 }}
        />
      )}
      {snapshot.paused &&
        snapshot.jobs.some((job) =>
          (daemon
            ? ['queued', 'waitingResource', 'running']
            : ['queued', 'paused', 'waitingResource']
          ).includes(job.status),
        ) && (
          <Alert
            showIcon
            type="warning"
            title={t('队列当前处于暂停状态')}
            description={
              daemon
                ? t('请选择要继续的任务，再点击“继续所选”。')
                : t('新添加的任务已进入等待队列，请点击“继续队列”开始解压。')
            }
            action={
              <Button
                size="small"
                type="primary"
                aria-label={daemon ? t('横幅继续所选') : t('横幅继续队列')}
                disabled={startDisabled}
                onClick={startQueue}
              >
                {daemon ? t('继续所选') : t('继续队列')}
              </Button>
            }
            style={{ marginBottom: 16 }}
          />
        )}
      <div className="section-card">
        <div className="toolbar">
          <Space wrap align="center">
            <Button
              type="primary"
              icon={<PlayCircleOutlined aria-hidden />}
              disabled={startDisabled}
              onClick={startQueue}
            >
              {daemon
                ? snapshot.paused
                  ? t('继续所选')
                  : t('开始所选')
                : snapshot.paused
                  ? t('继续队列')
                  : t('开始队列')}
            </Button>
            <Button
              icon={<PauseCircleOutlined aria-hidden />}
              disabled={!connected || !snapshot.running || snapshot.paused}
              onClick={() => void run(() => api.pause())}
            >
              {t('暂停新任务')}
            </Button>
            <Button
              icon={<StopOutlined aria-hidden />}
              disabled={!connected || deletion.busy || !selected.length}
              onClick={() => void cancel(selected)}
            >
              {t('停止所选')}
            </Button>
            <Button
              disabled={
                !connected || deletion.busy || snapshot.running || !selectJobs.some(retryable)
              }
              onClick={() =>
                operateSelected((job) => (retryable(job) ? api.retry(job.id) : Promise.resolve()))
              }
            >
              {t('重试所选')}
            </Button>
            <Button
              icon={<DeleteOutlined aria-hidden />}
              aria-label={t('删除所选任务')}
              loading={deletion.busy}
              disabled={!connected || deletion.busy || !selectJobs.length}
              onClick={() => deletion.request(selectJobs)}
            >
              {t('删除所选任务')}
            </Button>
            {selected.length > 0 && (
              <Tag color="processing" style={{ marginLeft: 4 }}>
                {t('已选 {count} 项', { count: selected.length })}
              </Tag>
            )}
          </Space>
          <Button
            danger
            disabled={!connected || deletion.busy || !completedJobs.length}
            title={t('删除已完成任务记录并清理临时缓存，原包与已发布结果保留')}
            onClick={() => deletion.request(completedJobs)}
          >
            {t('删除当前显示的已结束任务')}
          </Button>
        </div>
        <div className="toolbar">
          <Space>
            <Input.Search
              aria-label={t('搜索归档')}
              placeholder={t('搜索文件名或来源路径')}
              allowClear
              value={query}
              onChange={(event) => setQuery(event.target.value)}
              style={{ width: 290 }}
            />
            <Select
              aria-label={t('任务状态筛选')}
              value={filter}
              onChange={setFilter}
              style={{ width: 150 }}
              options={[
                { value: 'all', label: t('全部状态') },
                { value: 'running', label: t('处理中') },
                { value: 'waiting', label: t('等待处理') },
                { value: 'failed', label: t('需要处理') },
                { value: 'completed', label: t('已完成') },
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
          </Space>
          <Typography.Text type="secondary">
            SSD {snapshot.settings.cacheEnabled ? t('启用') : t('关闭')}
            {t('· 内包')}
            {snapshot.settings.expandNested
              ? t('共 {p0} 层', { p0: snapshot.settings.maxDepth })
              : t('保留')}{' '}
            · {snapshot.settings.verification === 'strict' ? t('目标回读') : t('标准验证')}
          </Typography.Text>
        </div>
        {viewMode === 'table' ? (
          <Table<JobView>
            rowKey="id"
            columns={columns}
            dataSource={jobs}
            virtual
            scroll={{ x: 1275, y: tableHeight }}
            pagination={false}
            rowSelection={{
              selectedRowKeys: selected,
              onChange: (keys) => setSelected(keys.map(String)),
              getCheckboxProps: (job) => ({
                disabled: deletion.busy,
                'aria-label': t('选择任务 {p0}', { p0: job.source.displayName }),
              }),
            }}
            locale={{
              emptyText: (
                <div
                  className="dropzone-container"
                  style={{
                    margin: '20px auto',
                    maxWidth: 440,
                    cursor: isDesktop ? 'pointer' : 'default',
                  }}
                  onClick={() => {
                    if (isDesktop && !scanning) pick(false);
                  }}
                  onKeyDown={(e) => {
                    if (isDesktop && !scanning && (e.key === 'Enter' || e.key === ' ')) {
                      e.preventDefault();
                      pick(false);
                    }
                  }}
                  tabIndex={isDesktop ? 0 : undefined}
                  role="region"
                  aria-label={
                    isDesktop
                      ? t('暂无解压任务，拖入压缩文件或使用上方按钮添加')
                      : t('暂无解压任务')
                  }
                >
                  <FileAddOutlined className="dropzone-icon" />
                  <div className="dropzone-title">
                    {isDesktop
                      ? t('暂无解压任务，拖入压缩文件或使用上方按钮添加')
                      : t('暂无解压任务')}
                  </div>
                  <div className="dropzone-sub">
                    {t('添加压缩文件或文件夹进行批量解压，支持分卷识别与密码匹配。')}
                  </div>
                </div>
              ),
            }}
          />
        ) : jobs.length === 0 ? (
          <div
            className="dropzone-container"
            style={{
              margin: '20px auto',
              maxWidth: 440,
              cursor: isDesktop ? 'pointer' : 'default',
            }}
            onClick={() => {
              if (isDesktop && !scanning) pick(false);
            }}
            onKeyDown={(e) => {
              if (isDesktop && !scanning && (e.key === 'Enter' || e.key === ' ')) {
                e.preventDefault();
                pick(false);
              }
            }}
            tabIndex={isDesktop ? 0 : undefined}
            role="region"
            aria-label={
              isDesktop ? t('暂无解压任务，拖入压缩文件或使用上方按钮添加') : t('暂无解压任务')
            }
          >
            <FileAddOutlined className="dropzone-icon" />
            <div className="dropzone-title">
              {isDesktop ? t('暂无解压任务，拖入压缩文件或使用上方按钮添加') : t('暂无解压任务')}
            </div>
            <div className="dropzone-sub">
              {t('添加压缩文件或文件夹进行批量解压，支持分卷识别与密码匹配。')}
            </div>
          </div>
        ) : (
          <QueueCardGrid
            jobs={jobs}
            selectedIds={selectedIds}
            onSelectChange={setSelected}
            connected={connected}
            nativeEngine={nativeEngine}
            daemon={daemon}
            snapshotRunning={snapshot.running}
            deletionBusy={deletion.busy}
            deletingIds={deletion.deletingIds}
            onOpenDetail={setDetail}
            onPromptPassword={openPasswordPrompt}
            onCancel={cancel}
            onRequestDelete={(targets) => deletion.request(targets)}
            run={run}
          />
        )}
      </div>
      <Modal
        title={t('正在扫描来源')}
        open={scanning}
        closable={false}
        destroyOnHidden
        footer={
          <Button danger disabled={scanStopping} onClick={cancelScan}>
            {scanStopping ? t('等待扫描停止') : t('取消扫描')}
          </Button>
        }
      >
        <Typography.Paragraph>
          {t('正在查找支持的归档并归组分卷。扫描不会验证归档内容是否完整，也不会修改源文件。')}
        </Typography.Paragraph>
        <Typography.Text type="secondary">
          {scanStopping
            ? t('取消请求已发出，等待读取结束。')
            : t('扫描规则：{p0}，{p1} 条输入排除规则。', {
                p0: snapshot.settings.recursiveScan ? '包含子目录' : '当前目录',
                p1: snapshot.settings.inputExclusions.length,
              })}
        </Typography.Text>
      </Modal>
      <QueueSourceReviewModal
        scan={scan}
        sources={sources}
        onSourcesChange={setSources}
        options={options}
        onOptionsChange={setOptions}
        showOptions={showOptions}
        onToggleOptions={setShowOptions}
        addingJobs={addingJobs}
        connected={connected}
        nativeEngine={nativeEngine}
        daemon={daemon}
        groups={groups}
        totalSelectedVolumes={totalSelectedVolumes}
        sourceColumns={sourceColumns}
        onPickOutputDir={() => {
          const epoch = scanEpochRef.current;
          void run(async () => {
            const path = await api.pickDirectory();
            if (path && scanEpochRef.current === epoch)
              setOptions((prev) => ({ ...prev, outputRoot: path }));
          });
        }}
        onCancel={() => {
          if (addingJobs || addBusyRef.current) return;
          scanEpochRef.current += 1;
          setScan(undefined);
          setShowOptions(false);
        }}
        onConfirmAdd={() =>
          void run(async () => {
            if (!scan || addBusyRef.current) return;
            addBusyRef.current = true;
            setAddingJobs(true);
            const epoch = scanEpochRef.current;
            const frozenSources = scan.sources.filter((source) =>
              selectedSourcePaths.has(source.path),
            );
            const frozenOptions = { ...options };
            try {
              const added = await api.addJobs(frozenSources, frozenOptions);
              const autoStart =
                frozenOptions.autoStartOnAdd ?? snapshot.settings.autoStartOnAdd ?? true;
              if (autoStart && !snapshot.paused && snapshot.engineAvailable) {
                if (daemon) {
                  if (added.length) await api.startSelected(added.map((job) => job.id));
                } else await api.start();
              }
              if (scanEpochRef.current === epoch) {
                setScan(undefined);
                setShowOptions(false);
              }
            } finally {
              addBusyRef.current = false;
              setAddingJobs(false);
            }
          })
        }
      />
      <QueuePasswordModal
        passwordJob={passwordJob}
        passwordOwnerPresent={passwordOwnerPresent}
        daemon={daemon}
        submittingPassword={submittingPassword}
        inputPassword={inputPassword}
        saveToVault={saveToVault}
        passwords={snapshot.passwords}
        onChangePassword={setInputPassword}
        onChangeSaveToVault={setSaveToVault}
        onSubmit={submitInstantPassword}
        onCancel={closePassword}
      />
      <JobDetails
        job={snapshot.jobs.find((job) => job.id === detail)}
        close={() => setDetail(undefined)}
        run={run}
        operationsDisabled={snapshot.running || deletion.busy}
        inputDisabled={deletion.busy || (snapshot.running && !daemon)}
      />
      {deletion.dialog}
    </>
  );
}
