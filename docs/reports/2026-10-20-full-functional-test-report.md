# OpenUnpack 全功能测试执行报告

2026-10-20 · 执行批次 `run-20261019T090629Z-cc4ac1` · **整体未通过，正式发行未验收**

本轮已执行现有跨栈基线、真实 HTTP→Host→Core→自包含 Worker→原生引擎、物理 HDD/SSD、协议互验及在线安全检查。发现 **4 项开放问题/门禁**，其中 API 连接中止根因仍待定位。未把 294 主用例全部标为通过。

## 可复核结果

| 检查 | 实际结果 | 证据 |
| --- | --- | --- |
| Rust 工作区全量 | 398 通过、0 失败、8 ignored；后续显式运行其中 3 项物理测试通过，剩余 5 项负载测量未跑 | [基线日志](../../_workspace/full-functional-testing-20261019/run-20261019T090629Z-cc4ac1/logs/cargo-baseline-01.log) |
| C# 原生 | 100 通过、0 失败、0 跳过；构建零警告/错误 | [原生日志](../../_workspace/full-functional-testing-20261019/run-20261019T090629Z-cc4ac1/logs/dotnet-native-01.log) |
| UI 自动化 | 默认 166/166；独立 audit 6/6 | [默认](../../_workspace/full-functional-testing-20261019/run-20261019T090629Z-cc4ac1/logs/ui-default-02.log)、[audit](../../_workspace/full-functional-testing-20261019/run-20261019T090629Z-cc4ac1/logs/ui-audit-01.log) |
| Worker v2 | 14 条断言通过 | [v2 日志](../../_workspace/full-functional-testing-20261019/run-20261019T090629Z-cc4ac1/logs/worker-v2-01.log) |
| 跨语言契约 | 19 个 Rust→真实 C# DTO→Rust 往返通过；结构检查 25 桌面 DTO、8 Worker DTO、268 字段与 5 个镜像通过 | [契约日志](../../_workspace/full-functional-testing-20261019/run-20261019T090629Z-cc4ac1/logs/contracts-01.log) |
| 实际物理盘 | 3 项通过：拓扑、HDD→SSD→HDD 内容往返、跨卷工具锁源/保全；不是吞吐验收 | [物理盘日志](../../_workspace/full-functional-testing-20261019/run-20261019T090629Z-cc4ac1/logs/physical-01.log) |
| 真实 API 最新轮 | 54 项父观察中 53 通过、1 失败；失败观察内的 270 个负向请求为 269 通过、1 连接中止 | [API 结果](../../_workspace/full-functional-testing-20261019/run-20261019T090629Z-cc4ac1/native-20261020-071628/results.json)、[270 请求](../../_workspace/full-functional-testing-20261019/run-20261019T090629Z-cc4ac1/native-20261020-071628/api-command-matrix.json) |
| CRC/Unsupported 全链回归 | 6 顺序×抢救开关，12/12 通过；均 failed、无发布、源包和旧哨兵保留 | [12 场景](../../_workspace/full-functional-testing-20261019/run-20261019T090629Z-cc4ac1/native-20261020-071628/mixed-error-api-regressions.json) |
| API 连接定向诊断 | urllib/http.client/一次发送 HTTP 三种方式共 30 请求，30/30 返回 401；没有关闭先前间歇失败 | [定向诊断](../../_workspace/full-functional-testing-20261019/run-20261019T090629Z-cc4ac1/transport-771e4c16/results.json) |
| UI 工程与 Clippy | lint/TS/Vite 构建通过，strict Clippy 通过 | [UI 工程](../../_workspace/full-functional-testing-20261019/run-20261019T090629Z-cc4ac1/logs/ui-engineering-01.log)、[Clippy](../../_workspace/full-functional-testing-20261019/run-20261019T090629Z-cc4ac1/logs/clippy-01.log) |
| 格式检查 | **失败**：Host 多语言文案和退出提示代码未满足 rustfmt | [fmt 差异](../../_workspace/full-functional-testing-20261019/run-20261019T090629Z-cc4ac1/logs/static-01.log) |
| 安全 | 当前候选 .NET 10.0.13 身份/当日官方服务基线、Roslyn、NuGet、npm 通过；严格 cargo audit **失败** | [审计](../../_workspace/full-functional-testing-20261019/run-20261019T090629Z-cc4ac1/logs/security-01.log)、[平台可达性](../../_workspace/full-functional-testing-20261019/run-20261019T090629Z-cc4ac1/results/security-triage.json) |

以上套件、API 父观察、HTTP 子请求及计划主项属于不同层级，**不得加成一个“全部通过总数”**。重复运行不累计通过数。

## 需要处理的问题

1. **FT-004 / P1：API 请求连接异常。** 两次连续全矩阵在未认证的 `POST /api/v1/commands/job_details` 遇到 Windows `10053`，预期应收到 HTTP 401。最新轮 Host 当时仍存活。较早两轮 270/270 通过，后续定向 30/30 也通过，所以目前只能确认异常可再次观察，尚不能归因到服务端、客户端或本机过滤软件。没有发现越权或错误发布。需要保留失败轮，定位发送/读取及早期认证拒绝的连接时序；写操作不能盲重试。
2. **FT-001 / P1：共享打包暂存载荷仍是 .NET 10.0.12。** `artifacts/worker` 的实际载荷与当前最低 10.0.13 策略不一致，验证器正确阻止通过。本轮实际 API 开发候选使用单独验证过的 10.0.13。该发现针对共享暂存输入，不能据此断言独立冻结的 API 便携 ZIP 也包含旧运行时。需要重新准备/验证全部载荷，再重建最终候选。
3. **FT-002 / P1：cargo fmt 门禁失败。** 差异位于 `apps/desktop-host/src/lib.rs` 的 `native_text` 及退出提示构造。本轮只测试，没有格式化或修复产品源码。
4. **FT-003 / P2：严格安全审计被 3 项新增维护告警阻断。** `zbus_names 4.3.4`、`zvariant 5.15.0`、`zvariant_derive 5.15.0` 为 unmaintained。按本轮 `x86_64-pc-windows-msvc` 实际 resolve 图从工作区根遍历，三者均不可达。原始 cargo audit 为 0 项漏洞，新增告警不是已证实的 Windows 可利用漏洞。仍保留严格门禁失败，未增加忽略规则，需独立审查上游更新或有目标证据的策略。

[机器可读问题台账](../../_workspace/full-functional-testing-20261019/run-20261019T090629Z-cc4ac1/results/findings.json)

## 为什么原生界面尚未执行完

首轮截图捕获与输入均受阻。**用户确认桌面已解锁后，已执行第 5 轮原生窗口重试：真实截图、窗口激活和空队列读取已恢复。** 新截图中可见 OpenUnpack 主界面及本轮隔离数据目录，不再沿用“当前黑屏”的判断。

点击“添加文件”首次返回 `failed to activate captured window`；刷新窗口并重试后返回 `GetCursorPos failed: 拒绝访问 (0x80070005)`。随后从最新焦点状态测试 Tab 键，同样失败。重置 JS 会话、重新加载受支持的 Sky API、重新选择工具返回的窗口后，Tab 仍返回相同错误。由此确认当前阻塞为原生输入访问，根因尚未确定；不能认定是产品点击故障，也不能把可读/可截图当作成功操作。未再要求用户重复解锁。

已停止该独立测试进程；没有完成文件选择、拖放、剪贴板、三语言原生标题/托盘/DPI、设置页创建密钥等界面操作。observer 的退出码 0 只代表观察脚本正常收尾，**不是原生 UI 测试通过**。

[原生最新状态](../../_workspace/full-functional-testing-20261019/run-20261019T090629Z-cc4ac1/results/native-ui-status.json)、[解锁后重试步骤](../../_workspace/full-functional-testing-20261019/run-20261019T090629Z-cc4ac1/results/native-ui-retry-05.json)、[真实窗口截图](../../_workspace/full-functional-testing-20261019/run-20261019T090629Z-cc4ac1/gui-39ffa795/01-empty-queue-0.png)、[空队列控件](../../_workspace/full-functional-testing-20261019/run-20261019T090629Z-cc4ac1/gui-39ffa795/01-empty-queue.json)。重试前报告及台账保存在 `reports/before-native-retry-05/`，原始失败记录没有被覆盖。

## 候选、环境与证据范围

- Host：当前源码 **debug/custom-protocol 开发构建**，SHA256 `4fcc4b61dc81591680af66a8c9573efdd38518ff8f3aa5a5b400e21a747e0017`；工作区 HEAD `cf6e34411c809d092c48ed2f4bdd4ab6160222c1`，存在原有脏改动。
- 实际 API Worker：自包含 .NET 10.0.13；runtimeconfig、CoreLib、CoreCLR、hostfxr 身份已校验，23 个 Worker/Engine 生产输入与冻结来源一致。未把“旁置候选文件存在”等同于逐进程采样全部已加载模块。
- 完整开发候选共 200 文件有 SHA256 清单。初始旧 Worker 也保留。此清单不能替代最终 ZIP/NSIS 或签名验收。
- Windows build 26300 x64，6 个逻辑处理器；C: NTFS/NVMe 与 D: NTFS/SATA 分别为物理盘 1 与 0。D: 仅在登记的新建 owner 目录运行合成样本；未拔出或卸载用户真实设备。
- Rust 使用本轮独立 `CARGO_TARGET_DIR`；原生套件与构建串行。物理内容正确性测试也独立执行，不报告为冻结条件下的吞吐/P95。
- 本轮执行期间有长时间授权/桌面等待。墙钟跨度不能视为 8h/24h 耐久负载。
- 产品实现/配置冻结的 266 文件、HEAD、Git index 前后保持一致。测试脚本、数据和本报告为新增；未暂存/提交/推送。

[源码前](../../_workspace/full-functional-testing-20261019/run-20261019T090629Z-cc4ac1/source-manifest-before.json)、[源码后](../../_workspace/full-functional-testing-20261019/run-20261019T090629Z-cc4ac1/results/source-manifest-after.json)、[候选文件清单](../../_workspace/full-functional-testing-20261019/run-20261019T090629Z-cc4ac1/results/candidate-manifest.json)、[设备清单](../../_workspace/full-functional-testing-20261019/run-20261019T090629Z-cc4ac1/results/environment.json)、[Worker 来源](../../_workspace/full-functional-testing-20261019/run-20261019T090629Z-cc4ac1/results/worker-source-provenance.json)

## 相对原方案的执行偏差

原方案要求先冻结最终载荷，同轮中途重建应另开 run。本次初始暂存运行时过期，随后构建当前开发 Host 并切换到隔离 10.0.13 Worker，仍在同一执行目录归档。因此源码回归和最终开发候选的 API 证据分别解释，不能把整轮当成同一正式便携包验收。`BASE-001`、`BASE-005` 保持 blocked。

已有测试脚本另写仓库 `artifacts/test-results`、`artifacts/verification`、`artifacts/issue-fixes-2026-10-10`。审计关键报告已复制到本轮 evidence，原生日志保留输出位置。没有使用个人密码库或用户业务归档进行测试。

两项探针前置条件错误已修正并保留旧记录：消费工具 token 后源文件已改名导致先返回 403；混合失败保留的合法工作区被误算为发布文件。修正后的工作区排除仅限当前任务声明路径、精确父目录/尝试名及 owner 标记通过的对象，目录外仍须只有原哨兵。它们不是产品缺陷。

UI 工程首轮外层曾因终端 GBK 打印 Unicode 出错，实际 lint/build 子命令退出 0；日志与结果保留。首次默认 UI 沙箱启动、未创建 GUI 输出目录、传输探针首次未建空 data 目录等无效启动不计产品失败，也不作为通过数。

## 294 项台账

状态计数：`{"not_run": 146, "running": 0, "passed": 5, "failed": 4, "blocked": 139, "not_applicable": 0, "invalid": 0}`。

`not_run + coverage: partial` 表示完整主用例未执行完，**已有子场景或相关回归证据**；不是“完全没做”，也不是完整通过。通过的 5 个基线主项仅覆盖明确记录的证据管理范围。完整功能和发行主项严格保留缺口。

| 模块 | 主项数 | 主项状态 | 已有证据与剩余范围 |
| --- | --- | --- | --- |
| BASE | 8 | blocked 2, passed 5, failed 1 | 已冻结/核对源、清单与台账；完整基线有失败，最终候选冻结存在偏差。 |
| UI | 16 | blocked 16 | 组件/独立 audit 通过；解锁后真实窗口截图、激活及空队列读取已恢复。鼠标和键盘输入均被 Windows 拒绝，重建工具连接后仍失败，未完成原生业务操作。 |
| SCAN | 12 | not_run 12 | 当前生产扫描及回归套件通过，真实 API 单包扫描通过；四类完整分卷、GUI 和全部边界未逐项展开。 |
| FMT | 16 | not_run 16 | C# 原生格式/安全及 Core 管线回归通过，混合 CRC/Unsupported 全链 12/12；不等于全部格式、加密、容量子矩阵完成。 |
| NEST | 9 | not_run 9 | 现有真实 Worker/Core 嵌套、重试及部分交付回归通过；完整深度、故障及 GUI 组合未全部展开。 |
| QUEUE | 18 | not_run 18 | 入队、开始、排队取消、重试、暂停、历史和删除保全已有真实链路；全部阶段、并发档位及 GUI 交错未完成。 |
| CACHE | 12 | not_run 12 | 真实 C: NVMe 与 D: SATA HDD 拓扑、HDD→SSD→HDD 内容往返及跨卷工具保全通过；完整边界、故障和性能对比未完成。 |
| OUT | 12 | blocked 12 | 现有碰撞、严格读回和保全回归通过；本轮混合失败源/哨兵保留；故障提交点和全部清理子状态未逐项完成。 |
| VAULT | 14 | blocked 14 | 真实 API 新增/修改/揭示/删除，Core DPAPI/上限/原子性等回归通过；真实窗口、第二账户和十万条合并性能未完成。 |
| IMPORT | 12 | not_run 10, blocked 2 | 真实 TXT 导入、CSV 坏引号可见及消费 token 重放拒绝；Core 严格 CSV 回归通过；GUI 与全部格式/编码未完成。 |
| BACKUP | 8 | blocked 8 | 真实加密导出/预览/恢复/消费 token 重放拒绝，Core 加密与上限回归通过；第二账户/故障/GUI 未完成。 |
| BROWSE | 8 | not_run 8 | 真实 Worker 检查三文件 ZIP 清单正确，Core 取消/预算回归通过；大树可视交互和多应用共享会话未完整执行。 |
| TOOLS | 10 | blocked 10 | 真实预览/批准/改名及 token 重放拒绝；跨卷锁源及目标保全通过；三类全部规则和 GUI 未完整执行。 |
| SET | 10 | not_run 9, blocked 1 | 真实保存/读回及部分运行生效，协议结构已核对；32 字段逐项非默认、重启、实际行为三方验收未完成。 |
| LANG | 9 | blocked 9 | 语言相关组件/Host 编译基线完成；三语言原生标题、托盘、删除确认及布局未执行。 |
| API | 42 | not_run 35, failed 1, blocked 6 | 42 命令的两种入口均有负向请求证据；最新 270 请求 269 通过、1 连接中止。7 类完整模板未全部展开。 |
| APIS | 18 | not_run 17, failed 1 | 现有 HTTP/并发/密钥存储回归通过，真实权限/目录/脱敏/SSE 首事件等部分通过；连接异常及完整边界仍未关闭。 |
| REC | 14 | blocked 14 | 已有恢复、租约、资源、提交、锁定回归通过；未准备可回滚 VM/VHD，全部真实阶段故障未执行。 |
| SEC | 14 | blocked 13, failed 1 | 现有原生路径/对象/IPC/Job/DPAPI 等回归和当前依赖审计已执行；100 轮 S06、跨账户和模糊预算未执行。 |
| PERF | 10 | blocked 10 | 物理设备内容正确性通过，不是性能验收。未冻结独占性能基线，未跑 100 循环或 8h/24h 负载。 |
| REL | 14 | blocked 14 | 当前是开发候选；未生成并验收最终正式签名 ZIP/NSIS，也无干净 Win10/11 安装矩阵。 |
| REG | 8 | blocked 8 | 历史修复相关原生/Core 回归通过，混合错误 12/12 和实际 .NET 10.0.13 已复验；完整 GUI/发行条件未关闭。 |

[逐项 JSON：294 项含 API 子场景](../../_workspace/full-functional-testing-20261019/run-20261019T090629Z-cc4ac1/results/plan-results.json) · [逐项 CSV](../../_workspace/full-functional-testing-20261019/run-20261019T090629Z-cc4ac1/results/plan-results.csv) · [32 设置与 15 状态](../../_workspace/full-functional-testing-20261019/run-20261019T090629Z-cc4ac1/results/settings-state-coverage.json)

## 尚需完成的验收

1. 修复或明确处置 FT-001～004，重新冻结最终便携候选到新 run；重跑受影响功能与必需基线。当前工程 G1 和完整候选 G0 没有关闭，不能进入正式发行通过。
2. 恢复可交互 Windows 桌面，完成真实文件/目录选择、拖放、队列详情、所有页面、剪贴板、API 应用创建/撤销、语言/DPI/托盘。现有 DOM 组件测试不替代这些项目。
3. 逐字段完成 32 设置的非默认保存→重开→实际行为，并补完格式/分卷/嵌套/清理/权限的全部参数组合。
4. 在已登记 VM/VHD 和隔离账户完成满盘、断卷、9 阶段 Host 故障、第二账户 DPAPI、确定性 S06 100 轮及跨卷替换等；现有 Native/锁定回归不自动关闭这些条件。
5. 冻结独占性能环境后完成各并发档位、十万条实际合并、100 循环、8h 混合负载与正式候选 24h。当前没有吞吐/P95 或耐久达标结论。
6. 最终签名字节在干净 Windows10/11 上验收便携/NSIS、WebView2 已有/缺失/离线、升级/回滚/卸载及完整包文件身份。

本轮结论：已执行范围内的核心内容回归大多通过，仍有明确门禁、连接异常及环境缺口；**不能批准“全功能 294 项通过”或正式发行**。本轮没有执行产品修复。
