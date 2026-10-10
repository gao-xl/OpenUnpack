# Gemini 基础任务子 agent

把文件查找、代码阅读、字面搜索、摘要和简单文件修改交给 Gemini，主 agent 负责明确任务与复核结果。

## Codex 插件

本项目已包含 Codex 插件清单 `.codex-plugin/plugin.json`、MCP 配置 `.mcp.json`、插件图标和 `delegate-basics` 委派技能。它是本地 Windows 插件，使用现有 Windows 加密凭据，模型为 `gemini-3.8-flash-high`。

Windows 一键安装或更新（需要 Node.js 22+ 和桌面自带的 `codex.exe`）：

```powershell
.\setup.ps1
# 连同真实网关任务一起验证：
.\setup.ps1 -VerifyLive
```

脚本准备个人插件源、调用桌面 Codex 安装并验证实际缓存中的 MCP 服务。已有同名插件可以直接更新；重复执行不会添加重复市场条目，未变化的插件源不产生新备份。缺少依赖时执行 `npm ci --ignore-scripts`。首次使用另一台机器时先运行 `set-credential.ps1` 设置该机器的加密凭据。

Windows 运行中的版本目录可能被占用。安装器检测到相同版本已启用时不会重新覆盖缓存，而是逐文件校验并验证启动；发布更新需要增加版本号。当前修复版为 **1.0.2**，插件清单、npm 包和 MCP 运行时版本一致。

准备脚本把运行时代码、完整测试和检查脚本复制到用户目录 `.codex\plugins\gemini-subagent`，并把它加入 `.agents\plugins\marketplace.json`。升级先复制到临时目录，准备成功后替换，旧源码备份保存在 `.codex\plugins\.gemini-subagent-backup-*`。保留已有模型、网关地址和任务限制；保留市场中的其他插件。配置冲突或复制失败会停止，旧插件不会被半成品替换。插件包不包含凭据，更新不触碰 Windows 加密密钥。

个人源准备好后，当前桌面自带的 Codex 支持直接安装：

```powershell
& (Get-Command codex.exe).Source plugin add gemini-subagent@personal-tools --json
& (Get-Command codex.exe).Source plugin list --marketplace personal-tools --json
```

本机已执行安装并核实 `installed: true`、`enabled: true`。重连插件或在新聊天中说“把当前项目中的基础阅读和搜索任务交给 Gemini”。也可以重启 Codex，在插件目录中选择个人插件源 **Personal Tools**，安装 **Gemini Subagent**。本地插件发现与安装流程参考 [官方插件文档](https://developers.openai.com/plugins/build/plugins)。该版本未提交到公开插件市场。

本机 PATH 中的 npm `codex.ps1` 是较旧版本，缺少插件安装子命令；上述命令显式使用桌面自带的 `codex.exe`。

插件模式每次委派必须传 `workspace` 绝对路径，供主 agent 选择当前授权项目。Gemini 工具仍仅接受相对此工作区的文件路径。跨项目使用无需修改全局固定工作区。

插件已安装启用，并在当前聊天通过插件入口完成真实委派。早先单独注册的 `gemini_subagent` 已设置为禁用，避免两个入口；需要时可在 Codex MCP 设置中重新开启。

已配置 OpenAI 格式网关 `https://gateway.yarrn.eu.org/v1`，模型固定为用户指定的 `gemini-3.8-flash-high`。模型可在 `config.json` 修改，或通过 `GEMINI_MODEL` 环境变量覆盖。使用 `/chat/completions` 与 `/models`；本版本不实现 Gemini 原生 `generateContent`、流式输出和多模态输入。

## Windows 启动

需要 Node.js 22 或更高版本、PowerShell，以及网关密钥。安装依赖：

```powershell
cd 'C:\Users\im\Documents\ChatGPT\批量解压缩\gemini-subagent'
npm ci --ignore-scripts
.\set-credential.ps1
.\run.ps1 --task '查找并总结项目的核心入口，引用文件和行号' --workspace '..' --json
```

`set-credential.ps1` 隐藏输入并通过 Windows DPAPI 保存密钥到用户目录 `.codex\gemini-subagent\gateway-key.dpapi`。此文件只可由同一 Windows 用户在同一机器解密。当前机器的网关凭据已经配置；重新设置时才需要运行该脚本。代码、配置和 MCP 注册项不包含明文密钥。启动器临时把密钥传给子进程，结束后恢复原有环境变量。

只读为默认值。修改文件时主动带 `--allow-write`：

```powershell
.\run.ps1 --task '把 README.md 中的指定错别字改正，并复核' --workspace '..' --allow-write --json
.\run.ps1 --models
```

macOS/Linux 可设置 `GEMINI_API_KEY` 后使用 `node src/cli.mjs`；DPAPI 启动器仅适用于 Windows。`GEMINI_BASE_URL` 可覆盖网关地址，远程必须使用 HTTPS。

## 主 agent / Codex 接入

提供标准 stdio MCP 服务，工具名称为 `gemini_delegate`。注册当前项目：

```powershell
.\install-codex.ps1
```

脚本先备份现有用户 Codex 配置，再追加标准 MCP 配置段 `gemini_subagent`，不会覆盖同名服务或修改其他设置。工具超时设为 300 秒，启动超时设为 20 秒。工作区固定为当前批量解压缩项目，可用 `install-codex.ps1 -Workspace '其他目录'` 为首次注册指定另一根目录。已注册后的路径可在 Codex MCP 设置中调整。

重新连接 MCP 或打开新聊天后，主 agent 可以调用：

```json
{
  "task": "查找批量解压缩的核心实现，解释执行流程。必须引用文件和行号，只读。",
  "context": "仅处理文件阅读和摘要；无需修改，结果控制在 300 字以内。",
  "subdirectory": "src",
  "allow_write": false
}
```

只传清晰目标、必要背景、相关项目约定和验收条件。子 agent 每次使用独立上下文，无法自动看到主聊天。主 agent 获取返回后检查 `status`、`answer`、`events` 与文件证据。工具描述会建议委派基础任务；是否调用由主 agent 决定，不强制自动分配所有任务。正在进行的聊天是否支持立即重载工具取决于客户端。

返回 JSON 包含状态、答案、模型、工作区、轮数、工具次数、耗时、网关报告的 token 用量和工具执行摘要。`completed` 表示模型给出最终回答，实际任务质量仍需主 agent 复核。`limit`、`incomplete`、`blocked`、`cancelled` 和 MCP `isError` 表明未完成；CLI 非成功结果返回退出码 2，启动/API 错误返回 1。token 用量为多个 API 请求累加，不代表费用。

默认注册只读服务。若另行把 MCP 启动参数加入 `--allow-write`，还需要每次委派传 `allow_write: true` 才能写入。写入允许新建普通文件和更新已有 UTF-8 文件；父目录必须已经存在。现有文件需要与读取时的 SHA256 一致，覆盖前备份到任务工作区的 `.gemini-subagent-backups`。备份返回路径可用于人工恢复。版本校验可发现一般并发修改，不能替代独占工作区或文件锁。

## 能力与边界

- 工具：`list_files`、`read_file`、`search_text`；可写任务另有 `write_file`。
- 访问仅限配置工作区或指定子目录，拒绝绝对路径、父目录逃逸、符号链接与 Windows junction。
- 跳过 `.git`、依赖、构建产物、常见凭据文件；文件资料中的指令不扩大任务范围。
- 最多 8 轮、24 次工具调用，每次请求最多 90 秒，任务最多 240 秒；429 和指定 5xx 最多重试两次。
- 读取和写入文件上限 256 KiB；读取最多 400 行；字面搜索最多扫描 1000 个文件并报告截断。
- 保留完整 assistant 消息和网关扩展字段，以保留 Gemini 的思考签名；不返回模型内部思考文本。
- 没有 shell、浏览器、部署、发消息或删除工具。它可以编写基础修改，无法自行运行项目测试。
- 路径检查是应用级权限限制，不是 OS 沙箱；不要在存在恶意进程同步替换文件的目录中使用可写模式。
- 任务背景及工具读取内容会发送给所配置网关。常见凭据文件会被屏蔽，但普通代码内的密钥仍需主 agent 避免委派读取。

## 验证

```powershell
npm test
.\run.ps1 -Mode live
```

自动测试覆盖工具循环、签名保留、错误回传、轮数/次数上限、取消、路径权限、文件备份、重试和真实 MCP 子进程握手。live 测试连接配置网关并通过 MCP 委派临时文件读取与修改，断言实际内容和备份；不会修改当前项目业务文件。

实现依据：[Gemini OpenAI 兼容接口](https://ai.google.dev/gemini-api/docs/openai)、[Gemini 思考签名](https://ai.google.dev/gemini-api/docs/generate-content/thought-signatures)、[MCP TypeScript SDK](https://ts.sdk.modelcontextprotocol.io/server)、[Codex MCP 配置](https://learn.chatgpt.com/docs/extend/mcp?surface=cli)。
