# OpenUnpack 依赖安全复核报告

审计日期：2026-10-17（北京时间）。范围：用户选择的第 2 项——Rust、前端、.NET、7-Zip 依赖复核。

**结论：`needs changes`。扫描和适用性核对已完成；发现 1 项随包运行时安全更新缺口、1 项自动检查覆盖不足。当前候选包不应仅凭包扫描的零告警进入正式发布。**

本轮只新增报告和审计证据，没有升级依赖、修改应用代码或覆盖候选包。修复尚未执行，人工 GUI、签名、干净机器等发布验收也不在本轮完成范围内。

## 1. 范围、方法与威胁模型

生产路径为 React/Tauri → Rust Core/WorkerClient → C# Worker → 7-Zip 原生引擎。攻击者可提供压缩包、文件名和压缩包元数据；程序还接收本机界面输入及经命名管道传递的 JSON 请求。重要资产为输出目录、用户原有文件、解压密码，以及桌面进程与 Worker 的完整性和可用性。

本轮按以下顺序执行：

1. 固定 16 项依赖清单、准备脚本及分发组件的 SHA-256，保留原始工作区状态。
2. 使用最新拉取的 RustSec 库扫描完整 Cargo.lock；保留未抑制的原始结果，再核对 Windows 构建图。
3. 使用 npm 官方源扫描前端锁文件，包括开发依赖；检查结果结构、退出状态和漏洞统计。
4. 使用 NuGet 官方源扫描解决方案中的三个项目及未纳入解决方案的 ProtocolChecks 项目，并另取完整包清单。
5. 单独核对自包含 .NET 运行时、7-Zip DLL、官方发行元数据和安全公告，补足包管理器扫描无法覆盖的内容。
6. 重新校验 20261016 三个候选文件的哈希，从安装包重新解出六项组件身份文件，并直接读取便携 ZIP 比较；核对当前 13 项源清单与候选源码 ZIP 一致。
7. 复核输入未变，输出适用性判断、风险和可执行的修复验收条件。

未知漏洞、依赖本身的全面源码审计、宿主 Windows/WebView2/系统级软件安全更新，以及未分发的 gemini-subagent 开发插件不包含在本次依赖复核结论中。零告警仅指此次查询的数据源没有返回匹配记录。

## 2. 执行结果

| 对象 | 范围与结果 | 判断 |
| --- | --- | --- |
| Rust | Cargo.lock 共 513 项依赖；RustSec 漏洞条目命中 0；另有 2 条提示 | Windows 当前目标无适用告警；提示保留，见下文 |
| 前端 | npm 报告总依赖 356，生产分类 73、开发分类 284；所有严重程度漏洞计数均为 0 | 本次数据库查询通过；分类有交集，不能相加 |
| .NET NuGet | Engine、Worker、Tests、ProtocolChecks 共 4 个项目，包清单无第三方 NuGet 包，漏洞命中 0 | 仅包层面通过，不代表自包含运行时通过 |
| .NET 随包运行时 | 实际为 Microsoft.NETCore.App 10.0.12；官方 2026-10-13 已发布 10.0.13 安全更新 | 需修复，DS-01 |
| 7-Zip | 实际 DLL 26.04；工具目录、Worker、安装包、便携包所核对 DLL 相同；固定 MSI 哈希与上游资产摘要一致 | 本轮未发现适用于 26.04 的已知公告；不作无漏洞保证 |
| 审计自动化 | 当前脚本主要检查 NuGet 包和 Cargo/npm，未动态检查随包运行时安全修补状态 | 需补充，DS-02 |

RustSec 数据库：1,334 条公告，提交 `64f92cdc64401fab47baa10e2e11fe382aa57155`，数据库更新时间 `2026-10-17T07:27:42+08:00`。本次 cargo audit 没有传入忽略条目。Windows 平台过滤后，从全部 workspace 根遍历所有依赖类型，覆盖 291 个包（包含工作区项目）。

工具版本：cargo 1.96.0、cargo-audit 0.22.2、Node.js 24.15.0、npm 11.12.1、.NET SDK 10.0.301。工具和公告快照均保存在证据目录。

## 3. 发现的问题与修复要求

### DS-01：自包含运行时仍分发 10.0.12，缺少已发布的安全修补

- **严重程度：High，组件安全基线层面。** 项目实际可利用性未证实；不能据此声称当前程序可被远程执行代码。
- **置信度：High（实际版本、公告版本范围和候选包身份均已核对）。**
- **状态：未修复，发布前应关闭。**

`artifacts/worker/OpenUnpack.Worker.runtimeconfig.json` 指定 10.0.12；实际 `System.Private.CoreLib.dll` 的产品版本为 `10.0.12-servicing.26422.108`。安装包与便携包中的运行时配置、CoreCLR、CoreLib、MsQuic、7-Zip 和引擎来源记录共六项文件，均与本机 Worker 文件逐项哈希相同。

`scripts/prepare-desktop.ps1:3` 默认发布 10.0.12；第 10 行只拒绝低于 10.0.12 的版本。`scripts/collect-release-materials.mjs:49` 的已审阅运行时名单也固定在 10.0.12。因此重新运行现有发布流程仍会保留该版本。

微软的 [.NET 10.0.13 发行说明](https://github.com/dotnet/core/blob/main/release-notes/10.0/10.0.13/10.0.13.md) 与[官方发行元数据](https://builds.dotnet.microsoft.com/dotnet/release-metadata/10.0/releases.json)均确认这是 2026-10-13 的安全更新。下表列出运行时仓库公告中明确覆盖 win-x64 10.0.12、修复版本为 10.0.13 的五项记录；它们是**组件版本命中数，不是五条已证实可利用的业务漏洞**。

| 公告 | 上游等级及影响 | 本项目适用性判断 |
| --- | --- | --- |
| [CVE-2026-70197](https://github.com/dotnet/runtime/security/advisories/GHSA-4mpw-cpwj-hxxj) | High，Sockets 特定异步数据报使用方式可导致越界写和代码执行 | 组件版本命中；生产 C# 源码未发现 `SocketAsyncEventArgs` / `ReceiveFromAsync` 调用，未建立攻击入口 |
| [CVE-2026-96233](https://github.com/dotnet/runtime/security/advisories/GHSA-87vj-4hqv-22p9) | High，特定日期解析可导致进程终止 | 组件版本命中；生产 C# 源码未发现 `DateTime.Parse` / `TryParse` / `ParseExact` 调用，未建立攻击入口 |
| [CVE-2026-69651](https://github.com/dotnet/runtime/security/advisories/GHSA-3rxq-g8rj-r2cf) | High，XML 处理中的资源消耗问题 | 组件版本命中；当前 IPC 使用 System.Text.Json，未发现 XML 解析入口 |
| [CVE-2026-58280](https://github.com/dotnet/runtime/security/advisories/GHSA-mc4q-93x5-f6hj) | High，XML 处理中的资源消耗问题 | 同上；没有证明压缩包内容进入此解析路径 |
| [CVE-2026-66822](https://github.com/dotnet/runtime/security/advisories/GHSA-wxxw-5cvc-53j7) | Medium，HTTP 重定向可能泄露代理认证信息 | 组件版本命中；当前 Worker 未发现 HttpClient/SocketsHttpHandler 调用，未建立攻击入口 |

本次对 `src` 的调用模式搜索和 Worker 入口阅读只提供静态适用性证据，不等同于对所有隐式框架调用的形式化证明。未执行漏洞利用或构造攻击样本。

同时排除了容易误报的范围：Tauri 桌面界面不等于 WPF；Worker 未引用 WindowsDesktop/WPF 或 ASP.NET。Unix 互斥锁公告不能直接归入 Windows 风险。MsQuic 公告中具体受影响的包版本也不能仅凭 `msquic.dll` 文件存在来命中；当前 DLL 产品版本为 `2.5.10.154561281`，且未发现 QUIC 业务调用。本报告没有将整份 .NET 发行说明中的所有 CVE 计为项目漏洞。

**具体修复与验收：**

1. 将 Worker 的审阅运行时基线更新为 10.0.13；统一准备脚本、发布检查和法律材料收集中的版本，保留显式版本校验。
2. 重新发布 win-x64 自包含 Worker 到独立的新目录，检查 runtimeconfig、CoreLib 产品版本和完整运行时载荷；不能只更新本机全局运行时，也不能只替换 Worker.exe。
3. 获取对应版本的 LICENSE/THIRD-PARTY-NOTICES，审阅并更新材料哈希、版本清单和完整载荷哈希；不得跳过现有法律材料门禁。
4. 使用更新后的真实 Worker/Engine 运行协议检查、原生压缩包回归、取消与资源回收检查，再构建新的安装包和便携包。只重跑 NuGet 扫描不足以验收本项。
5. 给新候选文件单独的日期/版本标识，核对完整分发载荷和清单，保留 20261016 旧文件及证据用于比较。修复后重新查询在线公告，确认选定版本仍满足安全基线。

微软在[运行时安全公告](https://github.com/dotnet/runtime/security/advisories/GHSA-4mpw-cpwj-hxxj)中明确要求，受影响的自包含应用需重新编译和部署。当前固定 SDK 10.0.301 也应在修复时与受支持的 SDK 安全更新一并评估；本轮没有执行系统级工具升级，也未把 SDK 版本差距计为已证实的生产漏洞。

### DS-02：自动依赖检查漏掉随包运行时的安全更新状态

- **严重程度：Medium。**
- **置信度：High。**
- **影响：** `scripts/audit-security.ps1:37` 查询的是 Worker 的 NuGet 包。项目没有第三方 NuGet 包时，可以得到零告警，但实际分发的运行时仍落后于安全更新；原有 10.0.12 固定白名单也不会自动知道新的安全公告。后续可能再次形成“扫描通过、运行时过旧”的发布候选。
- **状态：未修复。** 本轮通过单独检查运行时补足了这次审计，不代表自动化已经具备该能力。

**具体修复与验收：**

1. 在发布前检查中读取实际发布目录的 runtimeconfig，结合官方 .NET 发行元数据判断是否存在适用的后续安全更新。
2. 将“NuGet 包审计”和“分发运行时审计”分别记录状态；缺少数据源、解析失败、版本不明或离线跳过时，不应写成安全检查通过。
3. 检查范围应覆盖实际发现的全部 csproj；本轮额外扫描了未纳入解决方案的 ProtocolChecks 项目。
4. 增加必要的策略回归：旧运行时+零 NuGet 包应失败；已审阅修补版本应通过；元数据缺失/格式错误应失败；候选包与本机发布目录不一致应失败。
5. 对 7-Zip 保留官方发行版本、资产摘要和 DLL 身份检查，另记录公告复核日期；版本固定和哈希匹配证明来源与完整性，不能代替漏洞情报更新。

### DS-03 / DS-04：Cargo.lock 的非 Windows 提示需持续跟踪

| 项目 | 风险等级 / 置信度 | 影响与处置 |
| --- | --- | --- |
| proc-macro-error 1.0.4，RUSTSEC-2024-0370 | 当前 Windows 范围 Low / High | 上游停止维护；不在此次 Windows 过滤元数据中，也不在从工作区根可达的图内。保留原始提示，未来支持其他平台时追踪上游替代方案 |
| glib 0.18.5，RUSTSEC-2024-0429 | 当前 Windows 范围 Low / High | 特定 VariantStrIter 迭代实现有未定义行为；修复版本 >=0.20.0。不在当前 Windows 图内。将来启用 GTK/Linux 路径前，应更新相应上游依赖并重新核对调用路径，不能沿用本次 Windows 结论 |

这两项不是当前 Windows 发布的漏洞阻断项，也不能从锁文件删除后假装问题消失。公告原文与版本范围保留在未抑制的 `cargo-audit-raw.json` 中；每次构建目标、依赖或 feature 改动后都应重新证明适用性。

## 4. 7-Zip 来源与公告核对

实际引擎版本是 26.04，DLL SHA-256 为 `899cd73d7fc60e1c1c35c7de0f5bed3dc69bfe2cecbd48014d868a347970a0b1`。`scripts/setup-engine.ps1` 的固定 MSI 摘要为 `0b01334a418654293513449f61d6bfc99e5196ca371e3c3dc961eda57cd535c6`，与[官方 26.04 发行资产](https://github.com/ip7z/7zip/releases/tag/26.04) API 返回的 digest 一致。

[上游历史记录](https://www.7-zip.org/history.txt)列明 26.04 含安全修复，先前 XZ 内存破坏与 Mark-of-the-Web 问题分别已在 26.02、26.03 修复。此次未发现一条明确覆盖 26.04 的适用公告。该结论依赖本次可取得的官方记录和公告检索；不是对所有解析器、未知漏洞或应用输出路径防护的保证。不要因为某个文件扩展名不在界面列表，就推断原生解析器一定不会处理相应格式。

## 5. 证据、可复核性与最终门禁

证据目录：`_workspace/dependency-security-2026-10-17/`。

| 证据 | 用途 |
| --- | --- |
| `inputs-before.json`、`inputs-after.json`、`input-integrity.json` | 16 项输入前后哈希比较；全部一致 |
| `cargo-audit-raw.json`、`cargo-audit-exit.json`、`rustsec-advisory-db/` | 无抑制的原始 RustSec 结果、退出状态及数据库快照 |
| `cargo-windows-metadata.json`、`rust-applicability.json` | Windows 目标依赖图与两条提示不可达的证据 |
| `npm-audit-all.json`、`npm-audit-exit.json` | npm 官方源返回的完整锁文件扫描结果与退出状态 |
| `dotnet-audit.json`、`dotnet-protocol-audit.json` 及对应 exit 文件 | 四个项目的 NuGet 漏洞查询 |
| `dotnet-packages.json`、`dotnet-protocol-packages.json`、`dotnet-parser-validation.json` | 包清单及既有严格解析策略校验；避免将缺失结果误作零告警 |
| `dotnet-releases-10.json`、`dotnet-runtime-advisories.json`、`dotnet-runtime-applicability.json` | 官方 .NET 发行/公告快照与 win-x64 运行时版本匹配 |
| `7zip-release-26.04.json`、`7zip-history.txt` | 官方资产摘要和安全修复历史快照 |
| `binary-provenance-verification.json`、`fresh-setup-payload/` | 三个候选包哈希、六项分发依赖身份文件的新提取/比较结果 |
| `source-manifest-parity.json` | 当前 13 项源清单与 20261016 源码 ZIP 一致 |
| `summary.json`、`tool-versions.json`、`evidence-sha256.json` | 机器可读结论、工具版本与顶层证据文件哈希 |

可复核操作包括：cargo audit（指定本次数据库且不忽略公告）、cargo metadata（locked/offline，目标 x86_64-pc-windows-msvc）、npm audit（官方源、package-lock-only、ignore-scripts），以及 dotnet list package 的 vulnerable/include-transitive/json 查询。聚合脚本 `summarize-scans.mjs` 只读取已保存结果。`verify-binaries.ps1` 为保留证据而拒绝覆盖已有提取目录，重跑须选择新的证据位置。

本轮重新校验的旧候选文件未改变：安装包 SHA-256 `00d0803b6df7b5de6e6262cd57cfa31dfbb796c354cd1cb48efa516aace36287`；便携包 `e4daa06103bb799ee51e0b7e4b93e423cbcfe24481864d6a6f89b4edbf08243f`；源码包 `9558abb195ea202c5b7cfaf1987d8ec8bc17a57ae5e9747e41af7c4e03ac95e1`。

**发布阻断项：DS-01 的运行时安全基线缺口。** DS-02 应随该修复补齐以防回归。总体结论保持 `needs changes`；本次完成的是依赖安全复核，运行时升级、重新构建与验收仍是下一步工作。
