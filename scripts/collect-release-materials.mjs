import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import { execFileSync } from 'node:child_process';
const [rootArg, metadataArg, outputArg, ...optionArgs] = process.argv.slice(2);
if (!rootArg || !metadataArg || !outputArg) throw new Error('Expected root, Cargo metadata and output paths.');
let offline = false, workerDirectoryArg;
for (let index = 0; index < optionArgs.length; index++) {
  const option = optionArgs[index];
  if (option === '--offline') { offline = true; continue; }
  if (option === '--worker-directory') {
    const directory = optionArgs[++index];
    if (workerDirectoryArg !== undefined || !directory?.trim() || directory.startsWith('--')) {
      throw new Error('Expected exactly one directory after --worker-directory.');
    }
    workerDirectoryArg = directory;
    continue;
  }
  throw new Error(`Unknown release material option: ${option}`);
}
const root = path.resolve(rootArg), output = path.resolve(outputArg);
const workerDirectory = path.resolve(root, workerDirectoryArg ?? 'artifacts/worker');
if (!output.startsWith(path.join(root, 'artifacts') + path.sep)) throw new Error('Output must be inside project artifacts.');
const metadata = JSON.parse(fs.readFileSync(metadataArg, 'utf8'));
// Check the actual PE/deps identity before writing or downloading any release material.
const runtimePolicy = JSON.parse(fs.readFileSync(path.join(import.meta.dirname, 'runtime-policy.json'), 'utf8'));
const runtimeIdentity = JSON.parse(execFileSync('pwsh', ['-NoProfile', '-File', path.join(import.meta.dirname, 'verify-published-runtime.ps1'),
  '-WorkerDirectory', workerDirectory], { encoding: 'utf8', windowsHide: true }));
const runtime = runtimeIdentity.version;
const reviewedRuntime = runtimePolicy.reviewedRuntimes.find(entry => entry.version === runtime);
if (runtimeIdentity.status !== 'passed' || !reviewedRuntime) throw new Error('Published runtime has not passed the shared reviewed-runtime policy.');
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
// The upstream registry CDN intermittently resets connections on shared CI
// runners; retry transient transport failures before failing the build.
async function fetchWithRetry(url, attempts = 3) {
  let lastError;
  for (let attempt = 1; attempt <= attempts; attempt++) {
    try {
      return await fetch(url, { signal: AbortSignal.timeout(90000), headers: { 'User-Agent': 'OpenUnpack-release-builder' } });
    } catch (error) {
      lastError = error;
      if (attempt < attempts) await new Promise(resolve => setTimeout(resolve, attempt * 5000));
    }
  }
  throw lastError;
}
async function download(url, relative, expected) {
  const target = path.join(output, relative);
  let bytes = fs.existsSync(target) ? fs.readFileSync(target) : undefined;
  if (!bytes || (expected && sha(bytes) !== expected)) {
    if (offline) throw new Error(`Offline material not cached: ${relative}`);
    const response = await fetchWithRetry(url);
    if (!response.ok) throw new Error(`Upstream download HTTP ${response.status}: ${url}`);
    bytes = Buffer.from(await response.arrayBuffer());
  }
  if (expected && sha(bytes) !== expected) throw new Error(`Release material checksum mismatch: ${relative}`);
  return { ...save(relative, bytes), source: url };
}
const engine = JSON.parse(fs.readFileSync(path.join(root, 'tools/7zip/provenance.json'), 'utf8'));
if (engine.version !== '26.04' || sha(fs.readFileSync(path.join(root, 'tools/7zip/7z.dll'))) !== engine.dllSha256) throw new Error('Engine version/provenance mismatch.');
// The engine licence ships as release material and must match the hash recorded when it was
// extracted from the pinned MSI; an unverified copy must never enter the notices bundle.
const engineLicense = fs.readFileSync(path.join(root, 'tools/7zip/License.txt'));
if (sha(engineLicense) !== engine.licenseSha256) throw new Error('Engine License.txt does not match the pinned MSI provenance.');
records.push({ component: '7-Zip', version: '26.04', license: 'LGPL-2.1-or-later AND BSD-2-Clause AND BSD-3-Clause AND LicenseRef-unRAR-restriction', files: [
  save('7zip/License.txt', engineLicense),
  await download('https://github.com/ip7z/7zip/releases/download/26.04/7z2604-src.7z', '7zip/7z2604-src.7z', '6bcfd2679d178174df06d09d50147dd7b5980d7beac896136689dfc9d51e0fd9'),
  await download('https://www.gnu.org/licenses/old-licenses/lgpl-2.1.txt', '7zip/COPYING.LESSER-2.1.txt'),
  copy(path.join(root, 'tools/7zip/provenance.json'), '7zip/binary-provenance.json'),
] });
// The self-contained publish and its notices must refer to the same runtime release.
records.push({ component: '.NET Runtime', version: runtime, license: 'MIT and component-specific notices', files: [
  await download(`https://raw.githubusercontent.com/dotnet/runtime/v${runtime}/LICENSE.TXT`, 'dotnet/LICENSE.TXT', reviewedRuntime.licenseSha256),
  await download(`https://raw.githubusercontent.com/dotnet/runtime/v${runtime}/THIRD-PARTY-NOTICES.TXT`, 'dotnet/THIRD-PARTY-NOTICES.TXT', reviewedRuntime.thirdPartyNoticesSha256),
  save('dotnet/published-runtime-identity.json', Buffer.from(JSON.stringify(runtimeIdentity, null, 2) + '\n')),
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
async function upstreamLicense(repository, ref, destination, extraPaths = []) {
  const slug = repoUrl(repository); if (!slug) throw new Error(`No upstream GitHub license source for ${repository}`);
  for (const name of [...extraPaths, 'LICENSE', 'LICENSE-MIT', 'LICENSE.txt', 'LICENSE.md', 'LICENSE-MPL', 'LICENSE.BSD', 'LICENSE-MIT.txt', 'license', 'license.md', 'COPYING']) {
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
// Swagger UI is embedded as reviewed static assets, without a build-time ZIP
// extractor. Preserve upstream Apache notices and check the exact served bytes.
if (metadata.packages.some(pkg => pkg.name === 'openunpack-local-api')) {
  const source = path.join(root, 'assets/legal/swagger-ui');
  const provenance = JSON.parse(fs.readFileSync(path.join(source, 'provenance.json'), 'utf8'));
  const files = provenance.files.map(entry => {
    if (!['LICENSE', 'NOTICE'].includes(entry.path) || sha(fs.readFileSync(path.join(source, entry.path))) !== entry.sha256) throw new Error('Swagger UI legal material integrity mismatch.');
    return copy(path.join(source, entry.path), `swagger-ui/${entry.path}`);
  });
  files.push(copy(path.join(source, 'provenance.json'), 'swagger-ui/provenance.json'));
  for (const entry of provenance.assets) {
    if (!['swagger-ui.css', 'swagger-ui-bundle.js', 'swagger-ui-standalone-preset.js'].includes(entry.path)) throw new Error('Unexpected Swagger UI asset.');
    const asset = path.join(root, 'crates/local-api/assets/swagger-ui', entry.path);
    if (sha(fs.readFileSync(asset)) !== entry.sha256) throw new Error('Swagger UI static asset integrity mismatch.');
    files.push(copy(asset, `swagger-ui/dist/${entry.path}`));
  }
  records.push({ component: provenance.component, version: provenance.version, license: provenance.license, repository: provenance.repository, files });
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
    // These published packages keep their actual copyright/license notices
    // inside source or README files, rather than a separate LICENSE file.
    if (pkg.name === 'esrecurse') {
      const header = fs.readFileSync(path.join(directory, 'esrecurse.js'), 'utf8').match(/^\/\*[\s\S]*?\*\//)?.[0];
      if (!header?.includes('Redistribution and use') || !header.includes('THIS SOFTWARE IS PROVIDED')) throw new Error(`Missing embedded BSD notice: ${key}`);
      files.push(save(`${destination}/NOTICE.txt`, Buffer.from(header + '\n')));
    } else if (pkg.name === 'imurmurhash') {
      const readme = fs.readFileSync(path.join(directory, 'README.md'), 'utf8');
      if (!readme.includes('Permission is hereby granted') || !readme.includes('THE SOFTWARE IS PROVIDED')) throw new Error(`Missing embedded MIT notice: ${key}`);
      files.push(save(`${destination}/README-with-license.md`, Buffer.from(readme)));
    } else if (pkg.name === 'natural-compare') {
      const readme = fs.readFileSync(path.join(directory, 'README.md'), 'utf8');
      if (!readme.includes('Copyright (c) 2012-2015 Lauri Rooden') || !readme.includes('The MIT License')) throw new Error(`Missing publisher copyright: ${key}`);
      files.push(save(`${destination}/README-with-copyright.md`, Buffer.from(readme)));
      files.push(await download('https://raw.githubusercontent.com/spdx/license-list-data/v3.27.0/text/MIT.txt', `${destination}/MIT-license-template.txt`, 'b05785f9f18e6716bab63424b11454513b9943a222595b70411009202fc592b5'));
    }
  }
  if (!files.length) {
    const fallback = npmFallbacks[pkg.name];
    if (fallback) {
      files.push(await upstreamLicense(`https://github.com/${fallback[0]}`, fallback[1], destination));
    } else {
      // Generated packages may omit the root license. Resolve the exact published
      // revision, bound to this lockfile, instead of taking a moving default branch.
      const material = await download(`https://registry.npmjs.org/${encodeURIComponent(pkg.name)}/${encodeURIComponent(pkg.version)}`, `${destination}/registry-metadata.json`);
      const published = JSON.parse(fs.readFileSync(path.join(output, material.path), 'utf8'));
      const repository = typeof published.repository === 'string' ? published.repository : published.repository?.url;
      if (published.name !== pkg.name || published.version !== pkg.version
          || !resolved.integrity || published.dist?.integrity !== resolved.integrity
          || !/^[0-9a-f]{40}$/i.test(published.gitHead ?? '')) {
        throw new Error(`Unverified npm license revision for ${key}`);
      }
      files.push(material);
      const extraPaths = pkg.name === '@humanfs/types' ? ['packages/core/LICENSE'] : [];
      files.push(await upstreamLicense(repository, published.gitHead, destination, extraPaths));
    }
  }
  records.push({ component: pkg.name, ecosystem: 'npm', version: pkg.version, license: pkg.license ?? resolved.license, repository: pkg.repository, development: !!resolved.dev, integrity: resolved.integrity, files });
}
save('manifest.json', Buffer.from(JSON.stringify({ formatVersion: 1, platform: 'Windows x64', records }, null, 2) + '\n'));
save('README.txt', Buffer.from('OpenUnpack dependency materials\n\nKeep this directory with binary redistribution. manifest.json lists exact component versions and material SHA-256 values. rust/*.crate are original source archives; 7zip/7z2604-src.7z is exact upstream source. .NET files correspond to the self-contained Worker runtime. npm contains installed production and development dependency notices; build tools are included conservatively. WebView2 runtime is a separate Microsoft component governed by its own terms.\n', 'utf8'));
console.log(`Collected ${records.length} dependency records, notices and exact Rust/7-Zip source materials.`);
