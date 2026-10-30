import fs from 'node:fs';

export const releasePolicy = JSON.parse(fs.readFileSync(new URL('./source-release-policy.json', import.meta.url), 'utf8'));
if (releasePolicy.schemaVersion !== 1 || !Array.isArray(releasePolicy.documentationFiles)
  || !Array.isArray(releasePolicy.privateDirectoryNames) || !Array.isArray(releasePolicy.excludedPaths)) {
  throw new Error('Invalid source release policy.');
}
const documents = new Set(releasePolicy.documentationFiles);
export function sourcePathRejection(relative, isDirectory = false) {
  const name = relative.replaceAll('\\', '/');
  const folded = name.toLowerCase();
  if (folded.split('/').some(part => releasePolicy.privateDirectoryNames.includes(part))) return 'private workspace material';
  if (releasePolicy.excludedPaths.some(excluded => folded === excluded.toLowerCase() || folded.startsWith(`${excluded.toLowerCase()}/`))) return 'retired prototype or local helper';
  if (folded.startsWith('docs/')) {
    const allowed = isDirectory
      ? releasePolicy.documentationFiles.some(file => file.startsWith(`${name}/`))
      : documents.has(name);
    if (!allowed) return 'documentation outside the reviewed public list';
  }
  return null;
}
