import * as fs from 'node:fs/promises';
import path from 'node:path';
import { createHash, randomUUID } from 'node:crypto';

const LIMIT = 256 * 1024;
const SKIP = new Set(['.git', '.codex', '.agents', '.gemini-subagent-backups', 'node_modules', 'bin', 'obj', 'artifacts', '.vs']);
const sensitive = name => /^\.env(?:\.|$)/i.test(name) || /(?:\.dpapi|\.pem|\.key|\.pfx|\.p12|\.vault(?:\..*)?)$/i.test(name)
  || /^(?:credentials(?:\..*)?|secrets?(?:\..*)?|id_rsa|id_ed25519|config\.local\.json)$/i.test(name);
const hash = content => createHash('sha256').update(content).digest('hex');
const integer = (value, min, max, fallback) => {
  if (value === undefined) return fallback;
  if (!Number.isInteger(value) || value < min || value > max) throw new Error(`整数范围必须为 ${min}..${max}。`);
  return value;
};
const schema = (name, description, properties, required) => ({ type: 'function', function: {
  name, description, parameters: { type: 'object', properties, required, additionalProperties: false },
} });

export class Workspace {
  static async open(root, allowWrite = false) {
    const canonical = await fs.realpath(root);
    if (!(await fs.stat(canonical)).isDirectory()) throw new Error('工作区必须是目录。');
    return new Workspace(canonical, allowWrite);
  }
  constructor(root, allowWrite) { this.root = root; this.allowWrite = allowWrite; }

  contains(target) {
    const relative = path.relative(this.root, target);
    return relative === '' || (!relative.startsWith(`..${path.sep}`) && relative !== '..' && !path.isAbsolute(relative));
  }

  async resolve(relative = '.') {
    if (typeof relative !== 'string' || relative.includes('\0') || path.isAbsolute(relative)
        || relative.includes(':')) throw new Error('必须使用工作区相对路径。');
    const target = path.resolve(this.root, relative);
    if (!this.contains(target)) throw new Error('路径超出工作区。');
    for (const component of path.relative(this.root, target).split(path.sep)) {
      if (SKIP.has(component.toLowerCase()) || sensitive(component)) throw new Error('不允许访问凭据或内部目录。');
    }
    // Reject symbolic links/junctions, including links to locations inside the workspace.
    let current = this.root;
    for (const component of path.relative(this.root, target).split(path.sep).filter(Boolean)) {
      current = path.join(current, component);
      try {
        if ((await fs.lstat(current)).isSymbolicLink()) throw new Error('不允许通过符号链接访问文件。');
        if (!this.contains(await fs.realpath(current))) throw new Error('实际路径超出工作区。');
      } catch (error) { if (error.code !== 'ENOENT') throw error; }
    }
    return target;
  }

  definitions() {
    const tools = [
      schema('list_files', '列出工作区文件；自动跳过依赖、构建产物、凭据和链接。', {
        path: { type: 'string' }, limit: { type: 'integer', minimum: 1, maximum: 500 },
      }, []),
      schema('read_file', '读取 UTF-8 文件，返回行号和 SHA256；最大 256 KiB。', {
        path: { type: 'string' }, start_line: { type: 'integer', minimum: 1 },
        max_lines: { type: 'integer', minimum: 1, maximum: 400 },
      }, ['path']),
      schema('search_text', '按字面文本搜索文件，返回路径、行号与匹配行。', {
        query: { type: 'string' }, path: { type: 'string' },
        limit: { type: 'integer', minimum: 1, maximum: 100 },
      }, ['query']),
    ];
    if (this.allowWrite) tools.push(schema('write_file', '写入 UTF-8 文件。现有文件必须提供 read_file 返回的 SHA256；新文件传 null。覆盖前自动备份。', {
      path: { type: 'string' }, content: { type: 'string' },
      expected_sha256: { anyOf: [{ type: 'string' }, { type: 'null' }] },
    }, ['path', 'content', 'expected_sha256']));
    return tools;
  }

  async walk(relative = '.', limit = 500) {
    const start = await this.resolve(relative);
    const files = [];
    let visited = 0;
    let truncated = false;
    const visit = async dir => {
      const entries = await fs.readdir(dir, { withFileTypes: true });
      entries.sort((a, b) => a.name.localeCompare(b.name));
      for (const entry of entries) {
        if (++visited > 10000 || files.length >= limit) { truncated = true; return; }
        if (SKIP.has(entry.name.toLowerCase()) || sensitive(entry.name) || entry.isSymbolicLink()) continue;
        const full = path.join(dir, entry.name);
        if (entry.isDirectory()) await visit(full);
        else if (entry.isFile()) files.push(path.relative(this.root, full).split(path.sep).join('/'));
        if (truncated) return;
      }
    };
    const stat = await fs.stat(start);
    if (stat.isDirectory()) await visit(start);
    else if (stat.isFile()) files.push(path.relative(this.root, start).split(path.sep).join('/'));
    return { files, truncated };
  }

  async readBytes(relative) {
    const target = await this.resolve(relative);
    const stat = await fs.stat(target);
    if (!stat.isFile() || stat.size > LIMIT) throw new Error('只允许读取不超过 256 KiB 的普通文件。');
    const data = await fs.readFile(target);
    if (data.length > LIMIT || data.includes(0)) throw new Error('文件过大或为二进制文件。');
    // Fatal decoding prevents accidental corruption when a file uses another encoding.
    new TextDecoder('utf-8', { fatal: true }).decode(data);
    return data;
  }

  async execute(name, args, signal) {
    signal?.throwIfAborted();
    if (!args || typeof args !== 'object' || Array.isArray(args)) throw new Error('工具参数必须是对象。');
    if (name === 'list_files') return this.walk(args.path, integer(args.limit, 1, 500, 300));
    if (name === 'read_file') {
      const data = await this.readBytes(args.path);
      const lines = data.toString('utf8').split(/\r?\n/);
      const start = integer(args.start_line, 1, Number.MAX_SAFE_INTEGER, 1);
      const count = integer(args.max_lines, 1, 400, 200);
      return { path: args.path, sha256: hash(data), total_lines: lines.length,
        content: lines.slice(start - 1, start - 1 + count).map((line, i) => `${start + i}: ${line}`).join('\n'),
        truncated: start - 1 + count < lines.length };
    }
    if (name === 'search_text') {
      if (typeof args.query !== 'string' || !args.query.length || args.query.length > 1000) throw new Error('搜索词长度必须为 1..1000。');
      const limit = integer(args.limit, 1, 100, 30);
      const listing = await this.walk(args.path, 1000);
      const matches = [];
      let skipped = 0;
      for (const file of listing.files) {
        signal?.throwIfAborted();
        let data;
        try { data = await this.readBytes(file); } catch { skipped++; continue; }
        const lines = data.toString('utf8').split(/\r?\n/);
        for (let i = 0; i < lines.length; i++) {
          if (lines[i].includes(args.query)) matches.push({ path: file, line: i + 1, text: lines[i].slice(0, 2000) });
          if (matches.length >= limit) return { matches, truncated: true, skipped };
        }
      }
      return { matches, truncated: listing.truncated, skipped };
    }
    if (name === 'write_file') {
      if (!this.allowWrite) throw new Error('当前任务为只读模式。');
      if (typeof args.content !== 'string' || Buffer.byteLength(args.content) > LIMIT) throw new Error('写入内容必须是不超过 256 KiB 的字符串。');
      if (args.expected_sha256 !== null && !/^[a-f0-9]{64}$/.test(args.expected_sha256 ?? '')) throw new Error('需要 expected_sha256 或新文件的 null。');
      const target = await this.resolve(args.path);
      let previous;
      try { previous = await this.readBytes(args.path); } catch (error) { if (error.code !== 'ENOENT') throw error; }
      if ((previous ? hash(previous) : null) !== args.expected_sha256) throw new Error('文件已经变化；重新读取后再修改。');
      signal?.throwIfAborted();
      let backup;
      if (previous) {
        const backupDir = path.join(this.root, '.gemini-subagent-backups');
        try { if ((await fs.lstat(backupDir)).isSymbolicLink()) throw new Error('备份目录不能是符号链接。'); }
        catch (error) { if (error.code !== 'ENOENT') throw error; }
        await fs.mkdir(backupDir, { recursive: true });
        backup = path.join(backupDir, `${randomUUID()}.bak`);
        await fs.writeFile(backup, previous, { flag: 'wx' });
      }
      // Parent directories must exist. This avoids creating unexpected directory trees.
      const temporary = path.join(path.dirname(target), `.gemini-write-${randomUUID()}.tmp`);
      try {
        await fs.writeFile(temporary, args.content, { flag: 'wx' });
        await this.resolve(args.path);
        let current;
        try { current = await this.readBytes(args.path); } catch (error) { if (error.code !== 'ENOENT') throw error; }
        if ((current ? hash(current) : null) !== args.expected_sha256) throw new Error('写入前发现并发修改，已取消。');
        signal?.throwIfAborted();
        if (!previous) {
          // Hard link creates the final name exclusively, avoiding overwrite races for new files.
          await fs.link(temporary, target);
        } else await fs.rename(temporary, target);
      } finally { await fs.unlink(temporary).catch(() => {}); }
      return { path: args.path, sha256: hash(args.content), backup, bytes: Buffer.byteLength(args.content) };
    }
    throw new Error(`未知工具：${String(name).slice(0, 80)}`);
  }
}
