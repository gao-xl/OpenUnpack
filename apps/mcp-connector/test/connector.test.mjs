import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { once } from 'node:events';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import test from 'node:test';
import { Client } from '@modelcontextprotocol/client';
import { StdioClientTransport } from '@modelcontextprotocol/client/stdio';
import { Batches, PLAN_TTL_MS } from '../dist/batches.js';
import { validateApiUrl } from '../dist/api.js';

const cwd = fileURLToPath(new URL('..', import.meta.url));
const entry = fileURLToPath(new URL('../dist/index.js', import.meta.url));
const token = 'test-only-application-key-never-print';
const secret = 'test-only-password-never-print';
const source = (name = 'sample.zip', missingVolume = false) => ({
  path: `C:\\Input\\${name}`, displayName: name, volumes: [`C:\\Input\\${name}`], size: 100, missingVolume,
});
const settings = () => ({
  outputRoot: 'C:\\Output', passwordGroup: '素材', expandNested: true, verification: 'standard',
  maxDepth: 2, conflictPolicy: 'rename', cacheEnabled: false, cacheRoot: '', inputExclusions: [],
  outputExclusions: [], unwrapSingleRoot: false, salvageCorrupted: true,
  sourceCleanup: 'delete', cleanEmptyDirs: true, autoStartOnAdd: true,
  unrelatedDesktopSetting: 'private-value',
});
const job = (id, status = 'queued', archive = source()) => ({
  id, source: archive, options: settings(), status, stage: 'queued', progress: 0,
  message: '', outputPath: null, verificationScope: '', passwordLabel: secret, nodes: [],
});
function send(response, status, value) {
  response.writeHead(status, { 'Content-Type': 'application/json' });
  response.end(JSON.stringify(value));
}
async function fixture(t, override, env = {}) {
  const state = { calls: [], jobs: new Map([['unrelated', job('unrelated')]]), sources: [source()],
    notices: [], settings: settings(), addCount: 0 };
  const http = createServer(async (request, response) => {
    const chunks = [];
    for await (const chunk of request) chunks.push(chunk);
    try {
      const body = JSON.parse(Buffer.concat(chunks).toString('utf8'));
      assert.equal(request.url, '/api/v1/commands');
      assert.equal(request.headers.authorization, `Bearer ${token}`);
      state.calls.push(body);
      if (override && await override(body, response, state)) return;
      switch (body.command) {
        case 'read_settings': return send(response, 200, { data: state.settings });
        case 'scan_sources': return send(response, 200, { data: { sources: state.sources, notices: state.notices } });
        case 'add_jobs': {
          state.addCount++;
          const jobs = body.args.sources.map((archive, index) => ({ ...job(`job-${state.addCount}-${index}`, 'queued', archive), options: body.args.options }));
          for (const value of jobs) state.jobs.set(value.id, value);
          return send(response, 200, { data: jobs });
        }
        case 'start_selected_jobs': {
          for (const id of body.args.jobIds) state.jobs.get(id).status = 'running';
          return send(response, 200, { data: null });
        }
        case 'job_details': return send(response, 200, { data: state.jobs.get(body.args.jobId) ?? null });
        case 'cancel_jobs': {
          assert.equal(typeof body.args.jobId, 'string');
          state.jobs.get(body.args.jobId).status = 'cancelled';
          return send(response, 200, { data: null });
        }
        case 'retry_job': {
          state.jobs.get(body.args.jobId).status = 'running';
          return send(response, 200, { data: null });
        }
        default: throw new Error(`Unexpected command ${body.command}`);
      }
    } catch {
      send(response, 500, { error: { message: 'mock failure' } });
    }
  });
  http.listen(0, '127.0.0.1');
  await once(http, 'listening');
  const url = `http://127.0.0.1:${http.address().port}`;
  const transport = new StdioClientTransport({
    command: process.execPath, args: [entry], cwd, stderr: 'pipe',
    env: { ...process.env, OPENUNPACK_API_TOKEN: token, OPENUNPACK_API_URL: url, ...env },
  });
  let stderr = '';
  transport.stderr?.on('data', chunk => { stderr += chunk.toString(); });
  const client = new Client({ name: 'openunpack-protocol-tests', version: '1.0.0' });
  t.after(async () => {
    await client.close();
    http.closeAllConnections();
    await new Promise(resolve => http.close(resolve));
    assert.ok(!stderr.includes(token));
    assert.ok(!stderr.includes(secret));
  });
  await client.connect(transport);
  const call = async (name, args) => {
    const result = await client.callTool({ name, arguments: args });
    assert.ok(!JSON.stringify(result).includes(token), 'tool result must not contain the API key');
    assert.ok(!JSON.stringify(result).includes(secret), 'tool result must not contain password metadata');
    assert.equal(result.content[0].type, 'text');
    assert.deepEqual(JSON.parse(result.content[0].text), result.structuredContent);
    return { ...result.structuredContent, isError: result.isError ?? false };
  };
  return { state, call, client, transport };
}

test('real stdio initialize, tools/list, plan, execute and status preserve scope and frozen options', async t => {
  const { state, client, call } = await fixture(t);
  assert.equal(client.getServerVersion().name, 'openunpack');
  const { tools } = await client.listTools();
  assert.deepEqual(tools.map(tool => tool.name).sort(), [
    'cancel_batch', 'execute_extraction', 'get_batch_status', 'inspect_archive_content', 'plan_extraction', 'retry_batch', 'retry_job_with_password', 'start_batch',
  ]);
  for (const tool of tools) {
    assert.equal(tool.annotations.openWorldHint, false);
    assert.equal(tool.annotations.readOnlyHint, ['plan_extraction', 'get_batch_status', 'inspect_archive_content'].includes(tool.name));
  }
  state.sources.push(source('missing.7z.001', true));
  state.notices.push({ path: 'C:\\Input\\partial.crdownload', message: 'Download is incomplete.' });
  const plan = await call('plan_extraction', { paths: ['C:\\Input'], outputRoot: 'D:\\素材', passwordGroup: 'new-group', expandNested: false, verification: 'strict' });
  assert.equal(plan.status, 'ready');
  assert.equal(plan.ready.length, 1);
  assert.equal(plan.blocked[0].reason, 'missingVolume');
  assert.equal(plan.notices.length, 1);
  assert.equal(plan.settings.outputRoot, 'D:\\素材');
  assert.equal(plan.settings.sourceCleanup, 'keep');
  assert.equal(plan.settings.cleanEmptyDirs, false);
  assert.ok(!JSON.stringify(plan).includes('unrelatedDesktopSetting'));
  assert.equal(state.calls.some(value => value.command === 'add_jobs'), false);
  state.settings.outputRoot = 'C:\\ChangedAfterPlan';
  const execution = await call('execute_extraction', { planId: plan.planId });
  assert.equal(execution.status, 'submitted');
  assert.deepEqual(execution.jobIds, ['job-1-0']);
  const add = state.calls.find(value => value.command === 'add_jobs');
  assert.equal(add.args.sources.length, 1, 'missing volumes must never be submitted');
  assert.equal(add.args.options.outputRoot, 'D:\\素材');
  assert.equal(add.args.options.passwordGroup, 'new-group');
  assert.equal(add.args.options.expandNested, false);
  assert.equal(add.args.options.verification, 'strict');
  assert.equal(add.args.options.autoStartOnAdd, false);
  assert.equal(add.args.options.sourceCleanup, 'keep');
  assert.equal(add.args.options.cleanEmptyDirs, false);
  assert.equal(state.jobs.get('unrelated').status, 'queued');
  const result = state.jobs.get(execution.jobIds[0]);
  result.status = 'partial';
  result.outputPath = 'D:\\素材\\sample';
  result.nodes = [{ id: 'node1', name: 'nested.zip', status: 'needsPassword', stage: 'nested', message: 'Needs a password', passwordLabel: secret }];
  const status = await call('get_batch_status', { jobIds: execution.jobIds });
  assert.equal(status.counts.completed, 0);
  assert.equal(status.counts.needsAttention, 1);
  assert.equal(status.jobs[0].outputPath, result.outputPath);
  assert.equal(status.jobs[0].failedNodes[0].name, 'nested.zip');
  assert.ok(!JSON.stringify(status).includes('options'));
  assert.ok(state.calls.every(value => !['start_jobs', 'pause_jobs', 'queue_snapshot', 'history', 'passwords'].includes(value.command)));
});

test('parallel and repeated execute calls share one add and one selected start', async t => {
  const { state, call } = await fixture(t, async body => {
    if (body.command === 'add_jobs') await new Promise(resolve => setTimeout(resolve, 60));
    return false;
  });
  const plan = await call('plan_extraction', { paths: ['C:\\Input'] });
  const [first, second] = await Promise.all([
    call('execute_extraction', { planId: plan.planId }), call('execute_extraction', { planId: plan.planId }),
  ]);
  const third = await call('execute_extraction', { planId: plan.planId });
  assert.deepEqual(first, second);
  assert.deepEqual(first, third);
  assert.equal(state.addCount, 1);
  assert.equal(state.calls.filter(value => value.command === 'start_selected_jobs').length, 1);
});

test('start failure retains job IDs; start_batch only starts queued selected IDs', async t => {
  let denyStart = true;
  const { state, call } = await fixture(t, (body, response) => {
    if (body.command === 'start_selected_jobs' && denyStart) {
      send(response, 403, { error: { message: `${token} ${secret}` } }); return true;
    }
    return false;
  });
  const plan = await call('plan_extraction', { paths: ['C:\\Input'] });
  const execution = await call('execute_extraction', { planId: plan.planId });
  assert.equal(execution.status, 'submittedStartUnconfirmed');
  assert.deepEqual(execution.jobIds, ['job-1-0']);
  assert.equal(execution.error.code, 'API_PERMISSION');
  assert.deepEqual(await call('execute_extraction', { planId: plan.planId }), execution);
  state.jobs.set('already-running', job('already-running', 'running'));
  state.jobs.set('already-complete', job('already-complete', 'completed'));
  denyStart = false;
  const started = await call('start_batch', { jobIds: [...execution.jobIds, 'already-running', 'already-complete'] });
  assert.deepEqual(started.startedJobIds, execution.jobIds);
  assert.deepEqual(state.calls.filter(value => value.command === 'start_selected_jobs').at(-1).args.jobIds, execution.jobIds);
  assert.equal(state.addCount, 1);
  assert.equal(state.jobs.get('unrelated').status, 'queued');
  const again = await call('start_batch', { jobIds: execution.jobIds });
  assert.equal(again.status, 'nothingToStart');
});

test('only confirmed per-job cancellation and preserveOptions retries touch the selected batch', async t => {
  const { state, call } = await fixture(t);
  state.jobs.set('a', job('a', 'running'));
  state.jobs.set('b', job('b', 'completed'));
  const cancelled = await call('cancel_batch', { jobIds: ['a', 'b'] });
  assert.deepEqual(cancelled.results, [{ jobId: 'a', status: 'cancelRequested' }]);
  assert.deepEqual(cancelled.skippedJobIds, ['b']);
  const retried = await call('retry_batch', { jobIds: ['a', 'b'] });
  assert.deepEqual(retried.results, [{ jobId: 'a', status: 'retryRequested' }]);
  assert.deepEqual(state.calls.find(value => value.command === 'retry_job').args, { jobId: 'a', partial: false, preserveOptions: true });
  assert.equal(state.jobs.get('unrelated').status, 'queued');
  assert.equal(state.jobs.get('b').status, 'completed');
});

for (const [status, code] of [[400, 'API_REJECTED'], [401, 'API_AUTH'], [403, 'API_PERMISSION'], [409, 'API_BUSY'], [429, 'API_BUSY'], [500, 'API_FAILURE']]) {
  test(`HTTP ${status} errors are actionable and omit remote error details`, async t => {
    const { call } = await fixture(t, (_body, response) => {
      send(response, status, { error: { code: 'ignored', message: `Bearer ${token} password=${secret}` } }); return true;
    });
    const result = await call('plan_extraction', { paths: ['C:\\Input'] });
    assert.equal(result.isError, true);
    assert.equal(result.error.code, code);
  });
}

test('HTTP redirects are not followed and cannot receive the authorization key', async t => {
  let redirectedRequests = 0;
  const target = createServer((_request, response) => { redirectedRequests++; response.end('unexpected'); });
  target.listen(0, '127.0.0.1');
  await once(target, 'listening');
  t.after(() => new Promise(resolve => target.close(resolve)));
  const { call } = await fixture(t, (_body, response) => {
    response.writeHead(307, { Location: `http://127.0.0.1:${target.address().port}/?token=${token}` });
    response.end(); return true;
  });
  const result = await call('plan_extraction', { paths: ['C:\\Input'] });
  assert.equal(result.error.code, 'API_REDIRECT');
  assert.equal(redirectedRequests, 0);
});

test('API timeout returns a safe bounded error', async t => {
  const { call } = await fixture(t, () => true, { OPENUNPACK_API_TIMEOUT_MS: '60' });
  const started = Date.now();
  const result = await call('plan_extraction', { paths: ['C:\\Input'] });
  assert.equal(result.error.code, 'API_TIMEOUT');
  assert.ok(Date.now() - started < 2000);
});

test('lost add response consumes the plan; repeated execute does not submit again', async t => {
  let attempts = 0;
  const { call, state } = await fixture(t, (body, response) => {
    if (body.command !== 'add_jobs') return false;
    attempts++;
    // Simulate the server accepting a job and losing the response.
    state.jobs.set('accepted-but-response-lost', job('accepted-but-response-lost'));
    response.destroy(); return true;
  });
  const plan = await call('plan_extraction', { paths: ['C:\\Input'] });
  const first = await call('execute_extraction', { planId: plan.planId });
  assert.equal(first.status, 'submissionUnknown');
  assert.equal(first.error.outcomeUncertain, true);
  assert.deepEqual(await call('execute_extraction', { planId: plan.planId }), first);
  assert.equal(attempts, 1);
});

test('all missing volumes block execution without adding jobs', async t => {
  const { state, call } = await fixture(t);
  state.sources = [source('missing.7z.001', true)];
  const plan = await call('plan_extraction', { paths: ['C:\\Input'] });
  assert.equal(plan.status, 'blocked');
  assert.equal(plan.planId, null);
  assert.equal(plan.ready.length, 0);
  assert.equal(state.addCount, 0);
});

test('HTTP 400 after partial add is uncertain and consumes the plan without retrying add', async t => {
  let attempts = 0;
  const { call, state } = await fixture(t, (body, response) => {
    if (body.command !== 'add_jobs') return false;
    attempts++;
    state.jobs.set('partly-saved', job('partly-saved'));
    send(response, 400, { error: { message: `database write failed ${token}` } });
    return true;
  });
  const plan = await call('plan_extraction', { paths: ['C:\\Input'] });
  const first = await call('execute_extraction', { planId: plan.planId });
  assert.equal(first.status, 'submissionUnknown');
  assert.equal(first.error.outcomeUncertain, true);
  assert.deepEqual(await call('execute_extraction', { planId: plan.planId }), first);
  assert.equal(attempts, 1);
});

test('empty successful add does not try to start the existing desktop queue', async t => {
  const { call, state } = await fixture(t, (body, response) => {
    if (body.command !== 'add_jobs') return false;
    send(response, 200, { data: [] }); return true;
  });
  const plan = await call('plan_extraction', { paths: ['C:\\Input'] });
  const execution = await call('execute_extraction', { planId: plan.planId });
  assert.equal(execution.status, 'nothingAdded');
  assert.deepEqual(execution.jobIds, []);
  assert.ok(!state.calls.some(value => value.command === 'start_selected_jobs'));
});

test('token echoes and escaped Bearer messages redact without corrupting structured output', async t => {
  const { call, state } = await fixture(t);
  state.jobs.set('a', { ...job('a', 'failed'), message: `message ${token} Bearer abc\\\"tail` });
  const result = await call('get_batch_status', { jobIds: ['a'] });
  assert.match(result.jobs[0].message, /\[REDACTED\]/);
});

test('expired or restarted plans reject without submission; durable job IDs remain queryable', async t => {
  let time = 100_000;
  const calls = [];
  const api = { redact: value => value, async command(command) {
    calls.push(command);
    if (command === 'read_settings') return settings();
    if (command === 'scan_sources') return { sources: [source()], notices: [] };
    throw new Error('No mutations allowed');
  } };
  const batches = new Batches(api, () => time);
  const plan = await batches.plan({ paths: ['C:\\Input'] });
  time += PLAN_TTL_MS;
  await assert.rejects(batches.execute(plan.planId), error => error.code === 'PLAN_EXPIRED');
  await assert.rejects(new Batches(api).execute(plan.planId), error => error.code === 'PLAN_EXPIRED');
  assert.deepEqual(calls.sort(), ['read_settings', 'scan_sources']);
  const first = await fixture(t);
  first.state.jobs.set('persistent-job', job('persistent-job', 'completed'));
  const result = await first.call('get_batch_status', { jobIds: ['persistent-job'] });
  assert.equal(result.counts.completed, 1);
});

test('strict schemas reject duplicate IDs, empty batches, generic commands and unknown parameters', async t => {
  const { state, client } = await fixture(t);
  for (const args of [{ jobIds: [] }, { jobIds: ['a', 'a'] }, { jobIds: ['a'], command: 'start_jobs' }]) {
    const result = await client.callTool({ name: 'start_batch', arguments: args });
    assert.equal(result.isError, true);
  }
  assert.equal(state.calls.length, 0);
  const result = await client.callTool({ name: 'execute_extraction', arguments: { planId: 'invalid' } });
  assert.equal(result.isError, true);
});

test('startup rejects missing keys and non-loopback origins without exposing their values', () => {
  const invalid = [
    'https://127.0.0.1:47980', 'http://localhost:47980', 'http://example.com',
    'http://127.1:47980', 'http://2130706433:47980', 'http://0x7f000001:47980',
    `http://127.0.0.1:47980/?token=${token}`, `http://${token}@127.0.0.1:47980`,
    'http://127.0.0.1:65536', 'http://127.0.0.1:47980/path',
  ];
  for (const value of invalid) assert.throws(() => validateApiUrl(value), error => error.code === 'CONFIG_URL');
  assert.equal(validateApiUrl('http://127.0.0.1:47980/'), 'http://127.0.0.1:47980');
  const result = spawnSync(process.execPath, [entry], {
    env: { ...process.env, OPENUNPACK_API_URL: `https://example.com/?token=${token}`, OPENUNPACK_API_TOKEN: token }, encoding: 'utf8', timeout: 3000,
  });
  assert.equal(result.status, 1);
  assert.equal(result.stdout, '');
  assert.ok(!result.stderr.includes(token));
  const noKey = spawnSync(process.execPath, [entry], {
    env: { ...process.env, OPENUNPACK_API_URL: 'http://127.0.0.1:47980', OPENUNPACK_API_TOKEN: '' }, encoding: 'utf8', timeout: 3000,
  });
  assert.equal(noKey.status, 1);
  assert.match(noKey.stderr, /CONFIG_TOKEN/);
  assert.equal(noKey.stdout, '');
});
