# OpenUnpack 本机 API

服务使用 Axum，Utoipa 生成 OpenAPI，Swagger UI 资源随程序打包。它与桌面界面共用同一个 Coordinator、队列、密码库及 Worker。适用于同一台电脑的脚本和第三方应用；OpenUnpack 必须保持运行。

## 启用和授权

在“设置 → 本机 API”启用服务，默认端口 47980。为每个应用创建独立密钥，选择权限，并通过本机目录选择器批准读取/写入目录。密钥仅显示一次；保存到调用方自己的安全存储。OpenUnpack 持久化密钥摘要，不能再次显示原密钥。撤销后新请求被拒绝，事件订阅也会关闭；已接受的解压任务仍由桌面队列管理，可暂停或取消。

仅监听 `127.0.0.1`，示例地址 `http://127.0.0.1:47980`。也可使用同端口的 localhost；服务严格校验 Host 和 Origin。服务默认关闭，不支持通过 HTTP 创建密钥或扩大自身授权。

启用后打开 `http://127.0.0.1:47980/docs`。Swagger 的 Authorize 按钮填写应用密钥，提供各命令完整参数模型；`/openapi.json` 可供工具生成客户端。控制台页面、健康检查和文档静态资源可公开读取；业务请求均须带 `Authorization: Bearer <密钥>`，不能放在 URL 或 cookie 中。

## 调用约定

主入口 `POST /api/v1/commands`，正文为 `{"command":"命令名","args":{...}}`。无参数命令省略 args，例如 `{"command":"queue_snapshot"}`。参数字段使用 camelCase。

也可调用 `POST /api/v1/commands/命令名`，正文直接写参数对象；无参数命令发送 `{}`。两种方式经过相同权限和参数校验。请求使用 `Content-Type: application/json`，正文上限 20 MiB。

成功返回 `{"requestId":"...","data":...}`；失败返回 `{"requestId":"...","error":{"code":"...","message":"..."}}`。常见状态：400 参数/业务拒绝，401 无有效密钥，403 权限或目录未批准，404 路径不存在，405 方法错误，413 正文过大，429 并发已满，500 内部异常。调用方可记录 requestId，避免记录包含密码或密钥的请求正文。

`GET /api/v1/capabilities` 返回命令目录、权限目录和资源上限。`GET /api/v1/events` 使用同一 Authorization 头订阅 SSE；先收到 ready，随后 state-changed 带 sessionId/revision。事件只通知状态变化，客户端据此重新获取获准读取的数据。重新连接或 resync 时获取完整当前状态。浏览器原生 EventSource 不能设置 Authorization，需使用支持流式读取的 fetch 或 HTTP 客户端。


浏览器控制台在服务根地址打开。密钥仅在当前浏览器会话中保存，并绑定服务地址。普通前端开发预览默认离线；桌面界面连接守护进程仅接受本机 HTTP 根地址，切换前先验证健康和能力。连接后的队列按钮仅提交显式选中的任务，本机文件选择及本机后处理操作不会被转发为远端文件操作。

浏览器 WebSocket 先用 Authorization 头调用 `POST /api/v1/ws-ticket`，再以 `GET /api/v1/ws?ticket=...` 连接；票据有效期 15 秒且只能使用一次。支持设置请求头的客户端也可直接用 Bearer 头建立 WebSocket；不接受 URL 中的 token。SSE/WS 使用相同权限检查，撤销或到期会关闭现有订阅，进度及输入事件会按文件读取授权隐藏路径。

## 业务命令

| 功能 | 命令 |
| --- | --- |
| 状态/历史 | snapshot、queue_snapshot、read_settings、history、history_page、job_details |
| 扫描/排除 | scan_sources、cancel_scan、preview_exclusions、cancel_preview_exclusions |
| 队列 | add_archive、submit_input、add_jobs、start_jobs、start_selected_jobs、pause_jobs、cancel_jobs、retry_job、retry_node、remove_job、clear_completed、clear_history、recover_job |
| 密码库 | passwords、save_password、delete_password、reveal_password、copy_password、bulk_password_action |
| 导入 | read_import_file、preview_import、confirm_import |
| 加密备份 | export_backup、preview_restore_backup、confirm_restore_backup |
| 设置 | read_settings、save_settings |
| 归档检查/选择提取 | inspect_archive、cancel_inspect_archive、extract_selective |
| 文件工具 | preview_toolbox_operation、execute_toolbox_operation |
| 桌面辅助 | open_path、pick_files、pick_directory、pick_save_backup、pick_open_backup |

每个具体模型以当前程序 `/docs` 为准。确认导入、恢复或文件工具操作须使用对应预览返回的 token，内容或状态变化后重新预览。文件工具执行还须将选中的预览 plans.sourcePath 放入 approvedPaths，不能为空；失败后令牌可能已消耗，重新预览后再试。API 不会跳过既有发布保护、源清理策略或预览令牌。

### 提取所选归档条目

`extract_selective` 的 args 为 `{"request":{"archivePath":"绝对源路径","entryPaths":["folder/file.txt"],"destinationDir":"绝对输出目录","flatten":false}}`，需要 files.read、files.write 以及实际源分卷和输出目录授权。省略 destinationDir 时使用源归档所在目录，该默认目录同样必须有写入授权。entryPaths 为 1 至 10000 个区分大小写的归档根相对字面路径；选择目录包含其后代，不能用通配符或父级路径扩大范围。

提取经过私有工作区和完整读回校验，再发布到输出根下新建的唯一子目录。返回 outputDirectory、extractedFiles 和 totalBytes；flatten 为 true 时展平文件，重名冲突会拒绝。损坏归档不会通过该命令发布为完整结果。可在 request 中提供仅本次使用的 password；缺密码或密码错误时保留原选择并由调用方重试，命令不会自动保存密码、删除源包或启动共享队列。执行中撤销授权会在发布前的复核中拒绝提交。

## 权限和队列语义

任务、文件、密码、设置及桌面交互分别授权。`snapshot` 包含所有状态，需要 jobs.read、vault.read、settings.read；低权限任务读取可用 queue_snapshot。未授予 vault.read/settings.read 时，该接口保留 AppSnapshot 的字段结构，密码列表为空、密码库错误为 null、设置使用安全默认值、数据目录脱敏。任务/历史/详情中的 passwordGroup 置为空字符串，passwordLabel 为 null；文件路径还需 files.read 和对应目录授权，未授权的位置会脱敏。

密码名称/分组与密码明文分开授权；reveal_password、copy_password 和可由调用方解密的 export_backup 需要 vault.reveal。源包删除/回收及删除空目录另需 files.delete。文件访问还必须落在本应用批准的位置，不能因另一个 API 应用或界面此前选过目录而获得授权。

API 的扫描/检查 requestId 由服务加上应用前缀，显式取消只作用于本应用的请求。归档检查及输出排除预览仍使用桌面的“最新请求替换旧请求”共享会话语义；新的检查可能替换界面或另一应用尚未结束的检查，调用方应处理取消结果。暂停还需要 settings.write；单任务取消会检查该任务的目录和执行权限，不传 jobId 时仅取消当前应用有权操作的任务。

start_jobs 启动共享队列的当前待运行任务，调用应用必须获准访问本轮全部待运行任务的源卷、输出及缓存位置；任一任务未批准则拒绝启动。API 启动冻结本轮任务 ID，之后界面新添任务不会自动被该轮 API 启动带跑；界面显式“开始”属于新的本机用户操作，恢复通常队列调度。重试使用当前设置并再次校验，执行前重扫实际卷；撤销后尚未开始的受限任务保留在队列等待重新批准。应用重启后 API 启动也须重新校验，内存策略不作为持久授权。已开始任务不会因撤销而自动取消。

### 指定本次任务集合

`start_selected_jobs` 参数为 `{"jobIds":["任务编号1","任务编号2"]}`。接受 1 至 1000 个不重复、非空编号，全部必须存在且处于 `queued` 或 `waitingResource`。先核对整个集合的状态、目录和权限，再改变调度范围；无关待处理任务不会因本次启动被领取。运行中的 API 调度可以加入另一组已批准的待运行任务。桌面调度正在运行、本机用户暂停或资源保护暂停时，API 限定启动会拒绝。本机暂停由本机恢复。

API 发起的 `pause_jobs` 还需 settings.write，并核对当前 API 批次；不能接管正在运行的本机批次。只有该来源的暂停可由具备 settings.write 的客户端显式调用 `start_selected_jobs` 恢复，恢复前重新核对所选及当前运行任务，后续调度范围收窄到所选任务和已运行任务。暂停时也可显式选择原 API 批次内的运行任务作为恢复锚点；普通添加、重试或 start_jobs 不会隐式恢复。没有待调度任务时暂停会返回说明，取消最后一个待调度任务也不会唤醒其他仍暂停的运行任务。

为避免新 API 任务在确认启动前被桌面调度器领取，API 添加/重试也会在桌面调度正运行时拒绝；此时稍后重试即可。API 添加的文件和选项只核对本次请求，不再因无关待处理任务的目录未授权而失败。既有 `start_jobs` 全队列行为仍保留。

`retry_job` 可增加 `preserveOptions: true`，例如 `{"jobId":"任务编号","preserveOptions":true}`，沿用该任务原输出目录、密码组、缓存和清理策略，并重新授权实际文件。缺省为 false，保持原先读取当前设置的行为。API 重试、内包重试仅启动所选任务。`cancel_jobs` 传具体 `jobId` 可以取消该任务，包括尚未开始的 `waitingResource`；不传仅作用于当前应用有权操作的任务。

按任务集合并不增加多租户可见性隔离。持有 jobs.read/jobs.write 的应用仍具备上述共享队列权限；客户端应只操作本次 `add_jobs` 返回的编号。MCP 连接器返回的 `jobIds` 可在重连后用于逐项查询；连接器内尚未提交的计划会过期或随进程退出丢失。已提交任务保存在桌面中，不因连接器断开而自动取消。使用方式见 [MCP 连接器](../apps/mcp-connector/README.md)。

文件选择、打开目录及复制密码需本机桌面交互权限；文件选择会弹出本机窗口，不能用于无人值守脚本。用户实际在选择框批准的文件/目录会加入该应用授权，HTTP 不能自行添加路径。仅有 HTTP 验证不代表这些原生窗口操作已完成验收。

`add_archive` 会解析本次有效选项、核对实际分卷，`autoStart` 仅启动本次接纳的任务；`deleteSource: false` 保留原包。随请求给出的密码和 `submit_input` 的密码仅供当前任务临时使用，持久保存需要单独授权并显式调用密码库写入命令。运行中的等待密码任务应提交 `submit_input`，无需重新排队。

## 调用示例

参考 [Python 示例](../examples/local-api.py) 和 [PowerShell 示例](../examples/local-api.ps1)。两者从当前进程的 `OPENUNPACK_API_TOKEN` 读取密钥，不在参数或日志中输出密钥；示例提交任务时保留源文件。将压缩包所在目录和输出目录预先加入应用授权。

服务以本机登录用户权限运行，目录授权是 API 的应用级限制；它不构成针对同用户任意进程或管理员的操作系统沙箱。不要把 API 转发到公网。
