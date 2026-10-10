import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
const [rootArg, metadataArg, outputArg] = process.argv.slice(2), offline = process.argv.includes('--offline');
if (!rootArg || !metadataArg || !outputArg) throw new Error('Expected root, Cargo metadata and output paths.');
const root = path.resolve(rootArg), output = path.resolve(outputArg);
if (!output.startsWith(path.join(root, 'artifacts') + path.sep)) throw new Error('Output must be inside project artifacts.');
const metadata = JSON.parse(fs.readFileSync(metadataArg, 'utf8'));
const sha = bytes => crypto.createHash('sha256').update(bytes).digest('hex');
const cargoChecksums = new Map(fs.readFileSync(path.join(root, 'Cargo.lock'), 'utf8').split(/\n\[\[package\]\]\r?\n/).flatMap(section => {
  const name = section.match(/^name = "([^"]+)"/m)?.[1], version = section.match(/^version = "([^"]+)"/m)?.[1], checksum = section.match(/^checksum = "([0-9a-f]+)"/m)?.[1];
  return name && version && checksum ? [[`${name}-${version}`, checksum]] : [];
}));
const records = [];
function save(relative, bytes) {
  const target = path.join(output, relative);
  fs.mkdirSync(path.dirname(target), { recursive: true });
  fs.writeFileSync(target, bytes);
  return { path: relative.replaceAll('\\', '/'), sha256: sha(bytes), size: bytes.length };
}
function copy(source, relative) { return save(relative, fs.readFileSync(source)); }
async function download(url, relative, expected) {
  const target = path.join(output, relative);
  let bytes = fs.existsSync(target) ? fs.readFileSync(target) : undefined;
  if (!bytes || (expected && sha(bytes) !== expected)) {
    if (offline) throw new Error(`Offline material not cached: ${relative}`);
    const response = await fetch(url, { signal: AbortSignal.timeout(45000), headers: { 'User-Agent': 'OpenUnpack-release-builder' } });
    if (!response.ok) throw new Error(`Upstream download HTTP ${response.status}: ${url}`);
    bytes = Buffer.from(await response.arrayBuffer());
  }
  if (expected && sha(bytes) !== expected) throw new Error(`Release material checksum mismatch: ${relative}`);
  return { ...save(relative, bytes), source: url };
}
const engine = JSON.parse(fs.readFileSync(path.join(root, 'tools/7zip/provenance.json'), 'utf8'));
if (engine.version !== '26.04' || sha(fs.readFileSync(path.join(root, 'tools/7zip/7z.dll'))) !== engine.dllSha256) throw new Error('Engine version/provenance mismatch.');
records.push({ component: '7-Zip', version: '26.04', license: 'LGPL-2.1-or-later AND BSD-2-Clause AND BSD-3-Clause AND LicenseRef-unRAR-restriction', files: [
  copy(path.join(root, 'tools/7zip/License.txt'), '7zip/License.txt'),
  await download('https://github.com/ip7z/7zip/releases/download/26.04/7z2604-src.7z', '7zip/7z2604-src.7z', '6bcfd2679d178174df06d09d50147dd7b5980d7beac896136689dfc9d51e0fd9'),
  await download('https://www.gnu.org/licenses/old-licenses/lgpl-2.1.txt', '7zip/COPYING.LESSER-2.1.txt'),
  copy(path.join(root, 'tools/7zip/provenance.json'), '7zip/binary-provenance.json'),
] });
// The self-contained publish and its notices must refer to the same runtime release.
const runtimeConfig = JSON.parse(fs.readFileSync(path.join(root, 'artifacts/worker/OpenUnpack.Worker.runtimeconfig.json'), 'utf8'));
const runtime = runtimeConfig.runtimeOptions.includedFrameworks?.find(f => f.name === 'Microsoft.NETCore.App')?.version;
if (runtime !== '10.0.9') throw new Error('Expected pinned self-contained .NET 10.0.9; update version and notice checksums together when upgrading.');
records.push({ component: '.NET Runtime', version: runtime, license: 'MIT and component-specific notices', files: [
  await download(`https://raw.githubusercontent.com/dotnet/runtime/v${runtime}/LICENSE.TXT`, 'dotnet/LICENSE.TXT', 'cfc21f5e8bd655ae997eec916138b707b1d290b83272c02a95c9f821b8c87310'),
  await download(`https://raw.githubusercontent.com/dotnet/runtime/v${runtime}/THIRD-PARTY-NOTICES.TXT`, 'dotnet/THIRD-PARTY-NOTICES.TXT', '66f1d4e44973185519bb4aa8a9718eb22fc7af2cc532e3ae9cfc4c127ee7fc54'),
] });
const nodes = new Map(metadata.resolve.nodes.map(n => [n.id, n])), reachable = new Set(), pending = [...metadata.workspace_members];
while (pending.length) {
  const id = pending.pop(); if (reachable.has(id)) continue; reachable.add(id);
  for (const dep of nodes.get(id)?.deps ?? []) pending.push(dep.pkg);
}
function repoUrl(repo) {
  const match = typeof repo === 'string' && repo.match(/github\.com[/:]([^/]+\/[^/#?]+?)(?:\.git)?(?:$|[/?#])/);
  return match?.[1]?.replace(/\.git$/, '');
}
async function upstreamLicense(repository, ref, destination) {
  const slug = repoUrl(repository); if (!slug) throw new Error(`No upstream GitHub license source for ${repository}`);
  for (const name of ['LICENSE', 'LICENSE-MIT', 'LICENSE.txt', 'LICENSE.md', 'LICENSE-MPL', 'license', 'COPYING']) {
    if (offline && !fs.existsSync(path.join(output, destination, name))) continue;
    try { return await download(`https://raw.githubusercontent.com/${slug}/${ref}/${name}`, `${destination}/${name}`); }
    catch (error) { if (offline || !String(error).includes('HTTP 404')) throw error; }
  }
  throw new Error(`Missing upstream license: ${slug} at ${ref}`);
}
const crates = metadata.packages.filter(p => reachable.has(p.id) && p.source).sort((a, b) => `${a.name}-${a.version}`.localeCompare(`${b.name}-${b.version}`));
for (const pkg of crates) {
  const directory = path.dirname(pkg.manifest_path), key = `${pkg.name}-${pkg.version}`, destination = `rust/${key}`, files = [];
  const licenseNames = fs.readdirSync(directory).filter(name => /^(licen[sc]e|notice|copying|copyright)/i.test(name) && fs.statSync(path.join(directory, name)).isFile());
  if (pkg.license_file && !licenseNames.includes(pkg.license_file)) licenseNames.push(pkg.license_file);
  for (const name of licenseNames) files.push(copy(path.join(directory, name), `${destination}/${name}`));
  if (!files.length) {
    if (pkg.name === 'selectors' && pkg.license === 'MPL-2.0') {
      // The exact upstream crate preserves MPL notices in its source headers but
      // no root license file. Supply the official license linked by those headers.
      files.push(await download('https://www.mozilla.org/media/MPL/2.0/index.txt', `${destination}/LICENSE-MPL-2.0.txt`));
    } else {
      const vcs = JSON.parse(fs.readFileSync(path.join(directory, '.cargo_vcs_info.json'), 'utf8'));
      files.push(await upstreamLicense(pkg.repository, vcs.git.sha1, destination));
    }
  }
  // Exact crates.io archives preserve MPL-covered source and original upstream notices.
  // Include all reachable Windows dependencies, including build/test tools, to simplify auditing.
  const registryDirectory = path.basename(path.dirname(directory));
  const cargoRoot = path.resolve(directory, '../../../..');
  const archive = path.join(cargoRoot, 'registry/cache', registryDirectory, `${key}.crate`);
  if (!fs.existsSync(archive)) throw new Error(`Missing exact Cargo source archive: ${key}`);
  if (sha(fs.readFileSync(archive)) !== cargoChecksums.get(key)) throw new Error(`Cargo source checksum differs from Cargo.lock: ${key}`);
  files.push(copy(archive, `${destination}/${key}.crate`));
  records.push({ component: pkg.name, ecosystem: 'cargo', version: pkg.version, license: pkg.license, repository: pkg.repository, files });
}
const ui = path.join(root, 'apps/desktop-ui'), lock = JSON.parse(fs.readFileSync(path.join(ui, 'package-lock.json'), 'utf8'));
const npmFallbacks = {
  '@ant-design/icons-svg': ['ant-design/ant-design-icons', 'master'],
  '@esbuild/win32-x64': ['evanw/esbuild', 'v0.28.2'],
  '@rollup/rollup-win32-x64-gnu': ['rollup/rollup', 'v4.64.2'],
  '@rollup/rollup-win32-x64-msvc': ['rollup/rollup', 'v4.64.2'],
  '@tauri-apps/cli-win32-x64-msvc': ['tauri-apps/tauri', 'dev'],
  'is-mobile': ['juliangruber/is-mobile', 'master'],
  'saxes': ['lddubeau/saxes', 'v6.0.0'],
  'stackback': ['shtylman/node-stackback', 'master'],
};
const seen = new Set();
for (const [relative, resolved] of Object.entries(lock.packages).sort(([a], [b]) => a.localeCompare(b))) {
  if (!relative) continue;
  const directory = path.join(ui, relative); if (!fs.existsSync(path.join(directory, 'package.json'))) continue;
  const pkg = JSON.parse(fs.readFileSync(path.join(directory, 'package.json'), 'utf8')), key = `${pkg.name}@${pkg.version}`;
  if (seen.has(key)) continue; seen.add(key);
  const destination = `npm/${pkg.name.replaceAll('/', '__')}@${pkg.version}`, files = [];
  for (const name of fs.readdirSync(directory).filter(name => /^(licen[sc]e|notice|copying|copyright)/i.test(name) && fs.statSync(path.join(directory, name)).isFile())) {
    files.push(copy(path.join(directory, name), `${destination}/${name}`));
  }
  if (!files.length) {
    const fallback = npmFallbacks[pkg.name]; if (!fallback) throw new Error(`No npm license material for ${key}`);
    files.push(await upstreamLicense(`https://github.com/${fallback[0]}`, fallback[1], destination));
  }
  records.push({ component: pkg.name, ecosystem: 'npm', version: pkg.version, license: pkg.license ?? resolved.license, repository: pkg.repository, development: !!resolved.dev, integrity: resolved.integrity, files });
}
save('manifest.json', Buffer.from(JSON.stringify({ formatVersion: 1, platform: 'Windows x64', records }, null, 2) + '\n'));
save('README.txt', Buffer.from('OpenUnpack dependency materials\n\nKeep this directory with binary redistribution. manifest.json lists exact component versions and material SHA-256 values. rust/*.crate are original source archives; 7zip/7z2604-src.7z is exact upstream source. .NET files correspond to the self-contained Worker runtime. npm contains installed production and development dependency notices; build tools are included conservatively. WebView2 runtime is a separate Microsoft component governed by its own terms.\n', 'utf8'));
console.log(`Collected ${records.length} dependency records, notices and exact Rust/7-Zip source materials.`);
