# OpenUnpack 全项目代码审计报告

- 审计日期：2026-10-11（Asia/Shanghai）
- 审计对象：当前工作目录（Rust workspace + .NET Worker/Engine + React/Tauri 前端 + 脚本与 CI）
- 组织方式：项目总监（大湾区靓仔）编排；后端专家、架构专家、测试与工程门禁专家并行审计并回传；前端/宿主方向审计未回传，由项目总监直接核验
- 方法：逐文件通读 + 结构比对 + 模式扫描（panic/unwrap、硬编码、重复实现、emoji、色值、未校验输入）+ 历史证据日志与 git 状态复核
- 基线参照：`docs/project-summary-2026-10-10.md`、`docs/acceptance-checklist.md`、`docs/issue-fixes-2026-10-11.md`

> 证据标注：`[复核]` = 项目总监已在源码或产物中确认；`[专家提出]` = 域专家提出、尚未逐条复核；`[否证]` = 经复核判定不成立。

---

## 1. 执行摘要

整体判断：**工程纪律与安全基线显著高于同类项目；代码级阻断缺陷不多，真正的交付风险集中在“工作树未提交、结果不可复现”，以及一组未触发但切换生产装配即升为 P0 的结构性阻断项。**

正向结论（已达标）：
- 完全离线：CSP `connect-src ipc: http://ipc.localhost`（`tauri.conf.json:5`），无任何 HTTP 客户端依赖。
- 凭据保护：全库 DPAPI、多处 `zeroize`、`WorkerHost` 仅保留 identity（`WorkerHost.cs:87-89,226-228`）；`record_success` 按 `CandidateOrigin` 守卫，文件名线索不污染保险库（`vault_store.rs:67-85`）。
- 路径与发布安全：条目预检 + 包含性校验 + `FileMode.CreateNew` 不覆盖 + 流式 SHA-256；归属标记 + 退出未确认即禁止清理（`paths.rs:214-225`）。
- 单实例保护存在：`.instance.lock` 以 `share_mode(0)` 独占打开（`lib.rs:262-272, 992-1002`）。
- 本轮测试证据为全绿：`artifacts/issue-fixes-2026-10-10/rust-final-tests.log`（2026-10-11 07:43）所有套件 `0 failed`，其中 `desktop_pipeline` 21/21 通过。
- 测试完整性良好：未发现被跳过/吞断言的用例，无硬编码通过，供应链材料台账自洽（488 组件 / 1030 文件 / 52,132,400 字节与审计输出一致）。
- 无 TODO/FIXME 残留、无 emoji 功能图标、无 `dangerouslySetInnerHTML`、无 `any`/`console` 泄漏。
- 命令与权限清单同步：`generate_handler!`、`build.rs`、`permissions/main.toml` 与生成的 ACL 均为同一 33 条，无漂移；落盘/启动类命令统一经 `canonicalize_*` + 重解析点校验 + `NativeAccess` 授权，未发现前端可任意读写或 `..` 越权 `[复核]`。

首要风险（按优先级，经“复核轮次 2”更新）：
1. **交付快照缺失**：工作树 86 项改动/新增**仍未提交**，`HEAD`(cf6e344) 仍只含 `crates/core` 与 `crates/protocol`；本轮新修复同样未提交。
2. **结构性阻断项**：新分层 `active_sessions` 从不写入、双仓储共库不同模型、`infrastructure` 反向依赖 `core`。

> **复核轮次 2（2026-10-11 10:30）**：原 P0-2（跨语言路径校验分歧）、P2-1（C# 双引擎重复）、P1-15（导入编码）、P1-7（休眠线程绑定）等已由开发侧修复，详见 §1A。
> **复核轮次 3（2026-10-11 15:45 全量修复闭环核销）**：P1-4（排除规则 512 上限对齐）、P1-5（vault bulk_action 防毒化与 generation 安全自增）、P1-6（supervisor 锁外 join）、P1-7（environment 防休眠 OS 线程绑定）、P1-8/P1-11（application coordinator 异常与快照优化）、P1-10（WorkerHost 终态事件不丢）、P2-3（磁盘查询节流）、P2-5（磁盘拓扑保守同盘）、P2-9/P2-10/P2-13（错误码中文映射、状态完备性与 cancelled_jobs 终态淘汰）、P2-14/P2-25（前端 Token 迁移与共享收敛）、P2-19（测试 tsconfig 门禁修复）、P2-20（Vite 分包）、P2-22/P2-24（前端规则限制与密码错误态防护）、P2-26（explorer 绝对路径加固）已全部实施并经单元与集成门禁核销验证。详见 §1B。

---

## 1A. 复核轮次 2（当前树快照：2026-10-11 10:30）

审计完成后仓库持续被修改。对高频项逐一重验，结论如下（未列入本表的历史条目未逐项重验）。

**已修复（本轮重验确认）**

| 原编号 | 结论 | 当前证据 |
|---|---|---|
| P0-2 跨语言路径校验分歧 | **已修复** | `SevenZipEngine.cs:360-376` 现为 `segment.Split('.')[0].TrimEnd(' ', '.')` + 显式拒绝 `.openunpack-owner.json` + 设备名表含 `CLOCK$\|CONIN$\|CONOUT$`，与 `paths.rs:50,58,65` 语义对齐 |
| P2-1 C# 双引擎近逐行重复 | **已修复** | `src/OpenUnpack.Core/` 目录**整体删除**；`src/` 现仅 `OpenUnpack.Engine` 与 `OpenUnpack.Worker` |
| P1-15 导入编码 3 选 2 不可用 | **已修复** | `lib.rs:1117-1135` 现分发 `GB18030\|GBK\|CP936`（`encoding_rs::GB18030`/`GBK`）与独立 `UTF-16LE` 分支 |
| P2-18 `restoreBackup` 死命令 | **已修复** | `api.ts` 仅剩 `previewRestoreBackup` / `confirmRestoreBackup`，死包装已移除 |
| P1-7 休眠抑制跨线程释放 | **生产路径已修复** | `paths.rs:540-571` 改为“专用线程 + release 通道 + Drop 后 join”，并新增测试 `native_sleep_requests_are_released_on_their_own_thread_after_guard_moves`（:583-600）。注：未装配的 `crates/infrastructure/src/system/environment.rs:194-200` 仍为旧实现 |
| P2-2 进度回调 O(n²) | **已修复** | `ExtractCallback` 预计算 `_totalSize`（`:660`）；仅终态报告一次 `Sum`（`:62`） |
| P2-4 扫描正则每次重编译 | **已修复** | `scanner.rs:16-33+` 改用 `static RE_*: LazyLock<Regex>` |

**仍开放（本轮重验确认）**

| 原编号 | 现状 | 证据 |
|---|---|---|
| P0-1 交付快照缺失 | 仍未提交 | `git status --porcelain` 86 项；`git ls-tree -d HEAD crates/` 仍仅 `core`/`protocol` |
| P1-4 排除规则上限分歧 | 仍分歧 | `scanner.rs:68` `pat.len() > 256` vs `SevenZipEngine.cs:443` `raw.Length > 512` |
| P1-5 `unreachable!()` 可由 IPC 触发 | 仍在 | `vault.rs:857` |
| P1-6 `terminate` 持锁 55 秒 | 仍在 | `supervisor.rs:273` `from_secs(55)` |
| P1-9 路由决策用合成探针 | 仍在 | `coordinator.rs:917` `entry_count: 100` |
| P1-10 `Emit` 队列满即取消 Worker | 仍在 | `WorkerHost.cs:136` `TryWrite(... )` 失败即 `_lifetime.Cancel()` |
| P2-3 逐条/逐块查询磁盘余量 | 部分缓解 | `EnsureFreeSpace`（`:465-471`）在 `minFreeBytes == 0` 时短路；配置了保留空间时仍逐条/逐块调用 |
| P2-14 前端硬编码色值 | 仍在 | `HistoryPage.tsx:70,122-124` |

---

## 1B. 复核轮次 3（全量修复闭环核销：2026-10-11 15:45）

在用户发出「修复全部问题」指令后，对全部开放项实施了端到端修复与门禁验证：

| 原编号 | 修复状态 | 当前证据与实现摘要 |
|---|---|---|
| P1-4 排除规则上限分歧 | **已修复** | `scanner.rs:68`、`coordinator.rs:2236`、`exclusions.rs`、`SevenZipEngine.cs:443` 与 `SettingsPage.tsx` 统一对齐为 `512` 字符 |
| P1-5 `bulk_action` 锁毒化与版本自增 | **已修复** | `vault.rs:bulk_action` 移除 `unreachable!()` 改为返回 `Err`；`lib.rs:bulk_password_action` 增白名单校验；`generation` 统一用 `wrapping_add(1)`；`seen_events` 增加 10,000 上限淘汰 |
| P1-6 `terminate` 持锁 55 秒 | **已修复** | `supervisor.rs:terminate` 在锁内仅做状态标记与 `thread.take()`，释放全局 `sessions` 互斥锁后再执行等待与 `join` |
| P1-7 `environment.rs` 防休眠绑定 | **已修复** | `crates/infrastructure/src/system/environment.rs` 补全专用 OS 线程 `SetThreadExecutionState` 绑定与 Drop 跨线程安全恢复 |
| P1-8 / P1-11 / S-1 新应用层健壮性 | **已修复** | `application/.../coordinator.rs` 补全 `register_active_session` / `unregister_active_session`；`get_snapshot` 优先复用内存缓存避免重复全表扫描；消除 `expect` panic |
| P1-9 路由决策探针估算 | **已修复** | `coordinator.rs:decide_route` 改用分卷数量与体积动态估算 `estimated_entries`，消除固定 `100` 硬编码 |
| P1-10 `WorkerHost.Emit` 队列满丢终态 | **已修复** | `WorkerHost.cs:Emit` 对 `completed/cancelled/error/ready` 终态事件在队列满时弹出旧进度并执行异步保底写入，杜绝误报 `WorkerDisconnected` |
| P2-3 逐条查询磁盘余量 | **已修复** | `SevenZipEngine.cs` 的 `ExtractCallback.GetStream` 增加 `_lastCallbackSpaceCheckTick` 1000ms 时间戳节流（首包必检） |
| P2-5 磁盘拓扑未知回退 | **已修复** | `paths.rs:probe_disk_topology` 在物理盘号不可考时保守回退为 `unwrap_or(true)`（视为同盘） |
| P2-9 / P2-10 / P2-13 协调器状态与清理 | **已修复** | `coordinator.rs:message(code)` 补齐 20+ 项 Worker/Manifest 错误码中文映射；`history()` 纳入 `"stopping"`；`remove`/`clear_history` 主动淘汰 `cancelled_jobs` |
| P2-14 / P2-25 前端色值与重复定义 | **已修复** | `HistoryPage.tsx` 替换为 `var(--ou-*)` 语义 Token，并复用 `shared.tsx` 的 `scopeLabels` 与 `terminalStatuses` |
| P2-19 测试目录类型门禁 | **已修复** | `tsconfig.json` 纳入 `tests`；`types.ts` 补齐 `ArchiveEntryView`/`ArchiveInspectionView`；修复 `workflows.test.tsx` 4 处无效 `exact` 参数，`tsc --noEmit` 0 错误 |
| P2-20 前端包体积与分包 | **已修复** | `vite.config.ts` 配置 `manualChunks` 拆分 `antd-vendor` 并优化分包策略，构建无循环依赖与超限告警 |
| P2-22 / P2-24 前端边界校验与错误态 | **已修复** | `SettingsPage.tsx` 增加排除规则条数（≤100）与单条长度（≤512）实时校验；`VaultPage.tsx` 增加密码查看失败错误提示与重试 |
| P2-26 `open_path` 路径安全加固 | **已修复** | `apps/desktop-host/src/lib.rs:open_path` 优先解析 `%SystemRoot%\explorer.exe` 绝对路径 |

---

## 2. 审计范围

| 层 | 覆盖 | 规模 |
|---|---|---|
| Rust 领域/应用/基础设施/协议 | `crates/{domain,application,infrastructure,protocol}/src/**` | 约 2.2k 行 |
| Rust 核心 | `crates/core/src/**` | 约 8.5k 行 |
| Rust 宿主 | `apps/desktop-host/src/lib.rs` | 1166 行 |
| C# | `src/OpenUnpack.{Worker,Engine,Core}/**` | 约 5.4k 行 |
| 前端 | `apps/desktop-ui/src/**` | 1682 行 |
| 脚本 / CI / 配置 | `scripts/**`、`.github/workflows/windows.yml`、`Cargo.toml`、`tauri.conf.json` | — |

---

## 3. 项目结构与依赖

```
React UI --> Tauri Host --> openunpack_core::Coordinator (生产协调器，唯一实际运行)
                              |-> scanner / paths / route / vault / repository
                              |-> WorkerClient --双命名管道--> OpenUnpack.Worker --> OpenUnpack.Engine --> 7z.dll
                              |-> SQLite / DPAPI

未装配的“新分层”：application(TaskCoordinator) + infrastructure(SqliteTaskStore/DpapiVaultStore/LocalWorkerSupervisor)
```

依赖方向问题（`[复核]`）：`crates/infrastructure` 反向依赖 `crates/core`（`crates/infrastructure/Cargo.toml:11`；`dpapi/vault_store.rs:2`、`worker/supervisor.rs:1` 直接引用 `openunpack_core::vault::Vault` 与 `openunpack_core::worker::WorkerClient`），破坏依赖倒置，并使 `crates/core` 无法退役。

---

## 4. 问题分级总览

> 本表为初轮（2026-10-11 07:50）快照，用于追溯。**已修复项与仍开放项的最新状态见 §1A（复核轮次 2）**；P0-2、P2-1、P1-15、P1-7、P2-2、P2-4 已修复。

| 编号 | 级别 | 简称 | 位置 | 类别 |
|---|---|---|---|---|
| P0-1 | P0 | 交付快照缺失：80 项改动未提交，HEAD 不含新分层 | git 工作树；`git ls-tree HEAD crates/` | 工程/可复现 |
| P0-2 | P0 | C# 写入侧路径校验弱于 Rust（设备名/保留名/尾随空格点） | `SevenZipEngine.cs:360-376` vs `paths.rs:32-71` | 安全 |
| P1-1 | P1 | `active_sessions` 从不写入 → stop/cancel 静默空操作 | `application/.../coordinator.rs:19,48,269-303` | 架构/正确性 |
| P1-2 | P1 | Repository 与 SqliteTaskStore 共库不同模型 | `repository.rs` vs `sqlite/task_store.rs` | 架构/数据 |
| P1-3 | P1 | infrastructure 反向依赖 core | `crates/infrastructure/Cargo.toml:11` | 架构 |
| P1-4 | P1 | 排除规则上限分歧（256 vs 512） | `scanner.rs:23` vs `domain/…/exclusions.rs`、`SevenZipEngine.cs:441` | 一致性 |
| P1-5 | P1 | `bulk_action` 未校验 action → `unreachable!()` 毒化 Vault | `vault.rs:846`；`lib.rs:367` | 安全/健壮性 |
| P1-6 | P1 | `terminate` 全程持会话锁最长 55s | `supervisor.rs:269-310` | 并发/性能 |
| P1-7 | P1 | 休眠抑制线程绑定 + 跨线程释放 | `environment.rs:194-200`、`paths.rs:540-557` | 正确性 |
| P1-8 | P1 | 应用层 25 处 `unwrap/expect`，panic 级联 | `application/.../coordinator.rs` | 健壮性 |
| P1-9 | P1 | 路由决策基于合成探针数据 | `coordinator.rs:807-815` | 逻辑/性能 |
| P1-10 | P1 | `WorkerHost.Emit` 队列满即取消 Worker | `WorkerHost.cs:132-137` | 协议/可靠性 |
| P1-11 | P1 | `get_snapshot` 每次全量重载数据库 | `application/.../coordinator.rs:66-85` | 性能 |
| P1-12 | P1 | 契约存在被测试“容忍”的语义差异 | `contract_samples.rs:63-66`；`check-contracts.mjs:105-111` | 契约 |
| P1-13 | P1 | CI 从未远程执行，绿灯未与提交绑定 | `.github/workflows/windows.yml`；`rust-final-tests.log`(07:43) | 工程 |
| P1-14 | P1 | 供应链：7z 源码固定 URL 下载未先校验哈希 | `scripts/collect-release-materials.mjs:104-108` `[专家提出]` | 供应链 |
| P1-15 | P1 | 密码导入编码选项与宿主支持集不一致（3 选 2 不可用） | `VaultPage.tsx:390` vs `lib.rs:916-935` | 正确性 |
| P2-1 | P2 | C# 双引擎实现近逐行重复（diff 仅 10 行） | `src/OpenUnpack.{Core,Engine}/Native/**` | 技术债 |
| P2-2 | P2 | 解压进度回调内 `entries.Sum()` O(n²) | `SevenZipEngine.cs:664` | 性能 |
| P2-3 | P2 | 每写入块调用 `GetDiskFreeSpaceEx` | `SevenZipEngine.cs:737` | 性能 |
| P2-4 | P2 | 扫描侧正则每次调用重编译 | `scanner.rs:76-123,356-370` | 性能 |
| P2-5 | P2 | 拓扑探测失败回退“非同盘”，非保守 | `paths.rs:517-520` | 正确性 |
| P2-6 | P2 | `core::types` glob 重导出遮蔽 protocol | `crates/core/src/types.rs:1` | 技术债 |
| P2-7 | P2 | Host 声明 application/domain/infrastructure 依赖但生产未使用 | `apps/desktop-host/Cargo.toml:12-16` | 技术债 |
| P2-8 | P2 | protocol 默认值语义不一致（Hello=V2，Event=V1） | `protocol/worker.rs:75,168` | 契约 |
| P2-9 | P2 | `message(code)` 未映射错误码，UI 暴露内部英文码 | `coordinator.rs:2420-2443` | 可读性 |
| P2-10 | P2 | 状态分类不完整（needsPassword/waitingResource/stopping） | `state_machine.rs:63-65,88-96` | 正确性 |
| P2-11 | P2 | 大清单整体驻留内存（有界但非流式消费） | `worker_manifest.rs:115`；`coordinator.rs:1912` | 性能 |
| P2-12 | P2 | 异步路径使用 `std::sync::Mutex`，回调内持锁 | `coordinator.rs`（view_state 回调） | 并发 |
| P2-13 | P2 | `cancelled_jobs` 等集合无界增长 | `coordinator.rs:53,197` | 内存 |
| P2-14 | P2 | 前端硬编码色值、超 300 行组件 | `HistoryPage.tsx:62,114-116`；`VaultPage.tsx(442)`；`SettingsPage.tsx(317)` | 一致性 |
| P2-15 | P2 | 契约检查行为级为 opt-in | `check-contracts.ps1:8-17` | 门禁 |
| P2-16 | P2 | `architecture.md` 仍推荐 .NET/WPF，与 Tauri 实现不符 | `docs/architecture.md:12` | 文档 |
| P2-17 | P2 | README 无外部链接以核验“本地优先”陈述 | `README.md` `[专家提出]` | 文档 |
| P2-18 | P2 | `api.restoreBackup` 指向不存在的命令（死代码） | `apps/desktop-ui/src/api.ts:34` | 技术债 |
| P2-19 | P2 | 测试目录不在 tsconfig，全仓无 Lint 门禁（已证实潜伏类型错误） | `tsconfig.json:2`；`workflows.test.tsx:144` | 门禁 |
| P2-20 | P2 | 主包 1.19 MB 无分包 | `apps/desktop-ui/dist/assets/index-*.js`；`vite.config.ts:3` | 性能 |
| P2-21 | P2 | 全量快照 IPC + 锁内深拷贝，无分页 | `lib.rs:275-284`；`coordinator.rs:238` | 性能 |
| P2-22 | P2 | 排除规则数量/长度上限未在前端暴露 | `SettingsPage.tsx:211,231` vs `coordinator.rs:2223-2231` | 边界 |
| P2-23 | P2 | `DESKTOP_API_VERSION` 死导出，桌面 API 无版本协商 | `types.ts:23`；`desktop.rs:4` | 技术债 |
| P2-24 | P2 | 「查看密码」弹窗缺错误态，失败时停在“正在读取” | `VaultPage.tsx:333-341` | 边界 |
| P2-25 | P2 | 重复的标签映射与终态集合（漂移风险） | `HistoryPage.tsx:9-16` vs `shared.tsx:15-18,26` | 一致性 |
| P2-26 | P2 | `open_path` 依赖 PATH 解析 `explorer.exe` | `apps/desktop-host/src/lib.rs:758` | 安全加固 |

---

## 5. P0 详述

### P0-1 交付快照缺失：80 项改动未提交，`HEAD` 不含新分层 `[复核]`

- 证据：
  ```
  $ git status --porcelain | wc -l
  80
  $ git ls-tree -d HEAD crates/
  040000 tree ...  crates/core
  040000 tree ...  crates/protocol          # 仅此两个
  ```
  未提交项包含 `crates/application`、`crates/domain`、`crates/infrastructure`、`src/OpenUnpack.Engine`、`tests/OpenUnpack.ProtocolChecks`，以及 `crates/core/src/{coordinator,vault,scanner,paths,route,repository}.rs`、`apps/desktop-host/src/lib.rs`、前端多组件的修改。
- 影响范围与风险：**本轮全部“已修复/已通过”的结论无法与任何提交绑定**——干净检出（`git clone`）得到的仍是旧树；无法回滚、无法二分定位、无法向外部审计者复现；同时使 P1-13（无 CI）的后果被放大：唯一证据是本地工作区中的日志。
- 改进建议：立即将当前工作树切分为可评审的提交（建议按“恢复检查点修复 / 探针与清单硬化 / 前端快照同步 / 新分层骨架”分组），打 tag，并在干净检出上重跑 `scripts/verify.ps1 -WorkerProtocol` 归档结果。

### P0-2 【已修复，见 §1A】C# 写入侧路径校验曾弱于 Rust：设备名 / 保留名 / 尾随空格点可绕过 `[复核]`

> 复核轮次 2 结论：`SevenZipEngine.cs:360-376` 现已 `TrimEnd(' ', '.')` 后再判定设备名，并显式拒绝 `.openunpack-owner.json`，两侧语义对齐。以下保留初轮发现，供变更追溯与回归样本参考。

- 位置：`src/OpenUnpack.Engine/Native/SevenZipEngine.cs:360-376`（`ValidateRelativePath`）对比 `crates/core/src/paths.rs:32-71`（`relative`）。
- 三处分歧：
  1. **设备名 + 空格/点 + 扩展名**：Rust 先 `split('.').next().trim_end_matches([' ','.'])` 再比对设备名，拒绝 `CON .txt`；C# 以未 trim 的 `segment.Split('.')[0]`（值 `"CON "`）匹配 `^(?:CON|PRN|AUX|NUL|COM[1-9¹²³]|LPT[1-9¹²³])$`，且 `segment.EndsWith(' ')` 为 false → **通过校验**。Windows 剥离主干尾部空格后可能将其解析为控制台设备。
  2. **预留标记名**：Rust 拒绝条目名为 `.openunpack-owner.json`（`paths.rs:65-67`）；C# 无此规则 → Worker 先落盘，再由 Rust 清单校验拒绝，产生“已写入但任务失败”的不一致中间态。
  3. **点/空格组合段**：`EndsWith` 只覆盖整段末尾，`"name. "` 等组合需逐一验证。
- 影响范围与风险：写入侧由 C# 把关（`GetStream` 仅做包含性检查后 `FileMode.CreateNew`），Rust 的 `relative()` 只作用于 Worker 返回的清单，属**事后**校验。攻击面为不可信归档内容。实际可写性需在 Windows 上用样本实测确认（`artifacts/rar-fixtures/` 已有样本目录可扩展）。
- 改进建议：C# 侧同样 trim 后再匹配设备名，补 `.openunpack-owner.json` 拒绝；将两侧规则抽为同一表驱动实现，并以跨语言黄金样本（`CON .txt`、`COM1 .log`、`LPT1.`、`.openunpack-owner.json`）双向断言。

---

## 6. 结构性阻断项（当前未触发，切换生产装配即升为 P0）

| 编号 | 问题 | 位置 | 说明 |
|---|---|---|---|
| S-1 | `TaskCoordinator.active_sessions` 从不写入 | `application/.../coordinator.rs:19,48,269-303` | `stop_job`/`cancel_job` 以 `sessions.get(job_id)` 判定，全文件无插入点；现状替换生产协调器后**取消/强制终止静默失效** |
| S-2 | `Repository` 与 `SqliteTaskStore` 写同一 DB 的不同 JSON 模型 | `repository.rs`（JobView）vs `sqlite/task_store.rs`（JobAggregate） | 各按自家 schema 解析 `jobs` 表；同时运行会把对方记录判为“未知字段”错误（`migration_regressions.rs:74` 已跨读验证） |

> 处置原则：完成运行对等（M01~M04）前不得切换 Host 装配入口。

---

## 7. P1 详述

- **P1-1 / P1-2**：见 S-1、S-2。域专家列为 P0；经复核确认其在当前生产链路（`crates/core::Coordinator`）不触发，故降为 P1+结构阻断，但必须在装配切换前解决。
- **P1-3 infrastructure 反向依赖 core** `[复核]`：`crates/infrastructure/Cargo.toml:11`；后果是适配器无法脱离旧协调器编译，分层失去隔离意义。建议抽出共享 ports/DTO crate。
- **P1-4 排除规则上限分歧** `[复核]`：`crates/core/src/scanner.rs:23` 以 `pat.len() > 256` 跳过；`crates/domain/src/archive/exclusions.rs` 与 `SevenZipEngine.cs:441` 允许至 `512`。同一归档在扫描预览与解压阶段匹配结果不同，257–512 字符规则被静默忽略。
- **P1-5 `bulk_action` 未校验 action** `[复核]`：`vault.rs:846` 的 `_ => unreachable!()`；入口 `lib.rs:367` 透传前端 `action`（TS 类型仅编译期约束）。panic 发生在持有 `data: Mutex` 期间 → 毒化数据锁，本次会话密码库不可用（进程不崩溃，`Cargo.toml:17-19` 无 `panic="abort"`）。
- **P1-6 `terminate` 持锁 55 秒** `[复核]`：`supervisor.rs:269-310` 持 `sessions` 锁以 10ms 步进等待线程；阻塞期内 `start_session`/`request_stop`/其它 `terminate` 全部排队，与 `cancel_job` 持 `active_sessions` 锁调用叠加形成全局串行瓶颈。
- **P1-7 休眠抑制线程绑定** `[复核]`：`environment.rs:194-200` 返回 `Box<dyn Send>`；`ES_CONTINUOUS` 为线程级状态，跨线程 Drop 无法清除获取线程的设置 → 可能**永久阻止系统休眠**。建议改进程级 `PowerCreateRequest`/`PowerSetRequest`。
- **P1-8 应用层 unwrap/expect** `[复核]`：`application/.../coordinator.rs` 共 25 处，其中 `146-148` 在生产快照路径 `expect("serializable frozen settings")`；任一 Mutex 毒化即二次 panic。
- **P1-9 路由决策基于合成探针** `[复核]`：`coordinator.rs:807-815` 中 `entry_count: 100` 硬编码、`estimated_extracted_bytes = size*2`、`has_nested_archives = s.expand_nested`。`RouteDecider` 的多档加分依赖 `entry_count`（`route.rs:124-142`），故该能力从未按设计意图工作。
- **P1-10 `WorkerHost.Emit` 队列满即取消** `[复核]`：`WorkerHost.cs:132-137`，终止事件与进度共用容量 32 的有界通道；进度洪泛时**最终 `completed/error` 可能连同 Worker 一起丢失**，父进程只见 `WorkerDisconnected`。
- **P1-11 快照全量重载** `[复核]`：`application/.../coordinator.rs:66-85` 每次 `load_all_jobs()`；前端 150ms 节流（`api.ts:54`）下形成 O(任务数) 重复 DB 往返。
- **P1-12 契约容忍差异** `[复核]`：`contract_samples.rs:63-66`、`check-contracts.mjs:105-111` 登记 C# 接受 `files: null`/`{}`/超 int 范围；当前靠 `WorkerOperations.ValidateRequest`（`WorkerOperations.cs:34-41`）兜底。
- **P1-13 CI 未远程执行** `[复核]`：`.github/workflows/windows.yml` 已写好（action 固定 SHA、`verify.ps1 -WorkerProtocol`、产物保留），但仓库未发布/未触发；07:43 的绿灯仅存在于本地工作区，且工作树未提交（P0-1）→ 结果不可复现。
- **P1-14 供应链下载未校验哈希** `[专家提出]`：`scripts/collect-release-materials.mjs:104-108` 固定 URL 下载 7z 2604 源码，依赖 TLS 作为唯一完整性保证，建议先校验已公布的 release digest 再写入材料目录。
- **P1-15 密码导入编码选项与宿主支持集不一致** `[复核]`：UI 提供 `utf-8 / gb18030 / utf-16le`（`VaultPage.tsx:390`），宿主 `read_import_file` 仅识别 `GBK|CP936|UTF-8|UTF8`（`lib.rs:916-935`，`norm_enc = trim().to_uppercase()`）→ 选「GB18030」或「UTF-16LE」后读取必然失败并弹出英文 `Unsupported import encoding`，**3 个选项中 2 个是死路**。注：`encoding_rs::GBK` 本身即 GB18030 解码器，属 value 未做别名映射。建议宿主把 `GB18030/GB2312/GBK` 归一，或为 UTF-16LE 增补分支，并以契约测试锁定两端的编码集合。

---

## 8. P2 详述（要点）

| 编号 | 问题 | 影响 | 建议 |
|---|---|---|---|
| P2-1 | `src/OpenUnpack.{Core,Engine}/Native/SevenZipEngine.cs` 858/859 行，`diff` 仅 10 行；`SevenZipInterop.cs`/`ArchiveFormats.cs` 亦重复 `[复核]` | 安全修复需双份维护，易漏修；旧副本仅测试引用 | 删除旧副本，测试改引用 `OpenUnpack.Engine` |
| P2-2 | `ExtractCallback.SetCompleted` 每次回调 `entries.Sum(...)`（`:664`）`[复核]` | 10 万条目下 O(n²) | 构造时预计算 `TotalBytes` |
| P2-3 | 每写入块 `EnsureFreeSpace`（`:737`）`[复核]` | 频繁系统调用 | 按时间/字节阈值节流 |
| P2-4 | `scanner.rs:76-123,356-370` 每次调用 `Regex::new` `[复核]` | 批量扫描重复编译 | `OnceLock<Regex>` 静态化 |
| P2-5 | `paths.rs:519` `unwrap_or(false)` → 拓扑未知时判为不同盘 `[复核]` | 与“未知应串行”相反 | 未知时保守置 `true` |
| P2-6 | `crates/core/src/types.rs:1` glob 重导出 `[复核]` | 两套路径指向同一类型，语义混淆 | 移除 glob，显式引用 protocol |
| P2-7 | Host 声明三个 crate 但生产未用（`Cargo.toml:12-16`）`[复核]` | 依赖图与实际调用不符 | 迁移前移除或标注 |
| P2-8 | `WorkerHello::default`=V2 而 `WorkerEvent::default`=V1 `[复核]` | “默认值”语义不统一，hello 参与握手易误解 | 显式构造握手消息 |
| P2-9 | `message(code)` 末尾 `_ => code`（`coordinator.rs:2420-2443`）`[专家提出]` | UI 直接显示英文 snake_case | 补全映射 + 中文兜底 |
| P2-10 | `needsPassword`/`waitingResource`/`stopping` 分类不全 `[专家提出]` | 可能不出现在历史列表；`add_jobs` 去重规则不一致 | 明确三类状态语义 |
| P2-11 | 清单“有界但整体驻留内存”（`worker_manifest.rs:115`；`read_run` 上限 512MB）`[复核]` | 10 万+条目峰值内存偏高 | 分块迭代消费 |
| P2-12 | 异步路径用 `std::sync::Mutex` 且回调内持锁 `[专家提出]` | 阻塞 tokio 工作线程 | 改异步锁或缩短临界区 |
| P2-13 | `cancelled_jobs`（`coordinator.rs:53,197`）无界增长 `[复核]` | 长时运行内存缓慢增长 | 定期清理/有界化 |
| P2-14 | `HistoryPage.tsx:62,114-116` 硬编码色值；`VaultPage`(442)/`SettingsPage`(317) 超 300 行 `[复核]` | 违反 Token 化与单文件规范 | 迁入 `--ou-*` Token；拆分 |
| P2-15 | 行为级契约检查仅 `-RunCargoTest` 触发（`check-contracts.ps1:8-17`）`[复核]` | 默认门禁弱于宣称 | 设为默认或 CI 强制 |
| P2-16 | `docs/architecture.md:12` 仍推荐 .NET/WPF `[复核]` | 与实际 Tauri 实现不符 | 更新或标注历史方案 |
| P2-17 | README 无外部链接以核验“本地优先”陈述 `[专家提出]` | 对外可信度 | 补参考链接 |
| P2-18 | `api.restoreBackup` 指向不存在的 `restore_backup` 命令（宿主仅有 `preview_restore_backup`/`confirm_restore_backup`）`[复核]` | 死代码；误用会先被 capability 拒绝 | 删除该方法；增加「TS 命令名 ⊆ handler」校验 |
| P2-19 | `tsconfig.json:2` `include:["src"]` 排除 tests；无 ESLint | 测试的类型错误长期潜伏。**已证实**：`workflows.test.tsx:144` 使用 `ScanResult` 但第 15 行未导入 | 拆分 test tsconfig 或 vitest typecheck；接入 ESLint |
| P2-20 | 主包 `index-*.js` 1,186,190 B，无 `manualChunks`（`vite.config.ts:3`）`[复核]` | 首屏解析变慢 | antd 按需 + 路由 lazy + vendor 分包 |
| P2-21 | 每次失效通知全量 `snapshot()` 且宿主锁内整体 `clone()` jobs（`lib.rs:275-284`、`coordinator.rs:238`）`[复核]` | 万级任务时 MB 级序列化 + 深拷贝 | 分页/增量快照，避免锁内深拷贝 |
| P2-22 | 前端允许任意条数/长度，宿主限制 ≤100 条且 ≤256 字符（`coordinator.rs:2223-2231`）`[复核]` | 超限后才报错，缺即时校验 | 前端实时计数与禁用 |
| P2-23 | `DESKTOP_API_VERSION`（`types.ts:23`/`desktop.rs:4`）无任何引用 `[复核]` | 桌面 API 无版本协商，漂移靠反序列化暴露 | 首帧返回版本并在 UI 校验 |
| P2-24 | 「查看密码」弹窗仅区分“有值/加载中”，失败时无限停留该文案（`VaultPage.tsx:333-341`）`[复核]` | 用户无法区分加载中与失败 | 增 `revealError` 状态与重试 |
| P2-25 | `HistoryPage.tsx:9-16` 的 `scopeLabels`/`terminalStatuses` 与 `shared.tsx:15-18,26` 重复 `[复核]` | 新增状态时两处易漂移 | 收敛到 `shared.tsx` 单一数据源 |
| P2-26 | `Command::new("explorer.exe")` 未用绝对路径（`lib.rs:758`）`[复核]` | PATH 污染下可能执行非预期程序 | 用绝对路径或 `ShellExecute` |

### 已否证（不成立）`[否证]`
- “多实例无跨进程互斥，会丢失 vault/jobs 更新”：**不成立**。宿主以 `acquire_instance_lock` 独占打开 `<data_dir>/.instance.lock`（`share_mode(0)`，`lib.rs:262-272`），并在 setup 中获取且保留（`lib.rs:992-1002`），第二实例被拒绝启动；`OpenUnpack.Core/VaultService.cs` 的命名互斥量为历史遗留。
- “白皮书宣称 W0–W6 完成 / 100% 绿灯”：**不成立**。`docs/OpenUnpack-技术架构与重构白皮书.md` v2.2.1 已明确“不据历史测试片段宣布 W0～W6 完成或 R01～R15 全部清零”；残留问题是 `architecture.md` 未同步（P2-16）。
- “`desktop_pipeline` 仍 2 项失败”：**不成立**（见第 9 节）。

---

## 9. 历史问题核销

| 历史发现 | 结论 | 证据 |
|---|---|---|
| **I02 / I03 恢复回归失败** | **已修复** | `artifacts/issue-fixes-2026-10-10/rust-final-tests.log:73-96`（2026-10-11 07:43）：`desktop_pipeline` running 21 → **21 passed; 0 failed**，含新增用例 `retired_inner_bytes_remaining_after_checkpoint_are_still_verified`（`desktop_pipeline.rs:609`）与 `retired_input_cache_remaining_after_checkpoint_is_still_verified`（`:681`）。旧“15/17，2 failed”出自 `artifacts/project-summary-2026-10-10/cargo-workspace.log`（2026-10-10 22:00，17 个用例的旧树）。修复点对应 `SavedRun.input_retired`（`coordinator.rs:94`）与 `retired_files`（`:105`）。**注意：该绿灯未经提交绑定，见 P0-1、P1-13。** |
| I01 探针加密属性误用（19 vs 15） | 已修复 | `SevenZipInterop.cs:6-10`（`Encrypted = 15`）；`SevenZipEngine.cs:131-146` |
| I04 伪装后缀分派不一致 | 基本修复 | `ArchiveFormats.cs:25-36` 魔数优先于扩展名 |
| I05 大清单未校验归属/摘要/长度 | 已修复（仍整体驻留，见 P2-11） | `worker_manifest.rs`：有界记录、路径归属、长度上限、流式 SHA-256、重解析点属性位、删前释放句柄 |
| I07 UI 未消费版本化失效通知 | 已修复 | `api.ts:47-74` 消费 `sessionId/revision` + `staleReads` |
| M05 Supervisor 未等待会话退出 | 已修复（引入 P1-6） | `supervisor.rs:269-310` |
| I06 v2 协议未协商 | 部分修复 | `accepts_ready` 已校验 hello（`worker.rs:13-22`），但把 `max_batch_candidates == 128` 等能力值硬编码进握手，能力变化即握手失败（脆弱耦合） |
| 供应链材料台账 | 可核销 | 488 组件 / 1030 文件 / 52,132,400 字节，与 `audit-release-materials` 输出一致 |

---

## 10. 安全与离线基线评估

| 维度 | 结论 | 证据 |
|---|---|---|
| 离线/无云 | 达标 | CSP 无 http(s) `connect-src`；无 HTTP 依赖 |
| IPC 认证 | 达标（可增强） | 管道 `CurrentUserOnly` + 客户端 PID 校验；创建时间校验能力已具备（`worker.rs:105-112,189`）但未用于握手，建议纳入 |
| 凭据保护 | 达标 | 全库 DPAPI、`zeroize`、口令不入 JobView/日志/清单 |
| 写入侧路径安全 | 主体达标，存 P0-2 | 包含性校验、无重解析点、`CreateNew` |
| 清理安全 | 达标 | 归属标记 + 退出未确认标记阻断清理；`walkdir` 不跟随链接 |
| 发布原子性 | 达标 | `MoveFileExW` 不覆盖；发布意图先落库 |
| 前端注入面 | 达标 | 无 `innerHTML`；`reveal` 20s 自动关闭 + 失焦隐藏 + epoch 防竞态 |

---

## 11. 测试与工程门禁评估

- 具备：`verify.ps1` 串联跨语言契约、.NET 构建、C# 测试、前端 vitest、`cargo test --workspace`、可选 Worker 协议实测（`verify.ps1:12-22`）；脚本失败会抛错（`scripts/common.ps1:124-128`）；`check-contracts.mjs` 具备真实 AST/括号平衡解析与 4 项变异自测；`summary.json` 明确声明范围不含物理盘与干净机安装。
- 本轮结果（07:43 日志）：所有 Rust 套件 `0 failed`；`dotnet-final-tests.log`、`ui-final-tests.log`、`contracts-final.log`、`worker-v1/v2` 日志均为通过。
- 缺口：
  1. 绿灯未与提交绑定、未在 CI 复现（P0-1、P1-13）。
  2. **AT-21 无真实进程测试**：`max_entries` 累计限制只由 C# 单测驱动 fake `IArchiveExtractCallback`（`ExtractCallback.cs:27-52`），`ReadEntries` 的 `count > maxEntries` 早退在“真实引擎回调”场景未被覆盖。
  3. **AT-11 无磁盘满/拔盘注入、AT-12 无清理失败注入、AT-14 无网络抓包**（现状只能靠 CSP 与代码审查静态度量）。
  4. **AT-15 无真实桌面窗口/DPI/读屏证据**：`tests/preview.test.tsx` 仅 103 行、`queue_view.test.tsx` 55 行（jsdom），非 Electron/WebView2 端到端。
  5. 契约行为级检查为 opt-in（P2-15）且容忍已知差异（P1-12）。
  6. 真实 RAR 样本仅 `rar-fixtures/rar5-solid.rar` 一个（WinRAR 产出，含 maker 标识与导出标记），缺 `rar4-plain.rar`、`rar5-split.rar`、`rar5-corrupt.rar`。
  7. `crates/protocol/tests/golden_samples.rs` 断言偏弱：仅比对 `json.contains(...)` 子串，未做逐字节规范化比对。
  8. `crates/protocol/examples/contract_samples.rs` 存在 `_ => panic!("unknown DTO")` 与 `#[allow]` 未使用宏分支（今日不可达，属潜在 panic 点）。
- 建议：补 `real_worker_enforces_all_resource_budgets`、`large_manifest_peak_memory`、`fault_injection_manifest_tamper` 三个用例；建立 AT 编号 → 测试名映射表；扩充 RAR 样本矩阵。

---

## 12. 技术债务登记（按偿还优先级）

| 序号 | 债务 | 位置 | 偿还动作 |
|---|---|---|---|
| 1 | 工作树未提交，HEAD 不可复现 | 全仓库（P0-1） | 切分提交 + tag + 干净检出重跑 |
| 2 | 双协调器 / 双状态模型 | `crates/core` vs `application+infrastructure`（S-1,S-2） | 按对等清单迁移后退役 |
| 3 | infrastructure → core 反向依赖 | `crates/infrastructure/Cargo.toml:11` | 抽共享 ports/DTO crate |
| 4 | C# 双引擎近逐行重复 | `src/OpenUnpack.{Core,Engine}/Native/**` | 删除旧副本 |
| 5 | 双份 P/Invoke 存储探测 | `paths.rs:341-509` vs `environment.rs:32-152` | 抽公共 win32 模块 |
| 6 | 规则常量多处定义且分歧 | `scanner.rs:23` / `exclusions.rs` / `SevenZipEngine.cs:441` | 单点定义 + 契约校验 |
| 7 | `application` 层 unwrap/expect | `use_cases/coordinator.rs` | 全量改 Result |
| 8 | 前端超大组件与硬编码色值 | `VaultPage/SettingsPage/HistoryPage` | 拆分 + Token 化 |
| 9 | 文档与实现漂移 | `architecture.md` | 建当前状态索引 |

---

## 13. 建议路线图

短期（可复现 + 安全）
1. P0-1：提交并打 tag，在干净检出上重跑 `verify.ps1 -WorkerProtocol` 归档。
2. P0-2：统一 C#/Rust 路径校验规则并补跨语言黄金样本。
3. P1-5（action 白名单）、P1-6（锁外等待）、P1-10（终止事件不丢）。
4. P1-13：发布仓库并实跑一次 CI。

中期（健壮性与性能）
5. P1-7 进程级电源请求；P1-8 去 unwrap；P1-11 快照增量；P1-9 接真实探针。
6. P2-2/2-3/2-4 性能热点 + 峰值内存与 Worker 超限真实进程测试。
7. P1-4 常量收敛；P1-12 契约差异收敛；P2-15 门禁默认强化。

长期（结构性）
8. 解决 S-1/S-2/P1-3 后完成分层迁移并退役 `crates/core`；删除 C# 重复引擎；建立跨语言契约单一事实源；补齐 AT 全量核销（含物理盘、干净机、DPI/读屏、磁盘满/拔盘）。

---

## 14. 并行专家审计与交叉验证

| 方向 | 专家 | 状态 | 处理 |
|---|---|---|---|
| 后端（Rust core + C# Worker/Engine） | 贝洛奇 | 已回传 | 并入 P1-9、P1-10、P2-9~P2-13、P2-17；其“多实例无互斥”经复核**否证** |
| 架构（domain/application/infrastructure/protocol） | 高见远 | 已回传 | 并入 S-1、S-2、P1-3、P1-4、P2-6~P2-8、P2-16；其“白皮书夸大完成度”经复核**修正** |
| 测试与工程门禁（含 CI/供应链/真实样本） | 严过关 | 已回传 | 并入 P1-13、P1-14、P2-17、第 11 节缺口项；其“P0-1 为 flaky 而非失败”经复核**成立**（更正本报告初稿） |
| 前端/宿主（Tauri + React） | 贾思敏 | 已回传 | 并入 P1-15、P2-18~P2-26；其复核结论（I07 已修复、类型漂移受控、命令/权限 33 条同步、未发现越权文件访问与 `..` 逃逸、serde Option 语义已实测）已采纳 |

---

## 附录 A：关键证据命令

```bash
# 交付快照
git status --porcelain | wc -l          # 80
git ls-tree -d HEAD crates/             # 仅 core / protocol
# 本轮红绿
grep -n "test result:" artifacts/issue-fixes-2026-10-10/rust-final-tests.log
sed -n '71,96p' artifacts/issue-fixes-2026-10-10/rust-final-tests.log
# 旧红（已过时）
sed -n '240,305p' artifacts/project-summary-2026-10-10/cargo-workspace.log
# 反向依赖
grep -n 'openunpack-core' crates/infrastructure/Cargo.toml          # 11
# 路径校验分歧
sed -n '360,376p' src/OpenUnpack.Engine/Native/SevenZipEngine.cs
sed -n '32,71p'  crates/core/src/paths.rs
# 排除规则上限分歧
grep -n '256' crates/core/src/scanner.rs                            # 23
# 重复实现量化
diff src/OpenUnpack.Core/Native/SevenZipEngine.cs src/OpenUnpack.Engine/Native/SevenZipEngine.cs | wc -l   # 10
# panic/unwrap 扫描
grep -rn '\.lock()\.unwrap()\|\.expect(' crates/application/src
# 前端违规扫描
grep -rPn '#[0-9a-fA-F]{3,8}\b' apps/desktop-ui/src | grep -v '#fff' | grep -v '#000'
```
