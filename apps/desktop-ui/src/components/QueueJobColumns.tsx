import { Button, Space, Tag, Tooltip, Typography } from 'antd';
import type { ColumnsType } from 'antd/es/table';
import { t } from '../i18n';
import { api } from '../api';
import { analyzeJobIssue, formatJobMessage } from '../jobTroubleshooting';
import type { JobView, SourceGroup } from '../types';
import {
  ArchiveFormatBadge,
  JobAccelerationBadge,
  StatusTag,
  displayPath,
  formatBytes,
  retryable,
} from './shared';
import { taskDeleteBlockedReason } from './TaskDeleteDialog';
import { JobStageCell } from './QueueProgressCards';

export interface QueueJobActionsContext {
  connected: boolean;
  nativeEngine: boolean;
  daemon: boolean;
  snapshotRunning: boolean;
  deletionBusy: boolean;
  deletingIds: string[];
  onOpenDetail: (jobId: string) => void;
  onPromptPassword: (job: JobView, saveDefault: boolean) => void;
  onCancel: (ids: string[]) => void;
  onRequestDelete: (jobs: JobView[]) => void;
  run: (work: () => Promise<unknown>) => Promise<void>;
}

export function useQueueJobColumns(ctx: QueueJobActionsContext): ColumnsType<JobView> {
  const {
    connected,
    nativeEngine,
    daemon,
    snapshotRunning,
    deletionBusy,
    deletingIds,
    onOpenDetail: setDetail,
    onPromptPassword,
    onCancel: cancel,
    onRequestDelete,
    run,
  } = ctx;
  return [
    {
      title: t('归档文件'),
      key: 'name',
      width: 275,
      render: (_, job) => {
        return (
          <div style={{ display: 'flex', alignItems: 'flex-start', gap: 8 }}>
            <ArchiveFormatBadge name={job.source.displayName} />
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
                {job.source.volumes.length > 1
                  ? t(' · {p0} 个分卷', { p0: job.source.volumes.length })
                  : ''}
              </div>
            </div>
          </div>
        );
      },
    },
    {
      title: t('状态'),
      key: 'status',
      width: 140,
      render: (_, job) => (
        <Space size={6} wrap>
          <StatusTag status={job.status} />
          <JobAccelerationBadge job={job} />
        </Space>
      ),
    },
    {
      title: t('当前阶段'),
      key: 'stage',
      width: 270,
      render: (_, job) => <JobStageCell job={job} />,
    },
    {
      title: t('密码范围'),
      key: 'password',
      width: 145,
      render: (_, job) => job.options.passwordGroup || t('全部启用密码'),
    },
    {
      title: t('输出 / 提示'),
      key: 'message',
      width: 250,
      render: (_, job) => {
        const analysis = analyzeJobIssue(job);
        if (analysis) {
          return (
            <Tooltip
              title={
                <div style={{ maxWidth: 300, fontSize: 12 }}>
                  <div style={{ fontWeight: 600, marginBottom: 4 }}>{analysis.title}</div>
                  <div style={{ marginBottom: 4 }}>{analysis.summary}</div>
                  <ul style={{ margin: 0, paddingLeft: 16 }}>
                    {analysis.guideSteps.slice(0, 2).map((s, i) => (
                      <li key={i}>{s}</li>
                    ))}
                  </ul>
                </div>
              }
            >
              <Space
                orientation="vertical"
                size={2}
                style={{ width: '100%', cursor: 'pointer' }}
                onClick={() => setDetail(job.id)}
              >
                <Space size={4}>
                  <Tag
                    color={analysis.category === 'needsPassword' ? 'warning' : 'error'}
                    style={{ margin: 0, fontSize: 11 }}
                  >
                    {analysis.categoryLabel}
                  </Tag>
                  <Typography.Text
                    type={analysis.category === 'needsPassword' ? 'warning' : 'danger'}
                    ellipsis
                    style={{ fontSize: 12, maxWidth: 140 }}
                  >
                    {analysis.title}
                  </Typography.Text>
                </Space>
                <Typography.Text type="secondary" ellipsis style={{ fontSize: 11, maxWidth: 220 }}>
                  {formatJobMessage(job.message) || analysis.summary}
                </Typography.Text>
              </Space>
            </Tooltip>
          );
        }
        return (
          <Typography.Text ellipsis style={{ maxWidth: 220, fontSize: 12 }}>
            {formatJobMessage(job.message) ||
              displayPath(job.outputPath) ||
              displayPath(job.options.outputRoot) ||
              t('原始归档所在目录')}
          </Typography.Text>
        );
      },
    },
    {
      title: t('操作'),
      key: 'actions',
      width: 245,
      fixed: 'right',
      render: (_, job) => (
        <Space size={0} wrap>
          <Button size="small" type="link" onClick={() => setDetail(job.id)}>
            {t('详情')}
          </Button>
          {(job.status === 'needsPassword' ||
            (job.status === 'running' && job.stage === 'needsPassword')) && (
            <Button
              size="small"
              type="link"
              style={{ color: 'var(--ou-warning)', fontWeight: 600, padding: '0 4px' }}
              disabled={!connected || deletionBusy}
              onClick={() => {
                onPromptPassword(job, !daemon);
              }}
            >
              {t('输入密码')}
            </Button>
          )}
          {retryable(job) && job.status !== 'needsPassword' && (
            <>
              <Button
                size="small"
                type="link"
                disabled={!connected || snapshotRunning || deletionBusy}
                onClick={() => void run(() => api.retry(job.id))}
              >
                {t('重试')}
              </Button>
              <Button
                size="small"
                type="link"
                style={{ padding: '0 4px' }}
                disabled={!connected || snapshotRunning || deletionBusy}
                onClick={() => {
                  onPromptPassword(job, false);
                }}
              >
                {t('指定密码重试')}
              </Button>
            </>
          )}
          {['running', 'queued', 'paused', 'waitingResource'].includes(job.status) && (
            <Button
              size="small"
              type="link"
              danger
              disabled={!connected || deletionBusy}
              onClick={() => void cancel([job.id])}
            >
              {t('停止')}
            </Button>
          )}
          {job.status === 'stopping' && (
            <Button
              size="small"
              type="link"
              danger
              style={{ padding: '0 4px' }}
              disabled={!connected || deletionBusy}
              title={t('退出尚未确认时保留任务和临时缓存；停止完成或重启后处理恢复记录')}
              onClick={() => onRequestDelete([job])}
            >
              {t('尝试删除')}
            </Button>
          )}
          {job.outputPath &&
            ['completed', 'cleanupPending', 'partial', 'salvaged'].includes(job.status) && (
              <Button
                size="small"
                type="link"
                disabled={!nativeEngine}
                onClick={() => void run(() => api.openPath(job.outputPath!))}
              >
                {t('打开')}
              </Button>
            )}
          <Button
            size="small"
            type="link"
            danger
            aria-label={t('删除任务 {p0}', { p0: job.source.displayName })}
            title={taskDeleteBlockedReason(job)}
            disabled={!connected || deletionBusy || !!taskDeleteBlockedReason(job)}
            loading={deletingIds.includes(job.id)}
            onClick={() => onRequestDelete([job])}
          >
            {deletingIds.includes(job.id) ? t('正在删除') : t('删除任务')}
          </Button>
        </Space>
      ),
    },
  ];
}

export function createSourceReviewColumns(): ColumnsType<SourceGroup> {
  return [
    {
      title: t('归档'),
      key: 'source',
      render: (_, source) => (
        <>
          <strong>{source.displayName}</strong>
          <div className="muted" style={{ wordBreak: 'break-all' }}>
            {displayPath(source.path)}
          </div>
        </>
      ),
    },
    { title: t('大小'), dataIndex: 'size', render: formatBytes },
    {
      title: t('分卷'),
      key: 'volumes',
      render: (_, source) =>
        source.missingVolume ? (
          <Tag color="error">{t('缺少分卷')}</Tag>
        ) : (
          t('{p0} 卷', { p0: source.volumes.length || 1 })
        ),
    },
  ];
}
