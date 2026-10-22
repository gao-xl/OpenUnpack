# Coordinator 私有状态组件改造

日期：2026-10-20。范围：现役 Rust Coordinator 内部职责提取；保留原有脏工作区。组件实现完成，库/编译/静态门禁通过；完整原生回归仍有未核销项。验证表记录本轮结果，不能用此前报告代替。

## 实现与边界

从原来的文件分组继续提取三个拥有私有状态的组件，禁止它们引用或回持 Coordinator：

| 组件 | 实际所有权与调用方式 |
|---|---|
| `cache_budget::CacheBudgetManager` | 配额预留、未知大小任务归属、阶段 BudgetClaim。资源错误携带暂停要求，由 Coordinator 修改调度状态。租约仅通过退出策略共享原有请求注册表、Worker 回执与停止标记；不复制进程状态，Coordinator 析构后也不提前释放未确认写进程的预留。 |
| `snapshot_projector::SnapshotProjector` | delta 基线和引擎探测 TTL。采样与基线写入共同串行化，锁序为 projector → view_state → 字段/引擎探测；同步采样闭包不被储存。普通 snapshot/queue_snapshot 不推进 delta 基线。 |
| `publication::PublicationService` | 发布门、原格式 Journal，以及不可复制的 staging/sealed/committed 阶段对象。正常发布与 verify 恢复共用提交组件和原 Repository；实际 Worker 请求、暂停/取消检查、源清理互斥继续由 Coordinator 驱动。 |

Coordinator 的生产字段从 43 个降至 41 个，直接 Mutex 字段从 30 个降至 25 个；数量均不含测试专用 topology probe。`execution.rs` 从 1,068 行降至约 917 行，`recovery.rs` 从 371 行降至约 302 行。收益来自私有状态与调用边界，不以文件变短作为解耦完成证明。

任务生命周期、删除门禁、任务/进度一致性投影及嵌套解压主循环仍由现役 Coordinator 管理；本轮没有将全部状态转换接入 Domain 状态机，也没有新建另一套调度器或可写任务库。

## 保留的不变量

1. 原 Journal 和 SQLite planned intent 先于临时目录创建、直接搬移或 Worker 写回。
2. 校验树对象转移到无覆盖 rename，不按路径重新打开一个替代对象。底层 LockedTree 仍按原设计在 NTFS rename 前关闭子条目 lease，保留根与祖先对象；提交 receipt 保留这些身份直到源清理结束。本轮未加强或放宽底层安全 IO 的威胁边界。
3. rename 成功到 publication/job/success-outbox 原子事务之间没有 await 或取消分支。源清理始终晚于该事务。
4. 两次冲突检查保留原 skip/改名行为；恢复不应用正常发布的自动改名策略。
5. Worker 未确认退出时继续保留资源预留并阻止后续写入/清理；任务删除与晚到 put、可见数据与 revision 的原有互斥保持。
6. 数据库 v1、Journal 字段、宿主 API、Worker/桌面协议不变；验证耗时仍在 ready SQLite 写入前结算。

## 新增回归与门禁

- 预算 2 项：Coordinator 销毁后的未确认退出保护；暂停要求由编排层应用。
- 快照 4 项：任务/密码/设置/state 的 delta 往返；session/stale 回退；采样失败不丢基线；双线程慢采样的基线连续性。
- 发布 4 项：实际 Windows 根身份保持及 publication/job/outbox 提交；rename 后数据库注入失败的核验恢复；竞争后的 skip；进入同步 commit 后不再读取取消 token。最后一项直接验证组件，Coordinator 的 commit 前 checkpoint 仍保留，不能解释为端到端 seal 后取消也必须发布。
- 发布数据库故障注入属于同进程写入失败恢复验证，不等同于宿主骤停、断电或磁盘损坏。
- 架构门禁检查组件单一归属、私有账本/缓存/门、禁止 Coordinator 引用、禁止原字段回流，并新增真实负向控制。兼容同时加入工作区的 CLI：仅允许依赖 Protocol 与拥有 ApiCommand 契约的 Local API，另有禁止直接依赖 Core 的负向控制。

## 本轮验证

| 检查 | 结果 |
|---|---|
| 修改前 Core 库基线 | 124 通过，0 失败，1 项原配置忽略 |
| 修改后 Core 库 | 134 通过，0 失败，1 项原配置忽略 |
| 发布组件定向回归 | 4/4 通过，真实 Windows 文件系统与 SQLite |
| 架构门禁 | 14 项负向控制通过 |
| 三方契约门禁 | 30 个桌面 DTO、8 个 Worker DTO、302 字段与 7 项变异控制通过 |
| 本轮 Rust 文件格式 | `rustfmt --check --edition 2024 --config skip_children=true ...` 通过 |
| v1 存储/租约/Worker 注册表集成 | architecture_convergence 5/5 通过 |
| 真实 Worker 普通单归档管线 | 隔离 TEMP 后单例通过；实际经历解压、校验、规划与发布 |
| Core 完整集成/原生管线 | 未通过；系统 TEMP 首轮失败，隔离 TEMP 重跑只取得部分通过，重负载用例仍有失败，详见下文 |
| 工作区所有目标编译 | `cargo check --workspace --all-targets --locked --offline` 通过，15m38s；另有 Local API 测试使用弃用方法的警告，本轮未改该测试 |
| 并行更新后的当前 Core 复核 | `cargo check -p openunpack-core --all-targets --locked --offline` 再次通过，2m21s；发布 4 项重新编译并全部通过 |
| 其他工作区库回归 | 额外编译在内存压力下中止，未取得测试汇总，不计为通过；未以放宽资源保护来换取测试结果 |

首次完整原生测试在系统 TEMP 下出现 7-Zip 夹具生成失败和 Worker IoError。独立 ZIP/7z 命令复现：同一程序写系统 TEMP 返回拒绝访问，写工作区目录成功。只对本轮测试进程设置 TEMP/TMP 为 `_workspace/cc-native-temp-20261020` 后，真实 Worker 单归档解压/验证/发布通过；完整原生重跑使用同一个已编译、已哈希的测试二进制，不修改或跳过测试断言，不改变系统权限或安全设置。失败及重跑记录均保留。

隔离 TEMP 后仍有两类未核销问题：

- 30,000 条目大清单检查返回 `WorkerExitUnconfirmed`（67.84s）；其根因本轮未定位，不能把它自动归因于 TEMP 或内存，也不能宣称已修复。
- 高压缩比缓存单例以 `--exact --nocapture` 记录：任务一直处于 `extracting`，随后显示“等待可用内存”，在既有 90 秒边界后失败，总计 93.66s，尚未进入本轮提取的发布阶段。现场观测可用内存约 1.3 GiB，仍有其他会话的编译活动。本轮预算公式/Worker 请求处理与修改前逐段一致，但未在修改前版本原生复现此失败，不能仅凭静态比对判定原生行为等价。

为减少争用，已停止本轮额外库测试编译和未完成的重负载原生批次；没有停止其他会话或用户程序。缓存竞争和完整 resource_recovery 批次因此不记为通过。部分桌面测试的通过不能替代完整原生验收。

可在资源允许时执行 `_workspace/coordinator-components-20261020/run-native-binaries.ps1` 复核。脚本使用隔离 TEMP，并先核对本轮 Coordinator 源码、实际测试二进制及 Worker/7-Zip 哈希；若源码或物料变化则拒绝使用旧结果，需重新构建并固定。`baseline-source/` 仅保存了修改前 Coordinator 的独立对照来源，未运行其原生测试，不作为“修改前已通过/已失败”的证据。

## 证据、回退与验收范围

本轮副本、命令脚本、日志、源码/测试二进制/固定 Worker/7-Zip 哈希保留于 `_workspace/coordinator-components-20261020/`。固定 Worker 复制自本日架构收敛的物料，本轮未改 C#/TS 协议；集成用例通过 `OPENUNPACK_TEST_WORKER` 使用固定路径。

同时有其他会话修改 watcher、hook、CLI 和工作区清单；均保留。首次 `--locked` 曾在 CLI 清单与锁文件写入间隙失败，锁文件就绪后检查恢复；没有替其他会话改写 Cargo.lock。最终以源码指纹与日志记录的目标为验证范围。

最终指纹复核发现并行任务在 `integrity.rs::validate_settings` 增加了热文件夹输出回流校验，调用新 `intake_guard` 模块。本轮三个组件和自身修改文件的哈希没有漂移；该共享辅助文件更新不包含在最初 134 项库测试的冻结源码中。已保留此更新并重新通过当前 Core 全目标编译和发布 4 项回归，记录见 `core-current-check.log`、`publication-current-tests.log`、`parallel-source-drift.json`；没有把旧原生二进制当成这一新辅助版本的完整验收。原生复跑脚本对这种漂移会拒绝执行，需重新构建/固定后再运行。

回退须对照 `before/` 逐文件撤销本轮修改，合并保留副本后其他会话的新增内容；禁止 reset/clean 或整目录覆盖。没有数据库迁移，无需用户数据降级。

自动回归不替代原生 GUI、签名安装包、干净机器、真实第二卷/物理介质拔除及断电验收；本轮不重新核销既有 S06 的发布边界。
