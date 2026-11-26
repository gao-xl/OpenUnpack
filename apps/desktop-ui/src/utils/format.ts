import { t } from '../i18n';
import type { JobView } from '../types';

export const formatBytes = (value: number) => {
  if (!Number.isFinite(value) || value <= 0) return '0 B';
  const units = ['B', 'KiB', 'MiB', 'GiB', 'TiB'];
  const index = Math.min(4, Math.floor(Math.log(value) / Math.log(1024)));
  return `${(value / 1024 ** index).toFixed(index === 0 ? 0 : 1)} ${units[index]}`;
};
// Renders canonical paths returned by the native side (\\?\C:\... / \\?\UNC\...) in the
// conventional Windows form. Display-only: values sent back over IPC stay untouched.
export const displayPath = (value: string | null | undefined): string => {
  if (!value) return '';
  if (value.startsWith('\\\\?\\UNC\\')) return `\\\\${value.slice(8)}`;
  if (/^\\\\\?\\[a-zA-Z]:/.test(value)) return value.slice(4);
  return value;
};
export const percent = (value: number) => Math.round(Math.max(0, Math.min(1, value)) * 100);
export const retryable = (job: JobView) =>
  ['failed', 'needsPassword', 'cancelled', 'interrupted', 'waitingResource'].includes(job.status);
export const terminal = (job: JobView) =>
  [
    'completed',
    'cleanupPending',
    'failed',
    'needsPassword',
    'cancelled',
    'interrupted',
    'partial',
    'salvaged',
    'skipped',
  ].includes(job.status);
export const canDeliverPartial = (job: JobView) =>
  !job.outputPath &&
  !!job.workspace &&
  ['failed', 'needsPassword', 'interrupted'].includes(job.status) &&
  job.nodes.some((node) => node.depth === 0 && ['completed', 'salvaged'].includes(node.status)) &&
  job.nodes.some(
    (node) => node.depth > 0 && ['failed', 'needsPassword', 'interrupted'].includes(node.status),
  );
export const verificationLabel = (scope: string) =>
  scope.includes('targetReadback')
    ? t('引擎完整性 / 目标文件回读')
    : scope.includes('sourceStreamSha256')
      ? t('引擎完整性 / 写回复制源流摘要')
      : scope.includes('salvaged')
        ? t('抢救提取（已跳过损坏条目回读）')
        : ((
            {
              targetReadback: t('目标文件回读'),
              sourceStream: t('复制源流摘要'),
              archiveIntegrity: t('归档引擎完整性'),
              standard: t('标准流程验证'),
              full: t('完整目标回读'),
              salvagedWithCrcWarning: t('抢救提取（带CRC损坏警示）'),
              salvagedPartial: t('抢救提取（部分条目损坏）'),
              inPlaceVerified: t('原地解压（解压前全量校验 + 输出清单核对）'),
            } as Record<string, string>
          )[scope] ??
          (scope || t('尚未完成')));
