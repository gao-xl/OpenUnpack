import * as fs from 'node:fs/promises';
import path from 'node:path';
import os from 'node:os';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';
import { createHash, randomUUID } from 'node:crypto';

export const PAYLOAD = ['.codex-plugin', '.mcp.json', 'assets', 'skills', 'src', 'scripts', 'test', 'node_modules',
  'config.json', 'package.json', 'package-lock.json', 'run.ps1', 'set-credential.ps1', 'setup.ps1', 'README.md', 'VALIDATION.md'];
const read = async file => {
  try { return await fs.readFile(file, 'utf8'); } catch (error) { if (error.code !== 'ENOENT') throw error; }
};
const json = value => JSON.stringify(value, null, 2) + '\n';
const inside = (parent, child) => {
  const relative = path.relative(parent, child);
  return relative !== '' && relative !== '..' && !relative.startsWith(`..${path.sep}`) && !path.isAbsolute(relative);
};
async function fingerprint(root, items) {
  const entries = [];
  const visit = async relative => {
    const file = path.join(root, relative);
    const stat = await fs.lstat(file);
    if (stat.isSymbolicLink()) throw new Error('Plugin payload cannot contain symbolic links.');
    if (stat.isDirectory()) {
      for (const name of (await fs.readdir(file)).sort()) await visit(path.join(relative, name));
    } else entries.push([relative.split(path.sep).join('/'), createHash('sha256').update(await fs.readFile(file)).digest('hex')]);
  };
  for (const item of items) await visit(item);
  return JSON.stringify(entries);
}
async function atomicWrite(file, contents) {
  const temporary = `${file}.${randomUUID()}.tmp`;
  await fs.writeFile(temporary, contents, { flag: 'wx' });
  try { await fs.rename(temporary, file); } finally { await fs.unlink(temporary).catch(() => {}); }
}

export async function preparePlugin({ source, userRoot = os.homedir() }) {
  source = await fs.realpath(source);
  userRoot = await fs.realpath(userRoot);
  const pluginsRoot = path.join(userRoot, '.codex', 'plugins');
  const target = path.join(pluginsRoot, 'gemini-subagent');
  const marketFile = path.join(userRoot, '.agents', 'plugins', 'marketplace.json');
  const originalMarket = await read(marketFile);
  const market = originalMarket ? JSON.parse(originalMarket.replace(/^\uFEFF/, ''))
    : { name: 'personal-tools', interface: { displayName: 'Personal Tools' }, plugins: [] };
  if (!market.name || !Array.isArray(market.plugins)) throw new Error('Invalid existing personal marketplace.');
  const manifest = JSON.parse(await read(path.join(source, '.codex-plugin', 'plugin.json')));
  const pkg = JSON.parse(await read(path.join(source, 'package.json')));
  if (manifest.name !== 'gemini-subagent' || pkg.version !== manifest.version) throw new Error('Plugin identity/version mismatch.');
  const entry = market.plugins.find(item => item.name === manifest.name);
  if (entry && (entry.source?.source !== 'local' || entry.source.path !== './.codex/plugins/gemini-subagent')) {
    throw new Error('A different gemini-subagent marketplace source already exists; preserved.');
  }
  await fs.mkdir(pluginsRoot, { recursive: true });
  if (!inside(userRoot, await fs.realpath(pluginsRoot))) throw new Error('Plugin directory escapes the user root.');
  let exists = false;
  try {
    const stat = await fs.lstat(target);
    if (stat.isSymbolicLink() || !stat.isDirectory()) throw new Error('Existing plugin target is not a plain directory.');
    const existingManifest = await read(path.join(target, '.codex-plugin', 'plugin.json'));
    if (!existingManifest || JSON.parse(existingManifest).name !== manifest.name) throw new Error('Existing directory belongs to another plugin; preserved.');
    exists = true;
  } catch (error) { if (error.code !== 'ENOENT') throw error; }
  const lockPath = path.join(pluginsRoot, '.gemini-subagent-install.lock');
  const lock = await fs.open(lockPath, 'wx');
  const stage = path.join(pluginsRoot, `.gemini-subagent-stage-${randomUUID()}`);
  const backup = path.join(pluginsRoot, `.gemini-subagent-backup-${Date.now()}-${randomUUID()}`);
  let movedOld = false;
  let promoted = false;
  let changed = true;
  try {
    await fs.mkdir(stage);
    for (const item of PAYLOAD) await fs.cp(path.join(source, item), path.join(stage, item), { recursive: true, force: false, errorOnExist: true });
    if (exists) {
      if (source !== target) {
        const defaults = JSON.parse(await read(path.join(stage, 'config.json')));
        const previous = JSON.parse(await read(path.join(target, 'config.json')));
        for (const name of Object.keys(defaults)) if (Object.hasOwn(previous, name)) defaults[name] = previous[name];
        await fs.writeFile(path.join(stage, 'config.json'), json(defaults));
      }
      try { changed = await fingerprint(stage, PAYLOAD) !== await fingerprint(target, PAYLOAD); }
      catch (error) { if (error.code !== 'ENOENT') throw error; }
    }
    if ((await read(marketFile)) !== originalMarket) throw new Error('Marketplace changed during preparation; cancelled.');
    if (changed) {
      for (const location of [stage, target, backup]) if (!inside(pluginsRoot, location)) throw new Error('Unsafe plugin deployment path.');
      if (exists) { await fs.rename(target, backup); movedOld = true; }
      await fs.rename(stage, target);
      promoted = true;
    }
    if (!entry) {
      market.plugins.push({ name: manifest.name, source: { source: 'local', path: './.codex/plugins/gemini-subagent' },
        policy: { installation: 'AVAILABLE', authentication: 'ON_INSTALL' }, category: 'Productivity' });
      await fs.mkdir(path.dirname(marketFile), { recursive: true });
      if (originalMarket) await fs.writeFile(`${marketFile}.${Date.now()}.bak`, originalMarket, { flag: 'wx' });
      await atomicWrite(marketFile, json(market));
    }
    return { pluginSource: target, marketplace: marketFile, marketplaceName: market.name,
      pluginId: `${manifest.name}@${market.name}`, version: pkg.version, changed,
      backup: movedOld ? backup : undefined, credentialBundled: false };
  } catch (error) {
    if (promoted) await fs.rename(target, stage);
    if (movedOld) await fs.rename(backup, target);
    throw error;
  } finally {
    try {
      try {
        const actual = await fs.realpath(stage);
        if (actual !== stage || !inside(pluginsRoot, actual)) throw new Error('Unsafe staging cleanup path.');
        await fs.rm(stage, { recursive: true, force: true });
      } catch (error) { if (error.code !== 'ENOENT') throw error; }
    } finally { await lock.close(); await fs.unlink(lockPath); }
  }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    const { values } = parseArgs({ options: { source: { type: 'string' }, 'user-root': { type: 'string' } } });
    console.log(json(await preparePlugin({ source: values.source || fileURLToPath(new URL('..', import.meta.url)), userRoot: values['user-root'] })));
  } catch (error) { console.error(error.message); process.exitCode = 1; }
}
