import { t } from '../i18n';
import { useEffect, useState } from 'react';
import { Typography } from 'antd';
import type { JobView, TransferProgress } from '../types';

const FRESHNESS_MS = 5_000;
const transferStages = new Set(['caching', 'extracting', 'nested', 'copying', 'verifying']);
const finiteNonnegative = (value: number) => Number.isFinite(value) && value >= 0 && value <= Number.MAX_SAFE_INTEGER;
const safeBytes = (value: number) => Number.isSafeInteger(value) && value >= 0;

function sampleTime(value: string | undefined): number {
  if (typeof value !== 'string' || !/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d{1,9})?(?:Z|[+-]\d{2}:\d{2})$/.test(value)) return NaN;
  // Date.parse normalizes dates such as February 30; those are not valid samples.
  const calendarDate = new Date(`${value.slice(0, 10)}T00:00:00Z`);
  if (!Number.isFinite(calendarDate.getTime()) || calendarDate.toISOString().slice(0, 10) !== value.slice(0, 10) || Number(value.slice(11, 13)) > 23) return NaN;
  return Date.parse(value);
}

function isSampleFresh(sampledAt: number): boolean {
  if (!Number.isFinite(sampledAt)) return false;
  const now = Date.now();
  return sampledAt <= now && now - sampledAt < FRESHNESS_MS;
}

function validValues(progress: TransferProgress): boolean {
  return safeBytes(progress.completedBytes) && safeBytes(progress.totalBytes)
    && (progress.totalBytes === 0 || progress.completedBytes <= progress.totalBytes)
    && (progress.bytesPerSecond === null || finiteNonnegative(progress.bytesPerSecond))
    && (progress.remainingSeconds === null || finiteNonnegative(progress.remainingSeconds));
}

export function formatTransferRate(bytesPerSecond: number): string {
  if (!finiteNonnegative(bytesPerSecond) || bytesPerSecond <= 0) return '—';
  const units = ['B/s', 'KiB/s', 'MiB/s', 'GiB/s', 'TiB/s'];
  const index = Math.min(units.length - 1, Math.max(0, Math.floor(Math.log(bytesPerSecond) / Math.log(1024))));
  const amount = bytesPerSecond / 1024 ** index;
  if (index === 0 && amount < 0.1) return '<0.1 B/s';
  return `${amount.toFixed(index === 0 && Number.isInteger(amount) ? 0 : 1)} ${units[index]}`;
}

export function formatRemainingTime(seconds: number): string {
  if (!finiteNonnegative(seconds)) return '—';
  const rounded = Math.ceil(seconds);
  if (rounded < 60) return t("{value0} 秒", { value0: rounded });
  if (rounded < 3_600) return t("{value0} 分 {value1} 秒", { value0: Math.floor(rounded / 60), value1: rounded % 60 });
  if (rounded < 86_400) return t("{value0} 小时 {value1} 分", { value0: Math.floor(rounded / 3_600), value1: Math.floor(rounded % 3_600 / 60) });
  if (rounded >= 100 * 86_400) return t("超过 99 天");
  return t("{value0} 天 {value1} 小时", { value0: Math.floor(rounded / 86_400), value1: Math.floor(rounded % 86_400 / 3_600) });
}

/** The expiry timer hides old observations; it never counts down or invents progress. */
export default function TransferMetrics({ job }: { job: JobView }) {
  const [expiredSample, expireSample] = useState<string | null>(null);
  const active = job.status === 'running' && transferStages.has(job.stage);
  const progress = job.transferProgress;
  const sampledAt = sampleTime(progress?.updatedAt);
  const sampleKey = JSON.stringify([job.id, job.stage, progress?.updatedAt]);
  useEffect(() => {
    if (!active || !Number.isFinite(sampledAt) || expiredSample === sampleKey) return;
    const now = Date.now();
    if (sampledAt > now || now - sampledAt >= FRESHNESS_MS) return;
    // A clock correction cannot make a timer-expired observation fresh again.
    const timer = window.setTimeout(() => expireSample(sampleKey), sampledAt + FRESHNESS_MS - now);
    return () => window.clearTimeout(timer);
  }, [job.status, sampledAt, sampleKey, active, expiredSample]);

  if (job.status !== 'running') return null;
  if (!active) return <Typography.Text type="secondary" style={{ display: 'block', fontSize: 12 }}>{t("本阶段暂无法估算速度与剩余时间")}</Typography.Text>;

  const fresh = isSampleFresh(sampledAt);
  const stale = progress && (expiredSample === sampleKey || !fresh || !validValues(progress));
  let speed = t("正在估算");
  let remaining = t("正在估算");
  if (stale) {
    speed = t("等待新进度");
    remaining = t("等待新进度");
  } else if (progress && progress.completedBytes > 0 && progress.bytesPerSecond !== null && progress.bytesPerSecond > 0) {
    speed = formatTransferRate(progress.bytesPerSecond);
    remaining = progress.totalBytes > 0 && progress.remainingSeconds !== null ? t("约 {value0}", { value0: formatRemainingTime(progress.remainingSeconds) }) : t("暂无法估算");
  }
  const speedLabel = ['extracting', 'nested'].includes(job.stage) ? t("输出速度") : job.stage === 'verifying' ? t("回读速度") : t("传输速度");
  const remainingLabel = job.stage === 'nested' ? t("预计当前内包剩余") : t("预计本阶段剩余");
  return <div role="group" aria-label={t("实时速度与预计剩余时间")} style={{ fontSize: 12, lineHeight: 1.5, fontVariantNumeric: 'tabular-nums' }}>
    <Typography.Text type="secondary" style={{ display: 'block', fontSize: 'inherit' }}>{speedLabel}：{speed}</Typography.Text>
    <Typography.Text type="secondary" style={{ display: 'block', fontSize: 'inherit' }}>{remainingLabel}：{remaining}</Typography.Text>
  </div>;
}
