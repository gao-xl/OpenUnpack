import { t } from '../i18n';
import { memo, useMemo } from 'react';
import { useJobProgress, useAggregateProgress } from '../progressStore';
import { Progress } from 'antd';
import { ThunderboltOutlined, ClockCircleOutlined } from '@ant-design/icons';
import type { AppSnapshot, JobView } from '../types';
import { stageLabels } from '../types';
import { formatBytes, percent } from './shared';
import TransferMetrics, {
  formatTransferRate,
  formatRemainingTime,
  isSampleFresh,
  sampleTime,
  validValues,
} from './TransferMetrics';

export const JobStageCell = memo(function JobStageCell({ job }: { job: JobView }) {
  const live = useJobProgress(job.id);
  const isRunning = job.status === 'running';
  const isFailed = job.status === 'failed';

  const rawProgress =
    isRunning && live && live.progress !== undefined ? live.progress : job.progress;
  const pct = percent(rawProgress);

  const prog = job.transferProgress;
  const currentBytes =
    isRunning && live && live.completedBytes !== undefined
      ? live.completedBytes
      : (prog?.completedBytes ??
        (job.source.size > 0
          ? Math.round(job.source.size * Math.max(0, Math.min(1, job.progress)))
          : 0));

  const totalBytes =
    isRunning && live && live.totalBytes !== undefined && live.totalBytes > 0
      ? live.totalBytes
      : prog?.totalBytes && prog.totalBytes > 0
        ? prog.totalBytes
        : job.source.size;

  const stage = isRunning && live && live.stage ? live.stage : job.stage;

  return (
    <div className="stage-progress-cell">
      <div className="stage-progress-header">
        <span
          style={{
            fontWeight: isRunning ? 600 : 400,
            color: isRunning ? 'var(--ou-primary)' : undefined,
          }}
        >
          {isRunning && <span className="ou-pulse-dot" style={{ marginRight: 6 }} />}
          {t(stageLabels[stage] ?? stage)}
        </span>
        <span className="stage-data-bytes">
          {totalBytes > 0 ? `${formatBytes(currentBytes)} / ${formatBytes(totalBytes)}` : `${pct}%`}
        </span>
      </div>
      <Progress
        size="small"
        percent={pct}
        status={isFailed ? 'exception' : isRunning ? 'active' : undefined}
        strokeColor={
          isRunning ? { from: 'var(--ou-primary)', to: 'var(--ou-primary-hover)' } : undefined
        }
      />
      <TransferMetrics job={job} />
    </div>
  );
});

export const JobCardStageProgress = memo(function JobCardStageProgress({ job }: { job: JobView }) {
  const live = useJobProgress(job.id);
  const isRunning = job.status === 'running';
  const isFailed = job.status === 'failed';

  const rawProgress =
    isRunning && live && live.progress !== undefined ? live.progress : job.progress;
  const pct = percent(rawProgress);

  const prog = job.transferProgress;
  const currentBytes =
    isRunning && live && live.completedBytes !== undefined
      ? live.completedBytes
      : (prog?.completedBytes ??
        (job.source.size > 0
          ? Math.round(job.source.size * Math.max(0, Math.min(1, job.progress)))
          : 0));

  const totalBytes =
    isRunning && live && live.totalBytes !== undefined && live.totalBytes > 0
      ? live.totalBytes
      : prog?.totalBytes && prog.totalBytes > 0
        ? prog.totalBytes
        : job.source.size;

  const stage = isRunning && live && live.stage ? live.stage : job.stage;

  return (
    <>
      <div
        style={{
          display: 'flex',
          justifyContent: 'space-between',
          alignItems: 'center',
          fontSize: 11,
          color: 'var(--ou-text-muted)',
        }}
      >
        <span className="mono-text">
          {formatBytes(job.source.size)}
          {job.source.volumes.length > 1
            ? t(' · {p0} 个分卷', { p0: job.source.volumes.length })
            : ''}
        </span>
        <span>{t(stageLabels[stage] ?? stage)}</span>
      </div>

      <Progress
        size="small"
        percent={pct}
        status={isFailed ? 'exception' : isRunning ? 'active' : undefined}
        strokeColor={
          isRunning ? { from: 'var(--ou-primary)', to: 'var(--ou-primary-hover)' } : undefined
        }
        style={{ margin: 0 }}
      />

      <div
        style={{
          display: 'flex',
          justifyContent: 'space-between',
          alignItems: 'center',
          fontSize: 11,
          color: 'var(--ou-text-muted)',
        }}
      >
        <span className="stage-data-bytes">
          {totalBytes > 0 ? `${formatBytes(currentBytes)} / ${formatBytes(totalBytes)}` : `${pct}%`}
        </span>
        <TransferMetrics job={job} />
      </div>
    </>
  );
});

export const BatchProgressCard = memo(function BatchProgressCard({
  snapshot,
  counts,
}: {
  snapshot: AppSnapshot;
  counts: { total: number; running: number; waiting: number; completed: number; errors: number };
}) {
  const store = useAggregateProgress();
  const batchStats = useMemo(() => {
    let totalBytes = 0;
    let completedBytes = 0;
    let aggregateSpeed = 0;
    let maxRemainingSeconds = 0;
    let activeSpeedCount = 0;

    for (const job of snapshot.jobs) {
      const sourceSize = job.source.size || 0;
      totalBytes += sourceSize;

      if (['completed', 'cleanupPending'].includes(job.status)) {
        completedBytes += sourceSize;
      } else if (job.status === 'running') {
        const live = store.entries.get(job.id);
        if (live && live.completedBytes !== undefined) {
          completedBytes += Math.min(live.completedBytes, sourceSize);
          if (live.bytesPerSecond && live.bytesPerSecond > 0) {
            aggregateSpeed += live.bytesPerSecond;
            activeSpeedCount += 1;
          }
          if (live.remainingSeconds && live.remainingSeconds > maxRemainingSeconds) {
            maxRemainingSeconds = live.remainingSeconds;
          }
        } else {
          const prog = job.transferProgress;
          const fresh = prog ? isSampleFresh(sampleTime(prog.updatedAt)) : false;
          if (prog && fresh && validValues(prog)) {
            completedBytes += Math.min(prog.completedBytes, sourceSize);
            if (prog.bytesPerSecond && prog.bytesPerSecond > 0) {
              aggregateSpeed += prog.bytesPerSecond;
              activeSpeedCount += 1;
            }
            if (prog.remainingSeconds && prog.remainingSeconds > maxRemainingSeconds) {
              maxRemainingSeconds = prog.remainingSeconds;
            }
          } else {
            completedBytes += Math.round(sourceSize * Math.max(0, Math.min(1, job.progress)));
          }
        }
      }
    }

    const overallPercent =
      totalBytes > 0
        ? Math.round((completedBytes / totalBytes) * 100)
        : counts.total > 0
          ? Math.round((counts.completed / counts.total) * 100)
          : 0;
    return {
      totalBytes,
      completedBytes,
      overallPercent: Math.min(100, Math.max(0, overallPercent)),
      aggregateSpeed,
      hasSpeed: activeSpeedCount > 0 && aggregateSpeed > 0,
      remainingSeconds: maxRemainingSeconds > 0 ? maxRemainingSeconds : null,
    };
  }, [snapshot.jobs, counts.total, counts.completed, store]);

  return (
    <div className="batch-progress-card">
      <div className="batch-progress-header">
        <div className="batch-progress-title">
          {snapshot.running ? (
            <>
              <span className="ou-pulse-dot" />
              <span>{t('批量解压进行中')}</span>
            </>
          ) : snapshot.paused ? (
            <span>{t('队列已暂停')}</span>
          ) : counts.waiting > 0 ? (
            <span>{t('任务就绪，等待开始')}</span>
          ) : counts.completed === counts.total ? (
            <>
              <span className="ou-pulse-dot success" />
              <span style={{ color: 'var(--ou-success)' }}>{t('所有任务已完成')}</span>
            </>
          ) : (
            <span>{t('队列总览')}</span>
          )}
        </div>
        <div className="batch-progress-stats">
          <span className="batch-stat-item">
            <span>{t('进度：')}</span>
            <span className="batch-stat-highlight">
              {counts.completed} / {counts.total} {t('项')} ({batchStats.overallPercent}%)
            </span>
          </span>
          {batchStats.totalBytes > 0 && (
            <span className="batch-stat-item">
              <span>{t('数据量：')}</span>
              <span className="batch-stat-highlight">
                {formatBytes(batchStats.completedBytes)} / {formatBytes(batchStats.totalBytes)}
              </span>
            </span>
          )}
          {snapshot.running && batchStats.hasSpeed && (
            <span className="batch-stat-item">
              <span>
                <ThunderboltOutlined
                  style={{ marginRight: 4, color: 'var(--ou-primary)' }}
                  aria-hidden
                />
                {t('实时速率：')}
              </span>
              <span className="batch-stat-highlight" style={{ color: 'var(--ou-primary)' }}>
                {formatTransferRate(batchStats.aggregateSpeed)}
              </span>
            </span>
          )}
          {snapshot.running && batchStats.remainingSeconds !== null && (
            <span className="batch-stat-item">
              <span>
                <ClockCircleOutlined style={{ marginRight: 4 }} aria-hidden />
                {t('预计剩余：')}
              </span>
              <span className="batch-stat-highlight">
                {formatRemainingTime(batchStats.remainingSeconds)}
              </span>
            </span>
          )}
        </div>
      </div>
      <Progress
        percent={batchStats.overallPercent}
        status={snapshot.running ? 'active' : counts.errors > 0 ? 'exception' : undefined}
        strokeColor={
          snapshot.running
            ? { from: 'var(--ou-primary)', to: 'var(--ou-primary-hover)' }
            : counts.completed === counts.total && counts.total > 0
              ? 'var(--ou-success)'
              : undefined
        }
        style={{ marginBottom: 0 }}
      />
    </div>
  );
});
