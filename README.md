# 解压工坊 OpenUnpack

<p align="center">
  <strong>专为 Windows x64 打造的高性能、安全可控的批量归档解压与本地密码管理桌面应用</strong>
</p>

<p align="center">
  <img src="https://img.shields.io/badge/Platform-Windows%20x64-0078D6?logo=windows&logoColor=white" alt="Platform">
  <img src="https://img.shields.io/badge/License-MIT-green.svg" alt="License: MIT">
  <img src="https://img.shields.io/badge/Rust-1.96+-DEA584?logo=rust&logoColor=white" alt="Rust 1.96+">
  <img src="https://img.shields.io/badge/.NET-10.0-512BD4?logo=dotnet&logoColor=white" alt=".NET 10">
  <img src="https://img.shields.io/badge/UI-React%20%7C%20Ant%20Design-61DAFB?logo=react&logoColor=white" alt="React & Ant Design">
  <img src="https://img.shields.io/badge/Engine-7--Zip%2026.04-orange" alt="7-Zip Engine">
</p>

---

## 目录

- [项目简介](#项目简介)
- [核心特性](#核心特性)
- [系统架构](#系统架构)
- [格式支持与验收边界](#格式支持与验收边界)
- [快速上手](#快速上手)
  - [系统要求](#系统要求)
  - [安装与运行](#安装与运行)
  - [基础使用流程](#基础使用流程)
- [从源码构建](#从源码构建)
  - [环境准备](#环境准备)
  - [构建与调试命令](#构建与调试命令)
- [安全模型与数据保护](#安全模型与数据保护)
- [生态与自动化能力](#生态与自动化能力)
- [社区与项目规范](#社区与项目规范)
- [开源许可证说明](#开源许可证说明)

---

## 项目简介

**OpenUnpack** 针对日常海量压缩包处理场景设计（尤其适用于冷存储机械硬盘、多层嵌套压缩包、分卷压缩以及分散密码管理）。

软件通过“**本地密码库自动匹配** + **机械硬盘按需暂存到高速 SSD** + **多层递归自动展开** + **原子化校验回写**”的技术链路，在最大化保护原始数据安全的前提下，大幅消除解压等待与手工输密负担。应用以原生独立的 `OpenUnpack.exe` 交付，解压与密码匹配全部在本地物理机闭环运行，无任何云端数据上传行为。

---

## 核心特性

- **多语言与现代化交互**
  - 原生内置简体中文、English、日本語三种界面语言，支持在“设置与备份 → 界面与备份”中实时热切换并记忆；托盘菜单及退出提示同步跟随；
  - 支持浅色模式、深色模式及跟随系统，提供窗口置顶、托盘常驻后台及自定义完成提示音；
  - 支持文件/文件夹拖拽输入、递归目录扫描、分卷文件智能归组与输入排除预览。
- **本地密码管理与安全保护**
  - 密码库基于 Windows DPAPI（绑定当前 Windows 登录账户）提供硬件级安全加密；
  - 支持密码分组、即时过滤搜索与启停控制；
  - 支持单行 TXT 及带表头的 TSV/CSV（名称、密码、分组、备注）批量导入与去重预览，首尾空格忠实保留；
  - 提供独立口令加密的全量备份导出，在恢复导入时提供可视化差异预览与合并确认。
- **智能化解压与调度引擎**
  - **多层嵌套递归展开**：自动检测并解压压缩包内部的内层归档，支持配置最大嵌套层数与累计解压上限，防止“解压炸弹”耗尽存储；
  - **SSD 工作暂存加速**：针对机械硬盘（HDD）到机械硬盘的解压瓶颈，支持按需将任务暂存至高速 SSD 缓存区处理，完成后写回最终目标，兼顾容量与吞吐；
  - **稳健并发控制**：并发上限为 5 个任务，默认顺序处理（1 个）；系统根据实际物理磁盘介质类型（机械盘 vs 固态盘）智能仲裁并发度；
  - **生命周期管控**：支持任务暂停、取消、失败重试与执行详情追溯；内层压缩包缺密时支持“补充密码重试”或显式“交付部分成果”。
- **文件保护与完整性校验**
  - 默认**绝对保留源压缩包**；仅在显式配置且完整解压成功后才执行送回收站或彻底删除；
  - **解压后清理压缩包**：在“设置与备份 → 完成后动作 → 源压缩包处理”或添加任务时的“本批次选项 → 完成后源包”选择回收或直接删除。启用“展开内包”后，成功展开的各层内包及其分卷不保留在结果中；全部展开、完整交付且目标回读校验通过后，再按所选策略处理原始外包及全部分卷。失败、取消、部分交付、达到层数限制或使用输出筛选时保留原始外包，未展开的内包继续保留；保留策略仍会保留原始外包及分卷；
  - 同名目标目录支持“自动重命名编号”或“跳过”，严禁无提示盲目覆盖旧数据；
  - 提供两级校验保障：
    - **标准校验**：解压输出流与目标写入流实时比对 SHA-256，兼顾性能与数据一致性；
    - **严格校验**：写入完成后对目标文件进行全量逻辑回读比对。

---

## 系统架构

OpenUnpack 采用严格的**跨进程隔离边界**，划分为前端界面、Rust 协调中心与 C# 原生执行 Worker 三层：

```
┌────────────────────────────────────────────────────────┐
│             React / TypeScript / Ant Design            │  前端展示与交互
│              (Microsoft Edge WebView2 承载)             │  (apps/desktop-ui)
└───────────────────────────┬────────────────────────────┘
                            │  Tauri IPC 消息通道
┌───────────────────────────▼────────────────────────────┐
│              Rust 宿主与任务协调中心                    │  单一状态真源
│     (apps/desktop-host, crates/core, crates/protocol)  │  状态机 / DPAPI / SQLite
└───────────────────────────┬────────────────────────────┘
                            │  双向独立命名管道 (命令通道 + 进度通道)
┌───────────────────────────▼────────────────────────────┐
│             C# / .NET 10 Worker 执行进程               │  独立沙盒进程
│            (src/OpenUnpack.Worker & Engine)            │  进程崩溃不影响主界面
└───────────────────────────┬────────────────────────────┘
                            │  P/Invoke 动态加载
┌───────────────────────────▼────────────────────────────┐
│           官方未修改 7z.dll (7-Zip 26.04)              │  本地解码与流式解压
└────────────────────────────────────────────────────────┘
```

现行 Rust 源码归属、单一生产链和门禁见 [现行架构](docs/architecture-current.md)。Core 负责唯一生产编排，Infrastructure 提供共享 Worker、密码库和安全 IO；未接入的第二套任务/数据库原型已退役。

### 关键架构约束
1. **单一状态真源**：任务状态判定、发布与 SQLite 存储完全归属于 Rust 协调器；UI 与 Worker 均无权擅自修改数据库或擅自宣布任务成功；
2. **凭据隔离**：Worker 进程不拥有 SQLite 数据库权限，也无法获取完整密码库；仅在解压具体任务时接收协调器指派的有限候选密码；
3. **无锁化异步设计**：严禁持有状态读写锁跨越异步挂起点（`await`），规避死锁与管道阻塞；
4. **原子化发布流程**：工作暂存目录具备专属权限控制；只有在校验完全通过后才原子发布至目标路径；发布失败将保留现场供人工核对，绝不回滚误删成品。

---

## 格式支持与验收边界

项目坚持**实事求是**的技术原则。7-Zip 引擎具备广泛的格式解码能力，但功能宣称与实际经过回归测试的验收范围严格区分：

| 分类 | 格式与特性 | 说明 |
|---|---|---|
| **核心已验收格式** | `ZIP`, `7z`, `RAR` (RAR4/RAR5), 分卷压缩 (`.part1.rar`, `.001`, `.z01` 等) | 经过多场景人工样本与自动化回归用例全量验证 |
| **基础扩展支持** | `TAR`, `GZ`, `BZ2`, `XZ`, `ISO` 等 | 依赖 7-Zip 引擎底层原生解码器支持 |
| **多层嵌套展开** | 跨格式任意嵌套（如 7z 内部套分卷 ZIP 再套加密 RAR） | 具备层级熔断机制（默认限制嵌套深度与累计体积） |
| **暂不承诺范围** | 云端密码查询、暴力密码穷举破解、创建与打包压缩、多盘并行抢占式调度 | 属于首版开发范围之外的候选功能，请勿用于此类场景 |

> 详细的测试样本矩阵与已知边界见 [docs/acceptance-checklist.md](docs/acceptance-checklist.md)。

---

## 快速上手

### 系统要求

- **操作系统**：Windows 10 / 11 64位 (x64)；
- **运行时环境**：
  - **Microsoft Edge WebView2 Runtime**（Windows 11 通常已自带；Windows 10 若缺失，普通安装包会自动在线引导安装，离线安装包已内置）；
  - **.NET 运行时**：Worker 进程已内嵌自包含（Self-contained）的 .NET 10 运行时，用户**无需**单独安装 .NET SDK。

### 安装与运行

产物默认输出至 `artifacts/release/`：
- **便携版 (Portable)**：解压压缩包后，直接双击运行 `OpenUnpack.exe`（请保留同级目录下的 `worker/` 与物料文件夹）。用户配置与任务数据仍安全保存在本地 `%LOCALAPPDATA%\OpenUnpack` 中。
- **安装版 (NSIS Installer)**：双击执行安装向导，程序默认安装在 `%LOCALAPPDATA%\Programs\OpenUnpack`，与用户数据目录物理分离，后续升级与卸载不会清空您的密码库与任务记录。

> **关于未签名二进制**：本项目为开源项目，发布产物**不做 Authenticode 代码签名**。首次运行时 Windows SmartScreen 可能提示"未知发布者"，属预期现象，确认来自本项目发布页即可继续。请以下载后校验 SHA-256 为准（见下）。

### 完整性校验

每个发布包都随附 `SHA256SUMS.txt`，下载后请先核对再运行：

```powershell
Get-FileHash -Algorithm SHA256 .\OpenUnpack-0.2.0-windows-x64-portable.zip | ForEach-Object { $_.Hash.ToLower() }
# 与发布页 SHA256SUMS.txt 中对应条目逐字符比对，一致后再解压/安装
```

维护者侧对 Host 与 Setup 的完整性核查使用同一套门禁脚本（开源模式只记录并校验 SHA-256，不校验签名；若二进制确实带签名，则仍要求签名有效、具备 CodeSigning EKU 且匹配预期发行者）：

```powershell
pwsh -File scripts/verify-release-signatures.ps1 -HostExe <OpenUnpack.exe> -SetupExe <Setup.exe> `
  -EvidenceDirectory artifacts/release-signatures -AllowUnsigned
```

### 基础使用流程

1. **配置密码库**：进入“密码库”界面，手动录入常用密码，或通过 TXT/TSV 文件批量导入；导入时可在预览窗口确认解析结果；
2. **调整首选项**：进入“设置”界面，指定默认解压输出目录、同名冲突策略（自动重命名或跳过）。若解压源文件位于机械硬盘，建议在高速 SSD 盘符下指定一个缓存目录并启用“SSD 工作缓存”；
3. **添加任务并解压**：在“批量解压”界面中拖入文件或文件夹，核对扫描清单、分卷组合与排除提示，点击“开始解压”；
4. **异常排查与重试**：如遇解压失败，可点击任务查看详细执行日志。若因缺失密码导致失败，可在补充密码后针对该任务点击“重试”。

---

## 从源码构建

本项目采用自动化脚本管理构建环境与依赖，**必须在 64 位 PowerShell 7 (`pwsh.exe`)** 下执行。

### 环境准备

1. **Windows 10/11 x64**，安装 **PowerShell 7+**；
2. **Node.js 24.x LTS** 与 **Rust 1.96+**（`x86_64-pc-windows-msvc`）；
3. **.NET SDK 10.0.301**（须与根目录 `global.json` 一致）；
4. **Visual Studio 2022 C++ Build Tools**（必须包含完整 Windows SDK，脚本会通过 `vswhere` 探测 `Windows.h` 与 `ucrt.lib`）。

### 构建与调试命令

在仓库根目录下运行：

```powershell
# 1. 准备并编译前端及桌面物料
./scripts/prepare-desktop.ps1

# 2. 锁定获取 Rust 依赖并收集第三方许可资料
cargo fetch --locked --target x86_64-pc-windows-msvc
./scripts/collect-release-materials.ps1

# 3. 启动本地开发与调试窗口（支持热重载与原生桥联调）
./scripts/dev-desktop.ps1 -SkipPrepare

# 4. 执行全量原生协议与人工样本回归验证（不接触真实密码库）
./scripts/verify.ps1 -SkipInstall -WorkerProtocol

# 5. 打包便携版、源码包与标准 NSIS 安装包
./scripts/build-desktop.ps1 -SkipInstall -Installer

# 6. 打包集成 WebView2 离线安装器的完整分发包
./scripts/build-desktop.ps1 -SkipInstall -Installer -OfflineInstaller
```

---

## 安全模型与数据保护

数据安全与隐私保护是 OpenUnpack 的核心基石，详细技术规范与威胁模型见 [SECURITY.md](SECURITY.md)：

- **DPAPI 本地密保**：密码库明文受 Windows 数据保护 API 加密保护，密文密钥与当前 Windows 用户账户绑定；
- **便携保险库需显式启用**：Windows 上仅当设置 `OPENUNPACK_PORTABLE_VAULT=1`（或提供 `OPENUNPACK_VAULT_KEY`）时才会读取同目录 `.vault.key`；仅存在密钥文件不再自动切换便携模式，避免数据目录可写者植入密钥后接管保险库。升级后首次打开既有便携库需显式设置该变量；
- **口令传递**：实测 7-Zip 26.04 不会从重定向 stdin 读取口令（证据见 `artifacts/p2-09-stdin-proof/`），因此默认仍以 `-p<password>` 传给 7-Zip；设置 `OPENUNPACK_7Z_STDIN=1` 可启用 stdin 通道，供支持该特性的构建或不含敏感口令的场景使用；
- **全链路敏感信息脱敏**：控制台输出、任务记录与排错日志严禁输出密码明文、备份密码短语及用户隐私路径；
- **受保护的临时工作目录**：任务暂存区使用显式安全描述符，仅授予当前 Windows 账户完全控制权限，杜绝本地非授权进程访问中转文件；
- **路径遍历与注入防护**：针对压缩包内的相对路径、绝对路径、重解析点（Symlink / Junction）及 Windows 备用数据流（ADS）设置严格的规范化检查，相对路径限制 128 层，目录租约限制 256 层，超限输入直接拒绝；
- **页面 CSP 与守护进程接管**：WebView 的内容安全策略仅额外放行 `127.0.0.1` / `localhost` 回环地址的 HTTP/WS 连接（任意端口），用于前端切换至本机独立守护进程模式时接管业务调用与实时事件订阅；策略不含任何非回环地址，关闭守护进程后无出站连接。

---

## 生态与自动化能力

除图形界面外，OpenUnpack 提供了规范的本地自动化接入能力：

- **本机 HTTP API**：内置基于 Axum + Utoipa 的 OpenAPI 43 项受控接口，支持独立 API 密钥校验、任务批量提交与状态查询，默认处于关闭状态，详见 [docs/local-api.md](docs/local-api.md)；
- **本地 MCP 连接器**：提供符合 Model Context Protocol 标准的本地 stdio MCP 连接器（位于 `apps/mcp-connector/`），支持 AI 智能体编排批量解压计划、任务状态轮询与安全重试，详见 [apps/mcp-connector/README.md](apps/mcp-connector/README.md)。

### 无头模式（Headless Daemon）

以 `--headless`（别名 `-d` / `--daemon`）启动时可用以下参数：

| 参数 | 说明 |
|------|------|
| `--data-dir <路径>` | 覆盖数据目录 |
| `--port <端口>` | 覆盖本机 API 端口 |
| `--read-root <目录>` | 为默认 API 客户端授予只读目录，可重复 |
| `--write-root <目录>` | 为默认 API 客户端授予可写目录，可重复 |
| `--init-token` | 新建一个客户端并**打印完整令牌**（仅此一次）；不带该参数时只显示脱敏令牌 |
| `--trust-all` | 关闭全部路径授权（仅排障用；启动时会打印告警） |
| `--on-complete <脚本>` | 任务终态钩子，必须是已存在的绝对路径 |
| `--watch-dir <目录>` | 启动时注册监听目录，可重复 |

> 默认客户端只授予 `jobs.read/jobs.write/files.read/files.write/settings.read`，令牌 30 天过期；密码库读取与设置写入需在桌面端显式创建客户端。

---

## 社区与项目规范

我们遵循现代、规范的开源社区协作标准：

| 文档 | 说明 |
|---|---|
| [CONTRIBUTING.md](CONTRIBUTING.md) | 社区贡献指南、开发环境准备、跨栈协议对齐规则与提交规范 |
| [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md) | 贡献者行为准则（基于 Contributor Covenant 2.1） |
| [SECURITY.md](SECURITY.md) | 安全策略、威胁模型边界与漏洞私密披露指引 |
| [CHANGELOG.md](CHANGELOG.md) | 版本演进历史（遵循 Keep a Changelog 1.1.0 与 SemVer 2.0.0） |
| [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) | 第三方依赖、7-Zip 许可（LGPL + unRAR 条款）及 .NET 许可告知 |

---

## 开源许可证说明

OpenUnpack 应用程序自身源码基于 **[MIT 许可证](LICENSE)** 开源。

**第三方组件合规告知**：
- 本项目包含官方未修改的动态链接库 `7z.dll`（由 Igor Pavlov 开发），遵循 **GNU LGPL 2.1** 许可及部分 unRAR 限制条款（禁止使用该解压源码开发兼容 RAR 格式的压缩程序）；
- 内置的独立 Worker 包含 Microsoft .NET Runtime 组件，遵循其专有的开源分发许可与版权声明；
- 重新分发安装包或便携二进制时，必须完整保留 `LICENSE`、`THIRD_PARTY_NOTICES.md`、`worker/` 目录以及 `release-materials/` 目录下的所有许可材料与源码凭据。
