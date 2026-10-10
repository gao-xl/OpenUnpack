import { test } from 'node:test';
import assert from 'node:assert/strict';
import * as fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { Workspace } from '../src/workspace.mjs';
import { runTask } from '../src/agent.mjs';
import { Gateway } from '../src/gateway.mjs';
import { loadConfig } from '../src/config.mjs';

const config = { model: 'gemini-3.8-flash-high', baseUrl: 'https://example.com/v1', apiKey: 'unit-test-key',
  maxTurns: 3, maxToolCalls: 5, requestTimeoutMs: 1000, taskTimeoutMs: 10000, maxOutputTokens: 4096 };
async function fixture(t) {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'gemini-unit-'));
  t.after(() => fs.rm(root, { recursive: true, force: true }));
  await fs.writeFile(path.join(root, 'note.txt'), '第一行\nhello world\n');
  return root;
}
const call = (name, args, id = 'call-1') => ({ id, type: 'function', function: { name, arguments: JSON.stringify(args) } });
const response = (message, finish_reason = 'stop') => ({ choices: [{ message: { role: 'assistant', ...message }, finish_reason }] });

test('Gemini tool cycle retains extra fields/signatures and file evidence', async t => {
  const root = await fixture(t);
  let count = 0;
  const gateway = { chat: async messages => {
    if (count++ === 0) return response({ content: null, extra_content: { google: { thought_signature: 'opaque-signature' } },
      tool_calls: [{ ...call('read_file', { path: 'note.txt' }), extra_content: { google: { thought_signature: 'call-signature' } } }] }, 'tool_calls');
    assert.equal(messages[2].extra_content.google.thought_signature, 'opaque-signature');
    assert.equal(messages[2].tool_calls[0].extra_content.google.thought_signature, 'call-signature');
    assert.match(messages[3].content, /hello world/);
    assert.equal(messages[3].tool_call_id, 'call-1');
    return { ...response({ content: 'note.txt:2 包含 hello world。', tool_calls: null }), usage: { total_tokens: 7 } };
  } };
  const result = await runTask({ task: '读取 note', workspace: root, config, gateway });
  assert.equal(result.status, 'completed');
  assert.equal(result.toolCalls, 1);
  assert.equal(result.usage.total_tokens, 7);
  assert.equal(result.events[0].ok, true);
});

test('tool errors returned to model and writes denied by default', async t => {
  const root = await fixture(t);
  let count = 0;
  const gateway = { chat: async messages => {
    if (count++ === 0) return response({ tool_calls: [call('write_file', { path: 'new.txt', content: 'x', expected_sha256: null })] });
    assert.match(messages.at(-1).content, /只读/);
    return response({ content: '无法修改。' });
  } };
  const result = await runTask({ task: 'test', workspace: root, config, gateway });
  assert.equal(result.events[0].ok, false);
  await assert.rejects(fs.access(path.join(root, 'new.txt')));
});

test('task turn and tool limits report unfinished status', async t => {
  const root = await fixture(t);
  const gateway = { chat: async () => response({ tool_calls: [call('list_files', {})] }) };
  const result = await runTask({ task: 'test', workspace: root, config: { ...config, maxTurns: 1 }, gateway });
  assert.equal(result.status, 'limit');
  assert.equal(result.turns, 1);
  const limited = await runTask({ task: 'test', workspace: root, config: { ...config, maxToolCalls: 0 }, gateway });
  assert.equal(limited.toolCalls, 0);
  assert.equal(limited.status, 'limit');
});

test('length, refusals, missing answers and cancellation are not successful', async t => {
  const root = await fixture(t);
  const params = { task: 'test', workspace: root, config };
  assert.equal((await runTask({ ...params, gateway: { chat: async () => response({ content: 'cut' }, 'length') } })).status, 'incomplete');
  assert.equal((await runTask({ ...params, gateway: { chat: async () => response({ refusal: 'blocked' }) } })).status, 'blocked');
  await assert.rejects(runTask({ ...params, gateway: { chat: async () => response({ content: null }) } }), /没有返回答案/);
  const controller = new AbortController(); controller.abort();
  assert.equal((await runTask({ ...params, signal: controller.signal, gateway: { chat: async () => assert.fail() } })).status, 'cancelled');
});

test('duplicate tool IDs rejected before running tools', async t => {
  const root = await fixture(t);
  await assert.rejects(runTask({ task: 'test', workspace: root, config,
    gateway: { chat: async () => response({ tool_calls: [call('list_files', {}), call('list_files', {})] }) } }), /ID/);
});

test('workspace traversal, absolute paths, secrets and symlinks rejected', async t => {
  const root = await fixture(t);
  const files = await Workspace.open(root);
  for (const value of ['../other.txt', path.resolve(root, 'note.txt'), '.env', 'a/.env.production', '.git/config', 'key.dpapi', 'a:stream']) {
    await assert.rejects(files.resolve(value));
  }
  await fs.mkdir(path.join(root, 'outside-link'));
  await fs.symlink(path.join(root, 'outside-link'), path.join(root, 'link'), process.platform === 'win32' ? 'junction' : 'dir');
  await assert.rejects(files.resolve('link/note.txt'), /链接/);
  await fs.writeFile(path.join(root, '.env'), 'hidden');
  assert.deepEqual((await files.walk()).files, ['note.txt']);
});

test('UTF8 reads preserve hash, search has line evidence and binary files rejected', async t => {
  const root = await fixture(t);
  const files = await Workspace.open(root);
  const read = await files.execute('read_file', { path: 'note.txt', start_line: 2, max_lines: 1 });
  assert.equal(read.content, '2: hello world');
  assert.match(read.sha256, /^[a-f0-9]{64}$/);
  const search = await files.execute('search_text', { query: 'hello' });
  assert.equal(search.matches[0].line, 2);
  await fs.writeFile(path.join(root, 'binary'), Buffer.from([0, 1, 2]));
  await assert.rejects(files.readBytes('binary'), /二进制/);
  await fs.writeFile(path.join(root, 'invalid'), Buffer.from([0xff, 0xfe]));
  await assert.rejects(files.readBytes('invalid'));
});

test('writes require matching hash and preserve the prior version', async t => {
  const root = await fixture(t);
  const files = await Workspace.open(root, true);
  const read = await files.execute('read_file', { path: 'note.txt' });
  await assert.rejects(files.execute('write_file', { path: 'note.txt', content: 'new', expected_sha256: null }), /已经变化/);
  const result = await files.execute('write_file', { path: 'note.txt', content: 'new', expected_sha256: read.sha256 });
  assert.equal(await fs.readFile(result.backup, 'utf8'), '第一行\nhello world\n');
  assert.equal(await fs.readFile(path.join(root, 'note.txt'), 'utf8'), 'new');
  await assert.rejects(files.execute('write_file', { path: 'note.txt', content: 'stale', expected_sha256: read.sha256 }), /已经变化/);
  await files.execute('write_file', { path: 'new.txt', content: 'created', expected_sha256: null });
  assert.equal(await fs.readFile(path.join(root, 'new.txt'), 'utf8'), 'created');
  assert.ok(!(await files.walk()).files.some(file => file.includes('backups')));
});

test('gateway bearer auth, retry, timeouts, no redirects and sanitized errors', async () => {
  let attempts = 0;
  const gateway = new Gateway(config, async (url, opts) => {
    assert.equal(url, 'https://example.com/v1/chat/completions');
    assert.equal(opts.headers.Authorization, 'Bearer unit-test-key');
    assert.equal(opts.redirect, 'error');
    assert.equal(JSON.parse(opts.body).model, config.model);
    if (++attempts === 1) return new Response('rate limited', { status: 429, headers: { 'retry-after': '0' } });
    return Response.json(response({ content: 'ok' }));
  });
  await gateway.chat([], [], AbortSignal.timeout(2000));
  assert.equal(attempts, 2);
  const denied = new Gateway(config, async () => new Response('SECRET', { status: 401 }));
  await assert.rejects(denied.models(), error => /401/.test(error.message) && !error.message.includes('SECRET'));
  const timed = new Gateway({ ...config, requestTimeoutMs: 10 }, async (_, opts) => {
    await new Promise((_, reject) => opts.signal.addEventListener('abort', () => reject(new Error('SECRET'))));
  });
  await assert.rejects(timed.models(), /超时/);
});

test('config rejects insecure endpoints and missing credentials', async () => {
  await assert.rejects(loadConfig({}), /缺少/);
  for (const url of ['http://example.com/v1', 'https://user:pass@example.com/v1', 'https://example.com/v1?key=x']) {
    await assert.rejects(loadConfig({ GEMINI_API_KEY: 'test', GEMINI_BASE_URL: url }));
  }
  const loaded = await loadConfig({ GEMINI_API_KEY: 'test', GEMINI_BASE_URL: 'http://127.0.0.1:1234/v1/' });
  assert.equal(loaded.baseUrl, 'http://127.0.0.1:1234/v1');
});
