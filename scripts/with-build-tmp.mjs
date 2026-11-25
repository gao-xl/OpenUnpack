import fs from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';

const repoRoot = path.resolve(import.meta.dirname, '..');
const buildTmp = path.join(repoRoot, 'artifacts', 'build-tmp');
fs.mkdirSync(buildTmp, { recursive: true });

const args = process.argv.slice(2);
if (args.length === 0) {
  console.error('Usage: node scripts/with-build-tmp.mjs <command> [...args]');
  process.exit(2);
}

const env = { ...process.env };
if (process.platform === 'win32') {
  env.TEMP = buildTmp;
  env.TMP = buildTmp;
}

const [cmd, ...rest] = args;
const result = spawnSync(cmd, rest, {
  stdio: 'inherit',
  shell: process.platform === 'win32',
  env,
});

if (result.error) {
  console.error(result.error);
  process.exit(1);
}
process.exit(result.status ?? 1);
