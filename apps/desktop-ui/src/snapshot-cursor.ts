import type { AppSnapshot, SnapshotDelta, SnapshotUpdate, StateInvalidated } from './types';

export function validVersion(value: unknown): value is StateInvalidated {
  if (!value || typeof value !== 'object') return false;
  const version = value as StateInvalidated;
  return typeof version.sessionId === 'string' && version.sessionId.length > 0 &&
    typeof version.revision === 'string' && /^(0|[1-9]\d*)$/.test(version.revision) &&
    BigInt(version.revision) <= 18446744073709551615n;
}

export function applySnapshotDelta(base: AppSnapshot, delta: SnapshotDelta, sessionId: string, revision: string): AppSnapshot {
  const removedJobs = new Set(delta.jobsRemoved);
  const changedJobsMap = new Map(delta.jobsChanged.map(j => [j.id, j]));
  const updatedJobs = base.jobs
    .filter(j => !removedJobs.has(j.id))
    .map(j => changedJobsMap.get(j.id) ?? j);
  for (const changed of delta.jobsChanged) {
    if (!base.jobs.some(j => j.id === changed.id)) {
      updatedJobs.push(changed);
    }
  }

  const removedPwds = new Set(delta.passwordsRemoved);
  const changedPwdsMap = new Map(delta.passwordsChanged.map(p => [p.id, p]));
  const updatedPasswords = base.passwords
    .filter(p => !removedPwds.has(p.id))
    .map(p => changedPwdsMap.get(p.id) ?? p);
  for (const changed of delta.passwordsChanged) {
    if (!base.passwords.some(p => p.id === changed.id)) {
      updatedPasswords.push(changed);
    }
  }

  const settings = delta.settings ? { ...delta.settings } : base.settings;
  const state = delta.state;

  return {
    jobs: updatedJobs,
    passwords: updatedPasswords,
    settings,
    running: state ? state.running : base.running,
    paused: state ? state.paused : base.paused,
    vaultError: state ? state.vaultError : base.vaultError,
    dataDirectory: state ? state.dataDirectory : base.dataDirectory,
    engineAvailable: state ? state.engineAvailable : base.engineAvailable,
    recoverableCount: state ? state.recoverableCount : base.recoverableCount,
    cacheUsedBytes: state ? state.cacheUsedBytes : base.cacheUsedBytes,
    sessionId,
    revision,
  };
}

export class SnapshotCursor {
  private session = '';
  private revision = -1n;
  private retired = new Set<string>();
  private lastSnapshot: AppSnapshot | null = null;

  get currentRevision(): string | null {
    return this.revision >= 0n ? this.revision.toString() : null;
  }

  get currentSession(): string | null {
    return this.session || null;
  }

  get last(): AppSnapshot | null {
    return this.lastSnapshot;
  }

  accept(next: AppSnapshot): boolean {
    if (!validVersion(next) || this.retired.has(next.sessionId)) return false;
    const revision = BigInt(next.revision);
    if (next.sessionId === this.session && revision <= this.revision) return false;
    if (this.session && next.sessionId !== this.session) this.retired.add(this.session);
    this.session = next.sessionId;
    this.revision = revision;
    this.lastSnapshot = next;
    return true;
  }

  applyUpdate(update: SnapshotUpdate): AppSnapshot | null {
    if (!update || !validVersion(update)) return null;

    if (update.full) {
      if (this.accept(update.full)) {
        return this.lastSnapshot;
      }
      return null;
    }

    if (update.delta) {
      if (!this.lastSnapshot || this.session !== update.sessionId) {
        return null;
      }
      const synthesized = applySnapshotDelta(this.lastSnapshot, update.delta, update.sessionId, update.revision);
      if (this.accept(synthesized)) {
        return this.lastSnapshot;
      }
      return null;
    }

    return null;
  }
}
