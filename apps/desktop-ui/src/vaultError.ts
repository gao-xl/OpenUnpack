const PORTABLE_DISABLED_CODE = 'PORTABLE_VAULT_DISABLED';

type Translate = (key: string) => string;

/**
 * Turns backend vault errors into actionable copy.
 *
 * The portable-mode error is a configuration state, not a malfunction: the user
 * needs the cause and the way out, so the raw protocol string never reaches the
 * screen. Unknown errors pass through unchanged to keep diagnostics intact.
 */
export function humanizeVaultError(raw: string | null | undefined, t: Translate): string | null {
  if (!raw) return null;
  if (raw.includes(PORTABLE_DISABLED_CODE)) {
    return t('检测到便携密码库，但便携模式未开启。原因：应用只在显式启用时才读取密码库同目录的 .vault.key，防止他人放置密钥接管密码库。解决：设置环境变量 OPENUNPACK_PORTABLE_VAULT=1 后重启应用；若不再需要便携模式，可删除该 .vault.key，改用 Windows 账户加密。');
  }
  return raw;
}
