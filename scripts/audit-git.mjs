import fs from 'node:fs';
import path from 'node:path';
import { execFileSync } from 'node:child_process';
import { sourcePathRejection } from './source-release-policy.mjs';

const root = path.resolve(process.argv[2] ?? '.');
const entries = execFileSync('git', ['ls-files', '--stage', '-z'], { cwd: root, encoding: 'utf8' })
  .split('\0')
  .filter(Boolean)
  .map((line) => {
    const separator = line.indexOf('\t');
    const [mode, object, stage] = line.slice(0, separator).split(' ');
    return { mode, object, stage, relative: line.slice(separator + 1) };
  });
const objects = execFileSync('git', ['cat-file', '--batch'], {
  cwd: root,
  input: entries.map((entry) => `${entry.object}\n`).join(''),
  maxBuffer: 64 * 1024 * 1024,
});
let offset = 0;
for (const entry of entries) {
  const newline = objects.indexOf(10, offset);
  const header = objects.subarray(offset, newline).toString('ascii').split(' ');
  const size = Number(header[2]);
  if (header[0] !== entry.object || header[1] !== 'blob' || !Number.isSafeInteger(size))
    throw new Error('Invalid Git blob response.');
  entry.indexBytes = objects.subarray(newline + 1, newline + 1 + size);
  offset = newline + 1 + size + 1;
}
const generated =
  /(^|\/)(node_modules|target(?:_[^/]+)?|bin|obj|dist|\.git|\.ruff_cache|__pycache__|artifacts|_workspace[^/]*)(\/|$)|(^|\/)\.env(?:\.[^/]*)?$|\.(?:tsbuildinfo|pdb|pyc|vault(?:\.bak)?|sqlite(?:3)?(?:-wal|-shm)?|db|dpapi|pem|pfx|key)$/i;
const credentials = [
  /-----BEGIN (?:RSA |EC |OPENSSH )?PRIVATE KEY-----/,
  /\b(?:sk-[A-Za-z0-9_-]{24,}|gh[pousr]_[A-Za-z0-9]{30,}|AIza[0-9A-Za-z_-]{30,})\b/,
  /(?:api[_-]?key|access[_-]?token|client[_-]?secret)\s*[=:]\s*["'][A-Za-z0-9_+\/-]{32,}["']/i,
];
const failures = [];
for (const { relative, mode, stage, indexBytes } of entries) {
  const file = path.join(root, relative);
  const reason =
    sourcePathRejection(relative) ??
    (generated.test(relative) ? 'generated or sensitive file' : null);
  if (reason) {
    failures.push(`${relative}: ${reason}`);
    continue;
  }
  if (stage !== '0' || mode === '120000' || mode === '160000') {
    failures.push(`${relative}: unmerged, linked or embedded repository entry`);
    continue;
  }
  if (!fs.existsSync(file)) {
    failures.push(`${relative}: tracked file is absent`);
    continue;
  }
  if (fs.lstatSync(file).isSymbolicLink()) {
    failures.push(`${relative}: symbolic link`);
    continue;
  }
  const bytes = fs.readFileSync(file);
  if (
    [bytes, indexBytes].some(
      (content) =>
        !content.subarray(0, 8192).includes(0) &&
        credentials.some((rule) => rule.test(content.toString('utf8'))),
    )
  ) {
    failures.push(`${relative}: possible embedded credential (value redacted)`);
  }
}
if (failures.length) throw new Error(`Public Git content audit failed:\n${failures.join('\n')}`);
console.log(
  `Public Git content audit passed: ${entries.length} tracked paths; index and working files checked for private material, retired prototypes, generated files and credential patterns.`,
);
