# 验证记录

## 1.0.2 · 2026-10-08

已修复旧版安装缓存遗漏检查脚本与测试文件、文档未同步、安装脚本不能更新已有插件的问题。

- `npm test`：15 项全部通过，包含重复准备插件源不增加条目或备份、升级保留模型/网关与其他插件、凭据保持不变、失败恢复以及跨工作区隔离。
- `setup.ps1 -VerifyLive`：从实际安装缓存启动，MCP 版本与插件清单一致，工作区必填且默认只读。
- 真实网关读取测试通过：`gemini-3.8-flash-high`，2 轮、1 次工具调用，答案包含临时文件的真实随机标记。
- 包括完整检查脚本、测试和一键更新入口；源码、个人插件源、安装缓存和最终 ZIP 按相同发布内容同步。
- 复现 Windows 同版本运行缓存覆盖失败（os error 5）。1.0.2 安装器对已启用的相同版本只做完整文件一致性与启动检查；新版本使用新缓存目录。避免重装正在运行的同版本服务。

下面保留 1.0.0 的历史验证记录。

日期：2026-10-07。环境：Windows、Node.js 24.15.0、PowerShell 5.1 / 7。

固定模型：`gemini-3.8-flash-high`。网关：`https://gateway.yarrn.eu.org/v1`。

- `npm test`：11 项全部通过，包含 Windows PowerShell 启动的真实 MCP 子进程握手、工具列表和模拟多轮任务。
- `run.ps1 --models`：认证成功，指定模型存在；Windows PowerShell 5.1 启动也通过。
- `run.ps1 -Mode live`：真实 MCP 读取测试通过，2 轮、1 次工具调用；真实修改测试通过，4 轮、3 次工具调用。断言标记值、最终文件全部内容以及原始文件备份。
- 项目文档摘要委派：真实读取 `docs/software-spec.md`，2 轮、1 次工具调用，约 18 秒；返回中文摘要与行号。未改业务文件。
- MCP 注册配置：默认只读、启动 20 秒、工具 300 秒；使用 TOML 解析对比，原有 Codex 配置全部保留；密钥未写入 MCP 配置。
- 依赖安装审计：95 个包，0 个已知漏洞（安装当时的审计结果）。

实测网关最终消息含 `tool_calls: null`，已兼容并纳入回归测试。PowerShell 启动器参数绑定及 Windows PowerShell 5.1 脚本编码问题已修复；脚本自身使用 ASCII 文本，中文任务通过参数正常传递。

本机 npm 安装的 Codex CLI 无法解析桌面配置中的 `service_tier = "default"`。安装器因此直接追加官方支持的 MCP 配置段，不改动服务等级。保存的启动配置已通过 `scripts/check-install.mjs` 完成真实握手。当前聊天已加载 `mcp__gemini_subagent__gemini_delegate`，并真实完成代码阅读委派：3 轮、3 次读取工具调用，约 27 秒。

插件验证：

- 委派技能通过 `skill-creator/scripts/quick_validate.py` 格式校验。
- 自动测试已覆盖插件模式缺少工作区、相对工作区路径拒绝、两个不同项目的上下文隔离，以及固定模式不允许覆盖工作区。
- 用户目录插件副本通过自身 `.mcp.json` 启动与握手；主 agent 必须显式指定工作区，默认只读。
- `scripts/check-plugin.mjs --live`：插件副本真实委派临时文件读取通过，2 轮、1 次工具调用，返回真实随机标记。
- 使用桌面自带 `codex.exe plugin add gemini-subagent@personal-tools --json` 安装成功；`plugin list --marketplace personal-tools --json` 确认 installed 与 enabled 均为 true，实际缓存位于 `.codex/plugins/cache/personal-tools/gemini-subagent/1.0.0`。
- 安装缓存副本真实启动和 Gemini 文件读取测试通过，2 轮、1 次工具调用。
- 当前聊天已加载插件工具 `mcp__gemini_delegate__gemini_delegate`。通过该入口真实读取项目配置，正确返回默认模型与轮数上限，2 轮、1 次读取，约 8.5 秒。
- 先前独立注册的 MCP 入口仅禁用，保留配置可恢复；插件入口为默认使用路径。

PowerShell 5.1 启动器显式加载自身 Security 模块，避免从 PowerShell 7 继承模块路径导致 DPAPI 解密失败；Node 路径可以明确指定，或使用 Codex 提供的运行时。
