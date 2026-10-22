import type { HistoryPageQuery, JobHistoryPage, JobView } from '../src/types';

export function historyPageResult(jobs: JobView[], query: HistoryPageQuery = { offset: 0, limit: 10, status: 'all', search: '' }): JobHistoryPage {
  const search = query.search.trim().toLowerCase();
  const filtered = jobs.filter(job => (query.status === 'all' || job.status === query.status) && (!search || (job.source.displayName + ' ' + job.source.path).toLowerCase().includes(search)));
  const offset = Math.min(query.offset, Math.floor(Math.max(0, filtered.length - 1) / query.limit) * query.limit);
  return { items: filtered.slice(offset, offset + query.limit), offset, limit: query.limit, filteredTotal: filtered.length, total: jobs.length,
    completed: jobs.filter(job => job.status === 'completed').length, failed: jobs.filter(job => job.status === 'failed').length,
    recoverable: jobs.filter(job => ['needsPassword', 'failed', 'cancelled', 'interrupted', 'publishPending'].includes(job.status)).length,
    clearAllowed: !jobs.some(job => job.workspace || job.status === 'publishPending'), sessionId: 'fixture', revision: '1' };
}
