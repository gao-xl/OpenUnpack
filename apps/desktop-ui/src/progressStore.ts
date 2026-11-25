import { useSyncExternalStore, useMemo } from 'react';
import { api, type JobProgressUpdatePayload } from './api';

// Map of jobId -> latest progress payload
const progressStore = new Map<string, JobProgressUpdatePayload>();
const listenersByJob = new Map<string, Set<() => void>>();
const globalListeners = new Set<() => void>();

// `useSyncExternalStore` only re-renders when the snapshot identity changes.
// The Map is mutated in place, so the aggregate hook must expose a new wrapper
// object on every mutation; otherwise subscribers keep the stale reference and
// the memoized batch statistics never recompute.
let aggregateSnapshot: { entries: ReadonlyMap<string, JobProgressUpdatePayload> } = {
  entries: progressStore,
};

let initialized = false;

function refreshAggregateSnapshot() {
  aggregateSnapshot = { entries: progressStore };
}

export function resetProgressStore() {
  initialized = false;
  progressStore.clear();
  listenersByJob.clear();
  globalListeners.clear();
  refreshAggregateSnapshot();
}

export function initProgressStore() {
  if (initialized) return;
  initialized = true;

  if (typeof api?.onProgress === 'function') {
    api.onProgress((update: JobProgressUpdatePayload) => {
      if (!update || !update.jobId) return;

      progressStore.set(update.jobId, update);
      refreshAggregateSnapshot();

      const jobListeners = listenersByJob.get(update.jobId);
      if (jobListeners) {
        jobListeners.forEach((listener) => listener());
      }
      globalListeners.forEach((listener) => listener());
    });
  }
}

// Auto-initialize when running in browser/webview environment
if (typeof window !== 'undefined') {
  initProgressStore();
}

/**
 * Hook to subscribe to real-time progress for a specific job using React's useSyncExternalStore.
 * Ensures only the component rendering this specific job re-renders on progress ticks.
 */
export function useJobProgress(jobId: string): JobProgressUpdatePayload | undefined {
  return useSyncExternalStore(
    (onStoreChange) => {
      let set = listenersByJob.get(jobId);
      if (!set) {
        set = new Set();
        listenersByJob.set(jobId, set);
      }
      set.add(onStoreChange);
      return () => {
        set?.delete(onStoreChange);
        if (set?.size === 0) {
          listenersByJob.delete(jobId);
        }
      };
    },
    () => progressStore.get(jobId),
    () => undefined,
  );
}

/**
 * Selector hook for fine-grained subscriptions with custom projection.
 */
export function useJobProgressSelector<T>(
  jobId: string,
  selector: (update: JobProgressUpdatePayload | undefined) => T,
): T {
  const progress = useJobProgress(jobId);
  return useMemo(() => selector(progress), [progress, selector]);
}

/**
 * Hook to subscribe to aggregate progress ticks across running jobs.
 * Returns an immutable snapshot wrapper that changes identity on every update.
 */
export function useAggregateProgress() {
  return useSyncExternalStore(
    (onStoreChange) => {
      globalListeners.add(onStoreChange);
      return () => {
        globalListeners.delete(onStoreChange);
      };
    },
    () => aggregateSnapshot,
    () => aggregateSnapshot,
  );
}

/**
 * Helper to clear progress for a job (e.g. when completed or removed).
 */
export function clearJobProgress(jobId: string) {
  progressStore.delete(jobId);
  refreshAggregateSnapshot();
  const jobListeners = listenersByJob.get(jobId);
  if (jobListeners) {
    jobListeners.forEach((listener) => listener());
  }
}
