import { test } from 'node:test';
import assert from 'node:assert/strict';
import http from 'node:http';
import * as fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { Client } from '@modelcontextprotocol/sdk/client/index.js';
import { StdioClientTransport } from '@modelcontextprotocol/sdk/client/stdio.js';

test('MCP handshake, tool listing, multi-turn delegation and permission boundary', async t => {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'gemini-mcp-'));
  t.after(() => fs.rm(root, { recursive: true, force: true }));
  await fs.writeFile(path.join(root, 'fixture.txt'), 'protocol-evidence');
  const api = http.createServer(async (req, res) => {
    assert.equal(req.headers.authorization, 'Bearer test-key');
    let text = ''; for await (const chunk of req) text += chunk;
    const body = JSON.parse(text);
    assert.equal(body.model, 'gemini-3.8-flash-high');
    const toolResult = body.messages.find(m => m.role === 'tool');
    res.setHeader('Content-Type', 'application/json');
    res.end(JSON.stringify({ choices: [{ finish_reason: 'stop', message: toolResult
      ? { role: 'assistant', content: JSON.parse(toolResult.content).content }
      : { role: 'assistant', content: null, tool_calls: [{ id: 'read-1', type: 'function',
        function: { name: 'read_file', arguments: '{"path":"fixture.txt"}' } }] } }] }));
  });
  await new Promise(resolve => api.listen(0, '127.0.0.1', resolve));
  t.after(() => new Promise(resolve => api.close(resolve)));
  const windows = process.platform === 'win32';
  const transport = new StdioClientTransport({ command: windows ? 'powershell.exe' : process.execPath,
    args: windows ? ['-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File',
      fileURLToPath(new URL('../run.ps1', import.meta.url)), '-Mode', 'mcp', '--workspace', root]
      : [fileURLToPath(new URL('../src/mcp.mjs', import.meta.url)), '--workspace', root],
    env: { ...process.env, GEMINI_API_KEY: 'test-key', GEMINI_BASE_URL: `http://127.0.0.1:${api.address().port}/v1` },
    stderr: 'pipe' });
  const client = new Client({ name: 'test-client', version: '1.0.0' });
  t.after(() => client.close());
  await client.connect(transport);
  const listing = await client.listTools();
  assert.equal(listing.tools[0].name, 'gemini_delegate');
  assert.equal(listing.tools[0].annotations.readOnlyHint, true);
  const result = await client.callTool({ name: 'gemini_delegate', arguments: { task: 'read fixture' } });
  assert.equal(result.isError, false);
  assert.equal(result.structuredContent.status, 'completed');
  assert.match(result.structuredContent.answer, /protocol-evidence/);
  assert.equal(result.structuredContent.events[0].tool, 'read_file');
  const denied = await client.callTool({ name: 'gemini_delegate', arguments: { task: 'write', allow_write: true } });
  assert.equal(denied.isError, true);
  const escape = await client.callTool({ name: 'gemini_delegate', arguments: { task: 'read', subdirectory: '..' } });
  assert.equal(escape.isError, true);
  const override = await client.callTool({ name: 'gemini_delegate', arguments: { task: 'read', workspace: root } });
  assert.equal(override.isError, true);

  const dynamicTransport = new StdioClientTransport({ command: process.execPath,
    args: [fileURLToPath(new URL('../src/mcp.mjs', import.meta.url)), '--dynamic-workspace'],
    env: { ...process.env, GEMINI_API_KEY: 'test-key', GEMINI_BASE_URL: `http://127.0.0.1:${api.address().port}/v1` }, stderr: 'pipe' });
  const dynamicClient = new Client({ name: 'plugin-test', version: '1.0.0' });
  t.after(() => dynamicClient.close());
  await dynamicClient.connect(dynamicTransport);
  for (const workspace of [undefined, '.']) {
    const invalid = await dynamicClient.callTool({ name: 'gemini_delegate', arguments: { task: 'read', workspace } });
    assert.equal(invalid.isError, true);
  }
  const first = await dynamicClient.callTool({ name: 'gemini_delegate', arguments: { task: 'read', workspace: root } });
  assert.match(first.structuredContent.answer, /protocol-evidence/);
  const secondRoot = path.join(root, 'other-project');
  await fs.mkdir(secondRoot);
  await fs.writeFile(path.join(secondRoot, 'fixture.txt'), 'second-workspace');
  const second = await dynamicClient.callTool({ name: 'gemini_delegate', arguments: { task: 'read', workspace: secondRoot } });
  assert.match(second.structuredContent.answer, /second-workspace/);
  assert.ok(!second.structuredContent.answer.includes('protocol-evidence'));
});
