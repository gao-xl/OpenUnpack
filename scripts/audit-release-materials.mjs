import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
const root = path.resolve(process.argv[2] ?? 'artifacts/release-materials');
const manifest = JSON.parse(fs.readFileSync(path.join(root, 'manifest.json'), 'utf8'));
if (manifest.formatVersion !== 1 || !Array.isArray(manifest.records)) throw new Error('Invalid materials manifest.');
let count = 0, bytes = 0;
for (const record of manifest.records) {
  if (!record.files?.length || !record.version) throw new Error(`Missing material for ${record.component}`);
  for (const file of record.files) {
    const target = path.resolve(root, file.path);
    if (!target.startsWith(root + path.sep) || fs.lstatSync(target).isSymbolicLink()) throw new Error('Materials path escapes release directory.');
    const content = fs.readFileSync(target);
    if (content.length !== file.size || crypto.createHash('sha256').update(content).digest('hex') !== file.sha256) throw new Error(`Corrupt material: ${file.path}`);
    count++; bytes += content.length;
  }
}
for (const component of ['7-Zip', '.NET Runtime', 'tauri', 'react', 'antd']) {
  if (!manifest.records.some(record => record.component === component)) throw new Error(`Missing principal component ${component}`);
}
console.log(`Materials audit passed: ${manifest.records.length} components, ${count} verified files, ${bytes} bytes.`);
