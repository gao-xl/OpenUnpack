# OpenUnpack 最终重构方案

> 2026-10-20：本文保留历史设计/迁移背景；当前源码归属及运行链以 [现行架构](architecture-current.md) 为准。Core 已明确为唯一生产编排层，未装配的 TaskCoordinator / SQLite v2 原型退役，底层镜像收敛为 Infrastructure 单源实现。旧的 Core 整体退役及镜像双写要求不再作为当前执行指引。

版本：2.1 · 2026-10-10 · 状态：设计定稿建议，已按 7-Zip 与平台官方资料补充优化；不代表代码已完成或产品已验收。

> 任务状态注记（2026-10-11）：本方案为架构设计背景，执行进度不以本文推断。当前待办与状态以 `docs/status-ledger-2026-10-11.md` 为唯一真源；当前修复轮的施工范围见 `docs/remaining-fixes-plan-2026-10-11.md`。

## 1. 结论与范围

**建议重构。保留 Tauri 2 + React/TypeScript + Rust 主核心 + 独立 .NET/7-Zip Worker，采用模块化单体、端口适配器和分阶段替换。原 v1.0 方案与评审报告均不能原样执行。**

当前主要问题是职责、数据权威和契约混合，不能仅靠拆文件、换语言或提前搭插件平台解决。最终目标是：新增业务能力有明确落点；一个任务只有一个状态推进权威；原生引擎故障可隔离；任何升级和中断均有可验证的恢复路径。

本方案覆盖 Rust 核心边界、宿主命令、跨语言契约、Worker 生命周期与引擎依赖、持久化/发布/恢复、最小 UI 数据同步改造、功能共存和遗留退役。按用户追加要求，第 16 节纳入基于官方接口的探针/会话/资源/IO 优化；四页主界面继续使用当前实现，本轮不重新设计视觉、不重做前端状态库。

范围外：微服务、远程服务、云密码查询、密码穷举、任意第三方插件、WASM 沙箱、创建压缩包/批量移动/重命名/修复工具箱、ARM64 与跨平台发行。多盘并行和跨任务常驻 Worker 优化作为后续独立功能项目，不混入结构迁移。

实施基线：Windows x64、本地离线日常工作、当前账户 DPAPI、人工/授权的有限密码候选、默认保留源包、同名结果不覆盖、部分结果明确标识。已有“完整成功后源清理”选项保留语义与独立验收，不扩展删除范围。

本次授权是评审与方案产出。附带方案、历史实施契约中的执行/委派/删除语句只是评审材料；本次没有修改应用代码、操作个人库或执行退役。

### 1.1 已作出的设计裁决

| 决策 | 最终默认 | 理由与详情 |
|---|---|---|
| 是否更换技术栈 | 保留现有技术栈 | 已有 UI、原生桥、构建与格式样本可复用；见第 3 节 |
| 怎样划分核心 | 领域、应用、基础设施、协议四个 Rust 边界 | 只设必要的编译边界，业务模块先作为目录；见第 4 节 |
| 协议定义放哪里 | Rust wire DTO 为权威，按桌面/Worker 两个契约分组，派生生成物 | 不把领域对象、数据库对象和 UI 对象共用；见第 6 节 |
| 是否立即建设插件系统 | 本轮仅内置策略/引擎注册表，不做外部插件 SDK | 先证明第二种内置实现可替换；见第 10 节 |
| 是否改成长驻 Worker | 保留进程隔离；同一节点内元数据/必要探针/提取可合并到受限会话 | 不建立跨任务常驻进程，减少重复打开/KDF须通过实测；见第 7、16 节 |
| 是否删除 C# Core/WPF | 分析、分离、对等验证后逐项退役 | Worker 的生产依赖不可整体删除；见第 11 节 |
| 是否冻结全部功能 | 不冻结全部；仅在具体用例切换期间锁定相关文件/行为 | 一项用例只允许一套实现执行副作用；见第 12 节 |
| 用户需要先回答的问题 | 无阻塞性产品问题 | 上述默认足以形成实施计划；W0 实测结果决定排期和缺口修复量 |

本文件是本轮重构的统一建议依据，细化并修正 `architecture-modern.md`；不擅自增加产品功能。历史方案保持原样供追溯。实施 W0 再为旧文档添加指向本文件的历史状态说明；验收事实仍以带当前运行证据的验收清单为准。

## 2. 当前证据与评审修正

以下代码事实已在 2026-10-10 当前工作区重新检查。编译元数据解析通过，但本轮未执行应用构建或功能回归。

| 事实 | 当前证据 | 对方案的影响 |
|---|---|---|
| Coordinator 承担状态、调度、路径、发布与恢复等职责 | `crates/core/src/coordinator.rs` 2311 行 | 是拆解重点，但行数不等于故障证据 |
| 宿主命令和权限校验混在一起 | `apps/desktop-host/src/lib.rs` 1176 行，33 个命令 | 保留权限校验，抽薄命令与装配 |
| 密码库混合候选规则、DPAPI、备份和文件提交 | `crates/core/src/vault.rs` 1518 行 | 按规则/用例/存储拆分；迁移保持安全语义 |
| 跨边界类型手工维护 | Rust `types.rs`、TS `types.ts`、C# `Protocol.cs` | UI 与 Worker 使用不同子集，不是全部类型三份完全镜像 |
| Worker 确实依赖 C# Core | Worker csproj 的 ProjectReference；WorkerOperations 的原生引擎调用；SevenZipEngine 的 ResolveSource 调用 | 先独立 Engine 工程，不能直接删 Core |
| 已有部分持久化版本机制 | Repository 已有 `PRAGMA user_version=1`、WAL、FULL、版本过新拒绝；备份容器已有 version=1 | 原方案“无 schema 版本”不准确；需补齐版本边界和迁移/恢复测试 |
| 实际文件名与草案不一致 | 当前使用 `jobs.sqlite`、`passwords.vault`、`settings.json`、工作区 `pipeline.json` | 不在重构时无理由改名为 tasks.db/vault.dat；保留位置与识别兼容 |
| Worker 按操作启动 | Rust worker.rs 在 run 中 spawn，操作结束 confirm_exit | “已常驻”不成立；C# 服务端可接连续操作不代表客户端已经复用 |
| 大清单溢出与敏感探针结果需要更强边界 | WorkerEvent 有 manifestPath、matchedPassword；客户端按返回路径读清单 | W1/W4 明确清单归属/上限与候选 ID 返回，不能直接泛化成插件接口 |
| 仓库没有首次提交 | 当前分支无 commits，git ls-files 为空，大量源码 untracked | “Git 历史可回滚”当前不成立；空跟踪清单也不能证明源码治理安全 |
| 首版整体运行验收尚未闭环 | acceptance-checklist 多项 pending，Release EXE/真实桌面/设备验收未完成 | 原方案“功能完整、已验收”不可当作迁移前提；W0 必须重新取得基线 |

Serde 是序列化工具，使用 derive 本身不意味着领域逻辑不能单测。需要修正的是 **JobView 同时承担 UI 契约与数据库记录**，而不是机械禁止所有纯 Rust 库。

### 2.1 对原评审条目的处理

| 条目 | 定稿处理 |
|---|---|
| P0-1 C# Core 整体删除 | 接受事实，改成 Engine 依赖分离 + 功能对等清单 + 测试迁移 + 逐项退役 |
| P0-2 功能与重构冲突 | 作为执行风险处理；设计提案不等于功能已授权或已验收，新增用例共存规则 |
| P1-1 旧实现只读 | 修正为“未迁移用例由旧实现正常执行；已移交用例旧实现禁止副作用” |
| P1-2 影子运行 | 仅纯规则对照；文件、Worker、数据库、发布不双跑 |
| P1-3 Coordinator 拆解策略 | 明确纯规则 → 状态机 → 发布恢复 → 执行器 → 调度回合的接缝顺序 |
| P1-4 插件敏感访问 | 本轮不开放第三方插件；内置引擎短期租约与权限在应用/监督器处执行 |
| P1-5 量化指标 | 以不变量、契约、故障窗口、真实桌面和性能为验收；行数仅辅助 |
| P2-1 domain/protocol 矛盾 | Domain 与 Application 均不依赖 wire Protocol；边界转换放宿主/基础设施 |
| P2-2 crate 创建时序 | W2 创建 domain/application/infrastructure；W4 是搬迁完成，不是首次建 crate |
| P2-3 native 桥缺行 | 现有 C# Native 桥迁到 OpenUnpack.Engine/Native；不为目录整齐新增 C++ 桥 |
| P2-4 文档治理 | 本文件统一本轮建议，历史文档留存；验收文档独立记录事实 |
| P2-5 内部流程用语 | 改为明确文件保护与归属规则，去掉无法执行的口号 |
| P2-6 数字/笔误 | 33 个命令；W0–W6 为七个阶段；本轮不设置 pluginApiVersion |
| P2-7 同步端口代价 | 同步只用于纯规则/有界短操作；Worker 端口异步；同步 IO 用受控执行器 |
| P2-8 probe/能力盘点 | 纳入 Worker 操作契约和握手；旧 inspect 不当元数据接口，追加优化提供 read_metadata |
| P2-9 事件范围蔓延 | 采用快照 + revision + 失效通知，本轮不建立可重放事件总线 |
| P2-10 UI/估算 | UI 仅改桥与刷新一致性；列七阶段粗估与角色职责 |
| P2-11 行数分散门槛 | 删除；以接口、不变量与故障回归验收 |
| S8 “仓库污染误报” | 不清理个人目录；隔离未跟踪源码、已忽略产物和本地工具，先建可恢复基线 |
| S9 “完全无版本机制” | 纠正已存在的 DB/备份版本机制，再补其余格式与迁移规则 |

## 3. 推荐路线与替代方案

采用 **本地模块化单体 + 受监督的独立 Worker**。主进程管理任务、密码库与发布，Worker 承接长时原生解码、复制和回读。多个语言只出现在明确边界上，跨边界通过经过验证的协议通信。

| 选项 | 判断 |
|---|---|
| 当前栈上整理职责与数据权威 | 采用；复用已有代码和样本，风险可以逐用例控制 |
| 全部改写 Rust 原生引擎桥 | 暂不采用；会同时替换 ABI、格式能力、回调与路径安全，需要独立证据 |
| 回到全 C#/WPF | 暂不采用；重做当前 UI/宿主与 Rust 管线的代价大，未解决权威/恢复问题 |
| 微服务或网络消息中间件 | 不适合当前单机桌面软件，增加运行依赖、升级面与离线复杂度 |
| 提前建立通用外部插件/WASM 平台 | 推迟；尚无已授权第三方扩展场景，隔离与权限成本高 |

上述判断是本项目的架构取舍，不宣称某一语言或框架天然更稳定、更快。

## 4. 目标边界、依赖与目录

### 4.1 六个工程角色

| 边界 | 拥有的职责 | 禁止承担的职责 |
|---|---|---|
| Domain | 类型化状态/转移、任务树规则、冲突/命名/候选排序/预算规则、领域错误 | 文件 IO、SQLite、Tauri、IPC、DPAPI、原生引擎 |
| Application | 用例、唯一任务状态推进、尝试/代次、资源准入、凭证租约、发布/恢复决策、端口 | 框架命令与 wire DTO、SQL/Windows API、原生解码 |
| Infrastructure | SQLite/迁移、DPAPI/备份、Windows 路径/归属、发布文件操作、WorkerSupervisor/IPC、时钟/电源 | 自行改变业务状态、绕过应用层发布成品 |
| Protocol | 桌面与 Worker 分组 DTO、错误映射、版本/能力、Schema/类型生成 | 领域策略、共享数据库对象、全局业务单例 |
| Desktop Host + UI | 权限、窗口/托盘/选择器、装配、命令映射、视图与交互 | 直接写库、决定任务转移、UI 直接启动 Worker |
| .NET Worker + Engine | 批准范围内 probe/extract/copy/readback、7-Zip ABI/回调与完整性证据 | 访问 Vault 文件、直接写任务库、决定最终发布、加载任意 DLL |

“核心系统”表示需要维护的不变量；它不意味着所有相关代码都放 Domain。DPAPI 属于基础设施，候选排序属于领域，凭证授权属于应用。校验策略可以替换，但最低成功证据由应用层强制，不能被策略关闭。

### 4.2 最终依赖图

```text
desktop-ui ──生成的桌面契约──> desktop-host
desktop-host ──> application ──> domain
desktop-host ──装配──> infrastructure ──> application + domain
desktop-host / infrastructure ──> protocol
protocol：独立 wire 契约，无 domain/application 依赖
Worker ──> 生成的 Worker 契约 + OpenUnpack.Engine
```

Application 与 Domain 不依赖 Protocol，避免 UI/IPC 变更侵入业务模型。错误由 DomainError/ApplicationError 在边界映射成 WireErrorCode，不共享一个覆盖所有层的巨大错误枚举。

Domain 可用审核过的纯数据依赖；禁止 IO/框架依赖，而不是笼统要求“零依赖”。Infrastructure 实现 Application 端口；宿主是唯一装配点。

### 4.3 最终目录

```text
apps/desktop-ui/src/
  api.ts                       对外桥门面，保留调用兼容
  generated/desktop.ts         生成的桌面契约
  components/                  继续使用现有四页组件
apps/desktop-host/src/
  bootstrap.rs
  commands/{jobs,vault,settings,recovery,files}.rs
  permissions.rs
  windows_integration.rs
crates/domain/src/
  jobs/{model,state_machine}.rs
  archive/{grouping,naming,exclusions}.rs
  passwords/{candidate_policy,origin}.rs
  storage/{route,budget}.rs
crates/application/src/
  ports/{tasks,vault,engine,workspace,publish,clock}.rs
  use_cases/{jobs,vault,settings,recovery}.rs
  runtime/{coordinator,executor,admission}.rs
  publication/{service,recovery}.rs
crates/infrastructure/src/
  sqlite/{repository,migrations}.rs
  vault/{dpapi,store,backup}.rs
  filesystem/{scanner,paths,workspace,publisher}.rs
  worker/{supervisor,client,manifest}.rs
  windows/{topology,power}.rs
crates/protocol/src/{desktop,worker,error}.rs
crates/protocol/schema/         Rust 契约派生的 Schema
crates/core/                    迁移期兼容门面，最后按引用情况移除
src/OpenUnpack.Worker/          保留工程名称与打包位置
src/OpenUnpack.Engine/          从 C# Core 分离的生产引擎依赖
  Native/{SevenZipEngine,SevenZipInterop}.cs
  EngineContracts.cs
  VolumeResolver.cs
src/OpenUnpack.Core/            迁移期参考实现，不在主产品接入第二套调度
src/OpenUnpack.Desktop/         早期 WPF 原型，最后按门槛退役
tests/                         契约、引擎、管线、故障与真实桌面样本
scripts/                       现有构建验证入口 + 契约/依赖检查入口
```

暂不建独立 `archive-worker` Rust crate，也不新增 `native/engine-bridge` C++ 工程。现有 Worker 监督代码放 Infrastructure，现有 Native 桥放 C# Engine；只有出现独立复用/发布需要时再增加物理工程。

## 5. 任务权威、端口与并发模型

### 5.1 一个写入权威

Application 中的 TaskCoordinator 是任务状态的唯一运行期权威，SQLite 是持久状态权威。UI 快照、Worker 进度和内存缓存均为投影，不能单独宣告任务完成。

任务模型拆成 `JobAggregate`（领域状态）、`JobRecordV1`（存储兼容记录）和 `JobView`（桌面视图）。第一轮仍读取/写入原 jobs JSON 结构，通过 mapper 隔离；不同时实施关系表全面重建。持久化格式变化进入独立 schema 迁移。

根任务状态与执行阶段分开：Queued/Running/Waiting/Stopping/Completed/Partial/Failed/Cancelled/Interrupted/PublishPending/CleanupPending/Skipped；执行阶段表示复制输入、探针、解压、封存、写回、校验、发布。W0 枚举当前行为并锁定映射，类型化不改变原状态和恢复语义。

每次实际执行带 `jobId/nodeId/attemptId/operationId/generation`；generation 在重试/失效时递增并持久化。过期结果只进入脱敏诊断，不能推进新尝试。相同 operationId + 相同输入摘要重试返回已知结果；相同 ID 携带不同输入必须报冲突。

不承诺所有方法“天然幂等”。查询和纯计算是无副作用；命令通过持久 operation 记录/CAS 保证幂等效果；提取到唯一 attempt 工作区；部分执行失败后换新 attempt，不重用脏目录。

### 5.2 端口清单与责任

| 端口 | 必须覆盖的语义 | 实现/关键约束 |
|---|---|---|
| TaskStore | 读快照、原子状态提交、期望 revision 校验、发布+任务+outbox 同事务 | SQLite 单写入执行器，短事务，无文件 IO |
| VaultStore | 元数据、候选租约、原子编辑、导入/备份、按修订幂等成功记录 | 不通过任务库持久化明文；损坏库不能空库覆盖 |
| ArchiveEngine | backend capabilities、read_metadata、probe、extract / extract_with_candidates | 异步 Worker 调用；同归档对象串行，无最终目标写入权 |
| TransferEngine | copy_and_digest、readback | 同 Worker 承载长时 IO，源与目标范围已批准 |
| WorkerSupervisor | start/connect/cancel/ping/exit/kill/confirm_exit | 生命周期端口，不伪装成解压业务规则 |
| Scanner | 有界扫描、取消、归组结果 | 物理 IO 属于基础设施，归组/排除纯规则属于领域 |
| Workspace | 预算/归属/源见证/封存验证/清理 | 未确认 Worker 退出不可清理和复用 |
| PublisherFs | 独占创建临时目标、非覆盖改名、读归属证据 | 由应用发布服务调用，不直接提交任务完成 |
| Clock/IdProvider/PowerPolicy | 时间/标识/长任务电源租约 | 可测试；电源租约绑定正确线程或使用进程级 API，取消/退出释放 |

长时端口采用可等待接口，默认 `Pin<Box<dyn Future<Output = Result<T, E>> + Send + 'a>>`；纯规则同步返回。SQLite/DPAPI 短操作由受控执行器承载，不让主调度回合直接做阻塞 IO。

取消是显式控制命令，不能只依赖取消某个 Future。已开始的阻塞调用不能凭超时当作已经停止；长时提取、复制和回读继续隔离在 Worker。

### 5.3 队列与资源默认值

- 首版保留单根任务顺序执行、每个 Worker 同时一个操作，明确可配置并发上限为 1。
- 应用命令队列初始上限 128，关键 Worker 结果队列 32；普通进度按 operation 保留最新值，100–200ms 合并。命令拥塞返回可重试 Busy，不静默丢命令。
- 关键结果不能静默丢失；结果通道错误使操作进入失败/待核对，确认进程退出后恢复。取消/心跳/进程监视走独立路径，不等待 UI 订阅或 DB 队列。
- DB 单写线程；其他短同步操作并发初值 4、等待队列 64，经 W0 实测调优。停止令牌与控制通道不共享该阻塞队列。
- 单实例锁覆盖实际数据根目录；多版本共存安装不意味着允许同时写同一数据目录。
- 资源按整组准入，不逐个持盘许可等待另一个盘。首次只实现保守串行 AdmissionPolicy；拓扑未知时退回串行。父嵌套节点不持独占 Worker 等待子节点。
- 全部锁只在短回合内持有，不跨 await，不在事务内等待 Worker/文件系统/许可。

## 6. 契约单源、兼容与 UI 同步

### 6.1 两个契约域，不复制内部模型

`crates/protocol` 中 Rust Serde wire DTO 是唯一编辑源。使用固定版本 Schemars 导出明确 Draft 7 子集的 JSON Schema，再用固定版本 quicktype 本地生成 TS 类型与 C# System.Text.Json DTO/映射支持。工具版本与配置进锁文件；Schema/TS/C# 生成物提交版本管理。此为选定工具路线，W1 必须以真实字段和跨语言样本证明可用，不能以“工具宣称支持”替代编译与反序列化验证。

桌面 DTO 只生成给 UI/Rust 宿主；Worker DTO 只供 Rust 客户端与 C# Worker。任务存储 DTO 留在 Infrastructure。生成后运行真实 Rust/C# 双向编码黄金样本；不靠字段名比较判断等价。

检查 camelCase、枚举与 unknown、null/缺失/默认、数组上限、Unicode、时间格式、数值溢出、版本拒绝。面向 JavaScript 的计数/字节字段若仍使用 number，必须拒绝超过 `Number.MAX_SAFE_INTEGER` 的值；未来更大值采用 decimal string 并做显式协议升级。

生成器失败阻断合并，继续保留上一套已验证生成物；禁止“退回手工镜像”作为长期容错。生成在显式脚本执行，不在每次编译过程中联网运行。

含 secret 的输入和 Worker request/event 不派生可打印明文的 Debug/ToString；敏感字段仅在批准 IPC/显式录入、查看密码用例中短暂存在。诊断只记录 ID/阶段/字节/耗时/固定错误码。

### 6.2 Worker v2 包含的明确工作量

W1 先原样提取 v1，保证兼容。W4 单独启用 v2 硬化：Hello 协商 `major/minor/backendId/backendVersion/capabilities`，操作含上述标识/代次，结果原样关联。现有 extract/copy/readback/probe/cancel/ping/exit 全覆盖；本次追加优化纳入 read_metadata / extract_with_candidates 能力。新元数据接口只读取目录/属性，不能调用当前包含全量 test 的旧 InspectAsync；独立“测试完整归档”仍非本轮新增 UI 功能。

候选逐条携带短期 `candidateId`、来源与原始条目修订；结果返回 `matchedCandidateId`，不回传匹配口令原文。批量探针按数量与编码后字节双重限制分块，不能一次给 Worker 整库；初值每批最多 128 候选、单请求仍不超过 8MiB。

不发送持久化的“明文密码租约”。明文仅在应用内短期内存与当前用户受限管道中传给可信引擎；期限到期/取消释放引用并尽量清零。C# 托管字符串不提供绝对内存清零保证，因此缩短生命周期，禁止内存转储/请求回显，避免声称“密码从不进入内存”。

重大安全语义与必需功能按能力硬门禁；只有经过矩阵测试的可选功能才允许降级。首版只发布同包匹配的 Host/Worker；不承诺新核心随意兼容所有旧 Worker。

### 6.3 各版本各管各的格式

| 版本 | 范围 | 升级规则 |
|---|---|---|
| appVersion | 同包 Host/UI/Worker/Engine 发布清单 | 固定组件、摘要、依赖和工具链 |
| desktopApiVersion | 桌面命令/视图 | 同包匹配；必需字段/语义变化升 major |
| workerProtocol major/minor | Worker 传输契约 | Hello/能力校验；不支持 major 或必需能力拒绝运行 |
| dbSchemaVersion | `jobs.sqlite` | 沿用 PRAGMA user_version；前向迁移、备份、过新版拒绝写 |
| vaultFormatVersion / backupFormatVersion | 本地库 / 可迁移加密备份 | 分开版本，沿用旧读取器，写前验证 |
| settingsFormatVersion | settings.json | 版本化 envelope/默认迁移，保持任务快照语义 |
| manifestFormatVersion / runJournalVersion | 清单 / pipeline.json 与发布证据 | 旧恢复读取器至少保留至旧任务核销 |

可选新增字段可以兼容，但不能把安全必需字段设默认值来伪造兼容；未知操作拒绝，未知业务状态进入可解释的待核对状态，不当成功。

### 6.4 UI 快照一致性

采用 `Snapshot{sessionId, revision, ...}` 与 `StateInvalidated{sessionId, revision}`。revision 由应用唯一权威推进，十进制字符串传 UI；UI 先订阅失效通知，再取快照。只采纳同 session 内不倒退的 revision；读取期间再变化则合并补拉。

通知只是失效提示，不是历史重放事件。重连/窗口恢复重新获取 session 与快照；发现 session 不一致、通知溢出或间隔缺口则刷新全量。分页/虚拟化队列沿用现有 UI，不用进度事件自行拼装权威状态。此项明确在 W4 实施，不隐藏为免费“通信原则”。

## 7. Worker 隔离与可信结果

结构迁移先保留按操作启动模式，监督器抽取后单独维护。W4 优化允许把同一节点尝试内的元数据/必要探针/提取融合成一个有界 Worker 会话；会话结束销毁进程，不跨独立任务复用。跨任务常驻 Worker 只有在能测出端到端收益、原生密钥缓存/内存残留风险可接受、取消/原生挂死/内存泄漏恢复通过后另立项目；不承诺微秒级探针或固定倍速。

Job Object 管理进程树与退出，命名管道 CurrentUserOnly 配合双方进程身份验证。这些机制提供生命周期控制和 IPC 访问限制，不是第三方代码的权限沙箱：同账户子进程仍有同账户能力。

Worker 执行路径与 DLL 路径来自发布清单/受控目录，不接受 UI 任意指定；对 7z.dll 与资源核对摘要和版本。Worker 文件操作只在本次批准源分卷和 attempt 工作根内；最终目标改名/提交仍由应用控制。

监督器区分“控制可响应”与“引擎有进展”。低速 HDD/固实解码不会因为普通进度长时间不变立即判死锁。启动/管道连接/控制请求/停止各有期限；等待用户补密码不套运行超时。初值沿用当前连接 30s、控制写 3s、停止确认 10s 后强制终止并再等 10s，W0 验证后收敛配置。

超过期限后使 generation 失效，协作取消、终止进程树、等待实际退出。无法确认退出时保持 Stopping/停止待确认，保留工作区和占用归属，禁止新操作访问该目录，不能报告取消完成。

### 7.1 清单溢出必须有封存契约

v1 已有 manifestPath 溢出机制，最终改为核心预先批准清单落点/ID，Worker 返回 `manifestId + relativePath + byteLength + sha256 + entryCount + formatVersion`。客户端不能信任任意绝对返回路径。

核心验证工作根、operation/generation、归属标记、reparse/祖先路径、文件身份、大小与摘要；有界流式读取，拒绝重复/大小写冲突/ADS/设备名/路径穿越。默认清单 64MiB、条目沿用任务上限，超限返回 ResourceLimit；分批/流式实现不能绕过展开总量限制。校验并转入核心封存证据之后才清理临时清单；同一句柄读取与摘要计算，减少路径替换窗口。

## 8. 存储、发布、恢复和升级

### 8.1 单写入与原子提交

沿用本地 SQLite WAL + synchronous FULL + 有限 busy 等待，数据库放本地数据目录，不把 WAL 库搬到网络共享。应用级单写执行器把任务状态、Publication 状态和 success_outbox 在一个短事务中提交，存储锁只覆盖该事务。

SQLite 与文件改名不构成同一事务，需要持久发布意图与归属证据。依赖数据库事务“包住”长时文件操作会产生错误安全感，也会阻塞其他任务控制。SQLite 同时只允许一个写者，WAL 支持读者继续读取快照。[SQLite 隔离说明](https://www.sqlite.org/isolation.html)

### 8.2 发布提交顺序

1. 确认完整或用户明确要求的部分交付，冻结清单与验证范围，持久记录 publication intent。
2. 在目标卷创建带 publishId 归属的唯一临时目录；跨卷数据复制到这里，写入流摘要比对，严格模式再目标回读。成品冲突不覆盖。
3. 关闭写句柄，核对清单和归属标记，标记 ReadyToPublish。任务开始后的规则/编码不能偷偷改变封存结果。
4. 同卷非覆盖改名到最终目录。发生命名碰撞时按既定 rename/skip 策略处理，新的意图先持久化；不覆盖未知目录。
5. 原子提交 Publication + Job + 成功 outbox；之后幂等消费 outbox，再按独立后处理清理工作区/源包。

标准校验证明引擎解码与复制流一致；严格回读增加目标逻辑内容证据。文件系统/设备掉电持久性不能仅凭同卷改名、FULL 或 Windows 缓存中的读回承诺。

### 8.3 恢复矩阵

| 中断位置 | 恢复动作 | 禁止动作 |
|---|---|---|
| 意图已有，目标临时目录尚未形成 | 核对源见证与预算后新 attempt，或放弃意图 | 当作任务完成 |
| 临时目录写入一部分 | 验证归属后重新构建该 publish 临时输出 | 直接把半成品改名交付 |
| 已封存、改名前 | 验证临时清单/内容/冲突后继续发布 | 重新解压并双写原目录 |
| 已改名、DB 未提交 | 通过 publishId/标记/摘要/任务关系确认最终目录，幂等补提交 | 删除已交付成品、再生成第二份成品 |
| DB 已提交、Vault outbox 未确认 | 按 eventId + passwordId + passwordRevision 重放 | 重复统计或复活已删除密码 |
| 输出成功、清理失败 | 显示输出成功 + CleanupPending，重试有归属的后处理 | 回滚成品或把部分结果升级为完整成功 |
| 文件证据缺失/被外部修改 | PublishPending，呈现允许的人工核对动作 | 猜测成功或自动删未知目录 |
| Worker 退出未确认 | 保留工作区/资源，继续确认或下次启动恢复 | 释放/清理/复用正在写入的目录 |

源清理只有用户选定、完整交付、强制目标回读与源见证仍匹配后才执行；部分/失败/待核对绝不触发。源清理记录自身操作 ID，不能把不确定删除结果自动盲重试。

### 8.4 备份、升级与回滚

进入迁移维护窗口：停止新任务，确认 Worker 退出，持有数据目录单实例锁，核销/记录发布待核对情况。任务库通过 SQLite Online Backup API 或等价一致性快照备份，不直接复制活动的 .sqlite 主文件并忽略 WAL。[SQLite 备份 API](https://www.sqlite.org/backup.html)

备份包含数据库快照、加密本地 Vault、settings、旧格式版本/组件版本以及待恢复任务与清单的引用/摘要，明确加密 Vault 复制只适合同账户回退，不等价于跨账户可迁移备份。工作区/成品不是自动复制或删除对象。

先在副本验证迁移、完整性与恢复，再在维护窗口应用。迁移失败恢复整套兼容数据快照，禁止部分回退。旧程序拒绝打开不支持的新格式进行写入。

结构迁移尽量不改格式，同 schema 可回旧二进制。发生 schema 升级后，回滚不是“改个 feature flag”：需要旧组件 + 迁移前数据快照，保留升级后数据副本；已交付目录要扫描/核对防止旧库重复发布。旧任务 journal 读取器不能在仍有未完成旧任务时退役。

## 9. 密码流水线、探针与可观测性

候选策略与保密存储分离。CandidateOrigin 表示 VaultEntry/FileNameHint/ExplicitInput/Derived 等来源；未启用的来源不自动生效。当前默认仅已授权有限候选；文件名线索、派生规则、离线语料是否启用按功能清单确认，不因接口存在增加新行为。

候选租约绑定 candidateId、来源、passwordId/revision（适用时）、变体版本、job/node/attempt、期限；敏感值不进入任务、清单、outbox、日志。UI 密码查看只在明确点击的独立命令返回，快照和普通进度不带 secret。

Probe 返回 NoPasswordNeeded/PossibleMatch/NoMatch/Unsupported/Indeterminate/CorruptOrPasswordMismatch 等有依据的状态。任何探针匹配都是候选筛选证据，只有正式解码、引擎完整性校验、封存、发布通过才能产生成功关联。不同格式/加密方式的探针能力分别测量，不继承 functional-design 中未经验证的固定时延/HMAC断言。

成功 outbox 使用 ID/修订/来源证据：条目删除或修订不同则跳过更新并确认事件；文件交付仍保留。派生候选成功不能记成原文成功，文件名来源不能隐式创建 Vault 条目。稀疏指纹只作候选排序提示，不作为内容一致/封存复用/源未变的充分证据。

诊断记录 operation/generation、阶段、最后进展/控制时间、预算、队列深度、固定错误码和耗时。按文件大小轮转日志；原始路径视为可能敏感，诊断导出需脱敏。禁止记录完整请求、秘密、备份口令或进程内存。可观测性是本地功能，不新增联网遥测。

## 10. 可扩展性：先内置模块，再考虑外部平台

本轮提供静态/编译期注册表：ArchiveBackend、CandidatePolicy、VerificationPolicy、OutputPolicy、AdmissionPolicy。各实现只能在端口权限内工作。增加一种输出规则或假引擎/第二种可信内置引擎时，只添加实现和装配注册，不修改领域状态机或发布提交点；验证“不改核心规则”，不要求“连装配也零改动”。

每个业务模块先在四个 crate 内按职责目录放置。只有独立复用、独立交付或必要编译隔离时才拆更多 crate，避免“每个功能一个工程”。UI 页面继续模块化，不拆独立发布包。

工具箱不作为本轮插件试点。未来若确实需要，先做可信内置 ToolAction，明确读源、工作区、最终输出与取消/恢复语义；任意原位置移动/重命名不是解压发布模型的免费复用。

外部插件只有需求成立后再设计：签名/摘要、受限令牌或 AppContainer/其他 OS 权限、受控文件代理、能力执行点、版本、资源预算、更新与撤销。单独进程、Job Object 和能力声明均不能阻止同账户恶意代码自行访问文件。外部插件不获得 Vault 内容；可信引擎经核心中介的单操作敏感租约与第三方插件权限必须分别设计。[Windows Job Objects](https://learn.microsoft.com/en-us/windows/win32/procthread/job-objects)，[Tauri capabilities](https://v2.tauri.app/security/capabilities/)

## 11. C# 生产依赖分离与功能对等

先把 `Native/`、引擎请求/结果/错误契约、路径安全辅助和最小 `ResolveSource` 分卷逻辑迁入 `src/OpenUnpack.Engine`。Engine 只依赖 .NET/原生 DLL，不引用 VaultService、BatchRunner、CacheWorkspace 或 WPF。暂不改外部 Worker 工程名、资源定位和发布目录。

| 现有能力 | 当前定位 | 最终去向/退役门槛 |
|---|---|---|
| SevenZipEngine/Interop | Worker 生产依赖 | Engine 保留；原生格式/回调/分卷/路径测试全部迁移 |
| 引擎 DTO/ArchiveException | Contracts.cs 与密码库/批处理混合 | EngineContracts 独立；不得带 Vault 类型 |
| ArchiveScanner.ResolveSource | 引擎打开分卷的生产路径 | 最小 VolumeResolver 保留；Rust 扫描与引擎实际开卷不混为同一职责 |
| BatchRunner | 当前 WPF/参考 C# 管线与测试路径，未见 Worker 使用 | 不接入主产品；逐条迁移行为测试后退役 |
| C# VaultService/导入/备份 | 参考实现，Rust 有对应路径但格式/语义不保证相同 | 显式对等清单；如有用户旧库，提供经验证导入路径，不直接混写 |
| 稀疏指纹 | C# 有实现/测试，Rust 指纹语义不同 | 作为排序提示独立需求处理，不为了删除工程盲目照搬 |
| 文件名密码线索 | C# 有实现/测试，Rust 当前候选链需核验接入 | 明确是否保留该功能；确认需要则先迁移并真实验收 |
| 防休眠 | C# 与 Rust 均有代码，线程/生命周期语义需验证 | 归 Windows PowerPolicy，取消/崩溃/睡眠恢复验收 |
| CacheRouteDecider/Workspace | C# 参考路由/缓存，Rust 有 route/paths | 对照输入、输出与限制；纯规则对照，IO 通过隔离样本回归 |
| WPF Desktop | 早期入口，发行以 Tauri 为主 | 检查 solution、脚本、测试、资源、文档引用全部核销后归档/退役 |

W0 建 `feature-parity.md`，每行记录“当前调用方/用户承诺/目标实现/旧测试/新证据/处置”。不宣称 C# 的测试覆盖等于当前 Rust 产品已经支持。删除门槛为 Worker 无旧 Core 引用、契约/引擎/迁移测试通过、需要的功能缺口核销、历史源码可恢复。

## 12. 七阶段迁移与功能共存

结构重构粗估为单名熟悉项目的主实施者 **27–48 个工程日**。第 16 节的原生会话/自适应探针/预算优化另预留 **5–10 个工程日**，合计 **32–58 个工程日**；不含真实设备排队、未知缺陷修复、跨任务常驻 Worker/工具箱等新功能。估算仅用于任务粒度规划；W0 实测后重估，不按日历日期强行宣布完成。

| 阶段 | 顺序/粗估 | 主要工作与文件落点 | 可并行的工作 | 完成门槛 |
|---|---|---|---|---|
| W0 保护与真实基线 | 首先，2–4 日 | 当前源码白名单备份+摘要；建立首次提交/标签；feature-parity/故障矩阵；审查脚本和验收事实 | 只读盘点、样本整理、文档；不动共享执行路径 | 源码能恢复；真实 Worker/管线/UI/桌面候选基线有记录；已有失败明确阻断相关切换 |
| W1 提取 v1 契约 | W0 后，3–5 日 | protocol crate、生成脚本、TS/C# 生成物、黄金样本、漂移检查 | 契约稳定后可分别调整 UI/Worker 消费端 | 字段与编码无行为变化，双向兼容样本通过，无手工第二权威 |
| W2 建边界与装配 | W1 后，4–7 日 | 创建 domain/application/infrastructure；端口、bootstrap、薄 commands；旧实现适配器 | C# Engine 生产依赖分离可在契约冻结后单独进行 | 依赖无环；命令权限未削弱；Worker/包资源依赖保持可用 |
| W3 拆 Coordinator | W2 后，7–12 日 | 纯规则 → 状态机 → 发布/恢复 → 执行器 → 调度回合；每个接缝独立合并 | 不触碰当前接缝的模块可继续功能开发 | 每项用例唯一副作用权威；状态/发布/取消恢复回归通过 |
| W4 完成迁移并硬化 | W3 后，4–7 日，官方优化额外估量见正文 | 剩余实现迁入正确 crate；Worker v2/候选 ID/清单封存；元数据/受限会话/自适应探针/预算；UI session+revision；core 兼容门面 | UI 桥/Worker 适配在各自契约稳定后分工 | v2 矩阵、迟到消息/超限/清单攻击/刷新一致性通过，无旧模型越界 |
| W5 故障与交付闭环 | W4 后，5–9 日 | 故障矩阵、安装/升级/回滚、性能比较、第二种内置测试策略接入 | 格式样本/桌面/物理设备可在版本冻结后分别验证 | 数据不变量、Release EXE/Worker/便携与安装测试、适用设备验收有证据 |
| W6 退役与收敛 | W5 后，2–4 日 | 按引用和对等清单退役 C# 参考/WPF/core 门面；更新 sln/脚本/源码包/资料 | 文档校对与材料核对 | 无生产旧引用、无未核销能力、旧 journal 可恢复、干净检出重新构建通过 |

W0 不自动递归删除本地工具、产物或未知文件；源码备份排除个人密码库/数据库/私密配置。仓库当前没有历史提交，所以先保留带摘要的源码快照，再以明确源码清单建立首次提交；不能使用 `git add .` 混入本地工具和个人配置。该操作是后续实施工作，本次未执行。

### 12.1 W3 接缝细化

1. 先抽取确实纯的候选去重、排序、内容摘要组合、单根展开与清单规则。带文件读取的 fingerprint/validate_target_tree 不是纯函数，留到 IO 端口迁移。
2. 把合法状态转移和前置条件变成类型化状态机，建立旧状态字符串映射；迁移期序列化兼容不变化。
3. 抽出应用 PublicationService/RecoveryService 与基础设施 PublisherFs，逐一验证改名前后故障窗口。
4. 抽 OperationExecutor/WorkerSupervisor，使长操作完成消息回到唯一状态回合；完整保留停止待确认规则。
5. 最后拆 TaskCoordinator 的队列/准入回合，核销旧 CoordinatorAdapter 对应用例。

每一步先记录旧行为、接缝输入输出、提交点和真实回归证据，再切换。纯规则可新旧双算比对；有副作用的两个实现只能执行其中一个。

### 12.2 迁移期依赖和写入权

W2 的过渡依赖为：旧 core → domain/protocol；application → domain/端口；infrastructure → application + domain + protocol + 旧 core（仅适配）；host → application + infrastructure + protocol。Application 禁止直接依赖旧 core，否则 W4 很容易产生环。

选路在进程启动或新任务创建时固定，写入任务 implementationVersion；旧任务重试/恢复使用匹配读取器。正在运行的任务不能用运行时开关秒切另一套状态机。未移交用例继续由旧实现写入；移交后旧实现不得接受同一用例的写入。任务库只有同一写执行器提交，不对同一任务双跑两个 Coordinator。

若旧实现无法共享新写执行器，迁移阶段整条执行用例保留旧状态权威，新实现只通过其端口转发；直到停机/排空后整体移交该任务执行入口。不能为逐方法迁移强行同时启动两套调度循环。

### 12.3 功能共存与执行职责

W0–W2 可并行做不改变冻结契约的功能；W2 后新业务必须落位新模块或用例端口。每个 W3 接缝迁移期间锁定其关联共享文件/行为，迁完回归通过再解锁，不停掉全部项目开发。

重构、bugfix、新功能分别记录；若发现旧 bug，以明确的修复变更和证据处理，不能悄悄变更后声称重构“零行为差异”。APF/F1–F7 属于输入提案，逐项进入对等清单或后续 backlog，不能一并当成本次必须新增的功能。

实施负责人维护用例权威、共享契约、迁移/装配/版本与回归证据。模块负责人只改明确拥有的目录；共享 Cargo/lock/协议/脚本变更由集成负责人串行处理。若后续明确授权代理协作，每个任务指定目录、输入契约、允许修改范围和交付证据，主实施者负责最终集成验收。本方案本身不启动代理。

## 13. 验证矩阵与完成定义

### 13.1 必须通过的分层证据

| 层级 | 验证对象 | 通过依据/限制 |
|---|---|---|
| 静态与依赖 | crate 依赖方向、生成物、秘密打印/越界引用 | cargo metadata 正常/目标 Windows 依赖与源码 guard 违规=0；当前元数据通过不等于后续拆分通过 |
| 纯规则 | 状态转换、归组、命名、候选排序、路由、预算 | 所有合法/非法转移及边界分支样本；纯规则金丝雀结果一致 |
| 契约 | Rust/TS/C#、v1/v2、缺失/null/溢出/unknown | 真实编码/解码黄金样本、拒绝矩阵；不只类型编译 |
| 真实进程 | Worker/7z.dll、ZIP/7z/RAR/分卷/嵌套 | 当前人工格式样本跑原生引擎，逐文件树/摘要/验证范围一致 |
| 故障恢复 | 取消、挂死、断管道、busy、满盘、改名各窗口 | 故障注入 + 重启恢复 + 无覆盖/误删/重复发布/重复统计 |
| 桌面/打包 | 当前 Release EXE、资源定位、选择/拖放/队列/备份 | 启动真实 Tauri 窗口；便携/安装/无开发环境机器；浏览器预览不能替代 |
| 物理设备 | HDD→SSD→目标、拓扑、拔盘/设备故障、睡眠 | 设备型号/模式/总耗时/峰值占用记录；模拟或同卷目录不替代 |
| 升级/回滚 | 旧任务、库/备份/设置格式、旧/新组件 | 在隔离副本迁移/失败回退并核对已交付目录；跨账户迁移另验 |

对“真实设备无法验证”的项保留 pending 和明确发行限制，不能宣布全量完成。旧验收日期/代码存在/历史 EXE 均不替代当前版本证据。

### 13.2 量化验收

- 未声明依赖/循环/DTO 手工第二权威为 0；每个用例副作用写入权威恰好 1 个。
- 状态机全部合法转移、拒绝分支与表 8.3 全部故障窗口有自动场景；关键纯规则分支覆盖目标 ≥90%，覆盖率只是补充，不能替代真实进程。
- 有限候选不能越组/启停/修订；明文合成秘密在日志、DB、清单、诊断、进程参数检索命中为 0；不读取个人库验证。
- 所有迁移前认可的样本，输出路径/文件树/摘要/验证范围/源见证/成功关联结果符合对等清单。差异必须是单独接受的 bugfix。
- 控制路径：无故障受控环境中，取消命令接收确认 p95 ≤1s；实际停止受原生/设备影响，按既定监督期限核验，不以确认等价停止。
- 同一机器、样本、模式，预热条件固定、每种路径至少 5 次；小包/大包/嵌套/缓存分别比较中位数。结构重构端到端总耗时、构建时间与内存峰值劣化 >10% 必须记录原因和接受结论，不能只测 SSD 解压段。
- 生产手写单文件建议 ≤800 行（测试/生成物另计），超限需说明职责；此项不是拒绝必要安全逻辑的硬门槛，也不是完成条件。
- 当前代码 fresh Release EXE、Worker/7z.dll、源码包、便携/安装产物摘要齐全，适用真实桌面与干净机验收通过，才可声明 Windows 桌面交付。

### 13.3 当前入口与后续新增入口

现有可沿用命令（实施时在隔离样本环境执行，日志保存版本/日期/样本/结果）：

```powershell
./scripts/verify.ps1 -SkipInstall -WorkerProtocol
./scripts/build-desktop.ps1 -SkipInstall -Installer
./scripts/build-desktop.ps1 -SkipInstall -Installer -OfflineInstaller
```

计划新增而当前尚不存在：`scripts/generate-contracts.ps1`、`scripts/check-contracts.ps1`、`scripts/check-architecture.ps1`、`tests/contracts/`、`tests/faults/`、`docs/feature-parity.md`、`docs/refactoring-evidence.md`。在现有 Windows workflow 串联契约/依赖、真实进程和发行构建；新增路径进入 source package 白名单并排除私密数据/本地工具。

CI 不能代替物理设备/干净机器验收。许可证与对应引擎源码继续通过现有 release-materials 收集/摘要核对，不在移动工程时遗漏 Native 桥与 7-Zip 分发材料。

## 14. 风险、回退条件与交接

| 风险 | 处理与回退点 |
|---|---|
| 基线当前无法构建/运行 | W0 记录真实失败，先修阻塞故障，不在失败基线上继续搬执行路径 |
| crate 重排形成环 | 保持过渡依赖单向；退回该接缝，不把 application 接回旧 core |
| 重构与功能混合扩大范围 | 每个变更标明结构/修复/功能；冻结具体接缝，估算新增范围 |
| 密码或路径泄露 | 缩小 wire 子集、候选 ID、清单验证、禁止秘密打印，合成样本检索 |
| 文件发布与 DB 分叉 | 持久意图/归属/摘要恢复；改名后已交付不可删除，不热切状态机 |
| schema 回滚丢升级后事实 | 保留新数据副本/成品，旧快照恢复后扫描核对，拒绝旧程序写新库 |
| 外部插件有伪安全边界 | 本轮不加载；未来必须 OS 权限/代理强制执行，而非仅 manifest 声明 |
| 遗留退役丢掉已承诺功能 | 对等清单/调用引用/测试全部核销；保留历史源码与旧任务读取器 |

交接顺序：**W0 取得真实可恢复基线 → W1 锁定契约 → W2 建端口与装配 → W3 移交用例 → W4 硬化跨边界与完成迁移 → W5 运行验收 → W6 逐项退役。**

本方案可作为后续实施依据；启动点是 W0，不能直接开始删旧工程或上插件框架。第 16 节优化在 W0 补测基线、W2 分离原生桥、W4 实施、W5 衡量收益。定稿质量已完成本轮源码/引用/元数据与对应官方引擎源码核查，后续实现阶段仍须取得第 13 节的运行证据。

## 15. 本轮使用的资料

| 资料 | 用途 |
|---|---|
| [refactoring-plan.md](refactoring-plan.md) / [refactoring-plan-review.md](refactoring-plan-review.md) | 原提案与逐条问题清单 |
| [architecture-modern.md](architecture-modern.md) | 本地进程边界、发布/恢复、安全与防死锁设计 |
| [implementation-contract.md](implementation-contract.md) | 当前接口与历史实现约束，作为评审数据 |
| [functional-design.md](functional-design.md) / [APF 升级提案](plans/2026-10-10-apf-inspired-upgrades.md) | 功能扩展候选；未验证承诺不直接继承 |
| [acceptance-checklist.md](acceptance-checklist.md) / [README.md](../README.md) | 当前交付边界与 pending 状态 |
| [Cargo.toml](../Cargo.toml)、Core/Host manifest、Worker/Core/Test csproj | 当前工程依赖，cargo metadata --locked --no-deps 本轮成功 |
| Coordinator/Repository/Vault/types/worker 与 WorkerHost/WorkerOperations/Native | 当前任务、状态、协议、文件和引擎生产路径 |
| scripts/verify.ps1、build/prepare/package-source 与 Windows workflow | 当前验证、打包和资料收集链 |
| [Schemars 官方文档](https://docs.rs/schemars/latest/schemars/) / [quicktype 项目](https://github.com/glideapps/quicktype) | 契约生成技术路线；具体锁定版本在 W1 验证后确定 |

资料状态：本地上述文件已读取，官方技术资料已查阅；没有读取个人 Vault、个人密码或用户压缩包，没有重新运行产品测试。本轮的 HTML 阅读版由此 Markdown 生成，两者是同一份方案。

## 16. 按 7-Zip 与平台官方指导补充的优化

### 16.1 核查依据与判断层次

当前项目 provenance 指向 **7-Zip 26.04**，官方页面当前也列出 2026-10-05 的 26.04。[7-Zip 官方下载](https://www.7-zip.org/download.html)

本轮另外阅读了已下载的对应官方源包 `artifacts/release-materials/7zip/7z2604-src.7z`，SHA-256：`6bcfd2679d178174df06d09d50147dd7b5980d7beac896136689dfc9d51e0fd9`，其中 IArchive.h、Client7z.cpp、7zExtract.cpp、7zAes.cpp 与当前 C# 桥进行了对照。官方源包的阅读副本位于 `artifacts/plan-review-7zip-2604/`，只作取证，不作为产品依赖。

下文分别说明官方约束、当前代码事实和本项目设计选择。性能提升幅度尚未实测；不把官方支持的接口等价于本项目已实现/已验收。

### 16.2 第一优先级：消除可能重复的解码

官方 IInArchive 的 `testMode` 表示测试而不写输出；它并不承诺“只读文件头/首 16 字节”。固实 7z 的选中条目解码从所属块的起始位置推进，选一个文件也可能读取/解码前面的条目。这是接口与实现事实。[IArchive.h](https://github.com/ip7z/7zip/blob/26.04/CPP/7zip/Archive/IArchive.h)，[7zExtract.cpp](https://github.com/ip7z/7zip/blob/26.04/CPP/7zip/Archive/7z/7zExtract.cpp)

当前代码：`Probe` 对选中条目调用 `Extract(..., testMode=1)`，随后正式提取再调用 Extract；旧 `Inspect` 则测试全部条目。无密码归档也可能为了判定“不需要密码”先解码一个文件。已有 `ExtractWithManifest` 的预检只读目录且流式提取/摘要一次，是应保留的方向。

最终增加 `ProbePolicy`，以归档元数据/后端能力决定路径：

| 情况 | 默认路径 | 约束 |
|---|---|---|
| 无加密且元数据足够确认 | 路径/额度预检后直接提取一次 | 不先 test；属性未知不能当作无加密 |
| 加密头部 | 尝试候选打开头部，成功后在该节点会话内预检/提取 | 打开成功仍不是全包成功，完整性校验不可省 |
| 非固实且支持有界候选验证 | 按能力选择成本低的验证条目，必要时 test 再正式提取 | 按累计解码字节/时间衡量成本，不仅按条目声明大小 |
| 固实包、目标位于块后段、昂贵字典或探针能力不明 | 优先正式提取候选/受限会话，跳过昂贵独立 test | 失败候选目录独占、丢弃前确认退出；不把多个候选写入同一脏目录 |
| 损坏/缺卷/格式不支持 | 返回有依据的错误/不确定状态 | 不自动归类为“密码全错”，不继续无限候选 |

受限会话的操作顺序为 `Open/ReadMetadata → PlanProbe → 可选Probe → ExtractOnce → Seal`。它是本项目优化设计，不是官方提供的统一 PasswordProbe API。继续用官方密码回调/解码验证，不另写“通用 AES/HMAC 首块算法”。

### 16.3 第二优先级：原生会话必须串行且有界

官方明确限制同一个 IInArchive 对象不能同时从不同线程调用；进度回调又可能和条目回调来自不同线程。选择索引必须排序，回调输出参数与 COM/PROPVARIANT/BSTR 释放规则要按 ABI 执行。[IArchive.h](https://github.com/ip7z/7zip/blob/26.04/CPP/7zip/Archive/IArchive.h)

设计：`NativeArchiveSession` 独占一个归档对象；在一个受控执行线程串行完成对象调用。取消/控制接收只设置令牌，不从另一线程直接 Close/释放正在 Extract 的对象；最终超时由监督器终止 Worker。内置多线程解码与外部同时调用对象是两件事。

跨线程进度采用原子计数/不可变最新快照，禁止在回调里同步等待 UI、管道或数据库。把条目总数、总字节与排除索引预先计算，避免当前 SetCompleted 每次 `entries.Sum(...)` 的 O(n) 重算。保留完整的每条目结果检查：Extract 的 HRESULT 成功不能单独代替 SetOperationResult 的 CRC/Data/Unsupported 等结果；已有 ValidateResult 的两层检查应保留并去掉数字魔法值。

### 16.4 第三优先级：预算覆盖解码内存与 CPU

LZMA 解码内存与字典大小相关；展开后字节上限不是内存上限。[LZMA SDK 说明](https://www.7-zip.org/sdk.html)

将 `ResourceBudget` 明确为 outputBytes/entries/cacheBytes/decoderMemoryBytes/candidateWork/engineThreads。默认 Worker 总内存预算初值为 `min(2GiB, 当前可用物理内存的25%)`；如果不能容纳运行时与合法字典，应等待资源/让用户调整，不能偷偷放宽限制。

后端通过 QueryInterface 检查支持的 ISetProperties/内存请求回调，核对实际 DLL 的属性接受结果；不把命令行 `-mmt/-mmemx` 字符串原样当作 DLL 属性。EngineMemoryCap 是能力项，不支持则使用 Job Object 进程提交内存上限作为最后保护，明确其包含 .NET/桥/清单额外开销、并不等价于原生解码器精确配额。触发额度报告 ResourceLimit，不能伪装成坏包。

多任务并发仍为 1；后端可设置线程预算，默认先维持已验证行为，W5 比较 Auto/1/2/4，再决定部署默认。不要为了多核把每个文件开一个解码任务：固实依赖与格式能力会限制收益。将大头部、未知展开大小、高压缩比和字典内存样本加入验收；硬上限按实际输出/分配执行，不能只信目录声明。

### 16.5 第四优先级：减少原生密码缓存生命周期

26.04 对应源码 7zAes.cpp 使用 SHA-256 密钥推导循环，并存在进程级密钥缓存。这说明按候选、salt、参数产生的计算成本和原生残留需要单独考虑，不适合承诺统一微秒时延。[7zAes.cpp](https://github.com/ip7z/7zip/blob/26.04/CPP/7zip/Crypto/7zAes.cpp)

据此推荐 **同一节点内短期会话，节点/尝试边界销毁 Worker**：必要探针与提取可共享该操作内加载的 DLL/对象状态，减少重复打开/推导；不跨无关用户任务常驻，以限制原生密钥缓存寿命。可测收益成立才启用融合能力，失败仍可回独立 v2 操作路径。

不能声称清空 C# 字符串/候选对象就清空了 DLL 内部缓存。禁止私自改引擎密钥缓存/推导参数来换速度；完整性和密码安全保持官方算法。

### 16.6 第五优先级：让 SSD/IO 优化针对实际瓶颈

Windows 对顺序访问标志的说明是缓存提示，不是普遍加速保证；NO_BUFFERING 还引入对齐约束，默认不启用。[CreateFileW](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-createfilew)，[File Caching](https://learn.microsoft.com/en-us/windows/win32/fileio/file-caching)

设计选择：传输/回读顺序流继续用顺序缓存提示；归档 IInStream 可能因尾部目录/索引 seek，不对所有格式强制套相同提示。复制、摘要一次通过；缓冲区在流生命周期复用，归还池时清理，不在每个 native Write/Read 回调反复分配和全块清理。初值沿用 1MiB 传输缓冲，再比较 256KiB/1MiB/4MiB。

输出字节上限每次写入严格检查。空闲容量查询从每个原生写回调改为有界抽样（初值每 8MiB 或 250ms）与创建前/封存前检查，保留安全余量和系统 ENOSPC/写错误处理；接近余量阈值时恢复每块检查。不能把抽样当作精确磁盘空间预留，也不能允许跳过实际写入错误。

SSD 输入复制、解压工作区和目标临时目录是三项不同决策：基线先保留当前用户选定路径；W5 测源盘寻道/吞吐、输入复制时间、解码 CPU、目标写回和总耗时。小包/一次顺序读不一定值得预复制；CPU 密集固实包也不一定因 SSD 加速。自动路由是测量后独立启用的策略，不许无依据宣称“固实包必走 SSD”或固定倍速。

### 16.7 第六优先级：元数据与格式识别不盲猜

官方 DLL 示例支持通过 COM 风格接口装配引擎；LZMA SDK 的精简 7z 支持与完整 7z.dll 格式能力不同，不能用公共领域 SDK 的说明替代现有 DLL 的分发/能力规则。[7-Zip Developer FAQ](https://www.7-zip.org/faq.html)，[SDK 范围](https://www.7-zip.org/sdk.html)

将硬编码格式 GUID 尾号/属性数字改成与锁定版本对应的命名常量及启动能力盘点；有条件使用 DLL 导出的 handler 元数据，仍只启用产品格式白名单。元数据缺失返回 Unknown。魔数、扩展名和后端打开结果互相核验；不自动展开 DOCX/XLSX/JAR/APK，不因为 7-Zip 能打开可执行/SFX 就执行文件。

文件名已由官方后端正确转为 Unicode 时原样使用；不能做 GBK/UTF-8 的盲目“二次修复”。只有后端提供原始编码证据/支持明确 codepage 时才提供有界覆盖，并在创建文件前重新跑安全检查。乱码评分本身不允许覆写源包或重命名已发布成品。

锁定官方 7z.dll/对应源码/通知/摘要，不把加载路径中的 SHA 文件当作独立可信签名。移动 Engine 工程时核对资源与许可材料；版本升级另走候选/回归/回滚，不自动追最新版。[7-Zip 许可](https://www.7-zip.org/license.txt)

### 16.8 其他官方指导的具体落点

| 官方约束 | 本项目应做什么 | 实施阶段 |
|---|---|---|
| Tokio 已启动 spawn_blocking 不能靠 abort 停止 | DB 单写线程/短 IO 有界；长原生/设备 IO 用 Worker 与退出确认 | W2/W3；[Tokio 文档](https://docs.rs/tokio/latest/tokio/task/fn.spawn_blocking.html) |
| SQLite WAL 读写并发但只有一个写者 | 短提交与一致性备份；发布文件副作用独立恢复 | W3/W5；[SQLite 隔离](https://www.sqlite.org/isolation.html)、[备份](https://www.sqlite.org/backup.html) |
| Tauri capability/命令边界需明确 | 保留每命令权限检查，Rust 不因前端有权限便信任参数；禁止 UI 直接启动 shell/Worker | W2；[Tauri capability](https://v2.tauri.app/security/capabilities/) |
| Job Object 管理资源/进程，不等于代码沙箱 | 节点退出、提交内存保护、停止待确认；第三方平台继续推迟 | W4；[Windows Job Objects](https://learn.microsoft.com/en-us/windows/win32/procthread/job-objects) |

### 16.9 优化验收与执行顺序

先落实正确性与重复解码识别，再讨论常驻/并行：

1. W0 记录每个样本的 Native Open/Extract(test)/Extract(write) 调用计数、已读源字节、KDF阶段/候选耗时（能可靠观测时）、进程启动数、CPU/内存峰值、全流程时间。统计不含口令。
2. W2 分离 ReadMetadata/TestArchive/Extract 三种语义，核对 ABI、结果枚举、线程规则和回调热点。
3. W4 实现能力驱动 ProbePolicy、短期会话、候选 ID、内存/候选工作量额度、清单验证与进度快照。新增字段/操作通过 v2 显式协商，失败可回已验证独立操作路径。
4. W5 测无密码大单文件、非固实加密、固实大块/块后段、加密头部、混合加密、坏包/缺卷、大字典/大头部、很多小文件、嵌套与 HDD/SSD。

重点门槛：无密码正常提取不额外执行 test 解码；完整成功节点正式解码只进行一次（失败候选另计）；固实探针达到成本上限明确转入提取/不确定路径；同对象并发调用为 0；超限可恢复、取消不受进度阻塞、CRC/Data 错误绝不计成功。能否改善耗时按第 13 节测量，未测不报告提升百分比。
