import { describe, expect, it } from 'vitest';
import { humanizeVaultError } from '../src/vaultError';

const t = (key: string) => key;

describe('humanizeVaultError', () => {
  it('replaces the portable-mode error with cause and resolution', () => {
    const raw = 'PORTABLE_VAULT_DISABLED: portable vault detected but portable mode is disabled; set OPENUNPACK_PORTABLE_VAULT=1 to open it.';
    const message = humanizeVaultError(raw, t);
    expect(message).toContain('OPENUNPACK_PORTABLE_VAULT=1');
    expect(message).toContain('.vault.key');
    expect(message).not.toContain('PORTABLE_VAULT_DISABLED');
  });

  it('keeps unrelated vault errors untouched for diagnostics', () => {
    const raw = 'Vault storage is corrupted or unreadable';
    expect(humanizeVaultError(raw, t)).toBe(raw);
  });

  it('returns null when there is no error', () => {
    expect(humanizeVaultError(null, t)).toBeNull();
    expect(humanizeVaultError(undefined, t)).toBeNull();
    expect(humanizeVaultError('', t)).toBeNull();
  });
});
