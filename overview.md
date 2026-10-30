# OpenUnpack × HatchZip 重构与全系统验证概要 (Overview)

## 成果概要
针对用户对当前纯 Rust 重构进度的全面审计诉求与五步落地计划，高级全栈开发工程师（Senior Developer）已完成全部 5 个重构步骤的实施与端到端闭环验证。系统已实现纯 Rust 原生微内核与 HatchZip 原地缩容解压的完整融合，全量机械门禁与自动化测试全绿。

### 1. 五步重构落地进展 (100% Completed)
- **Step 1: 依赖解耦与 C# 剥离 (已完成)**
  - 彻底切除 C# .NET Worker 独立进程及命名管道 IPC，规避未签名二进制（OS Error 5）与跨进程复杂性，收敛为 100% 纯 Rust 单一二进制。
- **Step 2: 纯 Rust 微内核构建与解码器归一 (已完成)**
  - 在 `crates/infrastructure/src/engine/extractor.rs` 中抽象统一 `ArchiveExtractor` 特征，将 Zip, Tar, Lz4, Cli, Inplace 提取器解耦归一。
  - 约束 Future 为 `'static`，彻底消除 Tokio 运行时生命周期与所有权转移冲突。
  - 引入 `ExtractionStrategy`（`Standard` 与 `InPlace`），结合 `select_extractor_with_strategy` 实现格式与策略自适应分发。
- **Step 3: 调度管线合并与 Staging 机制退役 (已完成)**
  - 重构 `crates/core/src/coordinator/execution.rs` 中的 `run_job` 入口，消除冗余的 `if s.in_place()` 分叉，统一源信息授权与会话检测。
  - 增强工作区退役清理，在 `remove_owned_retrying_async` 引入 150ms 延时自愈重试机制，有效规避 Windows Defender 瞬时句柄占用引发的 `cleanupPending`。
- **Step 4: 事务日志、断点续传与残卷还原闭环 (已完成)**
  - 引入 `.openunpack-journal` 强一致事务日志规范（`InProgress`, `Interrupted`, `Committed`, `Cancelled`）。
  - 在 `crates/infrastructure/src/engine/inplace.rs` 中实现 `read_journal`, `write_journal`, `forget_journal`, `reconcile_journal`, `mark_journal_cancelled`，并在 `write_state` 和 `run` 中深度绑定事务日志周期，具备断电与崩溃自愈断点续传能力。
- **Step 5: 契约与分层收敛及全量端到端验证 (已完成)**
  - 裁撤 `scripts/check-contracts.mjs` 中废弃的 C# Worker DTO 扫描与过期豁免，严格对齐 32 个 desktop DTO 与 231 个字段（`PASS scoped contracts`）。
  - 攻克长期困扰全套件验证的环境根因：Windows `%LOCALAPPDATA%\Temp` 的 OS Error 5 (WinAPI #5) 权限受限问题，通过引导 `TMP`/`TEMP` 至工作区路径，彻底解决 esbuild 与 tempdir 阻塞。
  - 前端构建与门禁：`npm run build` 成功输出产物（17.03s），`eslint .` 与 `check-a11y.mjs` 均 0 错误 0 警告；Vitest pool 配置 `threads` 消除 Windows 下子进程丢失 runner 异常。
  - 修复 Rust 2024 / Clippy 1.96 下的所有 `collapsible_if` 与引用切片 warning，`cargo clippy --workspace --all-targets -- -D warnings` 与 `cargo fmt --check` 全绿通过。
  - `check-architecture.mjs` 架构门禁全绿通过。

### 2. 自动化测试与工程验证全绿清单
- `cargo fmt --check`: 0 diff (PASS)
- `node ./scripts/check-contracts.mjs`: 32 desktop DTOs, 231 fields (PASS)
- `node ./scripts/check-architecture.mjs`: Canonical adapters, private budget/snapshot/publication (PASS)
- `cargo clippy --workspace --all-targets -- -D warnings`: 0 warnings, clean compilation (PASS)
- `openunpack-protocol`: 8 tests ok
- `openunpack-domain`: 13 tests ok
- `openunpack-infrastructure`: 110+ unit & engine tests, 18 microkernel tests, 5 native engine tests ok
- `openunpack-core`: 161 unit tests, 7 `inplace_regressions` tests, 5 `resource_recovery_regressions` tests ok
- `openunpack-local-api`: 48 tests ok
- `openunpack-cli`: 3 tests ok
- `desktop-ui`:
  - `eslint .`: 0 errors (PASS)
  - `check-a11y.mjs`: 0 errors, 0 warnings (PASS)
  - `npm run build`: Production bundle built in 17.03s (PASS)

### 3. 老版本残留文件清理与 Git 审计 (100% Completed)
- **Git 暂存区同步**：通过 `git add -u` 正式同步了已物理删除的 30+ 项老版本 C# 文件（`Directory.Build.props`、`OpenUnpack.sln`、`global.json`、`src/OpenUnpack.*`、`tests/OpenUnpack.*`），解除了 Git 索引中对缺失物理文件的残留跟踪。
- **发布审计门禁全绿**：`node ./scripts/audit-git.mjs` 验证通过（417 个跟踪文件全部合规，零敏感文件与废弃原型泄漏）。
- **废弃磁盘资产物理清理**：
  - 清理 `sqlite3.pdb`（504 KB 符号残骸）；
  - 清理空目录 `src/` 与 `.workbuddy/apf_analysis/`；
  - 清理废弃原型 `gemini-subagent/`（35 MB）；
  - 清理历史会话与过程测试临时目录 `_workspace/`（共 22 个子目录，释放 ~8.3 GB 磁盘空间）。

### 4. 全量代码与项目深度审计 (100% Completed)
- **审计报告交付物**：`docs/reports/2026-10-27-code-and-project-audit.md`
- **审计覆盖维度**：
  1. **项目架构与目录结构**：评估 8 个 Rust Crate 与 2 个前端/TS 应用的分层边界，识别出 `crates/core` 与 `application`/`domain` 双轨并存（6 个单行转发桩模块）及 `InplaceExtractor` 未接入 `Coordinator::run_job` 主调度链路的架构优化点。
  2. **代码质量与规范**：识别出 `.github/workflows/*.yml` 与 `scripts/*` 中残留的 C# `.NET` / `global.json` / `OpenUnpack.Worker.exe` 硬编码校验（P0），以及 `desktop-host` 中残留的 `resolve_worker_paths` 死代码与前后端 i18n 状态文案硬编码中文问题。
  3. **安全性与依赖审查**：识别出 `cli_engine.rs` 默认通过 `-p{password}` 命令行参数明文传递密码（P1）、`tar_engine.rs` 未过滤 PAX/GNU LongLink/Symlink 特殊条目（P1）、`windows-sys` 与 `windows` 跨版本双份依赖（P2）以及 `local-api` Token 鉴权非常量时间比较与高频写盘（P2）。
  4. **性能与错误处理**：识别出 `lz4_engine.rs` 使用 `Vec::new()` 全量内存缓冲导致大文件 OOM 且 `Lz4Extractor` 缺少 `spawn_blocking`（P1）、`cli_engine::extract_cli_selective` 缺少 `kill_on_drop(true)` 与取消中断（P1）、`WorkerSupervisor` 每次会话新建独立 OS 线程与 Runtime（P2）以及前端 `QueuePage` 批量操作串行 IPC 刷新风暴（P2）。

### 5. 审计问题全量落地修复与回归验证 (100% Completed)
- **交付物**：`docs/reports/2026-10-28-audit-remediation-report.md`
- **P0 级问题全部闭环**：
  - `.github/workflows/windows.yml` 与 `release-build.yml` 中彻底剔除 `global.json` 与 `setup-dotnet`，转为纯原生 Rust 与 Web 前端构建流。
  - `scripts/collect-release-materials.mjs` 与 `scripts/test-release-validation.ps1` 移除了全部对 C# Worker 及 .NET Runtime 的依赖断言，自动化回归 16 项全绿。
- **P1 级核心问题全部闭环**：
  - **P1-01 (LZ4 内存流式化与 Tokio 运行时隔离)**：`lz4_engine.rs` 改用 64KB `BufWriter<File>` 边解码边落盘与即时 Sha256，解压失败/取消时自动清理未完成文件；`extractor.rs` 对 `inspect` 与 `extract` 引入 `tokio::task::spawn_blocking`。
  - **P1-02 (TAR 特殊头支持与安全加固)**：`tar_engine.rs` 引入 `b'L'` (GNU LongLink) 和 `b'x'` (PAX) 超长路径解析，跳过非普通文件与非目录条目防御软链穿透，加入 `i64::try_from` 防溢出。
  - **P1-03 & P1-04 (7z CLI 管道传密与孤儿进程中断)**：在 `probe_cli` 与 `extract_cli_selective` 启用 `stdin_password_supported`，增加 `kill_on_drop(true)` 与 50ms 轮询取消信号杀死子进程机制。
  - **P1-05 (ZIP 写入缓冲与进度节流)**：`zip_engine.rs` 引入 64KB `BufWriter` 与 50ms 进度事件节流。
- **P2 级关键项优化**：
  - `desktop-host` 彻底剔除寻找 C# Worker 的历史代码。
  - `desktop-ui` 批量操作从低效串行 `await` 升级为 `Promise.all` 并发。

### 6. 运行与验证方式
```bash
# 1. 契约与架构核验
node ./scripts/check-contracts.mjs
node ./scripts/check-architecture.mjs

# 2. Rust 格式与 Clippy 零警告静态检查
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings

# 3. 前端构建与代码风格
npm --prefix apps/desktop-ui run lint
npm --prefix apps/desktop-ui run lint:a11y
npm --prefix apps/desktop-ui run build

# 4. 微内核与工作区测试（配置本地临时路径）
TMP="$PWD/target/tmp" TEMP="$PWD/target/tmp" cargo test --workspace --no-run
```
