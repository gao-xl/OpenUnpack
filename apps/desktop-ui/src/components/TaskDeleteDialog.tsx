import { t } from '../i18n';
import { useRef, useState } from 'react';
import { Alert, Modal, Typography } from 'antd';
import { api } from '../api';
import type { JobView } from '../types';

type DeletionTarget = Pick<JobView, 'id' | 'source' | 'status'>;

export function taskDeleteBlockedReason(job: DeletionTarget): string | undefined {
  if (job.status === 'publishPending') return t("发布结果尚未核对，请先回读并核对或重试写回，再删除任务。");
  if (job.status === 'stopping') return t("正在等待后台进程停止，请停止完成后再删除任务。");
  return undefined;
}

export function useTaskDeletion(
  run: (work: () => Promise<unknown>) => Promise<void>,
  onRemoved: (ids: string[]) => void | Promise<void>,
) {
  const [jobs, setJobs] = useState<DeletionTarget[]>([]);
  const [busy, setBusy] = useState(false);
  const busyRef = useRef(false);
  const [errors, setErrors] = useState<string[]>([]);
  const [deletingIds, setDeletingIds] = useState<string[]>([]);

  const request = (items: DeletionTarget[]) => {
    if (busyRef.current || !items.length) return;
    setJobs(items);
    setErrors([]);
  };
  const confirm = () => {
    if (busyRef.current || !jobs.length) return;
    busyRef.current = true;
    setBusy(true);
    setDeletingIds(jobs.map(job => job.id));
    setErrors([]);
    void run(async () => {
      const removed: string[] = [];
      const failed: DeletionTarget[] = [];
      const messages: string[] = [];
      try {
        // Dispatch all requests before waiting, so queued selections can register
        // their stop/delete intent while an active task is still stopping.
        const results = await Promise.allSettled(jobs.map(async job => {
          await api.remove(job.id);
          return job.id;
        }));
        for (let index = 0; index < results.length; index++) {
          const result = results[index];
          const job = jobs[index];
          if (result.status === 'fulfilled') removed.push(result.value);
          else {
            failed.push(job);
            messages.push(`${job.source.displayName}：${result.reason instanceof Error ? result.reason.message : String(result.reason)}`);
          }
        }
        setJobs(failed);
        setErrors(messages);
        if (removed.length) await onRemoved(removed);
        if (messages.length) throw new Error(t("未能删除 {p0} 个任务。{p1}", { p0: failed.length, p1: messages.join('；') }));
      } finally {
        busyRef.current = false;
        setBusy(false);
        setDeletingIds([]);
      }
    }).catch(() => { /* App.run reports the error; failed tasks remain in this dialog. */ });
  };

  const dialog = <Modal
    title={jobs.length > 1 ? t("删除 {p0} 个任务？", { p0: jobs.length }) : t("删除任务？")}
    open={!!jobs.length}
    destroyOnHidden
    closable={!busy}
    keyboard={!busy}
    mask={{ closable: false }}
    confirmLoading={busy}
    okText={busy ? t("正在删除") : errors.length ? t("重试删除") : t("删除任务")}
    okButtonProps={{ danger: true, disabled: busy, 'aria-label': busy ? t("正在删除") : errors.length ? t("重试删除") : t("删除任务") }}
    cancelText={t("取消")}
    cancelButtonProps={{ disabled: busy }}
    onCancel={() => { if (!busyRef.current) { setJobs([]); setErrors([]); } }}
    onOk={confirm}
  >
    <Typography.Paragraph>{t("运行中的任务将先停止，确认后台进程停止后再删除任务记录，并清理本应用的临时缓存。")}</Typography.Paragraph>
    <Typography.Paragraph>{t("任务记录将同时从队列和处理记录中移除。")}</Typography.Paragraph>
    <Typography.Paragraph strong>{t("原始压缩包和已经发布到目标目录的结果会保留。")}</Typography.Paragraph>
    <Typography.Paragraph type="secondary">{t("删除任务不会删除这些文件，也不能恢复此前已执行的源包清理。")}</Typography.Paragraph>
    <ul style={{ maxHeight: 180, overflow: 'auto', paddingLeft: 24 }}>{jobs.map(job => <li key={job.id}>{job.source.displayName}</li>)}</ul>
    {busy && <Typography.Paragraph role="status">{t("正在等待任务停止并清理缓存，请稍候。")}</Typography.Paragraph>}
    {!!errors.length && <Alert type="error" showIcon title={t("这些任务未删除，可重试或取消")} description={<ul>{errors.map((error, index) => <li key={index}>{error}</li>)}</ul>} />}
  </Modal>;
  return { request, busy, deletingIds, dialog };
}
