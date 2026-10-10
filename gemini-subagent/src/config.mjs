import { readFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';

export async function loadConfig(env = process.env) {
  const config = JSON.parse(await readFile(fileURLToPath(new URL('../config.json', import.meta.url)), 'utf8'));
  config.baseUrl = env.GEMINI_BASE_URL || config.baseUrl;
  config.model = env.GEMINI_MODEL || config.model;
  config.apiKey = env.GEMINI_API_KEY;
  if (!config.apiKey?.trim()) throw new Error('缺少 GEMINI_API_KEY。Windows 请通过 run.ps1 启动。');
  const url = new URL(config.baseUrl);
  if (url.username || url.password || url.search || url.hash) throw new Error('API 地址不能包含凭据、查询参数或片段。');
  if (url.protocol !== 'https:' && !(url.protocol === 'http:' && ['localhost', '127.0.0.1', '[::1]'].includes(url.hostname))) {
    throw new Error('远程 API 必须使用 HTTPS。');
  }
  config.baseUrl = config.baseUrl.replace(/\/+$/, '');
  return config;
}
