# 本机 API 实现与验证

日期：2026-10-19。用户确定的范围：同一电脑的第三方应用调用全部现有功能，采用成熟 API 框架。工作区既有修改均保留，没有暂存、提交或推送。

## 实现

使用 Axum 0.8.9 和 Utoipa 6.0.0，直接接入 Tauri 宿主和现有 Coordinator，不增加 Python 服务或第二个数据库写入者。第三方仍可使用 Python、PowerShell、C#、Node 等 HTTP 客户端。

提供 42 个类型化命令，覆盖扫描/检查、任务控制/重试/恢复、密码库/导入/加密备份、设置、文件工具及桌面辅助。主入口为 `POST /api/v1/commands`，保留按名称调用的别名；OpenAPI 类型、实际命令枚举和能力目录的 42 个名称经自动检查一致。服务还有健康检查、能力目录及 SSE revision 通知。

设置页提供启停、端口、应用权限/批准位置、创建一次性显示的密钥、撤销和文档入口。首次默认关闭，仅绑定 127.0.0.1。HTTP 不能创建密钥或扩大自身授权。文档和 [Python](../examples/local-api.py)/[PowerShell](../examples/local-api.ps1) 示例见 [使用说明](local-api.md)。

Swagger UI 5.32.6 使用官方静态资源随程序嵌入。初版集成包的 ZIP 构建依赖出现 3 条安全公告，已移除整个集成包及该 ZIP 依赖，保留 Axum/Utoipa。静态资产保存原始 Apache LICENSE/NOTICE、ZIP 来源与逐文件 SHA-256；收集器核对实际资产并保存完整法律材料。最终 Cargo.lock 只比 API 前新增 7 个外部包及 1 个本项目 crate，没有升级既有包版本。

## 执行与认证边界

每应用独立随机密钥，配置只保存摘要；通过 Authorization Bearer 传递，拒绝 URL/cookie 认证、异常 Host/Origin 和重复 Authorization。限制 20 MiB 正文、16 个普通请求及 32 个 SSE 连接，暂停/取消保留独立执行通道。服务停止会关闭旧事件流，并等待旧监听端口释放。

权限分为任务、文件、密码、设置和本机交互。源删除/回收、明文/备份导出分别需要明确权限。目录在受信任的本机选择框批准，每次调用按当前应用重新校验；不会因另一个应用或 GUI 的全局文件授权而放行。

Core 增加可注入的任务执行策略：准入、当前设置重试、开始和恢复都在实际 mutation gate 中授权，运行时重扫实际兄弟卷后、工作区写入前再检查。API 开始冻结本轮任务 ID，未授权卷/撤销导致尚未开始的任务保留 queued。批量添加先对全部候选授权，避免后项拒绝时遗留前项；这不承诺数据库 I/O 故障的整批事务。inspect 和 scan 同样检查实际发现的卷。

低权限任务读取递归省略 options/passwordGroup/passwordLabel，顶层密码列表、设置与数据目录分别裁剪。显式检查取消 ID 加应用前缀；共享 inspect/输出预览仍保留 latest-wins 语义。SSE 只包含 sessionId/revision；宿主在返回 ready 流前注册 Core 订阅，避免延迟启动桥接任务漏掉首个并发变化。

## 验证证据

运行根目录：`_workspace/api-20261019/run-20261019-local-api-01/`。对应修复基线单独见 [此前问题修复报告](system-fixes-report-2026-10-19.md)，不能将其旧候选通过结论迁移至 API 候选。

| 检查 | 实际结果/范围 |
| --- | --- |
| 新 API HTTP/配置/并发 | 19/19；真实 TCP，含认证、来源、目录 scope 前置门禁、正文限制、42 项目录/schema 一致、官方本地资源、SSE 撤销/停用/重启、并发上限和 Windows 原子替换失败。业务 backend 为 mock，真实目录执行另看下列 native。 |
| Core library | 174 通过、1 ignored，22 个 Vault 测试本轮过滤；其中新增 8 项授权实际通过，覆盖双来源拒绝原子性、当前设置、实际卷、API/GUI 调度集合与 inspect。 |
| Vault 基线 | 两份 Vault 字节仍匹配前一修复冻结值，沿用 R 的 Core 188/Infrastructure 54 完整 library 证据；未把这一点说成当前 API 全库重新跑过。 |
| Host library | 14/14，含 6 项授权/脱敏/撤销/请求 ID 测试。 |
| 协议 | Rust golden 6 + revision wire 1 通过；结构检查 25 desktop DTO / 8 Worker DTO / 268 fields，5 组镜像通过。没有修改 Worker JSON 协议字段。 |
| UI | 新增目标测试 10/10，默认全套 158/158；全量 lint、TypeScript 和 Vite build 通过。属于 jsdom/桥接 mock，原生选择框另列。 |
| 静态门禁 | 全 workspace/all-targets 严格 Clippy 及 format 通过。 |
| 实际任务生命周期 | 新 Core + .NET 10.0.13 Worker 串行复测：release reliability 3/3、resource recovery 5/5。 |
| 第一 API 候选 | Host SHA `c4942d127f48fac84e943c9f67cf583f005d333c9048d8798090a52fc10da8c0`，`native-20261019-120203/results.json` 实际 HTTP 全链 33/33，无 harnessError；包含两个示例真实调用、解压和输出哈希、暂停、取消重试、密码 CRUD/明文、导入、加密备份恢复及工具重命名。 |
| 最终 SSE 候选 | `native-20261019-121510/results.json` 35/35、0 failed、harnessError=null；新增真实 Coordinator→SSE 通知与 UTF-8 文件导入，全部业务链在新 Host 上重新运行。Host SHA `676bc9ba52ce504cab8f7f233a4cb4fe8fae4d13ad6993f41f443d05f9b1efcd`。 |
| 安全审计 | `results/security-audit-after-static/security-audit.json` overall passed：实际 runtime/在线服务基线、4 个 NuGet 项目、Roslyn、Windows Cargo 图和 npm；0 已知漏洞，保留 zbus_names/zvariant/zvariant_derive 共 3 项 unmaintained 提示。 |
| 法律材料 | 609 components / 1203 verified files / 55,883,702 bytes。 |

首次 native lifecycle 与 release 编译并行时，128 MiB 取消后重试案例超过其 90 秒流水线等待期限；日志保留在 `native-lifecycle-01.log`。停止并行编译后的同源码串行复测通过，3 项总耗时 117.06 秒、另 5 项 27.53 秒。此报告不删去初次失败，也不把通过解释为性能承诺。第一个 HTTP 探针因未提交工具的 approvedPaths 被既有安全规则拒绝，修正调用夹具后 33/33；没有放宽产品规则。

`source-manifest-final.json` 是第一 API 构建的 375 项输入冻结；后续 SSE 和源码打包补齐的输入在 `source-manifest-release-02.json`。`03_api-security-review.md` 和 `04_api-final-review.md` 为只读独立审查，包含 P2 的发现、修复复读及执行证据。安全目录中旧 `release-audit.json` 不属于本轮验证，不能作为新安装或签名证据。

最终程序在 `candidate-final/OpenUnpack/OpenUnpack.exe`，保留其完整 worker/release-materials。第一次候选与全部失败/修正探针仍独立保存。另检查当前前端 40 项实际业务调用、Host 41 项业务命令均被 HTTP 的 42 项覆盖，缺失为 0；额外的 read_settings 用于独立设置读取。源码包显式包含 Swagger JS/CSS/HTML、原始许可证、两个调用示例和 UI lint 配置，避免源码缺失嵌入资源导致不能构建。

## 保留范围

这是本机开发候选的自动及实际 HTTP 验证，不是签名正式发行。原生文件选择/多选/拖放、剪贴板、干净 Windows 安装/升级/卸载、签名、长期负载及此前 S06 的完整真实竞态/第二磁盘验收仍需各自证据。本轮 ignored 的跨卷测试没有第二个指定可写卷，不能记作通过。

API 共用桌面任务和状态，不是多租户系统：jobs.read 可见共享任务的路径/失败消息；jobs.write 可暂停或取消共享队列。GUI 显式开始是新的可信本机操作，会恢复普通调度。撤销会拒绝新调用、订阅和尚未开始的受限任务，不自动取消已经接受并运行的作业。

部分恢复情况仍保守拒绝：普通 verify/retry 要求源可规范化，源已被外部移除时可能需在 GUI 处理；cleanup 专门保留安全清理入口。目录授权不是针对同用户任意进程或管理员的操作系统沙箱，不能用本轮 HTTP 门禁声称 S06 已全部闭合。
