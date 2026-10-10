# OpenUnpack 架构设计

版本：0.1 架构讨论稿 · 2026-10-07

本文件设计的是我们独立开源软件的目标架构，承接 software-spec.md 的 Windows、密码库、批量任务、双层解压和 SSD 要求；不是对参考软件内部实现的逆向结论。当前只调整设计文档，应用代码开发仍停止。既有早期骨架不代表已实现本架构。

## 1. 范围与架构决策

推荐采用一个本地桌面产品、两个进程：WPF 主程序 + 独立解压 Worker。业务模块在主程序内按职责分层；Worker 加载官方解压引擎。不引入后台常驻服务、网络 API 或账户体系。

| 项目 | 推荐决策 | 理由与边界 |
|---|---|---|
| 桌面界面 | .NET 10 / WPF + MVVM | Windows 原生界面；后台工作不占用 UI 线程 |
| 业务组织 | 分层模块，单一任务协调器 | 队列、嵌套、缓存、密码和发布由明确状态控制 |
| 解压执行 | 独立 Worker + 官方 7z.dll | 崩溃与内存回收易于隔离；独立进程不等于安全沙箱 |
| 密码保存 | 全库 JSON + 当前用户 DPAPI | 跟随 Windows 账户解锁；名称、分组、备注与口令一并保护 |
| 任务持久化 | SQLite | 保存任务树、阶段、发布意图和恢复记录，不存口令 |
| 临时内容 | SSD/目标卷工作目录 + 不可变结果清单 | 校验与恢复绑定具体文件版本，不复制一份额外的 SSD 成品树 |
| 本地通信 | 当前用户受限的命名管道 | 口令仅通过本机管道和引擎回调进入工作内存 |
| 默认并发 | 一个根任务，一个 Worker | 先控制机械硬盘读写竞争；接口保留后续按磁盘调度能力 |
| 校验 | 标准流式 / 可选完整目标回读 | 复用必要读写，验证结果明确标注范围 |

独立 Worker 原在功能草稿中列为后续增强。本架构建议提前到首版，便于取消、崩溃恢复和未来引擎替换；这是范围调整建议，尚未实现或经过用户验收。

## 2. 待定决策与建议默认

- 已确认：Windows 优先；库随 Windows 账户自动解锁；备份另设口令；机械硬盘来源；停止应用代码开发。
- 推荐默认：两个进程、SQLite 任务状态、双层任务树、标准流式验证、单根任务顺序处理。
- 可调整：20 GiB 缓存配额、保留空间阈值、源包按需缓存阈值、Worker 取消等待时间和任务清单保留期限。
- 发布前确定：产品名、最低 Windows 版本、依赖精确版本、许可证材料和公开仓库归属。

后文基于上述推荐默认画出一套主方案，不把建议当作用户已确认的具体技术偏好。

## 3. 系统与依赖关系

逻辑关系：界面 → 应用协调层 → 领域规则与接口 → 基础设施适配器。领域规则不引用 WPF、SQLite 或 7-Zip。

主程序包含界面、后台任务协调器、密码保险库、工作目录管理器、发布器和恢复器。Worker 只加载解压适配器并执行当前归档节点。

进程间关系：主程序通过受限命名管道提交节点操作，Worker 返回类型化结果和进度；主程序拥有任务状态和最终发布决定。

存储关系：主程序独占密码库和任务数据库的写入；Worker 只读指定压缩包/分卷，写当前尝试的暂存目录。

## 4. 模块职责与接口

| 模块 | 负责 | 主要输入/输出 |
|---|---|---|
| Presentation | 拖放、队列、密码库页面、配置与结果显示 | 用户命令 / 脱敏任务视图 |
| ScanService | 递归发现、路径去重、分卷分组、容器排除 | 来源列表 → ArchiveSourceGroup |
| JobCoordinator | 状态机、根任务/节点顺序、暂停、取消、重试 | JobOptionsSnapshot → JobEvents |
| PasswordResolver | 分组范围、禁用规则、关联优先、候选去重 | NodeIdentity → 单次 CandidateLease |
| VaultService | DPAPI、原子保存、加密备份、导入事务 | 密码库快照；只有本模块操作口令持久化 |
| EngineClient | Worker 生命周期、协议、错误映射 | Inspect / Extract / Cancel |
| NativeArchiveAdapter | 格式打开、条目预检、密码回调、解压与内建校验 | 归档节点 → NodeResultManifest |
| WorkspaceManager | 创建工作目录、归属标记、配额、句柄释放后的清理 | WorkspaceLease / StorageReservation |
| ResultPlanner | 合并内外层结果，保留普通文件，分配展开目录名 | 节点清单 → FinalResultManifest |
| VerificationService | 输出流摘要、复制流比对、严格目标回读 | FileVerificationRecord |
| Publisher | 目标临时目录、顺序写回、冲突处理与最终改名 | PublishPlan → PublishReceipt |
| JobRepository / RecoveryService | 阶段持久化、恢复归属、发布窗口处理、历史 | SQLite 事务与恢复建议 |

核心接口建议：IArchiveEngineClient、IPasswordVault、IJobRepository、IWorkspaceManager、IStorageBudget、IResultPlanner、IArtifactPublisher。接口传入取消令牌和类型化错误；业务层不解析 7-Zip 原始控制台文本。

原生错误映射为：需要密码、密码不匹配或加密数据损坏、普通损坏、缺卷、不支持方法、不安全条目、资源超限、I/O 错误、用户取消、Worker 崩溃。含歧义的引擎错误保留歧义，不强行归类。

## 5. 归档任务树

RootJob 表示用户加入队列的一组源包；ArchiveNode 表示每层需要解开的归档；Attempt 表示该节点一次候选密码尝试。每层密码、来源、结果和错误彼此独立。

例：outer.7z 内有 README.txt、资料/a.zip、b.rar。

- 根节点展开 outer.7z，保留 README.txt 和所有普通目录。
- 子节点分别处理资料/a.zip 和 b.rar，允许不同密码。
- 成功子节点映射到资料/a/ 和 b/；冲突时分配新目录名。
- 未展开子节点保留原内包，进入等待处理或显式部分交付。

最终结果由一份映射清单定义：最终相对路径 → 哪个节点的哪个已验证本地文件。无需把所有节点再复制合并成第三份 SSD 目录树，Publisher 可以直接按照映射清单写目标临时目录。

每个最终文件在正常写回流程中复制一次；取消或写回重试可能再次复制，不把“正常流程只写回一次”理解为绝不重试。默认保留原始外包，成功展开的派生内包可释放，保留内包选项单独计算输出容量。

首版深度建议 2 层；累计条目/解压字节上限覆盖整棵树，释放中间文件不重置累计限额。DOCX/XLSX/JAR/APK 等容器不自动展开。

## 6. 处理管线与 SSD 数据布局

正常管线：来源快照 → 分卷组确认 → 存储预留 → 按需缓存源包 → 外层提取及同步校验 → 发现内包 → 内层提取及同步校验 → 封存最终清单 → 顺序写回目标临时目录 → 所选验证 → 发布 → 更新历史/密码关联 → 清理。

SSD 工作目录概念布局：

- workspace-owner.json：不可变软件/任务/工作区 ID 与归属信息。
- input/：可选原始源包副本与完整分卷组。
- nodes/{nodeId}/attempts/{attemptId}/：当前密码尝试暂存。
- nodes/{nodeId}/result/：完整成功的节点结果。
- manifests/：节点与最终结果的不可变清单。

目录名只是定位方式，所有权必须同时匹配 SQLite 记录和工作区标记。manifest 描述文件与证据，不维护一份与数据库竞争的可变任务状态。

成功节点的尝试目录在同一工作区中转为 result，复用已有文件。释放派生压缩包、源包副本等中间内容前，确认没有活跃 Worker/读取句柄，相关结果可从保留源重建，且结果映射不再引用它。

缓存预算按实时占用和峰值预估管理：可选外包副本 + 未释放内包 + 节点结果 + 当前尝试 + 保留工作区。任务累计展开额度与实时缓存额度是两个指标，不能混为一谈。

## 7. 校验管线

每个最终文件的期望摘要绑定 nodeId、attemptId、文件版本、相对路径、大小与算法。在解压输出流中增量计算 SHA-256，同时读取引擎 CRC/MAC 等可用检查结果；节点全部结束且结果合格后封存清单。

标准模式：Publisher 从 SSD 读取文件并写向目标时，同步计算读取摘要，比较解压阶段的期望摘要；检查读取/写入/关闭错误、文件数量和大小。不另开一次 SSD 全扫描，也不完整回读 HDD。

严格模式：复用同一份期望摘要，写回结束后刷新并完整读取目标内容，比对后才发布；针对缓存绕过、设备行为和断电持久性单独记录证据边界。

来源流摘要不能证明 HDD 最终存储内容；标准结果显示“未做完整目标回读”。只核对大小也不能冒充内容摘要校验。没有内建校验码的格式/条目记录实际能力。

低成本目录/路径预检保留。常规成功路径不做完整 test → 完整 extract 两遍解码；格式允许的轻量密码试探只缩小候选，不能代表整包成功。固实归档的大块解码成本仍可能存在。

## 8. 线程、并发与进程通信

UI 线程只接收用户操作和合并后的进度快照；文件扫描、复制、摘要、备份加密和 SQLite 读写在后台执行。建议进度每 100–200 ms 合并刷新，阶段/失败事件立即通知，避免每个小文件或每个字节都触发 UI 更新。[WPF 官方线程模型](https://learn.microsoft.com/en-us/dotnet/desktop/wpf/advanced/threading-model)

Worker 通过受限本地命名管道接收版本化请求，采用 CurrentUserOnly/对应 ACL，握手核对协议版本和预期子进程身份。首版不通过 TCP/HTTP 暴露解压服务。[.NET 管道文档](https://learn.microsoft.com/en-us/dotnet/standard/io/pipe-operations)、[PipeOptions](https://learn.microsoft.com/en-us/dotnet/api/system.io.pipes.pipeoptions)

请求至少包含 protocolVersion、requestId、jobId、nodeId、attemptId、操作、来源组、批准的暂存根、资源预算。口令使用单次敏感消息，经原生密码回调送入引擎；不进入命令行、环境变量、数据库、manifest、诊断包或日志。worker 不直接读取密码保险库，不接收整份库。

消息有大小上限和长度分帧；海量条目清单分批/流式输出。原始引擎异常和文件名视为数据，UI 显示前清理格式，不把它们当作命令执行。

首版一个 Worker 可以连续处理当前根任务的多个节点/候选；任一崩溃后更换 Worker，不复用未知结果。独立进程提供故障隔离，未配置受限令牌/沙箱时仍具有当前用户权限。

## 9. 持久化与密码成功事件

推荐存储分工：

| 存储 | 内容 | 权威性与保护 |
|---|---|---|
| vault.dat + 前版备份 | 密码、分组、备注、成功统计、强内容关联 | 全库 DPAPI 加密；单写入者，原子替换 |
| tasks.db（SQLite） | 根任务/节点/尝试、阶段、配置快照、工作区归属、发布意图、成功事件 | 任务状态唯一权威；当前用户目录 ACL；不存口令 |
| settings.json | 用户配置与格式版本 | 无口令；配置更新原子保存 |
| 不可变 manifest | 来源版本、最终路径映射、大小、摘要、验证能力 | 与具体处理版本绑定，数据库记录其 ID 和内容摘要 |
| 加密迁移备份 | 密码库可迁移内容与格式版本 | 独立口令；PBKDF2 + AES-GCM，错误不改变当前库 |

SQLite 推荐表：Jobs、ArchiveNodes、Attempts、Workspaces、Publications、PasswordSuccessOutbox、SchemaVersion；不需要把每次进度刷新写入数据库。结果清单可包含大量文件，状态只保存清单引用和摘要。[Microsoft.Data.Sqlite](https://learn.microsoft.com/en-us/dotnet/standard/data/sqlite/)

密码库损坏时进入可见错误状态，禁止创建空库覆盖；读取 DPAPI 失败也不能降级成明文。DPAPI 使用当前用户范围，不使用允许本机所有用户解密的机器范围。[DPAPI 文档](https://learn.microsoft.com/en-us/windows/win32/api/dpapi/nf-dpapi-cryptprotectdata)

任务发布提交后，SQLite 事务产生 PasswordSuccessOutbox 事件；VaultService 通过 eventId 幂等应用成功次数/关联，把已应用标记与密码库快照一起原子保存，再确认事件。崩溃后重放不能重复增加成功次数。未投递完成时显示“文件完成，密码记录待保存”，不能回滚已交付文件。

两个数据存储不能宣称共享一个原子事务。首版单写入者和 outbox 是处理顺序与恢复策略；已应用事件标记仅在数据库确认且不再可能重放后才能回收。

## 10. 身份与文件版本

| 标识 | 作用 |
|---|---|
| jobId | 用户加入队列的根任务 |
| nodeId / parentNodeId | 一层归档及其父子关系 |
| attemptId | 单次密码尝试及其暂存目录 |
| workspaceId | 当前工作区与可清理归属 |
| manifestId | 一份封存结果与证据版本 |
| publishId | 一次目标目录发布 |
| passwordId | 库中密码记录，非口令 |
| sourceVersion | 路径、文件身份（可取得时）、长度、mtime 与来源描述 |

完整 SHA-256、局部采样指纹和元数据指纹用不同类型。只读到部分来源时不能生成或标注完整 SHA-256。元数据/局部指纹仅可提示候选密码，不能证明缓存或结果可复用。

强密码关联要求完整内容指纹；尽量在必要复制或输出中取得。快速直接读取路径拿不到完整指纹时，可使用弱提示先尝试并最终校验，不为记忆功能强制多读一遍整个 HDD 源包，也不把弱提示冒充强关联。

源打开期间限制共享写入/替换；文件版本发生变化使相关缓存和证据失效。任务配置使用不可变快照；用户补密码时仅刷新等待节点的候选快照，不改变正在写回的结果清单。

## 11. 发布与崩溃恢复

SQLite 事务只保证数据库内的操作，不能自动把 SSD 文件、HDD 改名和密码库一起提交。[SQLite 原子提交说明](https://www.sqlite.org/atomiccommit.html)

Publisher 使用可恢复流程：

1. 将 publishId、manifest 摘要、预期临时/最终位置及冲突规则持久化为发布意图。
2. 在目标卷创建独立临时目录，复制最终结果，执行所选验证。
3. 写入绑定 publishId/manifest 的发布标记，关闭句柄并记录“准备发布”。
4. 再次处理目标名称冲突，在目标卷内改名为最终目录。
5. 提交“已发布”与密码成功 outbox；随后独立清理 SSD 工作区。

发布标记是由软件生成的不含口令的控制信息，使用预留名称；归档中的同名条目不能覆盖它。标记采用主程序拥有的验证信息，并与数据库记录核对；不能把归档自带的任意 marker 文件当作软件发布证据。恢复完成后可按策略移除控制标记，保留任务数据库的完成记录。

恢复矩阵：

| 退出位置 | 重启处理 |
|---|---|
| 解压未完成 | 当前 attempt 不算成功；关闭 Worker 后清理或从头重试 |
| 节点结果已封存、内层等待 | 复核归属与文件版本后允许补密码重试内层 |
| 写回中 | 目标临时目录不算成品；保留已验证 SSD 结果，可重新写回 |
| 已完成检查、尚未改名 | 核对发布意图与目标冲突后继续发布或取消 |
| 已改名、数据库未提交 | 核对发布标记/清单与目标归属后补记发布；严格模式重新确认内容 |
| 发布已提交、缓存未清理 | 成品保留，清理作为独立可重试阶段 |
| 名称存在但标记不匹配 | 进入需要处理；不覆盖、不删除无关目录 |

标准模式恢复记录仍明确没有完整 HDD 回读。崩溃后无法确认的文件版本/结果状态进入待处理，不凭“目录存在”推断成功。

## 12. 取消、磁盘调度与性能

取消先向 Worker 发协作取消，原生读写回调检查令牌；界面显示“正在取消”。超时或 Worker 无响应时可终止该进程，确认进程退出、句柄关闭后再回收尝试目录。主程序同样停止复制、回读与新节点调度；已经发布的结果保留。

Worker 的进程生命周期与主程序绑定；可采用 Windows Job Object 管理子进程。主进程异常退出后的文件清理由下次恢复处理，不能假设内存 finally 总会执行。

首版单任务顺序执行；后续 Scheduler 按物理磁盘/已知卷映射配置读写许可，默认 HDD 同时一个 I/O 阶段。不同盘符可能属于同一物理盘，不能直接按盘符并发；无法确定拓扑时保守限制。

SSD、CPU、解码内存与目标 HDD 分别有预算。队列有界，内包结果超过配额时产生背压，避免同时展开一批内包导致缓存占满。暂不同时在同一 HDD 上读下一外包并写当前结果；跨物理盘重叠需实测后启用。

主要优化来自省去中间层 HDD 写入/读取、重复完整测试及额外全量校验读取。哈希、原生解码、文件创建和最后写回仍需时间；性能记录须包含全部阶段和峰值占用。

## 13. 后续代码组织建议与当前差距

这是后续目录建议，当前不创建这些项目或修改既有应用源码：

- src/OpenUnpack.Domain/：归档任务树、状态、资源与命名规则。
- src/OpenUnpack.Application/：协调器、候选策略、结果规划、接口与用例。
- src/OpenUnpack.Infrastructure/：DPAPI、SQLite、文件工作区、Publisher、Worker 客户端。
- src/OpenUnpack.Protocol/：版本化 IPC DTO，不包含 UI 或密码库持久化模型。
- src/OpenUnpack.Desktop/：WPF/MVVM、选择器、拖放与任务视图。
- src/OpenUnpack.Worker/：7-Zip 适配器、密码回调、安全提取与流式摘要。
- tests/：领域规则、协议、真实归档、崩溃窗口、SSD/HDD 性能及原生 UI 验收。

依赖方向：Desktop → Application → Domain；Infrastructure 实现 Application 的接口；Worker 与主程序共享 Protocol，而不共享 Vault 的持久化权限。工程装配点负责把适配器注入应用层。

现有目录中的早期 Core、Desktop 与测试骨架尚未迁移到该组织，也没有双进程/SQLite/嵌套发布等完整验收。日后恢复开发时先做映射和迁移，保留原有工作，不按文档名字盲目删除目录。

## 14. 备选方案、风险与验证

进程内 7z.dll 更简单、没有 IPC 启动开销，适合实验原型；但崩溃和强制取消会影响主程序。推荐正式首版使用独立 Worker。跨平台可后续替换 WPF 为 Avalonia，复用 Domain/Application 与引擎协议；当前不承担跨平台适配。

仅用 JSON 存任务更轻量，但大型任务树、频繁阶段变更和恢复意图维护困难；推荐 SQLite 管状态，JSON 只承担小配置、加密库和不可变清单。

验收必须包含：

- Worker 人为崩溃后 UI 可操作、任务有明确结果、源文件未变。
- 取消先停 Worker 再清理，清理不能跟随链接或越出所属工作区。
- 双层混合内容和不同密码的节点/输出映射正确。
- 标准模式没有额外全量 HDD 回读；流式摘要不符阻止发布。
- 严格模式验证目标读取而不是仅重复计算复制源流。
- 发布流程每个步骤之间强制退出，恢复不丢源、不误删、不重复成功计数。
- 密码不出现在进程参数、tasks.db、manifest、日志或诊断包中。
- SQLite 故障、DPAPI 故障和缓存盘移除均有可恢复或待处理状态。
- 同名冲突与发布标记伪造/归档同名条目不能诱导覆盖或错误恢复。
- UI、高 DPI、真实拖放与机械硬盘/SSD 性能分别验收，不用编译结果替代。

## 15. 实施顺序、迁移与交付

后续恢复开发时建议顺序：

1. 统一 Domain、任务树、状态与版本化接口；先明确工作区归属和验证能力模型。
2. 密码库、SQLite JobRepository、受限 Worker 协议可按接口并行实现。
3. 直接单层解压及真实格式回归通过后，加入双层 ResultPlanner。
4. 加入 SSD 存储预算、流式 Publisher 和可选严格回读。
5. 完成发布/清理窗口恢复与故障注入，再做性能和 UI 验收。
6. 固定依赖、保留许可证与准确源码材料，生成可回滚的 Windows 交付物。

迁移前备份密码库和任务状态；每个存储有 schema/version，旧版读到不支持版本应拒绝写入。应用升级失败可退回旧程序与迁移前备份，不能要求旧版强读新格式。源包和用户成品不作为回滚删除对象。

完成标准是架构边界、接口、状态、恢复、数据保护和验收方案可供实现；当前完成的是设计讨论稿，不是可发布软件。独立 Worker 与 SQLite 的首版范围建议仍可修订。

## 16. 已核对资料

- 功能依据：本目录 software-spec.md 0.2，含用户已确认需求和双层/分级校验建议。
- WPF 线程：[微软文档](https://learn.microsoft.com/en-us/dotnet/desktop/wpf/advanced/threading-model)。
- DPAPI 用户范围与完整性保护：[微软文档](https://learn.microsoft.com/en-us/windows/win32/api/dpapi/nf-dpapi-cryptprotectdata)。
- 本地管道：[.NET 文档](https://learn.microsoft.com/en-us/dotnet/standard/io/pipe-operations)、[CurrentUserOnly](https://learn.microsoft.com/en-us/dotnet/api/system.io.pipes.pipeoptions)。
- SQLite 数据访问与事务：[Microsoft.Data.Sqlite](https://learn.microsoft.com/en-us/dotnet/standard/data/sqlite/)、[SQLite 原子提交](https://www.sqlite.org/atomiccommit.html)。
- 7-Zip 引擎接入：[官方 FAQ](https://www.7-zip.org/faq.html)、[许可证](https://www.7-zip.org/license.txt)。
- 提取内建校验：[ZIP 官方实现](https://github.com/ip7z/7zip/blob/main/CPP/7zip/Archive/Zip/ZipHandler.cpp)、[7z 官方实现](https://github.com/ip7z/7zip/blob/main/CPP/7zip/Archive/7z/7zExtract.cpp)。
- 写入缓冲与刷新：[微软文档](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-flushfilebuffers)。

以上资料支持平台能力；模块划分、两进程、配额、outbox 和恢复流程是本项目的设计建议，不是文档对本软件实现质量的保证。
