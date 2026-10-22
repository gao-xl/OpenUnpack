# UI / API 修复与复验报告（2026-10-21）

本轮执行用户已授权的 UI / API 修复，沿用现有脏工作区；未 reset、暂存、提交或推送。此前审计快照见 [2026-10-20 审计](ui-api-usability-security-2026-10-20.md)。

## 结论

已关闭本轮确认的权限绕过、凭据与事件泄露、连接协议错误和密码交互问题。独立静态复核通过；最终完整前端 22 文件 / 199 项全部通过，API / CLI 47 项全部通过，真实 Windows Worker 解压的三个模式全部完成。

## 修复范围

| 问题 | 优先级 / 置信度 | 已实现行为 |
| --- | --- | --- |
| 快捷添加、输入重试绕过任务授权 / 带起无关队列 | P1 / 高 | AddArchive、SubmitInput 复用 ClientJobPolicy，冻结有效设置并核对实际卷，只启动目标任务；失败真实返回。 |
| 临时密码隐式写入密码库、false 清理映射错误 | P1 / 高 | 请求密码仅绑定成功接纳的任务；输入等待者先绑定 policy 再发送；deleteSource=false 对应 keep；持久保存需显式 vault.write 操作。 |
| 伴随密码文件扩大文件读取范围 | P1 / 高 | 每个候选单独授权，固定父目录及真实只读句柄，拒绝 reparse，实际读取限制 64 KiB。 |
| WS 撤销 / 到期后仍可提交命令 | P1 / 高 | HTTP、SSE、WS 共用 fresh client 判断；排队前及执行前再次授权，存量连接有到期 / 撤销检查。 |
| 进度 / 输入事件及快照泄露路径 | P1 / 高 | 按 FilesRead 和 canonical 目录逐字段投影；保留前端必需 JSON 结构，passwordGroup 空字符串、passwordLabel 为 null。 |
| WS URL 携带长期 Bearer | P2 / 高 | 浏览器用 15 秒、单次票据；拒绝 token query；支持请求头的客户端可用 Authorization。 |
| 通道关闭忙循环、请求与回复资源缺少边界 | P2 / 高 | 关闭的可选通道退出 select；WS 16 KiB、回复队列 / 发送期限；HTTP 单体 20 MiB、聚合正文预算及读取期限。 |
| 控制台被认证中间件遮挡、Origin / CSP 不匹配 | P1 / 高 | 控制台外壳公开、业务仍认证；严格 Host / Origin，原生 CSP 仅补本机回环连接。 |
| 前端信封 / 本机管理路由 / 连接密钥串用 | P1 / 高 | 解包 data / error.message，核验输出形状；本机管理保留 IPC；密钥绑定端点与会话，连接先健康 / 能力验证并防迟到提交。 |
| 离线预览误连、远端任务按钮禁用 | P1 / 高 | 服务 meta 标记启用同源控制台，普通预览离线；队列仅启动显式所选 queued / waitingResource，禁止远端任务触发本机打开 / 后处理。 |
| API 暂停后无安全恢复路径 | P1 / 高 | 记录暂停来源，仅 settings.write 的显式所选恢复可恢复 API 暂停；重验全部 active 与所选，范围为 selected ∪ active；本机 / 资源暂停仍不可接管。 |
| 运行中等待密码、详情旧流程、保存期间取消无效 | P1 / 高 | 队列和详情都使用 submit_input，默认不存库；仅确认按钮 loading，取消可退休草稿，迟到保存不能继续提交或跨端点发送。 |
| CLI status 请求不存在路由 / 空闲误报离线 | P2 / 高 | 使用认证 queue_snapshot、读取实际 port，显示 Online 及 Running / Idle；设置超时和响应大小上限。 |

## 运行证据

证据目录：`_workspace/ui-api-fixes-20261020/`。前端专项与全量存在重叠，以下不相加为独立测试总数。

| 验证 | 最终结果 | 日志 |
| --- | --- | --- |
| UI 完整套件 | 22 文件 / 199 项全部通过（324.53 秒） | ui-full-final-tests.log |
| UI 连接 / 队列 / 密码重点复验 | 24 / 24 | ui-password-queue-final.log |
| UI 信封 + 真实 Host 投影 JSON 契约 | 5 / 5 | ui-native-contract-tests.log |
| UI lint、TypeScript / Vite 构建 | 通过 | ui-lint.log、ui-build.log |
| Local API（HTTP / SSE / WS / Store / 限额） | 44 / 44 | api-cli-final-tests.log |
| CLI | 3 / 3 | api-cli-final-tests.log |
| Host 单元 / 授权集 | 25 通过，1 原生显式测试默认忽略 | host-final-tests.log |
| Core lib | 154 通过，1 第二卷场景原有忽略 | core-final-tests.log |
| 伴随密码授权真实 Windows 文件系统测试 | 5 / 5 | core-final-tests.log |
| MCP 连接器 | 20 / 20 | mcp-tests.log |
| DTO 合同 | 30 Desktop / 8 Worker DTO，303 字段 | contracts.log |
| 架构约束 | 通过，14 项负向控制通过 | architecture.log |
| 本轮 Rust 格式 | 通过 | rustfmt.log |
| Worker 当前源码构建 | 0 警告、0 错误 | native-worker-build.log |

### 真实 Windows 解压链路

显式运行 `local_api::native_tests::native_http_extraction_keeps_passwords_temporary_and_gui_jobs_queued`：1 / 1 通过（128.29 秒，含浏览器联调等待 120 秒）。它使用真实 TCP HTTP、DesktopBackend、Coordinator、认证命名管道、重新构建的 .NET Worker 和官方 7-Zip，验证三个 ZIP：普通归档、请求携带临时密码、运行中等待后 submit_input。

三个任务均完成并逐字节核对输出；原包保留，密码库为空，未授权 GUI sentinel 始终 queued / attempts=0，未扩大 NativeAccess。本测试使用隔离私有临时目录及合成密码，不读取用户归档。

真实浏览器联调：从公开控制台输入该隔离服务临时令牌，验证健康 / 能力后重载，显示 3 个已完成任务、1 个待处理 sentinel；详情能展示实际结果。选择未授权 sentinel 并点击开始所选，显示“此应用没有执行该操作或访问该目录的权限”，任务未启动。截图和 DOM：browser-console.jpg、browser-denied.jpg、browser-denied-dom.txt。测试监听器已关闭。

![真实控制台联调](../../_workspace/ui-api-fixes-20261020/browser-console.jpg)

### 测试启动问题与修正

Host unit exe 首次在加载阶段以 STATUS_ENTRYPOINT_NOT_FOUND 退出。dumpbin 确认其导入 TaskDialogIndirect，而测试 exe 未携带 Common Controls v6 激活清单。验证启动器将原测试 exe 复制到独立位置，使用 SDK mt.exe 嵌入清单后执行；原 Cargo 产物和产品构建配置未修改。`host-test-runner.ps1`、`host-tests.manifest` 可复现此过程。

前端首轮 194 / 199：一个测试在编辑期间读到旧 banner；四个详情用例因 AntD 测试模式固定 test-id 引起对话框名称碰撞。按实际密码输入框所属对话框定位后，进一步检出了 confirmLoading 拦取消的真实问题，已修并通过全部重点复验，保留提交 / 保存顺序和取消断言。原生首轮测试请求遗漏 args 包装，已按真实枚举修正；没有放宽接口校验。

## 冻结与复现

`rust-source-final.json`（58 文件）、`ui-source-final.json`（55 文件）及对应 after 清单记录本轮相关源码；native-worker-manifest.json 记录 26 个 Worker 源码 / 载荷 / 引擎项。最终完整 UI 结束后已再次核验，全部无漂移；见 final-drift-check.json。

Rust 使用 scripts/run-cargo.sh、单构建作业、target_architecture_transition、隔离 TEMP/TMP、--locked --offline。Host 单元增加 CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUNNER 指向证据目录 host-test-runner.ps1。Core 命令：cargo test -p openunpack-core --lib --test password_companion_authorization；API / CLI：cargo test -p openunpack-local-api -p openunpack-cli。

原生测试需显式 OPENUNPACK_TEST_WORKER、OPENUNPACK_TEST_ENGINE；OPENUNPACK_TEST_SNAPSHOT 可输出投影 JSON 给前端 audit 契约测试。所有密码均为临时合成夹具，报告不保留令牌。

## 安全模型与验收边界

本轮主要防御无授权 / 低权限本机 HTTP 客户端、恶意网页跨站调用，以及已撤销 / 过期客户端继续使用连接。服务保持 IPv4 回环监听；API 应用授权不能从其他应用或 GUI 曾批准的位置继承。它仍以当前 Windows 用户运行，不是同用户任意进程的操作系统沙箱。

本轮结果覆盖上述修复及受控实机后端 / 浏览器链路。完整原生窗口、拖放 / 托盘 / 安装升级、第二物理卷、远端 HTTPS 反代部署和发布包验收没有因此完成。Core 忽略项要求显式提供第二个可写卷；本次不把它计为通过。Worker 为当前源码的框架依赖构建，不能据此宣称自包含安装包运行时已验收。

已更新 docs/local-api.md 中的连接、票据、脱敏字段、临时密码和暂停恢复说明。独立复核：证据目录 03_qa.md。
