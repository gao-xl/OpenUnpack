# OpenUnpack 项目总结、问题与待开发清单

> 本文保留 2026-10-10 修复前的审查基线。后续修复、最终回归与桌面候选包状态见 [2026-10-11 修复记录](issue-fixes-2026-10-11.md)，下表中的原始失败不代表修复后的结果。

核对日期：2026-10-10（Asia/Shanghai）。范围：当前整个工作目录，包含已有未提交修改和未跟踪的新模块；Git 基准为 cf6e344。本文基于本轮源码核查、三名审查 agent 的交叉盘点和当前测试输出。没有修改产品代码，也没有访问个人密码库。

## 1. 当前项目判断

OpenUnpack 已有较完整的 Windows x64 桌面批量解压主链，版本仍为 0.1.0、未公开发布。当前阶段应描述为：**首版功能已有实现，仍有生产路径缺陷；分层重构处于过渡期，整版发行验收尚未闭环。**

不能将“已有代码”“部分自动测试通过”“完成重构”“发行产品通过验收”合并成一个完成状态，也不据此给出缺乏基准的完成百分比。

### 实际运行结构

~~~mermaid
flowchart LR
  UI["React / TypeScript / Ant Design"] --> Host["Tauri 桌面宿主"]
  Host --> Core["Rust Core：当前协调器"]
  Core --> Store["SQLite / DPAPI / 本机文件系统"]
  Core --> Pipe["认证双命名管道"]
  Pipe --> Worker[".NET Worker"]
  Worker --> Engine["OpenUnpack.Engine / 官方 7z.dll"]
  New["Domain / Application / Infrastructure"] -. "已有模块，尚未装配为桌面运行链" .-> Host
~~~

- Host 已在 Cargo.toml 声明新 crate 依赖，但实际启动仍调用旧 Coordinator::open：[宿主入口](C:/Users/im/Documents/ChatGPT/批量解压缩/apps/desktop-host/src/lib.rs:1027)。
- Worker 生产依赖已从旧 C# Core 切换至独立 Engine：[项目引用](C:/Users/im/Documents/ChatGPT/批量解压缩/src/OpenUnpack.Worker/OpenUnpack.Worker.csproj:10)。
- 旧 C# Core 仍被回归测试使用；WPF 原型源码已经删除。
- gemini-subagent 与 .pi Harness 属于开发协作工具，不属于桌面解压的生产依赖。

## 2. 已有功能与实现状态

以下“已实现”表示当前代码中具备能力；完整格式、设备和原生窗口验收仍须逐项闭环。

| 能力 | 当前实现 | 尚需核实或补完 |
|---|---|---|
| 批量录入与扫描 | 文件/文件夹选择、拖入、递归扫描、排除预览、分卷归组、部分魔数识别 | 识别与引擎分派一致性、SFX、完整变体格式矩阵 |
| 密码库 | 分组、搜索、启停、批量操作、TXT/TSV 导入预览与去重、保留密码原文 | 新分层密码用例对等、来源/修订/成功事件一致性 |
| 密码保护与备份 | 当前 Windows 账户 DPAPI；独立口令加密备份；恢复前预览确认 | 第二账户/另一台电脑迁移、真实升级与失败恢复 |
| 有限候选试探 | 保存密码轮试、成功关联、独立 Probe、文件名线索自动候选 | 混合加密 ZIP 缺陷、探针成本预算、候选 ID 与范围说明 |
| 解压与嵌套 | ZIP/7z/RAR 主链；内层不同密码；同名内包；局部重试和显式部分交付入口 | 两项恢复回归失败、RAR2 加密等未覆盖边界 |
| 队列与记录 | 根任务串行、暂停新任务、取消、阶段进度、节点详情、历史 | 万条队列、DPI、键盘/读屏、实际原生窗口验证 |
| SSD 工作区 | 可选输入缓存、展开工作区、配额/余量限制、目标临时写回与发布 | 物理 HDD→SSD、磁盘满/拔盘、全过程性能测量 |
| 校验与恢复 | 流式 SHA-256、严格目标逻辑回读、工作区归属、发布恢复、成功 outbox | 清单边界硬化、故障窗口、恢复路径缺陷 |
| 桌面体验 | 浅色/深色/系统主题、置顶、托盘、提示音、可选源包清理/单根剥离 | 当前 EXE、实际拖放/选择器/托盘/休眠生命周期 |
| 工程与发行 | 构建/验证脚本、Windows CI 配置、源码/便携/NSIS 打包、第三方材料 | 当前版本发行包、远程 CI、干净机器安装与回滚 |

已有 Probe、大清单落盘、磁盘拓扑检测和旧 Win32 防休眠 Guard，不能再按旧 APF 提案将它们整体列为“尚未开发”。它们的预算、生命周期、诊断和边界验证仍不完整。

## 3. 当前问题清单

P1 表示优先处理或阻止相应交付；P2 表示需要修复的普通问题。区分当前生产路径与尚未接入的新模块。

### A. 当前生产路径

| 编号 | 优先级 | 问题、触发与影响 | 本轮证据 / 处理方向 |
|---|---|---|---|
| I01 | P1 | 普通文件排在加密文件前面的混合 ZIP 被 Probe 判为无需密码，即使候选中有正确密码。协调器只安排无密码解压，跳过正确候选。 | 已用当前 Release Engine 原生复现：三种候选输入都返回 NoPasswordNeeded；正确密码完整解压 2 文件且字节一致。修正加密属性，覆盖混合条目与探针不确定时的回退。 |
| I02 | P1 | 内包补密码后局部重试失败，无法复用已完成外包及兄弟节点。 | 完整套件及独立复测均失败，checking 阶段报“目标清单中的文件缺失”。源码删除已展开内包，却从未将其登记进 retired_files，恢复仍要求父封存清单中的该文件存在。应持久化消费/退役检查点后再删除，保留防篡改检查。 |
| I03 | P1 | 写回故障解除后重试仍失败，恢复不能完成已有封存结果的交付。 | 完整套件及独立复测均失败，checking 阶段报读写错误。源码删除 input 缓存目录，却未清空或退休 SavedRun.input_files，恢复仍回读已删副本。应统一缓存生命周期与持久化记录，继续核对原包 witness。 |
| I04 | P2 | 扫描器按魔数接受伪装扩展名，Engine 却按扩展名选 handler，任务收录后被拒绝 Unsupported。 | 同一 ZIP 原生提取成功；字节不变复制为 .bin 后 Probe 与 Extract 均 Unsupported。应统一格式证据、白名单和分派，不只改扫描列表。 |
| I05 | P1，清单硬化 | 超大清单落盘后，Rust 未验证返回路径归属、ManifestId/SHA256/ByteLength，也未限制全量读取；缺失清单路径会保留空 files 而继续返回结果。 | [Worker 客户端](C:/Users/im/Documents/ChatGPT/批量解压缩/crates/core/src/worker.rs:108)、[清单生成](C:/Users/im/Documents/ChatGPT/批量解压缩/src/OpenUnpack.Worker/WorkerOperations.cs:231)。代码证据明确，尚未进行清单攻击故障注入。须按可信工作区、有界长度、摘要和相对路径严格拒绝。 |
| I06 | P2 | 协议 v2 已有 DTO/字段，但生产仍是 v1；未实施 Hello 协商及 operation/generation 对应检查。 | [WorkerHost](C:/Users/im/Documents/ChatGPT/批量解压缩/src/OpenUnpack.Worker/WorkerHost.cs:42)、[Rust 事件检查](C:/Users/im/Documents/ChatGPT/批量解压缩/crates/core/src/worker.rs:260)。新字段回传不等于 v2 运行协议交付。 |
| I07 | P2 | sessionId/revision 已存在，但 UI 未消费版本化失效通知，快照数据与 revision 也没有一致读取边界。 | [UI 订阅](C:/Users/im/Documents/ChatGPT/批量解压缩/apps/desktop-ui/src/api.ts:45)、[Core 快照](C:/Users/im/Documents/ChatGPT/批量解压缩/crates/core/src/coordinator.rs:209)。补齐原子视图与旧快照拒绝，再验证并发刷新。 |

I01 的直接根因：[Engine 第 136 行](C:/Users/im/Documents/ChatGPT/批量解压缩/src/OpenUnpack.Engine/Native/SevenZipEngine.cs:136) 以属性 19 判断加密。锁定 7-Zip 26.04 的官方 [PropID.h](https://raw.githubusercontent.com/ip7z/7zip/26.04/CPP/7zip/PropID.h) 中 Encrypted 为 15、19 为 CRC；Bool 转换只接受 VT_BOOL，因此显式加密判断失效。完整包全为加密的既有测试并不能覆盖这种混合情况。

原生诊断证据：[result.json](C:/Users/im/Documents/ChatGPT/批量解压缩/artifacts/project-summary-2026-10-10/native-probe/result.json)、[运行命令](C:/Users/im/Documents/ChatGPT/批量解压缩/artifacts/project-summary-2026-10-10/native-probe/run-command.txt)。诊断 exit 0 表示缺陷已复现，不能计作产品通过。

### B. 新架构切换前必须补完

| 编号 | 优先级 | 当前缺口 | 影响 / 处理方向 |
|---|---|---|---|
| M01 | P1 | Host 仍装配旧 Core；新 TaskCoordinator 未具备启动、真实 Worker 事件驱动、嵌套处理、发布恢复与 outbox 完整流程，active_sessions 从未登记会话。 | 分层重构没有完成。逐个接缝迁移，维持唯一协调器，完成运行对等后切换 Host。 |
| M02 | P1 | 新 SQLite Store 能临时读取旧 JobView，但没有将转换结果写回，后续更新只按 JobAggregate 解析。 | 旧任务“能显示但不能继续更新”。实施有版本的事务迁移或统一兼容读写，验证中断/待核对任务。 |
| M03 | P1 | 迁移未保留每任务冻结 Settings 与嵌套 nodes；部分阶段回退 queued；投影使用当前全局设置并清空 stageTimes。 | 恢复和局部重试依据丢失；不能直接替换旧仓储。补足持久化模型及对等映射。 |
| M04 | P2 | 新协调器只在内存更新节点进度，真实 SQLite 只更新父任务；下次快照重读会丢节点更新。mock 却额外更新节点。 | 现有单测掩盖真实适配器差异。补真实 Application + SQLite 集成验证。 |
| M05 | P1，停止契约 | 新 Supervisor 已调用真实 WorkerClient，但 terminate 设置标志后立即返回，没有等待会话线程及子进程退出。 | 调用方不能据成功返回安全清理工作区。明确协作停止、强制终止与实际退出确认。 |
| M06 | P2 | 新 Vault 用例仅具备较小功能面；import_passwords 使用 trim，record_success 未携带成功事件 ID、候选 revision/来源；分组编辑、导入预览、备份恢复未完成对等。 | 接入后可能改变原文密码或成功记忆语义。优先适配已验证旧 Vault，不以新库生命周期测试代替生产对等。 |

主要定位：[旧任务读取](C:/Users/im/Documents/ChatGPT/批量解压缩/crates/infrastructure/src/sqlite/task_store.rs:103)、[旧任务更新](C:/Users/im/Documents/ChatGPT/批量解压缩/crates/infrastructure/src/sqlite/task_store.rs:184)、[任务投影](C:/Users/im/Documents/ChatGPT/批量解压缩/crates/application/src/use_cases/coordinator.rs:139)、[节点更新](C:/Users/im/Documents/ChatGPT/批量解压缩/crates/application/src/use_cases/coordinator.rs:300)、[Supervisor 终止](C:/Users/im/Documents/ChatGPT/批量解压缩/crates/infrastructure/src/worker/supervisor.rs:154)、[新 Vault 导入](C:/Users/im/Documents/ChatGPT/批量解压缩/crates/infrastructure/src/dpapi/vault_store.rs:425)。

这些新模块问题尚未影响当前旧 Core 的运行链；它们阻止安全切换与认可“重构完成”。

### C. 工程、文档与发行

- 契约检查已接入 verify/CI，但仍以整文件字段名正则判断，未比较 Rust/TS/C# 的所属类型、类型、可空性、默认值和真实序列化。必须改为真实跨语言共享样本/生成检查：[check-contracts.ps1](C:/Users/im/Documents/ChatGPT/批量解压缩/scripts/check-contracts.ps1:47)。
- 白皮书第 5 行宣称 W0～W6 完成、15 项清零、全套 100% 绿灯，与当前调用链和本轮测试不符；旧审查报告又包含已经修改的历史问题。需要建立当前状态索引，分别维护历史发现、代码修复和验证核销。
- README 声称只尝试已保存密码，但当前协调器会自动添加保险库之外的 filename_hint。应明确该来源的产品范围、默认行为、分组含义及成功记忆契约：[候选插入](C:/Users/im/Documents/ChatGPT/批量解压缩/crates/core/src/coordinator.rs:1082)。
- .pi agent 文档仍将 Worker 写为 stdin/stdout；实际为认证双命名管道。AGENTS 中 haiku/sonnet 标签与 .pi 的 Gemini 配置也不一致，依赖图需要同步。
- 关键新模块与大量修改尚未提交；当前 HEAD 不能独立代表本轮可运行源码。发布前应形成可恢复的完整版本快照，并验证干净检出构建。
- 现有 portable OpenUnpack.exe 时间为 2026-10-08 15:03:09。旧打包 Worker/Engine DLL 与本轮重建 DLL 的 SHA-256 不同，发行目录没有当前便携 ZIP/NSIS 安装器。不能将历史 EXE 作为当前版本验收。
- 前端构建通过，但主 JS chunk 约 1,185.91 kB、gzip 381.16 kB，触发 Vite 大包提示。可后续分包并测真实启动时间，属于优化项。

## 4. 本轮验证结果

只使用人工样本、临时或项目诊断目录。原生 Worker 测试使用本轮构建目录中的 Release Worker/Engine 与官方 7z.dll，不以旧 artifacts/worker 作为当前依据。

| 检查 | 当前结果 |
|---|---|
| .NET Worker / Engine Release build | 通过，0 警告、0 错误 |
| C# 测试程序 | 28/28 通过；含旧 Core 回归和新增 WorkerOperations 原生 ZIP/7z 场景 |
| 认证双管道 Worker 脚本 | 23/23 通过 |
| UI Vitest | 18/18 通过 |
| TypeScript / Vite production build | 通过，有主包体积提示 |
| Gemini 委派工具单元测试 | 15/15 通过；未核验当前插件安装/外部网关连接 |
| 跨语言字段存在检查 | 返回通过，但能力有限，不能证明契约一致 |
| Rust 全工作区 | 编译通过；Application 2/2、Core 单元 34/34 通过；desktop_pipeline 15/17，2 失败，完整命令 exit 101 |
| 两项失败独立复测 | 各自再次失败，错误阶段与信息一致 |
| 被完整命令中断的 Rust 套件 | Host 2/2、Domain 7/7、Infrastructure 2/2、Protocol 4/4；格式矩阵 5/5、扫描回归 5/5、WorkerClient 3/3，通过 |
| 混合加密 ZIP / .bin 对照 | 两项现存问题原生复现，见 I01/I04 |
| 当前 Release 桌面成品 / 原生窗口 / 干净机器 / 物理跨盘 | 本轮未验收；不能据前述自动测试宣布整版通过 |

主命令：cargo test --workspace --locked --offline -- --test-threads=1；使用项目 MSVC 环境并关闭增量编译。失败后独立复测两项失败、补跑剩余套件；非 Core 套件命令也重复执行了已通过的 Application 两项。按唯一测试项汇总，Rust 共 79 项通过、2 项失败；完整门禁仍失败。

补跑命令为 cargo test --workspace --exclude openunpack-core --locked --offline -- --test-threads=1，以及 cargo test -p openunpack-core --test format_matrix --test scanner_regressions --test worker_client --locked --offline -- --test-threads=1。曾将扫描目标误写为 scanner 的一次调用未运行测试，已更正重跑并单独留存 corrected 日志，不计为产品失败或测试通过。

证据目录：[本轮日志](C:/Users/im/Documents/ChatGPT/批量解压缩/artifacts/project-summary-2026-10-10)、[完整 Rust 输出](C:/Users/im/Documents/ChatGPT/批量解压缩/artifacts/project-summary-2026-10-10/cargo-workspace.log)、[源码摘要](C:/Users/im/Documents/ChatGPT/批量解压缩/artifacts/project-summary-2026-10-10/source-snapshot.json)。

当前验收清单 AT-01～AT-21 的整项状态仍 pending，含真实格式、故障、设备和原生窗口条件；已有自动覆盖应按实际运行范围核销，不能把 pending 解读为全部功能未实现。

### 历史问题的当前核销

| 历史发现 | 本轮判断 |
|---|---|
| Host 宏重复 / WorkerRequest 测试 E0063 编译失败 | 当前工作区编译通过，不能继续作为现存编译故障 |
| 损坏内包被误判需要密码（旧 R14） | 当前坏内包部分交付原生回归通过 |
| 完整传统 RAR 分卷误判缺卷（旧 R15） | 当前格式矩阵的传统 RAR/中间卷测试通过 |
| 新 Vault 静默当空库覆盖 | 代码已改为 DPAPI 整文件、严格读取和原子提交；本轮新库生命周期通过，旧格式迁移和失败注入仍应专门补验证 |
| 新 SQLite 无法读取旧任务 | 已有兼容读取，但首次更新、冻结选项与节点迁移仍不完整，不能核销为彻底解决 |
| Supervisor 完全为空实现 | 已有真实 WorkerClient 适配；实际退出确认仍未由新端口提供 |
| C# 没有新增协议字段 / 校验未接 CI | 当前字段已补齐、verify 已调用检查脚本；真实 v2 和跨语言防漂移仍未完成 |

本轮关键产品源码摘要在测试前后保持一致，见 [稳定性记录](C:/Users/im/Documents/ChatGPT/批量解压缩/artifacts/project-summary-2026-10-10/source-stability.json)。旧证据不能替代本轮结果，局部修复也不能支持“15 项全部清零”的声明。

## 5. 待开发与补完功能

### 先完成现有产品闭环

1. 修复混合加密 ZIP、两项恢复失败、识别/引擎不一致和清单校验；补相应原生回归，再跑完整门禁。
2. 建立可恢复的当前源码版本，重建当前 Release EXE、自包含 Worker、便携包、普通/离线 NSIS、摘要与第三方材料。
3. 完成原生桌面拖放、选择器、密码备份恢复、托盘、125/150/200% DPI、万条队列与键盘访问。
4. 在干净 Windows x64 机器验证安装、卸载、升级与回滚；覆盖 WebView2 已有/缺失情况。
5. 对物理跨盘、磁盘满/移除、睡眠恢复、发布崩溃窗口取得专门证据。
6. 若继续完成分层重构，先补迁移/模型/会话退出对等，再接入 Host；完成 v2、版本化 UI 同步后，按对等清单退役旧实现。

### 高价值后续功能与优化

| 功能 | 当前基础 | 后续交付内容 |
|---|---|---|
| 归档文件名编码 | 原生后端已返回 Unicode，尚无用户可控编码策略 | 有证据的 GBK/Big5/Shift-JIS 处理、置信度/手动覆盖、安全重校验；避免盲目二次转码 |
| 探针成本控制 | 已有 Probe，可能实际解码选定条目 | 能力驱动的有界试探、Inconclusive 回退、固实包避免重复解码、记录真实开销 |
| 来源完整的候选流水线 | Domain 有 CandidateOrigin，生产仍有字符串候选 | 候选 ID、来源、revision、有限批次与成功事件契约；保持原文密码 |
| 大清单流式处理 | 已有落盘 spill | 有界分块序列化/读取、清单封存与摘要、失败恢复；验证峰值内存 |
| 资源与解码预算 | 已有条目/输出/磁盘配额，Domain 有未接入的解码内存/候选上限字段 | 内存、候选工作量、时间/CPU 约束及可操作错误；真实 Worker 超限测试 |
| 稳定速度与 ETA | 已有阶段进度 | 平滑速率、剩余时间、不可估阶段提示、休眠后的计时基线 |
| 电源与磁盘诊断 | 已有拓扑检测和防休眠 Guard | 正确租约生命周期、失败处理/开关、盘符到物理盘映射与路由原因展示 |
| 格式覆盖 | 主流 ZIP/7z/RAR 测试入口已有 | RAR2 加密、混合条目、SFX/伪装后缀、支持方法与编码、缺卷/坏数据对照 |
| 可复现工程门禁 | 锁文件/CI/资料收集已有 | 真正跨语言黄金样本、真实适配器故障注入、干净检出构建与当前发行核销 |

### 首版之外的产品候选

CSV 导入、多物理盘并发、受限常驻 Worker、显式密码派生、离线词表/语料、额外引擎后端、压缩包制作、目录工具箱、完成后关机等应独立进入 backlog。离线语料与额外后端先明确来源许可、默认行为和预算。

云密码查询、穷举与账户体系属于现有首版明确排除项，不能作为当前版本的“漏做功能”；需要另行产品范围决策。

## 6. 建议执行顺序

**生产正确性与恢复 → 完整测试门禁 → 当前桌面发行与原生验收 → 分层迁移闭环 → 性能/体验优化 → 可选扩展。**

依据：[首版范围](C:/Users/im/Documents/ChatGPT/批量解压缩/docs/function-summary.md)、[验收清单](C:/Users/im/Documents/ChatGPT/批量解压缩/docs/acceptance-checklist.md)、[最终重构方案](C:/Users/im/Documents/ChatGPT/批量解压缩/docs/refactoring-plan-final.md:393)。APF 与 functional-design 中的提案和性能数字不直接当作当前实现或首版承诺。
