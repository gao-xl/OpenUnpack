# OpenUnpack 现行架构

2026-10-20 架构收敛决策。具体实施和验证证据见 [实施报告](reports/architecture-convergence-2026-10-20.md)，回退与完成条件见 [方案](plans/2026-10-20-architecture-convergence.md)。

## 单一生产链

```text
React / HTTP API
        ↓
Tauri Host → Core Coordinator（单一编排与任务状态权威）
                   ├─ Repository → SQLite v1 / JobView / publication / success outbox
                   ├─ Scanner / Watcher / Toolbox
                   └─ Infrastructure
                        ├─ WorkerClient → 受限双命名管道 → .NET Worker → Engine
                        ├─ DPAPI Vault
                        └─ 安全 IO / 回收 / 路由与磁盘拓扑 / 数据库租约

Host → Application VaultUseCases / SystemEnvironment 端口
Infrastructure → Application 端口 / Domain 纯策略 / Protocol
Application → Domain / Protocol
```

Core 是明确的集成层，其依赖 Infrastructure 是装配生产机制的方向。Infrastructure、Application、Domain、Protocol 均不得反向引用 Core。Application 的 Vault 用例和基础设施适配器保留；Domain 的纯模型/策略库不表示另一个可执行任务管线，也不意味着其状态机支配全部生产行为。

不再维护未被宿主装配的 `TaskCoordinator`、`JobUseCases`、`TaskStore`、`SqliteTaskStore`。这些原型的流程和数据格式没有与生产对等，已从构建路径退役。生产存储继续使用 v1；本次没有对现有数据执行 v2 转换，也没有创建第二个调度器或数据库写入权威。

## 源码所有权

| 职责 | 唯一实现 |
|---|---|
| 任务编排 | `crates/core/src/coordinator/` |
| 缓存预算预留、未知大小任务归属 | Coordinator 的 `cache_budget::CacheBudgetManager` |
| 快照差异基线、引擎探测 TTL | Coordinator 的 `snapshot_projector::SnapshotProjector` |
| 发布锁、发布日志及验证后提交 | Coordinator 的 `publication::PublicationService` |
| 任务、发布及成功事件事务 | `crates/core/src/repository.rs` |
| Worker 生命周期、进程级退出注册表 | `crates/infrastructure/src/worker/client.rs` |
| spill 清单读取与校验 | `crates/infrastructure/src/worker/manifest.rs` |
| 路径校验、父链固定及相对句柄 IO | `crates/infrastructure/src/paths.rs` |
| DPAPI、库格式、密码文件生命周期 | `crates/infrastructure/src/dpapi/vault.rs` |
| 数据库租约 | `crates/infrastructure/src/sqlite/database_lease.rs` |
| 回收与权限核对 | Infrastructure `saferecycle` / `recycle_shell` |

Core 下原有 Worker / Vault / paths / route / lease 路径只兼容再导出，因此旧调用方和测试使用同一类型、同一静态注册表和同一安全实现。修改底层行为只编辑唯一实现，不再双写或比较镜像。

## 协调器职责

`mod.rs` 保存任务/调度运行时与组件装配；`initialization` 负责打开与启动恢复；`snapshot` 在可见状态门内采样；`commands` 处理队列、重试、删除及配置；`inspection` 管理检查与规则预览；`state` 管理持久化和成功 outbox；`transfers` 管理测量投影；`scheduler` 负责认领和调度；`worker_requests` 负责请求；`resources` 将资源判定应用到调度状态；`execution` 驱动解压和发布阶段；`recovery` 驱动恢复；`pipeline_helpers` / `integrity` 保存相应辅助逻辑。原协调器测试保留在 `tests.rs`，发布组件的原生文件系统/SQLite 回归在 `publication/tests.rs`。

在文件拆分之后，现役链进一步提取了三个私有状态组件；它们不引用 Coordinator，也不持有它的 Arc/Weak：

- `CacheBudgetManager` 唯一持有配额预留和未知大小任务归属，返回预算租约及带暂停要求的错误。Coordinator 决定可见暂停/任务状态。租约通过窄退出策略共享原有请求注册表、Worker 退出回执和停止标记，不复制进程状态；即使 Coordinator 已销毁，仍能保留未确认退出写进程的预算。
- `SnapshotProjector` 唯一持有 delta 基线和引擎探测缓存。`snapshot_update` 的顺序为 projector 锁 → 可见状态门 → 字段锁/引擎探测锁；采样和基线更新在同一 projector 临界区完成，避免旧采样晚写导致基线倒退。同步采样闭包不被储存，持可见状态门时不得重入 `snapshot_update`。普通 `snapshot`/`queue_snapshot` 不推进 delta 基线。
- `PublicationService` 唯一持有发布锁和原格式 Journal。不可复制的 staging/sealed/committed 对象转移临时目录 lease 和同一个验证树。发布锁只保护冲突处理及无覆盖重命名；重命名到同一 Repository 的 publication/job/outbox 事务之间没有 await 或取消分支。提交 receipt 持有发布根及祖先身份至源清理结束；底层 LockedTree 的原有条目句柄释放语义保持。恢复借用同一事务入口，不新增数据格式或写入权威。

Coordinator 仍是唯一任务编排/写入权威，任务生命周期、删除互斥和一致性投影继续共用原有门禁；这不表示所有状态转换已迁入 Domain 状态机。宿主 API、冻结选项、数据库 v1、Journal、源文件清理条件及 outbox 投递顺序保持兼容。具体验证范围见 [组件改造报告](reports/coordinator-components-2026-10-20.md)。

## 门禁与演进

`node scripts/check-architecture.mjs --self-test` 核对真实 Cargo 依赖方向、唯一实现所有权、Core 再导出边界、已退役管线、协调器模块行数和三个组件的私有状态边界。负向控制包含组件回持 Coordinator、暴露内部状态及原字段回流；CLI 作为协议/API 客户端仅依赖 Protocol 与拥有 ApiCommand 契约的 Local API，不能绕过 API 直接依赖 Core。`scripts/check-contracts.ps1` 同时运行协议与架构检查。

未来迁移应直接提取和替换现役用例，以生产数据兼容与真实管线回归为门槛；不再并行搭建另一套协调器、另一个任务 JSON 或另一套可写数据库。历史迁移计划和旧架构讨论稿仅作背景资料，本文件是当前源码归属与运行链依据。

自动回归证明的范围为当前用户合成数据和真实 Worker 管线；原生界面、签名发布、干净机器和物理跨盘验收仍按独立验收要求执行。
