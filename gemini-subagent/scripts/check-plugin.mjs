import assert from 'node:assert/strict';
import * as fs from 'node:fs/promises';
import path from 'node:path';
import os from 'node:os';
import { randomUUID } from 'node:crypto';
import { createHash } from 'node:crypto';
import { PAYLOAD } from './prepare-plugin.mjs';
import { Client } from '@modelcontextprotocol/sdk/client/index.js';
import { StdioClientTransport } from '@modelcontextprotocol/sdk/client/stdio.js';

const rootFlag = process.argv.indexOf('--root');
const root = rootFlag < 0 ? path.join(os.homedir(), '.codex', 'plugins', 'gemini-subagent') : path.resolve(process.argv[rootFlag + 1]);
const manifest = JSON.parse(await fs.readFile(path.join(root, '.codex-plugin', 'plugin.json'), 'utf8'));
assert.equal(manifest.name, 'gemini-subagent');
const pkg = JSON.parse(await fs.readFile(path.join(root, 'package.json'), 'utf8'));
assert.equal(manifest.version, pkg.version, 'Manifest and package versions differ');
const config = JSON.parse(await fs.readFile(path.join(root, 'config.json'), 'utf8'));
const compareFlag = process.argv.indexOf('--compare-source');
if (compareFlag >= 0) {
  const source = path.resolve(process.argv[compareFlag + 1]);
  const compare = async relative => {
    const sourcePath = path.join(source, relative);
    if ((await fs.stat(sourcePath)).isDirectory()) {
      const expected = (await fs.readdir(sourcePath)).sort();
      assert.deepEqual((await fs.readdir(path.join(root, relative))).sort(), expected, `Installed directory differs: ${relative}`);
      for (const entry of expected) await compare(path.join(relative, entry));
    } else {
      const digest = content => createHash('sha256').update(content).digest('hex');
      assert.equal(digest(await fs.readFile(path.join(root, relative))), digest(await fs.readFile(sourcePath)),
        `Installed file differs: ${relative}. Publish a new version instead of overwriting an active version.`);
    }
  };
  for (const item of PAYLOAD) await compare(item);
  console.log(JSON.stringify({ installedPayloadMatchesSource: true, version: pkg.version }));
}
for (const declared of [manifest.skills, manifest.mcpServers, manifest.interface.composerIcon]) {
  assert.ok(declared.startsWith('./') && !declared.split('/').includes('..'));
  await fs.access(path.join(root, declared));
}
const mapping = JSON.parse(await fs.readFile(path.join(root, manifest.mcpServers), 'utf8'));
const server = mapping.mcpServers.gemini_delegate;
const client = new Client({ name: 'plugin-package-check', version: '1.0.0' });
const transport = new StdioClientTransport({ command: server.command, args: server.args,
  cwd: path.resolve(root, server.cwd), env: { ...process.env }, stderr: 'pipe' });
transport.stderr?.on('data', chunk => process.stderr.write(chunk));
try {
  await client.connect(transport);
  assert.equal(client.getServerVersion().version, pkg.version, 'MCP runtime version differs');
  const tools = await client.listTools();
  assert.equal(tools.tools[0].name, 'gemini_delegate');
  assert.equal(tools.tools[0].annotations.readOnlyHint, true);
  const denied = await client.callTool({ name: 'gemini_delegate', arguments: { task: 'read' } });
  assert.equal(denied.isError, true);
  assert.match(denied.content[0].text, /workspace/);
  console.log(JSON.stringify({ pluginLaunch: 'passed', explicitWorkspaceRequired: true,
    tool: 'gemini_delegate', model: process.env.GEMINI_MODEL || config.model, version: pkg.version }));
  if (process.argv.includes('--live')) {
    const fixture = await fs.mkdtemp(path.join(os.tmpdir(), 'gemini-plugin-live-'));
    const token = randomUUID();
    try {
      await fs.writeFile(path.join(fixture, 'fixture.txt'), `插件测试标记：${token}\n`);
      const result = await client.callTool({ name: 'gemini_delegate', arguments: {
        task: '使用 read_file 读取 fixture.txt，原样返回其中的测试标记值。只读，不修改。', workspace: fixture,
      } }, undefined, { timeout: 300000 });
      assert.equal(result.isError, false);
      assert.ok(result.structuredContent.answer.includes(token));
      assert.ok(result.structuredContent.events.some(event => event.tool === 'read_file' && event.ok));
      console.log(JSON.stringify({ livePluginTask: 'passed', turns: result.structuredContent.turns,
        toolCalls: result.structuredContent.toolCalls, model: result.structuredContent.model }));
    } finally { await fs.rm(fixture, { recursive: true, force: true }); }
  }
} finally { await client.close(); }
