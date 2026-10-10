import React, { useState, useEffect, useMemo } from 'react';
import { App as AntApp, Table, Button, Space, Tag, Input, Select, Popconfirm, Card, Statistic, Row, Col } from 'antd';
import { ReloadOutlined, DeleteOutlined, FolderOpenOutlined, EyeOutlined } from '@ant-design/icons';
import { api, isDesktop } from '../api';
import type { AppSnapshot, JobView } from '../types';
import { statusLabels } from '../types';
import { JobDetails, verificationLabel } from './shared';

const scopeLabels: Record<string, string> = {
  standard: '标准流程验证',
  targetReadback: '目标回读',
  sourceStream: '来源流校验(非目标回读)',
  stream: '来源流校验(非目标回读)',
  streamhash: '来源流校验(非目标回读)',
};
const terminalStatuses = new Set(['completed', 'failed', 'needsPassword', 'cancelled', 'partial', 'interrupted', 'publishPending', 'cleanupPending', 'skipped']);

export default function HistoryPage({ snapshot, refresh, run }: { snapshot: AppSnapshot; refresh: () => Promise<void>; run: (work: () => Promise<unknown>) => Promise<void> }) {
  const { message } = AntApp.useApp();
  const [historyList, setHistoryList] = useState<JobView[]>([]);
  const [search, setSearch] = useState('');
  const [statusFilter, setStatusFilter] = useState('all');
  const [selectedJob, setSelectedJob] = useState<JobView | null>(null);

  const loadHistory = async () => {
    if (isDesktop) {
      const list = await api.history();
      setHistoryList(list || []);
    } else {
      setHistoryList(snapshot.jobs.filter((j) => terminalStatuses.has(j.status)));
    }
  };

  const stateKey = snapshot.jobs.map(job => `${job.id}:${job.status}:${job.finishedAt ?? ''}`).join('|');
  useEffect(() => { void run(loadHistory); }, [stateKey]);

  const counts = useMemo(() => {
    let completed = 0, failed = 0, recoverable = 0;
    for (const j of historyList) {
      if (j.status === 'completed') completed++;
      if (j.status === 'failed') failed++;
      if (['needsPassword', 'failed', 'cancelled', 'interrupted', 'publishPending'].includes(j.status)) recoverable++;
    }
    return { completed, failed, recoverable, total: historyList.length };
  }, [historyList]);

  const filtered = useMemo(() => historyList.filter((j) => {
    const matchStatus = statusFilter === 'all' || j.status === statusFilter;
    const matchSearch = !search || j.source.displayName.toLowerCase().includes(search.toLowerCase()) || j.source.path.toLowerCase().includes(search.toLowerCase());
    return matchStatus && matchSearch;
  }), [historyList, statusFilter, search]);

  const disableClear = !isDesktop || snapshot.running || historyList.some((j) => j.status === 'publishPending' || !!j.workspace);

  const columns = [
    {
      title: '来源名称与路径',
      key: 'source',
      render: (_: unknown, r: JobView) => (
        <div>
          <div style={{ fontWeight: 600 }}>{r.source.displayName}</div>
          <div style={{ fontSize: 12, color: '#888', wordBreak: 'break-all' }}>{r.source.path}</div>
        </div>
      ),
    },
    {
      title: '状态',
      key: 'status',
      width: 130,
      render: (_: unknown, r: JobView) => {
        const color = r.status === 'completed' ? 'success' : r.status === 'failed' ? 'error' : r.status === 'publishPending' ? 'warning' : 'default';
        return <Tag color={color}>{statusLabels[r.status] || r.status}</Tag>;
      },
    },
    {
      title: '校验方式',
      key: 'verification',
      width: 170,
      render: (_: unknown, r: JobView) => <Tag>{scopeLabels[r.verificationScope] || verificationLabel(r.verificationScope)}</Tag>,
    },
    {
      title: '完成时间',
      key: 'finishedAt',
      width: 180,
      render: (_: unknown, r: JobView) => r.finishedAt || r.createdAt || '-',
    },
    {
      title: '操作',
      key: 'action',
      width: 260,
      render: (_: unknown, r: JobView) => (
        <Space size="small" wrap>
          <Button size="small" icon={<EyeOutlined />} onClick={() => setSelectedJob(r)}>详情</Button>
          {r.outputPath && (
            <Button size="small" icon={<FolderOpenOutlined />} disabled={!isDesktop} onClick={() => void run(() => api.openPath(r.outputPath!))}>打开</Button>
          )}
          {r.status === 'publishPending' ? (
            <Space wrap><Button size="small" type="primary" ghost disabled={!isDesktop || snapshot.running} onClick={() => void run(async () => { await api.recover(r.id, 'verify'); await loadHistory(); })}>回读并核对</Button><Button size="small" disabled={!isDesktop || snapshot.running} onClick={() => void run(async () => { await api.recover(r.id, 'retry'); await loadHistory(); })}>重试写回</Button></Space>
          ) : ['failed', 'cancelled', 'needsPassword', 'interrupted'].includes(r.status) ? (
            <Button size="small" disabled={!isDesktop || snapshot.running} onClick={() => void run(async () => { await api.retry(r.id); await loadHistory(); })}>重试</Button>
          ) : null}
          {r.workspace && ['failed', 'needsPassword', 'interrupted', 'cancelled', 'cleanupPending'].includes(r.status) && <Popconfirm title="清理保留的临时结果？" description="仅清理本应用标记的工作目录与临时目录，已发布结果与原始压缩包不会因此删除。" onConfirm={() => void run(async () => { await api.recover(r.id, 'cleanup'); await loadHistory(); })}><Button disabled={!isDesktop || snapshot.running} size="small">清理临时目录</Button></Popconfirm>}
          <Popconfirm title="移除这条记录？" description="仅移除记录，已发布结果保留。" disabled={!isDesktop || r.status === 'publishPending' || !!r.workspace} onConfirm={() => void run(async () => { await api.remove(r.id); await loadHistory(); })}><Button size="small" danger disabled={!isDesktop || r.status === 'publishPending' || !!r.workspace}>移除记录</Button></Popconfirm>
        </Space>
      ),
    },
  ];

  return (
    <div style={{ padding: 16, display: 'flex', flexDirection: 'column', gap: 16 }}>
      <div className="page-header"><h2 className="page-title">处理记录</h2><p className="page-description">查看处理结果、验证范围与中断原因，按最后记录的状态继续处理。</p></div>
      <Row gutter={16}>
        <Col span={6}><Card size="small"><Statistic title="历史任务总数" value={counts.total} /></Card></Col>
        <Col span={6}><Card size="small"><Statistic title="已完成" value={counts.completed} styles={{ content: { color: '#3f8600' } }} /></Card></Col>
        <Col span={6}><Card size="small"><Statistic title="处理失败" value={counts.failed} styles={{ content: { color: '#cf1322' } }} /></Card></Col>
        <Col span={6}><Card size="small"><Statistic title="可恢复/待核对" value={counts.recoverable} styles={{ content: { color: '#d48806' } }} /></Card></Col>
      </Row>

      <Card size="small">
        <Space wrap style={{ width: '100%', justifyContent: 'space-between', marginBottom: 12 }}>
          <Space wrap>
            <Input.Search placeholder="搜索文件路径或名称" allowClear style={{ width: 240 }} value={search} onChange={(e) => setSearch(e.target.value)} />
            <Select style={{ width: 150 }} value={statusFilter} onChange={setStatusFilter} options={[
              { label: '全部状态', value: 'all' },
              ...Object.entries(statusLabels).map(([k, v]) => ({ label: v, value: k })),
            ]} />
          </Space>
          <Space wrap>
            <Button icon={<ReloadOutlined />} onClick={() => void run(loadHistory)}>刷新记录</Button>
            <Popconfirm title="清理历史记录" description="仅清除历史任务记录，已提取/生成的文件保留，确定清空吗？" disabled={disableClear} onConfirm={() => void run(async () => { await api.clearHistory(); await loadHistory(); await refresh(); message.success('历史记录已清空'); })}>
              <Button danger icon={<DeleteOutlined />} disabled={disableClear}>清空历史记录</Button>
            </Popconfirm>
          </Space>
        </Space>

        <Table rowKey="id" columns={columns} dataSource={filtered} size="small" scroll={{ x: 1100 }} pagination={{ pageSize: 10, showSizeChanger: true }} locale={{ emptyText: '暂无历史记录' }} />
      </Card>

      <JobDetails job={historyList.find(job => job.id === selectedJob?.id)} close={() => setSelectedJob(null)} run={run} operationsDisabled={snapshot.running} />
    </div>
  );
}
