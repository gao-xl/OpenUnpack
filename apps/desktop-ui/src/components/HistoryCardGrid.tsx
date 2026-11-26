import { Button, Checkbox, Pagination } from 'antd';
import { FolderOpenOutlined } from '@ant-design/icons';
import { t } from '../i18n';
import { api } from '../api';
import type { JobSummary } from '../types';
import { ArchiveFormatBadge, StatusTag, displayPath, verificationLabel } from './shared';
import { taskDeleteBlockedReason } from './TaskDeleteDialog';
import VirtualGrid from './VirtualGrid';

export interface HistoryCardGridProps {
  items: JobSummary[];
  selectedIds: Set<string>;
  onSelectChange: React.Dispatch<React.SetStateAction<string[]>>;
  connected: boolean;
  nativeEngine: boolean;
  deletionBusy: boolean;
  deletingIds: string[];
  onRequestDelete: (jobs: JobSummary[]) => void;
  onLoadDetails: (id: string) => Promise<void>;
  run: (work: () => Promise<unknown>) => Promise<void>;
  offset: number;
  pageSize: number;
  filteredTotal: number;
  onPageChange: (number: number, size: number) => void;
}

export default function HistoryCardGrid({
  items: historyList,
  selectedIds,
  onSelectChange: setSelected,
  connected,
  nativeEngine,
  deletionBusy,
  deletingIds,
  onRequestDelete,
  onLoadDetails: loadDetails,
  run,
  offset,
  pageSize,
  filteredTotal,
  onPageChange,
}: HistoryCardGridProps) {
  return (
    <div>
      <VirtualGrid
        items={historyList}
        itemKey={(job) => job.id}
        minItemWidth={320}
        rowHeight={180}
        containerHeight={520}
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
                    aria-label={t('选择任务 ') + job.source.displayName}
                    onChange={(e) => {
                      const checked = e.target.checked;
                      setSelected((prev) =>
                        checked ? [...prev, job.id] : prev.filter((id) => id !== job.id),
                      );
                    }}
                  />
                  <ArchiveFormatBadge name={job.source.displayName} />
                  <div style={{ minWidth: 0, flex: 1 }}>
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
                      }}
                      onClick={() => void loadDetails(job.id)}
                    >
                      {job.source.displayName}
                    </Button>
                    <div
                      className="muted mono-text"
                      style={{ fontSize: 11, wordBreak: 'break-all', marginTop: 2 }}
                    >
                      {displayPath(job.source.path)}
                    </div>
                  </div>
                </div>
                <StatusTag status={job.status} />
              </div>

              <div
                style={{
                  display: 'flex',
                  justifyContent: 'space-between',
                  alignItems: 'center',
                  fontSize: 11,
                  color: 'var(--ou-text-muted)',
                }}
              >
                <span className="mono-text">{verificationLabel(job.verificationScope)}</span>
                <span className="tabular-nums">
                  {job.finishedAt
                    ? new Date(job.finishedAt).toLocaleTimeString()
                    : job.createdAt
                      ? new Date(job.createdAt).toLocaleTimeString()
                      : ''}
                </span>
              </div>

              {job.outputPath && (
                <div className="mono-text muted" style={{ fontSize: 11, wordBreak: 'break-all' }}>
                  <FolderOpenOutlined style={{ marginRight: 4 }} aria-hidden />
                  {displayPath(job.outputPath)}
                </div>
              )}

              <div
                style={{
                  display: 'flex',
                  justifyContent: 'flex-end',
                  gap: 6,
                  borderTop: '1px solid var(--ou-border-color)',
                  paddingTop: 6,
                  marginTop: 'auto',
                }}
              >
                <Button size="small" type="link" onClick={() => void loadDetails(job.id)}>
                  {t('详情')}
                </Button>
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
                  aria-label={t('删除记录 {p0}', { p0: job.source.displayName })}
                  title={taskDeleteBlockedReason(job)}
                  disabled={!connected || deletionBusy || !!taskDeleteBlockedReason(job)}
                  loading={deletingIds.includes(job.id)}
                  onClick={() => onRequestDelete([job])}
                >
                  {deletingIds.includes(job.id) ? t('正在删除') : t('删除')}
                </Button>
              </div>
            </div>
          );
        }}
      />
      <Pagination
        current={Math.floor(offset / pageSize) + 1}
        pageSize={pageSize}
        total={filteredTotal}
        showSizeChanger
        pageSizeOptions={[10, 20, 50, 100]}
        onChange={onPageChange}
        style={{ marginTop: 16, display: 'flex', justifyContent: 'flex-end' }}
      />
    </div>
  );
}
