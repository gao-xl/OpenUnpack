# OpenUnpack 本地 MCP 连接器

把 MCP 客户端中的批量解压请求交给正在运行的 OpenUnpack 桌面应用。先扫描并展示计划，用户确认后添加任务，只启动本批次；进度、异常处理和结果仍由桌面队列管理。

连接器只提供工具，不包含自然语言模型。需要一个支持本地 stdio MCP 的客户端，由该客户端的模型理解自然语言、展示计划并取得用户确认。没有联网模型调用或遥测代码；上层客户端是否把文件名、路径和状态发送给其模型，取决于客户端自己的设置。

## 准备

- Windows 上运行包含 `start_selected_jobs` 与 `retry_job.preserveOptions` 的新版 OpenUnpack。
- 安装 Node.js 24 或更新版本，并准备此源码目录。
- 在 OpenUnpack 的“设置 → 本机 API”开启服务，默认 `http://127.0.0.1:47980`。
- 为 MCP 创建独立应用密钥，授予 `settings.read`、`jobs.read`、`jobs.write`、`files.read`、`files.write`。
- 在该应用的目录授权中批准输入目录、输出目录，以及启用缓存时的缓存目录。仅使用密码分组名称不需要密码明文读取权限，不必授予 `vault.reveal`、备份、删除文件或桌面交互权限。

本机 API 必须保持运行。密钥仅显示一次，应存放在 MCP 客户端自己的安全配置中，限制配置文件访问权限，不要提交到 Git、聊天记录或日志。

在本目录执行：

```powershell
npm ci --ignore-scripts
npm run build
```

本包使用独立 `package-lock.json`，不会修改桌面前端依赖。运行入口为构建后的 `dist/index.js`。检查命令：

```powershell
npm run typecheck
npm test
```

## 配置 MCP 客户端

下面是通用 `mcpServers` 配置；替换为实际 Node 程序、连接器绝对路径和独立应用密钥。客户端的配置入口可能不同。

```json
{
  "mcpServers": {
    "openunpack": {
      "command": "node",
      "args": ["C:\\Tools\\OpenUnpack\\apps\\mcp-connector\\dist\\index.js"],
      "env": {
        "OPENUNPACK_API_TOKEN": "<独立应用密钥>",
        "OPENUNPACK_API_URL": "http://127.0.0.1:47980"
      }
    }
  }
}
```

建议 MCP 客户端直接启动 Node 入口。`npm start` 适合手动诊断，但 npm 本身会输出启动文字，不能作为 stdio JSON-RPC 的中间包装。

可配置环境变量：

| 变量 | 含义 |
| --- | --- |
| `OPENUNPACK_API_TOKEN` | 必填，OpenUnpack 的独立应用密钥，只用于 HTTP Authorization 请求头 |
| `OPENUNPACK_API_URL` | 可选，默认 `http://127.0.0.1:47980`；仅接受这个 IPv4 回环地址的 HTTP origin 与可选端口 |
| `OPENUNPACK_API_TIMEOUT_MS` | 可选，单次 API 请求超时，默认 `30000`，范围 `1..300000`；大目录扫描可适度增加 |

不支持 `localhost`、机器名、HTTPS、用户凭据、URL 路径、查询参数、片段或非回环服务。HTTP 重定向会被拒绝，不会把密钥转交重定向目标。stdout 专用于 MCP 协议，启动及传输错误只向 stderr 输出固定诊断，不记录密钥或远端错误正文。

## 使用流程

可以向 MCP 客户端提出：

> 扫描 C:\\Downloads 的压缩包，用“素材”密码组解压到 D:\\素材，保留原包。先把计划给我看，确认后再开始。

工具与参数：

| 工具 | 参数 | 行为 |
| --- | --- | --- |
| `plan_extraction` | `paths`；可选 `outputRoot`、`passwordGroup`、`expandNested`、`verification`（`standard`/`strict`） | 扫描获准路径，读取桌面设置并冻结任务选项，返回 ready、缺卷 blocked、扫描提示及计划摘要，不添加任务 |
| `execute_extraction` | `planId` | 用户确认后添加 ready 归档，仅启动返回的 `jobIds`；保存这些 ID 以便后续查询 |
| `get_batch_status` | `jobIds` | 逐 ID 读取状态、结果路径、内包失败摘要及处理提示，不读取整个队列 |
| `start_batch` | `jobIds` | 只启动本次选择中仍为 queued/waitingResource 的任务；已运行或已结束任务会跳过 |
| `cancel_batch` | `jobIds` | 按任务 ID 分别取消活跃任务，不发全队列取消/暂停；之后查询状态确认已停止 |
| `retry_batch` | `jobIds` | 仅重试 failed/needsPassword/cancelled/interrupted，保留每个任务的冻结选项和密码组 |

`jobIds` 为 1 到 1000 个不重复的任务 ID。连接器不会提供通用命令代理、全队列启动/暂停、密码明文、备份或源包删除工具。需要密码时，在 OpenUnpack 桌面密码库中补充或启用相应分组的密码，再重试。

每个新建计划继承当前桌面设置，允许覆盖输出根目录、密码组、内包展开和校验等级，同时强制 `sourceCleanup="keep"`、`autoStartOnAdd=false`、`cleanEmptyDirs=false`。不修改桌面全局设置。计划会显示冲突策略、过滤规则、内包层数、缓存和可能影响输出的设置；其他资源限制继承桌面设置。输出位置是根目录意向，实际最终目录名、冲突处理结果以及源内容仍由执行时的桌面检查决定。缺分卷归档永远不会从计划提交，补齐后重新扫描。

连接器在工具说明中要求上层客户端先展示计划并取得用户确认；这不是一个独立的桌面确认对话框。任务写入已有目录时，仍会使用计划中展示的冲突策略。API 目录授权是应用级边界，不是针对同一 Windows 用户其他程序的系统沙箱。

## 恢复与异常

- 计划在内存中保存 10 分钟，最多 100 个。连接器重启后计划丢失；已执行计划在存活期内重复或并发执行共享同一次提交，不重复添加任务。
- `execute_extraction` 返回 `submitted` 表示添加并发出了选中启动请求，不表示解压已经成功。用 `get_batch_status` 跟踪；`partial`、`salvaged`、发布恢复或缓存清理待处理都会列为需要关注。
- `submittedStartUnconfirmed` 表示任务已添加，启动结果未确认；使用返回的 `jobIds` 查询，再用 `start_batch` 启动仍在等待的任务，不应重新添加。
- `submissionUnknown` 表示添加请求可能部分或全部成功，包括超时、断连、HTTP 400/5xx、无效响应。计划会消耗，重复执行也不再次添加。先在桌面核对已接收的任务，避免直接重建计划造成重复；添加不是整批数据库事务。
- `submissionRejected` 表示已知的前置拒绝，例如密钥或权限不足。该计划同样消耗，修正后创建新计划。
- `nothingAdded` 表示服务确认没有新增任务，通常是归档已有活跃任务。连接器不会启动这些已有任务，需要在桌面查看。
- 活跃桌面全队列运行或暂停状态可能拒绝 API 添加、重试或选中启动。检查桌面状态后继续；连接器不会自行暂停其他任务。
- **关闭 MCP 连接、重启连接器或撤销密钥不会自动取消已接受的解压任务。** 保留 `jobIds`，重新连接后可以继续查询/控制获准的任务；无需连接器数据库。计划 ID 不能代替任务 ID，尚未获知任务 ID 的不确定提交需在桌面核对。
- 取消后可能有部分输出或正在停止的任务，应继续查询并在桌面确认结果。需要发布恢复的任务先在桌面恢复界面处理。

## 验证范围

`npm test` 通过官方 SDK 的客户端启动实际 stdio 入口，使用本机 mock HTTP 服务验证初始化、工具清单、计划/提交/进度、批次隔离、并发幂等、启动失败恢复、权限失败、密钥脱敏、重定向拒绝、超时、部分提交、计划失效及输入校验。后端权限与实际 Worker 解压需要另外进行真实桌面主程序集成验证。
