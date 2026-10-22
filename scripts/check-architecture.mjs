import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const metadata = JSON.parse(execFileSync('cargo', ['metadata', '--no-deps', '--format-version', '1', '--offline'], {
  cwd: root, encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'],
}));
const allowed = {
  'openunpack-protocol': [],
  'openunpack-domain': [],
  'openunpack-application': ['openunpack-domain', 'openunpack-protocol'],
  'openunpack-infrastructure': ['openunpack-application', 'openunpack-domain', 'openunpack-protocol'],
  'openunpack-core': ['openunpack-infrastructure', 'openunpack-protocol'],
  'openunpack-local-api': ['openunpack-protocol'],
  // ApiCommand is owned by local-api; the CLI remains an API client, never a Core host.
  'openunpack-cli': ['openunpack-protocol', 'openunpack-local-api'],
  'openunpack-desktop': ['openunpack-core', 'openunpack-application', 'openunpack-domain', 'openunpack-infrastructure', 'openunpack-protocol', 'openunpack-local-api'],
};
const bridges = {
  'crates/core/src/worker.rs': 'pub use openunpack_infrastructure::worker::client::*;',
  'crates/core/src/vault.rs': 'pub use openunpack_infrastructure::dpapi::vault::*;',
  'crates/core/src/paths.rs': 'pub use openunpack_infrastructure::paths::*;',
  'crates/core/src/route.rs': 'pub use openunpack_infrastructure::route::*;',
  'crates/core/src/database_lease.rs': 'pub use openunpack_infrastructure::sqlite::database_lease::*;',
  'crates/core/src/saferecycle.rs': 'pub(crate) use openunpack_infrastructure::saferecycle::*;',
};
const retired = [
  'crates/core/src/coordinator.rs',
  'crates/core/src/worker_manifest.rs',
  'crates/core/src/recycle_shell.rs',
  'crates/application/src/use_cases/coordinator.rs',
  'crates/application/src/use_cases/jobs.rs',
  'crates/application/src/ports/task_store.rs',
  'crates/infrastructure/src/sqlite/task_store.rs',
];
const files = new Map();
function visit(directory) {
  for (const entry of fs.readdirSync(path.join(root, directory), { withFileTypes: true })) {
    const file = `${directory}/${entry.name}`;
    if (entry.isDirectory()) visit(file);
    else if (file.endsWith('.rs')) files.set(file, fs.readFileSync(path.join(root, file), 'utf8'));
  }
}
for (const directory of fs.readdirSync(path.join(root, 'crates'))) {
  if (fs.existsSync(path.join(root, 'crates', directory, 'src'))) visit(`crates/${directory}/src`);
}
const edges = metadata.packages.flatMap(pkg => pkg.dependencies
  .filter(dependency => dependency.path)
  .map(dependency => [pkg.name, dependency.name]));
// Structural checks must inspect code, not names or calls quoted in comments/strings.
function rustCode(source) {
  let result = '';
  for (let i = 0; i < source.length;) {
    if (source.startsWith('//', i)) {
      const end = source.indexOf('\n', i);
      i = end < 0 ? source.length : end;
    } else if (source.startsWith('/*', i)) {
      let depth = 1;
      i += 2;
      while (i < source.length && depth) {
        if (source.startsWith('/*', i)) { depth++; i += 2; }
        else if (source.startsWith('*/', i)) { depth--; i += 2; }
        else i++;
      }
      result += ' ';
    } else {
      const character = source[i] === "'" || (source[i] === 'b' && source[i + 1] === "'")
        ? source.slice(i).match(/^b?'(?:\\(?:u\{[0-9a-fA-F_]+\}|x[0-9a-fA-F]{2}|.)|[^'\\\r\n])'/u) : null;
      if (character) {
        i += character[0].length;
        result += ' ';
        continue;
      }
      const raw = source[i] === 'r' || source[i] === 'b'
        ? source.slice(i).match(/^(?:br|r)(#*)"/) : null;
      if (raw) {
        const end = source.indexOf('"' + raw[1], i + raw[0].length);
        i = end < 0 ? source.length : end + 1 + raw[1].length;
        result += ' ';
      } else if (source[i] === '"') {
        i++;
        while (i < source.length) {
          if (source[i] === '\\') i += 2;
          else if (source[i++] === '"') break;
        }
        result += ' ';
      } else result += source[i++];
    }
  }
  return result;
}
const codeOnly = source => rustCode(source).replace(/\s/g, '');

function validate(sources, dependencies) {
  const code = new Map([...sources].map(([file, source]) => [file, rustCode(source)]));
  for (const [from, to] of dependencies) {
    assert(allowed[from]?.includes(to), `Forbidden dependency ${from} -> ${to}`);
  }
  assert(dependencies.some(([from, to]) => from === 'openunpack-core' && to === 'openunpack-infrastructure'), 'Core must use canonical Infrastructure implementations');
  for (const [file, expected] of Object.entries(bridges)) {
    assert.equal(codeOnly(sources.get(file) ?? ''), codeOnly(expected), `${file} must only re-export its canonical implementation`);
  }
  for (const file of retired) assert(!sources.has(file), `Retired pipeline returned: ${file}`);
  for (const [file, source] of sources) {
    assert(!/\b(?:TaskCoordinator|SqliteTaskStore|JobUseCases)\b|pub trait TaskStore\b/.test(code.get(file)), `Alternate task pipeline in ${file}`);
    assert(!/\binclude\s*!/.test(code.get(file)), `Rust source must not be compiled through include!: ${file}`);
    const pathAttributes = [...code.get(file).matchAll(/#\s*\[\s*(?:path\s*=|cfg_attr\s*\([^\]]*\bpath\s*=)/g)];
    if (pathAttributes.length) {
      assert(file === 'crates/core/src/repository.rs' && pathAttributes.length === 1
        && source.includes('#[path = "database_lease.rs"]'), `Unexpected module source alias in ${file}`);
    }
    if (file.startsWith('crates/core/src/coordinator/') && !file.endsWith('/tests.rs')) {
      assert(source.split('\n').length <= 1100, `Coordinator responsibility exceeds 1100 lines: ${file}`);
    }
  }
  const components = [
    'crates/core/src/coordinator/cache_budget.rs',
    'crates/core/src/coordinator/snapshot_projector.rs',
    'crates/core/src/coordinator/publication.rs',
  ];
  for (const file of components) {
    const source = code.get(file) ?? '';
    assert(source, `Missing coordinator component: ${file}`);
    assert(!/\bCoordinator\b/.test(source), `Component must not reference Coordinator: ${file}`);
    assert(!/\buse\s+super\s*::[^;]*\*/.test(source), `Component must import explicit parent contracts: ${file}`);
    assert(!/\bpub(?:\s*\([^)]*\))?\s+(?:reservations|unknown_owners|delta_cache|engine_probe|gate)\s*:/.test(source),
      `Component state must stay private: ${file}`);
  }
  const coordinator = code.get('crates/core/src/coordinator/mod.rs') ?? '';
  assert(!/\b(?:budget_reservations|unknown_budget_owners|snapshot_delta_cache|engine_probe|publish_gate)\s*:/.test(coordinator),
    'Extracted component state must not return to Coordinator');
  const owners = [
    [/pub struct WorkerClient\b/g, 'crates/infrastructure/src/worker/client.rs'],
    [/static UNCONFIRMED\b/g, 'crates/infrastructure/src/worker/client.rs'],
    [/pub struct Vault\b/g, 'crates/infrastructure/src/dpapi/vault.rs'],
    [/pub struct DatabaseLease\b/g, 'crates/infrastructure/src/sqlite/database_lease.rs'],
    [/struct BoundedRecords\b/g, 'crates/infrastructure/src/worker/manifest.rs'],
    [/pub fn relative\(/g, 'crates/infrastructure/src/paths.rs'],
    [/pub struct Coordinator\b/g, 'crates/core/src/coordinator/mod.rs'],
    [/pub struct Repository\b/g, 'crates/core/src/repository.rs'],
    [/pub\(super\) struct CacheBudgetManager\b/g, 'crates/core/src/coordinator/cache_budget.rs'],
    [/pub\(super\) struct BudgetClaim\b/g, 'crates/core/src/coordinator/cache_budget.rs'],
    [/pub\(super\) struct SnapshotProjector\b/g, 'crates/core/src/coordinator/snapshot_projector.rs'],
    [/pub\(super\) struct PublicationService\b/g, 'crates/core/src/coordinator/publication.rs'],
  ];
  for (const [pattern, owner] of owners) {
    const matches = [...code].flatMap(([file, source]) => [...source.matchAll(pattern)].map(() => file));
    assert.deepEqual(matches, [owner], `Implementation must have one owner: ${pattern}`);
  }
  const manifest = code.get('crates/infrastructure/src/worker/manifest.rs') ?? '';
  assert(/\bpaths\s*::\s*open_manifest_file\s*\(/.test(manifest), 'Manifest must use the canonical guarded file open');
}

validate(files, edges);
if (process.argv.includes('--self-test')) {
  assert.equal(codeOnly(`let quote = '"'; let byte = b'"'; let text = r#"// ignored"#; /* nested /* block */ comment */ fn keep<'a>() {}`), "letquote=;letbyte=;lettext=;fnkeep<'a>(){}");
  const rejects = (label, mutate) => {
    const sources = new Map(files), dependencies = edges.map(edge => [...edge]);
    mutate(sources, dependencies);
    assert.throws(() => validate(sources, dependencies), undefined, `Negative control missed: ${label}`);
    console.log(`PASS negative control: ${label}`);
  };
  rejects('reverse dependency', (_, dependencies) => dependencies.push(['openunpack-infrastructure', 'openunpack-core']));
  rejects('CLI bypasses API boundary', (_, dependencies) => dependencies.push(['openunpack-cli', 'openunpack-core']));
  rejects('worker fork in compatibility module', sources => sources.set('crates/core/src/worker.rs', sources.get('crates/core/src/worker.rs') + '\nstatic UNCONFIRMED: () = ();'));
  rejects('retired task pipeline', sources => sources.set(retired[3], 'pub struct TaskCoordinator;'));
  rejects('second manifest parser', sources => sources.set('crates/domain/src/fork.rs', 'struct BoundedRecords(usize);'));
  rejects('worker compiled again through path attribute', sources => sources.set('crates/core/src/lib.rs', sources.get('crates/core/src/lib.rs') + '\n#[path = "../../infrastructure/src/worker/client.rs"] mod fork;'));
  rejects('worker compiled again through include', sources => sources.set('crates/core/src/lib.rs', sources.get('crates/core/src/lib.rs') + '\ninclude!("../../infrastructure/src/worker/client.rs");'));
  rejects('weakened manifest open', sources => sources.set('crates/infrastructure/src/worker/manifest.rs', sources.get('crates/infrastructure/src/worker/manifest.rs').replaceAll('paths::open_manifest_file(', 'File::open(')));
  rejects('guarded call only in a comment', sources => sources.set('crates/infrastructure/src/worker/manifest.rs', sources.get('crates/infrastructure/src/worker/manifest.rs').replaceAll('paths::open_manifest_file(', 'File::open(') + '\n// paths::open_manifest_file(untrusted)\n'));
  rejects('coordinator monolith', sources => sources.set('crates/core/src/coordinator/commands.rs', '\n'.repeat(1101)));
  rejects('component holds coordinator', sources => sources.set('crates/core/src/coordinator/cache_budget.rs', sources.get('crates/core/src/coordinator/cache_budget.rs') + '\nstruct BackReference(Arc<Coordinator>);'));
  rejects('component imports grouped parent glob', sources => sources.set('crates/core/src/coordinator/snapshot_projector.rs', sources.get('crates/core/src/coordinator/snapshot_projector.rs') + '\nuse super::{*};'));
  rejects('component exposes state', sources => sources.set('crates/core/src/coordinator/publication.rs', sources.get('crates/core/src/coordinator/publication.rs').replace('    gate: Mutex<()>', '    pub(super) gate: Mutex<()>')));
  rejects('coordinator reclaims component state', sources => sources.set('crates/core/src/coordinator/mod.rs', sources.get('crates/core/src/coordinator/mod.rs') + '\nstruct ReintroducedState { budget_reservations: () }'));
}
console.log('PASS architecture: one production coordinator/store; canonical adapters; private budget/snapshot/publication components without Coordinator references; dependency direction and module budget.');
