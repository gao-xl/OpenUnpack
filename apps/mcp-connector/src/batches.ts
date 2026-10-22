import { randomUUID } from 'node:crypto';
import { z } from 'zod';
import { ConnectorError, safeError, type Api } from './api.js';

export const PLAN_TTL_MS = 10 * 60_000;
const MAX_PLANS = 100;
const MAX_BATCH_SIZE = 1000;
const text = z.string().max(32768);
const sourceSchema = z.object({
  path: text, displayName: text, volumes: z.array(text).max(10000),
  size: z.number().nonnegative(), missingVolume: z.boolean(),
});
const scanSchema = z.object({
  sources: z.array(sourceSchema).max(10000),
  notices: z.array(z.object({ path: text, message: text })).max(10000),
});
const settingsSchema = z.object({
  outputRoot: text, passwordGroup: text, expandNested: z.boolean(), verification: text,
  maxDepth: z.number(), conflictPolicy: text, cacheEnabled: z.boolean(), cacheRoot: text,
  inputExclusions: z.array(text), outputExclusions: z.array(text),
  unwrapSingleRoot: z.boolean(), salvageCorrupted: z.boolean(),
}).passthrough();
const jobSchema = z.object({
  id: z.string().min(1).max(256), source: sourceSchema, status: text, stage: text,
  progress: z.number(), message: text, outputPath: text.nullable(),
  verificationScope: text,
  nodes: z.array(z.object({ id: text, name: text, status: text, stage: text, message: text })).max(100000),
});
type Source = z.infer<typeof sourceSchema>;
type Job = z.infer<typeof jobSchema>;
type Options = z.infer<typeof settingsSchema>;

export const planInput = z.object({
  paths: z.array(z.string().min(1).max(32768)).min(1).max(1000).describe('Approved archive files or input directories on this computer.'),
  outputRoot: z.string().min(1).max(32768).optional().describe('Approved output directory. Omit to use the current desktop setting; empty desktop output means beside each archive.'),
  passwordGroup: z.string().max(256).optional().describe('Existing password group name; never supply a password or API key.'),
  expandNested: z.boolean().optional(),
  verification: z.enum(['standard', 'strict']).optional(),
}).strict();
export const batchInput = z.object({
  jobIds: z.array(z.string().min(1).max(256)).min(1).max(MAX_BATCH_SIZE)
    .refine(ids => new Set(ids).size === ids.length, 'jobIds must be unique')
    .describe('Persistent job IDs returned by execute_extraction. Only these jobs are queried or controlled.'),
}).strict();
export const executeInput = z.object({ planId: z.string().uuid() }).strict();
export const inspectInput = z.object({
  archivePath: z.string().min(1).max(32768).describe('Path to the local archive file to inspect.'),
  password: z.string().max(4096).optional().describe('Optional password to decrypt archive headers or test readability.'),
}).strict();
export const retryWithPasswordInput = z.object({
  jobId: z.string().min(1).max(256).describe('The target job ID that requires a password or failed.'),
  password: z.string().min(1).max(4096).describe('The plaintext password to attempt for this specific job.'),
}).strict();

interface Plan {
  id: string;
  expiresAt: number;
  sources: Source[];
  options: Options;
  execution?: Promise<Record<string, unknown>>;
}

function parse<T>(schema: z.ZodType<T>, value: unknown): T {
  const result = schema.safeParse(value);
  if (!result.success) throw new ConnectorError('API_RESPONSE', 'OpenUnpack returned an incompatible response. Check the API version.', true);
  return result.data;
}

const startable = new Set(['queued', 'waitingResource']);
const retryable = new Set(['failed', 'needsPassword', 'cancelled', 'interrupted']);
const inProgress = new Set(['queued', 'waitingResource', 'running', 'paused', 'stopping']);
const failures = new Set(['failed', 'needsPassword', 'interrupted', 'publishPending', 'cleanupPending', 'partial', 'salvaged']);

export class Batches {
  private readonly plans = new Map<string, Plan>();
  constructor(private readonly api: Api, private readonly now = Date.now) {}

  private cleanPlans(): void {
    for (const [id, plan] of this.plans) if (plan.expiresAt <= this.now()) this.plans.delete(id);
    while (this.plans.size >= MAX_PLANS) this.plans.delete(this.plans.keys().next().value!);
  }

  async plan(input: z.infer<typeof planInput>): Promise<Record<string, unknown>> {
    // Both calls are read-only. Freeze the returned settings for this plan,
    // without changing the desktop defaults or exposing unrelated settings.
    const [settingsValue, scanValue] = await Promise.all([
      this.api.command('read_settings'),
      this.api.command('scan_sources', { paths: input.paths, requestId: `mcp-${randomUUID()}` }),
    ]);
    const settings = parse(settingsSchema, settingsValue);
    const scan = parse(scanSchema, scanValue);
    const options: Options = { ...settings, ...Object.fromEntries(
      Object.entries(input).filter(([key, value]) => key !== 'paths' && value !== undefined),
    ), sourceCleanup: 'keep', autoStartOnAdd: false, cleanEmptyDirs: false };
    const ready = scan.sources.filter(source => !source.missingVolume);
    const blocked = scan.sources.filter(source => source.missingVolume);
    if (ready.length > MAX_BATCH_SIZE) {
      throw new ConnectorError('BATCH_LIMIT', 'The scan found more than 1000 ready archives. Select a smaller input directory or a smaller set of files.');
    }
    this.cleanPlans();
    const id = randomUUID();
    const expiresAt = this.now() + PLAN_TTL_MS;
    if (ready.length) this.plans.set(id, { id, expiresAt, sources: ready, options });
    return {
      status: ready.length ? 'ready' : 'blocked',
      planId: ready.length ? id : null,
      expiresAt: ready.length ? new Date(expiresAt).toISOString() : null,
      ready: ready.map(source => ({ path: source.path, name: source.displayName, size: source.size, volumeCount: source.volumes.length })),
      blocked: blocked.map(source => ({ path: source.path, name: source.displayName, reason: 'missingVolume', action: 'Add the missing volumes beside the archive, then create a new plan.' })),
      notices: scan.notices.slice(0, 100), noticeCount: scan.notices.length,
      settings: {
        outputRoot: options.outputRoot || '(beside each archive)', passwordGroup: options.passwordGroup,
        expandNested: options.expandNested, maxDepth: options.maxDepth, verification: options.verification,
        conflictPolicy: options.conflictPolicy, cacheEnabled: options.cacheEnabled,
        ...(options.cacheEnabled ? { cacheRoot: options.cacheRoot || '(desktop managed cache)' } : {}),
        inputExclusions: options.inputExclusions, outputExclusions: options.outputExclusions,
        unwrapSingleRoot: options.unwrapSingleRoot, salvageCorrupted: options.salvageCorrupted,
        sourceCleanup: 'keep', autoStartOnAdd: false, cleanEmptyDirs: false,
      },
      nextAction: ready.length
        ? 'Show this plan to the user and obtain their confirmation before execute_extraction. Only ready archives will be submitted. Save the returned jobIds to track or resume the batch.'
        : 'No ready archives. Resolve the missing volumes or scan notices and create a new plan.',
    };
  }

  async execute(planId: string): Promise<Record<string, unknown>> {
    const plan = this.plans.get(planId);
    if (!plan || plan.expiresAt <= this.now()) {
      throw new ConnectorError('PLAN_EXPIRED', 'This plan expired or belongs to another connector session. Create a new plan. If it was previously submitted, query its saved jobIds or check the desktop queue first.');
    }
    // Assign the promise before yielding. Concurrent and repeated execute calls
    // share one submission, even if the add response is lost or malformed.
    plan.execution ??= this.submit(plan);
    return plan.execution;
  }

  private async submit(plan: Plan): Promise<Record<string, unknown>> {
    let jobs: Job[];
    try {
      jobs = parse(z.array(jobSchema).max(MAX_BATCH_SIZE), await this.api.command('add_jobs', {
        sources: plan.sources, options: plan.options,
      }));
      if (new Set(jobs.map(job => job.id)).size !== jobs.length) throw new ConnectorError('API_RESPONSE', 'The API returned duplicate job IDs.', true);
    } catch (error) {
      const failure = safeError(error);
      return {
        status: failure.outcomeUncertain ? 'submissionUnknown' : 'submissionRejected', planId: plan.id,
        jobIds: [], error: failure,
        nextAction: failure.outcomeUncertain
          ? 'Do not submit this plan again. Inspect the OpenUnpack desktop queue to identify accepted jobs before creating another plan. This plan is consumed.'
          : 'This plan is consumed. Correct the permission, paths, or queue state, then create a new plan.',
      };
    }
    const jobIds = jobs.map(job => job.id);
    if (!jobIds.length) {
      return { status: 'nothingAdded', planId: plan.id, jobIds,
        nextAction: 'OpenUnpack did not add new jobs; these archives may already have active tasks. Review the desktop queue. This consumed plan will not add or start existing jobs.' };
    }
    try {
      await this.api.command('start_selected_jobs', { jobIds });
      return { status: 'submitted', planId: plan.id, jobIds,
        nextAction: 'Save these jobIds and use get_batch_status. Closing the MCP connection does not cancel accepted desktop tasks.' };
    } catch (error) {
      return { status: 'submittedStartUnconfirmed', planId: plan.id, jobIds, error: safeError(error),
        nextAction: 'The jobs were added. Query these jobIds, then use start_batch to start only jobs still queued or waitingResource. Do not create duplicate jobs.' };
    }
  }

  private async jobs(jobIds: string[]): Promise<{ found: Job[]; missing: string[] }> {
    const found: Job[] = [];
    const missing: string[] = [];
    // Small sequential read sets avoid flooding the desktop API. We never use
    // queue_snapshot/history, so unrelated tasks are not read into the model.
    for (const jobId of jobIds) {
      const value = await this.api.command('job_details', { jobId });
      if (value === null) missing.push(jobId);
      else {
        const job = parse(jobSchema, value);
        if (job.id !== jobId) throw new ConnectorError('API_RESPONSE', 'The API returned an unexpected job ID.', true);
        found.push(job);
      }
    }
    return { found, missing };
  }

  private summary(found: Job[], missing: string[]): Record<string, unknown> {
    const completed = found.filter(job => job.status === 'completed');
    return {
      counts: { total: found.length + missing.length, completed: completed.length,
        inProgress: found.filter(job => inProgress.has(job.status)).length,
        needsAttention: found.filter(job => failures.has(job.status) || job.status === 'cancelled').length,
        missing: missing.length },
      jobs: found.map(job => {
        const nodes = job.nodes.filter(node => failures.has(node.status));
        return {
          jobId: job.id, source: job.source.path, name: job.source.displayName,
          status: job.status, stage: job.stage, progress: job.progress,
          message: job.message, outputPath: job.outputPath, verificationScope: job.verificationScope,
          failedNodes: nodes.slice(0, 20).map(node => ({ nodeId: node.id, name: node.name, status: node.status, stage: node.stage, message: node.message })),
          failedNodeCount: nodes.length,
          ...(job.status === 'needsPassword' ? { action: 'Add or enable the correct password in the selected group in OpenUnpack, then call retry_batch. Do not send password plaintext through MCP.' } : {}),
          ...(['publishPending', 'cleanupPending'].includes(job.status) ? { action: 'Open this task in the desktop recovery interface and review it before continuing.' } : {}),
        };
      }),
      missingJobIds: missing,
    };
  }

  async status(jobIds: string[]): Promise<Record<string, unknown>> {
    const { found, missing } = await this.jobs(jobIds);
    return this.summary(found, missing);
  }

  async start(jobIds: string[]): Promise<Record<string, unknown>> {
    const { found, missing } = await this.jobs(jobIds);
    if (missing.length) return { status: 'notStarted', missingJobIds: missing,
      nextAction: 'Some selected jobs no longer exist. Review the batch and provide the remaining jobIds explicitly.' };
    const selected = found.filter(job => startable.has(job.status)).map(job => job.id);
    if (!selected.length) return { status: 'nothingToStart', ...this.summary(found, missing) };
    try {
      await this.api.command('start_selected_jobs', { jobIds: selected });
      return { status: 'startRequested', jobIds, startedJobIds: selected,
        nextAction: 'Query get_batch_status for the current state.' };
    } catch (error) {
      return { status: 'startUnconfirmed', jobIds, error: safeError(error),
        nextAction: 'Query these jobIds first. If they are still queued, check whether the desktop queue is busy or paused, then use start_batch again.' };
    }
  }

  async cancel(jobIds: string[]): Promise<Record<string, unknown>> {
    const { found, missing } = await this.jobs(jobIds);
    const selected = found.filter(job => inProgress.has(job.status) || job.status === 'needsPassword');
    const results = [];
    for (const job of selected) {
      try { await this.api.command('cancel_jobs', { jobId: job.id }); results.push({ jobId: job.id, status: 'cancelRequested' }); }
      catch (error) { results.push({ jobId: job.id, status: 'cancelUnconfirmed', error: safeError(error) }); }
    }
    return { jobIds, results, missingJobIds: missing,
      skippedJobIds: found.filter(job => !selected.includes(job)).map(job => job.id),
      nextAction: 'Query get_batch_status until running tasks stop. Existing output and original archives are retained.' };
  }

  async retry(jobIds: string[]): Promise<Record<string, unknown>> {
    const { found, missing } = await this.jobs(jobIds);
    const results = [];
    for (const job of found.filter(job => retryable.has(job.status))) {
      try {
        await this.api.command('retry_job', { jobId: job.id, partial: false, preserveOptions: true });
        results.push({ jobId: job.id, status: 'retryRequested' });
      } catch (error) { results.push({ jobId: job.id, status: 'retryUnconfirmed', error: safeError(error) }); }
    }
    return { jobIds, results, missingJobIds: missing,
      skippedJobIds: found.filter(job => !retryable.has(job.status)).map(job => job.id),
      nextAction: 'Query get_batch_status. Retried jobs use their saved extraction options; password changes must be made in the desktop password library.' };
  }

  async inspect(archivePath: string, password?: string): Promise<Record<string, unknown>> {
    const inspectId = randomUUID();
    const result = await this.api.command('inspect_archive', {
      path: archivePath,
      password: password ?? null,
      inspectId,
    });
    return (result && typeof result === 'object') ? (result as Record<string, unknown>) : { result };
  }

  async retryWithPassword(jobId: string, password: string): Promise<Record<string, unknown>> {
    const { found, missing } = await this.jobs([jobId]);
    if (missing.length) {
      throw new ConnectorError('JOB_NOT_FOUND', `Job ${jobId} does not exist in the current queue.`);
    }
    const job = found[0];
    if (!job) {
      throw new ConnectorError('JOB_NOT_FOUND', `Job ${jobId} does not exist in the current queue.`);
    }
    try {
      await this.api.command('retry_job', {
        jobId: job.id,
        partial: false,
        password,
        preserveOptions: true,
      });
      return { status: 'retryRequested', jobId: job.id, action: 'Retry started with the provided password.' };
    } catch (error) {
      return { status: 'retryFailed', jobId: job.id, error: safeError(error) };
    }
  }
}
