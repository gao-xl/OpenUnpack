# 解压工坊 OpenUnpack

Windows x64 批量解压桌面软件。管理自己保存的密码，把机械硬盘上的归档按需暂存到 SSD，展开内层归档，再把最终结果写回指定位置。项目应用源码采用 MIT 许可证。

最终运行入口是 **OpenUnpack.exe**。React / Ant Design 界面由 Tauri 的 Windows WebView2 窗口承载，文件访问、密码库、任务调度和解压在本机完成。浏览器中打开 Vite 地址只能预览界面；没有桌面桥时不会执行解压。使用安装版或便携版，无须启动开发服务器。

## 功能

- 文件/文件夹选择和拖入，递归扫描、分卷归组、输入排除预览。
- 密码分组、搜索、启用/停用、TXT/TSV 导入预览与去重。只尝试已保存的有限候选；不同内外包可使用不同密码。
- 当前 Windows 账户 DPAPI 保护密码库；独立备份口令加密导出，恢复前预览并确认合并。
- 顺序队列、阶段进度、暂停、取消、重试、任务详情和处理记录。内包失败可补密码重试，或显式交付含未展开内包的部分结果。
- 可选 SSD 输入缓存与工作目录、容量/余量限制、嵌套层数和累计展开限制。缓存是软件任务暂存，不是系统级磁盘驱动。
- 同名目录自动编号或跳过，不覆盖旧结果；输入/输出规则分别设置。标准流式校验或目标全量逻辑回读校验。
- 浅色/深色/跟随系统、置顶、托盘和提示音设置。默认保留原包；可选完整成功后送回收站或删除，部分/失败/待核对状态不触发源清理。

引擎能力与已验收范围必须分开看：ZIP / 7z / RAR 与分卷识别使用官方 7-Zip，实际格式、故障、桌面和设备测试见 [验收清单](docs/acceptance-checklist.md)。未通过的条目不能仅凭引擎支持宣称已经验收。CSV、云密码查询、穷举、创建压缩包、多盘并行、完成后关机属于首版之外的候选功能。

## 安装与开始使用

本地构建产物位于 `artifacts/release/`；当前没有公开下载仓库。便携包解压后运行 `OpenUnpack-portable/OpenUnpack.exe`，请保留旁边完整的 `worker/` 与 `release-materials/` 目录。便携包指免安装启动；用户数据默认仍写入 `%LOCALAPPDATA%\OpenUnpack`，不是把账户绑定密码库随 EXE 移动。

需要 Windows 10/11 x64 和 Microsoft Edge WebView2 Runtime。发行包内 Worker 自包含 .NET Runtime，用户无须单独安装 .NET SDK。普通 NSIS 安装包包含 WebView2 引导程序，缺少运行时会联网获取；离线安装选项包含 WebView2 离线安装器，体积更大。已有 WebView2 时，日常解压和密码管理不需要网络。

1. 在“密码库”添加密码，或导入 TXT/TSV 后核对预览。TXT 每行一个密码；TSV 使用“名称、密码、分组、备注”列。密码首尾空格按原文保留。
2. 在“设置”选输出位置、冲突策略、密码组。要使用 SSD，选择 SSD 上的缓存目录并开启缓存，预留足够空间。
3. 在“批量解压”添加文件或文件夹，核对扫描结果、分卷和排除提示，再开始。
4. 出错时查看任务阶段与处理记录。缺密码可补密码后重试；等待资源需调整容量/目录或清理保留工作区。发布待核对须走恢复入口，不直接把临时目录当成品。

标准验证在解压输出流和复制源流中计算并比对 SHA-256，同时检查写入错误；它不会另外把 HDD 目标全部读一遍。严格模式对目标逐文件逻辑读回，可能由 Windows 缓存满足，不能保证物理介质绕过缓存或掉电后永不丢失。SSD 是否更快取决于源/目标盘、解码负载、包大小和复制开销，项目不承诺固定倍速。

## 从源码构建

使用 **64 位 PowerShell 7**、Node.js 24、Rust 1.96 或更新的 MSVC 工具链、Visual Studio C++ Build Tools / Windows SDK，以及 `global.json` 对应的 .NET SDK 10.0.301。以干净机器的实际验收结果为准，不支持 Windows 7、x86 或 ARM64 发行目标。依赖获取、引擎/许可证收集及 WebView2 安装器下载需要网络；`Cargo.lock` 和 `package-lock.json` 锁定依赖，不承诺不同工具链环境下二进制逐位一致。

构建脚本会通过 `vswhere` / `vcvars64.bat` 初始化公开的编译器环境变量，并检查实际 Windows SDK 头文件和链接库。只有 SDK 目录、没有 `Windows.h` / `ucrt.lib` 的安装不算就绪；不会靠改 PATH 假装编译环境完整。

如果系统 SDK 不完整，脚本可使用 `artifacts/toolchain/sdk/` 中已下载并校验的官方 Microsoft Windows SDK NuGet 包作为备用路径，不安装到系统目录。此备用步骤本身不联网；源包没有包含 SDK，需要先由开发者准备系统 SDK 或对应的官方包。

在仓库根目录运行：

```powershell
./scripts/prepare-desktop.ps1
cargo fetch --locked --target x86_64-pc-windows-msvc
./scripts/collect-release-materials.ps1
./scripts/dev-desktop.ps1 -SkipPrepare
```

这是本机桌面开发窗口。单独运行 `npm --prefix apps/desktop-ui run dev` 仅做界面预览。

```powershell
# 验证真实 Worker / 原生引擎和人工样本；不接触个人密码库
./scripts/verify.ps1 -SkipInstall -WorkerProtocol

# 便携包、源码包及普通 NSIS 安装包
./scripts/build-desktop.ps1 -SkipInstall -Installer

# 包含 WebView2 离线组件的 NSIS 安装包
./scripts/build-desktop.ps1 -SkipInstall -Installer -OfflineInstaller
```

`-SkipPrepare` 仅用于已准备完整 Worker/UI 的工作区。`-OfflineMaterials` 只复用已经下载的许可证/源码资料，不代表依赖、NSIS 或 WebView2 构建全程无网络。源码 ZIP 使用显式目录白名单，排除个人数据库、备份、下载引擎、构建缓存、Gemini 网关子项目和配置；发行目录提供 SHA-256 校验文件。

## 项目结构

| 目录 | 作用 |
|---|---|
| `apps/desktop-ui` | React / TypeScript / Ant Design，桌面 API 桥 |
| `apps/desktop-host` | Tauri 窗口、原生选择器、权限边界和资源定位 |
| `crates/core` | Rust 扫描、密码库、SQLite 状态、协调器、Worker 客户端 |
| `src/OpenUnpack.Worker` | 独立 .NET 进程、双命名管道、7-Zip 与文件传输 |
| `src/OpenUnpack.Core` | Worker 使用的 C# 原生 7-Zip 适配及早期核心 |
| `src/OpenUnpack.Desktop` | 保留的早期 WPF 原型；当前发行不使用此入口 |
| `tests` / `crates/core/tests` | 人工测试样本、原生协议和端到端管线验证 |
| `scripts` / `.github` | 本机构建、验证、源码/便携/安装打包、Windows CI |
| `docs` | 规范历史、实现摘要和验收状态 |

## 数据与恢复

本地密码库与当前 Windows 账户绑定，复制原始库到另一个账户不等于可迁移备份。换电脑前导出加密备份并妥善保存口令；项目没有口令找回服务。`settings.json`、SQLite 任务记录和路径清单不以 DPAPI 加密，可能包含敏感文件名与路径。

程序重启后保留中断/待核对记录，不盲目覆盖现有输出。清理仅针对有可信归属标记的工作目录；发布已完成而清理失败时，应分别处理输出成功与清理待办。不要手工移动正在运行的工作目录，也不要通过删数据库来“修复”发布待核对记录。卸载或升级前导出密码库备份并保留用户数据；原始压缩包和有效成品不是版本回滚的删除对象。

贡献与安全报告见 [CONTRIBUTING.md](CONTRIBUTING.md) 和 [SECURITY.md](SECURITY.md)。第三方组件保留各自许可，尤其 7-Zip 的 LGPL / BSD / unRAR 限制与 .NET Runtime 通知，见 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。
