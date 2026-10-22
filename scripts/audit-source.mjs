import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
const root = path.resolve(process.argv[2] ?? '.');
const banned = /(^|\/)(node_modules|target(?:_[^/]+)?|bin|obj|dist|\.git|gemini-subagent|artifacts|\.gemini-subagent-backups|OpenUnpack-Archive|_workspace.*)(\/|$)|(^|\/)\.env(?:\.[^/]*)?$|\.(tsbuildinfo|vault(?:\.bak)?|sqlite(?:3)?(?:-wal|-shm)?|db|dpapi|pem|pfx|key)$/i;
const credentialPatterns = [
  /-----BEGIN (?:RSA |EC |OPENSSH )?PRIVATE KEY-----/,
  /\b(?:sk-[A-Za-z0-9_-]{24,}|gh[pousr]_[A-Za-z0-9]{30,}|AIza[0-9A-Za-z_-]{30,})\b/,
  /(?:api[_-]?key|access[_-]?token)\s*[=:]\s*["'][A-Za-z0-9_+\/-]{32,}["']/i,
];
let count = 0;
const fixturePrefix = 'tests/fixtures/upstream-archives/';
const fixtureManifestPath = path.join(root, fixturePrefix, 'provenance.json');
const fixtureManifest = fs.existsSync(fixtureManifestPath) ? JSON.parse(fs.readFileSync(fixtureManifestPath, 'utf8')) : undefined;
if (fixtureManifest && (fixtureManifest.repository !== 'https://github.com/adamhathcock/sharpcompress' || fixtureManifest.license !== 'MIT')) throw new Error('Unreviewed archive fixture provenance.');
const fixtures = new Map((fixtureManifest?.files ?? []).map(file => [fixturePrefix + file.file, file]));
function visit(dir) {
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const file = path.join(dir, entry.name), relative = path.relative(root, file).replaceAll('\\', '/');
    if (entry.isSymbolicLink() || banned.test(relative)) throw new Error(`Source release rejects path: ${relative}`);
    if (entry.isDirectory()) { visit(file); continue; }
    count++;
    if (fixtures.has(relative)) {
      const fixture = fixtures.get(relative), bytes = fs.readFileSync(file);
      if (bytes.length !== fixture.size || fixture.sha256 !== crypto.createHash('sha256').update(bytes).digest('hex')) throw new Error(`Fixture checksum mismatch: ${relative}`);
      continue;
    }
    if (/\.(png|ico|icns)$/i.test(file)) continue;
    const text = fs.readFileSync(file, 'utf8');
    if (credentialPatterns.some(pattern => pattern.test(text))) throw new Error(`Possible embedded credential in ${relative}; redact before packaging.`);
  }
}
visit(root);
for (const [relative] of fixtures) {
  if (!fs.existsSync(path.join(root, relative))) throw new Error(`Source release missing test fixture ${relative}`);
}
for (const required of ['LICENSE','README.md','SECURITY.md','CONTRIBUTING.md','Cargo.lock','apps/desktop-ui/package-lock.json','apps/desktop-host/tauri.conf.json','apps/desktop-host/installer.nsi','apps/desktop-host/installer-LICENSE-MIT.txt','scripts/build-desktop.ps1']) {
  if (!fs.existsSync(path.join(root, required))) throw new Error(`Source release missing ${required}`);
}
for (const required of ['crates/local-api/Cargo.toml', 'crates/local-api/assets/swagger-ui/index.html', 'examples/local-api.py', 'examples/local-api.ps1', 'assets/legal/swagger-ui/LICENSE', 'assets/legal/swagger-ui/NOTICE', 'apps/desktop-ui/eslint.config.js']) {
  if (!fs.existsSync(path.join(root, required))) throw new Error(`Source release missing API material ${required}`);
}
for (const required of ['apps/mcp-connector/package.json', 'apps/mcp-connector/package-lock.json', 'apps/mcp-connector/tsconfig.json', 'apps/mcp-connector/README.md', 'apps/mcp-connector/src/index.ts']) {
  if (!fs.existsSync(path.join(root, required))) throw new Error(`Source release missing MCP connector material ${required}`);
}
const swaggerProvenance = JSON.parse(fs.readFileSync(path.join(root, 'assets/legal/swagger-ui/provenance.json'), 'utf8'));
for (const asset of swaggerProvenance.assets) {
  if (!['swagger-ui.css', 'swagger-ui-bundle.js', 'swagger-ui-standalone-preset.js'].includes(asset.path)) throw new Error('Unexpected Swagger UI source asset.');
  const bytes = fs.readFileSync(path.join(root, 'crates/local-api/assets/swagger-ui', asset.path));
  if (crypto.createHash('sha256').update(bytes).digest('hex') !== asset.sha256) throw new Error(`Swagger UI source asset integrity mismatch: ${asset.path}`);
}
console.log(`Source release audit passed: ${count} allowlisted files; no gateway project, vault, database, private key or build outputs.`);
