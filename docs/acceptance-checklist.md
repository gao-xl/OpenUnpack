# 版本验收清单

> **历史文档（2026-10-11 核销）**：本清单为 2026-10-10 的版本验收基线，其状态列（含批量 pending）已不代表当前进度，
> 不能直接作为当前待办表。当前代码整改见 `docs/comprehensive-fixes-report-2026-10-13.md` 及 [S06 后续修复与候选验证](s06-directory-identity-report-2026-10-15.md)；安装与桌面验收历史状态见 `docs/status-ledger-2026-10-11.md`；本文件的证据入口引用仍可回溯。

审计日期：2026-10-10。范围为 Windows x64 首版，不将规范中的 P1/候选功能冒充已实现。原规范保持原样，本文以当前源代码和实际执行输出更新状态。**pending 表示尚无足以支持整项完成的当前证据；存在测试代码不等于测试通过。**

验证入口：`scripts/verify.ps1`，真实进程测试在 `crates/core/tests/` 与 `src/OpenUnpack.Worker/verify-worker.ps1`。所有测试必须使用人工样本和隔离目录。通过结果需保存命令、时间、版本、样本范围与终端输出，不能用历史 EXE 或浏览器预览替代。

## 功能与数据验收

| 编号 | 原要求与范围 | 当前覆盖入口 / 所需证据 | 状态 |
|---|---|---|---|
| AT-01 | 无密码 ZIP / 7z，内容一致，无无效成功记录 | Worker 协议脚本、`worker_client` 与 `desktop_pipeline`；逐文件比对 | pending |
| AT-02 | ZIP 传统/AES、7z 加密头，有限错密码轮试/成功关联 | `desktop_pipeline` 人工 7z；ZIP 两种加密必须各有真实样本运行结果 | pending |
| AT-03 | RAR/RAR5、加密与固实 | 必须各有真实格式样本及文件比对；引擎能力不能替代 | pending |
| AT-04 | 分卷、中间卷、缺卷、缓存完整组 | `scanner` 分卷单元测试、Worker 数字分卷、`selecting_first_real_split_volume...`；RAR/ZIP 变体另验 | pending |
| AT-05 | 中文/首尾空格密码，导入预览与去重 | Vault `import_tokens_are_bound_and_unicode_whitespace_is_preserved`；TXT/TSV 与原生导入文件/编码 | pending |
| AT-06 | 分组限制、停用、记忆但范围外 | Vault 候选过滤、修订/关联测试；核对原生管线不越组 | pending |
| AT-07 | 现有与处理中同名输出 | `output_rename_and_skip_preserve_previous_results_and_original`；改名窗口碰撞故障注入 | pending |
| AT-08 | 坏包、路径穿越、链接、资源限制 | `paths/scanner` 测试、Worker 协议脚本；ADS、设备名、大小写冲突与 reparse 实测 | pending |
| AT-09 | 解压/写回取消、异常退出 | `cancel_active_pipeline...`、`cancellation_remains_responsive...`；终止/进程退出/重启恢复故障注入 | pending |
| AT-10 | SSD 路径和跨卷写回，源不变、清理 | `ssd_nested_distinct_passwords...` 为同机人工路径测试；物理 HDD→SSD 跨卷需独立设备验收 | pending |
| AT-11 | 缓存配额、磁盘满/移除、目标不足 | 预算代码与错误状态；磁盘满、拔盘、目标断开真实注入 | pending |
| AT-12 | 清理失败、不删相似无关目录 | `owned_cleanup_cannot_remove_unmarked`；清理权限/句柄失败注入与遗留位置 | pending |
| AT-13 | DPAPI、备份错口令/篡改/损坏、迁移 | Vault 原生账户测试与备份测试；第二 Windows 账户/电脑迁移另验 | pending |
| AT-14 | 离线及脱敏 | 检查 CSP/IPC/日志，人工口令检索产物/数据库/参数；运行网络抓包验证常规流程 | pending |
| AT-15 | 大队列/DPI/拖放/选择器/键盘/读屏 | 前端单元测试 + 真实桌面 1 万队列、125/150/200% DPI、中文输入与操作 | pending |
| AT-16 | HDD→SSD 全流程性能对比 | 同一包、盘、模式测直解/SSD/双层，记录总耗时、各阶段和容量峰值；允许无加速 | pending |
| AT-17 | 双层、不同密码、混合普通文件、一次映射写回 | `ssd_nested_distinct_passwords...`；最终清单/树比对和阶段写回计数 | pending |
| AT-18 | 内包失败/同名/DOCX 容器不误展 | `two_nested_archives_with_same_stem...`、scanner 容器排除；缺内包密码后局部重试/部分交付 | pending |
| AT-19 | 标准/严格语义与回读 | `strict_scope_includes_actual_target_readback...`、Worker target tamper；确认标准不额外回读 | pending |
| AT-20 | 流中篡改/写回故障/复用封存 | Worker 摘要错配、读回篡改；写回中断后重试与源/封存状态核查 | pending |
| AT-21 | 多层/累计限制/峰值 | 深度上限、累计条目/字节测试，不能由释放中间文件绕过限制 | pending |

## 架构、故障与恢复验收

| 要求 | 必须观察的证据 | 状态 |
|---|---|---|
| 单 Worker 外包释放后展开多内包 | 多内包端到端通过；无父持唯一 Worker 等子；实际进程退出确认 | pending |
| 串行队列及暂停/取消 | 首版单根顺序；队列控制可执行；多盘许可反向申请属于 P1 并行实现后的专门验收 | pending |
| 配额拒绝及保留文件计量 | 当前尝试有界停止，保留工作区计入配额，资源操作不自等待 | pending |
| SQLite busy/损坏/不可写 | `busy_lock_has_a_finite_wait_and_can_recover`、事务回滚、损坏不空库覆盖 | pending |
| 进度洪泛/不读/断管道 | 单写入者、取消独立通道，结果不丢失，主进程实际退出确认 | pending |
| 原生引擎/传输挂起 | watchdog 协作/终止/Job Object/退出确认；未退出不清理或复用目录 | pending |
| 发布改名前后崩溃 | 持久意图+标记+清单核对，已交付不删、不重复成品或成功记录 | pending |
| Vault outbox 重复/迟到/修订 | Vault 修订、事件幂等、删除条目、Repository outbox 测试 | pending |
| 睡眠恢复/慢固实/清理失败 | 不误报死锁；输出成功与后处理分开；故障 UI 与位置可操作 | pending |
| 单实例/窗口 IPC 权限 | host 权限单元测试与实际重复启动；不可信 origin/越权路径不能调用 | pending |
| 可选源清理与单根剥离 | 默认 keep；完整成功、强制目标回读与源状态核对后才清理；回收站/永久删除单独验收 | pending |

## 构建、开源与发布验收

| 项目 | 证据与交付物 | 状态 |
|---|---|---|
| Gemini 协作 | `SECURITY.md` 由 Gemini CLI 实际 `write_file` 创建，status completed；维护者核对并修订 | 通过（文档生成） |
| 锁定依赖与 helper | 10/10 PowerShell/Node 脚本语法通过，Windows filtered metadata 成功；`artifacts/toolchain-smoke-*/result.json` 证明项目 SDK fallback 下真实 Windows.h/stdio.h 编译、kernel32/ucrt 链接与运行，rc/mt 路径检查通过；root 当前 host cargo check exit0，Release EXE 仍待验证 | 环境/host check 通过 |
| .NET 通知 | 10/13 自包含 Worker 升至 10.0.12；对应官方 LICENSE/THIRD-PARTY-NOTICES 固定 SHA 验证，collector 核对 580 个组件/1124 份文件；完整桌面安装包重新构建待验收 | 资料通过 |
| 7-Zip 对应源码 | 官方 26.04 source archive SHA 与 release digest 一致；10/10 collector 把对应源码/完整许可纳入材料并通过摘要审计 | 资料通过 |
| 所有依赖 notice/source | 10/10 collector 与 offline 复用均 exit0；488 组件、1030 个文件、52,132,400 字节；exact `.crate` 对 Cargo.lock checksum 再核对，材料自身逐文件 SHA 比对 | 通过（当前锁定依赖资料） |
| 源码 ZIP | 10/10 源码扫描 157 文件通过，ZIP 164 条目核对 required 文件与禁入目录成功；clean checkout 可还原构建仍待验证 | 部分通过 |
| Windows CI | 已编写 verify/build/zip/NSIS workflow；未发布 GitHub，未执行远程 CI | pending |
| 当前代码 Release EXE | fresh Release build，self-contained Worker，生产资源定位正确 | pending |
| 便携/NSIS/离线选项 | artifact+SHA，标准包缺 WebView2 可能联网；离线包包含运行时安装器 | pending |
| 干净机器安装/卸载/升级回滚 | Windows x64、无 Node/.NET SDK、WebView2 已有/缺失各验；用户库、源包、成品保留 | pending |
| 原生桌面冒烟 | 成品 EXE 启动，真实窗口/拖放/文件选择/队列/密码管理/恢复；浏览器预览不能代替 | pending |

当前不能据上述草案清单宣布整版验收完成。每次通过后将状态替换为带实际运行日期与证据位置的结论；未具备设备或格式样本的项保留 pending，并在发行说明明确范围。

## 2026-10-15 负载与工程优化补充证据

以 [本轮报告](load-optimization-report-2026-10-15.md) 和 `_workspace/load-optimization-2026-10-15/` 的冻结摘要/原始日志为准：上限提高到 5、默认保持 1，最终源码真实五 Worker 重叠复测通过；SSD/7z 的 1/2/3/5 档及 HDD 直写/缓存八轮完成。HDD 实际保持单盘串行，缓存路径在该样本中更慢。暂停新领取、真实进度后的单项取消/五 Worker 批停、正确输出与源不变、取消保留工作区计量通过。

最终 Rust 工作区 309 通过；默认忽略的 5 个负载入口、2 个物理盘入口和 1 个跨卷移动入口均另行执行通过。前端 133 项通过，严格 Clippy/格式/契约/镜像门禁通过。新安装/便携候选解包后匹配最终 Host 和全部 198 个 Worker 运行/许可文件，包内 Worker 各 14 项原生断言通过。

上述为当前 Windows NTFS 机器的后端、构建和解包证据。表中的原生 GUI、全格式真实样本、干净机器安装/升级/卸载、磁盘断开/拔除/满盘和其他文件系统等综合发布验收，仍按各自范围独立完成。10/1000 条合成历史快照样本未测 IPC 或界面渲染；主程序与安装包尚未签名。

## 2026-10-16 本机发布准备补充证据

详细范围与原始日志索引见 [本轮发布准备报告](release-readiness-report-2026-10-15.md)。统一 Worker 硬内存准入、auto/always 缓存、实际保留工作区预算、同卷免二次复制、有限队列快照、历史分页与按需详情已实施；最多 5 活跃任务、默认 1。Core 仍为生产链，启动全历史装载和 Host/WebView 全局硬限制不在本轮实现范围。

全工作区 351 通过、8 默认忽略的全量位于最后缓存修复之前；最后修复后另跑目录身份 1、真实解压 37、资源恢复 5、真实故障恢复 3 全部通过，严格 Clippy/格式/契约/镜像门禁通过。前端最终 148/148、类型及零警告检查通过。最终源码冻结 237 项，构建和原生验收后未变化；默认忽略的 5 个负载入口及 2 个物理盘、1 个跨卷入口另行全部执行通过，真实五 Worker 重叠、进度后取消/批停、源包不变、内容与退出确认通过。

真实便携 EXE 在独立数据目录完成 14 项已记录 GUI 检查：原生多文件/文件夹选择、队列 5 处理+2 排队、密码不足/一次性口令重试、归档浏览、31 条合成历史的第 4 页/搜索/按需详情、性能设置及重启。7 项真实解压的 14 文件 SHA-256/长度、7 个源包、publication 标记核对通过，重启后 38 条记录保持。合成历史不算真实解压。跨窗口拖放尝试被自动化窗界拒绝，仍需人工；UI 删除/暂停/取消按钮和 DPI/多显示器不记通过。

新标准 NSIS/便携包已解包匹配最终 Host（已知 bundle 标记差异单独处理），全部 198 项 Worker/runtime/许可匹配、便携 1331 条校验成功、包内 Worker 各 14 原生断言通过。同机独立审计产品安装/重复安装/卸载 4/4，1331 项完整安装摘要匹配，卸载后程序文件无残留、合成旧数据保持。该产品身份不同且跳过 WebView 安装，不能代替原版安装器的干净机/跨版本升级验收。

`readiness-20261016` 候选、最终源码包、验证 JSON 和独立校验文件已生成；此前带轮次的 10 项候选/校验摘要保持。Host/Setup 均 `NotSigned`，签名门禁正确失败。正式签名、干净 Windows/缺 WebView2、跨版本升级回滚、真实满盘/拔盘/SMB 中断及其他格式/文件系统验收仍需逐项完成，不能把本轮本机检查当整版正式发布批准。
