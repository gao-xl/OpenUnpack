import { setTimeout as delay } from 'node:timers/promises';

export class Gateway {
  constructor(config, fetchImpl = fetch) {
    this.config = config;
    this.fetch = fetchImpl;
  }

  async request(endpoint, body, signal) {
    for (let attempt = 0; attempt < 3; attempt++) {
      signal?.throwIfAborted();
      const timeout = AbortSignal.timeout(this.config.requestTimeoutMs);
      const requestSignal = signal ? AbortSignal.any([signal, timeout]) : timeout;
      let response;
      try {
        response = await this.fetch(`${this.config.baseUrl}/${endpoint}`, {
          method: body === undefined ? 'GET' : 'POST',
          headers: { Authorization: `Bearer ${this.config.apiKey}`, 'Content-Type': 'application/json' },
          body: body === undefined ? undefined : JSON.stringify(body),
          signal: requestSignal,
          redirect: 'error',
        });
      } catch {
        signal?.throwIfAborted();
        throw new Error(timeout.aborted ? '网关请求超时。' : '网关连接失败，请检查地址和网络。');
      }
      if (!response.ok) {
        // Do not put provider error bodies (possibly containing prompts/keys) into logs.
        const retryable = response.status === 429 || [500, 502, 503, 504].includes(response.status);
        await response.body?.cancel();
        if (retryable && attempt < 2) {
          const retryAfter = response.headers.get('retry-after');
          const seconds = Number(retryAfter);
          const dateMs = Date.parse(retryAfter);
          const pause = retryAfter && Number.isFinite(seconds) ? seconds * 1000
            : Number.isFinite(dateMs) ? dateMs - Date.now() : 500 * 2 ** attempt;
          await delay(Math.min(10000, Math.max(0, pause)), undefined, { signal });
          continue;
        }
        throw new Error(`网关 HTTP ${response.status}；请检查认证、模型及服务状态。`);
      }
      try { return await response.json(); }
      catch { throw new Error('网关返回了无效或未完整接收的 JSON。'); }
    }
  }

  chat(messages, tools, signal) {
    return this.request('chat/completions', {
      model: this.config.model,
      messages,
      tools,
      tool_choice: 'auto',
      stream: false,
      max_tokens: this.config.maxOutputTokens,
    }, signal);
  }

  async models(signal) {
    const payload = await this.request('models', undefined, signal);
    if (!Array.isArray(payload.data)) throw new Error('模型列表格式无效。');
    return payload.data.map(item => item.id).filter(id => typeof id === 'string' && /gemini/i.test(id));
  }
}
