import { describe, expect, it } from 'vitest';
import { SnapshotCursor, validVersion } from '../src/snapshot-cursor';
import type { AppSnapshot, JobView, SnapshotDelta, SnapshotUpdate } from '../src/types';
import { defaultSettings } from '../src/types';

const snapshot = (sessionId: string, revision: string) => ({
  jobs: [],
  passwords: [],
  settings: { ...defaultSettings },
  running: false,
  paused: false,
  vaultError: null,
  dataDirectory: 'test-data',
  engineAvailable: true,
  recoverableCount: 0,
  cacheUsedBytes: 0,
  sessionId,
  revision,
}) as AppSnapshot;

const makeJob = (id: string, message = 'initial'): JobView => ({
  id,
  source: { path: `${id}.zip`, displayName: `${id}.zip`, volumes: [`${id}.zip`], size: 100, missingVolume: false },
  options: { ...defaultSettings },
  status: 'running',
  stage: 'extracting',
  progress: 0.1,
  message,
  attempts: 1,
  outputPath: null,
  verificationScope: 'streamSha256',
  createdAt: '2026-10-12T00:00:00Z',
  finishedAt: null,
  workspace: null,
  nodes: [],
  stageTimes: {},
  transferProgress: null,
});

describe('快照版本', () => {
  it('十进制大整数按完整精度比较，重复和旧版本被拒绝', () => {
    const cursor = new SnapshotCursor();
    expect(cursor.accept(snapshot('first', '9007199254740992'))).toBe(true);
    expect(cursor.accept(snapshot('first', '9007199254740993'))).toBe(true);
    expect(cursor.accept(snapshot('first', '9007199254740993'))).toBe(false);
    expect(cursor.accept(snapshot('first', '9007199254740992'))).toBe(false);
  });
  it('会话切换后旧会话的迟到快照不能回滚界面', () => {
    const cursor = new SnapshotCursor();
    expect(cursor.accept(snapshot('first', '100'))).toBe(true);
    expect(cursor.accept(snapshot('second', '1'))).toBe(true);
    expect(cursor.accept(snapshot('first', '101'))).toBe(false);
  });
  it('无效版本、负数和溢出不能作为快照', () => {
    for (const revision of ['', '-1', '01', '18446744073709551616']) {
      expect(validVersion({ sessionId: 'fixture', revision })).toBe(false);
    }
    expect(validVersion({ sessionId: '', revision: '1' })).toBe(false);
    expect(validVersion({ sessionId: 'fixture', revision: 1 })).toBe(false);
  });
});

describe('快照增量合成器 (SnapshotCursor.applyUpdate)', () => {
  it('首个 update 携带 full 快照正常初始化并返回完整快照', () => {
    const cursor = new SnapshotCursor();
    const full = snapshot('sess-1', '1');
    full.jobs = [makeJob('job-1')];

    const update: SnapshotUpdate = {
      sessionId: 'sess-1',
      revision: '1',
      full,
      delta: null,
    };
    const result = cursor.applyUpdate(update);
    expect(result).not.toBeNull();
    expect(result?.sessionId).toBe('sess-1');
    expect(result?.revision).toBe('1');
    expect(result?.jobs.length).toBe(1);
    expect(cursor.currentRevision).toBe('1');
    expect(cursor.currentSession).toBe('sess-1');
  });

  it('应用 delta 更新：修改已有任务、新增任务、删除任务、更新 settings 与 state', () => {
    const cursor = new SnapshotCursor();
    const full = snapshot('sess-1', '1');
    full.jobs = [makeJob('job-1', 'initial'), makeJob('job-2', 'to-be-removed')];
    cursor.applyUpdate({ sessionId: 'sess-1', revision: '1', full, delta: null });

    const delta: SnapshotDelta = {
      jobsChanged: [makeJob('job-1', 'updated-msg'), makeJob('job-3', 'new-job')],
      jobsRemoved: ['job-2'],
      passwordsChanged: [{ id: 'p-1', name: 'pwd', groups: [], notes: '', enabled: true, successCount: 0, lastSuccess: null, revision: 1 }],
      passwordsRemoved: [],
      settings: { ...defaultSettings, completionSound: true },
      state: {
        running: true,
        paused: true,
        vaultError: null,
        dataDirectory: 'test-data',
        engineAvailable: true,
        recoverableCount: 2,
        cacheUsedBytes: 1024,
      },
    };

    const update: SnapshotUpdate = {
      sessionId: 'sess-1',
      revision: '2',
      full: null,
      delta,
    };

    const synthesized = cursor.applyUpdate(update);
    expect(synthesized).not.toBeNull();
    expect(synthesized?.revision).toBe('2');
    expect(synthesized?.jobs.map(j => j.id)).toEqual(['job-1', 'job-3']);
    expect(synthesized?.jobs.find(j => j.id === 'job-1')?.message).toBe('updated-msg');
    expect(synthesized?.passwords.length).toBe(1);
    expect(synthesized?.settings.completionSound).toBe(true);
    expect(synthesized?.running).toBe(true);
    expect(synthesized?.paused).toBe(true);
    expect(synthesized?.recoverableCount).toBe(2);
    expect(synthesized?.cacheUsedBytes).toBe(1024);
  });

  it('无 base 快照或会话不匹配时 delta 无法合成，返回 null 以触发回退', () => {
    const cursor = new SnapshotCursor();
    const delta: SnapshotDelta = {
      jobsChanged: [],
      jobsRemoved: [],
      passwordsChanged: [],
      passwordsRemoved: [],
      settings: null,
      state: null,
    };

    // 1. 无 base 快照
    expect(cursor.applyUpdate({ sessionId: 'sess-1', revision: '2', full: null, delta })).toBeNull();

    // 初始化一个会话
    cursor.applyUpdate({ sessionId: 'sess-1', revision: '1', full: snapshot('sess-1', '1'), delta: null });

    // 2. 会话不匹配
    expect(cursor.applyUpdate({ sessionId: 'sess-2', revision: '2', full: null, delta })).toBeNull();

    // 3. 乱序旧版本
    expect(cursor.applyUpdate({ sessionId: 'sess-1', revision: '1', full: null, delta })).toBeNull();
  });
});
