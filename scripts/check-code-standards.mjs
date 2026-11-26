import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';

const root = path.resolve(import.meta.dirname, '..');

function walk(dir, matcher, results = []) {
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    if (['target', 'node_modules', 'dist', '.git', 'artifacts'].includes(entry.name)) continue;
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) walk(full, matcher, results);
    else if (matcher(full)) results.push(full);
  }
  return results;
}

function rel(full) {
  return path.relative(root, full).replace(/\\/g, '/');
}

function isProductionRust(relativePath) {
  if (!relativePath.endsWith('.rs')) return false;
  if (!relativePath.startsWith('crates/') && !relativePath.startsWith('apps/desktop-host/src/')) {
    return false;
  }
  if (/\/tests?\//.test(relativePath)) return false;
  if (/(?:^|\/)(?:tests|authorization_tests|native_tests)\.rs$/.test(relativePath)) return false;
  return true;
}

// 1. Verify Cargo workspace lint configuration across all crates.
const rootCargo = fs.readFileSync(path.join(root, 'Cargo.toml'), 'utf8');
assert(rootCargo.includes('[workspace.lints.rust]'), 'Cargo.toml missing [workspace.lints.rust]');
assert(
  rootCargo.includes('[workspace.lints.clippy]'),
  'Cargo.toml missing [workspace.lints.clippy]',
);

const memberManifests = [
  ...walk(path.join(root, 'crates'), (p) => path.basename(p) === 'Cargo.toml'),
  path.join(root, 'apps/desktop-host/Cargo.toml'),
];
for (const manifest of memberManifests) {
  const txt = fs.readFileSync(manifest, 'utf8');
  assert(
    /\[lints\]\s*\r?\n\s*workspace\s*=\s*true/.test(txt),
    `${rel(manifest)} must inherit [lints] workspace = true`,
  );
}

// 2. Enforce Rust production file budgets and forbidden lint suppressions.
const MAX_RUST_PRODUCTION_LINES = 950;
const TOO_MANY_ARGS_ALLOWED = new Set(['crates/infrastructure/src/recycle_shell.rs']);

const rustFiles = [
  ...walk(path.join(root, 'crates'), (p) => p.endsWith('.rs')),
  ...walk(path.join(root, 'apps/desktop-host/src'), (p) => p.endsWith('.rs')),
];

for (const file of rustFiles) {
  const relativePath = rel(file);
  if (!isProductionRust(relativePath)) continue;
  const content = fs.readFileSync(file, 'utf8');
  const lineCount = content.split(/\r?\n/).length;
  assert(
    lineCount <= MAX_RUST_PRODUCTION_LINES,
    `Rust production file exceeds ${MAX_RUST_PRODUCTION_LINES} lines (${lineCount}): ${relativePath}`,
  );
  assert(
    !content.includes('clippy::result_large_err'),
    `Forbidden clippy::result_large_err suppression in ${relativePath}`,
  );
  if (content.includes('clippy::too_many_arguments')) {
    assert(
      TOO_MANY_ARGS_ALLOWED.has(relativePath),
      `Unapproved clippy::too_many_arguments suppression in ${relativePath}`,
    );
  }
}

// 3. Enforce TypeScript / TSX / CSS budgets and AntD / ESLint standards.
const MAX_UI_LINES = 900;
const uiFiles = [
  ...walk(path.join(root, 'apps/desktop-ui/src'), (p) => /\.(?:ts|tsx|css)$/.test(p)),
  ...walk(path.join(root, 'apps/mcp-connector/src'), (p) => /\.ts$/.test(p)),
];

for (const file of uiFiles) {
  const relativePath = rel(file);
  const content = fs.readFileSync(file, 'utf8');
  const lineCount = content.split(/\r?\n/).length;
  assert(
    lineCount <= MAX_UI_LINES,
    `Frontend/TS production file exceeds ${MAX_UI_LINES} lines (${lineCount}): ${relativePath}`,
  );
  if (relativePath.endsWith('.tsx')) {
    assert(
      !/<Space\b[^>]*\bdirection=["']vertical["']/.test(content),
      `Deprecated <Space direction="vertical"> in ${relativePath}; use orientation="vertical"`,
    );
    assert(
      !/<Drawer\b[^>]*\bwidth=/.test(content),
      `Deprecated <Drawer width=...> in ${relativePath}; use size=...`,
    );
  }
}

const eslintConfig = fs.readFileSync(path.join(root, 'apps/desktop-ui/eslint.config.js'), 'utf8');
for (const rule of [
  'react-hooks/set-state-in-effect',
  'react-hooks/immutability',
  'react-hooks/purity',
  'react-hooks/refs',
]) {
  assert(
    new RegExp(`['"]${rule}['"]\\s*:\\s*['"]error['"]`).test(eslintConfig),
    `apps/desktop-ui/eslint.config.js must enforce '${rule}': 'error'`,
  );
}

console.log(
  `PASS code-standards: ${memberManifests.length} Cargo manifests, ${rustFiles.length} Rust files, ${uiFiles.length} TS/TSX/CSS files validated.`,
);
