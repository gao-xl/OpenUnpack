# OpenUnpack 安全审计报告

日期：2026-10-13，Asia/Shanghai。结论：**needs changes（需要整改）**。正式发现 10 项：2 项 High、8 项 Medium；另列 2 项 Low 加固事项。未确认 Critical、SQL 注入、命令注入、XSS、IPC 身份验证绕过或静态 Zip Slip。

本次仅审计并提供修复方案，未修改业务代码。新增本报告及 `_workspace/security-audit-2026-10-13/` 下的独立验证材料。审计判断基于当前未提交工作区，不能仅以基准 commit 重现；关键文件摘要已保存。

## 范围、威胁模型与证据等级

基准 commit：`cf6e34411c809d092c48ed2f4bdd4ab6160222c1`；分支：`codex/open-unpack`。纳入当前修改、未跟踪的 Rust、TypeScript、C# 代码和构建/CI 脚本。识别 130 个应用源码/脚本入口，重点追溯安全相关调用链。`gemini-subagent`、`.pi` 等辅助工具不是应用运行链；没有对其单独插件安全性作出结论。没有读取个人密码库或实际用户归档。

真实主任务链为 React/Tauri → `Core Coordinator` → `Core WorkerClient` → .NET Worker → Engine。新 Application TaskCoordinator、Infrastructure Supervisor 和 Domain 解码内存预算尚未接入该主任务链；密码库 Application adapter 已接入同一个 Core Vault；SystemEnvironment 虽被构造和保存，当前没有生产调用。不能把全部新分层模块都当成未使用。

| 项目 | 内容 |
| --- | --- |
| 不可信输入 | 外部归档内容、格式元数据、文件名与分卷；密码导入/备份；目录选择及文件系统变化；WebView 命令参数 |
| 信任边界 | WebView → 原生宿主；宿主 → 文件授权范围；Host → Worker 命名管道；托管代码 → 原生 DLL；临时输出 → 正式发布；密码库 → 系统剪贴板 |
| 敏感资产 | 密码明文/加密备份、源归档、现有输出、任务数据库、当前用户文件、内存/CPU/磁盘和队列可用性 |
| 攻击者能力 | 提供恶意归档或目录内容；另行考虑能并发修改共享/同用户目录的主体。后者与仅提供归档的攻击者不同 |
| 应用身份模型 | 本机当前 Windows 用户与可信主窗口；不是具有登录、租户和远程 API 的服务。没有账户登录本身不属于认证漏洞 |

“动态确认”指使用独立、无害的小型样本调用当前代码或真实 Worker；“静态确认”指缺陷与可达调用链明确；标注潜在风险的内容没有完成实际资源耗尽或并发利用实验。本次没有执行真实资源耗尽、恶意代码、个人文件删除或系统目录修改。

## 发现清单

| ID | 严重度 / 可信度 | 问题 | 证据 |
| --- | --- | --- | --- |
| S01 | High / High | 密码探测绕过资源预算，缺少操作总期限和进程内存限制 | 小配额真实 Worker 动态确认 + 调用链 |
| S02 | High / High | 解压及默认流复制丢失互联网来源标记 | 带 ZoneId=3 的无害 ZIP 动态确认 |
| S03 | Medium / High | 密码复制可能进入 Windows 历史或云剪贴板 | 代码与微软接口文档；系统开关状态未读取 |
| S04 | Medium / High | 工具箱系统目录保护被规范路径绕过 | 只读调用当前 Rust 校验动态确认 |
| S05 | Medium / High | 工具箱执行未绑定已展示的预览 | 临时文本文件动态确认 + UI 调用链 |
| S06 | Medium / High | 工具箱不覆盖检查存在竞态；路径检查和使用分离 | 静态确认；实际竞态利用未执行 |
| S07 | Medium / High | LZ4 缺少完整性校验，截断帧可被当作成功 | 4 字节输出无害样本动态确认 |
| S08 | Medium / High | 防扩展点注入缓解策略使用错误枚举 | 实际策略查询 + 微软枚举核对 |
| S09 | Medium / High | 自动安全审计吞掉漏洞/扫描失败并仍可记录通过 | 静态确认失败分支 |
| S10 | Medium / High；ZIP 完整攻击链 Medium | 工具箱及主进程额外解析缺少统一资源预算 | 无界链静态确认；ZIP 目录边界小样本确认 |
| H01 | Low / High | 自包含 .NET Runtime 固定在已落后安全补丁的版本 | 实际发行资源和官方公告核对 |
| H02 | Low / High | 密码临时副本和部分 UI 状态清理不完整 | 静态确认；没有内存取证利用实验 |

### S01：密码探测绕过资源预算

**位置与可达性：** [Coordinator:1874](<C:/Users/im/Documents/ChatGPT/批量解压缩/crates/core/src/coordinator.rs:1874>) 在正常归档处理时先探测；密码库为空也执行。请求配置预算，但 [WorkerOperations:152](<C:/Users/im/Documents/ChatGPT/批量解压缩/src/OpenUnpack.Worker/WorkerOperations.cs:152>) 调用的 Probe 契约没有预算参数。[SevenZipEngine:146](<C:/Users/im/Documents/ChatGPT/批量解压缩/src/OpenUnpack.Engine/Native/SevenZipEngine.cs:146>) 枚举条目及 `:193` 的原生 test-mode 解码没有对应条目、实际解码字节限制；`:942` 的探测回调未约束工作量。

[Core WorkerClient:266](<C:/Users/im/Documents/ChatGPT/批量解压缩/crates/core/src/worker.rs:266>) 的 JobObject 只有退出联动等限制，未配置进程内存或 CPU 时间上限。`:514` 每次响应刷新计时，`:535` 的 120 秒是无响应超时；[WorkerHost:176](<C:/Users/im/Documents/ChatGPT/批量解压缩/src/OpenUnpack.Worker/WorkerHost.cs:176>) 的持续心跳不会证明解码有实际进度。

**动态证据：** 普通 ZIP 内含 4096 字节文本，Probe 请求设置 `maxEntries=1`、`maxTotalBytes=1`、`maxWrittenBytes=1`，真实 Worker 仍返回 `completed / NoPasswordNeeded`。该实验确认预算绕过，没有尝试耗尽机器内存或 CPU。

**影响与前提：** 用户处理外部归档时，高膨胀比、高字典内存需求或长时间解码可以在真正解压配额检查之前占用 CPU/内存和队列执行容量。取消机制仍存在；不能描述为无法取消，也没有证明当前引擎可被利用执行代码。

**修复：** 将 Probe 的条目数、元数据字节、解码工作量/内存和操作期限纳入 Host↔Worker↔Engine 契约。先做受限元数据检查，再限定测试对象；设置 JobObject 内存/CPU 上限以及独立于心跳的总期限。注意原生 `Extract(testMode=1)` 不一定写输出流，单纯给 NullOutStream 加计数不足以保证解码预算。

**验收：** 小配额探测必须以 `ResourceLimit` 终止；持续有心跳但没有进度的任务仍按期限退出；采用受控、有限资源样本验证内存限制和取消后真实进程退出。

### S02：互联网来源标记丢失

**位置：** [SevenZipEngine:804](<C:/Users/im/Documents/ChatGPT/批量解压缩/src/OpenUnpack.Engine/Native/SevenZipEngine.cs:804>) 和 [Lz4Engine:84](<C:/Users/im/Documents/ChatGPT/批量解压缩/src/OpenUnpack.Engine/Lz4Engine.cs:84>) 创建默认数据流；[WorkerOperations:280](<C:/Users/im/Documents/ChatGPT/批量解压缩/src/OpenUnpack.Worker/WorkerOperations.cs:280>) 的缓存/跨盘复制也只复制默认数据流。运行代码没有读取或传播 `Zone.Identifier` 的逻辑，正式发布不会补回标记。

**动态证据：** 输入 ZIP 有 `Zone.Identifier / ZoneId=3`，解包得到 4096 字节 `safe.txt`，输出文件不存在该流。

**影响与前提：** 攻击者提供互联网归档，用户解压后再打开其中的可执行文件、脚本或 Office 文件时，依赖来源标记的 Windows/Office 检查可能减弱。这不代表应用自动执行了文件，也不代表所有系统策略都被绕过。微软说明 Office 的互联网宏保护依赖 MOTW：[官方文档](https://learn.microsoft.com/en-us/microsoft-365-apps/security/internet-macros-blocked)。

**修复：** 在缓存前读取并验证来源 zone，将其作为任务信任元数据随缓存、嵌套解包和发布传播；向所有实际输出补写受控的来源标记。仅允许应用构造该 ADS，仍拒绝归档自行声明任意 ADS。跨盘复制必须保留此安全元数据；目标文件系统不能保存 ADS 时明确告知用户保护降级，不能静默丢弃。可采用 Windows Attachment Services 完成最终文件安全处理。

**验收：** NTFS 上的普通/加密/嵌套 ZIP、7z、RAR、LZ4 与缓存/跨盘交付均保留 Internet zone；无标记的本地源保持既有行为；不支持 ADS 的目标有明确处理策略。

### S03：系统剪贴板历史与云同步可保留密码

**位置：** [clipboard:20](<C:/Users/im/Documents/ChatGPT/批量解压缩/apps/desktop-host/src/clipboard.rs:20>) 只设置 `CF_UNICODETEXT`；[Host:713](<C:/Users/im/Documents/ChatGPT/批量解压缩/apps/desktop-host/src/lib.rs:713>) 的 30 秒清理仅清空当前剪贴板，不撤回已经进入历史或远端设备的副本。

**影响与前提：** 用户开启 Windows 剪贴板历史或设备同步并复制密码时，明文可能超过预计的 30 秒窗口继续存在。这项风险无需 XSS。未读取本机开关，也未触碰用户当前剪贴板。

**修复：** 在同一个 OpenClipboard 事务内注册并设置 `ExcludeClipboardContentFromMonitorProcessing`，或同时设置 `CanIncludeInClipboardHistory=0`、`CanUploadToCloudClipboard=0`；安全格式写入失败则拒绝复制密码。继续使用 sequence 条件清理。微软的支持机制见 [Clipboard Formats](https://learn.microsoft.com/en-us/windows/win32/dataxchg/clipboard-formats#cloud-clipboard-and-clipboard-history-formats)。第三方监控软件仍不受该机制完全约束，提示文字应说明边界。

**验收：** 用合成密码在启用历史/同步的隔离测试环境验证，不进入 Win+V 历史或其他设备；后续复制其他内容时原有 sequence 保护仍正确。

### S04：工具箱系统目录黑名单失效

**位置：** [Host:1326](<C:/Users/im/Documents/ChatGPT/批量解压缩/apps/desktop-host/src/lib.rs:1326>) 规范化路径，`:1349` 传递 Windows canonical 表示；[toolbox:25](<C:/Users/im/Documents/ChatGPT/批量解压缩/crates/core/src/toolbox.rs:25>) 仅匹配 `c:\windows` 等普通字符串且固定 C 盘。

**动态证据：** 当前 Rust 函数对实际 Windows 目录普通形式返回拒绝，对 `fs::canonicalize` 得到的同一目录返回允许。实验只读校验，未操作系统目录。

**影响与前提：** 用户经选择器授权后，本应禁止的关键目录可进入批量改名/移动/空目录清理流程；仅影响进程已有权限能修改的内容，不会突破 Windows ACL，也不是提权漏洞。

**修复：** 统一普通、verbatim、UNC 和别名的路径语义，使用 Path 组件/稳定文件身份比较祖先关系；从 Windows Known Folder/System Directory API 获取实际保护位置，不硬编码系统盘。将应用密码库、任务数据和运行资源目录纳入必要的保护范围。

**验收：** 普通与 `\\?\` 形式、非 C 系统盘及适用别名一致拒绝；`Windows-backup` 等名字相近的普通目录不会被误拒绝。

### S05：最终操作没有绑定用户看过的预览

**位置：** [ToolboxPage:150](<C:/Users/im/Documents/ChatGPT/批量解压缩/apps/desktop-ui/src/components/ToolboxPage.tsx:150>) 保存预览；规则控件变化不清除该结果；`:172` 发送最新 `currentOptions` 和旧 `selectedPaths`。[toolbox:508](<C:/Users/im/Documents/ChatGPT/批量解压缩/crates/core/src/toolbox.rs:508>) 在执行时重算计划，`:514` 只按源路径筛选，没有核对原预览的目标、选项或文件版本。

**动态证据：** 在专用临时目录，预览将 `safe.txt` 改成 `preview-safe.txt`；改变规则后，持原批准源列表执行，实际产生 `unpreviewed-safe.txt`。该实验确认后端解绑；实际桌面 UI 的复现链来自源码检查。

**影响：** 普通“预览→改规则→确认”操作或预览期间文件变化，可能执行未展示的批量改名/移动。风险是文件完整性及确认约束失效，不需要假设 WebView 被攻破。

**修复：** 影响计划的任意选项变化均使前端预览失效；异步预览有 epoch，过期响应不得覆盖新计划。后端保存或签发短期、一次性预览 token，绑定完整选项、源/目标计划、源文件身份与版本；执行时仅允许所绑定计划，变化则要求重新预览。不得把 `approvedPaths=None` 默认为已批准全部。

**验收：** 规则、目录、递归、冲突策略或文件状态改变后不能执行旧预览；重放/伪造/过期 token 拒绝；并发预览不会让旧响应恢复执行按钮。

### S06：不覆盖承诺存在竞态，文件检查与使用没有固定对象

**位置：** [toolbox:543](<C:/Users/im/Documents/ChatGPT/批量解压缩/crates/core/src/toolbox.rs:543>) 检查路径，`:553` 检查目标不存在，然后在 `:572` 使用 `fs::rename`，任何失败都在 `:576` 回退到覆盖式 `fs::copy` 并删除源。另有 [Host:1291](<C:/Users/im/Documents/ChatGPT/批量解压缩/apps/desktop-host/src/lib.rs:1291>) 授权/检查后 `:1308` 再按路径打开导入文件，以及原生 Engine 的 reparse 检查后创建路径式输出。

**影响与前提：** 目标在检查后被其他进程创建，或允许写入祖先目录的主体替换目录项时，操作对象可以变化，现有目标可能被覆盖。共享目录、同步软件也可造成非恶意竞争。缺陷静态明确；未运行竞态利用，更没有确认普通静态归档即可任意删除或逃逸。

**修复：** 同卷移动采用禁止替换的原子操作；仅明确跨卷错误允许进入复制回退。跨卷使用排他创建的临时目标，复制、校验、刷新后禁止替换地提交，再按稳定身份清理源。读取授权应绑定已打开句柄的最终路径和文件身份；写入和删除应使用稳定父目录/文件句柄，并拒绝重解析。来源清理、workspace 清理也应沿用此对象固定原则。

**验收：** 检查后创建目标、目录项替换、普通 rename 错误、跨卷中断时，不覆盖目标、不误删替换后的源；验证 Windows 原生行为，不能只用函数模拟。

### S07：LZ4 校验不足，可以把截断数据当作完整成功

**位置：** [Lz4Decoder:66](<C:/Users/im/Documents/ChatGPT/批量解压缩/src/OpenUnpack.Engine/Lz4Decoder.cs:66>) 读取但不验证头校验；`:131` 与 `:137` 跳过块/内容校验；`:88` 遇 EOF 直接结束而不要求 EndMark；`:49` 的声明内容大小只用于上限预检，没有与最终输出核对。[Lz4Engine:108](<C:/Users/im/Documents/ChatGPT/批量解压缩/src/OpenUnpack.Engine/Lz4Engine.cs:108>) 报告 `IsCorrupted=false`；[WorkerOperations:139](<C:/Users/im/Documents/ChatGPT/批量解压缩/src/OpenUnpack.Worker/WorkerOperations.cs:139>) 宣称 `archiveCrcAndStreamSha256`。

**动态证据：** 无害帧的头校验不正确、缺少 EndMark 和已声明的内容校验，仍返回成功及 4 字节输出。

**影响：** 损坏或被修改的归档可被误认为完整成功；输出自算 SHA256 只保证之后的复制一致，不能证明原输入解码完整。[Coordinator:2291](<C:/Users/im/Documents/ChatGPT/批量解压缩/crates/core/src/coordinator.rs:2291>) 根据非损坏状态等条件允许源清理，因此开启自动清理时可能丢失唯一原包。未在此次验证中删除源文件；此项不是独立代码执行漏洞。

**修复：** 验证 LZ4 帧要求的 xxHash 头、块及内容校验；必须读取结束标记和完整尾部；核对声明内容大小，拒绝不支持的特性。用实际验证能力命名验证范围；失败不得发布或进入源清理条件。当前 `tests/OpenUnpack.Tests/Program.cs:161` 的 HC=0x00 正向样本应替换成独立生成、校验正确的样本。

**验收：** 每种截断位置、头/块/内容校验错误、长度不一致均返回 Corrupt，并保留原包；合法独立样本正常解压，实际 Worker/Coordinator 完成状态与清理门禁同步。

### S08：进程安全缓解策略传入错误枚举

**位置：** [WorkerSandbox:160](<C:/Users/im/Documents/ChatGPT/批量解压缩/src/OpenUnpack.Engine/WorkerSandbox.cs:160>) 使用 `SetProcessMitigationPolicy(3, extPolicy, ...)` 并忽略返回值。

**证据：** 官方枚举中 3 是 `ProcessStrictHandleCheckPolicy`，`ProcessExtensionPointDisablePolicy` 为 6。独立验证进程查询扩展点禁用标志，在调用本函数前后均为 0。[微软枚举文档](https://learn.microsoft.com/en-us/windows/win32/api/winnt/ne-winnt-process_mitigation_policy) 支持该判断。

**影响与前提：** 声称启用的旧式扩展点 DLL 阻断没有生效。需要当地扩展配置或注入能力，不能据此断言归档可直接执行代码。现有 token 特权剥离和托管写范围守卫也不是 AppContainer 或 OS 文件访问沙箱。

**修复：** 使用有类型的具名枚举和对应结构；检查返回值、记录不包含敏感信息的错误码；启动时以 GetProcessMitigationPolicy 核对预期状态。对必需防护失败制定明确退出/降级策略，不能静默声称成功。

**验收：** 新启动 Worker 的扩展点策略实际为开启；不支持的系统、调用失败和已经设置的策略均有正确处理。

### S09：安全扫描失败后仍可能记录全部通过

**位置：** [audit-security.ps1:32](<C:/Users/im/Documents/ChatGPT/批量解压缩/scripts/audit-security.ps1:32>) 不检查依赖查询退出码；`:33` 依赖英文严重度关键字；`:36` 即使主动抛出漏洞错误，也在 `:38` 被通用 catch 捕获并作为离线/跳过处理。随后构造的汇总仍固定为通过。`-Offline` 跳过依赖查询也没有完整的 skipped 状态。

**影响：** NuGet 查询失败、输出语言变化或真实检测到漏洞时，CI 可能产生错误的安全证明，使存在问题的发行产物继续进入构建流程。本次直接执行 NuGet 查询成功且未报告包漏洞；这个事实不能修复脚本失败分支。

**修复：** 明确检查退出码，并使用可解析的结构化输出；分别记录 passed、failed、skipped。漏洞结果和 CI 网络/扫描错误必须使安全门禁失败；如允许离线工作，最终报告不能称全部扫描通过。摘要从真实扫描结果产生，不硬编码零问题；将包含自带 .NET Runtime 和原生 7-Zip 的发行 SBOM 纳入扫描范围。

**验收：** 用隔离模拟器输入高危结果、非零退出、超时、非英文输出和离线场景，核对退出码与摘要；不要在测试中覆盖真实验证证明。

### S10：工具箱和额外 ZIP 名称解析未统一限制资源

**位置：** [toolbox:349](<C:/Users/im/Documents/ChatGPT/批量解压缩/crates/core/src/toolbox.rs:349>) 可遍历并完整积累/排序大量文件及计划；规则 `:282` 按文件重复编译。宿主后台操作没有对应取消与并发上限，规则字段没有统一后端长度预算。不是传统灾难回溯 ReDoS：该处使用 Rust regex。

[zip_encoding:45](<C:/Users/im/Documents/ChatGPT/批量解压缩/crates/core/src/zip_encoding.rs:45>) 检查目录未超过整个文件，但后续读流没有限定在 `cd_size`；读取名称后有克隆和再次解码分配。[Coordinator:942](<C:/Users/im/Documents/ChatGPT/批量解压缩/crates/core/src/coordinator.rs:942>) 在 inspect 后调用，`:2077` 在提取后调用。Worker 自己的元数据预算不能自动约束这些后续主进程分配。

**证据与影响：** 当前解析器在极小样本声明 central directory 长度为 0 时，仍读出 2 个目录记录，确认目录边界校验缺陷。该异常样本没有证明能通过前置 7-Zip，因此不宣称完整恶意归档攻击链已复现。大目录扫描、未限制的路径/名称累计分配可导致应用或 WebView 长时间忙碌、内存上涨，具体资源耗尽阈值未测试。

**修复：** 统一条目数量、名称长度/深度、累计元数据字节、总时间及并发限制；流式或分页返回计划，加入取消；规则限制输入长度并编译一次。ZIP 编码识别优先复用可靠 Worker 元数据，或用明确的目录范围 reader 严格验证 EOCD、目录大小和记录一致性，将解析移出主进程。前端归档树使用迭代算法并限制节点/深度；当前极深树导致实际桌面崩溃的路径仍待验证。

**验收：** 受控大目录、长规则和有限异常 ZIP 样本在预算内退出；取消后没有继续后台累积；验证实际 Native→Host 路径而非仅解析器。

## 两项低优先级加固

**H01：自包含运行时补丁落后。** [prepare-desktop.ps1:11](<C:/Users/im/Documents/ChatGPT/批量解压缩/scripts/prepare-desktop.ps1:11>) 强制发布 .NET 10.0.9；实际 `artifacts/worker/OpenUnpack.Worker.runtimeconfig.json` 也包含 10.0.9；[collect-release-materials.mjs:49](<C:/Users/im/Documents/ChatGPT/批量解压缩/scripts/collect-release-materials.mjs:49>) 阻止其他版本。官方已经列出 10.0.10–10.0.12 发布：[.NET 官方版本记录](https://github.com/dotnet/core/blob/main/release-notes/10.0/README.md)。例如 [CVE-2026-50528](https://github.com/dotnet/runtime/security/advisories/GHSA-qvw7-jm5c-6hqw) 的受影响范围包含 10.0.9，修复于 10.0.10，但其涉及 TLS/SslStream；当前 Worker 未发现此 API 路径，不能把它当成已确认可利用的 OpenUnpack 高危漏洞。WPF 公告也不能自动套用到已移除 WPF 的当前 Worker。

升级至发布时已验证的最新安全补丁（此次官方记录至少到 10.0.12），同时更新 SDK/运行时、资料摘要与 notices，重新编译并打包自包含资源。只更新用户机器的系统 .NET 不会修复旧自包含产物。以实际发行文件和 SBOM 为准持续扫描，不能把“无显式 NuGet 包”视为无运行时依赖。

**H02：秘密临时副本残留。** [Vault:482](<C:/Users/im/Documents/ChatGPT/批量解压缩/crates/core/src/vault.rs:482>) 为预览克隆秘密到普通 BTreeSet；解析结构等临时副本也没有统一敏感类型管理。部分即时密码/归档模态关闭仅隐藏，没有清空外层 Hook 状态（`QueuePage.tsx:236/257`、`shared.tsx:97/113`、`ArchiveBrowser.tsx:136`）。主要风险是扩大进程内明文存在时间；没有发现秘密被日志或网络主动输出，也没有进行内存攻击实验。优先借用现有字符串，确实需要拥有的副本使用明确清零的容器/类型，覆盖错误路径；前端在关闭、成功、任务切换和隐藏时清空状态。JavaScript 字符串不能承诺物理内存擦除。

## 已验证的现有防护与常见漏洞排查

| 类别 | 本次判断与证据 |
| --- | --- |
| SQL 注入 | 未确认。Core Repository `:92/:130/:205/:262` 及新 TaskStore 使用绑定参数；没有发现用户输入拼接到 SQL 语句的可达路径 |
| 命令注入 | 未确认。Worker 启动采用固定程序与独立参数；open_path 使用固定 explorer 参数，不经 shell；不自动执行解压内容 |
| XSS / 模板注入 | 未确认。UI 文件名、密码名称与错误采用 React 文本渲染；未发现 raw HTML/eval sink；CSP 限制本地脚本及 IPC 网络连接 |
| 认证/授权 | 36 个 Tauri command 均核验可信 main 窗口与本地 origin；main capability 和命令白名单；原生选择器批准路径后有组件边界检查。缺口见 S04–S06。扫描源目录选择同时授予写权限，建议按 source-read/output-write/toolbox-modify 目的细化，但不把用户之后明确批准的操作误报为越权 |
| IPC | CurrentUserOnly 双命名管道、双方 peer PID 核验、8 MiB 帧限制、JSON 深度上限和 v2 request/operation/generation 关联；不通过 stdin/stdout 传生产 IPC |
| 归档路径 | 拒绝绝对/父目录跳转、ADS、设备名、尾空格/点、大小写冲突、链接和保留 owner 名称；CreateNew 防止静态已有输出覆盖。未确认仅靠静态归档可越界 |
| 大清单 | Worker/Host 对 spill 路径、128 MiB 上限、条目与字节预算、长度和 SHA256 进行验证，并使用稳定文件句柄读取 |
| 密码库 | 当前用户 DPAPI；正常列表/快照无 secret；敏感协议 Debug 脱敏；重要对象 Drop/Zeroizing；备份 AES-256-GCM、随机 salt/nonce、PBKDF2-SHA256 默认 600000 次，有迭代/大小上限 |
| 导入/恢复确认 | Vault HMAC-SHA256 token 绑定内容、格式/组及 generation；恢复绑定备份摘要；错误或过期确认不会半提交；S05 的工具箱没有同级保证 |
| 发布/源清理 | 主解压发布路径使用禁止替换的最终移动；源清理要求完整读回、派生树关闭、无损坏/排除和 witness 匹配；进程退出未确认时不清 workspace。S06 的竞态条件和 S07 的错误完整性报告仍需处理 |
| 数据库迁移 | OS lease、硬链接/重解析拒绝和版本检查；未发现格式迁移授权绕过 |
| 构建供应链 | npm/Cargo 锁文件；GitHub Actions 固定 commit 且 contents:read；7-Zip 官方 MSI/hash 固定，发行材料复核引擎摘要；源码发布白名单及秘密模式扫描 |

密码库 DPAPI 不能抵抗同一个 Windows 用户下的恶意进程；普通路径和 managed WriteScopeGuard 不是 OS 沙箱。任务路径、文件名和 settings 在本地明文存储属于现有公开边界，应通过用户目录 ACL、保留期限及诊断脱敏降低泄露；本次未把这些设计边界单独报成认证漏洞。没有全面扫描 Git 历史、个人数据、已安装包签名或执行 Native fuzz，因此不能给这些部分签发通过结论。

## 实际执行的验证

| 检查 | 结果 | 解释 |
| --- | --- | --- |
| `npm audit --json --ignore-scripts` | 356 个依赖记录；已知漏洞 0 | 同时查询生产与开发依赖；不能证明源码没有业务漏洞 |
| `cargo audit --json`（无忽略项） | 513 个依赖；vulnerabilities=0；2 项 warning | advisory database 最后提交日期 2026-10-12；告警见下文 |
| `cargo tree --locked --offline --target x86_64-pc-windows-msvc -i proc-macro-error` / `-i glib` | 两者均没有 Windows 构建依赖链可打印 | 锁文件存在不等于进入目标产物 |
| `dotnet list src/OpenUnpack.Worker/OpenUnpack.Worker.csproj package --vulnerable --include-transitive` | 成功；未报告易受攻击包 | 不足以覆盖自包含 Runtime/原生引擎 |
| `dotnet build tests/OpenUnpack.Tests/OpenUnpack.Tests.csproj -c Release -p:UseSharedCompilation=false` | 成功，0 warning / 0 error | Roslyn security analyzers 已启用；静态工具未发现 S01/S07/S08 仍不等于缺陷不存在 |
| `dotnet run --project tests/OpenUnpack.Tests/OpenUnpack.Tests.csproj -c Release --no-build -- .` | 51 passed，0 failed | 使用真实 7-Zip；结果目录 `artifacts/test-results/20261012-231501-b10fc1c8`（目录名为宿主 UTC） |
| `npm test -- --reporter=dot` | 11 文件，118 passed | 使用前端测试环境和 mock IPC，不能代替真实桌面验收 |
| `cargo test -p openunpack-core --lib --locked --offline -- --test-threads=1` | 98 passed，0 failed | 包含当前用户 DPAPI、数据库 lease、路径、manifest、恢复/清理边界等；不等于全部 workspace/发行验收 |
| 独立 .NET 小样本 / 真实 Worker | S01、S02、S07、S08 动态证据已保存 | 未运行内存耗尽或恶意代码 |
| 独立 Rust 小样本 / 当前 Core library | S04、S05 和 ZIP 目录边界动态证据已保存 | 只修改隔离临时文本，不触碰系统目录 |

Rust 两项告警：`proc-macro-error 1.0.4` 的 [RUSTSEC-2024-0370](https://rustsec.org/advisories/RUSTSEC-2024-0370.html) 是停止维护；`glib 0.18.5` 的 [RUSTSEC-2024-0429](https://rustsec.org/advisories/RUSTSEC-2024-0429.html) 是 VariantStrIter 不安全实现，修复版本 >=0.20.0。此次 Windows 目标反向依赖检查均没有进入构建链；不能称 Windows 发行版已命中这两项可利用漏洞。若未来支持其他平台，应重新评估并升级父依赖，豁免应绑定目标、原因与复核时间。

实际本地 `tools/7zip/7z.dll` 与 Worker 资源均为 26.04，脚本固定该版本与 MSI SHA256；[7-Zip 官方历史](https://www.7-zip.org/history.txt) 在此次查询列出 26.04。上游版本修复不自动修复本项目自写回调、复制和 LZ4 解析器的 S02/S07 问题。

验证材料目录：`_workspace/security-audit-2026-10-13/`，包含 `scope.txt`、`worktree-status.txt`、`source-hashes.json`、`native-probe-results.log`、`rust-probe-results.log` 及隔离样本。所有样本均为合成文本，不包含个人密码。

## 整改顺序与安全编码要求

1. **发布阻塞：S01、S02。** 将每个解码阶段纳入实际进程预算；保留来源标记并验证真实文件安全元数据。高可信 High 问题未关闭前，本次 verdict 为 needs changes。
2. **先修数据保护：S03–S07。** 防历史/云剪贴板、统一受保护目录识别、绑定预览、原子不覆盖、完整性失败禁止发布与清理。
3. **再修防护可验证性：S08–S10 与 H01/H02。** 具名 OS API 枚举并核查状态、CI 失败关闭、全阶段元数据预算、发行 Runtime 更新和秘密生命周期。

所有输入验证必须在实际可信宿主/Worker 后端执行，前端限制仅用于交互反馈。文件授权和确认应绑定用途、计划与实际文件对象，不能只保存路径字符串。摘要只能证明被摘要的数据本身，不能替代独立的格式校验或来源信任。依赖例外、离线扫描和未验证边界必须真实显示状态。

修复完成后的验收至少覆盖真实 Worker 和原生 Windows 文件系统；MOTW、剪贴板历史/同步、文件替换竞态、进程限制与完整源清理流程均需单独验证。现有全部通过的测试不应被解释为这些新增安全边界已经验收。
