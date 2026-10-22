# OpenUnpack 发版前审核报告（Pre-Release Audit）

> 审核日期：2026-10-22 09:11–09:30（GMT+8）  
> 审核标的：工作树（HEAD `cf6e344` + 292 项改动，含 194 个未跟踪文件）  
> 审核方式：台账/终审报告复核 + 机械门禁实测 + 环境阻塞复现 + 依赖/配置 diff 审查  
> 上游依据：`docs/audit-report-final-2026-10-20.md`（含 §7 修复记录）、`docs/status-ledger-2026-10-11.md` §1–§18、`docs/audit-fix-log-2026-10-21.md`



---

## 0. 审核结论

**NO-GO（证据不足型否决）——不建议以当前状态发版。**

代码层未发现未决 P0/P1 缺陷（终审以来新增修复全部闭环，今日机械门禁复测全绿）。

**2026-10-22 更新**：用户确认本项目为**开源项目、不做代码签名**，B5 已闭环（改为 SHA-256 完整性模式）；B3（版本与 CHANGELOG）已闭环。当前剩余三项硬条件：**冻结候选（B1）、提交绑定 CI（B4）、原生验收与性能基线（B6）**；B2 的可执行面已最大化取证，剩余部分仍受本机未签名二进制限制（今日复测仍存在）。

| 维度             | 评级                       |
| -------------- | ------------------------ |
| 代码质量（门禁/静态检查）  | ✅ 通过（今日实测全绿）             |
| 功能完整性（对照规划）    | ✅ 规划项全量交付，验收证据部分缺失       |
| 测试证据           | ❌ 全量测试被环境阻塞，无法取得         |
| 配置与依赖变更        | ⚠️ 变更合理，个别需补记            |
| 兼容性            | ⚠️ 关键兼容点已有防护与实测，跨版本升级未验证 |
| 文档更新           | ⚠️ 文档体系完备，CHANGELOG 严重滞后 |
| 发布工程（候选/签名/CI）     | ⚠️ 无当前源码候选、未绑定提交；签名按开源模式改为 SHA-256 完整性凭证（已闭环） |

---

## 1. 今日实测证据（2026-10-22）

| 检查                | 命令                                                       | 结果                                                       |
| ----------------- | -------------------------------------------------------- | -------------------------------------------------------- |
| Rust 格式化          | `cargo fmt --all -- --check`                             | 0 差异 ✅                                                   |
| 跨栈契约              | `node scripts/check-contracts.mjs`（含 `--self-test` 变异注入） | PASS ✅                                                   |
| 架构门禁              | `node scripts/check-architecture.mjs`                    | PASS（单一生产协调器/适配器边界合规）✅                                   |
| 前端类型              | `npx tsc -b`（apps/desktop-ui）                            | 0 error ✅                                                |
| 前端 Lint           | `npm run lint`                                           | 0 error / 0 warning ✅                                    |
| npm 生产依赖          | `npm audit --omit=dev --audit-level=low`                 | 0 漏洞 ✅                                                   |
| Rust 依赖           | `cargo audit -n`                                         | exit 0（5 条白名单警告）✅                                        |
| .NET Worker 构建    | `dotnet build src/OpenUnpack.Worker -c Release`          | 0 警告 0 错误 ✅                                              |
| 协议层单测             | `cargo test -p openunpack-protocol`                      | 8/8 通过 ✅                                                 |
| **前端单测（vitest）**  | `npx vitest run`                                         | **❌ 被阻塞：esbuild `winapi error #5`，无法读取 package.json**    |
| **tempdir 类集成测试** | `cargo test -p openunpack-core --test worker_client`     | **❌ 0/4 全失败：`tempdir` 创建报 `OS Error 5 (Access Denied)`** |

> 环境阻塞复现确认：终审报告（10-20）记载的"未签名子进程被主机策略限制"问题今日仍然成立，非仓库代码缺陷。测试二进制（rustc 编译、未签名）无法在 `%TEMP%` 与工作区创建目录；esbuild 同源被拒。

---

## 2. 阻塞项（Blocker，发版前必须解决）

### B1 无当前源码对应的冻结候选【发布工程·P0】

- 最新二进制候选为 `artifacts/release/OpenUnpack-0.1.0-local-api-20261019-*`（10-19），此后 10-20/10-21 又落地：冲突决策与语义版控引擎、选择性提取管道、硬链接去重、损坏抢救模式、Local API 2.0（WebSocket/RFC 7807）、无头守护进程、监听文件夹、审计修复（CONT-001/BUILD-001/SEC-001~005）等大量改动——**全部不在现有候选内**。
- 台账 §1#10「发布提交与远端 CI 仍未绑定」未闭合。
- **要求**：重建"绑定统一源码"的完整候选（Worker + UI + Host + NSIS），生成载荷 SHA-256 清单并绑定源码指纹，方可进入原生验收。

### B2 全量测试证据缺失（环境阻塞未解除）【测试证据·P0】

- 137 处依赖 `tempfile::tempdir()` 的 Rust 用例（含 `http_contract`、宿主授权、核心协调器）、前端 vitest 全量（118 用例）、`npm run build` 生产构建、MCP 连接器 20 项测试，在本机均无法执行（今日复测复现）。
- 终审报告已定性：**这是主机策略限制，不是仓库缺陷**；但按准出标准，无测试证据即不能放行。
- **要求**：换用正常执行环境的机器（或为本机测试二进制添加策略豁免）补跑：  
  `cargo test --workspace`、`npm --prefix apps/desktop-ui test`、`npm --prefix apps/desktop-ui run build`、`npm --prefix apps/mcp-connector test`，全绿后方可判定。

### B3 版本号与 CHANGELOG 未收敛【配置·P0】

- 全部 manifest（根 `Cargo.toml`、`crates/protocol`、`apps/desktop-ui/package.json`、`tauri.conf.json`）仍为 `0.1.0`；而 CHANGELOG 已将 `[0.1.0]` 记为 2026-10-15 发布。
- `[Unreleased]` 分区严重滞后：仅列 2 条候选（多盘调度、自动关机），未覆盖 10-15 以来的全部功能交付与修复（headless/Local API 2.0/WS/监听文件夹/选择性提取/冲突决策/去重/跨平台/安全修复系列）。
- **要求**：明确本次发版定位后二选一——  
  ① 若 0.1.0 从未对外分发（事实上无签名、无 CI，从未真正发布）：将 0.1.0 日期改为本次实际发版日，把 Unreleased 功能并入 0.1.0 完整重写；  
  ② 若视 10-15 候选为 0.1.0 基线：按 SemVer 升版 **0.2.0**，同步更新全部 manifest + 重写 [Unreleased] → [0.2.0]。

### B4 源码未提交、未绑定 CI【发布工程·P0】

- HEAD 仅 2 个提交（`cf6e344`），工作区 292 项改动（约 98 修改 + 194 未跟踪）未入库；远端 CI 从未对当前源码执行过。
- **要求**：按模块分批提交（协议→infrastructure→core→host/UI→脚本/文档），推送远端触发 `windows.yml` 全量门禁（fmt/clippy --locked/audit/契约/lint/tsc/dotnet），以 CI 绿作为发版快照基线。

### B5 签名门禁未放行【发布工程·P0/P1，需决策】→ **已闭环（开源无签名模式）**

- **原状**：10-15/16 实测 Host 与 Setup 均 `NotSigned`，证书存储无 CodeSigning 证书，`verify-release-signatures.ps1` 按设计拒绝（负向自检通过，门禁本身工作正常）。
- **用户裁定（2026-10-22）**：本项目为开源项目，**不做代码签名**。因此签名不再是发版阻塞项，完整性凭证改为发布页的 **SHA-256 清单**。
- **处理**：`scripts/verify-release-signatures.ps1` 新增显式 `-AllowUnsigned` 开关（**默认关闭**，不改变既有严格行为）：
  - 开源模式（`Status = NotSigned` 且无签名者）视为通过，仅记录 SHA-256，证据 JSON 标记 `mode = unsigned-oss-release`、`integrityOnly = true`；
  - **若二进制确实带签名，仍完整执行「签名有效 + CodeSigning EKU + 匹配预期发行者」三项策略**（不允许用该开关绕过签名校验）；
  - 严格模式（不带开关）行为不变，未签名仍硬失败。
- **实测验证**：① `scripts/test-release-validation.ps1` **16/16** 通过（向后兼容）；② 以真实未签名 PE（便携版 `OpenUnpack.exe` + NSIS `setup.exe`）双模式复测：`-AllowUnsigned` → **PASS**（exit 0，记录 SHA-256），严格模式 → **FAIL**（exit 1，正确拒绝）。
- **文档**：README「快速上手」补充未签名说明（SmartScreen 提示属预期）与用户侧 SHA-256 校验命令、维护者侧 `-AllowUnsigned` 用法。
- **发版动作**：发布时随包提供 `SHA256SUMS.txt`（`artifacts/release-materials/` 已有生成能力），并在发布说明中提示用户校验。

### B6 原生 UI 验收与性能/稳定性基线未执行【功能验收·P1】

- 权威原生套件（`artifacts/remaining-fixes-2026-10-11/native-qa/`，套件 A/B/C/Rules）依赖「已连接且未解锁」的 WTS 会话 + 冻结候选，未对当前代码执行；跨窗口拖放自 10-15 起保留人工验收。
- 性能基线（`measure-load.ps1`）与 24h 浸泡（准出：回退 ≤10%、无崩溃/无泄漏）未跑。
- **要求**：冻结候选建成后，在交互会话完成原生套件 + 性能对照 + soak，方可放行（终审预期的"GO WITH CONDITIONS"即指此项可带条件放行）。

---

## 3. 建议修复项（不阻塞，发版前宜处理）

| #  | 项                      | 说明                                                                                                                                                        | 建议                                                 |
| -- | ---------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------- |
| S1 | CHANGELOG 全面补记         | 与 B3 联动：新增功能（headless、WS、监听、选择性提取、冲突决策、去重、抢救、智能密码挖掘、跨平台）与修复（CONT-001、BUILD-001、SEC-001~005、QUAL-003/004、P2 系列）需完整入账                                       | 重写 Unreleased，保持 Keep a Changelog 结构               |
| S2 | CSP 放宽需文档化             | `tauri.conf.json` 的 `connect-src` 新增 `http/ws://127.0.0.1:*`、`localhost:*`（守护进程远程接管所需）。范围仅回环，可接受，但属安全面变更                                                  | README 安全模型补一句说明；确认不含非回环地址                         |
| S3 | cargo-audit 白名单一致性     | 本地 `cargo audit` 5 条 allowed warnings（含 RUSTSEC-2024-0370/0429），与 CI `--ignore` 列表及 `security-audit-policy.ps1` 白名单需保持一致（历史上有过 zip 9.1 critical 漏网教训，现已修） | 复核三处清单一致；台账 §15.1-8 的 CI cargo-audit 三处冗余已收敛，确认无回归 |
| S4 | 未跟踪文件分类提交              | 194 个未跟踪文件中含新增源码（`crates/cli`、`crates/local-api`、`coordinator/` 子模块、测试、脚本）与少量产物，提交前按 `.gitignore` 与源码审计脚本（`audit-source.mjs` 拦截 `_workspace*`/产物）分类       | 随 B4 分批提交时逐一归类                                     |
| S5 | QUAL-002（jsx-a11y）过渡门禁 | 上游 peer 不支持 ESLint 10，已 WONTFIX 并登记 OD-001，`check-a11y.mjs` 作为 TEMPORARY GATE 接入 CI                                                                       | 保持现状，上游跟进后移除（按脚本头部四步清单）                            |
| S6 | P2-09 口令通道文档           | 默认 argv 传密码（实测 7-Zip 26.04 不读重定向 stdin），`OPENUNPACK_7Z_STDIN=1` 可选                                                                                        | 已有 OD-002 与 README 说明，保持                           |

---

## 4. 可接受风险（已登记、有缓解，不视为阻塞）

| #  | 风险                                                    | 缓解与登记状态                                                                                                                     |
| -- | ----------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------- |
| A1 | 7-Zip CLI 经 argv 传密码可被同机进程读取（P2-09）                   | 实测证明 stdin 通道不可行（`artifacts/p2-09-stdin-proof/`），OD-002 登记，可选 `OPENUNPACK_7Z_STDIN`；攻击面限同机本地                                |
| A2 | 便携密码库密钥模型变更（P2-12）                                    | fail-closed 显式启用（`OPENUNPACK_PORTABLE_VAULT`），UI 错误人性化提示（`humanizeVaultError`）已交付；破坏性变更已在 README 说明，属有意安全收紧                 |
| A3 | 生产执行链仍在 Core 协调器（新 TaskCoordinator 迁移另列）              | 六大双轨镜像已收敛为 infrastructure 单源 + core 兼容转发（MIRROR 双写风险消除，今日 `check-architecture` PASS）；退役按 `plan-core-retirement` 四阶段推进，非发版风险 |
| A4 | DPAPI→AES-256-GCM 密码库跨平台迁移                            | OPENVLT1 格式向下透明兼容旧 DPAPI，已有回归测试                                                                                             |
| A5 | 未验证环境清单（真正跨版本升级、干净无 SDK 机器、离线 WebView2 安装、断电/拔盘等故障组合） | 若本次定位为**社区预览版**，可显式声明边界后接受；若定位正式 GA，则升级为阻塞项                                                                                 |

---

## 5. 配置与依赖变更审查（相对 git HEAD）

**Rust（Cargo.toml / Cargo.lock）**

- 工作区新增成员：`crates/domain`、`crates/application`、`crates/infrastructure`、`crates/local-api`、`crates/cli`（分层架构落地）。
- 新增外部依赖 42 项：`axum`/`tungstenite`（Local API + WebSocket）、`notify`（文件夹监听）、`utoipa`（OpenAPI）、`zip`（升级至 9.0.5，修复 RUSTSEC-2026-0369 9.1 critical）、`zopfli`、跨平台族（`inotify`/`kqueue`/`nix`）、`windows-sys` 等；无移除项。
- `cargo audit`：0 漏洞（5 条白名单警告）。

**前端（package.json）**

- `@tauri-apps/cli` 钉死 `2.12.1`（消除模板漂移）；新增 `eslint@^10.12.0` + `typescript-eslint@^8.71.1` + `react-hooks@^7.1.1`；新增 `lint` / `lint:a11y` 脚本。

**桌面宿主（tauri.conf.json）**

- NSIS 自定义模板 `installer.nsi`，`installMode: currentUser`，默认目录 `%LOCALAPPDATA%\Programs\OpenUnpack`（与数据目录物理隔离）——10-15/16 隔离安装实测 4/4 通过（拒写数据目录、重复安装保持数据、卸载保留合成数据）。
- CSP `connect-src` 增加 localhost 回环 http/ws（守护进程接管，见 S2）。
- `beforeBuildCommand` 显式 `cwd`（修复 UI 构建钩子工作目录缺陷）。

**CI（windows.yml）**

- 新增：cargo-audit（taiki-e action，SHA pin）、安全策略负向自测、前端 lint + a11y、npm/MCP 生产依赖审计、`audit-security.ps1 -DenyWarnings`、`cargo clippy --workspace --all-targets --locked -- -D warnings`。门禁体系完整。

**兼容性关键点**

- NSIS 目录拆分：包级隔离验证通过；**真正旧版→新版升级/回滚未实测**（列入 A5）。
- 协议契约：311 字段双向对齐 + 7 项变异注入自测通过；`SnapshotDelta` 增量协议带失配回退全量，向后兼容。
- 密码库：OPENVLT1 向下兼容 DPAPI。
- Worker 协议：22 Desktop DTO / 8 Worker DTO 黄金样本测试通过。

---

## 6. 文档更新情况

| 文档                                                              | 状态                                       |
| --------------------------------------------------------------- | ---------------------------------------- |
| README / CONTRIBUTING / CODE_OF_CONDUCT / SECURITY / .github 模板 | ✅ 10-20 全面重构，含无头模式参数表、安全模型、跨栈与 MIRROR 规范 |
| CHANGELOG                                                       | ⚠️ 结构规范，但 [Unreleased] 严重滞后（见 B3/S1）     |
| 审计链文档（audit-report-final / audit-fix-log / audit-plan-final）    | ✅ 完整闭环，终审报告 §7 已更新修复记录                   |
| 状态台账（status-ledger-2026-10-11）                                  | ✅ §1–§18 全程维护；本报告建议登记进 §6                |
| 决策登记（docs/decisions/OPEN-DECISIONS.md）                          | ✅ OD-001~003 在册                          |

---

## 7. 走向 GO 的条件路径（按序执行）

1. **版本收敛**（B3）：定版并统一 bump（建议 0.2.0），重写 CHANGELOG。
2. **提交入库**（B4）：分批 commit + 推远端，CI 全绿（当前机械门禁本机已全绿，预期 CI 可过；唯 `npm test`/`build`/MCP test 步骤需在 CI 环境验证——CI 是正常环境，可顺带补齐 B2 的前端证据）。
3. **补跑全量测试**（B2）：CI 绿 + 本机（或换机）`cargo test --workspace` 全绿。
4. **重建冻结候选**（B1）：Worker + UI + Host + NSIS 一体构建，载荷 SHA-256 与源码指纹绑定。
5. **原生验收 + 性能基线**（B6）：交互会话跑 native-qa 套件 A/B/C/Rules + `measure-load.ps1` 对照（回退 ≤10%）+ 24h soak。
6. **完整性凭证**（B5，已闭环）：开源分发不做签名，以 `-AllowUnsigned` 模式跑签名门禁记录 SHA-256，并随发布包提供 `SHA256SUMS.txt` 供用户校验（README 已提供校验命令）。

**当前判定**：B3/B5 已闭环，剩余 **1（提交入库）、2（补跑全量测试）、3（重建冻结候选）、4（原生验收与性能）** 四项。

- 完成 1–3 且 4 通过 → **GO**（开源发布无需签名，完整性以 SHA-256 清单为准）；
- 完成 1–3，4 的组合故障场景（跨版本升级、干净机、离线安装）未能覆盖 → **GO WITH CONDITIONS**（需在发布说明中显式声明未验证边界）。

---

## 8. 证据索引

| 证据             | 路径 / 命令                                                                            |
| -------------- | ---------------------------------------------------------------------------------- |
| 今日门禁实测         | 本报告 §1 表格（2026-10-22 09:11–09:25 本机执行）                                             |
| 终审报告（NO-GO 根因） | `docs/audit-report-final-2026-10-20.md`（§4 未覆盖面、§7 修复记录）                           |
| 修复日志           | `docs/audit-fix-log-2026-10-21.md`、`docs/audit-fix-log-2026-10-20.md`              |
| 状态台账           | `docs/status-ledger-2026-10-11.md` §1–§18                                          |
| 环境根因           | `artifacts/audit-final-2026-10-20/GATES.md`（未签名二进制限制）                              |
| P2-09 实测       | `artifacts/p2-09-stdin-proof/`                                                     |
| 安装隔离验证         | `artifacts/release/`（audit-installer.json、installer-final-extra.json，4/4）          |
| 负载基线           | `docs/release-readiness-report-2026-10-15.md`（性能实测表）                               |
| 物料与许可          | `artifacts/release-materials/`（manifest.json + SHA256SUMS.txt，2026-10-22 07:06 刷新） |

---

## 9. 逐项处理状态台账（2026-10-22 执行轮）

状态口径：**[已闭环]** 本轮已解决并验证 · **[部分闭环]** 可执行部分已完成，剩余依赖外部条件 · **[待外部条件]** 本机/当前上下文无法解决，已给出明确所需输入 · **[维持登记]** 本就是接受项，已复核登记有效。

### 9.1 阻塞项（B1–B6）

| # | 问题 | 根因分析 | 本轮处理 | 状态 |
|---|---|---|---|---|
| B1 | 无当前源码冻结候选 | 10-20/21 大量功能与修复落地后未重新构建；候选重建依赖 `npm run build`（vite/esbuild）产出前端 dist，而 esbuild 在本机被主机策略拒绝 | 已核实最新候选停留于 10-19（`OpenUnpack-0.1.0-local-api-20261019`），与本轮源码指纹不匹配；候选重建与源码冻结必须排在提交入库之后（否则立刻过期）。**未在本机强行构建半成品候选** | **待外部条件**：需换正常机器（或解除未签名二进制限制）后执行 `prepare-desktop.ps1` → `build-desktop.ps1` → 载荷 SHA-256 冻结 |
| B2 | 全量测试证据缺失 | 主机策略禁止未签名二进制（rustc 编译的测试二进制、esbuild.exe）创建目录与读取文件：表现为 `tempdir` → `OS Error 5`、esbuild → `winapi #5`、测试二进制启动 → `0xc0000139` | 本轮**扩大可执行证据面**：protocol 8/8、domain 13/13、infra engine 9/9、core intake_guard 5/5、infra dedup 2/2、core conflict_resolver 3/3、core selective 1/1、infra salvage 1/1 全部通过；.NET Worker Release 构建 0 警告 0 错误；UI 侧 `tsc -b` / ESLint / a11y 门禁全绿。仍阻塞：worker_client 0/4（tempdir OS 5）、vault success_events 1 条（tempdir OS 5）、desktop-host 授权测试（二进制 `0xc0000139` 无法启动）、vitest 全量、npm build、MCP 测试 | **部分闭环**：可执行面已最大化取证；剩余须换机补跑 `cargo test --workspace` / `npm test` / `npm run build` / MCP test |
| B3 | 版本与 CHANGELOG 未收敛 | 功能持续叠加未触发版本管理动作；CHANGELOG 自 10-20 重构后未随交付更新 | **已闭环**：① 版本统一升 **0.2.0**（根 `Cargo.toml` workspace、protocol、domain、tauri.conf.json、desktop-ui / mcp-connector 的 package.json 与 package-lock、`package-source.ps1` 默认值），`cargo update --workspace` 同步 Cargo.lock（12 处），`openunpack-desktop v0.2.0` 编译通过验证一致性；② CHANGELOG `[Unreleased]` 全面重写，覆盖 Local API 2.0、WebSocket、无头守护进程、CLI、监听文件夹、选择性解压、冲突决策引擎、硬链接去重、抢救模式、磁盘拓扑调度、智能密码挖掘、跨平台密码库等全部 10-15 以来交付，并补齐 Fixed / Security / Changed / Deprecated | **已闭环**（若发版定位改为「0.1.0 首次对外发布」，则回退版本串并把该内容并入 [0.1.0] 段——需用户确认定位） |
| B4 | 源码未提交、未绑定 CI | 多会话并行长期在工作区直接开发，无提交节奏；HEAD 仅 2 commits | 已产出**分批提交计划**（见 §9.4）并修复提交前置风险：`.gitignore` 新增 `/_workspace/`、`/.pi/`、`apps/desktop-ui/audit/` 三条忽略规则（其中 `_workspace` 含 5 万余文件，误提交后果严重；已由 `git check-ignore` 验证生效）。**未执行提交**：检出 `_workspace/2026-10-22-source-cleanup` 表明并行会话此刻仍在改动源码，立即批量提交会快照半成品状态 | **待外部条件**：需用户确认 ① 发版定位与提交时机 ② 并行会话是否 quiesce；条件满足后按 §9.4 九批次提交并推远端跑 CI |
| B5 | 签名门禁未放行 | 开源项目本就不做 Authenticode 签名，而门禁硬失败于未签名，导致"必须签名才能放行"的误解 | 用户裁定开源不签名 → 门禁新增显式 `-AllowUnsigned` 开源模式（默认关闭）：无签名时以 SHA-256 为完整性凭证并记录 `mode=unsigned-oss-release`；**若带签名仍要求签名有效 + EKU + 匹配发行者**，不可绕过。实测：自测 16/16 通过；真实未签名 PE 双模式复测 `-AllowUnsigned` PASS / 严格模式 FAIL（行为对称）。README 补充用户侧校验命令与 SmartScreen 说明 | **已闭环**（发版时随包提供 `SHA256SUMS.txt`） |
| B6 | 原生 UI 验收与性能/浸泡未执行 | 依赖冻结候选（B1）+ 已连接且未锁定的 WTS 交互会话；当前候选缺失 | 排队依赖，未执行；准出标准已在报告中固化（原生套件全过、性能回退 ≤10%、24h soak 无崩溃无泄漏） | **待外部条件**：B1 与 B2 解除后按序执行 |

### 9.2 建议修复项（S1–S6）

| # | 项 | 根因分析 | 本轮处理 | 状态 |
|---|---|---|---|---|
| S1 | CHANGELOG 滞后 | 交付速度快于文档同步 | 与 B3 一并完成，见 B3 | **已闭环** |
| S2 | CSP 放宽需文档化 | 守护进程远程接管需要放宽 `connect-src` 到回环地址，属安全面变更但未入档 | README「安全模型与数据保护」新增一条：说明仅放行 `127.0.0.1` / `localhost` 回环 HTTP/WS、用于本机守护进程接管、不含非回环地址、关闭守护进程后无出站连接 | **已闭环** |
| S3 | cargo-audit 白名单一致性 | 历史上出现过白名单漏配导致 9.1 级漏洞在 CI 必然失败；需确认三处清单一致 | 已核对：本地 5 条 allowed warnings = RUSTSEC-2024-0370（proc-macro-error 1.0.4）、RUSTSEC-2024-0429（glib 0.18.5）、RUSTSEC-2026-0370（zvariant 5.15.0）、RUSTSEC-2026-0371（zbus_names 4.3.4）、RUSTSEC-2026-0372（zvariant_derive 5.15.0），与 `scripts/security-audit-policy.ps1` 清单**逐项一致**；且 `audit-security.ps1` 通过 dot-source 复用该策略文件为唯一真源，无重复清单 | **已闭环** |
| S4 | 未跟踪文件分类 | 194 个未跟踪文件混杂新增源码、文档、脚本与会话产物 | 已分类（见 §9.4）：源码/文档/脚本应提交；`_workspace/`、`.pi/`、`apps/desktop-ui/audit/` 归入 `.gitignore` 并验证生效；`assets/legal`、根目录工程文件纳入提交批次 | **已闭环**（提交动作并入 B4） |
| S5 | jsx-a11y 过渡门禁 | 上游 peer 不支持 ESLint 10，无法启用官方插件 | 已复核过渡门禁有效：`npm run lint:a11y` → **0 error / 0 warning** 通过，已接入 CI；OD-001 登记在册，上游支持后按脚本头部四步清单移除 | **已闭环（维持）** |
| S6 | P2-09 口令通道文档 | 7-Zip 26.04 实测不读 stdin，默认只能是 argv | 已复核 README 安全模型已记录（含 `artifacts/p2-09-stdin-proof/` 证据与 `OPENUNPACK_7Z_STDIN=1` 可选通道），OD-002 在册 | **已闭环（维持）** |

### 9.3 可接受风险（A1–A5）

| # | 风险 | 本轮复核 | 状态 |
|---|---|---|---|
| A1 | 7-Zip argv 传密码（P2-09） | 证据包与 OD-002 在册，README 已说明 | **维持登记** |
| A2 | 便携库 fail-closed 显式启用（P2-12） | `OPENUNPACK_PORTABLE_VAULT` 说明在 README；界面 `humanizeVaultError` 映射已交付；OD-003 在册 | **维持登记** |
| A3 | 生产链仍在 Core、新协调器迁移另列 | 六大双轨镜像已收敛为 infrastructure 单源 + core 兼容转发（今日 `check-architecture` PASS），MIRROR 双写风险归零；按比例退役计划推进，不构成发版风险 | **维持登记** |
| A4 | 密码库 OPENVLT1 跨平台迁移兼容 | 向下透明兼容 DPAPI，已有回归（vault 单测受环境阻塞，换机后需补跑） | **维持登记**（换机补验） |
| A5 | 未验证环境清单（跨版本升级、干净机、离线安装、故障组合） | 无新增证据；若定位预览版可声明边界接受，GA 则升级为阻塞 | **维持登记**，需用户明确发版定位 |

### 9.4 提交批次计划（B4 待确认项）

建议在并行会话 quiesce 且发版定位确认后按序提交，每批提交前跑一遍机械门禁：

| 批次 | 主题 | 主要范围 |
|---|---|---|
| 1 | `chore(baseline)` | `.gitignore`、`.editorconfig`、`.dockerignore`、`AGENTS.md`、`PLAN.md`、`CODE_OF_CONDUCT.md`、`Dockerfile`、`docker-compose.yml` |
| 2 | `feat(protocol)` | `crates/protocol`、`crates/domain` + 对应 Cargo.lock 段 |
| 3 | `feat(infrastructure)` | `crates/application`、`crates/infrastructure` |
| 4 | `feat(core)` | `crates/core`（含 `coordinator/`）、`crates/local-api`、`crates/cli` |
| 5 | `feat(host)` | `apps/desktop-host`（schemas、`installer.nsi`、`tauri.conf.json`、原生测试） |
| 6 | `feat(ui)` | `apps/desktop-ui`、`apps/mcp-connector`（含两处 package-lock） |
| 7 | `build(ci)` | `scripts/`、`scripts/ci/`、`.github/`、`tests/`（C# 与 ProtocolChecks）、`src/OpenUnpack.Engine`、`OpenUnpack.sln`、`Directory.Build.props` |
| 8 | `docs` | `docs/`（含 `docs/plans` 19 项）、`README.md`、`CHANGELOG.md`、`THIRD_PARTY_NOTICES.md`、`SECURITY.md`、`CONTRIBUTING.md`、`.workbuddy/memory` |
| 9 | `chore(release)` | 版本串 0.2.0 相关收尾与标签准备 |

> 提交前必须排除：`_workspace/`、`.pi/`、`apps/desktop-ui/audit/`、`artifacts/`（已被 `.gitignore` 覆盖）；`scripts/audit-source.mjs` 作为二次拦截门禁。

### 9.5 本轮新增实测证据（补充至 §1）

| 检查 | 结果 |
|---|---|
| `cargo test -p openunpack-domain` | 13/13 通过 ✅ |
| `cargo test -p openunpack-infrastructure --lib engine` | 9/9 通过 ✅ |
| `cargo test -p openunpack-core --lib intake_guard` | 5/5 通过 ✅ |
| `cargo test -p openunpack-infrastructure --lib dedup` | 2/2 通过 ✅（另有 1 条 vault 用例因 tempdir 环境失败，非代码缺陷） |
| `cargo test -p openunpack-core --lib conflict_resolver` | 3/3 通过 ✅ |
| `cargo test -p openunpack-core --lib selective` | 1/1 通过 ✅ |
| `cargo test -p openunpack-infrastructure --lib salvage` | 1/1 通过 ✅ |
| `openunpack-desktop v0.2.0` 编译 | 通过（验证版本 bump 一致性）✅ |
| `npm run lint:a11y` | 0 error / 0 warning ✅ |
| `cargo test -p openunpack-desktop --lib authorization` | ❌ 测试二进制 `STATUS_ENTRYPOINT_NOT_FOUND (0xc0000139)` 无法启动（环境限制同源） |
