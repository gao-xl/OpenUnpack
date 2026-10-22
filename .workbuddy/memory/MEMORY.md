# OpenUnpack 项目长期记忆

## 状态与文档约定（2026-10-11 起）
- 待办与状态的唯一真源：`docs/status-ledger-2026-10-11.md`（§8–§15：§9–§12 六项功能全部交付、§13 架构 7 项清零、§14 安全 SEC-01–04、§15 审计修复轮补记）。完成任一项后先更新该台账对应行与证据路径，再考虑归档旧文档；状态升级必须附命令/日志/时间戳。**编号体系三套勿混淆**：审计报告（A1/A2/S2/F1/F2/R1/R2/S1/E1/E2）· §8（R1–R8/E1–E2）· §9–§12（F1–F6 功能）。
- 历史文档（状态列已失效，勿作待办表）：`docs/acceptance-checklist.md`、`docs/feature-parity.md`；`docs/refactoring-plan*.md`、`docs/plans/*` 仅作架构背景。
- 当前修复轮施工范围：`docs/remaining-fixes-plan-2026-10-11.md`。已定决策：不搬移现有数据目录 `%LOCALAPPDATA%\OpenUnpack`；NSIS 默认程序目录将调整为 `%LOCALAPPDATA%\Programs\OpenUnpack`；不提交、不清理其他工作区改动。
- 证据报告（引用保留，位于 docs/）：native-picker-fix / feature-speed-eta / multi-area-acceptance / release-audit / issue-fixes / remaining-fixes-validation-2026-10-12（均为 2026-10-11 或 10-12）。
- 审计与计划（2026-10-12）：审计报告 `docs/audit-report-2026-10-12.md`（已登记台账 §6；A1/R1 条目已按实测修正）；计划文档 `docs/plan-snapshot-delta-2026-10-12.md`（快照增量，未实施）、`docs/plan-core-retirement-2026-10-12.md`（core 退役四阶段，Phase 0 = types 导入路径切换，零风险可先执行）。
- **MIRROR 双写规则（2026-10-12 起）**：6 个双轨镜像文件（core/infra 各自的 vault、worker client、worker manifest、database_lease）带 `//! MIRROR WARNING` 横幅；任何行为修改必须同变更集双写两处。
- **并行会话约定**：本仓库存在多会话并行写入（2026-10-12 22:30–23:30 实测）；修改共享文档/CI/脚本前先 stat mtime + 读最新内容复核；CI 中 cargo-audit 现有 3 处执行（含 `scripts/audit-security.ps1`），冗余待收敛（台账 §15.1-8）。
- 审查复现资产：`artifacts/comprehensive-review-2026-10-12/`（原版 `rust-repro` 保留原样；修复后适配版 `rust-repro-postfix` 写入 `core-reproduction-postfix-results.json`，不改写原始证据）。仓库内永久回归：`crates/core/tests/resource_recovery_regressions.rs`（R1–R4）。

## 环境与工具约定
- **原生验收前置条件**：WTS 会话必须「已连接**且未锁定**」——`LogonUI` 在场（锁屏）时 WebView2 静默不挂载（仅超时，无报错），`qwinsta` 显示"已激活"不代表可用。
- **权威原生套件**：`artifacts/remaining-fixes-2026-10-11/native-qa/`（2026-10-11 起）；`artifacts/acceptance-2026-10-11/` 为历史参照（其驱动器两处潜在缺陷已于 21:10 修复：SendMessageText 入口点、exists 校验）。
- Windows 构建脚本需 pwsh 7（系统自带 5.1 会按 ANSI 误读 UTF-8 脚本导致解析失败）。可用：`C:\Users\im\.cache\codex-runtimes\codex-primary-runtime\dependencies\native\powershell\pwsh.exe`。
- 验收工作区：`artifacts/acceptance-2026-10-11/`（冻结候选、套件 A/B/C、dragdrop-sim、file/folder 对话框驱动、installer-test.ps1、cleanup-webview.ps1）。
- 候选冻结约定：构建输出立即复制到独立目录，并生成 BUILD-SOURCE-SHA256.txt / SHA256SUMS.txt；原生用例一律绑定该候选目录执行。
- 会话隔离要求：避免共享 `target/` 的并发构建（可致协议 crate 元数据陈旧型 E0425 抖动，check 通过而 build 失败）；强杀应用后用 `cleanup-webview.ps1` 清理孤儿 msedgewebview2（按启动时间过滤，避免误杀 SearchHost/M365Copilot 实例）。
- 遗留进程：PID 20220/30948 已于 2026-10-12 复核退出（无活动锁），本条关闭。
- 本机物理布局（验收真值）：`C:` = Samsung MZVLB512HBJQ NVMe SSD（磁盘 1）；`D:` = Toshiba MQ04ABD200 SATA HDD（磁盘 0）。跨盘测试夹具：`D:\openunpack-accept\`。
