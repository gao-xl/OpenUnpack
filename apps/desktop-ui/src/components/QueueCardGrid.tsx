import { Button, Checkbox, Space } from 'antd';
import { t } from '../i18n';
import { api } from '../api';
import { analyzeJobIssue } from '../jobTroubleshooting';
import type { JobView } from '../types';
import { ArchiveFormatBadge, JobAccelerationBadge, StatusTag, retryable } from './shared';
import { taskDeleteBlockedReason } from './TaskDeleteDialog';
import { JobCardStageProgress } from './QueueProgressCards';
import VirtualGrid from './VirtualGrid';

export interface QueueCardGridProps {
  jobs: JobView[];
  selectedIds: Set<string>;
  onSelectChange: React.Dispatch<React.SetStateAction<string[]>>;
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

export default function QueueCardGrid({
  jobs,
  selectedIds,
  onSelectChange: setSelected,
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
}: QueueCardGridProps) {
  return (
    <VirtualGrid
      items={jobs}
      itemKey={(job) => job.id}
      minItemWidth={320}
      rowHeight={225}
      containerHeight={540}
      renderItem={(job) => {
        const isSelected = selectedIds.has(job.id);

        return (
          <div
            className="telemetry-card"
            style={{
              padding: '12px 14px',
              border: isSelected ? '1.5px solid var(--ou-primary)' : undefined,
              display: 'flex',
              flexDirection: 'column',
              gap: 8,
              height: '100%',
              boxSizing: 'border-box',
            }}
          >
            <div
              style={{
                display: 'flex',
                alignItems: 'flex-start',
                justifyContent: 'space-between',
                gap: 8,
              }}
            >
              <div
                style={{
                  display: 'flex',
                  alignItems: 'flex-start',
                  gap: 8,
                  minWidth: 0,
                  flex: 1,
                }}
              >
                <Checkbox
                  checked={isSelected}
                  disabled={deletionBusy}
                  aria-label={t('选择任务 {p0}', { p0: job.source.displayName })}
                  onChange={(e) => {
                    const checked = e.target.checked;
                    setSelected((prev) =>
                      checked ? [...prev, job.id] : prev.filter((id) => id !== job.id),
                    );
                  }}
                />
                <ArchiveFormatBadge name={job.source.displayName} />
                <Button
                  type="link"
                  style={{
                    padding: 0,
                    fontWeight: 600,
                    fontSize: 13,
                    height: 'auto',
                    textAlign: 'left',
                    whiteSpace: 'normal',
                    wordBreak: 'break-all',
                    minWidth: 0,
                    flex: 1,
                  }}
                  onClick={() => setDetail(job.id)}
                >
                  {job.source.displayName}
                </Button>
              </div>
              <Space size={6} wrap>
                <StatusTag status={job.status} />
                <JobAccelerationBadge job={job} />
              </Space>
            </div>

            <JobCardStageProgress job={job} />

            {(() => {
              const analysis = analyzeJobIssue(job);
              if (!analysis) return null;
              return (
                <div
                  role="button"
                  tabIndex={0}
                  style={{
                    backgroundColor: 'var(--ou-bg-input)',
                    padding: '6px 8px',
                    borderRadius: 4,
                    fontSize: 11,
                    borderLeft:
                      analysis.category === 'needsPassword'
                        ? '3px solid var(--ou-warning)'
                        : '3px solid var(--ou-danger)',
                    cursor: 'pointer',
                  }}
                  onClick={() => setDetail(job.id)}
                  onKeyDown={(e) => {
                    if (e.key === 'Enter' || e.key === ' ') {
                      e.preventDefault();
                      setDetail(job.id);
                    }
                  }}
                >
                  <div
                    style={{
                      fontWeight: 600,
                      color:
                        analysis.category === 'needsPassword'
                          ? 'var(--ou-warning)'
                          : 'var(--ou-danger)',
                    }}
                  >
                    {analysis.title}
                  </div>
                  <div
                    style={{
                      color: 'var(--ou-text-secondary)',
                      marginTop: 2,
                      overflow: 'hidden',
                      textOverflow: 'ellipsis',
                      whiteSpace: 'nowrap',
                    }}
                  >
                    {analysis.guideSteps[0]}
                  </div>
                </div>
              );
            })()}

            <div
              style={{
                display: 'flex',
                justifyContent: 'flex-end',
                flexWrap: 'wrap',
                gap: 4,
                borderTop: '1px solid var(--ou-border-color)',
                paddingTop: 6,
                marginTop: 'auto',
              }}
            >
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
            </div>
          </div>
        );
      }}
    />
  );
}
