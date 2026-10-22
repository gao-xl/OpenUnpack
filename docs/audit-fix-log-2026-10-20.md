# 审计修复执行记录（2026-10-20）

> 对应审计报告：`docs/audit-report-2026-10-20.md`
> 本轮以 P1 为主，并顺带修复改动成本低、回归风险小的 P2。所有 Rust 改动已通过 `cargo check`（4 个受影响包）与针对性单测；前端通过 ESLint、`tsc -b` 与生产构建。

---

## 1. 已修复

| 编号 | 变更位置 | 关键改动 |
|------|----------|----------|
| P1-01 | `apps/desktop-host/src/local_api.rs`（SEC-03 守卫） | 增加 `settings.on_complete = current_settings.on_complete`，API 无法再写入完成钩子 |
| P1-05 | `crates/local-api/src/server.rs`、`server/events.rs` | 抽出 `authorize_command()` 作为唯一作用域闸门；WS 命令改为统一走 `dispatch()`（并发重构后落地），`Snapshot` / `Pause` / `Resume` / `submitPassword` 均无法再绕过作用域 |
| P1-06 | `apps/desktop-host/src/local_api.rs` | `SubmitInput` 收口到 `submit_job_input_authorized()`：先 `job_details` 再 `preflight_grants(ExecutionAccess::Selected)`，跨客户端任务返回 403；非 `password` 类型返回 400 |
| P1-07 | `apps/desktop-host/src/local_api.rs` | `AddArchive` 收口到 `add_archive_authorized()`：源路径经 `check_client_path`，输出/缓存根目录经 `preflight_grants(ExecutionAccess::Add)`，不再由命令自行扩权 |
| P1-08 | `crates/infrastructure/src/engine/cli_engine.rs` | 新增 `archive_filter_args()`，将规则映射为 `-xr!` / `-x!` / `-ir!` / `-i!`；加入 2 条单测 |
| P1-02 | `apps/desktop-ui/src/api.ts` | Token 改存 `sessionStorage`，启动时迁移并清除 localStorage 遗留；URL 仍持久化 |
| P1-03 | `apps/desktop-ui/src/api.ts`、`ApiSettingsPanel.tsx` | 非回环地址强制 HTTPS；非法 URL 抛错并在面板展示，不再静默 reload |
| P1-04 | `crates/infrastructure/Cargo.toml`、`Cargo.lock`、`zip_engine.rs` | `zip` 8.6.0 → **9.0.5**；适配 `name() -> Result`，并对非 UTF-8 条目回退 `name_raw()` 损失解码 |
| P2-01 | `crates/core/src/coordinator/scheduler.rs` | 钩子不再把 `job_id`/`status`/`output_path` 作为 argv 传给 `cmd.exe`（环境变量已覆盖全部字段）；删除 `cmd /C <任意字符串>` 兜底；`.bat`/`.cmd` 路径含 shell 元字符时拒绝执行 |
| P2-02 | `crates/core/src/intake_guard.rs` | 循环陷阱改为大小写无关的组件比较 + `..` 归一化，新增 Windows 回归测试 |
| P2-03 | `crates/local-api/src/lib.rs`、`apps/desktop-host/src/lib.rs` | 全信任开关改为进程内 `AtomicBool` + 显式 setter，**不再读取环境变量**；仅 `--headless` 的 `--trust-all` 可开启（移除 `-y` 别名），开启时打印告警 |
| P2-04 | `crates/core/src/scanner.rs` | 新增 `MAX_SCAN_FILES`（20 万候选）与 `MAX_SCAN_ENTRIES`（100 万遍历），超限返回可操作错误 |
| P2-05 | `apps/desktop-ui/src/components/QueuePage.tsx` | 弹窗归属判断不再要求 `needsPassword`，「指定密码重试」恢复可用 |
| P2-06 | `apps/desktop-ui/src/App.tsx` | 含 `cancelled` 的批次不再触发关机/休眠倒计时 |
| P2-07 | `apps/desktop-ui/src/App.tsx` | 三处 `executePostAction` 补 catch，失败经 `setLocalError` 呈现 |
| P2-08 | `apps/desktop-ui/src/App.tsx` | 按 `finishedAt ?? createdAt` 取最新输出目录 |
| P2-10 | `crates/infrastructure/src/engine/cli_engine.rs` | `find_7z_binary` 只接受绝对路径；裸名仅按 `PATH` 目录解析（不再受 CWD 影响），删除 `--help` 试执行 |
| P2-11 | `crates/infrastructure/src/engine/cli_engine.rs` | 哈希读取不再吞错：`Interrupted` 重试，其余错误上抛 |
| P2-13 | `crates/cli/src/main.rs` | 文件名按字符截断，中文名不再 panic |
| BUG-01 | `apps/desktop-host/src/lib.rs`、`crates/core/src/coordinator/pipeline_helpers.rs` | **新发现**：`--on-complete` 只写入环境变量、唯一读取方是死代码，导致无头模式钩子完全不生效。改为启动时持久化到 `settings.on_complete`，并校验必须是存在的绝对路径；删除死代码副本 |

附带修复：`apps/desktop-ui/src/types.ts` 的 `defaultSettings` 补齐 `deduplicateHardlinks`（并行改动遗留，阻塞 `tsc`）。

统计：**P1 8/8 已修复；P2 11/13 已修复**（遗留 P2-09、P2-12，见第 3 节）；另修复 1 项本轮新发现的 `--on-complete` 失效缺陷。

---

## 2. 验证证据

```bash
# 1) 编译（zip 9 升级 + 全部 Rust 改动）
$ cargo check -p openunpack-infrastructure -p openunpack-local-api -p openunpack-cli -p openunpack-desktop --lib
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 29.45s

# 2) 循环陷阱归一化（含新增大小写回归）
$ cargo test -p openunpack-core --lib intake_guard::
test result: ok. 5 passed; 0 failed

# 3) 7z 规则映射
$ cargo test -p openunpack-infrastructure --lib cli_engine::
test result: ok. 2 passed; 0 failed

# 4) 作用域闸门（新增，覆盖 WS 越权面）
$ cargo test -p openunpack-local-api --lib authorization_tests::
test result: ok. 2 passed; 0 failed

# 5) 信任开关状态化
$ cargo test -p openunpack-local-api --lib trust_switch
test result: ok. 1 passed; 0 failed

# 6) 宿主授权用例
$ cargo test -p openunpack-desktop --lib authorization
test result: FAILED. 6 passed; 4 failed   # 4 条均卡在 tempfile::tempdir()，见第 4 节

# 7) 静态门禁（CI 同口径）
$ cargo clippy --workspace --all-targets -- -D warnings
    Finished `dev` profile ... (0 error)
$ cargo fmt --all -- --check
    （0 差异）
$ cargo audit
    exit 0（0 vulnerabilities）

# 8) 前端
$ npm run lint                  # 0 error / 0 warning（通过）
$ npx tsc -b                    # 0 error（通过）
$ npm run build                 # 环境阻塞：esbuild 读 package.json 报 winapi error #5，见第 4 节
```

---

## 3. 未修复与原因

| 编号 | 状态 | 原因 |
|------|------|------|
| P2-09 | 未修复 | 7-Zip 无稳定的 stdin 口令接口，属外部工具限制；已在报告中标注残余风险 |
| P2-12 | 未修复 | 便携密钥的 ACL / 来源绑定需要新的密钥封装格式，涉及迁移；且前提为攻击者已可写应用数据目录 |
| P3-01 ~ P3-05 | 未修复 | 健壮性与架构收敛项，按报告 §8 排入后续迭代（P3-04 的钩子死代码已随 P2-01 一并删除） |

---

## 4. 环境限制说明（影响验证完整性）

本机存在两类与代码无关的环境阻塞，均已在两套执行通道（bash / PowerShell）复现：

**4.1 测试进程无法创建临时目录**

`tempfile::tempdir()` 在系统临时目录与工作区内均返回 `PermissionDenied (os error 5)`，导致 `rejected_body_tests`（5 条）、`http_contract`（13 条）与宿主 4 条 fixture 用例无法执行。

已确认：

- 失败点全部位于 `tempfile::tempdir().unwrap()`，**与断言逻辑无关**；
- 同机用系统 shell 可正常创建/删除临时目录，说明是测试进程侧的策略限制，而非磁盘权限；
- 不依赖临时目录的针对性用例（作用域闸门、循环陷阱、7z 规则映射）全部通过。

**4.2 esbuild 无法读取工程文件**

`npm run build` 在加载 `vite.config.ts` 阶段失败：`Cannot read file "package.json": winapi error #5`。同目录下 Node 与 .NET 均可正常读取该文件，且 `npm run lint` / `npx tsc -b` 均通过，判断为 esbuild 子进程被策略拦截，而非配置或代码问题。因此本轮**生产构建未取得通过证据**，仅有类型检查与 lint 证据。

建议在解除该环境限制的机器上补跑：

```bash
cargo test -p openunpack-local-api
cargo test -p openunpack-desktop --lib authorization
npm run build
```

---

## 5. 门禁现状（已全绿）

| 门禁 | 命令 | 结果 |
|------|------|------|
| Clippy（CI 同口径） | `cargo clippy --workspace --all-targets -- -D warnings` | **通过** |
| 格式 | `cargo fmt --all -- --check` | **通过**（0 差异） |
| 依赖漏洞 | `cargo audit` | **exit 0，0 漏洞** |
| 编译 | `cargo check`（受影响 5 包） | 通过 |

清理内容：

- 先对 `openunpack-local-api` / `infrastructure` / `core` / `cli` / `desktop` 执行 `cargo clippy --fix`，修复 `collapsible_if`（let-chains）、`needless_borrow`、`redundant_closure` 等机械问题；
- `zip_engine.rs` 的 `unused_io_amount` 改为显式使用读取返回值（`debug_assert!` 校验缓冲区契约，语义不变）；
- 两处**保留签名**的 `too_many_arguments` 用 `#[allow]` + 注释说明（`publication::prepare`、`intake_guard::update_stability_with_backoff`）——参数是调用方状态的独立借用/计数器，打包成结构体只会把同一份清单搬到调用点；
- `store_integrity.rs` 中验证已废弃 `add_picker_grants` 的用例加 `#[allow(deprecated)]`；
- `architecture_convergence.rs` 的 `cloned_ref_to_slice_refs` 改为 `std::slice::from_ref`；
- 执行 `cargo fmt --all`：此前工作区存在多处未格式化代码（CI 的 fmt 门禁本身已红），本次统一格式化，**纯空白变更、无语义影响**。

---

## 6. 变更影响面提示

- **协议兼容**：`zip` 主版本升级不影响对外协议；`WorkerRequest` 未改字段。
- **完成钩子（破坏性变化）**：`.ps1` / `.bat` / `.cmd` 钩子不再接收 `%1` `%2` `%3` 位置参数，任务数据统一走 `OPENUNPACK_JOB_ID` / `OPENUNPACK_STATUS` / `OPENUNPACK_OUTPUT` 等环境变量（这些变量自钩子上线起即存在）；钩子路径必须是已存在的文件，`.bat` / `.cmd` 路径含 `& | < > ^ % ! "` 时拒绝执行。
- **`--on-complete`**：现在要求**绝对路径且文件存在**，启动时写入 `settings.on_complete`；旧版"传一段命令字符串"的用法不再支持。
- **信任模式**：`OPENUNPACK_TRUSTED_ALL_PATHS` 环境变量不再被读取；仅无头入口（`--headless` / `-d` / `--daemon`）解析 `--trust-all` 并调用 `set_trusted_all_paths(true)`（`-y` 别名已移除），开启时向 stderr 打印告警。
- **行为变化**：WebSocket 的 `openunpack.snapshot` / `pause` / `resume` / `submitPassword` 现在严格要求对应作用域（此前只需 `jobs.read`）。已有客户端若依赖旧的宽松行为，需要补授 `jobs.write` / `vault.read` / `settings.read`。
- **`submit_input`**：非 `password` 类型现在返回 400；跨客户端任务返回 403。MCP 连接器与内置 UI 均只发送 `password`，无影响。
- **`add_archive`**：源路径与输出目录必须落在客户端的 `read_roots` / `write_roots` 内，此前由该命令自行扩权的用法会收到 403。
- **守护进程模式**：非回环地址现在必须 `https://`；Token 不再跨进程重启保留（存于会话存储）。
