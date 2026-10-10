import { test } from 'node:test';
import assert from 'node:assert/strict';
import * as fs from 'node:fs/promises';
import path from 'node:path';
import os from 'node:os';
import { preparePlugin, PAYLOAD } from '../scripts/prepare-plugin.mjs';

async function fixture(t) {
  const temporary = await fs.mkdtemp(path.join(os.tmpdir(), 'gemini-deploy-'));
  t.after(async () => {
    const actual = await fs.realpath(temporary);
    assert.ok(path.dirname(actual) === await fs.realpath(os.tmpdir()) && path.basename(actual).startsWith('gemini-deploy-'));
    await fs.rm(actual, { recursive: true, force: true });
  });
  const source = path.join(temporary, 'source');
  const userRoot = path.join(temporary, 'user');
  await fs.mkdir(source); await fs.mkdir(userRoot);
  const folders = ['.codex-plugin', 'assets', 'skills', 'src', 'scripts', 'test', 'node_modules'];
  for (const item of PAYLOAD) {
    if (folders.includes(item)) { await fs.mkdir(path.join(source, item)); await fs.writeFile(path.join(source, item, 'fixture'), item); }
    else await fs.writeFile(path.join(source, item), 'fixture');
  }
  await fs.writeFile(path.join(source, '.codex-plugin', 'plugin.json'), JSON.stringify({ name: 'gemini-subagent', version: '1.0.0' }));
  await fs.writeFile(path.join(source, 'package.json'), JSON.stringify({ name: 'gemini-subagent', version: '1.0.0' }));
  await fs.writeFile(path.join(source, 'config.json'), JSON.stringify({ model: 'gemini-3.8-flash-high', baseUrl: 'https://gateway.example/v1', maxTurns: 8 }, null, 2) + '\n');
  return { source, userRoot };
}

test('initial deployment is repeatable without duplicate entries or new backups', async t => {
  const params = await fixture(t);
  const first = await preparePlugin(params);
  assert.equal(first.changed, true);
  assert.equal(await fs.readFile(path.join(first.pluginSource, 'scripts', 'fixture'), 'utf8'), 'scripts');
  const second = await preparePlugin(params);
  assert.equal(second.changed, false);
  assert.equal(second.backup, undefined);
  const fromInstalledSource = await preparePlugin({ ...params, source: first.pluginSource });
  assert.equal(fromInstalledSource.changed, false);
  const market = JSON.parse(await fs.readFile(first.marketplace, 'utf8'));
  assert.equal(market.plugins.length, 1);
});

test('upgrade preserves custom model, marketplace peers, credentials and old version', async t => {
  const params = await fixture(t);
  const first = await preparePlugin(params);
  const market = JSON.parse(await fs.readFile(first.marketplace, 'utf8'));
  market.plugins.push({ name: 'other-plugin', source: { source: 'local', path: './other' } });
  await fs.writeFile(first.marketplace, JSON.stringify(market));
  const customized = { model: 'custom-gemini', baseUrl: 'https://private.example/v1', maxTurns: 5 };
  await fs.writeFile(path.join(first.pluginSource, 'config.json'), JSON.stringify(customized));
  const credentialDir = path.join(params.userRoot, '.codex', 'gemini-subagent');
  await fs.mkdir(credentialDir);
  await fs.writeFile(path.join(credentialDir, 'gateway-key.dpapi'), 'encrypted-fixture');
  for (const file of ['package.json', '.codex-plugin/plugin.json']) {
    await fs.writeFile(path.join(params.source, file), JSON.stringify({ name: 'gemini-subagent', version: '1.0.1' }));
  }
  const updated = await preparePlugin(params);
  assert.equal(updated.version, '1.0.1');
  assert.ok(updated.backup);
  assert.equal(JSON.parse(await fs.readFile(path.join(updated.backup, 'package.json'), 'utf8')).version, '1.0.0');
  assert.deepEqual(JSON.parse(await fs.readFile(path.join(updated.pluginSource, 'config.json'), 'utf8')), customized);
  assert.equal(JSON.parse(await fs.readFile(first.marketplace, 'utf8')).plugins.length, 2);
  assert.equal(await fs.readFile(path.join(credentialDir, 'gateway-key.dpapi'), 'utf8'), 'encrypted-fixture');
  await assert.rejects(fs.access(path.join(updated.pluginSource, 'gateway-key.dpapi')));
});

test('failed staging leaves old installation and marketplace intact and releases lock', async t => {
  const params = await fixture(t);
  const first = await preparePlugin(params);
  const priorMarket = await fs.readFile(first.marketplace, 'utf8');
  await fs.unlink(path.join(params.source, 'run.ps1'));
  await assert.rejects(preparePlugin(params), { code: 'ENOENT' });
  assert.equal(await fs.readFile(path.join(first.pluginSource, 'run.ps1'), 'utf8'), 'fixture');
  assert.equal(await fs.readFile(first.marketplace, 'utf8'), priorMarket);
  const dirs = await fs.readdir(path.dirname(first.pluginSource));
  assert.ok(!dirs.some(name => name.includes('stage-') || name.endsWith('.lock')));
});

test('conflicting marketplace source is preserved', async t => {
  const params = await fixture(t);
  const marketPath = path.join(params.userRoot, '.agents', 'plugins', 'marketplace.json');
  await fs.mkdir(path.dirname(marketPath), { recursive: true });
  const contents = JSON.stringify({ name: 'personal-tools', plugins: [{ name: 'gemini-subagent', source: { source: 'local', path: './someone-else' } }] });
  await fs.writeFile(marketPath, contents);
  await assert.rejects(preparePlugin(params), /different/);
  assert.equal(await fs.readFile(marketPath, 'utf8'), contents);
});
