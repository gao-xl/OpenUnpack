/** Only deliberately safe messages cross the stdio boundary. */
export class ConnectorError extends Error {
  constructor(
    readonly code: string,
    message: string,
    readonly outcomeUncertain = false,
  ) {
    super(message);
    this.name = 'ConnectorError';
  }
}

export const DEFAULT_API_URL = 'http://127.0.0.1:47980';
const MAX_RESPONSE_BYTES = 20 * 1024 * 1024;

export function validateApiUrl(value: string): string {
  // Validate the spelling as well as the parsed host: reject alternative IP
  // encodings, userinfo, URL credentials, paths, queries, and fragments.
  if (!/^http:\/\/127\.0\.0\.1(?::[1-9][0-9]{0,4})?\/?$/.test(value)) {
    throw new ConnectorError('CONFIG_URL', 'OPENUNPACK_API_URL must be an http://127.0.0.1 origin, with an optional port.');
  }
  try {
    const url = new URL(value);
    if (url.hostname !== '127.0.0.1' || url.protocol !== 'http:') throw new Error();
    return url.origin;
  } catch {
    throw new ConnectorError('CONFIG_URL', 'OPENUNPACK_API_URL contains an invalid port or origin.');
  }
}

export interface Api {
  command(command: string, args?: Record<string, unknown>): Promise<unknown>;
  redact(text: string): string;
}

export class LocalApi implements Api {
  private readonly origin: string;
  constructor(
    private readonly token: string,
    url = DEFAULT_API_URL,
    private readonly timeoutMs = 30_000,
  ) {
    this.origin = validateApiUrl(url);
    if (!token || token.length > 4096 || /\s|[\x00-\x1f\x7f]/.test(token)) {
      throw new ConnectorError('CONFIG_TOKEN', 'Set OPENUNPACK_API_TOKEN to a dedicated OpenUnpack application key.');
    }
    if (!Number.isSafeInteger(timeoutMs) || timeoutMs < 1 || timeoutMs > 300_000) {
      throw new ConnectorError('CONFIG_TIMEOUT', 'The API timeout must be an integer between 1 and 300000 milliseconds.');
    }
  }

  redact(text: string): string {
    return text.split(this.token).join('[REDACTED]').replace(/Bearer\s+[^\s"<>]+/gi, 'Bearer [REDACTED]');
  }

  async command(command: string, args: Record<string, unknown> = {}): Promise<unknown> {
    const abort = new AbortController();
    const timer = setTimeout(() => abort.abort(), this.timeoutMs);
    try {
      const response = await fetch(`${this.origin}/api/v1/commands`, {
        method: 'POST',
        headers: { Authorization: `Bearer ${this.token}`, 'Content-Type': 'application/json' },
        body: JSON.stringify({ command, args }),
        redirect: 'manual',
        signal: abort.signal,
      });
      if (response.status >= 300 && response.status < 400) {
        await response.body?.cancel();
        throw new ConnectorError('API_REDIRECT', 'The API returned a redirect. Check the local API port; no redirect was followed.', true);
      }
      // Do not include the remote error body, status text, URL, or native fetch
      // exception: any of them could contain the application key or secrets.
      if (!response.ok) {
        await response.body?.cancel();
        const errors: Record<number, [string, string]> = {
          400: ['API_REJECTED', 'OpenUnpack rejected the operation. Check the selected tasks and desktop queue; an active desktop run or paused queue may need attention.'],
          401: ['API_AUTH', 'The application key is invalid or revoked. Create or configure a dedicated key in OpenUnpack.'],
          403: ['API_PERMISSION', 'The application lacks a required permission or directory approval. Approve the input, output, and enabled cache directories in OpenUnpack.'],
          404: ['API_NOT_FOUND', 'The API route or requested resource was not found. Check the OpenUnpack version and selected paths.'],
          409: ['API_BUSY', 'The desktop queue is busy or the selected task state changed. Check its status before retrying.'],
          429: ['API_BUSY', 'The API is busy. Query task status and retry the scoped operation later.'],
        };
        const [code, message] = errors[response.status] ?? ['API_FAILURE', 'The local API failed. Check the OpenUnpack desktop application.'];
        // A business error can follow earlier per-job database writes: an add
        // batch is not a transaction. Treat 400 as uncertain, just like 5xx.
        throw new ConnectorError(code, message, response.status === 400 || response.status >= 500);
      }
      const reader = response.body?.getReader();
      if (!reader) throw new ConnectorError('API_RESPONSE', 'The local API returned an empty response.', true);
      const chunks: Uint8Array[] = [];
      let bytes = 0;
      while (true) {
        const { done, value } = await reader.read();
        if (done) break;
        bytes += value.byteLength;
        if (bytes > MAX_RESPONSE_BYTES) {
          await reader.cancel();
          throw new ConnectorError('API_RESPONSE', 'The API response exceeded the connector size limit. Use a smaller batch.', true);
        }
        chunks.push(value);
      }
      let envelope: unknown;
      try { envelope = JSON.parse(Buffer.concat(chunks).toString('utf8')); } catch {
        throw new ConnectorError('API_RESPONSE', 'The API returned invalid JSON. Check the local API port.', true);
      }
      if (typeof envelope !== 'object' || envelope === null || !('data' in envelope) || 'error' in envelope) {
        throw new ConnectorError('API_RESPONSE', 'The API returned an unexpected response. Check the OpenUnpack version.', true);
      }
      return envelope.data;
    } catch (error) {
      if (error instanceof ConnectorError) throw error;
      if (abort.signal.aborted) {
        throw new ConnectorError('API_TIMEOUT', 'The local API timed out. A submitted operation may still be running; check the desktop queue before submitting again.', true);
      }
      throw new ConnectorError('API_UNAVAILABLE', 'Cannot reach the local OpenUnpack API. Keep OpenUnpack running and enable its local API. A submitted operation may still have been accepted.', true);
    } finally {
      clearTimeout(timer);
    }
  }
}

export function safeError(error: unknown): { code: string; message: string; outcomeUncertain: boolean } {
  if (error instanceof ConnectorError) {
    return { code: error.code, message: error.message, outcomeUncertain: error.outcomeUncertain };
  }
  return { code: 'CONNECTOR_FAILURE', message: 'The connector could not complete the operation. Check task status before retrying.', outcomeUncertain: true };
}
