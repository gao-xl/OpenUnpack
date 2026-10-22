# OpenUnpack 全方位深度审计计划（2026-10-12 完整版）

## 1. 审计目标与方法论

本计划针对 **OpenUnpack**（Windows x64 高性能批量解压缩与目录整理桌面系统，采用 **Rust + TypeScript + C# (.NET 10)** 三栈混合架构）制定全方位、可量化、证据驱动的审计方案。

- **核心原则**：**「每一项审计策略必须绑定自动化验证命令或精确代码取证，禁止主观臆断」**。
- **覆盖面**：涵盖 **系统架构与双轨一致性（D1）**、**跨栈协议与序列化契约（D2）**、**深度安全与沙箱纵深防御（D3）**、**并发控制与资源生命周期（D4）**、**前端工程与交互韧性（D5）**、**构建发布与许可证溯源（D6）** 六大维度，共 **24 项具体审计策略（S-01 ~ S-24）**。
- **闭环要求**：严格按序执行全部 24 项策略；若执行中发现任何偏差或隐患，当场修复并重新运行门禁验证，最终产出《OpenUnpack 全方位深度审计执行报告》。

---

## 2. 审计对象与物理边界

| 子系统 | 路径范围 | 核心职责 |
|---|---|---|
| **Rust 协议与领域层** | `crates/protocol`、`crates/domain`、`crates/application` | 跨进程/跨语言强类型 DTO、错误码体系、领域规则与用例编排 |
| **Rust 基础设施层** | `crates/infrastructure` | SQLite 持久化、租约锁、DPAPI 凭据保险箱、Worker 命名管道客户端 |
| **Rust 核心协调器** | `crates/core` | 批处理状态机、物理盘拓扑路由、扫描器、编码修复、文件整理工具箱 |
| **Tauri 桌面宿主** | `apps/desktop-host` | 原生窗口、文件/目录选择器、ACL 权限清单、受控 `explorer.exe` 调用 |
| **React 前端工程** | `apps/desktop-ui` | React 19 + TypeScript + Ant Design 界面、增量快照游标、虚拟目录树 |
| **C# 解压引擎与沙箱** | `src/OpenUnpack.Engine` | 7-Zip COM 互操作、LZ4/Zstd 解码、`WorkerSandbox` 令牌降权、`WriteScopeGuard` |
| **C# 独立 Worker** | `src/OpenUnpack.Worker` | 双向认证命名管道服务端、任务分发、进程优先级控制与溢出清单回写 |
| **工程化与合规资产** | `scripts/`、`tools/7zip/`、`.github/workflows/` | 契约自检、Roslyn SAST / cargo-audit 安全门禁、7-Zip 哈希溯源、NSIS 打包 |

---

## 3. 六大维度 × 24 项审计策略矩阵

### 维度一：系统架构、分层依赖与双轨镜像一致性（D1）

| 策略编号 | 审计策略名称 | 审计目标与取证方法 | 通过门禁标准（Acceptance Gate） |
|---|---|---|---|
| **S-01** | Workspace Crate 依赖方向与无反向借用审计 | 检查 `Cargo.toml` 及各子 crate 依赖声明，验证 `protocol ← domain ← application ← infrastructure` 单向依赖关系 | `infrastructure` 0 处引用 `openunpack-core`；`cargo check --workspace` 0 警告 0 错误 |
| **S-02** | 双轨镜像代码一致性与双写横幅审计 | 逐一比对 4 组双轨文件（`vault.rs`、`worker.rs/client.rs`、`database_lease.rs`、`worker_manifest.rs/manifest.rs`）的 `MIRROR WARNING` 横幅与核心实现（含 `JobGuard` UI 隔离） | 4 组文件均含双写横幅；除模块路径差异外核心逻辑 100% 同步 |
| **S-03** | Rust 生产代码零 `unwrap/todo/unimplemented` 审计 | 扫描 `crates/**/src/**/*.rs` 与 `apps/desktop-host/src/**/*.rs` 生产代码段（排除 `#[cfg(test)]`） | 生产路径 `unwrap()` = 0、`todo!()` = 0、`unimplemented!()` = 0 |
| **S-04** | C# 与 TS 生产代码静态规范与类型逃逸审计 | 扫描 `src/**/*.cs` 与 `apps/desktop-ui/src/**/*.{ts,tsx}` 的阻塞调用与类型逃逸 | C# 0 处 `async void` / `.Result` / `.Wait()`；TS 0 处 `any` / `@ts-ignore` |

### 维度二：跨技术栈协议契约与序列化对齐（D2）

| 策略编号 | 审计策略名称 | 审计目标与取证方法 | 通过门禁标准（Acceptance Gate） |
|---|---|---|---|
| **S-05** | Rust ↔ TS ↔ C# 225 字段强类型契约双向对齐与变异自测 | 运行 `node scripts/check-contracts.mjs --self-test`，检查 22 个 Desktop DTO 与 7 个 Worker DTO 的字段名、可空性与枚举定义 | 225 个字段 0 差异；5 项变异注入自测试 100% 检出 |
| **S-06** | Protocol 黄金样本与 V1/V2 握手协议审计 | 运行 `cargo test -p openunpack-protocol`，审查 V1/V2 序列化兼容性与 `WorkerHello` 能力协商 | 7/7 协议测试全绿通过 |
| **S-07** | 含密协议 DTO 脱敏 Debug 与 `Zeroize on Drop` 审计 | 审查 `CandidateItem`、`PasswordInput`、`WorkerRequest`、`WorkerEvent` 的 `Debug` 实现与内存擦除逻辑 | 所有密钥字段 `Debug` 输出为 `[REDACTED]`；`Drop` 自动调用 `zeroize()` |

### 维度三：深度安全性与 Worker 沙箱纵深防御（D3）

| 策略编号 | 审计策略名称 | 审计目标与取证方法 | 通过门禁标准（Acceptance Gate） |
|---|---|---|---|
| **S-08** | Windows Restricted Token 进程特权永久剥离审计 | 审查 `WorkerSandbox.StripTokenPrivileges()` 与 `Program.cs` 初始化顺序，运行沙箱令牌剥离测试 | 除 `SeChangeNotifyPrivilege` 外全部特权永久移除（`GetTokenPrivilegeCount() <= 1`） |
| **S-09** | 进程安全缓解策略与安全 DLL 加载（SEC-04）审计 | 检查 `SetDllDirectory("")`、`DisableExtensionPoints`、`NoRemoteImages`、全量 `[DllImport]` 的 `DefaultDllImportSearchPaths` 及 `7z.dll` 重解析点检查 | 100% P/Invoke 标注 `DllImportSearchPath.System32`；CA5392 0 告警；`7z.dll` 校验 SHA256 与无重解析点 |
| **S-10** | `WriteScopeGuard` 细粒度写范围守卫全写点覆盖审计 | 审查 `SevenZipEngine.cs`、`Lz4Engine.cs`、`WorkerOperations.cs` 全部磁盘写操作，验证源包保护、系统目录黑名单与只读模式拦截 | 全部 7 处写点前置调用 `DemandWriteAccess`；越界写入、覆写源包、只读模式写文件 100% 抛出 `UnsafePath` |
| **S-11** | 宿主 `explorer.exe` 启动参数消毒与保留设备名阻断（SEC-03）审计 | 审查 `apps/desktop-host/src/lib.rs` 中 `strip_verbatim_prefix` 与 `sanitize_explorer_target` 并运行单测 | 剥离 `\\?\` 前缀、拦截 `-`/`/` 开关走私与 `CON/PRN/AUX/NUL/COM/LPT` 设备名；8/8 宿主单测通过 |
| **S-12** | Zip-Slip、重解析点穿越与解压炸弹防御（SEC-01/02）审计 | 审查 7-Zip 与 LZ4 引擎的相对路径校验、`EnsureNoReparsePoint` 祖先链检查及 `maxAllowedBytes` 流式熔断 | 路径穿越、ADS 备用流、软链接/Junction、超限解压炸弹 100% 阻断 |
| **S-13** | 全栈依赖漏洞与 Roslyn SAST 静态安全分析审计 | 运行 `pwsh scripts/audit-security.ps1` 与 `npm audit --omit=dev` | `cargo-audit` 0 漏洞、Roslyn SAST 0 违规、`npm audit` 0 漏洞 |

### 维度四：并发控制、资源上限与进程生命周期韧性（D4）

| 策略编号 | 审计策略名称 | 审计目标与取证方法 | 通过门禁标准（Acceptance Gate） |
|---|---|---|---|
| **S-14** | Rust 宿主 `JobGuard` 进程与 UI 隔离沙箱审计 | 审查 `worker.rs` 与 `client.rs` 中 `JobGuard::attach_raw` 的扩展限额与 UI 限制标志 | 同时启用 `KILL_ON_JOB_CLOSE | DIE_ON_UNHANDLED_EXCEPTION` 及 8 项 `JOB_OBJECT_UILIMIT_*` |
| **S-15** | 双命名管道双向 PID 认证与退出确认（`confirm_exit`）审计 | 审查管道服务端/客户端 PID 互验、首实例独占标志及 `UNCONFIRMED` 毒化锁恢复 | 管道 PID 不匹配立即拒绝；毒化锁通过 `PoisonError::into_inner` 恢复不丢记录 |
| **S-16** | 物理盘拓扑感知调度与 SSD 缓存配额预留审计 | 审查 `route.rs` 介质检测与 `coordinator.rs` 的 HDD 排他调度、`BudgetClaim` 在飞预留，运行回归测试 | `resource_recovery_regressions` 4/4 通过；缓存峰值严格受控于配额上限 |
| **S-17** | 超大归档清单（>8MiB）流式溢出与 SHA-256 原子消费审计 | 审查 `CreateCompletedEventAsync` 与 `worker_manifest::consume`，运行 30,000 条目大清单测试 | >8MiB 清单零内存暴涨流式落盘、SHA-256 校验通过后原子清理，>128MiB 触发配额熔断 |
| **S-18** | CRC 损坏抢救三态生命周期与源文件删除硬熔断（F5）审计 | 审查 `salvaged` 状态流转与 `source_cleanup_allowed` 守卫条件 | 存在任何 CRC 损坏条目时 `source_cleanup_allowed = false`，源压缩包 100% 保全 |

### 维度五：前端工程质量、交互韧性与可访问性（D5）

| 策略编号 | 审计策略名称 | 审计目标与取证方法 | 通过门禁标准（Acceptance Gate） |
|---|---|---|---|
| **S-19** | 前端 TypeScript 严格类型检查与 ESLint 静态质量审计 | 在 `apps/desktop-ui` 执行 `npx tsc --noEmit` 与 `npm run lint` | `tsc` 0 错误；ESLint 0 Error |
| **S-20** | 前端异步竞态防护、受控折叠与即时密码交互审计 | 审查 `ArchiveBrowser`、`HistoryPage`、`VaultPage`、`QueuePage` 的序号门与代次绑定 | 慢请求乱序返回不覆盖新状态；草稿代次变更阻止陈旧导入；加密任务支持单点直接输密 |
| **S-21** | 前端全套组件与工作流自动化测试审计 | 在 `apps/desktop-ui` 执行 `npm test`（Vitest 全量测试集） | 11 个测试文件、118 项测试用例 100% 通过 |

### 维度六：构建发布、许可证溯源与全栈端到端回归（D6）

| 策略编号 | 审计策略名称 | 审计目标与取证方法 | 通过门禁标准（Acceptance Gate） |
|---|---|---|---|
| **S-22** | 官方 7-Zip 引擎与许可证 SHA-256 锁定溯源审计 | 核验 `tools/7zip/provenance.json` 与 `7z.dll`、`7z.exe`、`License.txt` 实机哈希及 `THIRD_PARTY_NOTICES.md` | 三项核心二进制/许可证文件 SHA-256 与 provenance.json 100% 一致 |
| **S-23** | 源码包白名单与仓库卫生审计 | 检查仓库是否存在未清理的废弃工程引用（`OpenUnpack.Core`）、构建残留或敏感配置 | 核心文档与构建脚本 0 处活动引用废弃目录；`.gitignore` 与打包脚本严密过滤临时产物 |
| **S-24** | .NET 51 项原生引擎测试与 Rust 全栈测试回归审计 | 运行 `dotnet run --project tests/OpenUnpack.Tests -c Release` 及 Rust 各层测试集 | .NET 51/51 全绿通过；Rust `protocol`/`domain`/`application`/`infrastructure`/`core`/`desktop` 全绿通过 |
