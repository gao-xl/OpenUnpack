# OpenUnpack 问题修复记录

日期：2026-10-10 至 2026-10-11，Asia/Shanghai。对应 [修复前项目审查](project-summary-2026-10-10.md) 的问题编号。保留原有未提交改动；不访问个人密码库；测试使用临时目录、公开压缩包和合成口令。

## 已修复的生产路径问题

| 编号 | 结果与实现 |
|---|---|
| I01 | 加密属性采用 7-Zip 的 Encrypted=15；混合 ZIP 优先测试加密条目。未知加密属性保持不确定结果，由协调器尝试批准的候选。AES 与 ZipCrypto 的普通条目排前回归覆盖旧 Core 和独立 Engine。 |
| I02 | 消费内包前持久化退休记录；恢复核对保留文件及残留内包，允许已消费的内包缺失。局部重试复用外包和成功兄弟节点。 |
| I03 | 删除缓存输入前持久化退休检查点，保留原摘要；支持旧 v1 SavedRun 的安全迁移。缓存完全删除可以恢复，残留缓存遭篡改则拒绝。 |
| I04 | Engine 按有限头部签名选择已有 8 种格式，支持改扩展名的 ZIP/7z/RAR 等。扫描器拒绝可执行文件、MZ 自解压包及未支持的 Zstandard；正常后续分卷不强求重复签名。TAR 首字段是文件名，以完整 512 字节头校验和识别合法 TAR，避免把 MZ 文件名当程序；含 USTAR 与旧 V7 实际样本。 |
| I05 | 大清单直接流式写入独占临时文件，计算长度/SHA256后重命名。Rust 强制校验工作区路径、随机 ID、128MiB 上限、摘要、条目限制和相对路径；异常保留证据且拒绝结果，只删除验证通过的自有清单。 |
| I06 | v1 ready 携带真实 Hello v2 能力；当前 Core 发 v2 请求。所有响应及控制指令关联 requestId/operationId/generation，拒绝旧响应和错误取消/退出，保留 v1 控制兼容。v2 Probe 只返回候选 ID，不回传口令；保存的批准候选每批最多 128 条。 |
| I07 | Core 与 Host 的可见写入及 snapshot 使用统一门锁；revision 以十进制字符串传输，UI 用 BigInt 比较，拒绝旧快照/退休会话；注册监听完成后补读 snapshot，关闭首次监听窗口。 |

文件名口令提示只提升启用且符合密码组范围的已保存候选；没有把未保存、禁用或组外提示作为新口令注入。取消探测后立即检查 CancellationToken，避免原生 Abort 被初次探测试解误判为 Corrupt。

## 新分层模块的接入前修复

| 编号 | 结果与边界 |
|---|---|
| M02/M03 | SQLite 在事务内迁移旧 JobView，统一兼容读取/更新，保留冻结配置、全部嵌套节点、阶段时间和状态。未知格式及失败事务拒绝，不静默降级；曾被旧新模型丢掉的 options 保持缺失并提示需确认。 |
| M04 | Application 持久化完整聚合与指定节点进度，提交失败不会先改内存/revision；通过真实 SQLite 验证。 |
| M05 | Supervisor 持有并等待 JoinHandle，同时依赖 WorkerClient 的真实进程退出确认；退出不确定保留错误及会话，阻止成功清理。 |
| M06 | 新 Vault Store 复用现有 DPAPI Vault，保留原文、分组、导入预览、备份恢复及原子写入。成功事件携带稳定 ID、来源、候选 revision，防重放及过期更新。 |
| M01 | **仍待开发**：新 TaskCoordinator 的完整启动、嵌套、事件驱动、发布恢复/outbox及运行对等。Host 继续使用现有 Core Coordinator，新旧任务 JSON 不允许两套协调器同时写同一库。 |

## 验证与交付

最终固定源码快照的完整门禁、Windows 候选包构建及隔离启动检查已通过。期间另有会话修改工作目录，故最终测试与构建均绑定 `frozen-source-b714beb948b54032bdcb65aabcdfd5b9/`，附 258 个源码文件摘要；快照保存后没有变化。并行会话此后的修改不包含在本候选或本次通过声明内。此前候选仅留作内部对照。日志统一保存在 `artifacts/issue-fixes-2026-10-10/`，保留第一次失败和修正后的通过结果。

已独立通过：UI 23 项及前端构建；C# 最终 40 项回归（包括真实大清单生产端，使用 4 万条元数据，不声称解压了 4 万实体文件）；v1 管道兼容 23 项；v2 原生控制、候选命中及实际解压 14 个断言；Rust/C# 实际 DTO 19 组互读；13 个桌面 DTO、7 个 Worker DTO 的 161 个字段结构检查及 4 组破坏性变异检查。

| 最终命令 | 结果 | 日志 |
|---|---|---|
| `cargo test --manifest-path <固定快照>/Cargo.toml --target-dir <项目>/target --workspace --locked --offline -- --test-threads=1`（MSVC 环境、CARGO_INCREMENTAL=0） | **108 通过，0 失败，0 忽略**；含 Core 41、真实桌面管线 21、格式 6、Scanner 5、管道 3、Host 2、Application 3、Domain 7、Infra 15、Protocol 5 | `rust-frozen-final.log`（此前 106/107 轮次日志保留） |
| `npm --prefix apps/desktop-ui test` | **23 通过**；修正 Vitest include 后实际收录新增 .test.ts，4 个测试文件 | `ui-final-tests.log` |
| `npm --prefix apps/desktop-ui run build` | TypeScript/Vite 通过；既有大 chunk 提示保留 | `ui-tests.log` |
| `dotnet run --project tests/OpenUnpack.Tests/OpenUnpack.Tests.csproj -c Release --no-restore` | **40 通过，0 失败**；nativeAvailable=true；含取消回归与 USTAR/V7 首名 MZ；原稀疏指纹 100ms 门槛未放宽，独立运行通过 | `dotnet-tar-edge-final.log`（先前轮次日志保留） |
| `scripts/check-contracts.ps1 -RunCargoTest` | 结构检查、4 个变异、19 个实际序列化互读与 golden samples 通过 | `contracts-final.log` |
| `src/OpenUnpack.Worker/verify-worker.ps1` | v1 兼容 **23/23** | `worker-v1-compat.log` |
| `scripts/verify-worker-v2.ps1` | 真实 v2 控制/候选/解压 **14 断言通过** | `worker-v2-final.log` |

`scripts/verify.ps1` 已接入实际 DTO 互读，`-WorkerProtocol` 同时检查 v1/v2。第三方材料离线审计通过：488 个组件、1030 个材料文件。初次 `source-freeze.json` 后增加取消/TAR 回归，并检测到并行编辑；最终以固定快照的 `SOURCE-SHA256.json` 为准。固定快照 UI 与通过 23 项测试的 UI 源码摘要一致。原工作区 `.workbuddy/memory/2026-10-10.md` 有末尾空行提示，本轮保留该原有文件；产品补丁另作范围内检查。

取消缺陷另有真实前后对照：复制 Engine 到隔离诊断目录，只撤回初次 native Extract 后的取消检查；先观察进程真实读取超过 2MiB 数据再取消，不用猜测延迟。修复前 `Corrupt`、修复后 `Cancelled`，日志 `probe-cancel-native.log` 与 `q01/results.json`。长期回归在旧 Core 和独立 Engine 均确认 Cancelled。

TAR 格式边界也有真实前后对照：USTAR/V7 首条目为 MZ-file.txt，旧过滤返回 Unsupported，新 Engine 解出独立原始字节；`tar-edge-native-before-after.log` 与 `tar-edge/results.json`。长期 C# TAR 回归在两实现覆盖，Rust 增加实际扫描至发布的完整链回归。

固定快照 Release 宿主与自包含 .NET 10.0.9 Worker 已构建，7z.dll 和许可材料齐全。最终候选目录 `artifacts/issue-fixes-2026-10-10/OpenUnpack-fixed-2026-10-11-final/`。候选包内 Worker 用同目录 7z.dll 再跑真实 v2 解压/控制 **14 断言通过**，没有旧 OpenUnpack.Core DLL 依赖，日志 `packaged-worker-frozen-native.log`。Cargo 的 lib/bin PDB 同名提示保留在构建日志，构建成功；不修改发布目标以隐藏提示。

最终 OpenUnpack.exe 为实际 Windows Release 程序，在独立 OPENUNPACK_DATA_DIR 与 WebView2 profile 下成功启动，确认本机 WebView2 子进程及 SQLite 初始化，并正常关闭、退出码 0，日志 `desktop-frozen-startup.log` 和 `host-smoke-939c0f4507064e10b62206f550835584/results.json`。关闭时 WebView2 输出窗口类注销 1412 日志，保留原始输出；这次烟雾检查不覆盖所有交互或资源长期运行。没有打开个人库。

最终便携 ZIP：`artifacts/issue-fixes-2026-10-10/OpenUnpack-fixed-2026-10-11-final-windows-x64-portable.zip`；解压整个目录后运行 OpenUnpack.exe，附自包含 Worker、7-Zip 引擎、许可/源材料、修复记录和 SHA256SUMS。固定源码另存快照目录及 SOURCE-SHA256.json。旧发行目录保持原样，没有公开发布，也没有构建或验证新的安装器。

DTO 层仍明确记录 C# 对非空引用的 null/缺失值比 Rust 宽松，以及 usize 与 C# int 容量范围差异；真实操作入口另作校验。两端均拒绝数值字符串、负数和无符号溢出。该工具没有把这些差异伪装成完全相等。

尚未取得物理 HDD/SSD 跨盘性能、干净机器安装、系统故障/断电、升级回滚或完整人工桌面 UI 验收。缓存路由分数与临时目录回归不是物理磁盘性能证明。CSV、SFX/Zstandard、完整新架构切换等仍按原待开发清单管理。
