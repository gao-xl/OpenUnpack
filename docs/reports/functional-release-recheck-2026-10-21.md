# OpenUnpack 功能模拟复验与发版结论

> 本文保留修复前状态；最新修复与完整回归结果见 [已知问题修复与复验](functional-fixes-validation-2026-10-21.md)。

日期：2026-10-21，汇总时间：2026-10-21T17:51:19.289485+08:00。

**结论：NO-GO，当前 0.1.0 不能正式发版。** 本轮重新复现了归档缺文件仍报告完整成功、选择提取的IPC/API集成缺口、回收站清理失败和英日翻译缺失。当前最终安装包、可信签名、真实窗口操作和干净机升级验收也没有完成。数据完整性问题应优先修复，不能只修测试使门禁变绿。

## 标的、方法与完整性

受测标的是 HEAD `cf6e34411c809d092c48ed2f4bdd4ab6160222c1` 上的现有脏工作区。先保留源码快照与 SHA-256，再并行运行 React/jsdom、独立 C# Engine/Worker 套件，并串行验证 Rust Coordinator、真实 Worker/7-Zip、API 和故障路径。全部在隔离测试目录中使用合成样本或仓库既有测试fixtures；没有读取用户归档、修改产品源码、改写原测试断言、提交、推送或发布。

本轮受保护输入 642 项，执行前后 全部一致；UI 独立 70 项和 Worker 独立 92 项也全部一致。与今天较早全工作区测试的 642 项输入逐文件匹配，因此其完整日志可用于定位当前失败；早一轮结果与本轮重跑结果分别记录，不累计为独立测试总数。

## 本轮实际执行结果

| 范围 | 结果 | 证据边界 |
|---|---|---|
| C# 原生功能套件 | 100 passed / 0 failed / 0 skipped | 独立复制当前源码后重新编译；真实 Engine/Worker/官方 7-Zip |
| 归档完整性差分探针 | 4 正常对照通过 / 4 损坏场景失败 | 完整 ZIP/TAR、缺尾 TAR、成员头截断 TAR；抢救开/关；7z t 独立对照 |
| UI 全套 | 200 passed / 2 failed / 0 pending，共 202 | 23 文件，未过滤、未放宽断言；React/jsdom |
| TypeScript/Vite、ESLint、轻量 a11y | 均通过 | 生产前端产物写独立证据 dist；a11y 仅仓库既有静态规则 |
| Core 单元 | 157 passed / 1 failed / 1 ignored；exit 101 | 包含 SQLite、恢复、预算、队列、扫描、密码与路径逻辑；忽略项仍按原设备条件保留 |
| 便携 Vault 原始测试 | 1 passed / 1 failed / 0 ignored；exit 101 | 失败为测试前置状态错误；独立探针见下文 |
| 8 项真实桌面后端流程 | 5 passed / 3 failed | 嵌套不同口令、严格回读、密码耗尽、取消、已完成任务删除，以及文件名口令/重名/回收站失败复验；各次其余37项为过滤未执行，不能计作通过 |
| 真实格式、Worker IPC、故障恢复 | 13 passed / 0 failed / 0 ignored；exit 0 | RAR4/RAR5、加密与分卷、分卷 ZIP、进度、源移动、占用目标、取消恢复等既有原生矩阵 |
| Local API / CLI / Protocol | 55 passed / 0 failed / 0 ignored；exit 0 | HTTP/SSE/WS、撤销/过期、限额、拒绝正文传输、协议语料等 |
| Host 单元 | 28 passed / 0 failed / 1 ignored；exit 0 | 给独立测试 exe 副本加 Common Controls v6 清单后运行；不代表正式 Host 包或真实 GUI 已验收 |
| 真实 HTTP→Host→Core→Worker 解压 | 1 项通过，含 3 个业务流程 | 普通 ZIP、请求临时口令、等待后 submit_input；逐字节核对输出，原包保留、库为空、无关 sentinel 未启动 |
| 真实 API 选择提取 | 失败：queue_snapshot 对照 200，extract_selective 400 invalid_request | 当前源码对应 Host 测试二进制，独立清单副本；不是安装包 |
| 独立便携 Vault 状态探针 | 拒绝未启用、显式启用后重开及口令回读均通过 | 正确清除两个 opt-in 环境变量；未修改原失败用例 |
| 独立回收站探针 | 锁源通过，Shell 回收失败，0x80070057 | 私有 ACL 目录内的合成文件；原文件仍在 |
| MCP | 20 passed / 0 failed / 0 skipped；exit 0 | 本地 HTTP/MCP 服务测试；不是正式 Host 全命令业务验收 |
| cargo fmt / 架构 / 三端 DTO 静态契约 / Worker 运行时身份 | 均通过 | Worker 实际自包含载荷核验为 .NET 10.0.13；不是整版发版许可 |

今天较早的完整 Rust 工作区日志已结束，退出 101：统计到 421 passed / 5 failed / 8 ignored，另有原始 Host 测试程序加载失败（0xc0000139），共4个失败 target。本轮是有针对性的复验，没有宣称再次完成整工作区或294项功能总计划。

## 阻止发版的问题

1. **RR-01 / P1：归档缺文件仍报告完整成功。** `partial-header.tar` 是包含 first.txt 和 second.txt 的正常 TAR 的前1,100字节。独立官方7-Zip报告 Unexpected end of archive / exit2；当前源码重编的 Engine 在抢救开和关两种模式下均报告 completed、archiveCrcAndStreamSha256，实际上只有 first.txt，second.txt 缺失且未标损坏。缺结束块的另一份 TAR 同样没有报告归档级错误。定位为 `src/OpenUnpack.Engine/Native/SevenZipEngine.cs:665` 的归档级完整性判定缺口。本探针没有执行该 TAR 的 Coordinator 发布或清源，不能扩展成已证明用户原件被删除。

2. **RR-02 / P1：归档浏览“提取所选”链路未接通。** UI 实际发送 extract_selective，Host handler 和 Core 实现存在，但未列入 build.rs 的应用命令清单及 main-commands ACL，也未列入 Local API 的 ApiCommand。源码、生成 ACL 与当前锁定 Tauri 运行时代码证明原生 IPC 将在 handler 前被 ACL 拒绝；本轮真实 HTTP 已验证同名命令返回400，而 queue_snapshot 对照返回200。原生点击没有执行，不能把静态 ACL 结论说成已点击复现。UI 失败测试本身另有旧 addJobs 预期和无返回 mock 问题，应与真实集成缺口分别修复。

3. **RR-03 / P2：自动移入回收站失败。** 真实 Coordinator 流程成功交付后未移走源包。进一步独立探针确认 RecyclableSource::open 成功，后续 Shell recycle 返回0x80070057，文件仍在。已将失败缩小到 Shell 回收阶段；尚未确定具体 COM 调用或回调根因。保留源文件符合失败保护要求，但不能认定回收站功能通过。

4. **RR-04 / P2：英文、日文各缺3条翻译键。** 成功提取提示、“提取所选至...”和便携密码库未启用长提示会回退中文，完整 UI 套件稳定失败。

5. **RR-05 / 验收契约：密码候选限制与若干测试未收敛。** 文件名中的口令现在会无条件作为临时候选加入，原有“只能优先使用已启用且组内已保存候选”测试因此仍失败。没有证明读取了停用或组外 Vault 记录，而是从文件名独立派生同一字符串；需明确允许范围、与设置和文档一致，再建立回归。另有 overwrite 已允许而旧测试要求拒绝、相同内容自动去重而旧测试要求另存新目录、portable 测试忘记关闭 opt-in 等明确测试/行为不一致。原失败照实保留，不能删断言宣布通过。

6. **RR-06 / 发版验收：缺最终成品闭环。** 本轮前端构建已通过，但没有将当前最终 Setup/portable、全载荷摘要、可信签名、真实窗口和干净 Win10/11 安装/升级/卸载绑定成一次验收。现有签名证据为旧 Host 和本轮 Worker 的 NotSigned，不含当前最终 Setup。`scripts/ci/verify-native-ui.ps1` 的16项直接写入PASSED，没有驱动候选并断言结果，不能采用。窗口选择器、拖放、托盘、DPI以及物理盘/SMB故障和长时稳定性保持未验收。

## 发版前关闭条件

按顺序处理：归档级错误识别并补 Worker→Core 发布/清源回归；接通选择提取的 IPC ACL 与 API 并完成成功/取消/错误流程；修复回收站并验证源身份保护和失败保留；补翻译；统一口令与冲突策略契约、修正测试夹具并跑绿相关及全工作区套件；最后冻结并构建新的 Setup/portable，完成签名、真实 GUI 和干净机安装/升级/卸载验收。未完成上述事项前，结论保持 NO-GO。

## 复现与证据入口

- 本轮机器汇总：[`summary.json`](../../_workspace/release-recheck-20261021-root/summary.json)。
- Rust各命令、工作目录、时间、退出码与原始日志：[`results.json`](../../_workspace/release-recheck-20261021-root/results.json)；执行器：[`run.ps1`](../../_workspace/release-recheck-20261021-root/run.ps1)。单作业、私有TEMP、锁定离线依赖，保留既有 libsqlite3-sys debug=0 构建绕行；没有修改产品配置或降低断言。WorkerClient 固定使用的 Release 路径另行以当前源码执行 dotnet build，0警告/0错误；见 [`shared-worker-build-result.json`](../../_workspace/release-recheck-20261021-root/shared-worker-build-result.json)。实际使用的两套Worker、Engine、7-Zip二进制摘要见 [`tested-runtime-manifest.json`](../../_workspace/release-recheck-20261021-root/tested-runtime-manifest.json)。
- 输入快照及完整性：[`input-integrity.json`](../../_workspace/release-recheck-20261021-root/input-integrity.json)。
- Worker/C#重编、样本和语义断言：[`report.md`](../../_workspace/release-recheck-20261021-worker/report.md)、[`assertions.json`](../../_workspace/release-recheck-20261021-worker/assertions.json)。采集器exit0不等于通过，独立语义断言实际exit1。
- UI完整套件与集成链路：[`report.md`](../../_workspace/release-recheck-20261021-ui/report.md)、[`commands.json`](../../_workspace/release-recheck-20261021-ui/commands.json)、[`selective-integration.json`](../../_workspace/release-recheck-20261021-ui/selective-integration.json)。
- 真HTTP及选择提取：[`native-api-result.json`](../../_workspace/release-recheck-20261021-root/native-api-result.json)、[`native_probe.py`](../../_workspace/release-recheck-20261021-root/native_probe.py)。使用与当前642输入一致的既有Host测试二进制，新副本SHA核验后执行；临时合成连接令牌文件已删除。
- Vault/回收站独立探针：[`environment-probe-observations.json`](../../_workspace/release-recheck-20261021-root/environment-probe-observations.json)、[`environment_probe.rs`](../../_workspace/release-recheck-20261021-root/environment_probe.rs)、[`environment-probe-library-identities.json`](../../_workspace/release-recheck-20261021-root/environment-probe-library-identities.json)。
- 独立证据复核及旧失败分类：[`evidence-review.md`](../../_workspace/release-recheck-20261021-qa/evidence-review.md)。

本报告取代较早汇总中的“Rust未完成、UI198/4、前端构建阻塞”状态；原始历史日志保留可回溯。模拟与后端实测通过的部分不等于原生桌面或发行包接受。
