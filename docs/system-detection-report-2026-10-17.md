# OpenUnpack 本机功能与原生链检测报告

日期：2026-10-17。runId：`run-20261017-functional-01`。

本轮完成既有工程回归和新增独立探针，**确认四类功能/可靠性异常，另有两项过期 UI 审计用例**。常规套件通过不能抵消探针失败，整体检测门禁仍未通过。此次没有修复应用源码、升级依赖、修改个人密码库、提交或发布。

这是 [56 项全系统检测计划](plans/2026-10-17-system-detection.md) 的一次有界执行，不是全部计划结案，也不是原生窗口或正式发行验收。

## 结果概览

| 检查 | 本轮实际结果 | 范围与解释 |
| --- | --- | --- |
| C# 当前源码构建 | 通过，0 warning / 0 error | Release；可执行测试 harness 实际运行 |
| C# 原生 harness | **74 通过、0 失败、1 跳过** | nativeAvailable=true；辅助 RAR 夹具目录缺失而跳过，不等于 RAR 全无覆盖 |
| UI lint / build | 通过 | lint 零警告；不代表已运行真实窗口 |
| UI 正式测试 | **148 通过，15 个文件** | 组件/mock 范围 |
| UI 独立 audit | **4 通过、2 失败** | 两项使用过期历史 API 夹具，失败保留；详见下文 |
| Rust 五个非 Host crate | **345 通过、0 失败、8 默认忽略** | Core、Domain、Application、Infrastructure、Protocol；命令耗时 917.29 秒；没有构建/验收 Host |
| 原生桌面处理链子集 | **38/38** | 含 16,000 真实文件传输与逐文件目标读回、取消、删除、恢复、嵌套、LZ4、缓存、源包/已有输出保护；38 项合计 388.48 秒，非独立性能基准 |
| 格式矩阵 | **6/6** | 既有真实归档及 RAR/分卷等样本；没有据此宣称任意格式/损坏组合均可用 |
| 故障可靠性 / 资源恢复 | **3/3、5/5** | 真实 Worker 取消重开、移走源包后重试、目标占用；资源/缓存恢复 |
| fmt / 严格 Clippy | 通过 | fmt 全工作区；Clippy 为上述五个 crate 的 all-targets、warnings=error，未覆盖 Host 编译 |
| 契约、镜像、自测、Rust↔C# DTO 往返 | 通过 | 实际 corpus 导出、C# 反序列化/往返、Rust 校验，不只是结构 grep |
| 冻结 Worker v2 管道检查 | **14 项断言通过** | 真实具有关联身份的管道；独立于 GUI |
| 新增 Core 探针 | **58 项观察：38 匹配、20 不匹配** | Scanner 48 项=32/16；CSV 6 项=3/3；备份 4 项=3/1。20 个断言失败归为三类问题 |
| 新增混合错误探针 | **28 组：22 匹配、6 不匹配** | 每组分别调用 Engine 和真实 Worker；13 个 ZIP 均经原生 7-Zip 确认；invalidFixtures=0、harnessErrors=0 |

以上包含父集合与子集合，不应相加为测试总数或独立缺陷数量。原始命令退出码均保留；batch 最终退出 1，原因包括 audit 与新增探针失败。Core 探针退出 2 同时可能表示断言/基础设施异常，本轮 JSON 无基础设施失败记录，不能仅凭退出码称其未执行。

机器汇总：[observed-summary.json](../_workspace/system-detection-20261017/run-20261017-functional-01/results/observed-summary.json)。命令、参数、耗时、日志位置见证据目录 `results/checks-*.json`；实际执行器见 [run-checks.ps1](../_workspace/system-detection-20261017/run-20261017-functional-01/run-checks.ps1)。

## 确认的问题与修复任务

### F-01：公共写入可保存 100,001 条，成功导出的备份却不能由自身恢复

**严重度：高，备份可靠性；置信度：高。关联 DET-030。**

对照链已实际执行：小库公共新增/导出/恢复成功恢复 3 条；100,000 条有效加密种子可导出，自身恢复预览识别 100,000 条；随后一次公共 `Vault::upsert` 添加第 100,001 条，内存和重新打开后的条数均为 100,001。

此时 `export_backup` 返回成功，生成 **18,770,781 字节**的备份，并替换预先创建的合成目标 sentinel；自身 `preview_restore_backup` 和实际 `restore_backup` 都拒绝，恢复目标仍为 0 条。源密码库仍可打开，没有验证任何个人密码实际丢失。

根因对应 `crates/core/src/vault.rs:648` 的导出未与 `:784` 的 reader 100,000 条上限保持一致；公共新增也可越过该数量。问题是“成功导出的文件不能恢复”，不能把替换合成 sentinel 单独描述成未经授权的覆盖漏洞。

修复任务：统一公共写入、导出和恢复的数量/字节限制；若导出拒绝，必须保留既有目标；若导出成功，必须能被自身 reader 接受。增加 100,000 / 100,001 边界回归，并覆盖备份目标替换时的失败原子性。

边界：初始十万条直接生成符合当前 schema 的 DPAPI 合成库，没有重放十万次 GUI 添加。大型正例执行到恢复预览；没有执行十万条完整合并。第 100,001 条的公共写入、重开、失败恢复均为真实执行。

证据：[Core 探针](../_workspace/system-detection-20261017/run-20261017-functional-01/results/core-probe/synthetic-probe-9944-1792239652098233500/probe-report.json)、[独立复核](../_workspace/system-detection-20261017/run-20261017-functional-01/results/qa-core-probe-review.md)。

### F-02：混合 CRC 与不支持方法时，错误分类及抢救结果取决于条目顺序

**严重度：中，错误分类与抢救品质；置信度：高。关联 DET-011。此前源码怀疑已转为原生复现。**

使用明确构造并经原生 `7z t` 确认的 ZIP：`c` 为 CRC 错误、`u` 为方法 222 不支持、`v` 为正常条目。所有六种排列分别运行普通/抢救模式：

| 顺序 | 普通模式 Engine / Worker | 抢救模式 Engine / Worker |
| --- | --- | --- |
| `cuv`、`cvu`、`vcu` | Corrupt / error(Corrupt) | completed / completed，scope=salvagedWithCrcWarning |
| `ucv`、`uvc`、`vuc` | Unsupported / error(Unsupported) | Unsupported / error(Unsupported) |

普通模式三例仍正确报告失败，异常在于不支持方法被更早的 CRC 错误遮蔽。抢救模式三例则真实返回 completed：不支持条目原声明 45 字节，实际留下 **0 字节、isCorrupted=true** 的文件，与 CRC 损坏项合并为“3 项输出、2 项损坏”的警告。

源码 `src/OpenUnpack.Engine/Native/SevenZipEngine.cs:954` 只保存首个非零 OperationResult；`:637` 的校验只按该值判断 Unsupported，再由 `:661` 的抢救分支允许返回。控制样本和改变排列后的差异支持此因果解释。

修复任务：保留各条目错误类型，统一不支持方法与 CRC 的处理优先级；六种排列、两个模式结果必须一致地表达不支持方法。随后补 Core 全链发布/品质/源清理验证，不能只以 Worker terminal 判定完成修复。

**结论边界：** Worker 已带 CRC 抢救 scope 和损坏标记；Core 还有 salvaged 品质和禁止源清理逻辑。本探针未驱动 Core/UI，因此不能声称界面显示完整成功、最终目录已发布缺文件，或源包被误删。28/28 的源包、sentinel、已产生的受支持内容、Worker 关联身份、唯一终态与实际正常退出保护检查均通过。

探针名为 `CompletedManifestMatchesDisk` 的 false 断言包含“不支持方法不能 completed”的政策判断，**不是清单哈希与磁盘不一致**。另行重新读取文件，16 份 completed 层结果的清单长度/哈希均与实际磁盘一致，包含被标为损坏的空文件。

证据：[原始汇总与事件](../_workspace/system-detection-20261017/run-20261017-functional-01/results/salvage-probe/results.json)、[独立复核](../_workspace/system-detection-20261017/run-20261017-functional-01/results/qa-salvage-review.md)、[实际清单/磁盘再核对](../_workspace/system-detection-20261017/run-20261017-functional-01/results/salvage-manifest-disk-verification.json)。

### F-03：非 ASCII 大小写混用的同名分卷被错误拆组

**严重度：中，扫描/导入正确性；置信度：当前输入下高。关联 DET-008。**

四种家族 `.7z.001/.002`、`.part01.rar/.part02.rar`、`.rar/.r00`、`.zip/.z01`，分别测试目录、首卷、后续卷、同时选择两卷。

ASCII 大小写混合和 Unicode 相同大小写的 **32 个对照通过**；`Ä/ä` 混合的 **16 个场景全部失败**：一个双卷组变成两个单卷来源，缺卷标记为 `[false,true]`。例如 `Ä.7z.001` 与 `ä.7z.002` 均存在，扫描却拆成两个来源。

额外 12 项实际路径别名检查显示，小写别名都可解析到相同内容，且目录没有实际小写副本，排除了本次样本目录采用大小写敏感语义的解释。`scanner.rs:482` 家族发现使用 Unicode 小写化，`:737/:743/:748/:757` 归组只做 ASCII 小写化，策略不一致。

修复任务：让卷发现、归组、去重使用与被测文件系统语义相符的一致比较方式；保留四家族、四入口及 ASCII/Unicode 对照，另补真实可解码混合大小写分卷全链验证。

边界：占位字节刻意不是有效归档，结论仅证明生产 scanner 的错误拆组、重复来源和不一致缺卷提示，没有证明真实解压失败、假成功或丢包，也不外推所有 Unicode 字符和其他文件系统。

证据：[Core 探针](../_workspace/system-detection-20261017/run-20261017-functional-01/results/core-probe/synthetic-probe-9944-1792239652098233500/probe-report.json)、[目录别名检查](../_workspace/system-detection-20261017/run-20261017-functional-01/results/scanner-case-semantics.json)。

### F-04：畸形 CSV 引号被静默解释，密码字段可被去引号或拼接

**严重度：中，导入校验/字段完整性；行为置信度高，严格拒绝政策需明确。关联 DET-029。**

未闭合引号、未加引号字段中的内嵌引号、结束引号后的额外文本三例，均显示 preview 新增 1 / 错误 0，确认导入成功，磁盘发生变化，重开后存在 1 条记录。导入后的授权 reveal 匹配探针预先定义的去引号/后缀拼接值。三个合法引用、转义和多行正例均通过。

`vault.rs:1267` 的 CSV 状态机允许未加引号字段中途进入引号状态，`:1333` 在 EOF 引号未闭合时仍提交行，解释了无错误提示的结果。

修复任务：明确支持的 CSV 方言；对无效引号结构给出行错误，避免静默改变密码字段。保留合法转义、空白、多行内容的逐字节正例和预览/确认不产生半导入的负例。

边界：“所有畸形输入必须整批拒绝”是本探针选择的严格政策，不是已确认的完整产品规范。不能把三例称为三个独立漏洞或合法 CSV 普遍损坏。此次 reveal 在导入后即时核对，重开只再次核了数量，没有声称重开后的密码逐字节 reveal 也已执行。全部密码均为合成值，报告不输出秘密或确认 token。

证据：[Core 探针与六例结果](../_workspace/system-detection-20261017/run-20261017-functional-01/results/core-probe/synthetic-probe-9944-1792239652098233500/probe-report.json)、[独立复核](../_workspace/system-detection-20261017/run-20261017-functional-01/results/qa-core-probe-review.md)。

## 测试自身的问题

### T-01：两项历史页 audit 沿用旧 API

`apps/desktop-ui/audit/comprehensive-review-2026-10-12.test.tsx` 只 mock `history()`，现行页面使用 `historyPage(query)` 与分页 DTO。两项均在“旧方法应被调用一次”的首次断言失败，未进入想验证的迟到响应/进度快照场景，不能据此认定产品发生竞态或重复读取。

正式历史分页回归使用现行 API，包含过期查询与串行读取场景，本轮在 148/148 中通过；这不替代两个过期 audit 场景维护后的重新执行。维护任务需同时处理分页 DTO、串行请求模型和 2 秒轮询，不能只改函数名或放宽超时。独立 audit 仍保留 4 通过 / 2 失败。

证据：[完整原因复核](../_workspace/system-detection-20261017/run-20261017-functional-01/results/qa-audit-failures.md)。关联 DET-004/006/028。

### T-02：测试结论需要保持覆盖范围

- `worker_client` 四项仍直接绑定 `src/OpenUnpack.Worker/bin/Release/net10.0-windows`；其他主要原生链使用 `OPENUNPACK_TEST_WORKER` 指定的冻结自包含载荷。两个入口都已记录身份，不能仅凭环境变量假定全部用例都用了同一个 EXE。
- `resource_recovery_regressions.rs` 的某个缓存采样使用 `unwrap_or(0)` 且只断言峰值上界，读目录失败可能伪装成零，应改为显式失败并验证确有缓存字节。该 rig 实际配置 always cache；本发现不是“已经证明生产缓存额度失效”。另有更严格 desktop_pipeline 回归，不能把采样缺口外推为所有缓存测试无效。
- 部分进度/并发测试只观察终态进度、running 或配置，不足以证明连续进度、实际重叠或进度后的取消时延。本轮 release_reliability 的真实进度/退出证据只支撑相应场景；没有执行 30 次取消分位数、独占吞吐或长期稳定性验收。

## 版本绑定与输入完整性

分支 `codex/open-unpack`；HEAD `cf6e34411c809d092c48ed2f4bdd4ab6160222c1`。当前大量未提交源码参与检测，HEAD 本身不代表被测内容。冻结 **341 个源码/构建配置/测试/样本输入**，另冻结初始载荷 199 文件、当前源码自包含载荷 198 文件，以及 framework Worker 7 个直接文件。检测后四组均无内容/长度变化。

tracked 工作区状态前后均为 90 项且完全相同；原有唯一暂存删除 `apps/desktop-ui/tsconfig.tsbuildinfo` 保留。新增内容是报告、探针和隔离检测产物。Git 枚举旧 `_workspace` 长路径有警告；源码/载荷完整性由逐文件哈希核对得出，未把有警告的全量 untracked 枚举冒充完整清单。

当前源码重新发布 Worker 成功后才运行主要原生回归，独立 `CARGO_TARGET_DIR`，构建/原生执行串行。主要载荷 SHA-256：

| 文件 | SHA-256 |
| --- | --- |
| OpenUnpack.Worker.dll | `e80f4af0d401b08030d2448c37c6013e07a387f7ae893b5fb5c136898c26ff40` |
| OpenUnpack.Engine.dll | `818248e4996d2710660428e1a802a8717ea0730ad5b27d2875ccd44cf2431070` |
| 7z.dll | `899cd73d7fc60e1c1c35c7de0f5bed3dc69bfe2cecbd48014d868a347970a0b1` |

工具环境记录为 Windows 10.0.26300 x64、Rust 1.96.0 MSVC、Node 24.15.0、.NET SDK 10.0.301；机上 dotnet Host/共享运行时为 10.0.9。C# harness 为框架依赖运行，主要 Worker 和 salvage 探针为自包含 **10.0.12**，不能声称所有 C# 检查使用相同运行时。

初次 sandbox 构建和缺 RID assets 属于准备失败，保留原始记录；获准重跑后构建成功，并从已有缓存离线恢复 win-x64 10.0.12 runtime pack，再重新发布。`NuGetAudit=false` 只用于这次离线准备，不代表依赖安全扫描通过，没有据此更新安全结论。

证据：[输入完整性](../_workspace/system-detection-20261017/run-20261017-functional-01/results/input-integrity.json)、[tracked 状态比较](../_workspace/system-detection-20261017/run-20261017-functional-01/results/tracked-status-comparison.json)、[初始 run 信息](../_workspace/system-detection-20261017/run-20261017-functional-01/run-info.json)。

## 跳过、未执行及后续顺序

Rust 8 项默认忽略：1 项需要显式第二可写卷，2 项需要真实 HDD/SSD 设备，5 项是要求独占显式执行的负载/性能/取消测量。C# 另 1 项因辅助 `artifacts/rar-fixtures` 缺失跳过。正式 Rust 格式矩阵已使用自己的真实样本运行，不能把辅助入口跳过写成 RAR 完全未测。

本轮未执行：真实 Host/WebView 的拖放、按钮、DPI/多屏和读屏；干净 Windows 的正式产品安装/升级/卸载/回退；可信签名；专用满盘/脱机/VHD/SMB；第二账户 DPAPI；长期负载与随机变体矩阵。它们仍为未验收范围，不能由上述组件/后端证据补齐。

[10/17 依赖安全报告](dependency-security-report-2026-10-17.md) 的 DS-01（随包 runtime）与 DS-02（自动审计覆盖 runtime）本轮没有修复或重新联网核对公告；实际继续使用 10.0.12。它们仍按先前报告保留，不能称此次功能构建成为可正式发布的安全候选。

建议修复/复验顺序：F-01 备份上下限一致性 → F-02 混合错误聚合 → F-03 分卷比较策略 → F-04 CSV 校验与 T-01 测试维护。保留本轮失败样本作为回归输入，修复后用新 runId 重新冻结并复跑受影响链，再推进真实窗口和外部环境。完整 56 项任务继续保留未覆盖子场景，不能因本轮通过数而整项关闭。

原始日志不会覆盖；复跑应建立新 runId、独立构建目录和证据目录，沿用本轮明确的 oracle。现有脚本会拒绝覆盖部分日志/夹具目录，不应直接把旧目录反复作为新运行现场。
