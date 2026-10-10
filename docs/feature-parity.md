# OpenUnpack 功能对等与生产依赖核销清单 (Feature Parity Matrix)

版本：1.0 · 2026-10-10 · 阶段：W0 真实基线与依赖分离规划

依据 `docs/refactoring-plan-final.md` 第 11 节要求建立。本清单追踪 C# 历史代码向生产模块 `OpenUnpack.Engine` 分离以及对应 Rust 核心实现的对等与核销状态。

---

## 1. 核心能力对等与处置矩阵

| 能力项 | 原位置 (OpenUnpack.Core) | 当前调用方 | 目标定位 (重构去向) | 对应 Rust 核心实现 | 状态与处置门槛 |
|---|---|---|---|---|---|
| SevenZipEngine & SevenZipInterop | `Native/SevenZipEngine.cs`, `Native/SevenZipInterop.cs` | WorkerOperations, 格式测试 | `src/OpenUnpack.Engine/Native/` | Worker 生产依赖保留；不改原生 ABI | 迁移至独立 Engine 工程，Worker 生产直接依赖，无旧 Core 引用 |
| 引擎异常与 DTO | `Contracts.cs` (ArchiveException, ExtractEntry, etc.) | Worker, Engine | `src/OpenUnpack.Engine/EngineContracts.cs` | `crates/protocol::worker` | 分离纯引擎契约，彻底剥离 Vault 和 BatchRunner 杂项 |
| 分卷解析 (ResolveSource) | `ArchiveScanner.cs` 中的分卷解析逻辑 | SevenZipEngine 打开分卷 | `src/OpenUnpack.Engine/VolumeResolver.cs` | `crates/domain::archive::grouping` / `crates/core::scanner` | 提取最小分卷开卷解析器，与上层全量归组扫描解耦 |
| BatchRunner | `BatchRunner.cs` | 仅历史测试 / WPF 原型 | 参考实现，不接入主产品 | `crates/application::runtime::coordinator` | 保留供对比验证，主产品管线以 Rust Application Coordinator 为准，最终退役 |
| C# VaultService / 导入导出 | `VaultService.cs`, `PasswordImport.cs` | 历史测试 / WPF 原型 | 参考实现，不接入主产品 | `crates/infrastructure::vault` | Rust 具备完整 DPAPI 存储与导入备份能力，对等核销后退役 |
| 稀疏指纹提取 | `Native/SevenZipEngine.cs` / `Contracts.cs` | 候选排序提示 | 仅作候选提示，独立核对 | `crates/domain::passwords` | 不作为内容一致/源未变判定依据；依需求核对接入 |
| 文件名密码线索 | `PasswordImport.cs` | 历史测试 | 规则提取 | `crates/domain::passwords::candidate_policy` | 候选排序规则，默认按安全基线只使用已授权候选 |
| 路径与工作区管理 | `CacheWorkspace.cs`, `CacheRouteDecider.cs` | 历史测试 / WPF | 纯规则提取至 Domain，IO 归 Infra | `crates/domain::storage`, `crates/infrastructure::filesystem` | 纯规则双向比对，工作区生命周期受 WorkerSupervisor 严控 |
| 防休眠 (PowerPolicy) | `Native/` SetThreadExecutionState | 宿主 / Worker | Windows 基础设施端口 | `crates/infrastructure::windows::power` | 生命周期受租约管理，取消与异常退出可靠释放 |
| WPF Desktop 入口 | `src/OpenUnpack.Desktop/` | 早期原型工程 | 历史归档 | `apps/desktop-host` + `apps/desktop-ui` | 待 Solution、脚本、测试清理后按 W6 退役 |

---

## 2. Worker 生产依赖隔离要求

1. `OpenUnpack.Worker.csproj` 移除 `<ProjectReference Include="..\OpenUnpack.Core\OpenUnpack.Core.csproj" />`。
2. `OpenUnpack.Worker.csproj` 增加 `<ProjectReference Include="..\OpenUnpack.Engine\OpenUnpack.Engine.csproj" />`。
3. `OpenUnpack.Engine` 仅包含：
   - `Native/SevenZipEngine.cs`
   - `Native/SevenZipInterop.cs`
   - `EngineContracts.cs`
   - `VolumeResolver.cs`
4. `OpenUnpack.Engine` 绝不引用任何密码库、WPF 或批处理调度逻辑。

---

## 3. 验收与退役门槛

- [ ] W0: 建立 feature-parity 清单并锁定基线。
- [ ] W2: 创建 `OpenUnpack.Engine` 并重构 `OpenUnpack.Worker` 引用，编译通过。
- [ ] W4: Worker v2 协议与 EngineContracts 对齐。
- [ ] W6: 彻底从 Solution 与构建脚本中解耦旧 `OpenUnpack.Core` / `OpenUnpack.Desktop`。
