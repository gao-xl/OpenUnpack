#!/usr/bin/env node
// TEMPORARY GATE — tracked as OD-001 in docs/decisions/OPEN-DECISIONS.md.
//
// Replace this file with eslint-plugin-jsx-a11y as soon as that package supports
// ESLint 10 (its 6.10.2 release stops at peer `eslint@^9`, while this project uses
// ESLint 10.12). Removal checklist, in one change set:
//   1. delete scripts/check-a11y.mjs and the `lint:a11y` script in package.json;
//   2. drop the extra `npm run lint:a11y` step from .github/workflows/windows.yml;
//   3. enable `jsx-a11y` recommended rules in eslint.config.js;
//   4. mark OD-001 RESOLVED with the plugin version that made this possible.
//
// Lightweight accessibility gate for the desktop UI, covering the two checks
// with the highest regression value using the TypeScript compiler already
// installed.
import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join, relative } from 'node:path';
import { createRequire } from 'node:module';

// TypeScript lives in the UI workspace, not at the repository root.
const requireFromUi = createRequire(new URL('../apps/desktop-ui/package.json', import.meta.url));
const ts = requireFromUi('typescript');

const ROOT = process.argv[2] ?? 'apps/desktop-ui/src';
const JSX_INTERACTIVE = new Set(['button', 'a', 'input', 'select', 'textarea', 'summary', 'option']);
const findings = [];

function walk(dir) {
  for (const entry of readdirSync(dir)) {
    const path = join(dir, entry);
    const stat = statSync(path);
    if (stat.isDirectory()) {
      if (entry !== 'node_modules') walk(path);
      continue;
    }
    if (path.endsWith('.tsx')) inspect(path);
  }
}

function attribute(node, name) {
  return node.attributes.properties.find(
    property => ts.isJsxAttribute(property) && property.name.getText() === name,
  );
}

function tagName(node) {
  return node.tagName.getText();
}

function report(file, node, rule, message) {
  const { line, character } = node.getSourceFile().getLineAndCharacterOfPosition(node.getStart());
  findings.push({ file, line: line + 1, column: character + 1, rule, message });
}

function inspect(file) {
  const source = ts.createSourceFile(file, readFileSync(file, 'utf8'), ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX);
  const visit = node => {
    if (ts.isJsxOpeningElement(node) || ts.isJsxSelfClosingElement(node)) {
      const tag = tagName(node);
      const lower = tag.toLowerCase();
      // Component-level widgets (Menu, Typography.Link, ...) usually render their
      // own keyboard handling; only native lowercase tags are judged here.
      const native = /^[a-z][a-z0-9]*$/.test(tag);
      if (lower === 'img' && !attribute(node, 'alt')) {
        report(file, node, 'a11y-img-alt', '<img> requires an alt attribute (use alt="" for decorative images).');
      }
      if (attribute(node, 'onClick')) {
        const hasKeyboardPath = attribute(node, 'role') || attribute(node, 'tabIndex') || attribute(node, 'onKeyDown');
        if (native && !JSX_INTERACTIVE.has(lower) && !hasKeyboardPath) {
          report(file, node, 'a11y-click-target', `<${tag}> has onClick but no role/tabIndex/onKeyDown; keyboard users cannot reach it.`);
        }
      }
    }
    ts.forEachChild(node, visit);
  };
  visit(source);
}

walk(ROOT);

for (const finding of findings) {
  const location = relative(process.cwd(), finding.file);
  const level = finding.rule === 'a11y-img-alt' ? 'error' : 'warning';
  console.log(`${location}:${finding.line}:${finding.column} ${level} ${finding.rule} ${finding.message}`);
}

const errors = findings.filter(finding => finding.rule === 'a11y-img-alt').length;
console.log(`a11y: ${errors} error(s), ${findings.length - errors} warning(s) across ${ROOT}`);
process.exit(errors > 0 ? 1 : 0);
