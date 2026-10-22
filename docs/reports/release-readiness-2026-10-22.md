# OpenUnpack 0.1.0 发版审查与最终候选复验

更新：2026-10-22T08:53:37+08:00。HEAD `cf6e34411c809d092c48ed2f4bdd4ab6160222c1`，分支 `codex/open-unpack`，保留原有未提交工作。

**当前结论：NO-GO，不能正式发版，也不能直接公开现有 Git 历史。** 新候选已构建并通过载荷验证；HDD负载矩阵在持续高磁盘负载下超时，尚需空闲盘对照定位；原生交互工具受阻，可信签名、Win10/11干净机和公共源码审查也未完成。以下逐项区分测试、成品与外部条件。

本轮延续 [上一轮修复](functional-fixes-validation-2026-10-21.md)，没有降低损坏归档、授权、源文件保护或回收凭据门禁。442 项源码/配置输入与上一轮最终验证相同；本轮重点是权限归因、普通用户复验、完整新包及开源审计。

## 回收问题已取得正向证据

工作区目录和其中 EXE 继承了 Windows Low mandatory integrity；实际子进程为 RID4096。父 PowerShell 为 Medium 不能证明子进程权限正常。将同 SHA 测试程序复制到普通用户临时目录后实际 RID8192，三种原生回收方式均成功；PreDelete 为0x282，PostDelete 有非空回收凭据，源文件移除。

原始 `recycle_bin_cleanup_when_enabled_and_verified` 在同 SHA 的正常部署中独立通过，本次完整工作区日志中的同一用例也通过。保留严格凭据断言，没有提权、重标目录、跳过测试或退化为永久删除。旧报告里的 Low 环境失败仍保留，本报告补充其关闭依据。微软对用户和 EXE 完整性共同决定子进程权限的说明见 [Mandatory Integrity Control](https://learn.microsoft.com/en-us/windows/win32/secauthz/mandatory-integrity-control)。

## 检测结果

| 范围 | 结果 | 证据和限制 |
|---|---|---|
| 首次完整 Rust 工作区 | 468 passed / 1 failed / 9 ignored；exit 101 | 同一冻结输入；普通用户部署；首次失败保留；原始忽略项继续单列。 |
| 最终串行完整 Rust 工作区 | 进行中，尚无最终整套结果 | 独立新Medium执行目录；保持全部原测试/断言；不将HDD显式失败改为通过。 |
| 缓存配额失败模块复跑 | 5/5通过；exit 0 | 同一原始测试镜像；串行执行且不改断言。首次整套468/1失败记录保留，不能改写成整套全绿。 |
| 真实 HTTP→Host→Core→Worker | exit 0 | 原条件忽略项；含临时密码、选择提取与无关队列保护。 |
| 串行复验真实HTTP | 等待最终串行套件完成 | 同一冻结输入；独立原生服务与临时目录。 |
| 实际依赖/运行时公告 | runtime / Cargo / UI npm / MCP npm 均通过 | 本轮在线扫描；Cargo 按真实 Windows 依赖图仅排除不可达组件公告。 |
| 完整安全审计 | passed | 实际 .NET Roslyn SAST、全部发现的 NuGet 项目、运行时服务基线、严格 Cargo/npm；不是策略单测。 |
| 新 Setup 与 portable | passed | 逐文件 Worker198项、真实.NET10.0.13、Host只允许Tauri bundle marker差异、完整便携清单、物料和两包Worker v2各14断言。 |
| 原生 GUI | 0 passed / 16 blocked | 新完整候选1457项文件复制核对、真实Host RID8192、主页面UIA可读；截图/输入工具失败，未完成16项独立交互验收。 |
| 物理拓扑及跨卷移动 | 首轮拓扑1/1、跨卷移动1/1通过 | 真实 C: NVMe SSD 与 D: SATA HDD；跨卷源锁定/目标保护。 |
| HDD→SSD缓存→HDD | 严格重跑通过 | 首轮在caching阶段等待可用内存而超时；原始失败、合成源包与现场保留。 |
| SSD负载矩阵 | 1/1通过；并发1和5各10任务 | 16MiB大文件+128个小文件/任务；全部输出清单、SHA-256及源包保留通过。Debug Core + Release Worker，不作为Release性能。 |
| HDD负载矩阵 | failed | 首个hdd-direct/并发1/10任务未完成600秒测量时限；D盘连续采样idle0%、队列3–5。尚不能把失败单独归因产品或环境；用户已指定本轮先完成其他检测，空闲盘对照延期。 |
| 其余显式负载用例 | cpu: passed；controls: passed；snapshots: passed | CPU、真实进度暂停/取消、合成历史快照独立执行；全流程命令在HDD失败后中止的未执行项目不计通过。 |
| 上一轮C#原生/UI/MCP | 109/109；214/214；20/20 | 本轮比对442项源码/配置输入无变化，C#27项相同；保留既有最终证据，不冒称本轮重复执行。 |
| 可信签名与干净机 | 签名门禁failed；干净机未完成 | 实际Host/Setup均NotSigned，缺真实发行者pin/可用证书；没有干净Win10/11实际安装/升级/卸载记录。 |


## 新成品身份与保存位置

版本仍为 **0.1.0**；未改名冒充Beta，未发布任何GitHub Release。候选目录为 `_workspace/release-finalization-20261021/package-20261022-attempt2/`。

| 文件 | SHA-256 |
|---|---|
| OpenUnpack-0.1.0-windows-x64-standard-setup.exe | 7f2fb3943dfe4130cef0207d3db22698d2112b311c992710704f94403d58a91f |
| OpenUnpack-0.1.0-windows-x64-portable.zip | df46b0d8edb44f7ce745ffdf1bb7c3b49b7a2da3841550526c0575c6a0c2377c |
| portable Host / reference-host.exe | 8f31d05096eabbafcd6822fa5f71e3cba3aacd4f8249f3842c6a58c55584e164 |

Worker完整198文件。Engine DLL `9228d87d495569cdf85cd3928e6e00214fcb4f84afb9223213ebdf72aef53e82`；Worker DLL `4341e47900b841ffe7c977f6b96a540c9a07cc19a3df58fc6b84d5fb9a4d008f`。首轮新包因Low esbuild无法操作系统TEMP失败；隔离构建TEMP重试后完成，原失败日志保留。新包受保护构建输入前后漂移为0。

本次测试保护输入645项；当前完整性结果：`True`。该范围包括源码/配置、官方7-Zip与暂存Worker；文档报告和生成构建输出分开处理。普通用户源码部署逐项匹配该清单，清单摘要 `c4f86d43dec4a82c04dfde791dfe98b1cac464596dee7b5da9adf6ae0b82cc1c`。新的普通目录只用于本地测试，不是经过公开审查的源码导出。

Host单元测试副本嵌入Common Controls v6清单，原Cargo二进制不修改；转换前后摘要在执行镜像记录中单独保存。此适配不构成正式Tauri Host或原生GUI验收。


## 原生与资源阻塞的处理

原生工具按支持的sky接口选择了精确候选路径与窗口；截图失败 `IGraphicsCaptureItemInterop.CreateForMonitor ... 0x80070057`，点击失败 `coordinate input geometry is unavailable`。重新观察窗口并重试激活后仍为 `GetCursorPos failed: 拒绝访问。 (0x80070005)`。UIA能读取“就绪”、空队列及隔离数据路径，但不能证明选文件、拖放、暂停取消、托盘、DPI或辅助功能通过。已停止原生输入尝试；16项均为blocked，证据结构校验无错误，门禁为incomplete。

本轮空的隔离GUI实例在核对PID/路径/SHA后结束，只释放自己的测试资源，未关闭用户应用或改动用户数据。物理跨盘首轮出现实际内存等待超时，不能改保护阈值凑通过；完成其他检查后串行执行原断言重跑。所有失败日志/现场保留，最终状态以重跑结果为准。

完整套件中的缓存配额并发用例曾超时；相同二进制的完整5项模块串行复跑已通过。首次物理跨盘失败中明确观察到“等待可用内存”，结束自己的空GUI实例后其余套件继续推进；这支持资源压力的解释，但原始整套失败与串行复跑结果仍分开报告。首轮安全聚合在npm输出JSON解析处失败，单独npm审计0漏洞，之后原脚本完整重跑通过；不凭缺失或不可解析的报告发放通过结论。

HDD负载命令的SSD配置1/5均通过，但首个HDD direct配置1在时限内未完成，后续HDD缓存/并发5未到达，不能宣称它们通过。测试总耗时含夹具和队列准备，时限为测量阶段600秒。连续磁盘采样为D盘idle0%、队列3–5；这证明当前不是空闲盘环境，不足以证明产品没有问题。用户指定“本轮先完成其他检测”，因此本轮不再重跑HDD矩阵，空闲盘对照延期；未关闭用户应用。三个后续SSD侧用例另行执行，HDD失败始终保留为发行阻断项。

## GitHub 开源方案与阻断项

完整方案：[GitHub开源与发版方案](../plans/2026-10-22-github-open-source-release.html)，配套 [23项任务及依赖](../plans/2026-10-22-github-open-source-release.tasks.json)。包含源码允许清单、许可/对应源码、中文/英文文档、贡献与安全入口、CI/CD、签名、SBOM/来源证明、Beta/Stable门禁、安装矩阵与回滚。

开源审计确认当前HEAD/索引包含 `.workbuddy` 267个已跟踪文件，其中含第三方分析及内部笔记，另有 `gemini-subagent` 29个已跟踪文件；生产范围有256个未跟踪文件，remote为0。因此既不能直接公开当前历史，也不能只推已跟踪文件。推荐保留原私有仓库，用经过权属、秘密、隐私与依赖闭包审查的允许清单导出建立新公共历史。

现有源码包扫描通过只覆盖脚本的扫描规则；它仍有行为准则/示例/集成脚本遗漏与公开docs筛选缺口，不能当作权属或隐私审查通过。README的43项命令描述也已漂移，当前契约命令数为46。方案明确修正文案承诺，避免把DPAPI写成硬件级隔离、把单元测试写成原生验收。

当前可以继续本地候选验收和开源准备。对外源码需要G0/G1；公开Beta及Stable均保留现有可信签名门禁。建议首发`v0.1.0-beta.1`，实施版本变更后重建、签名，并对最终成品重新做GUI与干净机验收，不能沿用0.1.0诊断包的结果。没有创建远端、提交、推送或公开文件。

## 证据入口


- [首次完整工作区/HTTP命令结果](../../_workspace/release-finalization-20261021/medium-results.json)

- [首次完整Rust日志](../../_workspace/release-finalization-20261021/logs/medium-workspace.log)

- [最终串行Rust日志](../../_workspace/release-finalization-20261021/logs/serial-workspace.log)

- [缓存配额原始模块复跑](../../_workspace/release-finalization-20261021/resource-recovery-retry.json)

- [原始回收正向结果](../../_workspace/release-finalization-20261021/recycle/core-positive-result.json)

- [三种原生回收结果](../../_workspace/release-finalization-20261021/recycle/medium-results.jsonl)

- [旧最终验证与当前源码比对](../../_workspace/release-finalization-20261021/prior-validation-source-comparison.json)

- [真实GUI门禁](../../_workspace/release-finalization-20261021/gui-validation.json)

- [GUI案例与工具错误](../../_workspace/release-finalization-20261021/gui-acceptance.json)

- [物理盘首轮与跨卷移动](../../_workspace/release-finalization-20261021/extended-functional-results.json)

- [物理盘严格重跑](../../_workspace/release-finalization-20261021/extended-retry-functional-results.json)

- [当前完整安全审计](../../_workspace/release-finalization-20261021/final-security-result.json)

- [负载首轮失败记录](../../_workspace/release-finalization-20261021/load-results.json)

- [其余负载用例](../../_workspace/release-finalization-20261021/remaining-load-results.json)

- [HDD负载连续采样](../../_workspace/release-finalization-20261021/hdd-io-observations.json)

- [实际签名门禁](../../_workspace/release-finalization-20261021/signature-gate-result.json)

- [本轮输入完整性](../../_workspace/release-finalization-20261021/input-integrity.json)

- [开源只读审计](../../_workspace/release-finalization-20261021/open-source-readiness.md)

- [最终包载荷验证](../../_workspace/release-finalization-20261021/package-20261022-attempt2/payload-check-7b5a2386d50c4d0db7c7c672b59e627e/payload-verification.json)
