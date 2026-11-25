# OpenUnpack 项目阶段总结与历史报告汇总（2026-11-25）

> **报告日期**：2026-11-25  
> **基准分支状态**：本地 `main`（`cfd5fd4`，领先 `origin/main` `ae2d9b9` 共 3 个提交）+ 工作区未提交改动（177 个已修改追踪文件、38 个未追踪项）  
> **本文档定位**：汇总并归档 2026-10-22 至 2026-11-18 期间的多轮审计、诊断、修复与重构报告，基于 2026-11-25 对当前工作区的全量脚本、前端与 Rust 实测结果给出项目阶段状态定论与后续收口清单。本文档合并替代已清理的 14 份阶段性审计/修复/诊断旧报告。

---

## 一、 核心结论（TL;DR）

1. **架构主线已收敛到位**：项目已从早期“Rust 协调器 + C#/.NET Worker”混合架构彻底收敛为 **Rust 单体调度内核 + 外部 `7z.exe` 原生引擎（`NativeEngine` / `cli_engine.rs`）+ React/TypeScript/Ant Design 桌面 UI + Loopback Local API + MCP Connector**。
2. **核心安全性与可靠性高危缺口已大部分在最新代码修复**：
   - `cli_engine.rs` 已移除密码探测与解压命令行中的 `-p` 参数，全面切换为 **stdin 管道注入密码**（`password_probe_and_extract_via_stdin_succeeds_without_argv_exposure` 实测通过），`OPEN-DECISIONS.md` 中的 `OD-002` 在代码层实际已解决；
   - 补齐 `ProcessJob`（`JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`）防僵尸进程、`-sccUTF-8` 与 `oem_codepage::decode_oem_text`、外部调用超时约束（`CLI_PROBE_TIMEOUT=30s` / `CLI_INSPECT_TIMEOUT=60s`）及 `is_safe_filter_pattern`；
   - 原地引擎与冲突消解（`conflict_resolver.rs`、`inplace.rs`、`publication.rs`）已补齐双向内容一致比对（Step 2 反向检查防多余非噪声文件误判）、文件名/伴随密码 hints 动态挖掘、孤立临时回收目录启动清理；
   - 建立结构化 `[UnpackError:...]` 错误信封与前端 i18n 排障引导，并完成 4 套视觉皮肤（`win11` / `win98` / `modern` / `cyberpunk`）、`VirtualGrid` 虚拟列表与 `OPT-001 / OPT-002 / OPT-006` 性能修复。
3. **当前处于“功能与安全显著推进、但工程与文档待收口”阶段**：
   - **最大工程风险在于未提交状态堆积**：工作区跨越 10-31 至 11-18 的 177+ 修改文件尚未按主题拆分提交；
   - **发布产物版本滞后**：`artifacts/release/` 下压缩包与安装包构建于 `2026-11-17 15:03–15:04`，尚未覆盖 `2026-11-18` 的核心引擎、冲突消解与 UI 错误诊断修复；
   - **文档同步滞后**：`README.md`、`docs/architecture-current.md` 仍包含旧 `.NET Worker` 图示或描述，`docs/decisions/OPEN-DECISIONS.md` 仍将 `OD-002` 标为 `OPEN`，`CHANGELOG.md` 未更新 11 月重要变更；
   - **少量历史审计债务与物理机验收待完成**：如 `QA-5` / `QA-7` / `QA-8` / `QA-9`、`ARC-1`、剩余优化项（`OPT-003/004/005/007/008`、`SOL-09`）、`desktop-host` 软链接/联接检查改动复核，以及真机干净系统安装/UI/杀软兼容性验收。

---

## 二、 项目阶段演进脉络（Phase 1 → Phase 4）

| 阶段 | 时间跨度 | 核心目标与代表变更 | 关键成果与遗留转折点 |
|------|----------|--------------------|----------------------|
| **Phase 1：Beta 发布与 CI 止血** | 2026-10-14 ~ 10-23 | 完成目录身份、加载优化、架构收敛与协调器拆解（`architecture-convergence-2026-10-20`、`coordinator-components-2026-10-20`）；发布 `0.1.0-beta.1` / `0.1.0-beta.2`。 | 暴露出 CI 主线阻塞、Windows 目录删除/长路径/锁冲突等早期可靠性问题，倒逼后续引擎与清理机制重构。 |
| **Phase 2：去 .NET 化与原地引擎重构** | 2026-10-27 ~ 10-31 | 参考 HatchZip 收敛架构，以 `NativeEngine`（CLI 调度 `7z.exe`）替代 `.NET Worker`，实装 `InplaceEngine`（防套娃/原地合并/回收站安全清理）；形成 `ae2d9b9` → `cfd5fd4` 提交基线并清理仓库/构建体积。 | 架构包袱显著减轻，但 10-27 ~ 10-30 多轮审记指出仍存大组件耦合、部分契约/文档未同步及若干稳定性边界待补强。 |
| **Phase 3：便携化存储与安全/脚本密集审计修复** | 2026-11-01 ~ 11-06 | 推进便携模式数据与密钥存储，于 11-03 开展架构/QA/安全多维综合审计；11-05~11-06 集中落实 15 项整改及 PowerShell 审计脚本（G1–G5）修复与 `artifacts/verification` 校验链路闭环。 | 命令行 `-p` 明文暴露口令（`OD-002`）、Local API 细节及若干边界问题留待后续代码彻底根治。 |
| **Phase 4：体验皮肤、性能优化与 11-18 深度加固** | 2026-11-17 ~ 11-25（当前） | 11-17 引入 4 套主题皮肤、`VirtualGrid`、NSIS 安装向导图形资源、SQLite v1→v2 索引（`OPT-006`）及进度节流修复（`OPT-001/002`）；11-18 完成 `cli_engine.rs` stdin 密码传输、OEM/UTF-8 编码修复、`conflict_resolver.rs` 双向内容比对、`UnpackError` 排障信封及完成钩子 `CREATE_NO_WINDOW`。 | 技术能力进入较成熟可用状态，但所有 11-17/11-18 进展均留在工作区未提交，发布产物与说明文档亟需同步收口。 |

---

## 三、 当前项目状态全景（2026-11-25 工作区实测）

> 说明：由于仓库路径包含中文字符（`批量解压缩`）且运行在 AppContainer 沙箱中，本次验证统一采用以下非入侵性环境参数以确保工具链正常运行：
> - Rust 构建目录：`$env:CARGO_TARGET_DIR = "C:\Users\im\AppData\Local\openunpack-target"`
> - 临时目录（避免 esbuild 在系统 Temp 受限）：`$env:TMP = "$PWD\artifacts\build-tmp"; $env:TEMP = "$PWD\artifacts\build-tmp"`

### 3.1 版本控制与发布制品

- **Git 分支与工作树**：`main` 分支 `HEAD = cfd5fd4`，领先 `origin/main`（`ae2d9b9`）3 个提交；工作区存在 **177 个已修改追踪文件 + 38 个未追踪项**（含 `apps/desktop-ui/.build-tmp/` 及 10-31 ~ 11-18 开发积累）。
- **现有发布产物（`artifacts/release/`，构建于 `2026-11-17 15:03–15:04`）**：
  - `OpenUnpack-0.1.0-source.zip`：`2,788,018` 字节
  - `OpenUnpack-0.1.0-windows-x64-portable.zip`：`73,067,047` 字节
  - `OpenUnpack-0.1.0-windows-x64-standard-setup.exe`：`69,304,929` 字节
  - `SHA256SUMS.txt`：`60,763` 字节
  - **状态判定**：上述构建产物**未包含 `2026-11-18` 的未提交修复**，下次发版前必须全量重编并重新生成校验和。

### 3.2 契约、架构、无障碍、策略与安全门禁实测

| 检查项 | 执行命令 | 实测结果 | 备注 |
|---|---|---|---|
| 跨端契约校验 | `node ./scripts/check-contracts.mjs` | **PASS**（33 desktop DTOs, 236 fields） | 前后端与协议字段对齐 |
| 架构依赖边界检查 | `node ./scripts/check-architecture.mjs` | **PASS** | 分层依赖规则通过 |
| Git 追踪与文档白名单审计 | `node ./scripts/audit-git.mjs` | **PASS**（删除 `overview.md` 后实测 423 tracked paths） | 受 `source-release-policy.json` 与 `.gitignore` 双重约束，`phase-summary-2026-11-25.md` 已通过 `POLICY_CHECK: ALLOW` |
| 无障碍合规检查 | `node ./scripts/check-a11y.mjs` | **PASS**（0 error(s), 0 warning(s)） | — |
| 发布物料完整性审计 | `node ./scripts/audit-release-materials.mjs` | **PASS**（674 components, 1378 verified files, 64,841,562 bytes） | 对应本地已有 staging/release 物料 |
| MCP Connector 单测 | `npm --prefix apps/mcp-connector test` | **PASS**（22/22） | — |
| 发布验证脚本测试 | `pwsh.exe -NoProfile -File .\scripts\test-release-validation.ps1` | **PASS**（16/16） | — |
| 安全审计策略测试 | `pwsh.exe -NoProfile -File .\scripts\test-security-audit-policy.ps1` | **PASS**（37/37） | — |
| 运行时策略测试 | `pwsh.exe -NoProfile -File .\scripts\test-runtime-policy.ps1` | **PASS**（37/37） | — |
| 离线安全审计 | `pwsh.exe -NoProfile -File .\scripts\audit-security.ps1 -Offline` | **PASS (passed_with_skipped)** | `cargo-audit`: 596 deps, 0 vulnerabilities；离线模式跳过联网项，更新 `artifacts/verification/security-audit.json` |

### 3.3 Rust 格式 / Clippy 与前端工程门禁实测

| 检查项 | 执行命令 | 实测结果 |
|---|---|---|
| Rust 格式检查 | `cargo fmt --all -- --check` | **PASS**（0 差异） |
| Rust 静态检查 | `cargo clippy --workspace --all-targets -- -D warnings` | **PASS**（0 警告） |
| 前端 TypeScript 编译检查 | `apps/desktop-ui`: `npx tsc -b` | **PASS**（exit 0） |
| 前端 ESLint 检查 | `apps/desktop-ui`: `npm run lint` | **PASS**（0 错误） |
| 前端 Vitest 测试套件 | `apps/desktop-ui`: `vitest run --run` | **PASS**（34 个测试文件、295 条用例全通过，耗时 659.62s） |
| 前端生产构建 | `apps/desktop-ui`: `npm run build`（设置 `TMP/TEMP` 至 `artifacts/build-tmp`） | **PASS**（built in 15.78s） |

### 3.4 Rust 各 Crate 与集成测试实测明细

| Crate / 测试集 | 实测结果 | 失败/环境说明 |
|---|---|---|
| `openunpack-protocol` / `application` / `cli` | **PASS**（各 0 tests ok） | 无编译或执行失败 |
| `openunpack-domain` | **PASS**（18 passed, 0 failed） | — |
| `openunpack-infrastructure` (`--lib`) | **PASS**（125 passed, 0 failed） | 含 stdin 传密验证 `password_probe_and_extract_via_stdin_succeeds_without_argv_exposure` |
| `openunpack-infrastructure` 集成测试 | **PASS**：`native_engine_tests`（12/12）、`portable_vault_tests`（4/4）、`store_tests`（1/1） | — |
| `openunpack-local-api` (`--lib` + `--tests`) | **PASS**：`--lib` 12/12；`concurrency_limits` 3/3、`http_contract` 15/15、`rejected_body_transport` 3/3、`store_integrity` 7/7、`websocket_security` 8/8（共 60 passed, 0 failed） | — |
| `openunpack-core` (`--lib`) | **PASS**（171 passed, 0 failed, **1 ignored**） | 存在 1 条被忽略用例 |
| `openunpack-core` 直接通过的集成测试 | **PASS**：`architecture_convergence`（5/5）、`chaos_resilience_regressions`（17/17）、`exclusion_parity`（1/1）、`archive_inspection`（3/3）、`inplace_regressions`（7/7）、`output_rule_preview`（1/1）、`password_companion_authorization`（5/5）、`resource_recovery_regressions`（5/5）、`scanner_regressions`（5/5）、`selective_extraction`（5/5）、`worker_client`（4/4） | — |
| `openunpack-core`: `archive_integrity_regressions` / `format_matrix` / `release_reliability` | 默认环境受沙箱与宿主临时目录污染失败（0/5、4/6、0/3）；**指定 `$env:OPENUNPACK_7Z_PATH = "C:\Users\im\AppData\Local\openunpack-target\debug\tools\7zip\7z.exe"` 后全数 PASS**（**5/5**、`0.68s`；**6/6**、`4.34s`；**3/3**、`125.88s`） | **失败根因已实证**：<br>1) 沙箱工作区 `tools/7zip/7z.exe` ACL 仅授予 AppContainer `Write,Delete,Synchronize` 而缺 `ReadAndExecute`，引发 `-1073741502`（`0xC0000142`）；<br>2) `format_matrix` 与 `release_reliability` 硬编码优先查找 `C:\Users\im\AppData\Local\Temp\ou-7z\7z.exe`，该目录缺失 `7z.dll`（报 `Codec Load Error`）。 |
| `openunpack-core`: `hook_regressions` | **1 passed, 1 failed**（`test_on_complete_hook_execution` 失败） | **失败根因**：测试将含中文路径 `批量解压缩` 的 `$env:TMP` 以 UTF-8 写入 `.bat` 交由 `cmd.exe` 执行，在系统 ANSI/GBK 代码页下出现编码失配（非核心逻辑回归）。 |
| `openunpack-desktop`（`apps/desktop-host`） | **PASS**（`src/lib.rs`: 34 passed, 0 failed, **1 ignored**；`src/main.rs` 及其他目标 0 tests） | 1 条被忽略用例：`local_api::native_tests::native_http_extraction_keeps_passwords_temporary_and_gui_jobs_queued`（标注需显式构建的 Windows Worker 与官方 7-Zip payload）；本摘要表写完前已完成全套复核。 |

---

## 四、 核心热点模块复杂度现状

当前工作区存在若干超千行的“大模块”，是后续可维护性与拆分重构的重点关注对象（2026-11-25 实测行数）：

- **桌面宿主与基础设施层**：`apps/desktop-host/src/lib.rs`（`3,248` 行）、`crates/infrastructure/src/dpapi/vault.rs`（`3,115` 行）、`crates/infrastructure/src/paths.rs`（`2,265` 行）、`crates/infrastructure/src/engine/cli_engine.rs`（`1,252` 行）
- **核心调度层**：`crates/core/src/coordinator/scheduler.rs`（`993` 行）、`commands.rs`（`935` 行）、`execution.rs`（`781` 行）、`execution_extract.rs`（`457` 行）、`execution_reconcile.rs`（`187` 行）
- **桌面前端页面**：`apps/desktop-ui/src/components/QueuePage.tsx`（`1,823` 行）、`SettingsPage.tsx`（`1,541` 行）、`ToolboxPage.tsx`（`1,078` 行）

---

## 五、 跨报告遗留问题与技术债汇总（待办总清单）

### 5.1 安全与可靠性未闭环项

1. **`QA-7` 持久化错误静默吞噬**：`crates/core/src/coordinator/mod.rs:321` 仍存在 `let _ = self.core.put(&job);`，作业状态落盘失败不会被上层捕获或告警。
2. **`QA-8 / M4` Local API Web Root 与 CSP 安全策略**：`crates/local-api/src/server.rs`（`resolve_web_root()` 及 CSP 配置）仍未严格校验静态资源目录权限，且 CSP 含 `script-src 'self' 'unsafe-inline'`。
3. **`QA-9` 与 `QA-5` 输入校验一致性**：`crates/local-api/src/command.rs:635` 口令长度校验为 `4..=1024`，但 `PreviewImport` / `ConfirmImport` / `RetryJob` / `SubmitInput` 等命令间输入长度/边界限制尚不统一。
4. **`ARC-1` 进度推送节流粒度**：`crates/core/src/coordinator/transfers.rs:136,164` 虽引入 `THROTTLE_INTERVAL = 60ms`，但每次进度变化仍触发 `self.changed_locked()` 推送任务快照变更，大批量任务下的事件风暴治理未彻底改为增量/版本差。
5. **`X-5 / M20` Rust 工具链版本锁定缺失**：仓库根目录缺少 `rust-toolchain.toml`，根 `Cargo.toml` 也未声明 `rust-version`（MSRV）。
6. **待人工复核的未提交关键改动**：
   - `apps/desktop-host/src/lib.rs:374` 在工作区中将重解析点判断从 `FILE_ATTRIBUTE_REPARSE_POINT != 0` 改为了 `meta.file_type().is_symlink()`，需确认 Windows Junction（目录联接）不会被误放行；
   - `crates/core/tests/format_matrix.rs` 与 `release_reliability.rs` 优先读取固定临时路径 `C:\Users\im\AppData\Local\Temp\ou-7z\7z.exe` 的测试探测逻辑脆弱，且 `hook_regressions` 在含非 ASCII 字符的 `TMP` 路径下存在 `.bat` UTF-8/ANSI 编码兼容瑕疵。

### 5.2 性能优化清单收口（对应 `optimization-issues-2026-11-17.md` 与 `performance-optimization-plan-2026-11-17.md`）

| 优化编号 | 当前状态 | 说明 |
|---|---|---|
| **OPT-001 / OPT-002 / OPT-006** | **已实装待提交** | 进度存储/队列重渲染优化、SQLite 索引 v1→v2 迁移已在工作区实现 |
| **OPT-004 / OPT-005 / OPT-007** | **部分落地** | 仍需对照方案文档补齐剩余边界与端到端压测验证 |
| **OPT-003 / OPT-008 / SOL-09** | **仍待实施** | 列入下一轮性能优化优先项 |

### 5.3 文档同步、Git 收口与硬件级验收

- **文档同步**：更新 [README.md](file:///c:/Users/im/Documents/ChatGPT/批量解压缩/README.md) 与 [architecture-current.md](file:///c:/Users/im/Documents/ChatGPT/批量解压缩/docs/architecture-current.md) 清理旧 `.NET Worker` 过时架构图（`ARC-3`）；更新 [OPEN-DECISIONS.md](file:///c:/Users/im/Documents/ChatGPT/批量解压缩/docs/decisions/OPEN-DECISIONS.md) 将 `OD-002`（stdin 传密码）标为已解决；补齐 [CHANGELOG.md](file:///c:/Users/im/Documents/ChatGPT/批量解压缩/CHANGELOG.md) 与 [audit-fix-log-2026-10-21.md](file:///c:/Users/im/Documents/ChatGPT/批量解压缩/docs/audit-fix-log-2026-10-21.md) 交叉说明。
- **Git 与制品收口**：清理临时构建残余（如 `apps/desktop-ui/.build-tmp/`、`artifacts/build-tmp/`），将 177 个修改与相关新增源码按功能域拆分提交，并在合入后重新执行完整打包与 `artifacts/release/` 校验和生成。
- **物理机端到端验收空白**：在非沙箱物理 Windows 环境中，对照 [acceptance-checklist.md](file:///c:/Users/im/Documents/ChatGPT/批量解压缩/docs/acceptance-checklist.md) 补齐**干净机器 NSIS 安装/卸载、UAC/SmartScreen/杀软兼容、真机 GUI 交互（含 4 皮肤与 10000 条虚拟列表）、真实硬件断点/磁盘满异常**人工验收。

---

## 六、 历史报告汇总与清理台账

本次整理遵循“保留活跃规划/正式规范/里程碑设计与发布说明，合并归档阶段性重复审计/诊断/修复流水账”原则。

### 6.1 已汇总并删除的 14 份无用/过时报告

以下报告的核心有效信息与未闭环项均已纳入本总结（或已被后续代码提交覆盖），现已从仓库删除：

| 序号 | 原路径 | 原文时期与主题 | 清理原因（汇总去重/过时） |
|---|---|---|---|
| 1 | `overview.md`（Git 追踪，已 `git rm`） | 2026-10-28 HatchZip 重构五步法与阶段总览 | 内容停留在 10-28，已被本总结替代且未纳入正式文档索引 |
| 2 | `docs/diagnosis-ci-mainline-2026-10-23.md` | 2026-10-23 CI 主线构建与脚本故障诊断 | 历史排查记录，所涉 CI 问题已通过主线提交修复 |
| 3 | `docs/reports/published-beta-audit-2026-10-22.md` | 2026-10-22 `0.1.0-beta.2` 发布包专项审计 | 针对早期 Beta + 旧 Worker 架构的切片审计，已大幅过时 |
| 4 | `docs/reports/2026-10-27-system-audit-report.md` | 2026-10-27 系统级代码与结构审计 | 与同日审计高度重叠，后续多轮重构已改变大部分代码形态 |
| 5 | `docs/reports/2026-10-27-code-and-project-audit.md` | 2026-10-27 代码与工程综合审计 | 同上，有效长期结论已并入后续规范与本总结 |
| 6 | `docs/reports/2026-10-28-audit-remediation-report.md` | 2026-10-28 审计整改回执 | 阶段性修复确认单，已被 11-03 综合审计与后续代码演进覆盖 |
| 7 | `docs/reports/2026-10-29-refactor-audit.md` | 2026-10-29 重构后阶段审计 | 阶段性中间审计，已被 11-03 审计系列及工作区最新重构覆盖 |
| 8 | `docs/reports/2026-10-30-pre-release-audit.md` | 2026-10-30 预发布体检报告 | 针对 10-30 快照的预发布检查，时效已过 |
| 9 | `docs/tech-audit-2026-11-03.md` | 2026-11-03 技术审计草稿/摘要 | 与同日 `2026-11-03-comprehensive-tech-audit.md` 严重重复 |
| 10 | `docs/audit-2026-11-03-qa.md` | 2026-11-03 QA 维度审计分卷 | 同属 11-03 审计拆分碎片，仍有效项已汇总至本报告第五章 |
| 11 | `docs/audit-2026-11-03-arch.md` | 2026-11-03 架构维度审计分卷 | 同上，仍有效项已汇总至本报告第四、五章 |
| 12 | `docs/audit-report-2026-11-03.md` | 2026-11-03 审计报告副本/汇总稿 | 与同日综合审计报告多头并存，且 11-05 与 11-18 已修复其中大量项 |
| 13 | `docs/reports/2026-11-03-comprehensive-tech-audit.md` | 2026-11-03 全方位技术审计主报告 | 11-03 审计总报告；其已修复项及仍遗留项已全部提炼收敛至本报告 |
| 14 | `docs/audit-fix-2026-11-05.md` | 2026-11-05~06 15 项修复与 G1–G5 脚本整改记录 | 阶段性修复流水账；其中剩余 3 项风险与脚本验证状态已并入本总结 |

### 6.2 保留的正式文档与活跃资产

以下文档承担产品规范、契约定义、活跃计划、核心架构决策或历史版本发布记录，继续保留（内部目录 `.workbuddy/` 与 `.agent-teams/` 亦原样保留）：

- **正式规格与契约**：[README.md](file:///c:/Users/im/Documents/ChatGPT/批量解压缩/README.md)、[docs/README.md](file:///c:/Users/im/Documents/ChatGPT/批量解压缩/docs/README.md)、[software-spec.md](file:///c:/Users/im/Documents/ChatGPT/批量解压缩/docs/software-spec.md)、[functional-design.md](file:///c:/Users/im/Documents/ChatGPT/批量解压缩/docs/functional-design.md)、[local-api.md](file:///c:/Users/im/Documents/ChatGPT/批量解压缩/docs/local-api.md)、[architecture-current.md](file:///c:/Users/im/Documents/ChatGPT/批量解压缩/docs/architecture-current.md)、[OPEN-DECISIONS.md](file:///c:/Users/im/Documents/ChatGPT/批量解压缩/docs/decisions/OPEN-DECISIONS.md)、[acceptance-checklist.md](file:///c:/Users/im/Documents/ChatGPT/批量解压缩/docs/acceptance-checklist.md)、[CHANGELOG.md](file:///c:/Users/im/Documents/ChatGPT/批量解压缩/CHANGELOG.md)
- **基础设计演进与原地引擎记录（被规格文档交叉引用）**：[architecture-convergence-2026-10-20.md](file:///c:/Users/im/Documents/ChatGPT/批量解压缩/docs/reports/architecture-convergence-2026-10-20.md)、[coordinator-components-2026-10-20.md](file:///c:/Users/im/Documents/ChatGPT/批量解压缩/docs/reports/coordinator-components-2026-10-20.md)、[audit-fix-log-2026-10-21.md](file:///c:/Users/im/Documents/ChatGPT/批量解压缩/docs/audit-fix-log-2026-10-21.md)、[inplace-engine-2026-10-27.md](file:///c:/Users/im/Documents/ChatGPT/批量解压缩/docs/inplace-engine-2026-10-27.md)
- **活跃规划与优化清单**：`docs/plans/*`、[s06-directory-identity-plan-2026-10-14.md](file:///c:/Users/im/Documents/ChatGPT/批量解压缩/docs/s06-directory-identity-plan-2026-10-14.md)、[load-optimization-plan-2026-10-15.md](file:///c:/Users/im/Documents/ChatGPT/批量解压缩/docs/load-optimization-plan-2026-10-15.md)、[optimization-issues-2026-11-17.md](file:///c:/Users/im/Documents/ChatGPT/批量解压缩/docs/optimization-issues-2026-11-17.md)、[performance-optimization-plan-2026-11-17.md](file:///c:/Users/im/Documents/ChatGPT/批量解压缩/docs/performance-optimization-plan-2026-11-17.md)
- **历史 Release Notes**：[release-notes-0.1.0-beta.1.md](file:///c:/Users/im/Documents/ChatGPT/批量解压缩/docs/release-notes-0.1.0-beta.1.md)、[release-notes-0.1.0-beta.2.md](file:///c:/Users/im/Documents/ChatGPT/批量解压缩/docs/release-notes-0.1.0-beta.2.md)
