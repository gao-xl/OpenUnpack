# UI 与本机 API 可用性、安全验证

> 修复后复验见 [2026-10-21 修复报告](ui-api-fixes-validation-2026-10-21.md)。本文件保留 2026-10-20 的审计快照。

日期：2026-10-20（Asia/Shanghai）。结论：**needs changes；当前不能判定 API 可用且安全**。

本轮保留全部已有修改，只增加隔离验证用例、日志、源码指纹和本报告，未修复应用源码、提交或推送。仓库 AGENTS 的 scout/qa-inspector 并行审查已完成；执行模型继承主代理，不将角色标签当作 Haiku/Sonnet 的实际执行证据。

## 验证边界与版本

入口包括 React/Tauri 命令、HTTP JSON、SSE、WebSocket（含 JSON-RPC）、CLI/MCP。重点资产是目录授权、源文件/输出、密码库、任务状态、撤销和到期语义，以及宿主账户的执行权限。威胁模型包含未认证请求、权限受限的已授权客户端、撤销或过期的长连接；不将监听回环地址等同于客户端之间没有权限边界。

首轮后源码继续更新，因此区分早期日志和最新复测。最终优先使用 source-final-before.json / source-final-after.json 与带 latest/current 的日志。旧 scout.md、qa.md 是早期源码审查记录，不能单独作为当前缺陷清单。

- [_workspace/ui-api-validation-20261020/source-before.json](C:/Users/im/Documents/ChatGPT/批量解压缩/_workspace/ui-api-validation-20261020/source-before.json)：首轮 72 个文件指纹。
- [_workspace/ui-api-validation-20261020/source-latest-before.json](C:/Users/im/Documents/ChatGPT/批量解压缩/_workspace/ui-api-validation-20261020/source-latest-before.json)、[_workspace/ui-api-validation-20261020/source-final-before.json](C:/Users/im/Documents/ChatGPT/批量解压缩/_workspace/ui-api-validation-20261020/source-final-before.json)：恢复工作后的候选指纹。
- [_workspace/ui-api-validation-20261020/source-latest](C:/Users/im/Documents/ChatGPT/批量解压缩/_workspace/ui-api-validation-20261020/source-latest)：关键文件源码副本。
- [_workspace/ui-api-validation-20261020/line-evidence.json](C:/Users/im/Documents/ChatGPT/批量解压缩/_workspace/ui-api-validation-20261020/line-evidence.json)：最终定位依据。

## 仍需修复的安全问题

### S1 — 便捷接口未绑定完整客户端执行策略

**High / 高置信度（99%），源码调用链确认，未以真实用户文件执行破坏性复现。**

[apps/desktop-host/src/local_api.rs:864](C:/Users/im/Documents/ChatGPT/批量解压缩/apps/desktop-host/src/local_api.rs:864) 的 add_archive 虽然现在检查入参源路径/显式 outputDir，但仍继承全局 output/cache/cleanup，最终在 [apps/desktop-host/src/local_api.rs:931](C:/Users/im/Documents/ChatGPT/批量解压缩/apps/desktop-host/src/local_api.rs:931) 调用可信桌面 core_add_jobs，并在 935 调用全局 core_start_jobs。execution_access 没有 AddArchive 分支，缺少既有 AddJobs 的 add_jobs_authorized / start_selected_authorized、实际分卷及有效设置的完整预检。deleteSource 未提供时也没有按继承后的实际 cleanup 核验 files.delete。结果可能使用客户端未批准的全局输出/缓存，并启动无关 GUI 队列。

SubmitInput 已增加 job policy 检查，但未命中等待中的输入时，[apps/desktop-host/src/local_api.rs:981](C:/Users/im/Documents/ChatGPT/批量解压缩/apps/desktop-host/src/local_api.rs:981) 仍回退可信桌面 retry；[apps/desktop-host/src/lib.rs:1206](C:/Users/im/Documents/ChatGPT/批量解压缩/apps/desktop-host/src/lib.rs:1206) 使用全局当前设置重试并 start()，不再保持 API 的指定任务/策略边界。两条便捷路径还在没有 vault.write 的要求下持久化所给密码（local_api.rs:921、979）。

修复方向：将便捷接口翻译成既有的完整授权请求，对有效设置与发现的分卷预检，绑定 ClientJobPolicy，仅启动本次指定 ID；密码默认仅本次使用，持久化单独要求 vault.write。不能仅补一个入参路径检查。

### S2 — 已撤销 WebSocket 仍能把新操作派发给后台

**Medium / 高置信度（99%），真实回环 WebSocket + 隔离记录后台复现。**

[crates/local-api/src/server.rs:1071](C:/Users/im/Documents/ChatGPT/批量解压缩/crates/local-api/src/server.rs:1071) 的接收循环持有握手时的 client；[crates/local-api/src/server.rs:897](C:/Users/im/Documents/ChatGPT/批量解压缩/crates/local-api/src/server.rs:897) 只检查其 scope/参数，不查询当前撤销状态。活跃检查在独立发送循环（1275），空闲连接可等到下一次 15 秒 ping 才发现撤销。

探针先接收 ready/ping，再撤销具有 jobs.read/jobs.write/settings.write 的合成客户端，然后提交 JSON-RPC pause；后台记录到 1 次操作，预期为 0。真实 Host PauseJobs 分支仅检查 settings.write，随后进入 core_pause_jobs，未另查 active_client。没有实际暂停用户任务。

证据：[_workspace/ui-api-validation-20261020/transport-current.log](C:/Users/im/Documents/ChatGPT/批量解压缩/_workspace/ui-api-validation-20261020/transport-current.log)、[_workspace/ui-api-validation-20261020/transport-probe/src/lib.rs](C:/Users/im/Documents/ChatGPT/批量解压缩/_workspace/ui-api-validation-20261020/transport-probe/src/lib.rs)。修复方向：每个消息在副作用前重新核验 enabled、撤销、到期；统一调度同时覆盖并发预算，撤销主动关闭连接。

### S3 — 到期检查未覆盖持续授权

**Medium / 高置信度（100%），隔离 store 复现。**

[crates/local-api/src/store.rs:339](C:/Users/im/Documents/ChatGPT/批量解压缩/crates/local-api/src/store.rs:339) 的 active_client 只检查 enabled 和 ID 存在，忽略 expiresAt。探针证实同一个到期 token 的新 authenticate 被拒绝，但 active_client 仍为 true；SSE/WS 和 ClientJobPolicy 依赖后者，因此到期未覆盖长连接和持续任务授权。

修复方向：共用同一时钟/到期判断，在握手、派发、事件投影、持续任务授权中保持一致。证据：[_workspace/ui-api-validation-20261020/transport-current.log](C:/Users/im/Documents/ChatGPT/批量解压缩/_workspace/ui-api-validation-20261020/transport-current.log)。

### S4 — 事件流泄露未批准目录的完整归档路径

**Medium / 高置信度（100%），真实回环 WebSocket + 合成事件复现。**

仅 jobs.read、没有任何目录 grant 的 token 能收到 NeedPasswordEvent.archivePath。[crates/local-api/src/server.rs:1337](C:/Users/im/Documents/ChatGPT/批量解压缩/crates/local-api/src/server.rs:1337) 将全局事件直接序列化；SSE 也直接转发同一事件。它绕过 HTTP job_value 的目录脱敏。探针只广播 C:\SyntheticOutsideGrant\fixture.7z 之类的合成路径，未读取用户任务。

修复方向：按客户端投影/脱敏所有事件，并统一 Snapshot（local_api.rs:453）与 QueueSnapshot 的路径策略。证据：[_workspace/ui-api-validation-20261020/transport-current.log](C:/Users/im/Documents/ChatGPT/批量解压缩/_workspace/ui-api-validation-20261020/transport-current.log)。

### S5 — 关闭的可选事件通道使流失去正常等待

**Medium / 高置信度（95%），源码确认且现有关闭测试失败；未据此声称真实桌面必然占满 CPU。**

ApiBackend 默认 progress/need_input 返回 sender 已关闭的 receiver（server.rs:43-49）。SSE/WS select 分支忽略 Closed，持续可就绪。SSE 又在循环中重建 sleep，可能无法正常等待/终止。现有 RecordingBackend 正好使用这些默认实现。

直接运行现有 disabling_api_closes_existing_event_stream_without_waiting_for_an_update，约 5 秒后以 Winsock 10060 超时失败；测试进程正常退出 101，没有触发 12 秒外部终止上限。该执行证据是关闭测试失败，具体 CPU 归因仍以源码分析为限。见 [_workspace/ui-api-validation-20261020/sse-close-latest.stdout.log](C:/Users/im/Documents/ChatGPT/批量解压缩/_workspace/ui-api-validation-20261020/sse-close-latest.stdout.log)、[_workspace/ui-api-validation-20261020/sse-close-latest.stderr.log](C:/Users/im/Documents/ChatGPT/批量解压缩/_workspace/ui-api-validation-20261020/sse-close-latest.stderr.log)。修复方向：关闭后禁用对应分支，使用持久 interval/cancellation。

其他资源边界待补：HTTP 16 请求信号量在 JSON 解析之后；WebSocket 未显式限制小协议的消息/帧大小，也未复用普通操作预算。未运行大包/流量耗尽测试，不把假设负载当作实测。

## 可用性阻断

| 编号 | 当前问题与影响 | 证据及修复方向 |
|---|---|---|
| F1 | daemon 成功结果未解包，错误层级也读错。调用方拿到 requestId/data/meta 而非 AppSnapshot/数组，错误只显示 HTTP 403。 | [apps/desktop-ui/src/api.ts:91](C:/Users/im/Documents/ChatGPT/批量解压缩/apps/desktop-ui/src/api.ts:91)；4 项独立前端契约用例全部失败。统一读取 data / error.message 并校验响应形状。 |
| F2 | 新 Web 控制台 GET / 返回 401，普通浏览器导航无法先提供 Bearer；assets 同样被要求认证。 | guard 的 public 在 server.rs:647 定义后，又在 680 被不含首页/资源的新变量遮蔽。真实 HTTP 探针得到 401。仅公开无敏感数据的登录外壳/静态资源，业务 API 保持鉴权。 |
| F3 | 桌面 daemon fetch/ws 不符合 CSP；浏览器跨来源 POST 虽通过 OPTIONS，仍被正式请求的 Origin 规则拒绝。仅回环监听也不直接支持宣称的远端服务器。 | [apps/desktop-host/tauri.conf.json:5](C:/Users/im/Documents/ChatGPT/批量解压缩/apps/desktop-host/tauri.conf.json:5)；server.rs:671。先确定受支持的同源控制台/宿主代理方案，再设置最小允许范围，不应删除鉴权或宽泛放开来源。 |
| F4 | 浏览器 daemon 模式的队列开始/暂停/重试等仍 disabled={!isDesktop}；连接上也不能操作。 | [apps/desktop-ui/src/components/QueuePage.tsx:292](C:/Users/im/Documents/ChatGPT/批量解压缩/apps/desktop-ui/src/components/QueuePage.tsx:292)；SettingsPage 等也有同类能力判断。将支持的 API 能力与原生文件选择能力分开。 |
| F5 | 本机 API 管理和 set_ui_language 经 call() 被发往 daemon，但 HTTP 命令枚举不支持它们。 | [apps/desktop-ui/src/api.ts:94](C:/Users/im/Documents/ChatGPT/批量解压缩/apps/desktop-ui/src/api.ts:94)；保留本机 IPC 管理路由，远端 UI 明确不支持的操作。 |
| F6 | deleteSource=false 被映射为非法 source_cleanup=none。 | [apps/desktop-host/src/local_api.rs:904](C:/Users/im/Documents/ChatGPT/批量解压缩/apps/desktop-host/src/local_api.rs:904)；Core integrity.rs:8 仅接受 keep/recycle/delete，应映射 keep。 |
| F7 | API RecoverJob 校验不接受 Core 的 verify 恢复动作；SubmitInput 忽略 retry 错误仍回 success，且字段使用 job_id/input_type/resumed_in_flight，和 TS camelCase 类型不一致。 | command.rs:445；coordinator/recovery.rs:248；local_api.rs:979-988。同步契约并保留失败结果。 |
| F8 | CLI status 调用没有注册的 GET /api/v1/status。 | [crates/cli/src/client.rs:85](C:/Users/im/Documents/ChatGPT/批量解压缩/crates/cli/src/client.rs:85) 与 server router。尚未运行完整 CLI→原生 Host 桌面验收。 |

F1 的最新探针中，“浏览器根本不 fetch”已修正；该用例仍因返回值未解包而失败，不再把旧行为列为当前问题。

## 已看到修复的早期问题

- settings.write 注入 on_complete：最新 Host 在 local_api.rs:769 保留可信值；早期报告中的这项缺陷不再列为当前未修复高危。此轮未执行真实 shell hook。
- jobs.read WebSocket 提交密码：最新代码改为 authorize_command + typed execute。真实连接只读用例已通过；不能据此推导撤销/过期也正确。
- 已给 HTTP SubmitInput 增加 job policy，AddArchive 增加输入路径检查；剩余问题是可信 retry/start 的回退路径和继承设置。
- UI token 改为 sessionStorage，非回环 HTTP 地址被拒绝；未给这些源码变化补做独立的 WebView 凭据生命周期验收。

## 执行结果

| 检查 | 结果 | 日志 |
|---|---|---|
| 最新前端 lint | PASS | [_workspace/ui-api-validation-20261020/ui-lint-latest.log](C:/Users/im/Documents/ChatGPT/批量解压缩/_workspace/ui-api-validation-20261020/ui-lint-latest.log) |
| 最新 TypeScript + Vite 生产构建 | PASS | [_workspace/ui-api-validation-20261020/ui-build-latest.log](C:/Users/im/Documents/ChatGPT/批量解压缩/_workspace/ui-api-validation-20261020/ui-build-latest.log) |
| 首轮前端完整测试 | 159 通过 / 11 失败，共 170；19 文件 | [_workspace/ui-api-validation-20261020/ui-tests-workspace-temp.log](C:/Users/im/Documents/ChatGPT/批量解压缩/_workspace/ui-api-validation-20261020/ui-tests-workspace-temp.log) |
| 最新前端完整重跑 | 156 通过 / 14 失败，共 170；4 文件失败，15 文件通过；561.75 秒 | [_workspace/ui-api-validation-20261020/ui-tests-latest.log](C:/Users/im/Documents/ChatGPT/批量解压缩/_workspace/ui-api-validation-20261020/ui-tests-latest.log) |
| 最新 daemon 契约探针 | 0 通过 / 4 失败 | [_workspace/ui-api-validation-20261020/daemon-contract-latest.log](C:/Users/im/Documents/ChatGPT/批量解压缩/_workspace/ui-api-validation-20261020/daemon-contract-latest.log) |
| 最新真实 HTTP/WS 安全探针 | 1 通过 / 4 失败，共 5 | [_workspace/ui-api-validation-20261020/transport-current.log](C:/Users/im/Documents/ChatGPT/批量解压缩/_workspace/ui-api-validation-20261020/transport-current.log) |
| 最新 HTTP 合同测试（不含两个 SSE 测试） | 10 通过 / 2 失败；2 filtered | [_workspace/ui-api-validation-20261020/api-http-latest.log](C:/Users/im/Documents/ChatGPT/批量解压缩/_workspace/ui-api-validation-20261020/api-http-latest.log) |
| 最新 API store 持久化/回滚/认证测试 | 7 通过 / 0 失败 | [_workspace/ui-api-validation-20261020/api-store-latest.log](C:/Users/im/Documents/ChatGPT/批量解压缩/_workspace/ui-api-validation-20261020/api-store-latest.log) |
| 最新拒绝请求体 transport 测试 | 2 通过 / 1 失败（缺少预期 Content-Length） | [_workspace/ui-api-validation-20261020/api-store-transport-latest.log](C:/Users/im/Documents/ChatGPT/批量解压缩/_workspace/ui-api-validation-20261020/api-store-transport-latest.log) |
| 首轮 API library | 4 通过 / 1 失败（Expect/饱和拒绝分支仍 poll body） | [_workspace/ui-api-validation-20261020/local-api-tests.log](C:/Users/im/Documents/ChatGPT/批量解压缩/_workspace/ui-api-validation-20261020/local-api-tests.log) |
| 最新 SSE disable 关闭测试 | 0 通过 / 1 失败；5 秒超时 | [_workspace/ui-api-validation-20261020/sse-close-latest.stderr.log](C:/Users/im/Documents/ChatGPT/批量解压缩/_workspace/ui-api-validation-20261020/sse-close-latest.stderr.log) |
| 首轮 MCP stdio 连接器 | 20 通过 / 0 失败；使用合成 HTTP 服务，不是真实解压 | [_workspace/ui-api-validation-20261020/mcp-tests.log](C:/Users/im/Documents/ChatGPT/批量解压缩/_workspace/ui-api-validation-20261020/mcp-tests.log) |
| 最新跨语言合同 | PASS：30 Desktop DTO / 8 Worker DTO / 302 字段 | [_workspace/ui-api-validation-20261020/contracts-latest.log](C:/Users/im/Documents/ChatGPT/批量解压缩/_workspace/ui-api-validation-20261020/contracts-latest.log) |
| 最新架构检查及 14 个负向控制 | PASS | [_workspace/ui-api-validation-20261020/architecture-latest.log](C:/Users/im/Documents/ChatGPT/批量解压缩/_workspace/ui-api-validation-20261020/architecture-latest.log) |
| npm audit | 0 已知漏洞；不证明业务授权安全 | [_workspace/ui-api-validation-20261020/npm-audit.json](C:/Users/im/Documents/ChatGPT/批量解压缩/_workspace/ui-api-validation-20261020/npm-audit.json) |
| 最新 cargo audit | 0 已知漏洞；4 unmaintained + 1 unsound 提示（全锁文件扫描，未将其直接归因 Windows 可达路径） | [_workspace/ui-api-validation-20261020/cargo-audit-latest.json](C:/Users/im/Documents/ChatGPT/批量解压缩/_workspace/ui-api-validation-20261020/cargo-audit-latest.json) |

最新前端 10 个 API 面板测试失败来自 mock 缺少 getDaemonConnection，不等于真实面板渲染崩溃；另 1 项是新增文案缺英文/日文翻译。新增的另外 3 项失败来自未配置连接的浏览器自动将 window.location.origin 当 daemon，旧预览测试因此发起网络请求并报 ECONNREFUSED，而不是返回预览快照/明确拒绝原生操作。需要明确 Web 控制台与离线预览的模式契约，并补齐失败/未连接状态；不能仅修改断言制造通过。HTTP 两项失败分别是旧的 43 命令断言（现在 45）和 localhost 新允许行为与旧合同不一致，后者不能直接当成恶意 Origin 已被放行。

两个 SSE 用例未包含在主要 HTTP 批次中，因为默认关闭通道存在持续就绪风险；其中 disable 已另行单进程、有外部 12 秒上限验证并失败。revocation SSE 与完整 concurrency suite 未宣称通过；真实 WS 探针单独覆盖撤销。cargo 多 test 目标遇到前一目标失败会停止，store 随后独立运行，不能把未执行的目标算通过。

首次 Vite 构建在系统 TEMP 删除 esbuild 临时文件时 Access denied；仅为验证进程设置独立 TEMP/TMP 后构建成功，未修改系统安全策略。初次完整 UI 运行被中断并记录，随后在独立 TEMP 下重新跑完整套件。

## 复核命令与交付范围

Rust 命令通过 scripts/run-cargo.sh 初始化 MSVC/SDK，并使用 CARGO_TARGET_DIR=target_architecture_transition、私有 CARGO_HOME 与 CARGO_BUILD_JOBS=1 串行编译。TEMP/TMP 指向 _workspace/ui-api-validation-20261020/temp。所有探针 API store/客户端/路径均为合成内容，不使用用户数据库、密码或 API key。

```text
# apps/desktop-ui
npm run lint
npm run build
npm test -- --maxWorkers=1
node node_modules/vitest/vitest.mjs run --config audit/ui-api-validation-20261020/vitest.config.ts

# repository root, via scripts/run-cargo.sh
cargo test -p openunpack-local-api --locked --offline --test http_contract -- --test-threads=1 --skip sse_contains_only_revision_invalidations_and_closes_after_revocation --skip disabling_api_closes_existing_event_stream_without_waiting_for_an_update
cargo test -p openunpack-local-api --locked --offline --test store_integrity -- --test-threads=1
cargo test -p openunpack-local-api --locked --offline --test rejected_body_transport -- --test-threads=1
cargo test --manifest-path _workspace/ui-api-validation-20261020/transport-probe/Cargo.toml --locked --offline --lib -- --test-threads=1
node scripts/check-contracts.mjs
node scripts/check-architecture.mjs --self-test
cargo audit --json

# apps/mcp-connector, initial round
npm test
```

实际浏览器检查已打开首轮预览的设置与 API 面板，确认页面能渲染，预览下原生能力禁用；这不是 Tauri 原生桌面操作验收。未完成打包 exe 的登录/启停/撤销/解压/恢复全链路，也未做真实文件破坏、攻击性负载或跨磁盘压力测试。

修复次序：先收敛便捷接口的授权路径与撤销/到期，再统一事件投影；然后修正响应信封、控制台入口、支持的连接方式和功能契约，补齐 regression 后跑原生 Host/Worker 验收。
