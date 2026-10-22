# OpenUnpack 全系统检测任务计划

日期：2026-10-17。共 **56 项任务，34 项 P0、22 项 P1**。

本轮只生成检测计划并只读核对代码及既有证据，未执行系统测试、修复、依赖升级、安装或发布。所有任务本轮状态均为 planned；既有通过记录不能视为本轮通过。

[交互检查页](2026-10-17-system-detection.html) · [机器可读任务清单](2026-10-17-system-detection-tasks.json)。

## 范围与目标

目标是找出会让用户丢文件、错交付、密码失效、任务卡住和操作不可信的问题，并把每个结论追溯到真实版本、真实样本和原始证据。执行方式是已有回归 + 新增负例/组合 + 原生全链 + 受控故障环境。

本计划覆盖本地Windows x64桌面产品及声明的归档工作流；真实失败收集、数据库/vault、扫描/分卷/格式、嵌套、缓存/资源、IPC、发布/恢复/清理、页面交互、性能、依赖、安装与签名均有任务。

本轮只做方案。本计划不实施架构迁移，不新增云查询/穷举/压缩包创建，不承诺所有格式/文件系统支持；不改应用代码、依赖、用户设置、个人库或当前候选包。后续检测的破坏性注入只针对事先创建并登记的合成对象。

以当前Core生产链为主；每个重要场景建立单元/契约、真实Core→Worker→Engine、真实Host/WebView、实际候选包、外部环境五层证据。层级不得互相冒充。

攻击输入包括归档字节/条目名/元数据、文件系统名称空间变化和恶意/过期IPC。保护资产包括源包、既有输出、秘密与可恢复任务。程序不宣称对抗控制当前用户进程、管理员、内核或已持有高权限旧句柄的完整隔离。

分支 `codex/open-unpack`，HEAD `cf6e34411c809d092c48ed2f4bdd4ab6160222c1`；工作区已有大量未提交改动，HEAD不能单独代表当前源码。

## 推荐决策与待补环境

| 决策 | 背景 | 默认执行方式 | 负责人 |
| --- | --- | --- | --- |
| 本次计划是否直接修改应用？ | 本次交付范围是检测方案、任务清单和证据规范。后续检测发现缺陷时形成独立修复任务；应用代码、依赖、用户库和候选包本轮没有改动。 | 默认：先检测并保全失败现场，再按问题修复。 | 项目负责人 / qa-inspector |
| 先做哪些任务？ | 数据安全失败与检测器误判会影响所有后续结论；runtime已有明确未修复报告。 | 默认先 DET-001、002、004、005、006、049、050，再进入数据/状态/GUI流程；信息缺失不妨碍独立检查。 | qa-inspector |
| 没有干净机、签名或专用故障盘怎么办？ | 这些验收依赖真实环境，不能由开发机的模拟/单测替代。无需等待它们才开始本机检测。 | 默认本机部分先执行；对应任务标blocked，报告保留具体资源缺口。签名/正式发行尚不通过。 | 项目负责人 / 发行负责人 |
| 性能达到什么水平算合格？ | 仓库尚无覆盖全链GUI的统一性能服务目标。提前固定调查阈值，避免测完再修改标准。 | 暂定阈值见性能表。它们是计划建议；执行前冻结硬件、数据和阈值，既有更严格断言不放宽。 | 项目负责人 / qa-inspector |
| 要补充用户真实失败吗？ | 具体操作、报错和样本能提高发现概率，但目前尚未提供。 | DET-002持续收集并最小化去敏样本；本计划已有独立样本集，可以直接启动基础检测。 | 用户 / qa-inspector |

## 当前事实与证据边界

以下为本轮只读核对的源码事实或已保存报告，均不是本轮系统测试结论。旧记忆中的uuid失败已被后续报告更新。

| 类别 | 内容 | 依据 | 后续任务 |
| --- | --- | --- | --- |
| 当前源码确认 | 生产仍由 Core Coordinator 驱动，Application/Infrastructure 尚未完整接管；启动仍一次读取全部历史。 | apps/desktop-host/src/lib.rs:1908；crates/core/src/coordinator.rs:412 | DET-003、027、045 |
| 10/17 报告的未修复问题 | DS-01：实际随包运行时10.0.12低于报告当时审阅的10.0.13安全补丁；DS-02：自动审计未动态覆盖runtime。准备脚本与材料名单仍固定10.0.12。本轮未重新联网查询公告。 | docs/dependency-security-report-2026-10-17.md:42；scripts/prepare-desktop.ps1:3；scripts/collect-release-materials.mjs:49 | DET-049、050；正式发布阻断 |
| 既有通过，待刷新 | 10/16记录Rust351通过/8默认忽略，但全量早于最后缓存修复；最后目录身份1、管线37、资源恢复5、故障恢复3通过，严格门禁通过；UI148通过。不能把这些数相加成新的测试总数。 | docs/release-readiness-report-2026-10-15.md:48；_workspace/release-readiness-2026-10-15/cache-final-validation.json | DET-004建立同一冻结输入的统一基线 |
| 历史问题已有修复/回归 | uuid/Clippy、LZ4尾部/拼接/已有目标保护、高压缩比等待、S06本机NTFS对象身份已有后续整改。当前优先复验与扩组合，不重复报成现存缺陷。 | docs/comprehensive-fixes-report-2026-10-13.md；docs/s06-directory-identity-report-2026-10-15.md；tests/OpenUnpack.Tests/WorkerBoundaryChecks.cs | DET-010、012、017、025 |
| 新增待复现风险 | CRC在前、Unsupported在后的salvage组合可能丢失错误优先级；目前是源码风险，尚无本轮原生复现。 | src/OpenUnpack.Engine/Native/SevenZipEngine.cs:637、943；tests/OpenUnpack.Tests/Program.cs:75 | DET-011先做真实样本 |
| 测试入口覆盖差异 | C#辅助RAR入口依赖artifacts/rar-fixtures，缺目录可skipped且只检查非空；Rust format_matrix已有真实RAR/分卷与独立哈希，不能称RAR完全没测。 | tests/OpenUnpack.Tests/Program.cs:1339；crates/core/tests/format_matrix.rs:286 | DET-004、007、008统一样本与skip清单 |
| 本机GUI未覆盖 | 已有14项真实窗口记录与7项真实解压内容核对；拖放在输入前被工具拒绝，UI删除/暂停/取消未完整点验，DPI/多屏等尚缺。 | docs/release-readiness-report-2026-10-15.md:79；_workspace/release-readiness-2026-10-15/native-ui-validation.json | DET-036–042 |
| 发行/环境缺口 | 旧候选Host/Setup未签名；同机独立审计产品安装通过，正式产品干净机、WebView2、跨版升级回退、满盘/脱机/SMB等仍需对应环境。 | docs/release-readiness-report-2026-10-15.md:98、110；docs/s06-directory-identity-report-2026-10-15.md:106 | DET-047、048、052–054 |

## 执行顺序、交付与估算

| 阶段 | 执行关系 | 任务 | 估算 | 阶段输出与门禁 |
| --- | --- | --- | --- | --- |
| 0 / 输入与已知风险 | 顺序入口；样本准备可并行 | DET-001、002、049；建立DET-050的runtime负例 | 0.5–1人日 | 统一runId、输入清单、真实问题台账、runtime现况与环境缺口。G0：输入明确，旧证据不覆盖。 |
| 1 / 工程与检测器 | 编译/原生套件串行；只读映射可并行 | DET-003–006 | 1–1.5人日 | 同一冻结输入的全量基线、契约/镜像、必需样本和ignored清单。G1：任何必需失败先定位；安全可控的独立检查继续。 |
| 2 / 数据与事务 | 按依赖顺序；夹具准备可分工 | DET-008–012、015–019、021–026、030–035、044 | 2–3人日 | 原包/成品/秘密/终态零容忍，混合抢救与提交点故障先复现。G2：数据和状态不变量满足。 |
| 3 / 功能与真实桌面 | GUI同一时刻一位操作者 | DET-007、013、020、027–029、036–042 | 1.5–2人日 | 逐页面动作/状态记录，真实拖放与按钮全链。G3：没有mock提升为桌面通过。 |
| 4 / 压力与故障环境 | 测量独占；环境准备可并行 | DET-014、043–048 | 1–2人日 + 8h墙钟 | 性能分位数、泄漏趋势、随机种子、专用VHD/SMB证据。无环境项明确blocked。 |
| 5 / 候选与发行 | 安全runtime确定后才构建最终候选 | DET-050–055；回放DET-036–042受影响项 | 1–2人日；外部资源另计 | 新包身份、干净机、升级回退、实际签名后摘要。G4：本机候选和正式发行结论分别给出。 |
| 6 / 结案与修复闭环 | 所有任务/子场景状态汇齐后 | DET-056；核对DET-001–055全表 | 0.5人日 | 缺陷清单、风险排序、证据索引和修复任务。G5：pass有证据，blocked仍可见。 |

依赖语义：`dependsOn`默认表示需要相关核心场景输入/可审阅结果，不要求整项通过；`requiresPassed`才是必须通过的前置。所有写入/故障注入先完成DET-001对象归属核验。第二账户、HDD、VHD和分卷扩展缺失只阻塞对应子场景，不应卡住普通GUI/SSD耐久/独立SMB。最终拟发行候选需要安全与基线门禁通过。

先形成可复核失败，再编排修复。初始现存runtime缺口应尽早检测和报告；它不妨碍在有界隔离条件下检查当前版本。发现源/成品破坏、外部写入、秘密泄漏或假成功时停止相关危险批次并保存现场，独立安全的只读工作可继续。

## 环境、权限与数据影响

| 环境 | 准备 | 执行范围 | 结论边界 |
| --- | --- | --- | --- |
| E1 / 开发机原生 | 独立OPENUNPACK_DATA_DIR、冻结Worker、合成归档/密码、MSVC/SDK/Node/Rust/.NET；默认NTFS | 契约、完整套件、原生管线、GUI、基础性能；所有用户数据保留 | 本机GUI与依赖已安装条件；不能替代干净机 |
| E2 / 专用故障环境 | 限容VHD与唯一卷GUID、隔离VM、独立SMB共享、专用可移除设备 | ENOSPC/脱机/断连、特殊ACL/ADS、恢复；仅破坏登记的合成对象 | 不可用则DET-047/048相应子项blocked；不操作正在使用的用户盘 |
| E3 / 干净Windows | Windows10/11 x64、标准用户、无SDK；WebView2已有/缺失、联网/离线快照 | 首次安装、便携启动、正式产品升级/卸载/版本匹配回退 | 不可用则DET-052/053对应范围blocked；本机独立产品只给本机范围 |
| E4 / 发行签名 | 可信发行者证书或签名服务、预期指纹、最终实际分发字节 | Host与Setup签名、实际包内Host核验、签名后SHA | 不可用则DET-054 blocked；仍可执行未签名负向门禁 |
| E5 / 设备与账户 | 多显示器/不同DPI、隔离第二账户、可用读屏/云剪贴板环境 | 跨账户DPAPI边界、辅助功能、实际系统剪贴板策略 | 缺设备/账户逐子项记录，不修改用户系统设置凑通过 |

检测不改变生产数据模型或数据库schema。故障注入、坏schema、迁移与回退均只用合成副本。新建检测基础设施/测试用例属于后续执行工作，不在本轮假装已存在。所有日志使用公开合成口令；报告中不保存真实凭据，截图避开个人路径与秘密。

## 真值与跨层核对矩阵

| 对象 | 真值来源 | 实际比较链 | 判断与任务 |
| --- | --- | --- | --- |
| 输入/分卷 | 固定fixture-manifest与明文oracle | 扫描预览/入队→scanner→VolumeResolver→实际打开身份 | 同包只归组一次；无漏卷或未授权打开；DET-007/008/036 |
| 队列计数与恢复项 | 合成SQLite只读导出 + publication/保留目录 | queue_snapshot→Tauri事件→QueuePage | 全部活跃/可恢复项保留；最近20条仅普通完成；DET-028/039 |
| 历史分页/搜索/详情 | repository总量、查询条件及明确historyLimit | history_page/job_details→HistoryPage | 查询/翻页/搜索/删除末页ID与计数可解释一致；DET-028 |
| 阶段/进度/速度 | Worker已处理字节、实际文件长度、单调时钟 | Core阶段→IPC→TransferMetrics/任务详情 | 未知大小无伪百分比；ETA不冒称保证；DET-019/039/046 |
| 品质与交付 | 独立明文哈希、publication事务、owner标记、节点树 | Engine错误→Core质量→队列/历史/恢复动作 | salvaged/partial/pending不得变完整成功；DET-011/021/023 |
| 密码与分组 | 合成vault授权读取及变更前后状态摘要 | 候选选择/预览→UI计数→真实加密解压 | 原文、组、禁用、revision和成功关联一致；普通快照不含秘密；DET-029–031/038 |
| 资源与缓存 | 实际目录字节/卷身份、Job读回、准确PID创建时间 | 预算池/队列状态/预览Worker→界面提示 | 未知退出保留预留；采样读失败不算零；DET-025/043/044 |
| 工具箱计划 | 一次性token绑定的源/目标/选项/对象身份 | preview→用户所见→execute→文件变化 | 旧预览不可执行新选项；逐项结果与实际变化对应；DET-035 |
| 设置 | 合成设置文件、后端实际消费和实际路径/阶段 | SettingsPage→Host→Core→重启 | 仅核对现有明确设置；保存成功与实际生效分别断言；DET-041 |
| 依赖与发行 | 实际载荷身份、当日官方元数据、签名后哈希 | 构建脚本/材料→NSIS/ZIP→运行进程 | 包扫描、runtime、原生DLL、签名独立核对；DET-049–055 |

没有数据仓库或运营后台。此项目的真值是固定明文oracle、原始归档、任务/publication数据、实际文件身份、进程和分发字节。当前轮与旧轮只在相同输入/工具/硬件/模式下比较，不能直接对比不同轮次数量或吞吐。

## 页面、操作与状态矩阵

| 页面 | 动作 | 关键状态 | 任务 |
| --- | --- | --- | --- |
| 队列 | 导入→确认/取消→开始→暂停/恢复→单取消/批停→删除→重启 | pending/active/needsPassword/waitingResource/cleanupPending/pending/completed | DET-036/037/039/042 |
| 历史 | 搜索重置页、翻页、最后页删除、按需详情、失败重试和恢复 | 首次加载/已有数据加载失败/空结果/旧session/迟到详情 | DET-028/037/039 |
| 密码库 | 导入预览/确认、分组、禁用、显隐/复制、备份恢复 | 错误口令/超限/取消/重复提交/新草稿与旧响应 | DET-029–032/038 |
| 归档浏览 | 持续进度下打开/切源/关闭/展开与滚动 | 未知总量、超长/超宽树、过期结果、取消与资源等待 | DET-019/033/039/045 |
| 工具箱 | 预览→修改参数/换源→执行→取消→结果核对 | 加载/旧token/过期/重放/源替换/逐项错误 | DET-035/042 |
| 设置与宿主 | 已有设置保存/生效/重启、主题、置顶、托盘退出 | 保存失败、不同DPI、键盘/IME/读屏、睡眠恢复 | DET-040/041 |

本计划检测现有界面，不提出产品改版；HTML里的队列状态示意用于验收解释，不能当GUI证据。实际DPI、焦点、键盘和读屏必须来自真实窗口。

## 验收标准与调查阈值

| 指标 | 标准 | 处理 |
| --- | --- | --- |
| 原包/既有输出/外部目录 | 除授权的完整成功源清理外，内容与身份零非预期变化；外部sentinel零写入 | 任何违例立即停止相关危险批次、保全现场并记发布阻断 |
| 内容与终态 | 路径/数量/长度/SHA-256精确一致；假成功、品质抬升、重复交付零容忍 | 数据正确性失败；不能被其他通过数抵消 |
| 秘密 | 合成secret不进入普通日志、快照、SQLite和临时落盘；授权加密vault/备份除外 | 泄漏确认后阻断；测试明文oracle本身单列，不把夹具当泄漏 |
| 取消与GUI反馈 | 初始调查阈值：反馈P95≤300ms；准确Worker退出P95≤2s、最差≤10s，至少30次真实进度后取消 | 超阈值记P1性能/可靠性调查；不放宽现有更严格用例期限 |
| 历史和窗口 | 1000普通历史首屏≤3s；分页/搜索反馈P95≤500ms；10000记录交互>1s列调查 | 依实测机器/活跃记录量分类；执行前冻结，非既有产品承诺 |
| 吞吐比较 | 相同机器/样本/模式，1次预热+3次测量、轮换顺序；较冻结基线回退>20%调查 | 报告分位数、范围和扰动；不宣称普适SSD倍速 |
| 长期稳定性 | 100循环检查点+8h有界负载；无无界单调内存/句柄趋势，无本任务残留Worker | 持续增长保存时序与归属证据；单次工作集峰值不能证明泄漏 |
| 竞态与随机检测 | 确定性屏障覆盖为主；每条核心竞态100轮，随机每入口10000变体或2h总预算内实际量 | 记录安全成功/安全拒绝/异常各计数；样本通过不等于穷尽证明 |

## 56项任务

每项证据落在 `_workspace/system-detection-20261017/<runId>/<任务ID>/` 或被该目录索引的脚本唯一artifact目录。命令引用见后文；“新增harness/人工”不代表已有可自动执行脚本。

### A．基线、真实问题与证据

#### DET-001 · P0 · 冻结真实被测版本与脏工作区

- 状态：**未执行**；负责人：scout → qa-inspector；环境：本机；方式：只读 / 自动。
- 输入/结果前置：无；必须通过：通用输入/对象归属核验，其他见子场景。
- 命令：CMD-01、CMD-02。
- 依据：当前工作区已有大量未提交变更；10/16 报告的全量与最后定向回归属于不同时间点。
- 输入：当前完整源码（含未提交和未跟踪的必要源码）、锁文件、Host、全部 Worker/runtime/Engine/7z、候选包和工具链。

操作：

1. 记录 HEAD、分支、git status、OS/工具版本、配置与盘拓扑；不重置、不批量暂存。
2. 建立唯一 runId；按相对路径记录源码与样本 SHA-256；复制被测 Worker 全目录并记录实际运行路径，不只固定 EXE。
3. 结束时重算输入摘要；记录源码、载荷或配置漂移及受影响任务。

通过标准：

- 每份结果可以追溯到同一 sourceManifest、binaryManifest 与 fixtureManifest。
- 输入发生变化的任务标为需重跑；历史包、历史证据和用户脏改动全部保留。

证据：`git-status.txt`、`source-manifest.json`、`binary-manifest.json`、`tools.json`、`input-integrity.json`。

现有入口/扩展落点：`scripts/common.ps1`、`apps/desktop-host/src/lib.rs`、`_workspace/release-readiness-2026-10-15/code-freeze-final.json`。

#### DET-002 · P0 · 收集真实异常并形成可复现样本

- 状态：**未执行**；负责人：qa-inspector；环境：本机；方式：人工 / 问题复现。
- 输入/结果前置：DET-001；必须通过：通用输入/对象归属核验，其他见子场景。
- 命令：需人工步骤/新增检测入口。
- 依据：当前用户尚未给出具体失败操作；仅重复已有套件可能漏掉实际问题。
- 输入：用户后续补充的现象、软件版本、操作顺序和去敏样本；同时自建正常/损坏/加密/缺卷样本。

操作：

1. 为每个真实问题记录触发入口、实际/期望、频率、输入位置、当时设置与重启影响；无用户样本也继续执行其余任务。
2. 使用合成或去敏副本最小化复现；有秘密的原包留在本机授权范围，报告只用样本 ID。
3. 归入明确缺陷、环境原因、操作歧义或待复现，保存失败种子和原始日志。

通过标准：

- 真实问题有复现状态和下一步，不能用现有套件通过覆盖它。
- 无法复现时保留信息缺口，不编造根因；本项的缺信息不阻塞其他检测。

证据：`user-issues.md`、`reproduction.json`、`fixture-manifest.json`。

现有入口/扩展落点：`docs/acceptance-checklist.md`、`docs/status-ledger-2026-10-11.md`。

#### DET-003 · P1 · 核对生产链、命令权限与写入责任

- 状态：**未执行**；负责人：scout / rust-dev / ui-dev；环境：本机；方式：只读 / 结构检查。
- 输入/结果前置：DET-001；必须通过：通用输入/对象归属核验，其他见子场景。
- 命令：CMD-06。
- 依据：生产仍由 Core Coordinator 执行，新分层模块与生产路径并存；README 等历史描述需按源码校准。
- 输入：UI invoke、Host command/registration/permissions/capabilities、Coordinator、Worker、迁移适配器。

操作：

1. 逐入口追踪导入、开始、取消、删除、重试、恢复、密码、浏览、工具箱；将 handler/invoke/权限四方对应。
2. 标记任务、publication、vault 的唯一写入者；检查不可见命令、缺失权限、重名 DTO 和未使用生产分支。
3. 对照 Core 与新层的镜像门禁及故障语义，明确切换前欠缺；不在检测中切换架构。

通过标准：

- 每个 UI 动作有可达且受权限约束的生产实现和测试入口。
- 迁移完成度依据运行装配与行为对等，目录存在不算接管；差异形成单独问题。

证据：`production-callgraph.md`、`command-permission-matrix.json`、`ownership-map.md`。

现有入口/扩展落点：`apps/desktop-ui/src/api.ts`、`apps/desktop-host/src/lib.rs`、`apps/desktop-host/capabilities/main.json`、`crates/core/src/coordinator.rs`、`crates/application`、`crates/infrastructure`。

#### DET-004 · P0 · 统一跑最终源码的工程与原生基线

- 状态：**未执行**；负责人：qa-inspector；环境：本机；方式：自动。
- 输入/结果前置：DET-001；必须通过：DET-001。
- 命令：CMD-03、CMD-04、CMD-13、CMD-05。
- 依据：已有 351 项 Rust 全量和最终定向回归证据，但缺同一最终冻结输入上的新一轮统一结果。
- 输入：DET-001 的冻结输入；已准备的 Windows MSVC/SDK、.NET、Node、Rust 和锁定依赖。

操作：

1. 串行完成 .NET build+harness、UI lint/build/test、Rust fmt/all-targets Clippy/workspace tests；原生测试绑定冻结 Worker。
2. 用CMD-13从同一源码构建检测专用Worker及Host Release，固定UI摘要、完整runtime/7z与实际启动路径。不能证明旧EXE对应当前输入时不复用；OPENUNPACK_TEST_WORKER不能代替实际Host子进程路径核验。
3. 保存每个命令退出码、通过/失败/ignored/skipped 数量和完整 target 清单；排查运行 0 用例的过滤器。
4. 列举 audit/*.test.tsx、ignored 负载/物理盘/跨卷用例和未纳入 solution 的 ProtocolChecks，分别分派。

通过标准：

- 所有必需基线通过；失败真实分类，缺工具/依赖/网络标环境阻塞。
- 没有默默漏掉 target、把 dotnet test 当自定义 harness、把 ignored 或零用例算通过。
- 用于GUI的Host、内嵌UI、Worker/Engine和实际加载路径能追溯到本轮源码；已知旧runtime仅作隔离检测，安全缺口保持失败，不能当最终候选。

证据：`command-results.json`、`dotnet.log`、`ui.log`、`rust-workspace.log`、`test-inventory.json`、`skips.json`、`native-host-build-manifest.json`。

现有入口/扩展落点：`scripts/verify.ps1`、`.github/workflows/windows.yml`、`apps/desktop-ui/vitest.config.ts`、`apps/desktop-ui/audit/vitest.config.ts`、`tests/OpenUnpack.Tests/OpenUnpack.Tests.csproj`。

#### DET-005 · P0 · 协议、默认值与镜像负向检查

- 状态：**未执行**；负责人：qa-inspector / rust-dev / worker-dev；环境：本机；方式：自动。
- 输入/结果前置：DET-001；必须通过：通用输入/对象归属核验，其他见子场景。
- 命令：CMD-06。
- 依据：三种语言存在结构、null、整数和兼容性边界；既有结构与镜像检查需要绑定当前版本。
- 输入：desktop.rs ↔ types.ts；worker.rs ↔ Protocol.cs；Core ↔ Infrastructure 镜像。

操作：

1. 运行结构自检和真实 Rust→C#→Rust DTO 往返，将导出 corpus 放新 runId。
2. 覆盖 null/缺失/空字符串、枚举未知值、u64/JS安全整数、旧 snapshot 默认值、版本/能力字段组合。
3. 运行镜像变异负例；确认实际生产语义变化会触发门禁，规范化不能屏蔽安全差异。

通过标准：

- 结构与真实序列化一致；每个兼容差异有明确规则和样例。
- 错字段、可空性和关键策略漂移能被负向对照捕获，错误数据不产生默认成功。

证据：`contract-corpus/`、`contracts.log`、`mirrors.log`、`compatibility-matrix.json`。

现有入口/扩展落点：`scripts/check-contracts.mjs`、`scripts/check-architecture.mjs`、`crates/protocol/examples/contract_samples.rs`、`tests/OpenUnpack.ProtocolChecks`、`apps/desktop-ui/src/types.ts`。

> 2026-10-21 注：`scripts/check-mirrors.mjs` 已退役（双轨镜像已收敛为 re-export，由 `check-architecture.mjs` 覆盖单源约束）；本文其余处提到该脚本的位置均为历史记录。

#### DET-006 · P0 · 验证检测器自身不会误报通过

- 状态：**未执行**；负责人：qa-inspector；环境：本机；方式：自动 / 负向对照。
- 输入/结果前置：DET-004、DET-005；必须通过：通用输入/对象归属核验，其他见子场景。
- 命令：CMD-06、CMD-09。
- 依据：旧记录曾存在字段 tests:0 实为退出码、可选样本跳过、固定目录覆盖等误读风险。
- 输入：结果汇总器、fixture 清单、安全报告解析器、命令封装和证据目录。

操作：

1. 在隔离副本注入一次内容不匹配、命令非零、缺必需样本、0 tests、坏 JSON/缺分组/网络错误，观察门禁。
2. 检查 passed 只能来自完成的断言，blocked/skipped/未运行不被折算；每次执行写新目录。
3. 记录负向对照恢复后的输入哈希；旧 wrapper 只提取命令，不原地覆盖历史日志。

通过标准：

- 所有负面控制都能使对应门禁失败或明确阻塞。
- 汇总字段明确区分 exitCode 与 counts；没有用测试重试、扩大超时或删断言掩盖失败。

证据：`detector-negative-controls.json`、`result-schema-check.json`、`evidence-directory-audit.md`。

现有入口/扩展落点：`scripts/test-security-audit-policy.ps1`、`scripts/test-release-validation.ps1`、`_workspace/release-readiness-2026-10-15/verify-current.ps1`。

### B．格式、分卷、嵌套与引擎

#### DET-007 · P1 · 建立格式承诺与独立明文矩阵

- 状态：**未执行**；负责人：worker-dev / qa-inspector；环境：本机；方式：自动 / 原生。
- 输入/结果前置：DET-004；必须通过：通用输入/对象归属核验，其他见子场景。
- 命令：CMD-04、CMD-07。
- 依据：已有真实 Rust 格式矩阵及 upstream 哈希；C# 另有 artifacts/rar-fixtures 可跳过的辅助入口，需核对覆盖交集。
- 输入：ZIP/7z/RAR2–5/TAR/GZ/BZ2/XZ/Zstd/LZ4/CAB/WIM/LZH/SFX；README 或 UI 明确承诺支持的格式优先。

操作：

1. 为每种实际承诺格式列正常、空包、扩展名伪装、截断、坏头、条目损坏；标出仅引擎能力、未承诺和不支持格式。
2. 使用预先固定的明文路径/长度/SHA-256 oracle，复用 format_matrix.rs 与 upstream provenance，不从本次输出反推期望。
3. 把 C# 可跳过入口与 Rust 已有覆盖映射；必需矩阵缺样本失败，不以非空输出或引擎能打开代表正确。

通过标准：

- 承诺格式逐格得到可复核结果；所有成品路径、数量、长度和哈希与独立 oracle 一致。
- 不支持格式明确分类，SFX 只读取字节，绝不执行来源 EXE。

证据：`format-matrix.json`、`fixture-manifest.json`、`expected-files.json`、`actual-files.json`。

现有入口/扩展落点：`crates/core/tests/format_matrix.rs`、`tests/OpenUnpack.Tests/Program.cs`、`tests/fixtures/upstream-archives/provenance.json`、`src/OpenUnpack.Engine/CompositeArchiveEngine.cs`。

#### DET-008 · P0 · 分卷归组、任意卷导入与缺卷分类

- 状态：**未执行**；负责人：worker-dev / rust-dev；环境：本机；方式：自动 / 原生。
- 输入/结果前置：DET-007；必须通过：通用输入/对象归属核验，其他见子场景。
- 命令：CMD-04、CMD-07。
- 依据：扫描器、VolumeResolver 与原生引擎对首卷、后继卷和允许读取范围必须一致。
- 输入：7z .001、RAR2 .r00、RAR4/5 .partNN、ZIP .z01；完整与缺首/中/尾、错卷、重复卷样本。

操作：

1. 分别从首、中、末卷导入，混用零填充、大小写、同 stem 不同目录与巨大序号，检查只入队一组。
2. 删除/替换合成卷、锁住卷，覆盖实际打开路径授权和完整卷集的句柄租约。
3. 区分必要卷缺失与可选后继卷不存在；重复导入及递归扫描要去重。

通过标准：

- 完整归档内容与 oracle 一致；缺卷/坏卷/密码分类准确，无局部成功假报。
- 未授权路径或外部同名卷不被读取；所有源卷摘要保持且退出后锁释放。

证据：`volume-matrix.json`、`opened-source-identities.json`、`terminal-events.jsonl`、`files-before-after.json`。

现有入口/扩展落点：`crates/core/src/scanner.rs`、`crates/core/tests/scanner_regressions.rs`、`crates/core/tests/format_matrix.rs`、`src/OpenUnpack.Engine/VolumeResolver.cs`、`tests/OpenUnpack.Tests/PathIdentityChecks.cs`。

#### DET-009 · P0 · 加密、候选密码与错误优先级

- 状态：**未执行**；负责人：worker-dev / rust-dev；环境：本机；方式：自动 / 原生。
- 输入/结果前置：DET-007；必须通过：通用输入/对象归属核验，其他见子场景。
- 命令：CMD-04、CMD-07、CMD-08。
- 依据：明文条目在前、头加密、损坏密文和不支持方法会影响探测与终态归因。
- 输入：ZipCrypto/AES、7z 头加密开/关、RAR4/5 文件/头加密；公开合成口令与独立明文。

操作：

1. 候选为空、全部错误、正确位于首/中/末、128+候选、禁用组和 Unicode/首尾空格密码。
2. 构造前置明文条目、坏 CRC、缺卷和未知加密方法；检查 probe 与 extract 的分类一致。
3. 在候选尝试中取消、补临时密码、重试失败内包；只记实际匹配 candidateId。

通过标准：

- 不能因读到明文条目误判无需密码，Corrupt/Unsupported/MissingVolume 不统一误报 NeedsPassword。
- 不回传密码明文，不把未使用候选记为成功，取消后可安全重试。

证据：`encryption-matrix.json`、`candidate-events-redacted.jsonl`、`error-taxonomy.json`。

现有入口/扩展落点：`src/OpenUnpack.Engine/Native/SevenZipEngine.cs`、`src/OpenUnpack.Worker/WorkerOperations.cs`、`crates/core/tests/desktop_pipeline.rs`、`crates/core/src/vault.rs`。

#### DET-010 · P0 · LZ4 历史缺陷复验与边界组合

- 状态：**未执行**；负责人：worker-dev；环境：本机；方式：自动 / 原生。
- 输入/结果前置：DET-004；必须通过：通用输入/对象归属核验，其他见子场景。
- 命令：CMD-04、CMD-07。
- 依据：当前已有单帧策略和专门回归；历史连帧静默截断、尾部垃圾、已有目标误删不能直接当现存缺陷。
- 输入：合法单帧、拼接帧/skippable、垃圾尾部、坏 header/block/content checksum、错误 contentSize/descriptor、依赖块/字典。

操作：

1. 重跑 WorkerBoundaryChecks；对大小/预算/块长做 0、阈值−1、阈值、阈值+1 变体。
2. 通过真实 Worker 覆盖直写、缓存、嵌套、恢复、排除规则及目标 sentinel；检查未知展开大小到已知大小的转换。
3. 核对单帧政策：不支持变体明确 Unsupported，不能静默忽略第二帧；验证范围与 UI 品质声明一致。

通过标准：

- 合法结果完整；损坏/不支持变体不误报完整成功。
- 失败时原包与已有目标字节不变；只清理本次创建输出；品质不冒称无内建校验的格式已做原生 CRC。

证据：`lz4-corpus.json`、`lz4-results.json`、`sentinel-before-after.json`、`quality-scope.json`。

现有入口/扩展落点：`src/OpenUnpack.Engine/Lz4Decoder.cs`、`src/OpenUnpack.Engine/Lz4Engine.cs`、`tests/OpenUnpack.Tests/WorkerBoundaryChecks.cs`、`crates/core/tests/desktop_pipeline.rs`。

#### DET-011 · P0 · 抢救模式与混合错误的真实复现

- 状态：**未执行**；负责人：worker-dev / qa-inspector；环境：本机；方式：新增用例 / 原生。
- 输入/结果前置：DET-007、DET-009；必须通过：通用输入/对象归属核验，其他见子场景。
- 命令：CMD-04、CMD-07。
- 依据：源码保留首个 OperationResult，并在 salvage 且有文件时允许返回；CRC 在前、Unsupported 在后是待复现风险，尚非本轮确认缺陷。
- 输入：CRC 损坏 + Unsupported method + 正常文件的多成员包，交换顺序；叠加取消、缺卷和资源不足。

操作：

1. 制作能被原生引擎真实触发的最小多成员样本，先关闭再开启 salvage；不能只 mock terminal。
2. 对条目顺序、恢复入口、部分交付、严格回读和源清理开关逐组测试。
3. 观察逐文件损坏记录、顶层错误、publication 品质及 UI；保存引擎回调与跨层终态。

通过标准：

- Unsupported/取消/资源错误不被 CRC 抢救吞掉，失败不变普通完成。
- salvaged 品质在恢复和重启后保留；部分交付必须显式，绝不自动删除源包。

证据：`salvage-mixed-corpus.json`、`engine-operation-results.jsonl`、`publication-quality.json`、`native-ui-observation.md`。

现有入口/扩展落点：`src/OpenUnpack.Engine/Native/SevenZipEngine.cs:637`、`src/OpenUnpack.Engine/Native/SevenZipEngine.cs:943`、`tests/OpenUnpack.Tests/Program.cs:75`、`crates/core/src/coordinator.rs`。

#### DET-012 · P0 · 高压缩比、未知大小与累计预算

- 状态：**未执行**；负责人：rust-dev / worker-dev；环境：本机；方式：自动 / 原生。
- 输入/结果前置：DET-004；必须通过：通用输入/对象归属核验，其他见子场景。
- 命令：CMD-04、CMD-07。
- 依据：高压缩比等待资源已有修复与回归，应验证实际大小、估计、硬限制和共享预留的一致性。
- 输入：受限大小的高压缩比 ZIP、未知大小 LZ4、深层归档；entry/bytes/depth/cache/空间余量阈值与 u64 极值元数据。

操作：

1. 各阈值使用 −1/等于/+1，比较 probe/inspect/extract/copy/readback；永不真实无限展开。
2. 测试声明大小小于实际、空包、零预算、大整数溢出和嵌套累计额度；固定总时间和物理输出上限。
3. 共享预算两任务竞争、取消等待、释放额度后唤醒；永久超过配置限额与暂时竞争分开。

通过标准：

- 合法包在足够预算时可完成，实际超限有界停止并准确分类。
- 没有整数回绕、预算绕过、自等待或永久卡 waitingResource；保留工作区计入占用。

证据：`budget-boundaries.json`、`resource-reservations.jsonl`、`measured-output-bytes.json`。

现有入口/扩展落点：`crates/core/src/route.rs`、`crates/core/src/coordinator.rs`、`crates/core/tests/desktop_pipeline.rs`、`src/OpenUnpack.Worker/WorkerOperations.cs`。

#### DET-013 · P1 · 嵌套树、同名内包与部分交付

- 状态：**未执行**；负责人：rust-dev / worker-dev；环境：本机；方式：自动 / 原生。
- 输入/结果前置：DET-009、DET-012；必须通过：通用输入/对象归属核验，其他见子场景。
- 命令：CMD-07。
- 依据：父成功不能遮盖子失败，内外包可有不同密码，DOCX/APK 等容器不应被无意当通用嵌套包展开。
- 输入：同 stem 内包、不同层口令、父成功子坏/缺卷/Unsupported、深层空目录、容器文件及混合普通文件。

操作：

1. 展开多分支树，单独补失败子包密码，验证父成果复用而成功分支不重复发布。
2. 按边界深度、累计预算和排除规则检查树闭合；重试、清理、重启、部分交付各走一轮。
3. 对同名节点、重命名源和内容变化核对节点内容身份与成功密码关联。

通过标准：

- 节点状态、内容和品质可追溯，未展开内包清楚可见。
- 部分结果不会自动升级完整成功；树未闭合或品质不足时源包保持。

证据：`nested-tree-before-after.json`、`node-content-oracle.json`、`retry-publications.json`。

现有入口/扩展落点：`crates/core/src/scanner.rs`、`crates/core/src/coordinator.rs`、`crates/core/tests/desktop_pipeline.rs`、`apps/desktop-ui/tests/node-tree.test.tsx`。

#### DET-014 · P1 · 有界畸形输入与随机序列检测

- 状态：**未执行**；负责人：worker-dev / qa-inspector；环境：隔离测试进程；方式：新增 harness / 随机检测。
- 输入/结果前置：DET-007、DET-010、DET-012；必须通过：通用输入/对象归属核验，其他见子场景。
- 命令：需人工步骤/新增检测入口。
- 依据：定向检索尚未找到专用 fuzz/property runner；传统用例对组合边界的发现能力有限。
- 输入：已有最小正常/损坏 corpus，固定 PRNG seeds；LZ4、ZIP 元数据、manifest、JSON 和任务操作模型。

操作：

1. 首轮每解析入口至少 10,000 个有界变体，或 2 小时总预算内已执行量；逐例子进程/总字节/条目/深度/时间限制。
2. 根据生产状态模型随机生成开始/暂停/取消/重试/删除/恢复序列，保留 seed 和最短失败序列。
3. 对支持且语义一致的格式与独立工具/已知明文做差分；差异先分类，不盲信参考工具。

通过标准：

- 没有 Host 崩溃、越界输出、已有目标损坏、假成功或永久挂起。
- 崩溃/超时/差分均可由 seed 重放；低于计划样本数或时长注明实际覆盖，不写成无限输入证明。

证据：`fuzz-invocation.json`、`seeds.txt`、`corpus/`、`crashes/`、`sequence-replays.json`。

现有入口/扩展落点：`src/OpenUnpack.Engine/Lz4Decoder.cs`、`crates/core/src/zip_encoding.rs`、`crates/core/src/worker_manifest.rs`、`src/OpenUnpack.Worker/Protocol.cs`、`crates/core/src/coordinator.rs`。

### C．文件安全、发布与源清理

#### DET-015 · P0 · 恶意路径、名称与目录树边界

- 状态：**未执行**；负责人：worker-dev / rust-dev；环境：本机 NTFS；其他文件系统扩展；方式：自动 / 原生。
- 输入/结果前置：DET-004；必须通过：通用输入/对象归属核验，其他见子场景。
- 命令：CMD-04、CMD-07。
- 依据：归档条目名称与元数据不可信，扫描、Engine、manifest、复制和发布需采用一致的安全政策。
- 输入：../、绝对/UNC/盘符/verbatim、ADS、DOS 设备名、尾点/空格、大小写重复、文件目录冲突、超深/长路径与 Unicode。

操作：

1. 每类输入通过实际归档→Worker→Core 处理，外部放只读核对用 sentinel 目录。
2. 覆盖排除规则前后、重复条目、空目录、符号链接/reparse、受保护系统目录及近似目录名。
3. 记录拒绝点与错误，不在生产目录做实际写入。

通过标准：

- 外部 sentinel 零修改、零新增；无逃逸、任意 ADS 写入或受保护目录绕过。
- 对安全可表达名称给出一致结果，无法安全表达的路径明确失败而非静默丢文件。

证据：`path-corpus.json`、`external-sentinel-manifest.json`、`filesystem-events.jsonl`。

现有入口/扩展落点：`crates/core/src/paths.rs`、`crates/core/src/toolbox.rs`、`src/OpenUnpack.Engine/WorkerSandbox.cs`、`tests/OpenUnpack.Tests/PathIdentityChecks.cs`。

#### DET-016 · P0 · 原子不覆盖与并发同名发布

- 状态：**未执行**；负责人：rust-dev / worker-dev；环境：本机；方式：自动 / 原生竞态。
- 输入/结果前置：DET-015；必须通过：通用输入/对象归属核验，其他见子场景。
- 命令：CMD-04、CMD-07。
- 依据：预览/检查后目标可被其他任务创建；原子发布必须保护非本任务对象。
- 输入：既有文件/目录、编号/跳过策略、两任务同目标、rename/copy 前抢建目标、坏 LZ4 与临时文件碰撞。

操作：

1. 先对已有 sentinel 测正常、失败与重试，再用同步屏障在检查与写入之间制造碰撞。
2. 覆盖同卷发布、跨卷临时复制、取消和清理；核对临时对象归属及发布 ID。
3. 每条竞态至少 100 轮或确定性覆盖所有同步点，记录实际轮次。

通过标准：

- 既有对象身份/内容不变，编号/跳过行为一致。
- 任何失败只清理本任务新建对象；不可用普通 exists 检查作为原子安全证据。

证据：`collision-rounds.json`、`object-identities.json`、`sentinel-before-after.json`。

现有入口/扩展落点：`crates/core/src/paths.rs`、`crates/core/src/coordinator.rs`、`crates/core/tests/release_reliability.rs`、`src/OpenUnpack.Engine/Lz4Engine.cs`。

#### DET-017 · P0 · 祖先替换、junction、源锁与硬链接

- 状态：**未执行**；负责人：rust-dev / worker-dev；环境：本机 NTFS；方式：自动 / 原生竞态。
- 输入/结果前置：DET-015；必须通过：通用输入/对象归属核验，其他见子场景。
- 命令：CMD-04、CMD-07。
- 依据：S06 已有本机 NTFS 对象身份整改与真实回归，当前任务是重验并扩展组合，不重复宣称旧竞态未修。
- 输入：固定目录句柄、祖先改名/junction 切换、属性方式 reparse 变化、源/卷替换、旧打开句柄、锁/ACL/硬链接。

操作：

1. 重跑 PathIdentityChecks 和 Rust paths/toolbox；对解压、复制、回读、发布、清理、回收站逐动作组合。
2. 外部 sentinel 用身份、内容与文件系统事件观测；安全拒绝也核对源/工作区保留。
3. 明确当前用户恶意进程、管理员、内核及既授予高权限旧句柄的威胁边界。

通过标准：

- 写入/删除始终关联已批准对象，未证明安全时拒绝而保留现场。
- 没有外部写入/误删；操作结束后本次租约可释放，不永久锁死合法对象；不夸大为 OS 沙箱。

证据：`identity-race-rounds.json`、`handle-errors.json`、`external-filesystem-events.jsonl`。

现有入口/扩展落点：`crates/core/src/paths.rs`、`crates/core/src/saferecycle.rs`、`tests/OpenUnpack.Tests/PathIdentityChecks.cs`、`docs/s06-directory-identity-report-2026-10-15.md`。

#### DET-018 · P0 · 跨卷移动、失败回退与源对象保持

- 状态：**未执行**；负责人：rust-dev；环境：两个明确选定测试卷；方式：自动 / 原生。
- 输入/结果前置：DET-016、DET-017；必须通过：通用输入/对象归属核验，其他见子场景。
- 命令：CMD-08。
- 依据：跨卷复制和最终删除之间必须持有同一源对象；不是所有 rename 错误都允许复制回退。
- 输入：独立合成源/目标卷、工具箱移动、锁冲突/ACL拒绝/已有目标、复制中断。

操作：

1. 显式配置第二测试卷并核对其身份；运行现有 ignored 跨卷用例。
2. 在复制前/中/后和删除前改变源路径、抢建目标、取消或触发有限写失败。
3. 比较字节/哈希、对象身份与临时文件集合；记录仅明确跨卷错误进入回退。

通过标准：

- 成功时目标完整且仅删除原持有源对象；失败时源与已有目标保持。
- 临时对象有归属、可恢复；两卷结果不冒充两块不同物理磁盘的性能证据。

证据：`volumes.json`、`cross-volume-results.json`、`source-target-manifests.json`。

现有入口/扩展落点：`crates/core/src/toolbox.rs`、`crates/core/src/paths.rs`、`crates/core/tests/physical_disk_cross_device.rs`。

#### DET-019 · P0 · 大清单、内容校验与发布一致性

- 状态：**未执行**；负责人：rust-dev / worker-dev；环境：本机；方式：自动 / 原生。
- 输入/结果前置：DET-005、DET-016；必须通过：通用输入/对象归属核验，其他见子场景。
- 命令：CMD-04、CMD-07、CMD-08。
- 依据：生产端 spill 与 Rust 消费端均有单项覆盖，仍需真实双端联合以及校验失败负例。
- 输入：16,000/40,000 小文件、超 8 MiB 元数据、内联阈值±1、128 MiB清单边界、Unicode 转义、标准/严格校验模式。

操作：

1. 通过真实 Worker 生成 spill，Rust 实际消费；比较总数、末项、相对路径、长度和 SHA-256。
2. 篡改长度/hash/path/owner/代号、替换文件身份、清单发布碰撞及中途取消，必须真实拒绝。
3. 在写回/回读前注入单文件内容改变，检查标准流式验证与严格目标逻辑回读的实际覆盖。

通过标准：

- 全量清单和独立明文一致；不接受部分清单、错误代号或任意外部文件。
- 损坏负例不发布完整成功；不能把逻辑回读说成绕过系统缓存的物理持久性证明。

证据：`manifest-spill-events.jsonl`、`manifest-tamper-results.json`、`expected-actual-files.json`、`verification-scope.json`。

现有入口/扩展落点：`src/OpenUnpack.Worker/WorkerOperations.cs`、`crates/core/src/worker_manifest.rs`、`crates/core/tests/archive_inspection.rs`、`crates/core/tests/desktop_pipeline.rs`。

#### DET-020 · P1 · 来源标记与文件系统兼容边界

- 状态：**未执行**；负责人：worker-dev / rust-dev；环境：NTFS；隔离 exFAT/SMB 扩展；方式：自动 / 原生。
- 输入/结果前置：DET-015、DET-019；必须通过：通用输入/对象归属核验，其他见子场景。
- 命令：CMD-04、CMD-07。
- 依据：MOTW/ACL/对象身份保护依赖实际文件系统能力，不能为兼容性跳过安全要求。
- 输入：带 ZoneId=3 的无害归档、嵌套包、缓存/跨卷路径、16 KiB ADS 边界、只读目标及不支持 ADS 的测试卷。

操作：

1. 核对 Internet 来源经解压、内包、复制、发布传递，检查每个输出的 Zone.Identifier。
2. 制造 ADS读取过长/写入失败/ACL无法证明私有性的负例，记录拒绝和保留状态。
3. 把 FAT/exFAT、SMB、特殊 ACL 与 NTFS 分开出结论；无法提供测试卷时标 blocked。

通过标准：

- 需要传播的来源标记失败不得完整发布或清源；不得允许归档自定义任意 ADS。
- 不支持场景明确诊断，源/既有输出保持；支持声明只覆盖真实验证组合。

证据：`motw-before-after.json`、`filesystem-capabilities.json`、`acl-failure-results.json`。

现有入口/扩展落点：`tests/OpenUnpack.Tests/WorkerBoundaryChecks.cs`、`src/OpenUnpack.Engine/ZoneIdentifier.cs`、`crates/core/src/saferecycle.rs`、`docs/s06-directory-identity-report-2026-10-15.md`。

#### DET-021 · P0 · 源清理与回收站的完整成功门槛

- 状态：**未执行**；负责人：rust-dev / qa-inspector；环境：本机合成数据；方式：自动 / 原生 + GUI抽查。
- 输入/结果前置：DET-011、DET-013、DET-017、DET-019；必须通过：通用输入/对象归属核验，其他见子场景。
- 命令：CMD-07。
- 依据：默认保留源；源清理是数据丢失风险最高的动作之一，须与树闭合、品质和 publication 提交关联。
- 输入：保留/回收站/删除三策略；完整成功、salvaged、部分交付、缺密码、待核对、cleanupPending、锁/硬链接/ACL拒绝。

操作：

1. 仅使用合成源包，逐状态测试清理资格和全分卷集合；清理前后记录对象身份。
2. 在提交前后取消/重启/保存失败及 Shell 交接不确定时复验。
3. 确认失败工作区计量、清理重试幂等，用户已发布成品与密码库从不被任务删除波及。

通过标准：

- 只有品质/树/发布/退出证据全部满足时才可按用户选择清源。
- 部分、损坏、失败、未知退出、待核对和身份不确定均保留源；无误删、无晚到清理误伤。

证据：`cleanup-eligibility-matrix.json`、`source-before-after.json`、`recycle-handoff.json`、`retained-workspace-bytes.json`。

现有入口/扩展落点：`crates/core/src/coordinator.rs`、`crates/core/src/saferecycle.rs`、`crates/core/src/recycle_shell.rs`、`crates/core/tests/desktop_pipeline.rs`。

### D．状态机、持久化与恢复

#### DET-022 · P0 · 按事务边界强杀并验证重启恢复

- 状态：**未执行**；负责人：rust-dev / qa-inspector；环境：独立数据目录 / 专用测试进程；方式：故障注入 / 原生。
- 输入/结果前置：DET-004、DET-019；必须通过：通用输入/对象归属核验，其他见子场景。
- 命令：CMD-07。
- 依据：现有三项真实可靠性用例不等于所有发布/数据库窗口都已覆盖。
- 输入：正常/加密/嵌套/缓存/直写任务；扫描、解压、复制、回读、发布意图、改名前后、commit前后、清理阶段。

操作：

1. 以测试钩子/可观测屏障定位每个阶段；分别杀 Worker、杀 Host，并记录 PID+创建时间和准确注入时点。
2. 重开同一合成数据目录，核对 jobs/publications/outbox/目录标记与文件内容，执行恢复。
3. 每个关键窗口至少 3 次确定性重放；强杀不声称等同真实掉电。

通过标准：

- 原包、既有成品不变；未提交结果不假成功；已提交输出不会被清理误删。
- 恢复可重复且无重复交付/重复密码成功记账，未决状态保守保留。

证据：`crash-point-matrix.json`、`pid-events.jsonl`、`sqlite-after-restart.json`、`filesystem-before-after.json`。

现有入口/扩展落点：`crates/core/src/coordinator.rs`、`crates/core/src/repository.rs`、`crates/core/tests/release_reliability.rs`、`crates/core/tests/resource_recovery_regressions.rs`。

#### DET-023 · P0 · 提交后错误、品质保持与 outbox 幂等

- 状态：**未执行**；负责人：rust-dev；环境：本机隔离存储；方式：故障注入 / 自动。
- 输入/结果前置：DET-022；必须通过：通用输入/对象归属核验，其他见子场景。
- 命令：CMD-07。
- 依据：已交付内容、publication提交与后处理失败必须区分；旧 A02/A03 已有修复，需跨入口复验。
- 输入：完整/salvaged/partial 品质，commit后统计/保存/密码成功记账/源清理失败，重复事件与恢复。

操作：

1. 在提交后注入持久化失败或后处理失败，检查 cleanupPending/待核对诊断及有效输出。
2. 重放相同 success event 与恢复请求，核对 passwordId+passwordRevision、ack和outbox唯一性。
3. 跨 restart/delete/verify/cleanup 流程检查品质与验证范围不被抬升或丢失。

通过标准：

- 已提交成品不降为未交付失败，不因后处理错误删除输出。
- 品质保持、记录幂等、陈旧 passwordRevision 不修改新密码；诊断指向实际失败阶段。

证据：`post-commit-faults.json`、`outbox-before-after.json`、`quality-state-transitions.json`。

现有入口/扩展落点：`crates/core/src/repository.rs`、`crates/core/src/coordinator.rs`、`crates/core/src/vault.rs`、`crates/core/tests/desktop_pipeline.rs`。

#### DET-024 · P0 · 暂停、取消、删除与迟到事件竞态

- 状态：**未执行**；负责人：rust-dev / qa-inspector；环境：本机；方式：自动 / 原生竞态。
- 输入/结果前置：DET-004；必须通过：通用输入/对象归属核验，其他见子场景。
- 命令：CMD-07、CMD-08。
- 依据：已有删除/迟到put/恢复竞争回归；实际任务控制仍需组合与重复操作覆盖。
- 输入：排队、实际解压、copy/readback、waitingResource、cleanupPending 与已结束任务混合批次。

操作：

1. 反复快速暂停/恢复、单取消、批停、删除、重试，注入旧generation和乱序终态。
2. 在外层 future 被丢弃、删除调用者取消、Worker迟退、记录淘汰和恢复并发时重放。
3. 用准确 PID+创建时间确认退出后才释放工作区/额度，其他任务继续。

通过标准：

- 暂停后不再领取新任务；取消/删除幂等，旧事件不复活记录。
- 未经退出确认不清理或复用对象；删除不影响源、成品、密码库或其他任务。

证据：`control-sequences.json`、`late-event-replay.jsonl`、`worker-exit-proof.json`、`deleted-record-reopen.json`。

现有入口/扩展落点：`crates/core/src/coordinator.rs`、`crates/core/tests/desktop_pipeline.rs`、`crates/core/tests/worker_client.rs`、`crates/core/tests/load_profile.rs`。

#### DET-025 · P0 · 缓存路线、等待唤醒与旧工作区恢复

- 状态：**未执行**；负责人：rust-dev；环境：本机；有界 SSD/HDD扩展；方式：自动 / 原生。
- 输入/结果前置：DET-012、DET-024；必须通过：通用输入/对象归属核验，其他见子场景。
- 命令：CMD-07、CMD-08。
- 依据：最后修复涉及旧缓存工作区按实际目录计量与同卷直写发布，需纳入统一最终基线。
- 输入：cache关闭/auto/always；同卷/跨卷；未知大小/高压缩比；旧保留工作区与中途降低配额/修改路线。

操作：

1. 先保留缓存目录再改变全局设置重试，核对按持有对象的实际存储位置恢复。
2. 两任务共用不足以并行的缓存，取消/释放/失败清理后自动唤醒，观察FIFO和长期等待。
3. 直写同卷发布检查没有多余全量copy；并行占用采样记录delete-pending，不能把读失败当零。

通过标准：

- 不绕过缓存配额、没有自等待/饥饿，永久越限与暂时竞争分类准确。
- 未知目录身份拒绝恢复且保留现场；所有成功文件及源哈希正确。

证据：`cache-route-matrix.json`、`reservation-timeline.jsonl`、`retained-bytes.json`、`pipeline-stages.jsonl`。

现有入口/扩展落点：`crates/core/src/coordinator.rs`、`crates/core/src/route.rs`、`crates/core/tests/desktop_pipeline.rs:305`、`crates/core/tests/resource_recovery_regressions.rs`。

#### DET-026 · P0 · 数据库、配置与多实例故障

- 状态：**未执行**；负责人：rust-dev / qa-inspector；环境：合成数据副本；方式：自动 / 故障注入。
- 输入/结果前置：DET-001、DET-004；必须通过：通用输入/对象归属核验，其他见子场景。
- 命令：CMD-07。
- 依据：持久化与数据库租约是恢复和单写权威基础，启动时仍一次读全任务。
- 输入：SQLite schema各历史版本、WAL/SHM、空/截断/坏文件、只读目录、锁占用、坏设置、同数据目录双实例。

操作：

1. 仅复制合成库，注入读写/事务提交失败和不支持schema；检查启动错误与降级政策。
2. 两个 Host 同时打开同一目录、异常退出后重启，核对 DatabaseLease 与写冲突诊断。
3. 迁移失败回滚后比较原库、密码库、恢复行和输出引用，不直接编辑用户实际SQLite。

通过标准：

- 失败不创建空库覆盖旧数据，不静默重置设置/密码；缺权限可诊断。
- 不支持schema明确阻断；多个实例不会双领取或交错破坏任务/发布状态。

证据：`database-fault-matrix.json`、`schema-before-after.json`、`multi-instance-events.jsonl`、`backup-recovery-proof.json`。

现有入口/扩展落点：`crates/core/src/repository.rs`、`crates/core/src/database_lease.rs`、`crates/infrastructure/tests/migration_regressions.rs`、`apps/desktop-host/src/lib.rs`。

#### DET-027 · P1 · 重试、恢复与操作模型对等

- 状态：**未执行**；负责人：rust-dev / qa-inspector；环境：本机；方式：自动 / 模型对照。
- 输入/结果前置：DET-013、DET-022、DET-023；必须通过：通用输入/对象归属核验，其他见子场景。
- 命令：CMD-07。
- 依据：Domain 目标状态机与生产 Core 不完全同源，不能假定新层 guards 已约束当前运行。
- 输入：NeedsPassword/WaitingResource/Cancelled/Failed/partial/salvaged/cleanupPending/pending-publication 全状态。

操作：

1. 按生产允许动作生成 transition table；对非法操作、重复重试、缺工作区与目录替换逐格验证。
2. 同一故障用例分别驱动 Core 与可测试的新层，列出差异；未实现事务/outbox/身份保护记迁移缺口。
3. 节点级重试、部分交付、恢复不应全树重复运行或替换有效输出。

通过标准：

- 生产状态和UI动作一致，非法转换失败且不产生副作用。
- 迁移层差异明确记录且阻止未经验证的架构切换；现阶段保持生产Core路径。

证据：`transition-matrix.json`、`recovery-action-results.json`、`layer-parity-gaps.md`。

现有入口/扩展落点：`crates/domain/src/jobs/state_machine.rs`、`crates/application/src/use_cases`、`crates/core/src/coordinator.rs`、`crates/infrastructure/tests`。

#### DET-028 · P1 · 快照、历史分页与会话修订正确性

- 状态：**未执行**；负责人：rust-dev / ui-dev；环境：本机；方式：自动 + 原生抽查。
- 输入/结果前置：DET-005、DET-026；必须通过：通用输入/对象归属核验，其他见子场景。
- 命令：CMD-03、CMD-07、CMD-08。
- 依据：有限队列快照保留活跃/可恢复记录，普通完成记录只取近期；历史可见窗口和数据库总量不是一回事。
- 输入：10/1000/10000 条合成记录，混入活跃/可恢复/保留目录/旧session记录；最后一页删除、搜索与迟到详情。

操作：

1. 比较只读数据库总量、backend摘要、queue_snapshot、history_page和job_details，逐ID核对。
2. 模拟事件乱序、相同/旧revision、Host重启session变化、搜索快速修改和请求失败。
3. 核对所有恢复项不被20条近期上限裁掉，分页不会出现遗漏/重复/错误清空保护。

通过标准：

- 数据真值与各页面计数可解释一致；加载失败保留旧数据并可重试。
- 旧请求不覆盖新会话/查询/详情；隐藏页的工作区与恢复记录仍受删除保护。

证据：`snapshot-page-reconciliation.json`、`session-events.jsonl`、`history-errors.json`、`native-pages.png`。

现有入口/扩展落点：`crates/core/src/coordinator.rs`、`crates/protocol/tests/snapshot_revision.rs`、`apps/desktop-ui/src/snapshot-cursor.ts`、`apps/desktop-ui/tests/history-pagination.test.tsx`。

### E．密码、IPC 与操作授权

#### DET-029 · P1 · 密码导入、原文保持与原子操作

- 状态：**未执行**；负责人：rust-dev / ui-dev；环境：合成密码库；方式：自动 + 原生抽查。
- 输入/结果前置：DET-004；必须通过：通用输入/对象归属核验，其他见子场景。
- 命令：CMD-03、CMD-07。
- 依据：密码空白、分组上限、预览去重与合并是跨UI/后端的数据一致性边界。
- 输入：TXT/CSV/TSV，BOM/不同编码、引号/逗号/换行、空密码、首尾空格、Unicode、重复密码/分组，字节/数量边界。

操作：

1. 导入预览与确认分别校验；测试100分组+1合并、重叠合法组、备份叠加和超限旧数据。
2. 取消/关窗/切任务/重复提交/异步迟到时比较前后库摘要；原子失败不出现半导入。
3. 按启用/停用/组筛选核对真实候选，不以名称显示值替代密码原文字节。

通过标准：

- 密码不trim、不误改编码；去重与合并可解释，预览与实际提交一致。
- 失败和取消不污染库，边界错误不产生不可导出的新记录。

证据：`password-import-matrix.json`、`vault-state-digests.json`、`candidate-selection.json`。

现有入口/扩展落点：`crates/core/src/vault.rs`、`crates/domain/tests/password_safety.rs`、`apps/desktop-ui/src/components/VaultPage.tsx`、`apps/desktop-ui/tests/draft-lifecycle.test.tsx`。

#### DET-030 · P0 · DPAPI、备份恢复与旧格式兼容

- 状态：**未执行**；负责人：rust-dev / qa-inspector；环境：本机；隔离第二账户扩展；方式：自动 / 原生。
- 输入/结果前置：DET-029；必须通过：通用输入/对象归属核验，其他见子场景。
- 命令：CMD-07。
- 依据：账户绑定库与独立备份口令有不同迁移边界，不能把便携EXE误认为密码库跨账户可移动。
- 输入：旧/新合成vault、正确/错误备份口令、截断/篡改/超长口令、过期/重放恢复预览、第二测试账户。

操作：

1. 同账户读写、重启、备份导出/预览/合并；验证每次失败前后库和磁盘均保持。
2. 跨账户直接解密应失败；使用独立备份口令在第二合成库恢复，核对密码、分组与禁用属性。
3. 检查文件ACL、原子写、备份文件完整性和老格式路径；不导出真实库或DPAPI密钥。

通过标准：

- 错误口令/篡改/过期确认不导入任何部分记录；合法备份完整恢复。
- 直接复制账户绑定vault不被宣称支持迁移；报告/普通文件无合成密码明文。

证据：`vault-backup-results.json`、`backup-integrity.json`、`account-boundary.json`、`file-acl.json`。

现有入口/扩展落点：`crates/core/src/vault.rs`、`crates/infrastructure/tests/vault_regressions.rs`、`crates/infrastructure/src/dpapi/vault.rs`。

#### DET-031 · P0 · 临时密码、成功关联与秘密清理

- 状态：**未执行**；负责人：rust-dev / ui-dev / worker-dev；环境：合成密码库；方式：自动 + 原生。
- 输入/结果前置：DET-009、DET-029；必须通过：通用输入/对象归属核验，其他见子场景。
- 命令：CMD-03、CMD-07、CMD-08。
- 依据：临时口令、不保存重试、passwordRevision 和迟到请求容易跨任务串用。
- 输入：公开合成secret、保存/不保存、内外层不同口令、尝试中编辑/删除密码、旧generation成功事件。

操作：

1. 真实加密包不保存重试后核对vault/SQLite/快照/标准流/结构日志/临时文件无该口令。
2. 切任务/关弹窗/取消/卸载组件/旧保存迟到，确认只清理本草稿且不清新输入。
3. 修改密码revision后注入旧成功事件；重命名同内容与同名换内容分别检查成功关联。

通过标准：

- 临时密码只用于授权的本次任务；没有落盘泄漏、串任务或过期成功记账。
- 前端和托管内存清理按可证明范围报告，不承诺所有物理内存副本擦除。

证据：`synthetic-secret-scan.json`、`revision-race-results.json`、`draft-ownership.json`。

现有入口/扩展落点：`crates/core/src/vault.rs`、`crates/core/src/coordinator.rs`、`apps/desktop-ui/tests/draft-lifecycle.test.tsx`、`apps/desktop-ui/src/components/BulkPasswordBar.tsx`。

#### DET-032 · P1 · 系统剪贴板历史、同步与清理归属

- 状态：**未执行**；负责人：ui-dev / qa-inspector；环境：原生桌面合成秘密；方式：原生 / 人工。
- 输入/结果前置：DET-031；必须通过：通用输入/对象归属核验，其他见子场景。
- 命令：需人工步骤/新增检测入口。
- 依据：写入排除历史/同步格式和序号绑定有代码实现，仍需实际Windows行为证据。
- 输入：合成密码A、随后复制普通文本B、重复复制和超时清理；隔离账户或明确授权的系统配置。

操作：

1. 通过真实UI复制，核对排除历史/云格式是否设置成功；记录实际系统设置，不擅自修改用户设置。
2. A复制后立刻复制B及再次复制A，等待旧定时器，核对不会清空新剪贴板。
3. 在隔离账户验证Win+V和具备环境时云同步；无环境标blocked，不读取用户现有秘密内容。

通过标准：

- 序号/归属保证旧计时器不清新内容；保护格式设置失败明确拒绝复制。
- 只对实际观测的历史/同步组合给结论，工具API成功不能替代系统结果。

证据：`clipboard-sequence-events.json`、`clipboard-policy-observation.md`、`synthetic-history-check.png`。

现有入口/扩展落点：`apps/desktop-host/src/clipboard.rs`、`apps/desktop-ui/src/components/VaultPage.tsx`。

#### DET-033 · P0 · 原生命名管道身份、协议负例与背压

- 状态：**未执行**；负责人：worker-dev / rust-dev；环境：隔离测试进程；方式：新增进程harness / 原生。
- 输入/结果前置：DET-005；必须通过：通用输入/对象归属核验，其他见子场景。
- 命令：CMD-08。
- 依据：双向PID校验、8 MiB帧、JSON深度32、request/operation/generation关联是源码约束，需真实进程负例。
- 输入：错误父/服务PID、单管道连通、v1/v2握手、空/重复ID、generation 0/旧代、无换行、分片UTF8、坏JSON、边界帧、慢消费者。

操作：

1. 真实双管道重跑v2，再对每个身份/长度/版本/顺序负例注入并记录拒绝。
2. 事件背压、失联、仅心跳无业务进展下触发取消/总期限；同时发旧cancel/exit和新请求。
3. 检查每次operation终态唯一，坏请求不污染后续新Worker会话。

通过标准：

- 未授权进程不能操作，旧代事件/控制不影响当前代。
- 输入和等待有界；背压不阻止Host最终超时终止并确认退出；普通日志无秘密。

证据：`pipe-negative-cases.json`、`wire-events-redacted.jsonl`、`deadlines.json`、`exit-proof.json`。

现有入口/扩展落点：`src/OpenUnpack.Worker/WorkerHost.cs`、`src/OpenUnpack.Worker/Protocol.cs`、`crates/core/src/worker.rs`、`scripts/verify-worker-v2.ps1`。

#### DET-034 · P0 · Worker生命周期、防护与二进制解析

- 状态：**未执行**；负责人：rust-dev / worker-dev；环境：本机 / 隔离进程；方式：自动 / 原生故障注入。
- 输入/结果前置：DET-033；必须通过：通用输入/对象归属核验，其他见子场景。
- 命令：CMD-04、CMD-07、CMD-08。
- 依据：WriteScopeGuard 是应用约束，JobObject/mitigation 是另外的机制；缺失/替换Worker或DLL不能落到不可信路径。
- 输入：worker/7z.dll缺失/篡改、错误manifest、失效PID创建时间、崩溃、拒绝Job限制/mitigation、派生子进程和外层future丢弃。

操作：

1. 直接读取真实进程Job内存/ActiveProcessLimit/mitigation/权限状态，检查设置或读回失败的生产处理。
2. 测试异常退出、启动半失败、Host强杀、父future丢弃与未确认退出预留；核对PID+创建时间而非仅进程名。
3. 从候选目录启动并放置CWD/PATH假DLL，确认解析绑定打包路径且Release不回退源树。

通过标准：

- Host隔离Worker失败，未知退出不复用额度/目录；没有遗留本任务Worker。
- 缺防护/错组件明确失败，无不可信加载回退；不夸大为对抗任意恶意原生DLL的完整沙箱。

证据：`job-policy-readback.json`、`mitigation-readback.json`、`binary-resolution.json`、`process-lifecycle.jsonl`。

现有入口/扩展落点：`crates/core/src/worker.rs`、`crates/core/tests/worker_client.rs`、`src/OpenUnpack.Engine/WorkerSandbox.cs`、`apps/desktop-host/src/lib.rs`。

#### DET-035 · P0 · 工具箱预览令牌与异步归属

- 状态：**未执行**；负责人：rust-dev / ui-dev；环境：合成目录；方式：自动 + 原生GUI。
- 输入/结果前置：DET-016、DET-005；必须通过：通用输入/对象归属核验，其他见子场景。
- 命令：CMD-03、CMD-07。
- 依据：工具箱会重命名/移动/清理真实对象，执行必须绑定用户看到的有效预览。
- 输入：一次性token、过期/重放/空token、参数变化、源/目标替换、快速换选项、同requestId替换与旧取消。

操作：

1. 预览后改变每个关键字段和对象身份，尝试执行；另测合法执行后重放。
2. 快速改变UI选项、取消/关闭和同ID新请求，让旧结果/旧清理迟到。
3. 执行中锁冲突/权限失败/大目录预算，检查每项诊断、部分动作清单和可操作恢复。

通过标准：

- 旧/过期/被篡改计划不能改变文件，合法token只能消费一次。
- loading不永久挂起，旧结果不授权新选项；不覆盖/误删外部对象，结果按项可核对。

证据：`toolbox-token-matrix.json`、`preview-race-events.jsonl`、`toolbox-before-after.json`。

现有入口/扩展落点：`crates/core/src/toolbox.rs`、`apps/desktop-ui/src/components/ToolboxPage.tsx`、`apps/desktop-ui/tests/toolbox-preview.test.tsx`、`crates/core/src/coordinator.rs`。

### F．真实桌面用户流程

#### DET-036 · P0 · Explorer拖放与原生选择导入

- 状态：**未执行**；负责人：ui-dev / qa-inspector；环境：真实生产EXE；方式：原生GUI / 人工。
- 输入/结果前置：DET-001、DET-004；必须通过：DET-001。
- 命令：CMD-13。
- 依据：原生文件/文件夹选择已有记录；上轮跨窗口拖放被工具在输入前拒绝，既非通过也非应用失败。
- 输入：文件/文件夹/多选、深目录、分卷中间卷、重复输入、长中文路径、受限目录和空目录。

子场景前置：

- 普通ZIP文件/文件夹导入只需DET-004同源码检测构建和合成样本，不要求全部基线或分卷矩阵先通过。
- 分卷拖入只需DET-008对应样本与归组预期；缺某种格式仅阻塞该子场景。

操作：

1. 先核对实际启动EXE路径/Hash、UI摘要、独立数据目录及实际Worker子进程路径/Hash均匹配DET-004构建清单；如启动工具未继承测试目录环境，禁止默认使用个人数据目录。
2. 从真实Explorer分别拖入文件、多个文件与包含归档的文件夹；再走原生文件/文件夹对话框。
3. 覆盖扫描进度、排除预览、确认/取消、重复导入、扫描中换来源和关闭窗口。
4. 开始真实解压后逐文件核对内容；自动化无法跨窗口时由人工按相同步骤执行并留证。

通过标准：

- 扫描/确认/取消/入队数与样本清单一致，不漏分卷、不双入队、取消不入队。
- 证据来自实际EXE和Explorer；浏览器预览、mock或工具拒绝不能核销。

证据：`native-import-recording-or-steps.md`、`native-import-screenshots/`、`queue-input-reconciliation.json`、`output-oracle.json`。

现有入口/扩展落点：`apps/desktop-host/src/lib.rs`、`apps/desktop-ui/src/api.ts`、`apps/desktop-ui/src/components/QueuePage.tsx`、`scripts/native-file-dialog.ps1`。

#### DET-037 · P0 · 真实按钮的暂停、取消、删除和恢复

- 状态：**未执行**；负责人：ui-dev / qa-inspector；环境：真实生产EXE；方式：原生GUI。
- 输入/结果前置：DET-001、DET-004、DET-024；必须通过：通用输入/对象归属核验，其他见子场景。
- 命令：CMD-13。
- 依据：上一轮GUI没有完整点击删除/暂停/取消，前端与Core回归不能替代按钮全链。
- 输入：至少5项并发+2项排队、实际有进度的任务、等待资源/缺密码/待核对/已完成混合队列。

子场景前置：

- 只需同源码Host、可正常导入的ZIP合成队列与DET-024控制预期；完整拖放/格式矩阵通过不是前置。
- 若某动作已证实会误删，仅停止对应危险场景并保全失败，其他独立按钮继续。

操作：

1. 依次点击开始、暂停、恢复、单项取消、批停；必须在真实进度后触发并检查未选任务。
2. 删除运行项与历史项，取消删除对话框再确认；查看记录并重启验证不复活。
3. 对恢复/严格核对/清理按钮逐状态测试，保存点击到UI反馈与后端终态时间。

通过标准：

- 按钮可用性与后端状态一致；暂停不再领取，取消只影响目标。
- 原包和已发布成品保持，退出证明与数据状态一致；删除不复活，无无反馈误操作。

证据：`native-controls-steps.md`、`native-control-timestamps.json`、`pid-exits.json`、`restart-records.json`、`screenshots/`。

现有入口/扩展落点：`apps/desktop-ui/src/components/QueuePage.tsx`、`apps/desktop-ui/src/components/HistoryPage.tsx`、`apps/desktop-ui/src/components/TaskDeleteDialog.tsx`、`apps/desktop-ui/tests/task-deletion.test.tsx`。

#### DET-038 · P1 · 密码库与临时重试的原生全流程

- 状态：**未执行**；负责人：ui-dev / qa-inspector；环境：真实EXE / 合成vault；方式：原生GUI。
- 输入/结果前置：DET-004、DET-029、DET-031；必须通过：通用输入/对象归属核验，其他见子场景。
- 命令：CMD-13。
- 依据：密码流程已有组件与部分原生记录，需覆盖导入/备份/批量动作与草稿竞争的完整操作。
- 输入：公开测试口令、TXT/CSV/TSV、不同组、禁用项、错误/正确备份口令、内外包不同口令。

子场景前置：

- 同账户备份/恢复GUI仅需DET-030同账户样本和预期，不依赖第二账户或云同步验收。
- 第二账户迁移属于DET-030独立子场景，缺该环境不阻塞本项核心流程。

操作：

1. 从界面新增/编辑/搜索/分组/启用停用/批量导入去重/导出恢复并核对真实库。
2. 缺密码任务不保存重试与保存后重试各执行；错误口令、取消、切任务、重开弹窗逐项检查。
3. 在操作途中发起新草稿，让旧原生选择器/保存迟到，观察焦点和内容归属。

通过标准：

- 用户看到的预览、计数、组与后端一致；真实加密结果正确。
- 新草稿不被旧操作覆盖；临时口令不落库，错误恢复无半导入。

证据：`native-vault-workflow.md`、`vault-before-after-digests.json`、`draft-screenshots/`、`secret-scan.json`。

现有入口/扩展落点：`apps/desktop-ui/src/components/VaultPage.tsx`、`apps/desktop-ui/src/components/BulkPasswordBar.tsx`、`apps/desktop-ui/tests/workflows.test.tsx`。

#### DET-039 · P1 · 归档浏览、详情、进度和错误视图

- 状态：**未执行**；负责人：ui-dev / qa-inspector；环境：真实生产EXE；方式：原生GUI。
- 输入/结果前置：DET-028、DET-033；必须通过：通用输入/对象归属核验，其他见子场景。
- 命令：需人工步骤/新增检测入口。
- 依据：常态快照、懒加载详情和独立浏览Worker相互作用，易出现过期结果、重置或进度虚假。
- 输入：大目录树、未知大小LZ4、同名内包、缺密码/损坏/资源等待/不支持、分页和迟到详情。

操作：

1. 持续进度时浏览归档，快速切源/关闭/重开；选择节点、滚动、展开状态不应无故重置。
2. 核对阶段、字节、百分比、速度/ETA、品质与验证范围；未知总量不伪造完成百分比。
3. 历史页切换/搜索/详情/失败重试及后端失联，验证错误信息与可执行动作。

通过标准：

- 旧详情不显示到新源；浏览取消释放对应Worker，不阻塞队列控制。
- 错误/部分/损坏状态明显，进度与后台阶段一致；失败保留可读上下文并能重试。

证据：`native-browser-events.jsonl`、`ui-backend-state-matrix.json`、`progress-screenshots/`。

现有入口/扩展落点：`apps/desktop-ui/src/components/ArchiveBrowser.tsx`、`apps/desktop-ui/src/components/TransferMetrics.tsx`、`apps/desktop-ui/src/components/HistoryPage.tsx`、`apps/desktop-ui/tests/archive-browser.test.tsx`。

#### DET-040 · P1 · DPI、多显示器、键盘与读屏

- 状态：**未执行**；负责人：ui-dev / qa-inspector；环境：真实桌面 / 对应显示设备；方式：人工GUI。
- 输入/结果前置：DET-036；必须通过：通用输入/对象归属核验，其他见子场景。
- 命令：需人工步骤/新增检测入口。
- 依据：DPI/多屏/键盘仍是未验收范围，不能仅凭组件样式判断。
- 输入：100/125/150/200% DPI、常见窗口尺寸、多屏不同缩放、中文IME、键盘、可用读屏工具。

操作：

1. 逐页检查导航、表格、弹窗、长路径/错误文案；跨屏移动和最小窗口下操作关键按钮。
2. 仅键盘完成导入、确认、密码输入、取消/删除；核对Tab顺序、焦点恢复、Esc与Enter。
3. 检查深浅色/跟随系统、对比度和读屏标签；设备缺失项目明确blocked。

通过标准：

- 关键按钮和风险提示不遮挡，表格有可用滚动，密码不会意外展示。
- 键盘和焦点操作可完成核心流程；错误及状态不仅靠颜色表达。

证据：`dpi-device-matrix.json`、`native-accessibility-checklist.md`、`screenshots-by-scale/`。

现有入口/扩展落点：`apps/desktop-ui/src/App.tsx`、`apps/desktop-ui/src/components/shared.tsx`、`apps/desktop-ui/src/components/InterfaceBoundary.tsx`。

#### DET-041 · P1 · 托盘、关闭、重启和现有设置生效

- 状态：**未执行**；负责人：ui-dev / rust-dev / qa-inspector；环境：真实EXE / 合成设置；方式：原生GUI。
- 输入/结果前置：DET-024、DET-025、DET-038；必须通过：通用输入/对象归属核验，其他见子场景。
- 命令：需人工步骤/新增检测入口。
- 依据：只检查已经实现且含义明确的设置，不新增未约定设置；进程退出与关闭到托盘必须区分。
- 输入：当前支持的主题/置顶/托盘/提示音/并发/缓存/校验/输出策略；运行/等待/结束三类任务。

操作：

1. 在测试数据目录保存现有设置，核对重启保持及后端实际消费；auto/always和cache关闭分别观察。
2. 最小化、关闭到托盘、恢复、托盘退出、再次启动；检查Host/Worker、后台任务及数据租约。
3. 处理中退出/系统睡眠唤醒/会话恢复后检查状态、原包、工作区和临时口令。

通过标准：

- 界面显示、持久化值与实际行为一致；窗口关闭和退出有清楚反馈。
- 真正退出后本任务Host/Worker和租约释放，重启恢复准确，没有静默双运行。

证据：`settings-effect-matrix.json`、`tray-lifecycle.jsonl`、`restart-content.json`。

现有入口/扩展落点：`apps/desktop-host/src/lib.rs`、`apps/desktop-ui/src/components/SettingsPage.tsx`、`crates/core/src/coordinator.rs`。

#### DET-042 · P1 · 错误可操作性与桌面边界恢复

- 状态：**未执行**；负责人：ui-dev / qa-inspector；环境：真实EXE / 隔离故障；方式：原生GUI。
- 输入/结果前置：DET-026、DET-034、DET-039；必须通过：通用输入/对象归属核验，其他见子场景。
- 命令：需人工步骤/新增检测入口。
- 依据：后台分类正确也可能被界面吞掉，用户需知道发生阶段、保留内容和下一步。
- 输入：Worker缺失、引擎错误、输入移动、目标占用、无权限、坏配置、数据库不可写、网络输出失联及UI渲染错误。

操作：

1. 在副本候选/合成数据制造错误，记录从操作到界面消息/任务详情的映射。
2. 执行消息提供的重试、改目录、补密码、恢复/核对动作，并验证是否真正恢复。
3. 检查InterfaceBoundary、空态/加载/错误态和日志脱敏；不能依靠开发者控制台完成普通恢复。

通过标准：

- 不无限loading、不假成功；错误说明能定位阶段并给出有效操作。
- 失败不丢旧数据/输入，关键错误可复制去敏诊断；普通用户无需开发工具可理解处理。

证据：`native-error-action-matrix.json`、`screenshots/`、`recovery-outcomes.json`。

现有入口/扩展落点：`apps/desktop-ui/src/components/InterfaceBoundary.tsx`、`apps/desktop-ui/src/components/QueuePage.tsx`、`apps/desktop-ui/src/components/HistoryPage.tsx`、`apps/desktop-host/src/lib.rs`。

### G．性能、资源与故障环境

#### DET-043 · P1 · 并发、物理盘仲裁与完整吞吐

- 状态：**未执行**；负责人：qa-inspector / rust-dev；环境：明确拓扑的SSD/HDD；其他设备扩展；方式：原生性能测量。
- 输入/结果前置：DET-004、DET-025；必须通过：通用输入/对象归属核验，其他见子场景。
- 命令：CMD-08。
- 依据：本机曾测并发5与HDD/SSD，缓存样本更慢；结果不能推广到所有包或设备。
- 输入：并发1/2/3/4/5，10任务/档；ZIP小文件/大文件、7z大字典，直写/auto/always，同物理盘多卷与不同盘。

操作：

1. 编译完成后单独测，先核对物理盘ID和源/目标映射；运行显式ignored负载与物理盘入口。
2. 每档预热1次、测量3次，轮换顺序，记录缓存热度/全机负载；计时包含复制→解压→回写→选定校验。
3. 独立核对所有成品/源不变、Worker峰值和盘许可；不关闭系统保护或清空全机缓存来制造倍速。

通过标准：

- 有完整P50/P95/吞吐/IO/CPU/内存和样本说明；无同HDD错误并行、饥饿或内容错误。
- 相同机器/样本性能回退>20%触发调查，属于本计划初始调查阈值；缓存只按实测建议。

证据：`topology.json`、`measurements/`、`benchmark-summary.json`、`independent-output-check.json`。

现有入口/扩展落点：`scripts/measure-load.ps1`、`crates/core/tests/load_profile.rs`、`crates/core/tests/physical_disk_cross_device.rs`、`crates/core/src/route.rs`。

#### DET-044 · P0 · 全入口共享内存与低资源准入

- 状态：**未执行**；负责人：rust-dev / worker-dev / qa-inspector；环境：受限测试进程/VM；方式：自动 / 原生资源测量。
- 输入/结果前置：DET-024、DET-034；必须通过：通用输入/对象归属核验，其他见子场景。
- 命令：CMD-07、CMD-08。
- 依据：Worker池不等于Host/WebView或全应用内存上限；浏览/预览也应进入统一准入。
- 输入：解压+probe+inspect+规则预览混合，并发1..5；大字典、低commit余量、启动失败、未知退出和等待取消。

操作：

1. 记录各入口预留、实际Job读回与退出，覆盖64 KiB对齐及额度缩减边界。
2. 在隔离VM或有界注入资源探测，不耗尽用户整机；记录Host/WebView/全部归属Worker与系统commit。
3. 检查FIFO、取消等待、未确认退出保留预留、PID重用与回调释放。

通过标准：

- 所有Worker入口共享预算，真实上限可读回；错误或取消不泄漏额度。
- 资源探测失败不绕过保护；Host/WebView实测单列，不把池限额声称为整机硬限制。

证据：`resource-pool-timeline.jsonl`、`job-memory-readback.json`、`process-tree-memory.csv`、`admission-failures.json`。

现有入口/扩展落点：`crates/core/src/worker.rs`、`crates/core/tests/worker_client.rs`、`crates/core/tests/load_profile.rs`。

#### DET-045 · P1 · 历史规模、真实IPC与窗口响应

- 状态：**未执行**；负责人：ui-dev / qa-inspector / rust-dev；环境：真实EXE；方式：原生性能测量。
- 输入/结果前置：DET-028、DET-039；必须通过：通用输入/对象归属核验，其他见子场景。
- 命令：CMD-08。
- 依据：现有1000条基准只测Core snapshot+JSON，不覆盖启动全量历史、Tauri传输和WebView渲染。
- 输入：10/1000/10000条标记为未执行的合成历史；1000活跃摘要；宽/深节点树，10万历史为扩展压力。

操作：

1. 分开测启动加载、首屏、IPC字节/频率、分页搜索详情和实际窗口点击反馈；合成历史不算解压成功。
2. 每场景预热20次、测至少100次交互，记录P50/P95/P99、主进程/WebView/Worker内存。
3. 进度事件持续流入时滚动/搜索/选节点；比较有限快照、可恢复记录和启动全量加载成本。

通过标准：

- 首轮暂定调查阈值：1000条普通历史首屏≤3s，分页/搜索反馈P95≤500ms；10000条超过1s的交互列P1调查。
- 阈值需在执行前冻结并记录硬件；1000+活跃/可恢复项单列，不强行断言队列恒定20条。

证据：`startup-timing.json`、`ipc-payloads.json`、`interaction-latencies.csv`、`process-memory.json`、`render-observation.md`。

现有入口/扩展落点：`crates/core/src/coordinator.rs:412`、`apps/desktop-ui/src/api.ts`、`apps/desktop-ui/src/components/QueuePage.tsx`、`apps/desktop-ui/src/components/HistoryPage.tsx`。

#### DET-046 · P1 · 取消延迟、长时稳定性与句柄泄漏趋势

- 状态：**未执行**；负责人：qa-inspector / rust-dev / ui-dev；环境：本机合成负载；方式：原生耐久测试。
- 输入/结果前置：DET-024、DET-034；必须通过：通用输入/对象归属核验，其他见子场景。
- 命令：CMD-08、CMD-13。
- 依据：单次118ms后端取消记录不是GUI延迟或长期无泄漏证明。
- 输入：至少100次导入/浏览/取消/重试/删除循环，8小时有界队列，睡眠/唤醒与窗口开关。

子场景前置：

- 本机SSD有界耐久与后端取消不依赖物理HDD；DET-043缺HDD仍可执行。
- GUI反馈延迟只需DET-037对应按钮与同源码Host；HDD/特殊盘扩展独立判断。

操作：

1. 分别量点击反馈、请求发出、目标Worker退出和终态落盘，至少30个真实进度后取消样本。
2. 每分钟采集CPU/私有内存/句柄/线程/Worker/保留缓存；预热后各100轮空闲检查点比较。
3. 8小时内约束最大输出/磁盘余量，停止后核对所有仍保留对象和进程归属。

通过标准：

- 初始调查阈值：点击反馈P95≤300ms，Worker退出P95≤2s且最差≤10s；原有更严格用例期限仍保留。
- 无持续无界增长/遗留Worker/重复终态；空闲内存或句柄持续单调增长即调查，不能只凭一次GC峰值判泄漏。

证据：`cancel-latencies.csv`、`soak-timeseries.csv`、`idle-checkpoints.json`、`final-content-and-process-audit.json`。

现有入口/扩展落点：`crates/core/tests/load_profile.rs`、`crates/core/tests/worker_client.rs`、`apps/desktop-ui/src/api.ts`、`src/OpenUnpack.Engine/Native/SevenZipEngine.cs`。

#### DET-047 · P0 · 专用VHD满盘、脱机和写失败恢复

- 状态：**未执行**；负责人：qa-inspector / rust-dev / worker-dev；环境：专用限容VHD/VM；方式：隔离故障测试。
- 输入/结果前置：DET-022、DET-025；必须通过：通用输入/对象归属核验，其他见子场景。
- 命令：需人工步骤/新增检测入口。
- 依据：本机配额和强杀不能替代真实文件系统ENOSPC/脱机；不对用户正在使用的盘填满或卸载。
- 输入：全新专用有容量上限VHD，合成原包/缓存/目标/数据库分别放入；解压、复制、回读、commit、清理故障点。

操作：

1. 登记VHD文件、卷GUID、挂载点和归属；只在专用卷用有界填充触发满盘，在隔离VM脱机。
2. 记录错误时点与真实Win32码，释放空间/重新联机后重启/恢复/重试。
3. 对每次故障核对源、既有输出、publication、缓存计量和目标Worker退出。

通过标准：

- 不假成功、不误删；已提交输出保持；未知退出工作区不清理。
- 恢复幂等、可诊断；没有专用环境则blocked，不用mock替代该环境验收。

证据：`vhd-ownership.json`、`fault-point-results.json`、`win32-errors.json`、`recovery-content-manifests.json`。

现有入口/扩展落点：`crates/core/tests/release_reliability.rs`、`crates/core/src/coordinator.rs`、`src/OpenUnpack.Worker/WorkerOperations.cs`。

#### DET-048 · P1 · 隔离SMB、USB与特殊文件系统故障

- 状态：**未执行**；负责人：qa-inspector / rust-dev；环境：隔离共享/专用可移除介质；方式：环境验收。
- 输入/结果前置：DET-001、DET-017、DET-022；必须通过：通用输入/对象归属核验，其他见子场景。
- 命令：需人工步骤/新增检测入口。
- 依据：固定NTFS本机结果不能外推NAS/UNC别名、映射盘、FAT/exFAT或设备移除。
- 输入：专用SMB共享、UNC与映射盘别名、专用USB/测试VHD，源/缓存/目标三个位置分别断连。

子场景前置：

- SMB只需本项专用共享，不依赖DET-047的VHD可用或通过。
- USB、exFAT/特殊ACL、跨卷各自需要明确设备/样本及DET-018/020对应预期；按子场景blocked，互不串联。

操作：

1. 在隔离共享断开测试会话/停服务，USB只在专用无用户数据设备按测试环境步骤移除。
2. 覆盖复制/回读/发布/源清理中断及重连后恢复；核对盘身份变化和共享仲裁。
3. 记录不支持ACL/ADS/句柄语义时的拒绝；真实拔盘/物理掉电仅另有专用环境时执行。

通过标准：

- 无误覆盖/误删/假成功，失败状态与诊断准确；恢复不读取换盘后的非授权对象。
- 每种介质/协议分别出结论；缺环境明确blocked，不以C/D跨卷冒充设备故障。

证据：`device-share-topology.json`、`disconnect-events.jsonl`、`reconnect-recovery.json`、`support-boundaries.md`。

现有入口/扩展落点：`crates/core/src/route.rs`、`crates/core/src/paths.rs`、`crates/core/src/saferecycle.rs`、`docs/s06-directory-identity-report-2026-10-15.md`。

### H．依赖、候选包、安装与结案

#### DET-049 · P0 · 复核随包运行时安全基线 DS-01

- 状态：**未执行**；负责人：worker-dev / qa-inspector；环境：本机 + 官方公告访问；方式：依赖与载荷审计。
- 输入/结果前置：DET-001；必须通过：通用输入/对象归属核验，其他见子场景。
- 命令：CMD-09。
- 依据：10/17 本地依赖报告记录随包10.0.12落后当时10.0.13安全补丁，未修复；本轮未重新联网核验公告，不宣称可利用性。
- 输入：实际Worker runtimeconfig、CoreLib/CoreCLR/MsQuic、准备脚本、材料名单、便携与安装包；执行当天官方安全发行元数据。

操作：

1. 读取实际分发组件身份，与执行当天官方运行时安全元数据和适用架构公告对照并保存快照。
2. 核对默认RuntimeFrameworkVersion、已审阅名单、notice摘要及两个候选包是否一致，分清SDK/NuGet/runtime。
3. 若旧版本仍分发，输出确定缺陷和修复验收要求；修复后须另冻结新完整载荷并复跑相关门禁。

通过标准：

- 当前分发runtime满足当日已审阅安全基线；零NuGet告警不能代替本项。
- 仅组件版本命中不称业务漏洞可利用；旧版本/来源不明/元数据不可得不得过发布门禁。

证据：`official-release-metadata.json`、`runtime-advisory-applicability.json`、`runtime-binary-identities.json`、`candidate-runtime-parity.json`。

现有入口/扩展落点：`scripts/prepare-desktop.ps1:3`、`scripts/collect-release-materials.mjs:49`、`artifacts/worker/OpenUnpack.Worker.runtimeconfig.json`、`docs/dependency-security-report-2026-10-17.md`。

#### DET-050 · P0 · 全依赖与扫描失败门禁 DS-02

- 状态：**未执行**；负责人：qa-inspector / worker-dev；环境：联网审计；离线仅记受限结果；方式：自动 + 适用性审阅。
- 输入/结果前置：DET-005、DET-049；必须通过：通用输入/对象归属核验，其他见子场景。
- 命令：CMD-09。
- 依据：当前自动审计未动态覆盖自包含runtime，ProtocolChecks不在solution；CI npm生产扫描也不能代表开发依赖全覆盖。
- 输入：Cargo.lock及Windows可达图、npm全部依赖、四个csproj、runtime、7-Zip、实际分发文件与公告库快照。

操作：

1. 保存未抑制Cargo/npm/NuGet原始JSON与退出码，再审阅平台适用性；7-Zip固定资产摘要和版本单独核对。
2. 注入过期runtime+零NuGet、缺项目、坏JSON/缺响应组、网络失败、过期库/缺工具等负例。
3. 对DS-02缺少自动检查的路径立检测基础设施任务；执行前不伪造已存在命令。

通过标准：

- 四项目与实际runtime/原生DLL都有来源/时点；未知、离线跳过、解析错误不汇总为通过。
- 所有忽略项有当前目标不可达或正式适用性证据；扫描零记录只表明该快照没有匹配公告。

证据：`cargo-audit-raw.json`、`windows-reachability.json`、`npm-audit-all.json`、`nuget-four-projects.json`、`runtime-and-7zip.json`、`negative-policy-results.json`。

现有入口/扩展落点：`scripts/audit-security.ps1`、`scripts/security-audit-policy.ps1`、`scripts/test-security-audit-policy.ps1`、`tests/OpenUnpack.ProtocolChecks/OpenUnpack.ProtocolChecks.csproj`、`.github/workflows/windows.yml`。

#### DET-051 · P0 · 新候选包与最终源码逐项对应

- 状态：**未执行**；负责人：qa-inspector / rust-dev / worker-dev；环境：隔离构建副本 / 本机；方式：构建与解包审计。
- 输入/结果前置：DET-004、DET-049、DET-050；必须通过：DET-004、DET-049、DET-050。
- 命令：CMD-10、CMD-08。
- 依据：Host版本号或Worker.exe apphost哈希不能证明Engine/runtime新鲜；打包脚本有固定输出目录。
- 输入：最终源码冻结、已审阅安全runtime、Host+全部worker/runtime、7z、release-materials、标准/便携候选。

子场景前置：

- 本项指修复后拟发行的新候选，最终基线和安全依赖门禁必须通过。检测当前已知缺陷版本的GUI用CMD-13，不等待本项。

操作：

1. 先保全旧artifacts/worker/release/materials，在包含脏源码的隔离副本构建，不从干净HEAD丢掉改动。
2. 解包NSIS和ZIP逐文件核对；如存在Tauri bundle类型差异，只接受已证实唯一偏移，签名后另按实际签名核验。
3. 两个包内Worker分别跑v2，实际EXE用独立数据目录冒烟，Release缺依赖不回退源树。

通过标准：

- 源码、Host、Engine、runtime、7z、材料与当前候选一致，错误/混入旧载荷能被检测。
- 新包有独立SHA清单和来源证据，旧包完整保留；构建通过不冒充GUI/安装验收。

证据：`candidate-files.json`、`unpacked-payload-manifests.json`、`source-package-parity.json`、`package-native-v2.json`、`SHA256SUMS.txt`。

现有入口/扩展落点：`scripts/build-desktop.ps1`、`scripts/prepare-desktop.ps1`、`scripts/collect-release-materials.mjs`、`apps/desktop-host/tauri.conf.json`。

#### DET-052 · P1 · Windows干净机与WebView2安装矩阵

- 状态：**未执行**；负责人：qa-inspector；环境：Windows 10/11 x64干净VM；方式：原生环境验收。
- 输入/结果前置：DET-051；必须通过：DET-051。
- 命令：需人工步骤/新增检测入口。
- 依据：开发机已有SDK和WebView2，不能证明普通用户首次运行；标准包与离线包路径不同。
- 输入：Windows10/11代表版本、标准用户、无Node/.NET SDK；WebView2已有/缺失、联网/离线，标准/离线NSIS与便携包。

操作：

1. 每个基础镜像保存快照、OS补丁与运行时清单；安装/启动候选，记录下载与缺依赖行为。
2. 标准包无WebView2且断网应清楚失败；离线包应不依赖联网完成前置安装；便携缺WebView2给出可操作提示。
3. 跑小型正常/加密/缺卷流程并核对源/成品/退出，不只看窗口出现。

通过标准：

- 无开发环境亦可按声明运行；所有依赖和联网要求与提示一致。
- 每个必需环境有实际结果；没有VM时该门禁blocked，本机解包不能替代。

证据：`clean-machine-matrix.json`、`vm-snapshots.json`、`install-and-launch-logs/`、`native-smoke-files.json`。

现有入口/扩展落点：`README.md`、`scripts/build-desktop.ps1`、`apps/desktop-host/installer.nsi`、`apps/desktop-host/tauri.conf.json`。

#### DET-053 · P1 · 安装、真实版本升级、卸载和回退

- 状态：**未执行**；负责人：qa-inspector / rust-dev；环境：同机独立产品 + 干净VM正式产品身份；方式：安装生命周期验收。
- 输入/结果前置：DET-026、DET-051；必须通过：DET-051。
- 命令：CMD-11。
- 依据：已有OpenUnpackAudit独立产品的本机安装/重复安装/卸载证据，不是正式产品跨版本验收。
- 输入：冻结旧版和新版候选；合成vault/历史/待恢复publication/已发布成品；数据目录与安装目录冲突样本。

子场景前置：

- 本机独立审计产品不依赖干净VM。正式产品跨版场景需要DET-052对应VM和快照，但其余环境格不必全通过。

操作：

1. 先复用独立产品检测脚本，匹配唯一ProductName、同一NSIS解包载荷及新EvidenceDirectory。
2. 在快照VM以正式产品身份安装旧版建合成数据，升级新版、重复安装、卸载保留数据并重新安装。
3. 回退前核对schema向后兼容；不兼容时通过旧版应用+匹配的合成数据快照一起恢复，拒绝旧版静默重写新schema。

通过标准：

- 升级和卸载按约定保留vault、历史、恢复状态、源与成品；程序文件无不当残留。
- 安装不落入数据目录或沿链接误清理；回退恢复的应用/数据版本匹配，不破坏新数据。

证据：`audit-install-results.json`、`production-upgrade-matrix.json`、`data-before-after.json`、`rollback-proof.json`。

现有入口/扩展落点：`scripts/verify-installer-isolated.ps1`、`apps/desktop-host/installer.nsi`、`crates/infrastructure/tests/migration_regressions.rs`。

#### DET-054 · P1 · 实际分发Host和Setup的正式签名

- 状态：**未执行**；负责人：qa-inspector / 发行负责人；环境：可信证书或签名服务；方式：正式发行门禁。
- 输入/结果前置：DET-051；必须通过：DET-051。
- 命令：CMD-11。
- 依据：既有Host/Setup为NotSigned，门禁正确失败；缺证书是发行条件缺口，不等同功能缺陷。
- 输入：正式发行者身份/指纹、最终便携Host、安装包内Host、最终Setup及每次签名后字节。

操作：

1. 先核对bundle处理完成，再由有权发行流程签名各实际分发对象；不创建自签证书冒充正式签名。
2. 执行Authenticode Valid、CodeSigning EKU、预期指纹与信任链检查，并对错指纹/未签名/篡改负例验证拒绝。
3. 签名后重新解包/核验实际Host并重算所有分发摘要；不沿用未签名SHA。

通过标准：

- 实际分发对象签名有效且匹配可信发行者，包内也通过检查。
- 证书/服务不可用则blocked，不能用16项合成策略检查或一次NotSigned预期失败替代发行通过。

证据：`authenticode-final.json`、`publisher-policy-negative-cases.json`、`signed-payload-manifests.json`、`SHA256SUMS-signed.txt`。

现有入口/扩展落点：`scripts/verify-release-signatures.ps1`、`scripts/test-release-validation.ps1`、`scripts/build-desktop.ps1`。

#### DET-055 · P1 · 源码重建、许可证、秘密与CI遗漏

- 状态：**未执行**；负责人：qa-inspector / scout；环境：源码ZIP隔离副本 / CI可用时；方式：自动 / 构建审计。
- 输入/结果前置：DET-051；必须通过：通用输入/对象归属核验，其他见子场景。
- 命令：CMD-10、CMD-12。
- 依据：本地打包通过不证明源码ZIP可重建或远程CI真实执行；新增计划不得混入个人数据。
- 输入：最终源码ZIP、Cargo/npm锁文件、7-Zip来源、runtime notice、Windows workflow、隔离工具链。

操作：

1. 从源码ZIP新目录按文档准备/构建并运行最小原生管线；核对必需文件和被误排除测试样本。
2. 对解出的源码运行audit-source，对材料运行audit-release-materials；不对含.git/artifacts的工作区根直接跑源码包审计。
3. 核对锁版本、公告扫描、完整测试/ignored分派、CI超时和失败证据保留；无远程运行证据时仅记本地模拟。

通过标准：

- 源码包可依声明重建并执行核心流程；无密码库、数据库、私钥、凭据或构建缓存。
- 许可证/来源文件逐项对应实际组件；CI配置存在与CI跑过分开记录。

证据：`source-rebuild.log`、`source-audit.json`、`materials-audit.json`、`ci-coverage-matrix.json`。

现有入口/扩展落点：`scripts/package-source.ps1`、`scripts/audit-source.mjs`、`scripts/audit-release-materials.mjs`、`.github/workflows/windows.yml`、`THIRD_PARTY_NOTICES.md`。

#### DET-056 · P0 · 统一结案、风险排序与修复闭环

- 状态：**未执行**；负责人：qa-inspector → 项目负责人；环境：全部可用证据；方式：独立复核 / 报告。
- 输入/结果前置：DET-006、DET-011、DET-021、DET-022、DET-037、DET-044、DET-049、DET-050；必须通过：通用输入/对象归属核验，其他见子场景。
- 命令：需人工步骤/新增检测入口。
- 依据：检测完成、缺陷修复和正式发行是不同结论；不能让大批通过数淹没数据安全失败与外部blocked。
- 输入：DET-001–055 每项及子场景结果，runId对应的输入摘要和原始证据；需求→用例→命令→证据映射。

操作：

1. 逐项核对计划任务实际执行/未执行/阻塞，不把依赖已完成等同本项已完成；其他任务未结束时本项不能结案。
2. 按严重度、可复现性、用户影响、根因置信度和修复范围输出缺陷；历史已修、当前失败、待复现、未覆盖分别列。
3. 修复另建明确任务，冻结修复版本，先重放失败样本再跑受影响与最终全量；由另一检查者复核后关闭。

通过标准：

- 每个pass有完整证据；数据丢失/误覆盖/秘密泄漏/假成功/不可恢复等零容忍问题未关闭不得判系统检测通过。
- 检测报告可完整交付但系统仍needs changes；本机通过与正式发布通过分别给结论，任何必需环境blocked保留。

证据：`requirements-evidence-matrix.json`、`findings.md`、`summary.json`、`release-blockers.md`、`regression-followups.json`。

现有入口/扩展落点：`docs/acceptance-checklist.md`、`docs/dependency-security-report-2026-10-17.md`、`docs/plans/2026-10-17-system-detection-tasks.json`。

## 可复用命令与前置条件

命令在执行时独立捕获stdout、stderr、起止时间、cwd、退出码与用例数；PowerShell脚本使用Stop语义，原生程序通过Invoke-OpenUnpackTool检查非零。扫描有发现而非零时仍保留完整JSON并由结果门禁分类。固定目录聚合脚本需先保全旧证据或移至包含当前脏源码的隔离构建副本；不把新一轮日志写回旧证据目录。

### CMD-01 · 新建独立PowerShell会话与runId

在仓库根目录的PowerShell 7 x64新会话执行；仅本命令创建检测目录和冻结载荷。先核对选择的artifacts/worker完整且属于计划被测版本。会话退出后环境变量随进程消失；不打印完整环境。

```powershell
. ./scripts/common.ps1
Assert-OpenUnpackWindows
Import-OpenUnpackMsvc
$taskRoot = (Get-Location).ProviderPath
$taskRunId = 'run-' + (Get-Date -Format 'yyyyMMdd-HHmmss') + '-' + [guid]::NewGuid().ToString('N')
$taskRun = Join-Path $taskRoot ('_workspace/system-detection-20261017/' + $taskRunId)
New-Item -ItemType Directory -Path $taskRun -ErrorAction Stop | Out-Null
foreach ($taskChild in @('logs','data','fixtures','results','contracts')) {
    New-Item -ItemType Directory -Path (Join-Path $taskRun $taskChild) -ErrorAction Stop | Out-Null
}
$taskWorkerSource = (Resolve-Path -LiteralPath 'artifacts/worker').ProviderPath
Copy-Item -LiteralPath $taskWorkerSource -Destination (Join-Path $taskRun 'payload') -Recurse -ErrorAction Stop
$env:OPENUNPACK_TEST_WORKER = Join-Path $taskRun 'payload/OpenUnpack.Worker.exe'
$env:OPENUNPACK_DATA_DIR = Join-Path $taskRun 'data'
$env:CARGO_TARGET_DIR = Join-Path $taskRun 'cargo-target'
$taskEngine = Join-Path $taskRun 'payload/7z.dll'
if (-not (Test-Path -LiteralPath $env:OPENUNPACK_TEST_WORKER -PathType Leaf)) { throw 'Frozen Worker missing' }
if (-not (Test-Path -LiteralPath $taskEngine -PathType Leaf)) { throw 'Frozen engine missing' }
```

### CMD-02 · 记录状态、工具与完整载荷摘要

源码manifest还要覆盖rg列出的所有实际构建输入及独立fixture oracle；下段展示载荷与基础身份。结果不得包含完整环境、凭据或个人密码。

```powershell
git status --porcelain=v1 | Set-Content -LiteralPath (Join-Path $taskRun 'git-status.txt') -Encoding utf8NoBOM
git rev-parse HEAD | Set-Content -LiteralPath (Join-Path $taskRun 'head.txt') -Encoding utf8NoBOM
git branch --show-current | Set-Content -LiteralPath (Join-Path $taskRun 'branch.txt') -Encoding utf8NoBOM
$taskInputPaths = @(& rg --files --hidden crates apps src scripts tests .github)
if ($LASTEXITCODE -ne 0) { throw 'Cannot list source inputs' }
$taskInputPaths += @('Cargo.toml','Cargo.lock','Directory.Build.props','global.json','OpenUnpack.sln','.editorconfig','.gitignore','LICENSE','THIRD_PARTY_NOTICES.md')
$taskSourceRows = @($taskInputPaths | Sort-Object -Unique | ForEach-Object {
    @{ path = $_.Replace('\','/'); sha256 = (Get-FileHash -LiteralPath $_ -Algorithm SHA256).Hash.ToLowerInvariant() }
})
$taskSourceRows | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath (Join-Path $taskRun 'source-manifest.json') -Encoding utf8NoBOM
$taskPayload = Join-Path $taskRun 'payload'
$taskManifest = @(Get-ChildItem -LiteralPath $taskPayload -File -Recurse | ForEach-Object {
    @{ path = [IO.Path]::GetRelativePath($taskPayload, $_.FullName); size = $_.Length
       sha256 = (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant() }
})
$taskManifest | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath (Join-Path $taskRun 'binary-manifest.json') -Encoding utf8NoBOM
Invoke-OpenUnpackTool cargo @('--version')
Invoke-OpenUnpackTool rustc @('--version')
Invoke-OpenUnpackTool node @('--version')
Invoke-OpenUnpackTool npm @('--version')
Invoke-OpenUnpackTool dotnet @('--info')
```

### CMD-03 · UI正式套件与独立audit套件

正式Vitest只include tests/**；audit/**须另执行。audit断言如已过时，保留原失败并标明原因；不直接删断言。build会生成UI编译产物。

```powershell
Push-Location (Join-Path $taskRoot 'apps/desktop-ui')
try {
    Invoke-OpenUnpackTool npm @('run','lint','--','--max-warnings=0')
    Invoke-OpenUnpackTool npm @('run','build')
    Invoke-OpenUnpackTool npm @('test','--','--maxWorkers=2')
    Invoke-OpenUnpackTool npx @('--no-install','vitest','run','--config','audit/vitest.config.ts','--maxWorkers=2')
} finally { Pop-Location }
```

### CMD-04 · .NET真实可执行harness

项目OutputType=Exe；dotnet test不会运行此套件。还需核对harness使用的Engine/工具路径，不能直接把源码harness结果当包内Worker结果。若无restore缓存先列环境缺口，再按锁定配置恢复依赖。

```powershell
Invoke-OpenUnpackTool dotnet @('build','tests/OpenUnpack.Tests/OpenUnpack.Tests.csproj','-c','Release','-p:UseSharedCompilation=false')
Invoke-OpenUnpackTool dotnet @('run','--project','tests/OpenUnpack.Tests/OpenUnpack.Tests.csproj','-c','Release','--no-build','--','.')
# 可选专项；完整套件中已包含，不能重复计入总数。
Invoke-OpenUnpackTool dotnet @('run','--project','tests/OpenUnpack.Tests/OpenUnpack.Tests.csproj','-c','Release','--no-build','--','.','--path-identity-only')
Invoke-OpenUnpackTool dotnet @('run','--project','tests/OpenUnpack.Tests/OpenUnpack.Tests.csproj','-c','Release','--no-build','--','.','--worker-boundary-only')
```

### CMD-05 · Rust格式、严格检查与完整工作区

独立CARGO_TARGET_DIR；构建和原生/性能测量串行。--offline缺缓存是环境阻塞，后续依赖获取不得改锁文件。先记录全部tests/ignored，实际断言数量由日志解析。

```powershell
Invoke-OpenUnpackTool cargo @('fmt','--all','--','--check')
Invoke-OpenUnpackTool cargo @('clippy','--workspace','--all-targets','--locked','--offline','--','-D','warnings')
Invoke-OpenUnpackTool cargo @('test','--workspace','--locked','--offline','--','--list')
Invoke-OpenUnpackTool cargo @('test','--workspace','--locked','--offline','--','--test-threads=1')
```

### CMD-06 · 结构、镜像、真实DTO往返和检测策略

直接复用check-contracts.ps1内部命令，以新corpus路径保存。原脚本默认corpus固定在旧artifacts目录；历史_workspace wrapper亦不可原地重跑。

```powershell
Invoke-OpenUnpackTool node @('scripts/check-contracts.mjs','--self-test')
# check-mirrors.mjs 已于 2026-10-21 退役，由 check-architecture.mjs 覆盖
& ./scripts/test-security-audit-policy.ps1
& ./scripts/test-release-validation.ps1
$taskCorpus = Join-Path $taskRun 'contracts'
Invoke-OpenUnpackTool cargo @('run','-p','openunpack-protocol','--example','contract_samples','--locked','--offline','--','export',$taskCorpus)
Invoke-OpenUnpackTool dotnet @('run','--project','tests/OpenUnpack.ProtocolChecks/OpenUnpack.ProtocolChecks.csproj','-c','Release','--',$taskCorpus)
Invoke-OpenUnpackTool cargo @('run','-p','openunpack-protocol','--example','contract_samples','--locked','--offline','--','verify',$taskCorpus)
Invoke-OpenUnpackTool cargo @('test','-p','openunpack-protocol','--test','golden_samples','--locked','--offline')
```

### CMD-07 · 生产管线、格式、退出与恢复专项

OPENUNPACK_TEST_WORKER绑定冻结载荷；部分引擎或fixture路径仍在源码树，需在输入manifest里列出。针对具体函数过滤前先--list并要求实际用例数>0；下面直接运行整个target。

```powershell
foreach ($taskTarget in @('desktop_pipeline','format_matrix','archive_inspection','worker_client','release_reliability','resource_recovery_regressions','scanner_regressions','output_rule_preview')) {
    Invoke-OpenUnpackTool cargo @('test','-p','openunpack-core','--test',$taskTarget,'--locked','--offline','--','--test-threads=1')
}
```

### CMD-08 · 包内Worker与默认ignored专项

v2脚本可传候选路径，生成唯一目录；v1 verify-worker.ps1当前硬编码源码Release路径，不能声称已测包内。负载脚本ReportRoot显式选择新目录；HDD/跨卷段需登记测试卷后单独执行。

```powershell
& ./scripts/verify-worker-v2.ps1 -WorkerExecutable $env:OPENUNPACK_TEST_WORKER -EnginePath $taskEngine
& ./scripts/measure-load.ps1 -Worker $env:OPENUNPACK_TEST_WORKER -Profile ssd -Levels @(1,2,3,4,5) -MiB 64 -FixtureParent (Join-Path $taskRun 'fixtures') -ReportRoot (Join-Path $taskRun 'measurements')
# controls、cpu、snapshots分别调用，测量期间不并行编译。
# hdd还需-HddParent：执行者事先赋值为专用HDD测试父目录。
# 物理盘用例硬编码本机D:假设；换机器先校准源码/夹具并重新冻结输入。
# $env:OPENUNPACK_PD_ROOT = 已登记且存在的专用测试父目录
# $env:OPENUNPACK_TOOLBOX_SECOND_VOLUME = 已登记且存在的第二测试卷父目录
# Invoke-OpenUnpackTool cargo @('test','-p','openunpack-core','--test','physical_disk_cross_device','--locked','--offline','--','--ignored','--test-threads=1','--nocapture')
# Invoke-OpenUnpackTool cargo @('test','-p','openunpack-core','--lib','cross_volume_transfer_uses_locked_source_and_preserves_existing_target','--locked','--offline','--','--ignored','--test-threads=1','--nocapture')
```

### CMD-09 · 依赖原始结果与随包运行时复核

先分别收集原始结果，即使某工具失败仍继续其他独立扫描，最后统一判门禁。scripts/audit-security.ps1固定写artifacts/verification，只在保全旧目录后的隔离构建副本调用并捕获异常。此入口仍缺DS-02检查；DET-049/050须补官方runtime/7-Zip核对。recorded仅表示命令已记录，绝非passed。

```powershell
$taskScanRows = [Collections.Generic.List[object]]::new()
function Invoke-DetectionScan {
    param([string]$Name, [string]$Command, [string[]]$Arguments)
    $taskStartedAt = [DateTimeOffset]::UtcNow.ToString('o')
    $taskOut = Join-Path $taskRun ($Name + '.json')
    $taskErr = Join-Path $taskRun ($Name + '-stderr.log')
    $taskExit = $null
    $taskToolError = $null
    try {
        & $Command @Arguments 1> $taskOut 2> $taskErr
        $taskExit = $LASTEXITCODE
    } catch {
        $taskToolError = $_.Exception.Message
        Add-Content -LiteralPath $taskErr -Value $taskToolError -Encoding utf8NoBOM
    }
    $taskScanRows.Add(@{ name=$Name; startedAt=$taskStartedAt; finishedAt=[DateTimeOffset]::UtcNow.ToString('o')
        exitCode=$taskExit; toolError=$taskToolError; stdout=$taskOut; stderr=$taskErr
        status=$(if ($null -eq $taskExit) {'blocked'} else {'recorded'}) })
}
Invoke-DetectionScan -Name 'cargo-audit-raw' -Command 'cargo' -Arguments @('audit','--json')
Invoke-DetectionScan -Name 'windows-metadata' -Command 'cargo' -Arguments @('metadata','--format-version','1','--locked','--offline','--filter-platform','x86_64-pc-windows-msvc')
Push-Location (Join-Path $taskRoot 'apps/desktop-ui')
try {
    Invoke-DetectionScan -Name 'npm-audit-all' -Command 'npm.cmd' -Arguments @('audit','--include=dev','--package-lock-only','--ignore-scripts','--registry=https://registry.npmjs.org','--json')
} finally { Pop-Location }
Invoke-DetectionScan -Name 'nuget-solution' -Command 'dotnet' -Arguments @('list','OpenUnpack.sln','package','--vulnerable','--include-transitive','--format','json')
Invoke-DetectionScan -Name 'nuget-protocol' -Command 'dotnet' -Arguments @('list','tests/OpenUnpack.ProtocolChecks/OpenUnpack.ProtocolChecks.csproj','package','--vulnerable','--include-transitive','--format','json')
# 原始结果均收集后再运行现有聚合器；失败保留，不截断上面的独立扫描。
$taskAggregateError = $null
try { & ./scripts/audit-security.ps1 -DenyWarnings *> (Join-Path $taskRun 'existing-security-aggregate.log') }
catch { $taskAggregateError = $_.Exception.Message }
@{ gateStatus='not-evaluated'; scans=$taskScanRows; aggregateError=$taskAggregateError } | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath (Join-Path $taskRun 'scan-exits.json') -Encoding utf8NoBOM
# DET-050随后校验JSON结构/结果组/公告时点/退出码并判定；缺失与失败不得放行。
Get-Content -LiteralPath (Join-Path (Split-Path -Parent $env:OPENUNPACK_TEST_WORKER) 'OpenUnpack.Worker.runtimeconfig.json')
```

### CMD-10 · 候选构建的执行前置

模板有明确前置：DET-049/050安全基线已审阅；已保全旧候选；在包含未提交源码的隔离构建副本；材料版本和许可校验同步完成。当前默认10.0.12不可直接当最终候选。该代码默认阻断，执行者必须在前置真正满足后替换标记。

```powershell
throw '模板前置未满足：先完成安全runtime审阅、旧候选保全与隔离构建副本；不要直接运行默认旧runtime打包。'
# 在前置满足的新构建会话中设置：
# $taskReviewedRuntime = 当日官方元数据核对过的具体10.0.x版本
# & ./scripts/prepare-desktop.ps1 -RuntimeFrameworkVersion $taskReviewedRuntime
# & ./scripts/build-desktop.ps1 -SkipPrepare -Installer
# 离线安装器需独立候选轮次：-OfflineInstaller；固定输出必须先封存。
# 重建后重新绑定并冻结Worker/Host；不能继续沿用CMD-01中的旧载荷。
```

### CMD-11 · 隔离安装与真实签名核验模板

参数由该轮已审阅载荷赋值；EvidenceDirectory必须在artifacts下且为空。安装脚本只接受OpenUnpackAudit-32位hex及匹配安装器，不能对普通生产包改参数硬跑；正式产品升级在VM另执行。

```powershell
# 模板变量必须由DET-053/054绑定，未定义时不得调用。
# $taskAuditSetup = 独立审计产品安装器的绝对路径
# $taskAuditProduct = 安装器实际ProductName：OpenUnpackAudit-<32hex>
# $taskExtractedPayload = 同一NSIS刚解出的完整安装载荷
# $taskInstallEvidence = artifacts/<runId>/installer-<unique>
# & ./scripts/verify-installer-isolated.ps1 -SetupExe $taskAuditSetup -ProductName $taskAuditProduct -EvidenceDirectory $taskInstallEvidence -ExpectedPayloadDirectory $taskExtractedPayload -PreserveSyntheticData
# $taskFinalHost / $taskFinalSetup / $taskExpectedThumbprint = 实际分发对象与可信发行者40位指纹
# $taskSignatureEvidence = artifacts/<runId>/signatures-<unique>
# & ./scripts/verify-release-signatures.ps1 -HostExe $taskFinalHost -SetupExe $taskFinalSetup -EvidenceDirectory $taskSignatureEvidence -ExpectedSignerThumbprint $taskExpectedThumbprint
# 对便携Host和从最终Setup解出的Host各核验；每次使用新的EvidenceDirectory。
```

### CMD-12 · 只对解出的源码与材料运行交付审计

源码审计会拒绝.git、artifacts等目录，不应对工作区根直接运行。需先定义本轮解出的源码/材料绝对路径并留存摘要。

```powershell
# $taskExtractedSource = 最终源码ZIP解出的独立源码根目录
# $taskExtractedMaterials = 候选包实际release-materials目录
# Invoke-OpenUnpackTool node @('scripts/audit-source.mjs',$taskExtractedSource)
# Invoke-OpenUnpackTool node @('scripts/audit-release-materials.mjs',$taskExtractedMaterials)
```

### CMD-13 · 当前源码的隔离检测Host与Worker构建

在CMD-02输入清单建立后、Rust原生套件和GUI前执行。构建目录均为本runId新目录；从当前C#源码发布Worker，保持所选基线runtime版本和冻结7z，不覆盖旧候选。这是有已知安全缺口时的隔离检测版本，不能替代DET-051安全修复后的最终候选。实际启动必须继承测试数据目录，并核对Host及真实子进程路径/Hash；Host不会读取OPENUNPACK_TEST_WORKER。

```powershell
$taskBaselineConfig = Get-Content -Raw -LiteralPath (Join-Path $taskRun 'payload/OpenUnpack.Worker.runtimeconfig.json') | ConvertFrom-Json
$taskBaselineRuntime = @($taskBaselineConfig.runtimeOptions.includedFrameworks | Where-Object name -eq 'Microsoft.NETCore.App')[0].version
if ($taskBaselineRuntime -notmatch '^10\.0\.\d+$') { throw 'Unexpected baseline runtime' }
$taskFreshPayload = Join-Path $taskRun 'payload-current-source'
New-Item -ItemType Directory -Path $taskFreshPayload -ErrorAction Stop | Out-Null
Invoke-OpenUnpackTool dotnet @('publish','src/OpenUnpack.Worker/OpenUnpack.Worker.csproj','-c','Release','-r','win-x64','--self-contained','true',('-p:RuntimeFrameworkVersion=' + $taskBaselineRuntime),'-o',$taskFreshPayload)
foreach ($taskFile in @('7z.dll','7zip-License.txt','7zip-provenance.json','THIRD_PARTY_NOTICES.md')) {
    Copy-Item -LiteralPath (Join-Path $taskRun ('payload/' + $taskFile)) -Destination $taskFreshPayload -ErrorAction Stop
}
$taskFreshConfig = Get-Content -Raw -LiteralPath (Join-Path $taskFreshPayload 'OpenUnpack.Worker.runtimeconfig.json') | ConvertFrom-Json
if (@($taskFreshConfig.runtimeOptions.includedFrameworks | Where-Object name -eq 'Microsoft.NETCore.App')[0].version -ne $taskBaselineRuntime) { throw 'Runtime changed unexpectedly' }
Invoke-OpenUnpackTool npm @('--prefix','apps/desktop-ui','run','build')
Invoke-OpenUnpackTool cargo @('build','-p','openunpack-desktop','--release','--features','custom-protocol','--locked','--offline')
$taskNativeRoot = Join-Path $taskRun 'desktop-current-source'
New-Item -ItemType Directory -Path $taskNativeRoot -ErrorAction Stop | Out-Null
$taskNativeHost = Join-Path $taskNativeRoot 'OpenUnpack.exe'
Copy-Item -LiteralPath (Join-Path $env:CARGO_TARGET_DIR 'release/openunpack-desktop.exe') -Destination $taskNativeHost -ErrorAction Stop
Copy-Item -LiteralPath $taskFreshPayload -Destination (Join-Path $taskNativeRoot 'worker') -Recurse -ErrorAction Stop
foreach ($taskFile in @('LICENSE','THIRD_PARTY_NOTICES.md')) { Copy-Item -LiteralPath $taskFile -Destination $taskNativeRoot -ErrorAction Stop }
$env:OPENUNPACK_TEST_WORKER = Join-Path $taskNativeRoot 'worker/OpenUnpack.Worker.exe'
$taskEngine = Join-Path $taskNativeRoot 'worker/7z.dll'
$taskUiRoot = Join-Path $taskRoot 'apps/desktop-ui/dist'
$taskUiManifest = @(Get-ChildItem -LiteralPath $taskUiRoot -File -Recurse | ForEach-Object {
    @{path=[IO.Path]::GetRelativePath($taskUiRoot,$_.FullName);sha256=(Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant()}
})
$taskNativeManifest = @(Get-ChildItem -LiteralPath $taskNativeRoot -File -Recurse | ForEach-Object {
    @{path=[IO.Path]::GetRelativePath($taskNativeRoot,$_.FullName);sha256=(Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant()}
})
$taskSourceDrift = @($taskSourceRows | Where-Object { -not (Test-Path -LiteralPath $_.path -PathType Leaf) -or (Get-FileHash -LiteralPath $_.path -Algorithm SHA256).Hash.ToLowerInvariant() -ne $_.sha256 })
@{sourceManifestSha256=(Get-FileHash -LiteralPath (Join-Path $taskRun 'source-manifest.json') -Algorithm SHA256).Hash.ToLowerInvariant();host=$taskNativeHost;dataDirectory=$env:OPENUNPACK_DATA_DIR;runtime=$taskBaselineRuntime;ui=$taskUiManifest;payload=$taskNativeManifest;inputDrift=$taskSourceDrift;scope='Detection build; actual launched Host and child identities still need native verification'} | ConvertTo-Json -Depth 7 | Set-Content -LiteralPath (Join-Path $taskRun 'native-host-build-manifest.json') -Encoding utf8NoBOM
if ($taskSourceDrift.Count -ne 0) { throw 'Build inputs drifted; review and re-freeze before GUI evidence' }
# 后续实际启动$taskNativeHost并证明其使用OPENUNPACK_DATA_DIR；不能让原生工具默认打开个人库。
# 观察实际Worker子进程必须来自$taskNativeRoot/worker，并核对PID+创建时间和完整载荷Hash。
```

## 证据格式与最终报告

单个任务拆成子场景，每个子场景独立状态。`exitCode:0`与`counts.passed:0`含义不同；未知值用null。

```json
{
  "runId": "<唯一轮次>",
  "taskId": "DET-011",
  "caseId": "DET-011-03",
  "status": "planned",
  "evidenceLevel": "nativeCoreWorker",
  "startedAt": null,
  "finishedAt": null,
  "environment": {
    "os": null,
    "filesystem": null,
    "topologyId": null,
    "toolVersions": {}
  },
  "inputs": {
    "sourceManifestSha256": null,
    "binaryManifestSha256": null,
    "fixtureManifestSha256": null
  },
  "command": null,
  "cwd": null,
  "exitCode": null,
  "counts": {
    "passed": null,
    "failed": null,
    "ignored": null,
    "skipped": null
  },
  "expected": [
    "Unsupported不得被CRC抢救吞掉"
  ],
  "observed": [],
  "assertions": [
    {
      "name": "sourceUnchanged",
      "passed": null,
      "evidence": null
    }
  ],
  "evidence": [],
  "limitations": [],
  "blocker": null,
  "findingId": null,
  "rerunOf": null
}
```

证据等级：unitMock / contract / nativeCoreWorker / nativeHost / package / externalMachine。每份结果给出环境、输入哈希、原始日志、独立文件核对、准确进程身份与局限；passed必须所有必需断言完成。故障现场/随机seed可复放。

最终 `summary.json` 至少包含任务/子场景planned、passed、failed、blocked、skipped数量；按严重度排序的finding；未覆盖场景；源码与载荷漂移；本机范围结论；正式发布门禁结论。报告不可只给通过总数。

缺陷严重度：阻断级包括误删/覆盖、外部写入、明文泄漏、假完整成功、不可恢复、重要安全基线缺口；高包括核心流程稳定复现失败/死锁；中包括可恢复功能错误/明显性能问题；低包括不误导结果的可用性或维护缺陷。每条另列根因置信度。P0/P1只表示检测顺序。

## 定义完成与交接

- 第一批可直接按DET-001→004/005/049→006/050执行；scout整理输入/调用图，worker-dev负责原生样本，rust-dev负责事务/IO/资源，ui-dev负责真实窗口，qa-inspector独立核证。DET-004用CMD-13从当前源码构建隔离检测Host/Worker；正式安全候选由DET-051单列。只读准备可并行，实际Rust/.NET构建、性能测量和GUI操作者串行。
- 预计第一轮发现与风险报告3–5个工作日；完整检测约8–12人日（环境就绪、无重大新增阻断时），另含8小时耐久墙钟。新增harness和复现难度会影响工期，修复与外部资源等待另算；不是发布日期承诺。
- 所有56项先保持planned。dependsOn默认要求核心场景已有输入/可审阅结果，只有requiresPassed要求真正通过；第二账户/HDD/VHD等扩展逐子场景阻塞。实际执行使用running/passed/failed/blocked/skipped；skipped须写理由，必需项skipped不能放行。DET-056只有全表及子场景终态汇齐才能结案，其dependsOn不是全量结案清单。
- 每个确认问题独立列findingId、触发条件、影响、严重度、根因置信度、最小复现、受影响版本、修复落点与回归；测试优先级P0/P1不是漏洞严重度。外部未验项目不冒充代码缺陷。
- 新增报告与样本归入新runId；计划只新增本目录HTML/Markdown/JSON。后续如修复涉及DTO/持久化/发行组件，先备份合成状态和旧候选，窄范围改动，重放失败样本，运行受影响套件并在最终冻结版本跑完整基线。
- 回退保持应用与数据版本匹配。检测夹具失败保留现场，不执行全盘清理；只删除本轮登记且核对绝对路径/归属的合成子目录。任何用户目录、旧证据、已发布成品与密码库都不在默认清理范围。

检测计划交付完成的条件是56项都有可执行设计、通过标准和证据要求；系统检测完成的条件是实际执行后所有子场景状态与证据齐全；系统检测通过还要求必需项无未关闭失败或blocked；正式发布通过另需干净机、生命周期和正式签名。当前仅达到计划交付阶段。

## 已审阅来源

- [AGENTS.md](../../AGENTS.md)：跨栈角色与/review协作范围；本轮scout、qa-inspector和Worker领域均只读。角色沿用项目分工，执行模型继承本会话，未调用Haiku/Sonnet/Gemini。
- [.pi/prompts/review.md](../../.pi/prompts/review.md)：并行范围与跨边界核对。计划产物使用本轮唯一目录，没有覆盖历史01_scope/01_qa文件。
- [README.md](../../README.md)：Windows原生交付、功能承诺、构建环境与首版范围。
- [docs/dependency-security-report-2026-10-17.md](../../docs/dependency-security-report-2026-10-17.md)：最近依赖复核、DS-01/DS-02和实际候选runtime身份。
- [docs/release-readiness-report-2026-10-15.md](../../docs/release-readiness-report-2026-10-15.md)：10/16补充后的全量/定向回归区别、GUI、包与环境边界。
- [docs/acceptance-checklist.md](../../docs/acceptance-checklist.md)：10/10草案与10/15–16补充并存，主表pending不能整体当当前缺陷。
- [docs/comprehensive-fixes-report-2026-10-13.md](../../docs/comprehensive-fixes-report-2026-10-13.md)：历史安全/调度/品质/LZ4整改与原始问题编号。
- [docs/s06-directory-identity-report-2026-10-15.md](../../docs/s06-directory-identity-report-2026-10-15.md)：NTFS对象身份真实证据及文件系统/旧句柄/迁移层边界。
- [docs/security-audit-2026-10-13.md](../../docs/security-audit-2026-10-13.md)：初始威胁模型与历史失败来源；状态须以后续整改覆盖。
- [_workspace/release-readiness-2026-10-15/cache-final-validation.json](../../_workspace/release-readiness-2026-10-15/cache-final-validation.json)：最后缓存修复后的定向命令退出码。
- [_workspace/release-readiness-2026-10-15/native-ui-validation.json](../../_workspace/release-readiness-2026-10-15/native-ui-validation.json)：实际已操作的窗口流程与未覆盖拖放/按钮。
- [_workspace/dependency-security-2026-10-17/summary.json](../../_workspace/dependency-security-2026-10-17/summary.json)：10/17原始扫描摘要与runtime版本记录。
- [apps/desktop-ui/src/api.ts](../../apps/desktop-ui/src/api.ts)：队列/历史/浏览/预览的真实invoke入口。
- [apps/desktop-host/src/lib.rs](../../apps/desktop-host/src/lib.rs)：Core Coordinator装配、数据目录覆盖和命令注册。
- [crates/core/src/coordinator.rs](../../crates/core/src/coordinator.rs)：任务状态、发布、恢复、缓存与启动历史加载。
- [crates/core/src/worker.rs](../../crates/core/src/worker.rs)：双管道、共享资源准入、JobObject和确认退出。
- [src/OpenUnpack.Engine/Native/SevenZipEngine.cs](../../src/OpenUnpack.Engine/Native/SevenZipEngine.cs)：原生错误分类与salvage顺序风险。
- [tests/OpenUnpack.Tests/Program.cs](../../tests/OpenUnpack.Tests/Program.cs)：实际可执行harness、RAR辅助入口及已有manifest/格式检查。
- [tests/OpenUnpack.Tests/WorkerBoundaryChecks.cs](../../tests/OpenUnpack.Tests/WorkerBoundaryChecks.cs)：LZ4、MOTW和资源边界的现有回归。
- [tests/OpenUnpack.Tests/PathIdentityChecks.cs](../../tests/OpenUnpack.Tests/PathIdentityChecks.cs)：真实Windows目录/源/输出身份检查。
- [crates/core/tests/format_matrix.rs](../../crates/core/tests/format_matrix.rs)：现有真实RAR/ZIP分卷与独立载荷oracle。
- [crates/core/tests/desktop_pipeline.rs](../../crates/core/tests/desktop_pipeline.rs)：生产管线、嵌套、密码、删除、高压缩比与大传输。
- [scripts/verify.ps1](../../scripts/verify.ps1)：现有聚合入口与固定证据路径。
- [scripts/measure-load.ps1](../../scripts/measure-load.ps1)：显式ignored测量和可配置新ReportRoot。
- [.github/workflows/windows.yml](../../.github/workflows/windows.yml)：Windows检查/构建步骤；配置存在不能证明远程CI执行。
- [_workspace/system-detection-plan-2026-10-17/business-inputs-before.json](../../_workspace/system-detection-plan-2026-10-17/business-inputs-before.json)：本轮计划写入前237项业务/构建/测试输入摘要；交付时另核对保持。
