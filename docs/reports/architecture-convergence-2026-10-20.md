# 7.2 架构过渡债务收敛报告

日期：2026-10-20。实现已完成，运行验证正在执行；最终结果以本文验证表为准。

## 收敛结果

本次采用**生产实现收敛**：保留并拆解现役 Core Coordinator，将 Core 明确为集成/编排层，Infrastructure 承接唯一的外部机制实现。未采用把不完整 Application TaskCoordinator 接管生产的路线，也未把迁移或改名等同于完成原计划中的纯 Application 编排架构。

- 原约 8,900 行协调器拆为共享状态声明、13 个职责模块和原测试文件；最大生产模块 `execution.rs` 约 1,016 行，原外部接口不变。
- WorkerClient、Vault、database lease、manifest、相对路径策略五处镜像改为单源实现。Core 旧路径只再导出 Infrastructure 类型。四个进程级 Worker 注册表只有一个定义及编译实例。
- 安全 IO、私有命名空间检查、COM 回收和磁盘拓扑整组下沉，保留现役父链句柄、相对对象操作、私有 ACL 及退出确认边界。旧 Infrastructure manifest 的弱文件打开辅助已移除。
- 无生产消费者的 `TaskCoordinator`、`JobUseCases`、`TaskStore`、`SqliteTaskStore` 和专属 v2 迁移测试退役；Application 的现役密码用例、Domain 纯策略和 Infrastructure 端口适配器保留。
- 生产数据库仍为 SQLite v1 / JobView；数据目录、DPAPI 文件、发布/outbox 和 Worker/桌面协议不变。本次未打开或迁移用户真实数据库、密码库。
- 删除 `check-mirrors.mjs`，由 `check-architecture.mjs` 检查真实 Cargo 依赖、唯一实现、兼容桥、退役管线、源码重复装配及单模块行数。契约检查入口、贡献指南、PR 清单和开发角色指引均改为单源规则。

## 验证

| 项目 | 结果 / 证据 |
|---|---|
| 修改前结构基线 | 原五组镜像及协议自检通过 |
| 修改前 Rust 库基线 | 独立固定源码副本运行中断，退出 -1，未输出完整结束汇总；不记为全绿，保留 `baseline-lib.log` |
| 源体静态等价 | 9 项通过；91 个迁移测试保留；`infrastructure-source-proof.json` |
| 协调器完整性 | 251 个函数名称与数量一致，原测试内容去包装/空白后相等；独立 QA 复核 |
| 架构自检 | 9 项负向控制通过；覆盖反向依赖、第二管线、源码副本、重复装配、弱化 manifest 打开与单模块膨胀 |
| 格式检查 | `cargo fmt --all -- --check` 通过 |
| Worker 重新构建 | `dotnet build ... -c Release --no-restore -m:1` 通过，0 警告、0 错误 |
| 修改后 Core 库测试 | 122 通过，0 失败，1 项需要第二个卷的用例按原配置忽略；`rust-tests.log` |
| 新增存储/注册表兼容回归 | 5/5 通过；`core-integration.log`（首轮事务夹具状态错误已修正，未放宽生产约束） |
| 原生归档检查 | 常规预算/安全路径、GBK 两项通过；30,000 条目 / >8 MiB 清单首轮 `IoError`，固定 Worker/Engine 后单例重跑通过（25.90s）；日志 `native-spill-rerun.log` |
| 修改后 Rust 编译/回归 | 待运行结果 |

新增生产兼容回归位于 `crates/core/tests/architecture_convergence.rs`，覆盖：v1 冻结选项/嵌套状态/发布/outbox 重新打开、拒绝未来版本而不改写记录、拒绝未知 JSON 而不做部分转换、Core/Infrastructure 共用数据库租约、两条公开路径共用 Worker 退出回执注册表。WAL 用例保留同进程 witness 连接再打开 Repository，属于有已提交 WAL 的 reopen 验证，不是进程崩溃或断电恢复验收。

大清单首次失败原因未确定，保留失败和复跑证据，不将一次复跑通过解释为定位或修复了该 `IoError`。复跑使用 `worker-frozen/` 的固定物料及 `worker-frozen-manifest.json` 哈希，未修改生产解析、路径、资源或退出校验。该用例仅补充失败诊断，输出既有脱敏 Worker 诊断记录。

## 证据范围与保留边界

本次结束的是双任务管线和五处源码镜像维护；Core 不再以等待未完成协调器替换的临时层定位。Domain 的纯模型库仍不代表每个生产状态转换已接入领域状态机。未来提取用例应直接从现役链分离，经生产回归替换，禁止重建另一套任务存储或协调器。

Infrastructure 安全 IO 和现役 Core 一样以 Windows 为目标，不提供较弱的 Unix fallback。自动测试不替代原生 GUI、签名发布、干净机器安装、断电或物理跨盘验收；本次没有重新核销既有 S06 的全部发布边界。

## 回退与工作区保护

本轮备份和日志位于 `_workspace/architecture-transition-20261020/`。包含修改前文件、初始 Git 状态、固定基线源码、源体比对、独立 scout/rust-dev/qa 报告及构建日志。实际执行为 Codex 可用继承模型，未声称执行 AGENTS 表中的 Haiku/Sonnet。

为避免与其他 Rust 进程争用包缓存及目标目录，本轮使用独立 Cargo 状态目录、离线既有 registry 和 `target_architecture_transition`，构建串行并限制并发。修改前基线与修改后源代码分离。保留工作区所有既有未提交改动，未进行 reset、clean、批量暂存、部署或发布。

原有全树 `git diff --check` 报告 `.workbuddy/memory/2026-10-10.md` 与 UI `styles.css` 的既有空白问题，未改动这些无关文件；本次变更范围单独核对。

若需回退，按本轮副本逐文件恢复本次范围，同时保留副本后新增的其他修改。数据库没有格式升级，无需降级。
