# OpenUnpack v0.1.0-beta.1

首个**公开预发布（Beta）**。OpenUnpack 是面向 Windows x64 的批量归档解压与本地密码管理桌面应用：本地密码库自动匹配、机械硬盘按需暂存到高速 SSD、多层嵌套自动展开、原子化校验回写，全部在本机闭环运行，无任何云端上传。

> Beta 提醒：核心解压、密码库、监听文件夹与本机 HTTP API 已可正常使用；界面细节与部分边界场景仍在打磨。**请勿用于不可替代的唯一副本**，发现问题请提交 Issue。

## 下载与完整性校验

| 文件 | 说明 |
|---|---|
| `OpenUnpack-0.1.0-windows-x64-standard-setup.exe` | 标准 NSIS 安装包（默认安装到 `%LOCALAPPDATA%\Programs\OpenUnpack`，与数据目录物理分离） |
| `OpenUnpack-0.1.0-windows-x64-portable.zip` | 便携版，解压后直接运行 `OpenUnpack.exe`（请保留同级 `worker/` 与物料目录） |
| `OpenUnpack-0.1.0-source.zip` | 构建该产物对应的源码快照 |
| `SHA256SUMS.txt` | 全部发布文件的 SHA-256 校验清单 |

本项目为开源项目，**发布产物不做代码签名**，首次运行 Windows SmartScreen 可能提示"未知发布者"，属预期现象。**请先校验 SHA-256 再运行**：

```powershell
Get-FileHash -Algorithm SHA256 .\OpenUnpack-0.1.0-windows-x64-portable.zip | ForEach-Object { $_.Hash.ToLower() }
# 与 SHA256SUMS.txt 中对应条目逐字符比对，一致后再解压或安装
```

## 系统要求

- Windows 10 / 11 64 位（x64）
- Microsoft Edge WebView2 Runtime（Windows 11 通常自带；Windows 10 缺失时安装包会在线引导安装）
- Worker 已自包含 .NET 10 运行时，无需单独安装 .NET

## 本版本包含的核心能力

- **解压与调度**：双命名管道进程隔离（Rust 协调器 + 独立 C#/.NET 10 Worker，可替换官方 `7z.dll`）；任务队列与并发管控（上限 5，机械盘按实际介质仲裁并发度）；可选 SSD 工作缓存与中转目录；多层嵌套递归展开（含层数与累计上限保护）。
- **密码库与安全存储**：Windows DPAPI 加密（绑定当前 Windows 账户）；密码分组、搜索与启停；TXT/TSV/CSV 批量导入与去重；独立口令加密备份与恢复预览。便携库需显式设置 `OPENUNPACK_PORTABLE_VAULT` 才启用（fail-closed）。
- **校验与完整性**：标准流式校验（解压流与目标写入流实时 SHA-256 比对）与严格模式（逻辑目标重读校验）；同名目录自动编号或跳过；原子化发布，失败保留恢复现场，不回滚删除成品。
- **本机自动化**：Axum + Utoipa 的 OpenAPI 本机 HTTP API（独立密钥、目录授权、RFC 7807 错误模型）；WebSocket 实时进度与缺密码回调；无头守护进程模式；命令行客户端 `openunpack-cli`；本地 MCP 连接器（供 AI 智能体编排）。
- **智能化**：物理磁盘拓扑感知调度（HDD 排他串行化、引擎线程压制）；损坏归档抢救与部分提取；NTFS 硬链接去重；冲突决策与语义版控（7 种策略 + 内容感知跳过）；归档内选择性提取；智能密码挖掘（文件名特征 + 伴随文件嗅探）；冗余嵌套自动展平；ZIP 多语言编码自愈。
- **监听与集成**：监听文件夹自动入队，五层防抢读漏斗 + 独立 intake guard（环路自激、云盘占位符、P2P 预分配、磁盘爆盘等防护）；资源管理器右键菜单；下载器集成脚本；浏览器插件与 Python 客户端示例。

## 已知限制（Beta）

- 未做代码签名，SmartScreen 会提示未知发布者（已提供 SHA-256 校验方式）。
- 界面细节与部分边界场景仍在打磨；跨版本升级、干净无 SDK 机器、离线 WebView2 安装、断电/拔盘等组合故障场景**尚未完成完整验证**，请勿用于唯一副本。
- 7-Zip 引擎经实测不从重定向 stdin 读取口令，默认以命令行参数传递；可设置 `OPENUNPACK_7Z_STDIN=1` 启用备选通道。
- 本机 HTTP API 的目录授权是应用级限制，不构成针对同用户其他进程的操作系统沙箱；请勿将 API 暴露到公网。

详细变更记录见 [CHANGELOG.md](../CHANGELOG.md)，安全策略与威胁模型见 [SECURITY.md](../SECURITY.md)。
