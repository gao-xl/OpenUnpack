#!/usr/bin/env node
import { serveStdio } from '@modelcontextprotocol/server/stdio';
import { LocalApi, DEFAULT_API_URL, safeError } from './api.js';
import { createServer } from './server.js';

try {
  const timeout = process.env.OPENUNPACK_API_TIMEOUT_MS === undefined
    ? 30_000 : Number(process.env.OPENUNPACK_API_TIMEOUT_MS);
  const api = new LocalApi(process.env.OPENUNPACK_API_TOKEN ?? '', process.env.OPENUNPACK_API_URL ?? DEFAULT_API_URL, timeout);
  serveStdio(() => createServer(api), {
    onerror: () => { process.stderr.write('OpenUnpack MCP transport error.\n'); },
  });
} catch (error) {
  const failure = safeError(error);
  process.stderr.write(`${failure.code}: ${failure.message}\n`);
  process.exitCode = 1;
}
