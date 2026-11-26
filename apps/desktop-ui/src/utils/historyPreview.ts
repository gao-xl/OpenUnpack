import type { AppSnapshot, HistoryPageQuery, JobHistoryPage } from '../types';
import { terminal } from './format';

export function previewPage(snapshot: AppSnapshot, query: HistoryPageQuery): JobHistoryPage {
  const history = snapshot.jobs
    .filter(terminal)
    .sort((a, b) => b.createdAt.localeCompare(a.createdAt) || b.id.localeCompare(a.id))
    .slice(0, snapshot.settings.historyLimit);
  const search = query.search.trim().toLowerCase();
  const filtered = history.filter(
    (job) =>
      (query.status === 'all' || job.status === query.status) &&
      (!search ||
        job.source.displayName.toLowerCase().includes(search) ||
        job.source.path.toLowerCase().includes(search)),
  );
  const offset = Math.min(
    query.offset,
    Math.floor(Math.max(0, filtered.length - 1) / query.limit) * query.limit,
  );
  return {
    items: filtered.slice(offset, offset + query.limit),
    offset,
    limit: query.limit,
    filteredTotal: filtered.length,
    total: history.length,
    completed: history.filter((job) => job.status === 'completed').length,
    failed: history.filter((job) => job.status === 'failed').length,
    recoverable: history.filter((job) =>
      ['needsPassword', 'failed', 'cancelled', 'interrupted', 'publishPending'].includes(
        job.status,
      ),
    ).length,
    clearAllowed: false,
    sessionId: snapshot.sessionId,
    revision: snapshot.revision,
  };
}
