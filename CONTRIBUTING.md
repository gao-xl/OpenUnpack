# OpenUnpack 贡献指南

感谢你对 OpenUnpack 项目的关注与支持！

OpenUnpack 是一个专注于 Windows x64 平台的批量归档解压与本地密码管理工具。在设计上，我们坚持**数据安全第一**、**权限与路径显式可信**、**跨语言契约严格对齐**以及**不掩盖客观缺陷**的工程原则。

为了保障代码质量、系统的稳定可靠与社区的高效协作，请在参与贡献前仔细阅读本指南。

---

## 目录

- [1. 行为准则与基本原则](#1-行为准则与基本原则)
- [2. 报告缺陷与提出建议](#2-报告缺陷与提出建议)
- [3. 本地开发环境准备](#3-本地开发环境准备)
- [4. 系统架构与模块约定](#4-系统架构与模块约定)
- [5. 跨栈修改与协议对齐规范](#5-跨栈修改与协议对齐规范)
- [6. 分支管理与提交规范](#6-分支管理与提交规范)
- [7. 本地构建与验证流程](#7-本地构建与验证流程)
- [8. Pull Request 提交流程](#8-pull-request-提交流程)
- [9. 开源许可证与知识产权](#9-开源许可证与知识产权)

---

## 1. 行为准则与基本原则

本项目遵循 [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md) 所定义的行为准则。在参与讨论、提交代码或进行评审时，请保持专业、客观与尊重。

参与开发时需恪守以下基本底线：
1. **绝不破坏用户数据**：默认保留源压缩包；禁止自动删除用户未确认的数据；禁止无提示覆盖已有文件。
2. **严守数据安全边界**：密码库使用 Windows DPAPI 保护；严禁将明文密码、备份口令、私有路径打印至日志或提交至仓库。
3. **实测与证据为准**：任何功能宣称必须有实际测试覆盖；未验证的格式变体、异常场景或外设状态须如实标注为待验证（pending）。

---

## 2. 报告缺陷与提出建议

### 2.1 报告安全漏洞
如果您发现了可能涉及权限越界、路径遍历（Path Traversal）、命名管道非法提权或凭证泄露等安全隐患，**请勿通过公开 Issue 提交**。请严格遵循 [SECURITY.md](SECURITY.md) 的说明，通过私下渠道或 GitHub Security Advisories 提交。

### 2.2 报告常规缺陷 (Bug)
提交缺陷报告前，请确认：
- 查阅现有 Issue 列表，确认该问题未被重复提出；
- 使用**人工合成（Synthetic）且非敏感的测试样本**进行复现，严禁上传包含个人隐私、机密数据或真实业务内容的压缩包；
- 清洗错误日志：在贴出日志时，抹除用户名、个人绝对路径、网络标识等敏感元数据；
- 按照项目提供的 Issue 模板详细填写：Windows 构建版本、OpenUnpack 版本、归档格式类型、重现步骤与实际行为。

### 2.3 提交功能建议 (Feature Request)
我们欢迎对解压性能、调度策略、交互体验等维度的合理建议。提出功能建议时，请说明具体的使用场景与解决的痛点，并充分考虑对磁盘 I/O 竞争、系统资源占用及安全边界的影响。

---

## 3. 本地开发环境准备

OpenUnpack 是一个跨 Rust、TypeScript/React 与 C#/.NET 的复合桌面项目，针对 Windows x64 平台深度优化。

### 3.1 必需工具链与运行时

| 工具/依赖 | 最低版本要求 | 用途说明 |
|---|---|---|
| **操作系统** | Windows 10/11 x64 | 必须运行在真实或未锁定的 Windows 环境 |
| **PowerShell** | PowerShell 7+ (64-bit `pwsh.exe`) | 执行构建与验证脚本（系统自带 Windows PowerShell 5.1 会因编码问题解析失败） |
| **Rust** | 1.96+ (MSVC 工具链) | 核心协调器、状态管理与 Tauri 宿主 |
| **.NET SDK** | 10.0.301 (`global.json` 锁定) | Worker 进程与 7-Zip 原生引擎适配层 |
| **Node.js** | 24.x LTS (配合 npm) | React 前端界面构建与本地 MCP 连接器 |
| **C++ 构建工具** | Visual Studio 2022 C++ Build Tools | 提供 MSVC 链接器及完整 Windows SDK（须包含 `Windows.h` 和 `ucrt.lib`） |
| **WebView2** | Microsoft Edge WebView2 Runtime | Tauri 前端窗口运行时 |

### 3.2 环境变量与依赖配置
构建脚本通过 `vswhere` 与 `vcvars64.bat` 探测编译工具链。如果本地缺少标准安装的 Windows SDK，脚本会使用 `artifacts/toolchain/sdk/` 预校验的 NuGet 备用包。

在项目根目录下完成前端与原生依赖安装：
```powershell
# 1. 准备桌面依赖与物料
./scripts/prepare-desktop.ps1

# 2. 锁定获取 Rust 依赖
cargo fetch --locked --target x86_64-pc-windows-msvc

# 3. 收集发布物料与许可信息
./scripts/collect-release-materials.ps1
```

---

## 4. 系统架构与模块约定

OpenUnpack 由三个核心进程边界构成：

```
[ Web 前端 (React / AntD) ]
             ↕ (Tauri IPC / WebView2 桥)
[ 本地宿主与协调器 (Rust / crates/core, desktop-host) ]
             ↕ (双向双命名管道: 命令通道 + 进度通道)
[ 执行引擎 Worker (C# / .NET 10 + 7z.dll 进程隔离) ]
```

在编写或修改代码时，必须遵守以下架构约束：

1. **Rust 核心作为单一状态真源**：
   - 任务的生命周期、状态推进（待处理、解压中、暂停、成功、待核对、失败）以及最终文件发布全部由 Rust 协调器仲裁并持久化至 SQLite；
   - 前端 UI 仅作为状态的展现与命令触发入口，**UI 进程严禁自行判定或宣布任务成功**；
   - Worker 仅作为解压指令的执行体，**Worker 严禁直接访问 SQLite 数据库或解密密码库**。

2. **异步与锁操作约束**：
   - 严禁持有状态锁（如 `MutexGuard`、`RwLockWriteGuard`）跨越 `.await` 挂起点，以规避死锁风险；
   - 进程监督、进度转发、任务取消应解耦运行；资源回收操作必须显式等待底层进程或句柄释放。

3. **数据发布与不可逆操作红线**：
   - 成品发布应遵循原子语义：严禁直接将临时工作目录就地充当发布成品；
   - 绝不因后处理失败（如源文件送回收站失败、日志写入异常）而回滚或删除已成功发布的有效目标目录；
   - 目录与文件操作必须经过路径规范化及权限验证，阻断任何可能的重解析点（Junction/Symlink）攻击。

---

## 5. 跨栈修改与协议对齐规范

由于系统涵盖 Rust、C# 与 TypeScript 三种语言栈，任何涉及进程间通信的数据结构变更必须严格同步。

### 5.1 共享 DTO 与协议版本
- 通信消息定义位于 `crates/protocol` 与 Worker 对应的 DTO 定义中；
- 当变更字段、枚举值或阶段进度格式时，**必须同 PR 同步更新**：
  - Rust 协议定义：`crates/protocol/src/`
  - C# Worker 协议结构：`src/OpenUnpack.Worker/`
  - 前端类型声明：`apps/desktop-ui/src/`
- 保持协议版本号与尺寸边界验证一致，杜绝管道消息截断或反序列化失败。

### 5.2 单源实现与依赖方向
生产编排以 Core Coordinator 为唯一权威。Worker、Vault、manifest、安全 IO 和数据库租约在 Infrastructure 中各有一个实现；Core 的历史入口只再导出，修改行为时只编辑唯一实现。依赖方向为 Core → Infrastructure → Application / Domain / Protocol，下层不能反向依赖 Core。

运行 `node scripts/check-architecture.mjs --self-test` 验证依赖和源码所有权；禁止重新创建镜像文件、另一套任务协调器或可写 SQLite v2 管线。详见 [现行架构](docs/architecture-current.md)。

---

## 6. 分支管理与提交规范

### 6.1 分支模型
- 主分支为 `main`（或日常发布主线），保持时刻可编译、可测试状态；
- 开发新功能或修复问题请从最新主分支切出特性分支：
  - 功能特性：`feat/short-description`
  - 缺陷修复：`fix/issue-id-description`
  - 文档改进：`docs/scope-description`
  - 架构重构：`refactor/module-name`

### 6.2 提交信息约定 (Conventional Commits)
提交信息应具备自解释性，推荐采用 Angular / Conventional Commits 规范：

```
<type>(<scope>): <subject>

[optional body]

[optional footer(s)]
```

- **Type 类型**：
  - `feat`: 新增业务功能或支持新特性
  - `fix`: 修复程序缺陷
  - `refactor`: 代码重构（无功能增减或 Bug 修复）
  - `perf`: 性能优化
  - `test`: 增加或修改测试用例、回归验证夹具
  - `docs`: 文档变更
  - `build` / `ci`: 构建脚本、依赖更新、工作流配置调整
- **Scope 作用域**：如 `core`, `worker`, `ui`, `host`, `protocol`, `scripts` 等。
- **Subject 摘要**：以简明陈述句描述变更内容，避免无意义的“update”、“fix bug”。

---

## 7. 本地构建与验证流程

所有代码在提交 PR 前，必须在本地通过全量验证脚本。

### 7.1 本地桌面调试开发
```powershell
# 启动桌面调试环境（含 Tauri 宿主与 UI 开发窗口）
./scripts/dev-desktop.ps1 -SkipPrepare
```
*注：单纯运行 `npm --prefix apps/desktop-ui run dev` 仅启动 Vite 网页，不具备本地桌面原生调用能力。*

### 7.2 运行核心验证与回归测试
```powershell
# 验证真实 Worker / 原生 7-Zip 引擎与协议集成（使用人工样本，不接触个人密码库）
./scripts/verify.ps1 -SkipInstall -WorkerProtocol

# 执行 Rust 单元测试与协议回归测试
cargo test --workspace --locked
```

### 7.3 打包验证
```powershell
# 构建便携包、源码包及标准 NSIS 安装包
./scripts/build-desktop.ps1 -SkipInstall -Installer
```
安装包测试时请确认：
- 默认安装路径为 `%LOCALAPPDATA%\Programs\OpenUnpack`；
- 用户数据继续保持在 `%LOCALAPPDATA%\OpenUnpack`，安装与卸载流程不得误清用户已有任务与密码库。

---

## 8. Pull Request 提交流程

1. **Fork 并克隆仓库**至本地；
2. **在独立分支上进行开发**，保持改动职责单一；
3. **完成自测与自动化验证**，确保本地 `verify.ps1` 完全通过；
4. **填写 Pull Request 模板**：
   - 清晰陈述变更动机与行为差异；
   - 附带验证通过的测试命令与人工样本测试范围；
   - 如涉及跨栈契约，列出同步更新的文件清单；
   - 若存在尚未验证的边界（如特定 RAR 变体、极端并发、特异驱动器），请显式列出待验证声明；
5. **响应评审意见**：提交修改后保持代码整洁，待所有 CI 检查通过且维护者审核认可后合入。

---

## 9. 开源许可证与知识产权

- OpenUnpack 自身源代码采用 **MIT 许可证** 开源；
- 贡献者向本项目提交代码，即视为同意将贡献的代码按照 MIT 许可证授予本项目及所有下游用户；
- **第三方组件合规**：
  - 核心解压引擎依赖官方可替换的 `7z.dll`（遵循 GNU LGPL 2.1 规范及 unRAR 限制条款）；
  - 贡献中严禁直接复制未获授权的闭源专有代码、第三方受专利保护的解码算法或私有数据文件；
  - 引入任何外部第三方库必须附带完整的开源许可证说明，并通过 `scripts/collect-release-materials.ps1` 校验。
