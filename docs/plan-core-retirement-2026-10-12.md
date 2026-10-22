# OpenUnpack `crates/core` 退役计划（A1-中期）

> 2026-10-20：本文保留历史设计/迁移背景；当前源码归属及运行链以 [现行架构](architecture-current.md) 为准。Core 已明确为唯一生产编排层，未装配的 TaskCoordinator / SQLite v2 原型退役，底层镜像收敛为 Infrastructure 单源实现。旧的 Core 整体退役及镜像双写要求不再作为当前执行指引。

**状态**：计划文档（未执行任何切换） · **日期**：2026-10-12 · **编号口径**：沿用 `docs/audit-report-2026-10-12.md`（A1-中期），与台账 §8（comprehensive-review R1–R8）及 §9–§12（F1–F6 功能）编号体系无关。

**依据**：`docs/audit-report-2026-10-12.md` A1 建议 3 / A3 前注；`docs/status-ledger-2026-10-11.md` §1#1（执行链"另列"）、§1#6（infra 已自包含）、§10 P4（P5 边界：行为对等前 core 保持完整运行）。

**2026-10-15 安全边界更新**：下文行数、引用数量及旧测试计数是 2026-10-12 基线，不作为当前迁移验收结果。[S06 修复报告](s06-directory-identity-report-2026-10-15.md)新增了 Core 的父链句柄、同对象发布/清理、私有目录权限验证及回收回执。生产仍装配 Core；Infrastructure 清单的解析主体与纯相对路径校验分别受镜像门禁约束，其文件打开辅助仍没有 Core 完整的父链保护。本轮只对齐纯校验和加强门禁，未切换生产入口。旧计划中“toolbox 仅依赖 3 个路径符号、可直接独立迁移”和“manifest 已就绪”的结论由下面的安全前置条件替代。

---

## 1. 目标与范围

**目标**：在行为对等前提下，将 `crates/core` 的全部生产模块（包含 S06 新增的安全 IO 与回收模块）迁入新分层（protocol / domain / application / infrastructure），随后物理删除该 crate，终结"双轨镜像 + 双写"维护模式（见 A1 条目的 MIRROR 横幅约定）。

**不在范围（已定决策或另行排期）**：
- 不搬移现有数据目录 `%LOCALAPPDATA%\OpenUnpack`（既定决策）；
- 不改变任何对外行为：DPAPI 文件格式、SQLite schema 与迁移、租约锁语义、Worker 管道线协议在迁移前后必须逐字节兼容；
- A3（Coordinator 子模块拆分）：按审计建议在双轨收敛**后**另行考虑，本计划不包含。

**当前口径**（§10 P4）：`crates/core` 作为生产 Coordinator 运行时保持完整运行，待行为对等后单独退役。本计划即该退役动作的施工图。

## 2. 当前依赖面（2026-10-12 实测）

### 2.1 宿主对 core 的全部使用（唯一外部使用方）

| 依赖项 | 位置与数量 | 迁移难度 | 承接者 |
|---|---|---|---|
| `coordinator::Coordinator` | 10 处引用：生产构造 `Coordinator::open`（`lib.rs:1499`）、状态字段（`:79`）、适配器 `CoordinatorVaultStore`（`:90-93`，已实现 `application::VaultStore` 端口）等 | **高** | `application::TaskCoordinator` 存在但执行链未对等（§1#1 另列） |
| `types::` 再导出 | 顶层 13 类型导入（`lib.rs:2-11`）+ `SuccessEvent` 生产 1 处（`:149`）+ 测试构造 4 处 | **极低**（纯路径） | `crates/core/src/types.rs` 即 `pub use openunpack_protocol::*`；宿主已直接依赖 `openunpack-protocol` |
| `toolbox::{preview_toolbox, execute_toolbox}` | 生产各 1 处（`lib.rs:1366`、`:1391`，经 `authorize_toolbox_options` 授权后调用） | **中** | **无** —— 新分层未覆盖，是本计划唯一的独立功能缺口 |

`CoordinatorVaultStore` 适配器是现成的迁移接缝：宿主已能把生产协调器的密码库暴露给 application 端口，Phase 2 切换后该适配器随 core 一并删除。

### 2.2 core 模块清单与承继者状态（共 ≈11,600 行）

| 模块 | 行数 | 性质 | 新分层承继者 | 退役依赖 |
|---|---|---|---|---|
| `coordinator.rs` | 4097 | 独立 | `application::use_cases::coordinator.rs`（1256 行，未对等） | Phase 2 门槛 |
| `vault.rs` | 2167 | 镜像 → `infra dpapi/vault.rs` | 已就绪（infra 自包含，§13 P0） | 随 coordinator |
| `scanner.rs` | 1204 | 独立 | 无 | 随 coordinator |
| `paths.rs` | 767（历史） | 独立 | 纯校验部分内联于 infra；S06 后 toolbox 还依赖 `DirectoryGuard`、`LockedEntry`，完整安全 IO 尚未迁移 | 先对等句柄与权限保护，再随使用方迁移 |
| `toolbox.rs` | 766（历史） | 独立 | **无**；依赖 S06 同对象移动、清理和预览文件身份 | **Phase 1 安全前置条件满足后迁移** |
| `worker.rs` | 533 | 镜像 → `infra worker/client.rs` | 已就绪 | 随 coordinator |
| `repository.rs` | 465 | 独立 | `infra sqlite/task_store.rs` + `sqlite/database_lease.rs` | 随 coordinator |
| `route.rs` | 397 | 独立 | 无 | 随 coordinator |
| `worker_manifest.rs` | 325（历史） | 解析主体镜像 → `infra worker/manifest.rs` | 主体及纯路径校验有门禁；Infra 文件打开辅助未具备 Core 父链保护 | 先补 IO 对等，再随 coordinator |
| `zip_encoding.rs` | 319 | 独立 | 无 | 随 coordinator |
| `transfer_progress.rs` | 302 | 独立 | 无（transfers 投影） | 随 coordinator |
| `database_lease.rs` | 288 | 镜像 → `infra sqlite/database_lease.rs` | 已就绪（本轮补齐测试 5/5） | 随 repository |
| `types.rs` | 9 | 再导出层 | `openunpack_protocol` 直连 | **Phase 0 可先删** |

关键事实：宿主**不直接引用** scanner / route / zip_encoding / repository / paths —— 它们只被 `Coordinator` 内部使用，因此随 Phase 2 一次性退役，无需单独排期。

## 3. 迁移顺序（四阶段，每阶段独立提交可回滚）

### Phase 0 — 机械切换（零行为风险，可立即执行）

1. 宿主 16 处 `openunpack_core::types::X` 改为 `openunpack_protocol::X`（顶层导入块、`SuccessEvent`、测试构造）。
2. 验证：`cargo check -p openunpack-desktop --all-targets` + 宿主单测。
3. 效果：core 依赖面从三类缩减为两类（Coordinator + toolbox）；`types.rs` 可随 Phase 3 删除。

### Phase 1 — toolbox 归位（先完成安全 IO 对等）

1. 新家：`crates/infrastructure/src/toolbox/mod.rs`（IO 归属清晰：walkdir/regex/windows-sys 元数据均属基础设施层；application 层当前为纯编排、无 FS 依赖，不破坏其性质）。导出同签名 `preview_toolbox` / `execute_toolbox` / `validate_toolbox_directory`。
2. 安全前置条件：当前 `toolbox.rs` 还依赖 `DirectoryGuard` 与 `LockedEntry`。须先明确安全 IO 的共享归属，并验证父链固定、相对句柄操作、源对象身份、跨卷复制及同对象删除、大小写改名回退、预览令牌文件身份和目录清理全部对等。不能只复制三个纯校验函数，也不能让 Infrastructure 反向依赖 Core。纯路径校验继续受镜像门禁保护；迁移 NT IO 应单独设计与评审，本轮不复制该层。
3. 测试随迁：core toolbox 单测及 S06 对象替换、目录联接、大小写、跨卷哨兵与清理回归全量平移；保留宿主授权与前端 `ToolboxPage` 用例。先在现有生产实现下保存基线，再对照新实现运行，任何安全边界未核销都保留 Core 调用。
4. 宿主切换 2 处调用点 → 删除 `crates/core/src/toolbox.rs`。
5. 验证 gate：toolbox 单测 + 宿主授权测试 + `tsc`/vitest 不回归 + `cargo check --workspace`。

### Phase 2 — Coordinator 对等与宿主切换（主工程）

1. **门槛清单**（§1#1 原文）：调度、Worker 事件、嵌套归档、发布恢复、成功事件循环全部接入 `application::TaskCoordinator`；并吸收 scanner / route / zip_encoding / paths / repository 的相应能力（随协调器内聚迁移，不单独拆包）。
   - S06 安全门槛同步迁移：清单读取与清理固定同一对象及父链；工作目录/输出暂存租约覆盖 Worker 请求间隙；最终回读到发布保留根对象身份；子句柄释放前验证私有 ACL；源文件删除/回收保留身份及成功回执。Infrastructure 当前的 `ManifestHandle` 只固定末端文件，不满足完整门槛。
2. 宿主切换到 application 门面：替换 `Coordinator::open` 构造与 10 处引用；删除 `CoordinatorVaultStore` 适配器（端口直连）。
3. 验证 gate（硬性）：
   - core 全量测试套件在新链路上**等价通过**（98 lib + desktop_pipeline 31 + format_matrix 6 + archive_inspection 3 + worker_client 3 + scanner_regressions 5 + resource_recovery 4 + output_rule_preview 1，基线见台账 §13 表）；
   - 原生验收（套件 A/B/C + 规则）在活动未锁定会话中复跑通过；
   - 快照字段、恢复语义、取消路径与 core 版逐项对照（§1#2"速度事件接入"同期收口）。

### Phase 3 — 物理删除与收尾

1. 删除顺序：逐个"已无引用"模块 → `crates/core` 目录 → workspace `members` 与各 Cargo.toml 依赖（`openunpack-core` 引用清零）。
2. 镜像规则解除：6 处 `MIRROR WARNING` 横幅随删除失效；台账 §1#1 从"另列"改"已交付"，并登记本计划执行结果。
3. 验证：全仓 `cargo check --workspace --all-targets`、`cargo tree` 无 core 节点、release 构建三件套。

## 4. 不变量与防护

- **双写规则在 Phase 3 前持续有效**：`vault.rs` / `worker.rs` / `worker_manifest.rs` / `database_lease.rs` 四对镜像的任何行为修改必须同变更集双写（MIRROR 横幅）。
- **数据兼容红线**：DPAPI vault 文件、SQLite 数据库（schema + 版本门）、DB 租约锁、Worker 管道握手/清单协议，迁移前后互操作；Phase 2 切换前需做旧数据目录实机挂载验证。
- **禁止大爆炸删除**：仅删除"承继者测试全绿 + 引用清零"的模块；任何模块出现未覆盖行为，原样保留在 core 中并回到双轨。
- **回退保险**：Phase 2 未通过 gate 时，宿主可无条件回退 `Coordinator::open` 构造（core 代码未动）；`CoordinatorVaultStore` 适配器保留到切换被验收为止。

## 5. 建议排期（相对工作量）

| 阶段 | 量级 | 前置 |
|---|---|---|
| Phase 0 | 半天内（机械） | 无（可立即执行） |
| Phase 1 | 安全 IO 归属与对等验证方案完成后重估；旧的 1–2 天估算失效 | S06 安全前置条件；Phase 0 可独立安排 |
| Phase 2 | 主工程（与 §1#1"另列"项同一工作包） | Phase 1 完成；§1#2 速度事件收口 |
| Phase 3 | 收尾（Phase 2 gate 通过后立即） | Phase 2 验收 |

**下一步动作建议**：本轮先对齐纯相对路径校验并加强镜像门禁，生产继续使用 Core。Phase 0 按当前引用重新核对后安排；Phase 1 先完成安全 IO 共享归属与对等测试方案，验证 S06 边界后再切换 toolbox；Phase 2 排期以完整执行链工作包启动为准。
