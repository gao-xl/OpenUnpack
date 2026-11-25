#!/usr/bin/env node
/**
 * MCP 连接器入口（stdio transport）。
 *
 * 环境变量：`OPENUNPACK_API_TOKEN`（鉴权令牌）、`OPENUNPACK_API_URL`
 * （默认本机 local-api 地址）、`OPENUNPACK_API_TIMEOUT_MS`（默认 30 秒）。
 * 启动失败时以 `{code}: {message}` 写 stderr 并返回退出码 1。
 */
import { serveStdio } from '@modelcontextprotocol/server/stdio';
import { LocalApi, DEFAULT_API_URL, safeError } from './api.js';
import { createServer } from './server.js';

try {
  const timeout =
    process.env.OPENUNPACK_API_TIMEOUT_MS === undefined
      ? 30_000
      : Number(process.env.OPENUNPACK_API_TIMEOUT_MS);
  const api = new LocalApi(
    process.env.OPENUNPACK_API_TOKEN ?? '',
    process.env.OPENUNPACK_API_URL ?? DEFAULT_API_URL,
    timeout,
  );
  serveStdio(() => createServer(api), {
    onerror: () => {
      process.stderr.write('OpenUnpack MCP transport error.\n');
    },
  });
} catch (error) {
  const failure = safeError(error);
  process.stderr.write(`${failure.code}: ${failure.message}\n`);
  process.exitCode = 1;
}
