# 7.2 架构过渡债务收敛

日期：2026-10-20。实施前决策；验证结果另见报告。

## 问题与决策

生产入口仍是 Core Coordinator。Application TaskCoordinator 没有完整的解压、嵌套、发布、清理和恢复链；其 SqliteTaskStore 使用 v2 / JobAggregate，而生产 Repository 使用 v1 / JobView。直接切换会改变恢复与数据格式。两个实现的重复维护也不能证明行为对等。

本次选择生产实现收敛：Core 成为明确的生产编排/集成层，Infrastructure 为外部机制的唯一实现，Application 保留已使用的用例和端口，Domain 保留纯策略。退役无生产消费者的 TaskCoordinator、JobUseCases、TaskStore 和 SqliteTaskStore；不进行数据库迁移。该决策替代此前以不完整协调器替换生产协调器的退役路线。

目标依赖：desktop-host → core / application / infrastructure / protocol；core → infrastructure / protocol；infrastructure → application / domain / protocol；application → domain / protocol。禁止下层反向依赖 Core。

## 实施范围

1. 保存本轮文件副本及 Git 状态于 `_workspace/architecture-transition-20261020/`，保留所有既有修改。
2. Infrastructure 单源承接生产版 WorkerClient、Vault、database lease、manifest，整体下沉生产版 paths / saferecycle / recycle_shell / route，保留完整的父链固定、相对句柄和私有 ACL 保护。Core 仅兼容再导出，绝不以较弱的旧 Infrastructure manifest 辅助函数替代现役安全 IO。
3. Coordinator 拆为声明、初始化、快照、命令、检查、调度、Worker 请求、资源管理、执行、恢复、纯辅助及测试模块。保留同一 Coordinator、同一状态与写入权威，方法体以机械迁移为主。
4. 移除未装配的第二任务/存储管线及其专属测试；将有价值的版本拒绝、租约、原格式恢复回归保留在唯一生产 Repository 测试中。保留 VaultUseCases / SystemEnvironment 及其现役适配器。
5. 以依赖方向和单源所有权门禁替代五对内容镜像检查，加入真实的负向控制。更新现行架构说明、PR 清单和开发指导，历史审计保留并注明新决策。

## 完成条件与验证

- 不存在第二个任务协调器或可写入 v2 任务库的运行分支；生产入口/协议/数据目录/SQLite v1/DPAPI/Worker 安全语义保持。
- 五处镜像成为编译期共享的单一实现，旧 Core 入口只再导出；全仓依赖有向无环，架构门禁可阻止恢复双轨。
- Coordinator 单个生产模块控制在约 1100 行以内；测试随原职责保留，外部 API 不变。
- 运行修改前镜像/契约基线，修改后架构/契约门禁、cargo fmt、workspace 全目标检查及库/集成回归；真实 Worker 解压和恢复测试验证新的基础设施路径。原生 GUI、签名安装包与干净机器验收不由这些测试替代。

## 风险与回退

主要风险为模块可见性变化、安全 IO 下沉时遗漏依赖、进程内 Worker 静态注册表重复实例以及把旧原型的专属测试误当成现役覆盖。使用 Core 的完整安全实现、类型再导出、唯一 Worker 模块和现役回归控制这些风险。协议不改动，C#/TS 仅运行适当验证，不扩展功能。

回退按本轮备份逐文件恢复；不使用 reset/clean、不覆盖其他人的修改，不访问用户真实数据库或密码库。无需数据库降级步骤。
