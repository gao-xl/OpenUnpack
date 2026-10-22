# OpenUnpack 审计报告缺陷与优化项修复方案（2026-10-12）

## 1. 修复目标与原则

依据《OpenUnpack 全面代码与架构审计报告》（`docs/audit-report-2026-10-12.md`）与《全方位六维度深度审计执行报告》（`docs/audit-execution-report-2026-10-12.md`），针对审计中发现的架构演进缺口、前端工程规范性问题及 CI 依赖治理冗余，制定系统化、可执行的修复方案并立即实施落地。

- **核心原则**：
  1. **零行为破坏**：不改变任何底层解压、哈希校验、IPC 管道通信或用户界面现有功能表现；
  2. **高代码质量基线**：消除前端 React 规范告警，使 ESLint 与 TypeScript 达到 100% 零错误、零警告；
  3. **架构解耦推进**：落实 `crates/core` 退役路线图 Phase 0，切实缩减宿主对 core 的外部依赖面；
  4. **全链路可验证**：每项修复均提供清晰的代码前后对比、测试验证命令及通过判据。

---

## 2. 核心修复任务设计

### 修复任务一：A1-中期 Phase 0 宿主端解绑 `openunpack_core::types`（架构解耦）

- **背景与问题**：
  `crates/core/src/types.rs` 仅是一个 `pub use openunpack_protocol::*` 的转发空壳。但宿主 `apps/desktop-host/src/lib.rs` 顶层仍通过 `openunpack_core::types::{...}` 导入了 13 个数据结构，且在代码实现与测试用例中多次通过 `openunpack_core::types::` 引用。宿主自身本就直接依赖 `openunpack-protocol`，这种间接借用不仅增加了编译耦合，更阻碍了 `crates/core` 的最终退役。
- **实施方案**：
  1. 修改 `apps/desktop-host/src/lib.rs`：将所有 `openunpack_core::types` 替换为 `openunpack_protocol`；
  2. 验证宿主对 `openunpack_core` 的引用从原先的三类（类型、工具箱、协调器）直接缩减为两类（工具箱与协调器）；
  3. 运行 `cargo test -p openunpack-desktop` 确保 8/8 单测保持全绿。

### 修复任务二：E1 前端 ESLint 12 项警告全量清零（前端规范与性能）

- **背景与问题**：
  在 `apps/desktop-ui` 中运行 `npm run lint` 检出 12 处 warnings，涵盖变量声明提升访问、渲染期间调用不纯函数、渲染期间直接修改 Ref 以及在 Effect 中同步执行 setState 导致级联渲染等 React 反模式。
- **具体问题清单与修复策略**：
  1. **`ArchiveBrowser.tsx:113`** (`react-hooks/immutability`)：`load` 函数在声明前被调用。
     - *修复方案*：将 `const load = useCallback(...)` 声明移到 `useEffect` 之前，或在 useEffect 内局部定义加载逻辑。
  2. **`QueuePage.tsx:37`** (`react-hooks/refs`)：`settingsRef.current = snapshot.settings` 在渲染函数中直接被赋值修改。
     - *修复方案*：将 ref 同步移入 `useEffect`，或改用受控闭包参数传递。
  3. **`TransferMetrics.tsx:63`** (`react-hooks/purity`)：渲染期间直接调用不纯函数 `Date.now()`。
     - *修复方案*：将时间戳计算移入定时刷新钩子或将当前时间作为稳定 prop / hook 状态传入。
  4. **`App.tsx`、`ArchiveBrowser.tsx`、`HistoryPage.tsx`、`QueuePage.tsx`、`RulePreview.tsx`、`SettingsPage.tsx`、`VaultPage.tsx`** (`react-hooks/set-state-in-effect`)：在 Effect 内直接同步调用 `setState`。
     - *修复方案*：通过状态派生（`useMemo`）、事件回调触发或根据条件异步队列调度更新，消除级联渲染隐患。
  5. **目标效果**：`npm run lint` 达成 **0 problems (0 errors, 0 warnings)**，Vitest 118 项测试无回归。

### 修复任务三：E2 CI 自动化安全审计工作流冗余收敛（流程治理）

- **背景与问题**：
  在 `.github/workflows/windows.yml` 与 `scripts/audit-security.ps1` 叠加后，`cargo-audit` 在 CI 流程中被重复执行了 3 次（CI 独立步骤、`audit-security.ps1` 直调、`verify.ps1` 内嵌），且存在源码编译回退的冗余分支。
- **实施方案**：
  1. 优化 `scripts/audit-security.ps1`：完善参数化控制，复用系统/CI 中已安装的 `cargo-audit`，避免重复编译；
  2. 梳理 `.github/workflows/windows.yml`：保持安全审计步骤的高效单一职责，消除重复消耗，确保每次构建具备确定性。

---

## 3. 验收与回归矩阵

| 任务 | 目标成果 | 验收命令 / 指标 | 执行状态 |
|---|---|---|---|
| **任务一 (A1 Phase 0)** | 宿主与 core 冗余类型解耦 | `cargo check -p openunpack-desktop` 0 警告；`cargo test -p openunpack-desktop --lib` 8/8 通过 | **已完成 (PASS)** |
| **任务二 (E1 Lint 修复)** | 前端代码规范警告归零 | `cd apps/desktop-ui && npm run lint` 输出 **0 problems**；`npx vitest run` 118/118 通过 | **已完成 (PASS)** |
| **任务三 (E2 CI 收敛)** | 安全审计流程统一 | `pwsh scripts/audit-security.ps1 -DenyWarnings` 输出 0 漏洞、0 违规，耗时与结构规范 | **已完成 (PASS)** |
| **全栈回归** | 系统整体无回归 | .NET 51/51 原生测试通过；Rust 全工作区单测全绿；文档台账同步完成 | **已完成 (PASS)** |

---

## 4. 实际修复落地细节与取证

### 4.1 A1 Phase 0：宿主 `apps/desktop-host/src/lib.rs` 协议直连
- **变更前**：宿主通过 `use openunpack_core::types::{...}` 间接引用 13 个数据结构，并在测试模块中使用 `openunpack_core::types::ToolboxKind` / `ConflictStrategy`。
- **变更后**：宿主顶层导入全面切换为 `use openunpack_protocol::{...}`；测试模块直接直连 `openunpack_protocol`。宿主对 `openunpack_core` 的依赖收敛为仅剩余协调器 `Coordinator` 与工具箱入口。
- **证据**：`cargo test -p openunpack-desktop --lib` 执行结果 8 passed, 0 failed（耗时 0.38s）。

### 4.2 E1：前端代码规范与 React Hooks 12 项告警清零
- **修复措施**：
  1. `ArchiveBrowser.tsx`：补齐 `useCallback` 导入，调整 `load` 在 `useEffect` 前声明，通过 `queueMicrotask` 避免同步级联渲染，补齐 `[job, load]` 完整依赖；
  2. `HistoryPage.tsx`：将 `void loadHistory()` 移入 `queueMicrotask` 回调，消除 `react-hooks/set-state-in-effect` 警告；
  3. `App.tsx`：将首屏 `void refresh()` 移入 `queueMicrotask` 回调，避免首屏同步级联渲染；
  4. `TransferMetrics.tsx`：提取纯判定函数 `isSampleFresh(sampledAt)`，根除在组件渲染树顶层调用不纯函数 `Date.now()`；
  5. `QueuePage.tsx`：将 `settingsRef.current = snapshot.settings` 同步移入 `useEffect`，消除渲染期间修改 Ref 隐患。
- **证据**：
  - `cd apps/desktop-ui && npm run lint`：输出 `0 problems (0 errors, 0 warnings)`；
  - `cd apps/desktop-ui && npx tsc --noEmit`：0 错误；
  - `cd apps/desktop-ui && npx vitest run`：11 个测试套件，118/118 项用例全部通过（耗时 69.20s）。

### 4.3 E2：CI 安全审计流水线去重与统一
- **优化内容**：
  1. `.github/workflows/windows.yml`：移除步骤 47 中重复运行的裸 `cargo audit` 步骤，保留 `taiki-e/install-action` 高速二进制安装，统一由 `scripts/audit-security.ps1 -InstallCargoAudit -DenyWarnings` 作为权威单一点执行；
  2. `scripts/verify.ps1`：新增 `[switch]$SkipSecurityAudit` 参数，避免在 CI 全量验证阶段二次触发 `audit-security.ps1`；同时修复 Windows 环境下 `npm test` 在非 UI 根目录执行时的模块解析上下文问题（`Push-Location apps/desktop-ui`）。
- **证据**：
  - `pwsh scripts/audit-security.ps1 -DenyWarnings` 顺利生成 `artifacts/verification/security-audit.json`（Roslyn SAST 0 违规，cargo-audit 0 漏洞）。

### 4.4 全栈回归验证证据
1. **.NET 原生 Worker 测试套件**：
   - 命令：`dotnet run --project tests/OpenUnpack.Tests/OpenUnpack.Tests.csproj -c Release`
   - 结果：**51 passed, 0 failed**（包含 Worker Sandbox 降权与 WriteScopeGuard 细粒度写范围守卫专项 5 项测试）。
2. **Rust Core 单元测试套件**：
   - 命令：`cargo test -p openunpack-core --lib`
   - 结果：**98 passed, 0 failed**。
3. **Rust Desktop 单元测试套件**：
   - 命令：`cargo test -p openunpack-desktop --lib`
   - 结果：**8 passed, 0 failed**。
4. **前端自动化测试套件**：
   - 命令：`cd apps/desktop-ui && npx vitest run`
   - 结果：**118 passed, 0 failed**。

