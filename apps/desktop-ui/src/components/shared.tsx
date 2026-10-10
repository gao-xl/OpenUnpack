import { App as AntApp, Alert, Button, Descriptions, Drawer, Progress, Space, Tag, Tree, Typography } from 'antd';
import type { DataNode } from 'antd/es/tree';
import { api, isDesktop } from '../api';
import type { JobView } from '../types';
import { stageLabels, statusLabels } from '../types';
import { sourceCleanupLabels } from './SourceCleanupSelect';

export const formatBytes = (value: number) => {
  if (!Number.isFinite(value) || value <= 0) return '0 B';
  const units = ['B', 'KiB', 'MiB', 'GiB', 'TiB'];
  const index = Math.min(4, Math.floor(Math.log(value) / Math.log(1024)));
  return `${(value / 1024 ** index).toFixed(index === 0 ? 0 : 1)} ${units[index]}`;
};
export const percent = (value: number) => Math.round(Math.max(0, Math.min(1, value)) * 100);
export const retryable = (job: JobView) => ['failed', 'needsPassword', 'cancelled', 'interrupted', 'waitingResource'].includes(job.status);
export const terminal = (job: JobView) => ['completed', 'cleanupPending', 'failed', 'needsPassword', 'cancelled', 'interrupted', 'partial', 'skipped'].includes(job.status);
export const canDeliverPartial = (job: JobView) => !job.outputPath && !!job.workspace && ['failed', 'needsPassword', 'interrupted'].includes(job.status) && job.nodes.some(node => node.depth === 0 && node.status === 'completed') && job.nodes.some(node => node.depth > 0 && ['failed', 'needsPassword', 'interrupted'].includes(node.status));
export const verificationLabel = (scope: string) => scope.includes('targetReadback') ? '引擎完整性 / 目标文件回读' : scope.includes('sourceStreamSha256') ? '引擎完整性 / 写回复制源流摘要' : (({ targetReadback: '目标文件回读', sourceStream: '复制源流摘要', archiveIntegrity: '归档引擎完整性', standard: '标准流程验证', full: '完整目标回读' } as Record<string, string>)[scope] ?? (scope || '尚未完成'));
export function StatusTag({ status }: { status: string }) {
  const color = ['completed', 'cleanupPending'].includes(status) ? 'success' : ['failed', 'needsPassword'].includes(status) ? 'error' : ['running', 'stopping'].includes(status) ? 'processing' : ['interrupted', 'waitingResource', 'publishPending', 'partial'].includes(status) ? 'warning' : 'default';
  return <Tag color={color}>{statusLabels[status] ?? status}</Tag>;
}
export function JobDetails({ job, close, run, operationsDisabled = false }: { job?: JobView; close: () => void; run: (work: () => Promise<unknown>) => Promise<void>; operationsDisabled?: boolean }) {
  const { modal } = AntApp.useApp();
  const nodeTree = (parent: string | null): DataNode[] => (job?.nodes ?? []).filter(node => node.parentId === parent).map(node => ({
    key: node.id,
    title: <div className="node-detail"><Space wrap><strong>{node.name}</strong><StatusTag status={node.status} /><span className="muted">{stageLabels[node.stage] ?? node.stage} · {percent(node.progress)}%{node.passwordLabel ? ` · ${node.passwordLabel}` : ''}</span>{['failed', 'needsPassword', 'interrupted', 'waitingResource'].includes(node.status) && <Button size="small" disabled={!isDesktop || operationsDisabled || !job || !retryable(job)} onClick={event => { event.stopPropagation(); if (job) void run(() => api.retryNode(job.id, node.id)); }}>重试此节点</Button>}</Space><div className="muted">已尝试 {node.attempts} 次{node.message ? ` · ${node.message}` : ''}</div></div>,
    children: nodeTree(node.id),
  }));
  return <Drawer title={job?.source.displayName ?? '任务详情'} open={!!job} onClose={close} size={620}>
    {job && <>
      <div className="action-row"><StatusTag status={job.status} /><Typography.Text type="secondary">{stageLabels[job.stage] ?? job.stage} · 第 {job.attempts} 次尝试</Typography.Text></div>
      <Progress percent={percent(job.progress)} status={job.status === 'failed' ? 'exception' : undefined} />
      {job.message && <Alert type={retryable(job) ? 'warning' : 'info'} title={job.message} showIcon style={{ marginBottom: 20 }} />}
      <Space wrap style={{ marginBottom: 16 }}>
        {retryable(job) && <Button disabled={!isDesktop || operationsDisabled} onClick={() => void run(() => api.retry(job.id))}>按当前设置重试任务</Button>}
        {canDeliverPartial(job) && <Button disabled={!isDesktop || operationsDisabled} onClick={() => modal.confirm({ title: '交付部分结果？', content: '将交付已完成内容并保留未展开的内包；结果会明确标为部分交付，源包保留。你可以先补充密码并重试失败节点。', okText: '交付部分结果', cancelText: '返回', onOk: () => run(() => api.retry(job.id, true)) })}>交付部分结果</Button>}
        {job.status === 'publishPending' && <><Button disabled={!isDesktop || operationsDisabled} onClick={() => void run(() => api.recover(job.id, 'verify'))}>回读并核对发布</Button><Button disabled={!isDesktop || operationsDisabled} onClick={() => void run(() => api.recover(job.id, 'retry'))}>重试封存结果写回</Button></>}
        {job.status === 'cleanupPending' && <Button disabled={!isDesktop || operationsDisabled} onClick={() => void run(() => api.recover(job.id, 'cleanup'))}>重试清理缓存</Button>}
        {job.workspace && !job.outputPath && ['failed', 'needsPassword', 'interrupted', 'cancelled', 'waitingResource'].includes(job.status) && <Button danger disabled={!isDesktop || operationsDisabled} onClick={() => modal.confirm({ title: '清理本次临时结果？', content: '清理本应用创建的工作目录，原始压缩包保留。已展开内容将不再用于节点续接；后续需重新处理源包。', okText: '清理临时结果', cancelText: '保留以便重试', onOk: () => run(() => api.recover(job.id, 'cleanup')) })}>清理临时结果</Button>}
      </Space>
      <Descriptions column={1} bordered size="small" items={[
        { key: 'source', label: '原始归档', children: <Typography.Text copyable>{job.source.path}</Typography.Text> },
        { key: 'volumes', label: '分卷', children: job.source.volumes.length ? job.source.volumes.join('\n') : '单卷' },
        { key: 'output', label: '输出目录', children: job.outputPath ?? (job.options.outputRoot || '原始归档所在目录') },
        { key: 'cache', label: '工作目录', children: job.options.cacheEnabled ? `SSD 缓存 · ${job.options.cacheRoot || '未设置'}` : '目标卷临时目录' },
        { key: 'nested', label: '内包处理', children: job.options.expandNested ? `最多展开 ${job.options.maxDepth} 总层（包含外包）` : '保留内包' },
        { key: 'password', label: '密码范围', children: job.options.passwordGroup || '全部启用密码' },
        { key: 'verification', label: '已验证范围', children: verificationLabel(job.verificationScope) },
        { key: 'retention', label: '源文件策略', children: sourceCleanupLabels[job.options.sourceCleanup] || job.options.sourceCleanup },
        { key: 'rules', label: '输出排除', children: job.options.outputExclusions.length ? job.options.outputExclusions.join('、') : '未设置排除规则' },
      ]} />
      <Typography.Title level={5}>内外层处理节点</Typography.Title>
      {job.nodes.length ? <Tree treeData={nodeTree(null)} defaultExpandAll blockNode /> : <Typography.Text type="secondary">任务开始后显示各层处理状态。</Typography.Text>}
      {!!Object.keys(job.stageTimes).length && <><Typography.Title level={5}>阶段用时</Typography.Title><Descriptions size="small" column={2} items={Object.entries(job.stageTimes).map(([stage, duration]) => ({ key: stage, label: stageLabels[stage] ?? stage, children: `${(duration / 1000).toFixed(1)} 秒` }))} /></>}
      {isDesktop && job.outputPath && <Typography.Link onClick={() => void run(() => api.openPath(job.outputPath!))}>打开已发布结果</Typography.Link>}
      <Typography.Paragraph type="secondary" style={{ marginTop: 20 }}>标准验证不会重新读取所有目标文件；目标回读也不表示绕过系统缓存或证明断电后的持久性。</Typography.Paragraph>
    </>}
  </Drawer>;
}
