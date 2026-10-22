# OpenUnpack 最后一次发版前审核

审核日期：2026-10-11，Asia/Shanghai。环境：Windows 11 专业版，10.0.26300，x64。

## 结论

**稳定版发版结论：changes requested，暂不放行。** 本次确认的代码缺陷已修复，代码与原生后端的最终分套门禁通过；尚没有证据支持“全部桌面功能完全可用”。新构建目录仅为内部验收候选，尚未生成新 ZIP，不能沿用此前候选的发版通过声明。

后续原生单文件选择器问题已闭合：测试驱动只按 AutomationId=1 查找，误执行了许可证文件项的 Invoke。修正为精确的原生按钮定位后，不同目录中文/空格路径、取消和界面添加4项通过；新速度候选实际 EXE 的速度/详情/暂停/写回/回读/输出验证3项通过。详见 [原生选择器修复补充](native-picker-fix-2026-10-11.md)。这些证据没有替代文件夹/多选/拖放、完整备份恢复、干净机器和物理设备的验收；稳定版结论保持不放行。

适用审查规范：[pr-review-guidelines SKILL.md](C:/Users/im/.agents/skills/pr-review-guidelines/SKILL.md) 明确要求 **“Halt approval if required validation evidence is missing.”** 本次没有通过修改或跳过断言来批准稳定版。

## 审核对象与可复现性

- 之前的 `OpenUnpack-fixed-2026-10-11-final` ZIP 摘要一致，目录 1237 个原有文件摘要全部一致；它仍有本次复现的功能问题，不再作为发版推荐包。
- 新候选目录：`artifacts/release-audit-2026-10-11/OpenUnpack-audited-2026-10-11/`，实际 Windows Release EXE、自包含 .NET 10.0.9 Worker、7z.dll 与许可材料。
- 最终构建源码：`artifacts/release-audit-2026-10-11/source-final-d570cbe5e14c473a9435df2e630a7236/`，319 个构建源码/配置摘要；包含前端 package/lock/config。测试样本另用 `TEST-FIXTURES-SHA256.json` 绑定 47 个文件。
- 完整 Core 门禁先在 `source-213a58dc39154b88bac63c4c1fa4f6c9/` 运行。之后仅 `vault.rs` 增加旧版非法条目的备份出口守卫及回归，最终源码另跑全部 14 项 Vault 回归并构建。其他产品源码摘要相同；见 `source-lineage.json`、`validation-summary.json`。
- 门禁、原生及失败日志保存在 `artifacts/release-audit-2026-10-11/`。没有提交、公开发布、创建新安装器或改动个人密码库。
- 按项目 `/review` 流程由 Codex scout、QA 两份独立审查及主 agent 复核；后续按职责修复。执行者为 Codex 子 agent，未宣称使用 AGENTS 中的 Haiku/Sonnet。

## 已修复的实际问题

以下问题均有代码及回归证据；P1 的置信度为 100，UI 辅助入口问题为 P2。

| 问题 | 影响与修复 | 证据 |
|---|---|---|
| P1 暂停队列无法继续 | 开始按钮在 paused 时允许继续；暂停且取消全部任务时 drive 释放 running 状态 | UI 暂停→继续回归；旧版暂停取消回归失败，新版通过，源包保留 |
| P1 密码文件编码不匹配 | 原 UI 的 GB18030/UTF-16LE 不被 Host 支持。统一支持；严格拒绝非法字节、奇数 UTF-16 长度和无效 surrogate，保留空格与 Unicode | Host 编码回归通过；旧 EXE 的 Unsupported import encoding 已复现。修后原生文件选择/导入整体仍未验收 |
| P1 合并分组产生非法库 | TXT/TSV 重复项及备份合并在最终 union 后校验 100 组上限；失败整批不提交、不截断原数据 | 真实 DPAPI 100+1 导入/两备份旧版失败→新版通过；合法重叠合并通过 |
| P1 可导出不可恢复的备份 | 导出和恢复共享 4096 UTF-8 字节口令上限；旧版已持久化的非法条目在导出前提示先修正，旧库仍可打开并明确修正 | 超长 ASCII/Unicode、真实重开旧库、原子失败、显式修正后备份恢复回归通过 |
| P1 大清单写回/回读超出管道限制 | copy/readback 按实际 JSON 字节分批；整体先做路径冲突/累计额度校验，各批关联独立身份，任何失败/取消阻止发布 | 旧版 16000 实体文件、9328356 字节请求超过 8388608 上限而 publishPending；新版全部原始字节、复制、目标回读、数量及源 SHA 验证通过 |
| P1 防休眠线程释放错误 | 线程级 SetThreadExecutionState 在专属线程取得和清除，异步任务跨线程 Drop 也等待该线程结束 | 实际 Windows API 回归验证同一专属线程取得/释放且返回值非零，支持重叠 guard |
| P1 C# 写入预检弱于 Rust | 两引擎拒绝 owner 标记、CLOCK$/CONIN$/CONOUT$ 与 CON 空格别名；先检查整个归档，再写文件 | 旧 DLL 8 个危险名称×2 引擎均曾写入；新版 12 种 ZIP×2 实现零写入，GNU ./、目录及安全近似名原字节提取通过 |
| P2 界面入口与状态不一致 | 节点重试只显示实际支持的内包状态；需密码/等待资源记录进入 history；查看密码失败显示失败并可关闭 | 前端回归及实际 Core history 断言通过 |

修复主要文件：`apps/desktop-host/src/lib.rs`、`apps/desktop-ui/src/components/{QueuePage,VaultPage,shared}.tsx`、`crates/core/src/{coordinator,paths,vault}.rs`、两份 C# `Native/SevenZipEngine.cs`；回归位于 Host/Core 单元测试、`desktop_pipeline.rs`、`workflows.test.tsx` 与 C# `Program.cs`。

## 最终验证结果

Rust 为**119 个不同用例分套全部通过**，不是同一最终源码一次 `cargo test --workspace` 的 119 项声明。Core 48、真实桌面管线 23、格式 6、scanner 5、WorkerClient 3，加其余 33 项为 118；最终 Vault 14 项复核包含此前 13 项及新旧数据出口 1 项。最终源码和完整 Core 门禁源码仅上述 Vault 补丁不同。

| 命令/检查 | 结果 | 日志 |
|---|---|---|
| `cargo test --manifest-path <Core门禁源>/Cargo.toml --target-dir <项目>/target --workspace --locked --offline -- --test-threads=1` | Core48、管线23、格式6、scanner5通过；首次 WorkerClient3 因期望的 bin 路径未准备失败，未把该命令整体记为成功 | `rust-final.log` |
| `cargo test ... -p openunpack-core --test worker_client --locked --offline -- --test-threads=1` | 准备实际 Worker 后 3/3 通过，真实命名管道、取消、文件句柄及退出确认 | `rust-worker-final.log` |
| `cargo test --manifest-path <最终源>/Cargo.toml ... --workspace --exclude openunpack-core --locked --offline -- --test-threads=1` | 其余33通过，0失败/忽略，含 Host3、Application3、Domain7、Infra15、Protocol5 | `rust-rest-final.log` |
| `cargo test --manifest-path <最终源>/Cargo.toml ... -p openunpack-core --lib vault::tests:: --locked --offline -- --test-threads=1` | 最终 Vault14全部通过，含5项新发版回归 | `vault-final.log` |
| `npm --prefix <最终同摘要UI>/apps/desktop-ui test` / `run build` | 27/27，TypeScript/Vite通过 | `ui-final-r2.log`、`ui-build-r2.log` |
| `dotnet run --project <最终源>/tests/OpenUnpack.Tests/OpenUnpack.Tests.csproj -c Release --no-build -- <最终源>` | 44/44，nativeAvailable=true，含新增路径回归及6个 RAR。首次38项运行未带入旧 artifacts/rar-fixtures，补齐后重跑全部44项 | `dotnet-complete-final.log`；首次 `dotnet-final.log` 保留 |
| `scripts/check-contracts.ps1 -RunCargoTest` | 19组真实 Rust→C#→Rust互读、13桌面/7Worker DTO结构、161字段及4个变异检查通过 | `contracts-final.log` |
| `src/OpenUnpack.Worker/verify-worker.ps1` | v1兼容23/23 | `v1-final.log` |
| `scripts/verify-worker-v2.ps1 -WorkerExecutable <新候选>/worker/OpenUnpack.Worker.exe -EnginePath <新候选>/worker/7z.dll` | 包内 v2 控制、候选身份及实际加密解压14断言通过 | `v2-packaged-final.log` |
| `cargo build --manifest-path <最终源>/Cargo.toml ... -p openunpack-desktop --release --features custom-protocol --locked --offline` | 实际优化 Release 构建通过 | `release-build.log` |
| `node scripts/audit-release-materials.mjs <候选材料目录>` | 488组件、1030个材料文件、52132400字节摘要一致 | `materials.log` |

首次前端新增失败态回归因同时命中标题和页脚两个“关闭”按钮失败；修正测试定位后27项通过。原生导航检查此前没有打开设置内的“界面与备份”标签，因此等待隐藏的导出按钮失败；`native-partial-debug/ui-state.json` 确认设置页面已挂载且默认显示“输出管理”。补齐实际标签点击和正确可访问名称后，限定范围的5项检查全部通过。此前失败日志保留，断言没有删除。

实际成品桌面限定验证：启动及真实 Host snapshot、第二实例拒绝共用数据库、UI添加密码与搜索、真实 Host 文本导入预览/确认与精确口令、处理记录/设置导航、无 JS 错误、正常退出。最终5项通过见 `native-partial-tabs/results.json`、`native-partial-tabs.log`，正常退出码0。它明确排除了文件选择器、完整 GUI 解压和备份流程；后者只能由 Core/Worker/DPAPI 的原生后端证据支持，不能冒充完整界面验收。

## 放行前必须闭合

1. **原生文件夹、多选与拖放**：基本单文件选择、中文/空格路径、取消和界面添加已在后续实际速度候选中通过，旧错误路径明确来自测试驱动控件 ID 冲突。文件夹、多选和拖放仍需独立成品证据。
2. **完整桌面流程**：通过上述入口导入 GB18030/UTF-16LE，界面添加任务→暂停→继续→取消/重试，界面备份→错误口令→预览→确认恢复；检查真实成品字节及重启持久性。
3. **干净 Windows 机器**：无 Node/.NET SDK、WebView2已有/缺失环境启动；若发行安装版，还需安装/卸载/升级回滚证据。本轮只构建便携候选。
4. **设备与显示验收**：物理 HDD/SSD 跨盘、目标不足/断开、断电/恢复，以及125/150/200% DPI、大队列、键盘和读屏。本机临时目录测试不是物理设备验收。

## 非阻塞警告与边界

- Cargo lib/bin 同名 PDB 提示、Vite大 chunk 提示仍存在，构建通过；没有降低检查门槛以隐藏提示。
- WebView2关闭时窗口类注销1412日志保留，正常宿主退出码0；资源长期运行仍缺验收。
- DTO 的 C# null/缺失非空引用及整数容量差异仍有明确测试记录，生产入口验证未取消；不能宣称两端所有输入语义完全相等。
- 当前生产 Host 继续使用 Core Coordinator。新 TaskCoordinator 全生命周期切换仍待开发；未装配的新层问题不当成当前任务驱动失败，也不在这次审核中宣布迁移完成。
- CSV、SFX/Zstandard及原待开发功能未加入本轮发版范围。所有源码/旧包均保留；未访问个人库，未公开发布。

本报告取代历史代码审计中的“未提交即无法复现”及“任意bulk action必panic”等结论：固定源码/摘要存在，非法action已在持锁前拒绝。原进度洪泛挤满终止队列结论也不成立，progress采用独立合并路径。发版判断基于复核和实测，不能把历史报告或已有绿灯当作全功能证明。
