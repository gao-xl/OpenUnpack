# OpenUnpack 追加检测报告

日期：2026-10-18。runId：`run-20261018-continuation-01`。

**整体门禁仍未通过。** 本轮继续执行真实 Core → Worker → 7-Zip 链，补齐中断恢复、取消、有效 Unicode 分卷、导入重开和密码通信证据。原有四类功能问题仍未修复；其中两类的影响已由局部探针推进到真实生产处理链。运行时安全基线和签名发行证据也仍有缺口。

本轮未修改应用源码、升级依赖、操作个人密码库、提交或发布。检测数据写入和强制结束限定到合成数据及自有测试进程；另按现有脚本生成 UI/Host 构建产物，包含仓库的生成目录。报告区分“已观察通过”“已复现问题”和“未验收”，不把缺少环境当作通过。

前轮报告：[2026-10-17 功能检测](system-detection-report-2026-10-17.md)。总计划仍为 [56 项检测计划](plans/2026-10-17-system-detection.md)，未全部结案。

## 实际结果

| 检测方向 | 本轮结果 | 结论范围 |
| --- | --- | --- |
| 真实桌面 | 当前源码 UI、Release Host 构建成功；窗口和可访问性树可读 | **输入受环境阻挡**：激活及添加文件点击均报 Windows `GetCursorPos` 访问拒绝；完成输入动作 0 次；截图为黑色，不能确认视觉渲染 |
| 进程异常中断恢复 | **2/2 通过，0 未命中、0 探针错误** | 解压与写回阶段各一次；真实非零进度后终止自有测试宿主，重开 Core 并恢复 |
| 存储异常与数据保护 | **4 项安全行为符合预期**；另 1 项边界观察 | SQLite 忙锁、损坏 DB、损坏设置、损坏 vault；同进程双 Core 不属于 Host 单实例验收 |
| 导入和备份 | 合法 CSV **3/3**；畸形 CSV **3/3 仍被接受**；大备份仍被拒绝恢复 | 本轮补了重开后的授权字段核对；十万零一条备份复用前轮合成样本，未重复全量导出 |
| 密码通信与泄漏面 | **6/6 通过，0 探针错误，33 个普通文件无精确明文命中** | 冻结 Worker v2、正确/错误密码和候选、检查、解压、超长 Password；不覆盖完整 Core 生命周期 |
| 暂停、取消和快照 | 两个显式负载测试通过；含三类真实任务控制观察 | 单任务取消约 147 ms；批量取消停下约 174 ms；均为本机单次、调试后端观察 |
| 复杂归档/分卷 | 混合错误 **12 项中 9 匹配、3 不匹配**；有效分卷 12 个扫描观察、3 个全链场景 | 错误顺序依赖仍存在；真实 `Ä/ä` 数字分卷拆组并导致任务失败 |
| 文件系统故障与发行 | 环境、签名、实际载荷和微软元数据已核对 | 满盘/脱机/SMB/跨账户、干净机安装及可信签名仍**未验收** |

这些行包含父测试、子案例和采样，不应相加成独立测试总数。前轮的 345 个 Rust 测试、148 个 UI 测试等没有全部重复执行，也没有伪称为本轮新通过数。

机器汇总：[observed-summary.json](../_workspace/system-detection-20261018/run-20261018-continuation-01/results/observed-summary.json)。证据索引：[evidence-index.json](../_workspace/system-detection-20261018/run-20261018-continuation-01/results/evidence-index.json)，记录 63 份选定日志、源码和结果的 SHA-256，不是全部生成文件的清单。

## 仍然确认的问题

### F-01：成功导出的超限密码备份无法由自身恢复

**高，可靠性；置信度高。** 前轮已用公共新增、导出和实际恢复链证实：第 100,001 条可保存并重开，导出成功，但恢复 reader 拒绝。

本轮复制前轮合成源库和备份到新隔离目录，核对哈希一致；当前公共 reader 仍能读到源库 **100,001 条**，备份预览却报 `Backup payload exceeds maximum allowed size`，恢复目标仍为 0 条。原始源库和备份均未改变。

本轮不是重新完成十万次新增或十万条合并，也没有检查任何个人密码丢失。根因和完整边界链见前轮报告及 `crates/core/src/vault.rs:648`、`:784`。

修复验收：统一写入、导出和恢复限制；成功导出的备份必须能由自身 reader 接受；拒绝导出时保留原目标。保留 100,000 / 100,001 及字节上限边界回归。

### F-02：混合 CRC/不支持方法的顺序会改变最终处置，并可发布空的损坏文件

**中，错误分类与抢救品质；置信度高。** 本轮从 Worker 局部验证推进到真实 Core 调度、缓存、发布和源清理策略。

`c` 为 CRC 错误、`u` 为不支持的方法 222、`v` 为正常文件。六种排列各运行普通与抢救模式，12 项无超时、无探针错误：

| 场景 | Core 最终结果 | 实际发布 |
| --- | --- | --- |
| `cuv/cvu/vcu`，开启抢救 | `salvaged`，scope=`salvagedWithCrcWarning` | 各提交 3 个业务文件；`unsupported.txt` 原声明 45 字节，实际 0 字节且明确 `isCorrupted=true` |
| 其余 9 项 | `failed` | 无提交的业务文件 |

三项抢救消息明确说明“3 个文件中 2 项损坏”；12 项源包、既有输出哨兵、外部哨兵全部保留，即使设置为成功后删除源包也未误删。独立只读复核重新检查了这些哈希，以及三份正常成员哈希。

因此可以确认“不支持方法被更早的 CRC 错误掩盖，处置依赖条目顺序，抢救时发布了被标记的空文件”。**不能写成普通完整成功、未标记损坏或误删源包。** 本轮也没有完成 GUI 呈现验收。

根因位置：`src/OpenUnpack.Engine/Native/SevenZipEngine.cs:954` 只保留首个非零错误；`:646` 检查首错误是否 Unsupported，`:659` 允许其他错误进入抢救。Core 的源保护和品质标记见 `crates/core/src/coordinator.rs:3829`、`:3905`、`:3930`。

修复验收：聚合各条目错误类型，统一六种顺序的 Unsupported 处理；同时复验品质、实际文件内容、发布清单和源保护。不能只看 Worker 返回 completed。

证据：[12 项全链结果](../_workspace/system-detection-20261018/run-20261018-continuation-01/results/core-e2e-01/results.json)。探针语义门禁退出 1，原因正是三项不符合预期，不是启动失败。

### F-03：真实有效的 Unicode 混合大小写分卷被拆成两个失败任务

**中，扫描/处理正确性；置信度高，限于所测场景。** 本轮使用真实双卷 `.7z`，已由独立原生 `7z t` 成功读出两卷。已知内容为 100,000 字节。

| 命名 | 四种扫描入口 | 真实 Core → Worker 结果 |
| --- | --- | --- |
| ASCII `A/a` | 均为一个完整双卷来源 | 1 个任务 completed，输出内容及哈希正确 |
| Unicode `Ä/Ä` | 均为一个完整双卷来源 | 1 个任务 completed，输出内容及哈希正确 |
| Unicode `Ä/ä` | 均拆成两个单卷来源，缺卷标记 `[false,true]` | 生产 `add_jobs` 重扫后创建 2 个任务，分别以“损坏”和“缺卷”失败，无发布输出 |

所有源卷和无关输出哨兵保持不变。前轮四种家族占位样本只证明扫描拆组，本轮额外证明这一真实数字分卷在缓存处理链中失败；不能外推为所有 Unicode 文件名或所有分卷格式都失败。

根因仍为 `crates/core/src/scanner.rs:482` 的 Unicode 小写查找与 `:737` 的 ASCII 小写归组不一致；生产重扫入口见 `coordinator.rs:711`。修复后保留三个真实对照，并扩展前轮四家族/四入口矩阵。

### F-04：畸形 CSV 仍静默改变密码字段，重开后也保留解释后的值

**中，导入校验/字段完整性；行为置信度高，严格拒绝政策需明确。** 合法引用、转义、多行三个正例均正确。未闭合引号、字段内嵌引号、结束引号后缀三例仍为预览新增 1 / 错误 0，确认后写入。

本轮进一步在关闭并重开后进行授权 reveal，全部匹配探针预先定义的去引号/拼接值，说明这不是仅有界面预览的偏差。没有记录明文密码或确认 token。

修复验收：明确支持的 CSV 方言，异常结构应给出行错误，保留合法转义及多行内容。整批拒绝还是逐行拒绝需要产品规则，不能把三个畸形案例说成合法 CSV 普遍损坏。源码位置仍为 `vault.rs:1267`、`:1333`。

F-01、F-03、F-04 及存储观察证据：[observations.json](../_workspace/system-detection-20261018/run-20261018-continuation-01/results/import-backup-22676-1792324990292707400/observations.json)。该收集器退出 0 表示观察完成，不代表其中所有产品行为都通过。

## 新增通过的有界检查

### 异常终止与恢复

使用自有子进程运行真实 Coordinator 和 Worker，每阶段一次。在观测到未完成的真实流量后，只终止自己创建且 PID 对应的测试宿主：

| 阶段 | 命中进度 | 重开后的状态 | 恢复后 |
| --- | --- | --- | --- |
| 解压 | 110,592 / 83,492,912 字节 | interrupted | completed；1 份 committed |
| 写回 | 73,728 / 83,492,912 字节 | publishPending | 旧 publication abandoned，新 publication committed |

两份最终输出各 **4,003 个文件**，路径、长度、SHA-256 全部匹配独立 oracle。每例只有一个可见结果目录，源包和哨兵不变，合法 owner marker 单独记录。已观察到的 13 个 Worker 身份均退出，没有未确认进程。

独立复核额外重算了 oracle 和两份输出共 12,009 个文件哈希，零差异；以 SQLite 只读连接检查两库 `quick_check=ok`，各只有一个 completed 任务和一个 committed 发布。

限制：这是测试宿主的进程强杀，不是 Tauri 原生窗口操作或操作系统断电。每阶段只一次且均在早期进度，未命中精确事务提交窗口；未验证空目录、ACL、ADS、时间戳、源清理竞态或全部极短命进程。

证据：[恢复结果](../_workspace/system-detection-20261018/run-20261018-continuation-01/results/recovery-races-01/results.json)。

### 存储异常

- SQLite 写锁期间添加任务被拒绝，约 2,266 ms 返回 StorageError；内存/数据库都没有半条任务，释放后重试只增加 1 条，源包未变。
- 合成损坏数据库被安全拒绝，原文件未被替换。
- 合成损坏设置被拒绝，原配置和数据库未被重置。
- 合成损坏 vault 不妨碍 shell/Core 打开，但快照给出 vault 错误且密码数为 0；损坏文件未被静默重置。这是同账户合成损坏，不是跨账户 DPAPI 验收。

同进程创建两份公共 Core 成功，是边界观察，**不列为桌面单实例缺陷**。Core 的数据库格式租约本来允许共享；真正的 Host 在 `apps/desktop-host/src/lib.rs:602` 使用独占 `.instance.lock`，在 `:1886` 先锁后开 Core。本轮未实际完成 Host 二开交互验收，源码也不能替代运行验收。

### 密码与 Worker v2

6 项均有唯一且关联身份正确的终态，Worker 正常退出，无强杀：错误候选、错误后匹配候选、正确密码 inspect、正确密码 extract、错误密码 extract、超长 Password 拒绝。

正确候选只返回匹配 ID，`matchedPassword=null`。正确密码实际解出独立已知的 54 字节内容，其 SHA-256 与终态清单一致。错误候选为 WrongPassword；错误密码解压为 PasswordMismatchOrCorrupt。32,826 字符 Password 被 ProtocolError 拒绝；未覆盖 Candidate.Secret 超长。

原始事件、stdout、stderr 在脱敏前检查无合成密码；最终 33 个普通文件按 UTF-8/UTF-16LE 完整密码精确匹配扫描，0 命中。扫描包括加密 ZIP、输出/哨兵和脱敏证据文件；summary 的 exclusions 文案列出 encrypted fixture，但实际扫描列表和代码包含它。

边界：错误密码仍留下 0 字节 `payload.txt`，不能声称完全无输出文件。temp/private-data 为空，未初始化 Core DB/cache/vault，因此不代表这些位置已完成泄漏测试。最终 results.json 在扫描后写入，不在 33 个文件内，但未保存秘密 oracle。未覆盖已删除文件、ADS、变换编码、部分密码、剪贴板、内存擦除、转储、跨账户、v1、GUI。

证据：[密码边界结果](../_workspace/system-detection-20261018/run-20261018-continuation-01/results/worker-security-execution-01/results.json)。

### 暂停、取消与快照测量

显式执行此前忽略的 snapshots、controls 两个负载测试，分别通过，耗时 78.03 秒与 996.50 秒。后者包含造数和调试模式逐文件验证，不能把 996.50 秒当作取消等待时长。

| 观察 | 实际结果 |
| --- | --- |
| 暂停新任务 | 2 个已领取任务允许完成，其余 8 个保持等待；恢复后 10 个全部精确完成 |
| 单任务真实进度后取消 | 命令返回 0.0431 ms；终态及已绑定 Worker 实际退出约 **147.4032 ms**；1 个取消、其余 9 个精确完成 |
| 两个 Worker 实际重叠后的批量取消 | Worker 退出约 **130.1558 ms**，整批停止约 **173.7233 ms**；2 个取消、8 个排队任务恢复后精确完成 |
| 1,000 条合成历史记录快照 | 20 次预热＋500 次采样；生产队列快照含 20 条，序列化总耗时 **P95 4.390 ms**，39,687 字节；旧完整快照为 P95 150.246 ms，1,928,147 字节 |

控制样本每任务 138,412,032 字节/1,025 文件，完成任务均逐文件验证且全部源包哈希保持不变。取消后分别测量到约 60.7 MB / 87.2 MB 的有归属部分工作目录，属于恢复数据；这些是测量时观察，不能叫作无主泄漏，也不代表通过后的夹具清理仍保留这些文件。

测量使用输入哈希已复核的调试 Core 测试二进制和冻结 Worker，构建与其他本轮原生负载未并行运行；用户其他背景程序未被关闭。快照场景没有执行归档、没有 Worker，未测 UI/Tauri 往返或渲染。取消每类只一次，不是 30 次 P95、独占吞吐、正式包性能或 8 小时稳定性验收。

证据：[控制命令](../_workspace/system-detection-20261018/run-20261018-continuation-01/results/measure-controls-command.json)、[控制日志](../_workspace/system-detection-20261018/run-20261018-continuation-01/logs/measure-controls.log)、[快照日志](../_workspace/system-detection-20261018/run-20261018-continuation-01/logs/measure-snapshots.log)。完整原始测量路径和采样数据由机器汇总及 evidence-index 指向。

## 安全与发行缺口

实际冻结载荷和 GUI 候选仍带自包含 .NET **10.0.12**。2026-10-18 从微软官方元数据重新核对：最新 runtime **10.0.13**，2026-10-13 发布，security=true。见 [微软发行元数据](https://builds.dotnet.microsoft.com/dotnet/release-metadata/10.0/releases.json)。DS-01/DS-02 没有修复；自包含运行时不会随用户安装的新共享运行时自动更新。

元数据的 release-level CVE 列表有 44 项引用，不能据此说 Worker 有 44 个适用漏洞。本轮没有逐个可达性或利用链验证。当前安全服务基线落后是高置信度事实，具体 CVE 适用性仍未评估。

新 GUI Host、Worker、Engine，现有 portable 和 10/16 setup 的被检二进制均为 NotSigned。这是尚未达到签名发行门禁，不是已经遭到篡改的证据。Host 与旧 portable 的哈希不同，不能互相继承原生验收。

当前非管理员，虽有 VHD/Storage 命令，但未实际创建、挂载或格式化镜像。没有可用的第二交互账户上下文。既有 E: 可移动卷的卷级健康为 Warning，未用于测试、未拔盘、未离线或填满真实盘。专用 VHD 的限定镜像/磁盘对象安全方案已记入补充报告，本轮未执行。

证据：[安全与环境报告](../_workspace/system-detection-20261018/run-20261018-continuation-01/results/security-environment/security-environment-report.md)。

## 真实窗口的阻挡

当前源码 UI 构建成功，Host 以 `release + custom-protocol + locked + offline` 构建成功并复制到本轮独立候选目录；Worker 198 个文件逐一绑定。

窗口成功出现，可访问性读到队列 0 项及本轮私有数据目录；但 Windows 输入激活与添加文件点击均报 `GetCursorPos failed: 拒绝访问 (0x80070005)`。重新发现窗口后重试仍失败，截图为黑色。没有确认完成任何输入动作，不能把“启动了”写成按钮、拖放、视觉布局或任务操作通过。已向用户询问桌面/远程会话状态，尚无恢复确认；后台检测继续完成。

候选仅使用本轮 `private-data/gui`，未操作真实用户数据；窗口保持空闲，便于恢复交互环境后继续。证据：[窗口环境状态](../_workspace/system-detection-20261018/run-20261018-continuation-01/results/gui-environment-status.json)、[构建与二进制绑定](../_workspace/system-detection-20261018/run-20261018-continuation-01/results/gui-candidate.json)。

## 输入完整性与复跑

分支 `codex/open-unpack`，HEAD `cf6e34411c809d092c48ed2f4bdd4ab6160222c1`。检测前后逐文件核对：**341 个源码/配置/测试输入、198 个 Worker 载荷、GUI 内另 198 个 Worker 文件、2 个原生工具及新 Host 均未变**。

tracked 状态前后 90 项且逐行一致，原有唯一暂存删除 `apps/desktop-ui/tsconfig.tsbuildinfo` 保留。新增文件限于报告、测试探针、隔离样本和构建证据。新 Host SHA-256 为 `5ae638472f2b24283050b44d2b5fc31c26377603e51d70208ffb6c58d7c9ddd5`。

Rust 三个独立探针先验证全部 341 个输入与前轮相同，再链接前轮独立 target 中唯一的 Core 等库；保存源、链接参数、输入库和输出哈希。未将 HEAD 或任意旧 EXE 单独当作当前源码证明。C# 密码探针仅为准备阶段使用本地缓存离线恢复 10.0.12，关闭 NuGetAudit 不构成依赖安全扫描。

原先受旧执行限制阻挡的子任务没有计为执行通过；本轮更换可执行上下文后实际串行运行并保留日志。最终序列退出 1，来自 Core 混合错误三项语义失败；imports 收集退出 0、recovery 退出 0、security 退出 0。所有原始日志保留，未覆盖失败证据。

证据：[最终完整性](../_workspace/system-detection-20261018/run-20261018-continuation-01/results/input-integrity-final.json)、[执行序列](../_workspace/system-detection-20261018/run-20261018-continuation-01/results/probe-sequence.json)、[执行器](../_workspace/system-detection-20261018/run-20261018-continuation-01/run-probes.ps1)。

## 尚未关闭的检测任务

1. 修复并重新冻结验证 F-01、F-02、F-03、F-04；维护前轮两项过期历史页 audit。本轮没有重新执行或修复该 audit。
2. 更新随包安全运行时及自动审计基线，重建真实包并完成关联回归；完成签名与最终载荷身份核验。
3. 桌面可交互后执行文件/目录/多选/拖放、暂停取消重试、删除、历史、窗口二开、DPI/多屏/可访问性验收。
4. 在干净 Windows 完成安装、升级、卸载、失败回退；本轮没有创建新 setup 或执行这些操作。
5. 在专用可控环境验证满盘、脱机、SMB 中断、第二卷/路径替换竞态及跨账户 DPAPI；不以真实用户盘冒充故障卷。
6. 扩展到发布/清源的精确提交窗口、更多随机损坏/格式组合、30 次取消分布、正式包吞吐和 8 小时稳定性。当前单次观察不关闭这些项目，也不关闭全部 S06。

复跑必须使用新 runId 或明确的新 attempt，保留本轮失败样本及 oracle。已有通过项只在代码、载荷或相关环境发生变化时扩大复验，不以重复常规套件代替上述缺失证据。
