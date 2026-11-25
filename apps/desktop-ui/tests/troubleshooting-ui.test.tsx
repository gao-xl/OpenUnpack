import { render, screen } from '@testing-library/react';
import { App as AntApp, ConfigProvider } from 'antd';
import { beforeEach, expect, it, vi } from 'vitest';
import QueuePage from '../src/components/QueuePage';
import { JobDetails } from '../src/components/shared';
import type { AppSnapshot, JobView } from '../src/types';
import { defaultSettings } from '../src/types';

const bridge = vi.hoisted(() => ({
  onDrop: vi.fn().mockResolvedValue(() => {}),
  cancelScan: vi.fn(),
  retry: vi.fn(),
  retryNode: vi.fn(),
  recover: vi.fn(),
  openPath: vi.fn(),
}));

vi.mock('../src/api', () => ({
  api: bridge,
  isDesktop: true,
}));

const emptyRun = async (work: () => Promise<unknown>) => {
  await work();
};

const createJob = (overrides: Partial<JobView> = {}): JobView => ({
  id: 'job-1',
  source: {
    path: 'C:\\archives\\archive.zip',
    displayName: 'archive.zip',
    volumes: [],
    size: 1024 * 1024,
    missingVolume: false,
  },
  options: { ...defaultSettings },
  status: 'failed',
  stage: 'extracting',
  progress: 0.5,
  message: '密码不匹配或压缩包损坏，解压失败。',
  attempts: 1,
  outputPath: null,
  verificationScope: 'standard',
  createdAt: '2026-10-23T10:00:00Z',
  finishedAt: '2026-10-23T10:01:00Z',
  workspace: 'C:\\temp\\workspace\\job-1',
  nodes: [],
  stageTimes: {},
  transferProgress: null,
  ...overrides,
});

beforeEach(() => {
  vi.resetAllMocks();
  bridge.onDrop.mockResolvedValue(() => {});
});

it('JobDetails 呈现归档损坏与 CRC 校验失败的详细排障指引卡片', async () => {
  const job = createJob({
    status: 'failed',
    message: '密码不匹配或压缩包损坏，解压失败。',
  });

  render(
    <ConfigProvider button={{ autoInsertSpace: false }} theme={{ token: { motion: false } }}>
      <AntApp>
        <JobDetails job={job} close={() => {}} run={emptyRun} operationsDisabled={false} />
      </AntApp>
    </ConfigProvider>,
  );

  // 验证排障卡片内容
  expect(screen.getByText('排障与操作指引：')).toBeTruthy();
  expect(screen.getByText('密码不匹配或压缩包损坏')).toBeTruthy();
  expect(screen.getByText(/若该文件后缀曾被伪装/)).toBeTruthy();
  expect(screen.getByText(/允许抢救已损坏归档/)).toBeTruthy();
});

it('JobDetails 针对密码缺失呈现清晰录入与密码库导入提示', async () => {
  const job = createJob({
    status: 'needsPassword',
    message: '已保存密码未能打开压缩包，请补充密码后重试；损坏也可能导致此状态。',
  });

  render(
    <ConfigProvider button={{ autoInsertSpace: false }} theme={{ token: { motion: false } }}>
      <AntApp>
        <JobDetails job={job} close={() => {}} run={emptyRun} operationsDisabled={false} />
      </AntApp>
    </ConfigProvider>,
  );

  expect(screen.getByText('需要解压密码或已保存口令未命中')).toBeTruthy();
  expect(screen.getByText(/点击「输入密码」录入正确口令/)).toBeTruthy();
  expect(screen.getByText(/建议在「密码库」中添加密码或批量导入/)).toBeTruthy();
});

it('QueuePage 表格视图直观呈现精细分类标签与故障标题', async () => {
  const failedJob = createJob({
    id: 'job-corrupt',
    status: 'failed',
    message: '压缩包缺少分卷，请补齐后重试。',
  });

  const snapshot: AppSnapshot = {
    jobs: [failedJob],
    passwords: [],
    settings: { ...defaultSettings },
    running: false,
    paused: false,
    vaultError: null,
    dataDirectory: '',
    engineAvailable: true,
    recoverableCount: 0,
    cacheUsedBytes: 0,
    sessionId: 'fixture',
    revision: '1',
  };

  render(
    <ConfigProvider button={{ autoInsertSpace: false }} theme={{ token: { motion: false } }}>
      <AntApp>
        <QueuePage snapshot={snapshot} run={emptyRun} refresh={async () => {}} />
      </AntApp>
    </ConfigProvider>,
  );

  expect(screen.getByText('缺少分卷')).toBeTruthy();
  expect(screen.getByText('分卷压缩包缺少关联卷')).toBeTruthy();
});
