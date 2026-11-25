import { renderHook, act } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { api, type JobProgressUpdatePayload } from '../src/api';
import {
  useJobProgress,
  useJobProgressSelector,
  useAggregateProgress,
  clearJobProgress,
  initProgressStore,
  resetProgressStore,
} from '../src/progressStore';

describe('progressStore fine-grained selector & granular subscriptions', () => {
  beforeEach(() => {
    resetProgressStore();
  });

  it('triggers only the subscriber for the updated job, avoiding whole-queue re-renders', () => {
    let progressCallback: ((update: JobProgressUpdatePayload) => void) | undefined;
    vi.spyOn(api, 'onProgress').mockImplementation((cb) => {
      progressCallback = cb;
      return () => false;
    });

    // Re-init to attach spy
    initProgressStore();

    // Hook 1: job-1
    const { result: job1Result } = renderHook(() => useJobProgress('job-1'));
    // Hook 2: job-2
    const { result: job2Result } = renderHook(() => useJobProgress('job-2'));

    expect(job1Result.current).toBeUndefined();
    expect(job2Result.current).toBeUndefined();

    // Dispatch update for job-1
    act(() => {
      progressCallback?.({
        jobId: 'job-1',
        stage: 'extracting',
        progress: 0.42,
        completedBytes: 4200,
        totalBytes: 10000,
        bytesPerSecond: 1024 * 1024,
      });
    });

    // job-1 receives new values
    expect(job1Result.current).toBeDefined();
    expect(job1Result.current?.progress).toBe(0.42);
    expect(job1Result.current?.completedBytes).toBe(4200);

    // job-2 remains untouched (zero re-render / undefined)
    expect(job2Result.current).toBeUndefined();

    // Dispatch update for job-2
    act(() => {
      progressCallback?.({
        jobId: 'job-2',
        stage: 'copying',
        progress: 0.88,
        completedBytes: 8800,
        totalBytes: 10000,
      });
    });

    expect(job2Result.current).toBeDefined();
    expect(job2Result.current?.progress).toBe(0.88);
    // job-1 still keeps its state
    expect(job1Result.current?.progress).toBe(0.42);

    // Clear job-1
    act(() => {
      clearJobProgress('job-1');
    });
    expect(job1Result.current).toBeUndefined();
  });

  it('supports custom selector projections via useJobProgressSelector', () => {
    let progressCallback: ((update: JobProgressUpdatePayload) => void) | undefined;
    vi.spyOn(api, 'onProgress').mockImplementation((cb) => {
      progressCallback = cb;
      return () => false;
    });

    initProgressStore();

    const { result } = renderHook(() =>
      useJobProgressSelector('job-select', (p) => (p?.progress ? Math.round(p.progress * 100) : 0)),
    );

    expect(result.current).toBe(0);

    act(() => {
      progressCallback?.({
        jobId: 'job-select',
        progress: 0.654,
      });
    });

    expect(result.current).toBe(65);
  });

  it('aggregate progress hook tracks all active jobs for batch progress bars', () => {
    let progressCallback: ((update: JobProgressUpdatePayload) => void) | undefined;
    vi.spyOn(api, 'onProgress').mockImplementation((cb) => {
      progressCallback = cb;
      return () => false;
    });

    initProgressStore();

    const { result } = renderHook(() => useAggregateProgress());

    act(() => {
      progressCallback?.({
        jobId: 'job-agg-1',
        completedBytes: 500,
        totalBytes: 1000,
      });
    });

    expect(result.current.entries.get('job-agg-1')?.completedBytes).toBe(500);
  });

  it('aggregate snapshot identity changes on every update so memoized statistics recompute', () => {
    let progressCallback: ((update: JobProgressUpdatePayload) => void) | undefined;
    vi.spyOn(api, 'onProgress').mockImplementation((cb) => {
      progressCallback = cb;
      return () => false;
    });

    initProgressStore();

    const { result } = renderHook(() => useAggregateProgress());
    const before = result.current;

    act(() => {
      progressCallback?.({ jobId: 'job-agg-2', completedBytes: 250 });
    });

    const after = result.current;
    expect(after).not.toBe(before);
    expect(after.entries.get('job-agg-2')?.completedBytes).toBe(250);
  });
});
