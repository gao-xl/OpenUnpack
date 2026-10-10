import assert from 'node:assert/strict';
import * as fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { randomUUID } from 'node:crypto';
import { Client } from '@modelcontextprotocol/sdk/client/index.js';
import { StdioClientTransport } from '@modelcontextprotocol/sdk/client/stdio.js';
import { loadConfig } from '../src/config.mjs';
import { Gateway } from '../src/gateway.mjs';

const config = await loadConfig();
assert.ok((await new Gateway(config).models()).includes(config.model), '网关没有指定模型');
const root = await fs.mkdtemp(path.join(os.tmpdir(), 'gemini-live-'));
const token = randomUUID();
await fs.writeFile(path.join(root, 'fixture.txt'), `标记：${token}\n这是一份用于 Gemini 子 agent 集成测试的文件。\n`);
const client = new Client({ name: 'live-smoke', version: '1.0.0' });
const transport = new StdioClientTransport({ command: process.execPath,
  args: [fileURLToPath(new URL('../src/mcp.mjs', import.meta.url)), '--workspace', root, '--allow-write'],
  env: { ...process.env }, stderr: 'pipe' });
try {
  await client.connect(transport);
  const listing = await client.listTools();
  assert.equal(listing.tools[0].name, 'gemini_delegate');
  const read = await client.callTool({ name: 'gemini_delegate', arguments: {
    task: '使用 read_file 读取 fixture.txt，原样返回“标记：”后面的标记值，并引用文件行号。不要写文件。',
  } }, undefined, { timeout: 300000 });
  assert.equal(read.isError, false, JSON.stringify(read.content));
  assert.equal(read.structuredContent.status, 'completed');
  assert.ok(read.structuredContent.answer.includes(token), '返回内容未包含真实文件标记');
  assert.ok(read.structuredContent.events.some(event => event.tool === 'read_file' && event.ok));
  console.log(JSON.stringify({ test: 'live-mcp-read', model: config.model, status: 'passed',
    turns: read.structuredContent.turns, toolCalls: read.structuredContent.toolCalls }));
  const write = await client.callTool({ name: 'gemini_delegate', arguments: {
    task: '把 fixture.txt 第一行的“标记：”改成“验证标记：”，保留标记值及其余内容。先读取，再使用 write_file 修改，完成后再次读取核实。',
    allow_write: true,
  } }, undefined, { timeout: 300000 });
  assert.equal(write.isError, false, JSON.stringify(write.content));
  assert.equal(write.structuredContent.status, 'completed');
  assert.equal(await fs.readFile(path.join(root, 'fixture.txt'), 'utf8'), `验证标记：${token}\n这是一份用于 Gemini 子 agent 集成测试的文件。\n`);
  assert.ok(write.structuredContent.events.some(event => event.tool === 'write_file' && event.ok));
  const backups = await fs.readdir(path.join(root, '.gemini-subagent-backups'));
  assert.equal(backups.length, 1);
  console.log(JSON.stringify({ test: 'live-mcp-write', model: config.model, status: 'passed',
    turns: write.structuredContent.turns, toolCalls: write.structuredContent.toolCalls }));
} finally {
  await client.close();
  await fs.rm(root, { recursive: true, force: true });
}
