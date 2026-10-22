import fs from 'node:fs';
import path from 'node:path';
import { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const ts = createRequire(path.join(root, 'apps/desktop-ui/package.json'))('typescript');
const files = { desktop: 'crates/protocol/src/desktop.rs', worker: 'crates/protocol/src/worker.rs', typescript: 'apps/desktop-ui/src/types.ts', csharp: 'src/OpenUnpack.Worker/Protocol.cs' };
const sources = Object.fromEntries(Object.entries(files).map(([key, file]) => [key, fs.readFileSync(path.join(root, file), 'utf8')]));
const fail = message => { throw new Error(message); };
const camel = value => value.replace(/_([a-z])/g, (_, c) => c.toUpperCase());

// Scope each declaration before inspecting fields. Names elsewhere cannot satisfy it.
function declarations(source, declaration) {
  const result = new Map();
  for (const match of source.matchAll(declaration)) {
    const start = source.indexOf('{', match.index + match[0].length);
    if (start < 0) fail(`Missing body: ${match[1]}`);
    let depth = 1, quote = false, escaped = false, comment = false, end = start + 1;
    for (; end < source.length && depth; end++) {
      const c = source[end];
      if (comment) { if (c === '\n') comment = false; continue; }
      if (quote) { if (escaped) escaped = false; else if (c === '\\') escaped = true; else if (c === '"') quote = false; continue; }
      if (c === '/' && source[end + 1] === '/') { comment = true; end++; continue; }
      if (c === '"') { quote = true; continue; }
      if (c === '{') depth++;
      if (c === '}') depth--;
    }
    if (depth || result.has(match[1])) fail(`Unbalanced or duplicate DTO: ${match[1]}`);
    result.set(match[1], { body: source.slice(start + 1, end - 1), prefix: source.slice(source.lastIndexOf('}', match.index) + 1, match.index), start, end });
  }
  return result;
}
function rustDTOs(source) {
  const result = declarations(source, /\bpub\s+struct\s+(\w+)\s*/g);
  for (const [name, dto] of result) {
    if (!dto.prefix.includes('rename_all = "camelCase"')) fail(`${name}: unsupported serde casing`);
    dto.fields = new Map();
    let attributes = '';
    for (const line of dto.body.split('\n')) {
      if (line.trim().startsWith('#[')) { attributes += line; continue; }
      const match = /^\s*pub\s+(\w+)\s*:\s*(.+),\s*$/.exec(line);
      if (!match) { if (line.trim() && !line.trim().startsWith('//')) fail(`${name}: unsupported field syntax ${line.trim()}`); continue; }
      const type = match[2].replace(/\s/g, ''), option = /^Option<(.+)>$/.exec(type);
      const withRule = /with\s*=\s*"([^"]+)"/.exec(attributes)?.[1];
      if (withRule && withRule !== 'decimal_revision') fail(`${name}.${match[1]}: unmodelled serde conversion`);
      dto.fields.set(camel(match[1]), { type: option ? option[1] : type, nullable: Boolean(option), omitNull: attributes.includes('skip_serializing_if = "Option::is_none"'), decimal: withRule === 'decimal_revision' });
      attributes = '';
    }
  }
  return result;
}
function rustType(type, target) {
  if (type === 'String') return 'string';
  if (type === 'bool') return target === 'ts' ? 'boolean' : 'bool';
  if (/^(u32|u64|usize|f64)$/.test(type)) return target === 'ts' ? 'number' : ({ u32: 'uint', u64: 'ulong', usize: 'usize', f64: 'double' }[type]);
  const list = /^Vec<(.+)>$/.exec(type);
  if (list) return target === 'ts' ? `${rustType(list[1], target)}[]` : `List<${rustType(list[1], target)}>`;
  const map = /^BTreeMap<String,(.+)>$/.exec(type);
  if (map && target === 'ts') return `Record<string,${rustType(map[1], target)}>`;
  if (/^[A-Z]\w*$/.test(type)) return type;
  fail(`Unsupported Rust wire type: ${type}`);
}
function tsType(node, unions) {
  if (ts.isLiteralTypeNode(node)) {
    if (node.literal.kind === ts.SyntaxKind.NullKeyword) return 'null';
    if (ts.isStringLiteral(node.literal)) return `'${node.literal.text}'`;
    fail(`Unsupported TS literal type: ${node.getText()}`);
  }
  const primitive = new Map([[ts.SyntaxKind.StringKeyword, 'string'], [ts.SyntaxKind.NumberKeyword, 'number'], [ts.SyntaxKind.BooleanKeyword, 'boolean']]);
  if (primitive.has(node.kind)) return primitive.get(node.kind);
  if (ts.isArrayTypeNode(node)) return `${tsType(node.elementType, unions)}[]`;
  if (ts.isTypeReferenceNode(node)) {
    const args = node.typeArguments ? [...node.typeArguments].map(arg => tsType(arg, unions)) : [], domain = unions.get(node.typeName.getText());
    if (domain && !args.length) return domain.join('|');
    return node.typeName.getText() + (args.length ? `<${args.join(',')}>` : '');
  }
  if (ts.isUnionTypeNode(node)) return [...new Set(node.types.map(part => tsType(part, unions)))].sort().join('|');
  fail(`Unsupported TS wire type: ${node.getText()}`);
}
// Literal unions (`type X = 'a' | 'b'`) mirror serde camelCase enums. Expanding aliases
// keeps the exact value domain so it can be compared against the Rust enum below.
function tsLiteralUnions(source) {
  const ast = ts.createSourceFile(files.typescript, source, ts.ScriptTarget.Latest, true, ts.ScriptKind.TS), result = new Map();
  for (const node of ast.statements) {
    if (!ts.isTypeAliasDeclaration(node)) continue;
    if (!ts.isUnionTypeNode(node.type)) fail(`${node.name.text}: unsupported type alias`);
    result.set(node.name.text, node.type.types.map(member => {
      if (!ts.isLiteralTypeNode(member) || member.literal.kind !== ts.SyntaxKind.StringLiteral) fail(`${node.name.text}: unsupported union member`);
      return `'${member.literal.text}'`;
    }).sort());
  }
  return result;
}
// Rust `pub enum` with serde camelCase is the wire domain owner: every variant must have
// a matching TS literal, and no extra TS literal may exist. Any other casing is rejected.
function rustEnums(source) {
  const result = new Map();
  for (const [name, dto] of declarations(source, /\bpub\s+enum\s+(\w+)\s*/g)) {
    if (!dto.prefix.includes('rename_all = "camelCase"')) fail(`${name}: unsupported serde casing`);
    const values = [];
    for (const line of dto.body.split('\n')) {
      const trimmed = line.trim();
      if (!trimmed || trimmed.startsWith('//')) continue;
      if (trimmed.startsWith('#[')) fail(`${name}: unsupported variant attribute`);
      const match = /^([A-Z]\w*)\s*,?$/.exec(trimmed);
      if (!match) fail(`${name}: unsupported enum variant ${trimmed}`);
      values.push(match[1][0].toLowerCase() + match[1].slice(1));
    }
    if (!values.length || new Set(values).size !== values.length) fail(`${name}: empty or duplicate variants`);
    result.set(name, values);
  }
  return result;
}
function tsDTOs(source) {
  const ast = ts.createSourceFile(files.typescript, source, ts.ScriptTarget.Latest, true, ts.ScriptKind.TS), result = new Map();
  if (ast.parseDiagnostics.length) fail('TS parse errors');
  const unions = tsLiteralUnions(source);
  for (const node of ast.statements) {
    if (!ts.isInterfaceDeclaration(node)) continue;
    if (node.heritageClauses?.length) fail(`Unmodelled inherited DTO ${node.name.text}`);
    const fields = new Map();
    for (const member of node.members) {
      if (!ts.isPropertySignature(member) || !member.type) fail(`Unsupported member ${node.name.text}`);
      const name = member.name.getText();
      if (fields.has(name)) fail(`Duplicate TS field ${node.name.text}.${name}`);
      fields.set(name, { type: tsType(member.type, unions), optional: Boolean(member.questionToken), node: member });
    }
    result.set(node.name.text, { fields, node });
  }
  return result;
}
function csharpDTOs(source) {
  const result = declarations(source, /\bpublic\s+sealed\s+record\s+(\w+)\s*/g);
  for (const [name, dto] of result) {
    dto.fields = new Map();
    const properties = /((?:\[[^\]]+\]\s*)*)public\s+([A-Za-z_]\w*(?:<[^>]+>)?\??)\s+(\w+)\s*\{\s*get;\s*init;\s*\}/g;
    for (const match of dto.body.matchAll(properties)) {
      const nullable = match[2].endsWith('?');
      dto.fields.set(match[3][0].toLowerCase() + match[3].slice(1), { type: nullable ? match[2].slice(0, -1) : match[2], nullable, omitNull: /JsonIgnore[^\]]*WhenWritingNull/.test(match[1]) });
    }
    // Records may carry a redacted `ToString()` override; count it as a recognized
    // member so the public-surface sweep stays strict for every other declaration.
    const toStringOverrides = [...dto.body.matchAll(/\bpublic\s+override\s+string\s+ToString\s*\(\s*\)/g)].length;
    if (!dto.fields.size || dto.fields.size + toStringOverrides !== [...dto.body.matchAll(/\bpublic\s+/g)].length) fail(`${name}: unsupported/duplicate C# properties`);
  }
  return result;
}
const exceptions = [
  'PasswordInput.id/secret are input Option<String>: omission is accepted, and explicit null must remain in the TS union.',
  'Output serde(default) supports reading old snapshots; current TS output fields must be present when Rust emits them.',
  'AppSnapshot.revision is Rust u64 with decimal_revision wire string; TS must require string.',
  'WorkerCapabilities.maxBatchCandidates Rust usize / C# int is an explicit existing range difference; default 128 fits both, runtime matrix covers limits.',
  'TS number models JSON numeric fields, not arbitrary u64 arithmetic precision; revision text is checked separately.',
];
function equalFields(name, expected, actual) {
  for (const key of expected.keys()) if (!actual.has(key)) fail(`${name}.${key}: missing target field`);
  for (const key of actual.keys()) if (!expected.has(key)) fail(`${name}.${key}: extra target field`);
}
function check(source) {
  const rustLimit = /pub const MAX_CONCURRENT_JOBS:\s*u32\s*=\s*(\d+);/.exec(source.desktop)?.[1];
  const tsLimit = /export const MAX_CONCURRENT_JOBS\s*=\s*(\d+);/.exec(source.typescript)?.[1];
  if (!rustLimit || rustLimit !== tsLimit) fail('MAX_CONCURRENT_JOBS: Rust and TS limits must match');
  const desktop = rustDTOs(source.desktop), worker = rustDTOs(source.worker), typescript = tsDTOs(source.typescript), csharp = csharpDTOs(source.csharp);
  const domains = rustEnums(source.desktop);
  let fields = 0;
  for (const [name, dto] of desktop) {
    const target = typescript.get(name);
    if (!target) fail(`Missing TS interface ${name}`);
    equalFields(name, dto.fields, target.fields);
    for (const [field, definition] of dto.fields) {
      const actual = target.fields.get(field);
      // Enum-backed fields compare against the exact variant domain; a TS union may only
      // exist when the Rust side owns it as a serde enum, never as an unconstrained String.
      const members = domains.has(definition.type) ? domains.get(definition.type).map(value => `'${value}'`).sort() : [definition.decimal ? 'string' : rustType(definition.type, 'ts')];
      const expected = definition.nullable ? [...members, 'null'].sort().join('|') : members.join('|');
      if (actual.type !== expected) fail(`${name}.${field}: TS ${actual.type}, expected ${expected}`);
      const optional = definition.omitNull || (name === 'PasswordInput' && definition.nullable);
      if (actual.optional !== optional) fail(`${name}.${field}: optional=${actual.optional}, expected ${optional}`);
      fields++;
    }
  }
  for (const name of typescript.keys()) if (!desktop.has(name)) fail(`TS ${name} has no Rust desktop DTO`);
  for (const [name, dto] of worker) {
    const target = csharp.get(name);
    if (!target) fail(`Missing C# record ${name}`);
    equalFields(name, dto.fields, target.fields);
    for (const [field, definition] of dto.fields) {
      const actual = target.fields.get(field);
      let expected = rustType(definition.type, 'cs');
      if (name === 'WorkerCapabilities' && field === 'maxBatchCandidates' && expected === 'usize') expected = 'int';
      if (actual.type !== expected || actual.nullable !== definition.nullable) fail(`${name}.${field}: C# ${actual.type}${actual.nullable ? '?' : ''}, expected ${expected}${definition.nullable ? '?' : ''}`);
      if (actual.omitNull !== definition.omitNull) fail(`${name}.${field}: null serialization omission differs`);
      fields++;
    }
  }
  for (const name of csharp.keys()) if (!worker.has(name)) fail(`C# ${name} has no Rust worker DTO`);
  return { desktop: desktop.size, worker: worker.size, fields };
}
const result = check(sources);
if (process.argv.includes('--self-test')) {
  const revision = tsDTOs(sources.typescript).get('AppSnapshot').fields.get('revision').node;
  const withoutRevision = sources.typescript.slice(0, revision.pos) + sources.typescript.slice(revision.end);
  const request = declarations(sources.csharp, /\bpublic\s+sealed\s+record\s+(\w+)\s*/g).get('WorkerRequest');
  const changed = request.body.replace('public ulong? Generation', 'public string? Generation');
  if (changed === request.body) fail('Generation mutation could not be constructed');
  const wrongGeneration = sources.csharp.slice(0, request.start + 1) + changed + sources.csharp.slice(request.end - 1);
  for (const [name, modified, expected] of [
    ['MAX_CONCURRENT_JOBS drift', { ...sources, typescript: sources.typescript.replace(/MAX_CONCURRENT_JOBS\s*=\s*\d+;/, 'MAX_CONCURRENT_JOBS = 99;') }, 'MAX_CONCURRENT_JOBS'],
    ['AppSnapshot.revision removed', { ...sources, typescript: withoutRevision }, 'AppSnapshot.revision'],
    ['WorkerRequest.Generation ulong? -> string?', { ...sources, csharp: wrongGeneration }, 'WorkerRequest.generation'],
    ['NodeView.parentId null removed', { ...sources, typescript: sources.typescript.replace('parentId: string | null', 'parentId: string') }, 'NodeView.parentId'],
    ['WorkerEvent.hello null omission removed', { ...sources, csharp: sources.csharp.replace(/\[System.Text.Json.Serialization.JsonIgnore\([^\]]+WhenWritingNull[^\]]*\)\]/, '') }, 'WorkerEvent.hello'],
    ['ConflictStrategy union narrowed', { ...sources, typescript: sources.typescript.replace("export type ConflictStrategy = 'rename' | 'skip' | 'error';", "export type ConflictStrategy = 'rename' | 'skip';") }, 'ToolboxOptions.conflictStrategy'],
    ['ToolboxKind union member renamed', { ...sources, typescript: sources.typescript.replace("'normalizeExtensions' | 'batchRename' | 'flattenDirectory'", "'normalizeExtensions' | 'batchRename' | 'flattenDir'") }, 'ToolboxOptions.kind'],
  ]) {
    let rejected = false;
    if (!Object.keys(modified).some(key => modified[key] !== sources[key])) fail(`Mutation could not be constructed: ${name}`);
    try { check(modified); } catch (error) { if (!error.message.includes(expected)) throw error; rejected = true; }
    if (!rejected) fail(`Mutation not rejected: ${name}`);
    console.log(`PASS mutation: ${name}`);
  }
}
console.log(`PASS scoped contracts: ${result.desktop} desktop DTOs, ${result.worker} worker DTOs, ${result.fields} fields`);
for (const exception of exceptions) console.log(`Compatibility: ${exception}`);
