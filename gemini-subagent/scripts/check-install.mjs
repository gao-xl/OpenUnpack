import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { Client } from '@modelcontextprotocol/sdk/client/index.js';
import { StdioClientTransport } from '@modelcontextprotocol/sdk/client/stdio.js';

// The installer writes JSON-compatible single-line TOML strings and arrays.
// Only inspect its own table; never print or export other server credentials.
const lines = (await readFile(path.join(os.homedir(), '.codex', 'config.toml'), 'utf8')).split(/\r?\n/);
const start = lines.indexOf('[mcp_servers.gemini_subagent]');
assert.ok(start >= 0, 'gemini_subagent is not registered');
const section = [];
for (const line of lines.slice(start + 1)) { if (line.startsWith('[')) break; section.push(line); }
const field = name => JSON.parse(section.find(line => line.startsWith(`${name} = `)).slice(name.length + 3));
const client = new Client({ name: 'installed-launch-check', version: '1.0.0' });
const transport = new StdioClientTransport({ command: field('command'), args: field('args'), env: { ...process.env }, stderr: 'pipe' });
transport.stderr?.on('data', chunk => process.stderr.write(chunk));
try {
  await client.connect(transport);
  const tools = await client.listTools();
  assert.equal(tools.tools[0].name, 'gemini_delegate');
  assert.equal(tools.tools[0].annotations.readOnlyHint, true);
  assert.ok(tools.tools[0].description.includes('gemini-3.8-flash-high'));
  console.log(JSON.stringify({ registeredLaunch: 'passed', tool: 'gemini_delegate',
    readOnly: true, model: 'gemini-3.8-flash-high' }));
} finally { await client.close(); }
