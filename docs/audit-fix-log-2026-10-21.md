# 已知问题修复记录（2026-10-21）

> 输入：`docs/audit-report-final-2026-10-20.md` 第 2 节问题清单与结转项
> 决策方式：按用户指定的三条推荐路径执行并补齐证据
> 标的：工作树（HEAD `cf6e344`，并行会话持续写入）

---

## 1. P2-12 便携保险库密钥可被替换/拷贝

**根因**（`crates/infrastructure/src/dpapi/vault.rs`）

```rust
// 修复前
let use_portable = env::var("OPENUNPACK_PORTABLE_VAULT").is_ok()
    || env::var("OPENUNPACK_VAULT_KEY").is_ok()
    || path.parent().map(|p| p.join(".vault.key").exists()).unwrap_or(false); // 仅凭存在即切换
```

`resolve_vault_key` 同样无条件读取 `.vault.key`。任何能写应用数据目录的进程放入自选密钥，即可让后续保存使用攻击者已知的密钥；读取时也无来源校验。

**影响范围**：Windows 便携模式与备份导入导出；不影响 DPAPI 默认路径。定级 P2（前提：攻击者已可写数据目录）。

**改动**

| 点 | 改动 |
|----|------|
| `portable_vault_opted_in()` | 新增：Windows 仅认 `OPENUNPACK_PORTABLE_VAULT` 或 `OPENUNPACK_VAULT_KEY`；非 Windows 恒为真 |
| `vault_encrypt` | 删除"存在 `.vault.key` 即便携"的隐式分支 |
| `vault_decrypt` | 数据以 `OPENVLT1` 开头且未启用时返回明确错误，不再静默用外部密钥解密 |
| `resolve_vault_key` | 读取前校验 opt-in；密钥必须是普通文件（拒绝符号链接/目录） |

**兼容性**：既有便携库升级后首次打开失败并提示设置变量，属有意的 fail-closed；按提示操作即可恢复，无需迁移。

**测试**：`portable_vault_tests.rs` 从"无环境变量可重开"改为"无 opt-in 必须拒绝且错误含变量名，显式开启后成功"。

### 1.1 配套：UI 层可操作提示（本轮新增）

后端此前只返回英文原始错误，用户无法判断该做什么。现在：

| 层 | 改动 |
|----|------|
| 后端 | 错误串加稳定前缀 `PORTABLE_VAULT_DISABLED:`，并导出常量 `PORTABLE_VAULT_DISABLED_CODE` |
| 前端 | 新增 `src/vaultError.ts::humanizeVaultError()`：识别该前缀，替换为「原因 + 解决路径」的中文提示（说明为何 fail-closed；给出 `OPENUNPACK_PORTABLE_VAULT=1` 与删除 `.vault.key` 两条出路） |
| 接线 | `App.tsx` 的密码库告警条与统一 `run()` 错误出口都经过该函数；未知错误原样透传，保留诊断信息 |

**验证**：`apps/desktop-ui/tests/vault-error.test.ts` 三条断言（映射、透传、空值）；`tsc -b` 与 `eslint` 通过。运行时单测受 esbuild 环境限制，需 CI 执行。

---

## 2. P2-09 解压口令进入 7-Zip 命令行

**根因**（`crates/infrastructure/src/engine/cli_engine.rs`）

```rust
cmd.arg(format!("-p{}", pwd));   // inspect 与 extract 两处
```

Windows 上同用户进程可经 `Win32_Process.CommandLine`、ETW 或 Process Explorer 读取完整命令行，无需提权。

**影响范围**：所有加密归档的检查、解压、密码库自动匹配与重试链路。

### 2.1 实测：7-Zip 是否接受 stdin 口令（本轮补齐）

**方法**：使用官方 `7zr.exe`（单文件，不依赖 DLL，可绕过本机对未签名 DLL 的加载限制）与官方完整版 `7za.exe`（从 `7z2604-extra.7z` 解出），构造加密 7z，按四种字节序列写入 stdin，并以 argv 与"无口令"两组作对照。脚本：`artifacts/p2-09-stdin-proof/probe_stdin.py`。

| 用例 | 7zr 26.04 | 7za 26.04 | 结果 |
|------|-----------|-----------|------|
| argv `-pSECRET`（对照） | exit 0，已解出 | exit 0，已解出 | 可用 |
| stdin `SECRET\n` | exit 2 | exit 2 | 失败 |
| stdin `SECRET\r\n` | exit 2 | exit 2 | 失败 |
| stdin `SECRET`（无换行） | exit 2 | exit 2 | 失败 |
| stdin `WRONG\n` | exit 2 | exit 2 | 预期失败 |
| `-p-`（无口令） | exit 2 | exit 2 | 预期失败 |

原始记录：`artifacts/p2-09-stdin-proof/stdin-probe-result.json`（7zr）与 `stdin-probe-result-7za.json`（7za）。
**结论：7-Zip 26.04 不读取重定向 stdin 的口令，只在控制台提示时读取。** 前一版把 stdin 设为默认会直接破坏所有加密解压，必须回退。

### 2.2 修复（按实测结果调整）

| 点 | 改动 |
|----|------|
| `push_password()` | 统一构造开关：stdin 模式只加 `-p`（无值），argv 模式保留原行为 |
| `run_7z()` | stdin 模式写入口令并关闭管道，`kill_on_drop` + 30s 超时防止卡在控制台提示 |
| 默认策略 | **默认 argv**（与实测一致）；仅当 `OPENUNPACK_7Z_STDIN=1` 时尝试 stdin，失败自动回退并把结论缓存到 `STDIN_PASSWORD` |
| 注释 | 在常量处记录实测日期、版本与证据路径，避免后续误改回 stdin 默认 |

**测试**（本地通过，6/6）

```
cargo test -p openunpack-infrastructure --lib engine::cli_engine::
```

- `stdin_mode_keeps_the_password_out_of_argv`：stdin 模式 argv 只有 `-p`，无明文；
- `argv_mode_preserves_the_legacy_switch...`：默认路径行为不变；
- `missing_password_always_disables_the_prompt`：无口令仍 `-p-`；
- `stdin_channel_is_opt_in_by_default`：未设置环境变量时绝不走 stdin。

**残余风险（未消除）**：默认路径仍把口令放在 argv。要彻底消除需要上游支持口令文件/stdin，或改走进程内解密（仅 ZIP 可行）。已登记 OD-002。

---

## 3. QUAL-002 可访问性缺少门禁

**根因**：`eslint-plugin-jsx-a11y@6.10.2` 的 peer 范围为 `eslint ^3–^9`，项目使用 ESLint 10.12，强装需 `--legacy-peer-deps` 且运行时不保证兼容。

**改动**：`scripts/check-a11y.mjs`（TypeScript 编译器 API，无新依赖）提供两项检查：
- `a11y-img-alt`（error）：`<img>` 缺 `alt`；
- `a11y-click-target`（warning）：原生元素带 `onClick` 但无 `role`/`tabIndex`/`onKeyDown`。

组件级大写标签不参与判定，避免误报。接入 `npm run lint:a11y`，CI 的 `Lint frontend` 步骤串行执行。

### 3.1 标注为待替换项（本轮新增）

| 位置 | 标注 |
|------|------|
| `scripts/check-a11y.mjs` 头部 | 明确写出 TEMPORARY GATE、替换条件（插件 peer 支持 ESLint 10）与四步移除清单 |
| `docs/decisions/OPEN-DECISIONS.md` | 登记 OD-001，含 Blocked By / Resolves When / 状态 |
| 本文件 §5 | 列为后续项，指向 OD-001 |

**替换条件**：`eslint-plugin-jsx-a11y` 发布声明 `eslint@^10` 的版本后，同一变更集内删除脚本与 `lint:a11y`、移除 CI 步骤、在 `eslint.config.js` 启用 recommended、把 OD-001 置为 RESOLVED。

---

## 4. 验证汇总

| 检查 | 结果 |
|------|------|
| `cargo fmt --all -- --check` | 0 差异 |
| `cargo clippy --workspace --all-targets -- -D warnings` | 通过 |
| `cargo test -p openunpack-infrastructure --lib engine::cli_engine::` | **6/6 通过** |
| 7-Zip stdin 实测（7zr + 7za） | 完成，证据已归档 |
| `npm run lint` / `npx tsc -b` | 0 error |
| `npm run lint:a11y` | 0 error / 0 warning |
| `portable_vault_tests` / `vault-error.test.ts` | 本地受环境限制，需 CI 执行 |

## 5. 未完成与后续

1. **P2-09 残余风险**：默认仍用 argv；等上游支持口令文件或 stdin（OD-002）。
2. **P2-12 迁移体验**：是否提供"一键转 DPAPI"需产品决策（OD-003）。
3. **QUAL-002 替换**：跟踪插件版本，条件满足即按 §3.1 清单移除（OD-001）。
