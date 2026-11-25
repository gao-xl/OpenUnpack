# 优化问题清单（2026-11-17）

> 来源：2026-11-17 只读代码审查。审查范围覆盖性能、可读性、可维护性、资源占用和潜在问题。
> 证据等级：以下问题均有当前源码路径与行号依据；未运行完整构建、全量测试或真实负载测量。
> 状态规则：只追加和就地关闭；关闭时补充修复提交、验证命令、结果和日期。

## 严重级别定义

| 级别 | 定义 |
|---|---|
| P0 | 已证实的安全绕过、数据损坏、不可逆操作或核心流程不可用 |
| P1 | 已证实的用户可见正确性问题，或在大状态下可能造成明显性能/资源退化 |
| P2 | 已证实的局部低效、可维护性风险或需要基准验证的潜在扩展性问题 |

## 汇总

| 编号 | 优先级 | 状态 | 问题 | 主要影响 | 建议负责人 |
|---|---|---|---|---|---|
| OPT-001 | P1 | RESOLVED（待提交） | 批量进度聚合订阅可能不触发重渲染 | 用户可见进度停滞 | 前端 |
| OPT-002 | P1 | RESOLVED（待提交） | 原生/守护进程切换后实时进度监听可能失效或串源 | 实时进度缺失、旧事件污染 | 前端 |
| OPT-003 | P1 | OPEN | 快照 delta 只降低传输，后端仍按全量状态构建 | 大任务/大密码库下 CPU、内存和锁占用 | Core |
| OPT-004 | P2 | PARTIAL | 目录监听重复全目录扫描且历史集合无界 | 后台 CPU、I/O、电量和长期内存 | Core |
| OPT-005 | P2 | PARTIAL | 大历史记录下重复全量排序 | 快照和历史分页 CPU、临时分配 | Core |
| OPT-006 | P2 | RESOLVED（待提交） | `publications.job_id` 缺少索引 | 删除、恢复和清理历史的数据库成本 | Core/SQLite |
| OPT-007 | P2 | PARTIAL | SFX 探测重复读取，扫描对全文件执行魔数探测 | 大目录扫描 I/O 与耗时 | Core |
| OPT-008 | P2 | OPEN | 多个上千行模块提高修改和回归成本 | 可维护性、审查效率、并行冲突 | 前端/Core |

---

## 实施进展（2026-11-17）

本轮已直接落地以下代码改动（产品源码）：

| 关联 | 改动 | 验证 |
|---|---|---|
| OPT-001 | `progressStore.ts` 增加聚合快照包装对象，进度/清理时更新引用；`QueuePage.tsx` 改读 `store.entries`；测试补充“快照引用随更新变化”断言 | `npm test -- progress-store.test.tsx` 4/4；`npx tsc -b` 通过 |
| OPT-002 | `api.ts` 增加 `syncProgressListener()` 与代际计数，原生 `job-progress` 监听器随连接模式建立/拆除、不再重复注册或泄漏；`setDaemonConnection`/`onProgress` 接入同步；新增连接切换回归测试 | `npx tsc -b` 通过；`npx vitest run tests/progress-store.test.tsx tests/daemon-connection.test.ts` 20/20 |
| OPT-004 | `watcher.rs` 的 `to_remove` 由 `Vec` 改为 `HashSet`，消除每 tick 的线性查找；新增 per-tick 目录列表缓存，同目录多个分卷只 `read_dir` 一次 | `cargo test -p openunpack-core --lib -- watcher::` 7/7 |
| OPT-005 | `coordinator/mod.rs::top_n_newest` 签名由 `&mut [&JobView]` 修正为 `&mut Vec<&JobView>`，修复 `clear()`/`truncate()` 无法在切片上调用的编译错误（partial-select 最近 20/历史已生效） | `cargo test -p openunpack-core --lib -- snapshot:: commands::` 20/20 |
| OPT-006 | `repository.rs` 增加 `user_version` v1→v2 迁移，创建 `idx_publications_job_state(job_id,state)` 索引；新库直达 v2；补迁移/回滚/直达三组测试 | `cargo test -p openunpack-core --lib -- repository::` 6/6 |
| OPT-007 | `scanner.rs` 将显式 SFX 的 payload 扫描合并为一次，消除 512KB 重复读取 | `cargo test -p openunpack-core --lib -- scanner::` 17/17 |

OPT-004 的退避/TTL、OPT-005 的历史页增量索引、OPT-007 的快速路径魔数跳过等剩余部分仍 OPEN，属行为级或结构改动，需单独评审。

---

## OPT-001：批量进度聚合订阅可能不触发重渲染

- **优先级**：P1
- **状态**：RESOLVED（待提交）
- **影响维度**：正确性、性能感知、测试完整性

**证据**

- `apps/desktop-ui/src/progressStore.ts:5-33`：原地修改全局 `Map`。
- `apps/desktop-ui/src/progressStore.ts:81-91`：`useAggregateProgress()` 的 `getSnapshot` 始终返回同一个 `progressStore` 引用。
- `apps/desktop-ui/src/components/QueuePage.tsx:199-257`：批量统计依赖 `store`，并通过 `useMemo(..., [store])` 缓存。
- `apps/desktop-ui/tests/progress-store.test.tsx:104-124`：只断言 Map 内容变化，没有断言组件重渲染。

**原因与潜在后果**

`useSyncExternalStore` 依赖快照引用变化判断是否需要重渲染。当前监听器虽然会被调用，但 `getSnapshot` 返回同一个 Map 引用，React 可能认为状态未变；`useMemo` 也不会重新计算。批量进度卡中的总字节数、实时速率和预计剩余时间可能停留在旧值。

**建议**

1. 为聚合状态增加递增版本号，或让快照返回不可变对象/数组。
2. 最小改动方案：维护 `aggregateRevision`，每次进度事件递增，并让快照返回包含 revision 的新对象。
3. 测试补充“连续两次进度事件触发两次渲染”和“最终显示第二次进度”。

**预期收益**

修复用户可见的批量进度停滞，同时保留现有按任务隔离的订阅模型。

**取舍**

不可变快照会产生少量复制成本；聚合规模通常远小于全量任务快照，收益高于成本。

**建议验证**

- `npm test -- progress-store.test.tsx`
- 新增组件渲染计数断言，不只断言 Map 内容。

---

## OPT-002：原生/守护进程切换后，实时进度监听可能失效或串源

- **优先级**：P1
- **状态**：RESOLVED（待提交）
- **影响维度**：正确性、资源生命周期、跨模式一致性

**证据**

- `apps/desktop-ui/src/api.ts:588-600`：`onProgress()` 在注册时根据当时的 `activeDaemon` 决定是否挂接原生 `job-progress`。
- `apps/desktop-ui/src/progressStore.ts:18-40`：模块级 `initProgressStore()` 只执行一次，不随连接模式变化重建。
- `apps/desktop-ui/src/api.ts:389-407`：`setDaemonConnection()` 会递增 `connectionEpoch`，但进度存储没有同步重建。

**原因与潜在后果**

从原生模式切换到守护进程后，旧原生监听器可能继续投递本地进度；从守护进程切换回原生模式后，如果初始化时未挂接原生监听，实时进度可能完全缺失。快照订阅已有 epoch 防护，progress store 没有对应生命周期。

**建议**

1. 让 `api.onProgress()` 返回真正可执行的取消函数。
2. 按 `connectionEpoch` 管理进度监听生命周期。
3. 连接变化时取消旧监听并建立新监听。
4. 增加“原生 → daemon → 原生”切换回归测试。

**预期收益**

避免实时进度消失、旧任务事件污染当前连接视图。

**取舍**

需要更严格的监听生命周期管理；改动集中在前端 API 和 progress store。

**建议验证**

- 新增连接切换测试：原生事件不得在 daemon 模式投递；切回原生后必须重新收到事件。
- 手动验证桌面模式和守护进程模式互切后的任务进度。

---

## OPT-003：快照 delta 只减少传输，没有消除后端全量构建成本

- **优先级**：P1
- **状态**：OPEN
- **影响维度**：性能、资源占用、锁竞争、可扩展性

**证据**

- `crates/core/src/coordinator/snapshot.rs:22-48`：每次队列快照都会筛选、排序并克隆任务。
- `crates/core/src/coordinator/snapshot.rs:59-66`：每次都会重新获取全部密码视图。
- `crates/core/src/coordinator/snapshot_projector.rs:31-48`：每次捕获完整快照并保存为新基线。
- `crates/core/src/coordinator/snapshot_projector.rs:75-125`：每次 delta 都建立 HashMap/HashSet，并对全部任务、密码做深度比较。
- `crates/core/tests/load_profile.rs` 中的 `synthetic_snapshot_history` 是 ignored 手工测量；`.github/workflows/windows.yml` 没有运行该负载套件。

**原因与潜在后果**

当前 delta 协议降低了网络传输，但状态更新时后端仍执行 O（任务数 + 密码数 + 嵌套节点数） 的克隆、索引和比较。历史记录、密码或嵌套节点很多时，高频状态更新会放大 CPU、内存分配和锁占用。

**建议**

1. 任务、密码、设置分域维护 revision。
2. 密码库未变化时复用上次 `PasswordView` 投影。
3. 在 `view_state` 下维护不可变快照投影，任务变化只替换对应项。
4. “最近 20 条历史”维护增量索引，而不是每次重新计算。
5. 将 `synthetic_snapshot_history` 扩展到 10,000 任务/密码，并加入 `snapshot_update` p95/p99 预算。

**预期收益**

大历史、大密码库下显著降低快照构建 CPU、临时分配和锁持有时间。

**取舍**

失效逻辑更复杂；必须先保证 revision、session 和 delta 基线一致，不能为了性能牺牲一致性。

**建议验证**

- `./scripts/measure-load.ps1 -Worker <冻结Worker> -Profile snapshots`
- 扩展测量后比较 10/1000/10000 记录的 p50/p95/p99 与峰值内存。

---

## OPT-004：目录监听重复全目录扫描且历史集合无界

- **优先级**：P2
- **状态**：PARTIAL
- **影响维度**：性能、资源占用、长期运行稳定性

**证据**

- `crates/core/src/watcher.rs:458-494`：每 500ms 遍历所有 pending 候选并重新读取元数据。
- `crates/core/src/watcher.rs:511-513`：循环中使用 `to_remove.contains(path)`，候选多时是 O(n²)。
- `crates/core/src/watcher.rs:540-553`：每个稳定分卷都会重新 `read_dir` 扫描父目录。
- `crates/core/src/watcher.rs:411`、`585`：`enqueued_history` 随监听进程生命周期持续增长。
- `crates/core/src/watcher.rs:623-632`：新增监听目录时会把目录内全部既有文件加入历史集合。

**原因与潜在后果**

多分卷同时落盘或监听大目录时，重复目录扫描会放大 CPU 与磁盘 I/O；长期运行时历史集合增长会造成内存缓慢上升。

**建议**

1. `to_remove` 改为 `HashSet<PathBuf>`。
2. 按“父目录 + 分卷 stem”分组，每个 tick 每个目录最多扫描一次。
3. 对 preflight 持续失败的候选采用指数退避。
4. 给历史集合增加容量/TTL，或保存可轮换的紧凑指纹。

**预期收益**

降低后台 CPU、磁盘 I/O、电量消耗和长时间运行内存增长。

**取舍**

分组缓存和退避增加少量状态；TTL 方案需接受极端情况下“同路径、同大小、同 mtime 替换”可能重新入队。

**建议验证**

- 增加多分卷、大目录、持续 preflight 失败三组确定性测试。
- 负载测量中记录 watcher 线程 CPU、目录读取次数和常驻内存。

---

## OPT-005：大历史记录下重复全量排序

- **优先级**：P2
- **状态**：PARTIAL
- **影响维度**：性能、内存分配、可扩展性

**证据**

- `crates/core/src/coordinator/snapshot.rs:22-37`：每次 queue snapshot 都收集全部终态任务并排序，只为取最近 20 条。
- `crates/core/src/coordinator/commands.rs:310-326`：每次历史分页都收集全部历史并完整排序，再截断。

**建议**

1. 最近 20 条使用 partial select 或固定容量堆。
2. 历史页维护按 `created_at,id` 排序的增量索引。
3. 如果继续保持 SQLite 为权威，可将历史查询逐步下推到 SQL 排序和分页。

**预期收益**

在数千到上万历史记录下减少每次状态刷新的 CPU 与临时分配。

**取舍**

增量索引会增加写入路径复杂度；如果 `history_limit` 长期保持 1000–2000，优先级低于快照分域缓存。

**建议验证**

- 使用现有快照负载测量扩展 10,000 历史记录，比较 p95 和分配趋势。

---

## OPT-006：`publications.job_id` 缺少索引

- **优先级**：P2
- **状态**：RESOLVED（待提交）
- **影响维度**：数据库性能、长期数据增长

**证据**

- `crates/core/src/repository.rs:63-68`：`publications` 只有主键，没有 `job_id` 索引。
- `crates/core/src/repository.rs:150-165`：删除任务时按 `job_id` 查询并删除。
- `crates/core/src/repository.rs:177-198`：清理历史时执行按 `job_id` 关联的子查询。

**建议**

1. 通过 schema migration 增加索引，例如：`CREATE INDEX idx_publications_job_state ON publications(job_id, state);`
2. 增加旧版本数据库升级测试和事务回滚测试。

**预期收益**

历史与发布记录增多后，删除任务、恢复检查和清理历史的成本更稳定。

**取舍**

增加少量磁盘占用和写入开销；需要正式的 `user_version` 迁移，不能隐式改库。

**建议验证**

- 新增 migration 测试：旧库升级后索引存在，失败时整体回滚。
- 用 `EXPLAIN QUERY PLAN` 验证按 `job_id,state` 查询命中索引。

---

## OPT-007：SFX 探测重复读取，扫描对全文件执行魔数探测

- **优先级**：P2
- **状态**：PARTIAL
- **影响维度**：扫描性能、I/O、资源占用

**证据**

- `crates/core/src/scanner.rs:391-397`：每个候选路径都会先读取 512 字节。
- `crates/core/src/scanner.rs:399-405`：显式 SFX 调一次 `has_sfx_archive_payload()`。
- `crates/core/src/scanner.rs:418-420`：显式 SFX 又调用一次。
- `crates/core/src/scanner.rs:315-334`：每次 SFX payload 探测最多读取 512KB。
- `crates/core/src/scanner.rs:999`：目录遍历时每个文件都进入 `is_supported_archive_path()`。

**建议**

1. 单次读取后缓存 SFX 判定结果，消除重复 512KB 读取。
2. 对常见扩展先走快速路径，再仅在需要伪装识别时读取魔数。
3. 为 10 万混合文件目录增加扫描耗时基准，再决定是否调整“伪装归档识别”默认策略。

**预期收益**

大目录扫描时减少文件打开次数、首块读取和 SFX 重复 I/O。

**取舍**

如果跳过魔数识别，会改变伪装归档发现能力；建议先消除重复读取，行为级变化单独评审。

**建议验证**

- 构造普通扩展、伪装扩展、显式 SFX、伪 SFX 四组回归。
- 记录 10 万混合文件扫描耗时和读取字节数。

---

## OPT-008：多个上千行模块提高修改和回归成本

- **优先级**：P2
- **状态**：OPEN
- **影响维度**：可维护性、可读性、并行协作风险

**证据**

- `apps/desktop-ui/src/components/QueuePage.tsx`：1759 行。
- `apps/desktop-ui/src/components/SettingsPage.tsx`：1292 行。
- `apps/desktop-ui/src/components/ToolboxPage.tsx`：1124 行。
- `crates/core/src/coordinator/commands.rs`：1107 行。
- `crates/core/src/coordinator/scheduler.rs`：945 行。
- `crates/core/src/coordinator/execution.rs`：796 行。

**建议**

优先拆纯逻辑，而不是先做视觉性移动：

1. `QueuePage`：拆出扫描选择、密码提交、批量统计、任务操作列、删除流程。
2. `commands.rs`：按历史、队列控制、重试、删除分模块。
3. `scheduler.rs`：拆出资源准入、任务认领和调度策略。
4. 每次拆分保持公开行为不变，并同步收紧现有架构行数门禁。

**预期收益**

降低冲突概率，提升单测可定位性和代码审查效率。

**取舍**

文件数量增加，可能出现更多参数传递；应小步拆分，避免一次性大范围重排造成并行会话冲突。

**建议验证**

- 每轮拆分后运行 `npm run lint`、`npx tsc -b`、`cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets -- -D warnings`。
- 关键拆分后运行对应前端组件测试和 Core 协调器测试。

---

## 建议实施顺序

### 短期：低风险、高收益

1. OPT-001：修复聚合进度快照引用，并补渲染测试。
2. OPT-002：按 `connectionEpoch` 重建 progress 监听，覆盖模式切换。
3. OPT-004：先将 `to_remove` 改 `HashSet`，并复用每 tick 的目录扫描结果。
4. OPT-007：先消除 SFX 512KB 重复读取。

### 中期：有基准约束的结构优化

5. OPT-003：快照分域缓存，先只缓存密码视图和最近 20 条历史。
6. OPT-006：增加 `publications(job_id,state)` 索引及数据库迁移测试。
7. 将 `synthetic_snapshot_history` 纳入定期性能回归，而不是只保留手工 ignored 测试。

### 长期：降低维护成本

8. OPT-008：小步拆分 `QueuePage` 与 `commands.rs` 热点模块。
9. 为快照构建、watcher、扫描、SQLite 查询建立持续性能预算。

## 关闭记录

| 编号 | 关闭日期 | 修复提交 | 验证命令与结果 | 备注 |
|---|---|---|---|---|
| OPT-002 | 2026-11-17 | 待提交 | `npx tsc -b` 通过；`npx vitest run tests/progress-store.test.tsx tests/daemon-connection.test.ts` 20/20 | 原生进度监听随连接模式建立/拆除，代际计数防陈旧落定 |
| OPT-006 | 2026-11-17 | 待提交 | `cargo test -p openunpack-core --lib -- repository::` 6/6；`--test architecture_convergence` 5/5 | v1→v2 迁移建 `idx_publications_job_state(job_id,state)` |
