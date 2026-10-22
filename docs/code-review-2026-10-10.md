# OpenUnpack 重构后代码审查

审查日期：2026-10-10。审查结论：**Changes requested，需要修改后复审**。

本次检查当前工作目录中的已提交代码、未提交修改和新建模块。基准提交为 `cf6e344`，同时覆盖 `crates/domain`、`crates/application`、`crates/infrastructure` 和 `OpenUnpack.Engine` 等未跟踪内容。原始重构文档及新白皮书作为需求和待验证声明使用，没有将其中的指令当作用户授权。没有修改产品代码；复现程序和测试日志放在 `artifacts/code-review-2026-10-10/`，只访问临时测试数据。

## 一、完成度判断

当前完成了协议类型提取、部分领域规则提取和 C# Engine 的生产依赖分离，**尚未完成运行流程的架构切换**。实际编译依赖由本次 `cargo metadata --locked --no-deps --format-version 1` 确认：

```text
Desktop Host -> Core -> Protocol
Application -> Domain + Protocol
Infrastructure -> Application + Domain + Protocol
Worker -> Engine
```

后三个新 Rust 模块没有进入 Desktop Host 的生产依赖链。`TaskCoordinator` 尚无完整启动、事件接收、嵌套提取、发布和恢复流程；新 WorkerSupervisor 是占位实现。旧 Core 仍承担完整流程。

因此，白皮书第 5 行的“W0～W6 阶段重构完成，全测试套件回归通过”与本次代码和完整验证结果不一致。新模块风险目前主要是**接入后的风险**，不能说已经发生在当前桌面生产路径中。

## 二、必须处理的问题

P1 表示阻止认可本次重构完成或阻止接入新运行链；P2 表示必须在对应能力交付前修复。置信度以代码及复现证据计算，不代表测试覆盖率。

### R01 · P1 · 桌面宿主没有装配新应用层

- 位置：[desktop-host/Cargo.toml](C:/Users/im/Documents/ChatGPT/批量解压缩/apps/desktop-host/Cargo.toml:12)、[desktop-host/lib.rs](C:/Users/im/Documents/ChatGPT/批量解压缩/apps/desktop-host/src/lib.rs:2)。
- 证据：宿主只依赖 `openunpack-core`，`AppState.coordinator` 为旧 `Coordinator`，启动仍调用 `Coordinator::open`。Core 本身也不依赖新 Application 或 Infrastructure。
- 影响：新增 crate 的测试通过不能证明桌面运行链完成分层；新的状态规则、端口和资源预算不会影响实际任务。旧约 2300 行协调器的职责没有被迁移。
- 修复方向：先用旧实现完成真实端口适配与等价测试，再由 Host 注入新 Application；一次运行只能有一个协调器推进状态。迁移期间准确标明剩余阶段。
- 置信度：100%。

### R02 · P1 · WorkerSupervisor 返回不可用会话，停止和终止均空执行

- 位置：[supervisor.rs](C:/Users/im/Documents/ChatGPT/批量解压缩/crates/infrastructure/src/worker/supervisor.rs:25)。
- 证据：`_req_rx` 和 `_evt_tx` 在 `start_session` 返回时直接销毁。方法没有启动进程、调用 WorkerClient 或绑定 Job Object，`request_stop` 和 `terminate` 只返回 `Ok(())`。
- 复现：不存在的 Worker 路径仍报告启动成功；首次发送请求失败，事件接收端为 `Disconnected`；停止、终止也报告成功。对应 `worker_session`。
- 影响：新链一旦接入，所有操作都无法执行；停止成功的返回值不能证明进程已退出。
- 修复方向：用真实 WorkerClient 实现会话、认证管道和退出确认；在实现前返回明确未实现错误，不能成功返回。新协调器还须真正登记和回收活跃会话，并处理终止失败。
- 置信度：100%。

### R03 · P1 · 新密码库会静默忽略旧文件并在写入时覆盖它

- 位置：[vault_store.rs](C:/Users/im/Documents/ChatGPT/批量解压缩/crates/infrastructure/src/dpapi/vault_store.rs:26)，持久化位于同文件第 45 行。
- 证据：旧 `passwords.vault` 是整文件 DPAPI 密文；新实现直接按 `Vec<StoredItem>` JSON 读取。解析失败被忽略并返回空库，随后使用 `fs::write` 覆盖原文件。损坏的新 JSON 也会被当作空库。写入没有复用旧 Vault 的原子提交与失败回滚。
- 复现：用旧 Vault 写入 1 条合成密码，新 Store 打开后条目为 0；添加新条目后原文件已改变，旧 Vault 无法再打开。对应 `legacy_vault_upgrade`。
- 影响：接入并沿用原路径后，用户首次新增或导入密码可能丢失整个旧库；中断写入也缺少可靠保护。
- 修复方向：优先适配现有 Vault；确需新格式时必须识别版本、迁移、备份并原子替换。未知或损坏格式应报错并保留原字节，内存只在持久化成功后提交。
- 置信度：100%。安全与数据完整性严重程度：High，尚未接入生产链。

### R04 · P1 · 新 SQLite 仓储不能读取旧任务，也没有版本迁移

- 位置：[task_store.rs](C:/Users/im/Documents/ChatGPT/批量解压缩/crates/infrastructure/src/sqlite/task_store.rs:93)。
- 证据：旧数据库中存储的是 camelCase 的 `JobView`（含 `source`、`options`）；新仓储直接按扁平 `JobAggregate` 解析，包含 `source_path`、`generation` 等字段。两者继续使用 `user_version=1`，没有转换步骤；解析错误被忽略。
- 复现：旧 Repository 成功保存 1 条任务，新 SqliteTaskStore 成功打开同库但加载数量为 0。对应 `legacy_job_upgrade`。
- 影响：接入后任务历史及中断任务从界面消失，而不是明确报告不兼容；未完成发布记录也无法由新应用层正确恢复。
- 修复方向：保留独立 `JobRecordV1`，严格读出并转换领域对象；进行带版本的迁移，未知版本或坏记录不能静默跳过。新建数据库也要保留发布与成功 outbox 的完整契约。
- 置信度：100%。

### R05 · P1 · 删除任务取消了运行状态和未完成发布的保护

- 位置：[task_store.rs](C:/Users/im/Documents/ChatGPT/批量解压缩/crates/infrastructure/src/sqlite/task_store.rs:194)、[jobs.rs](C:/Users/im/Documents/ChatGPT/批量解压缩/crates/application/src/use_cases/jobs.rs:55)。
- 证据：直接 `DELETE FROM jobs`，不检查 `running/stopping`、未完成 publication，也没有关联清理事务。旧 Repository 有这些保护。
- 复现：保存 Running 任务并插入 `publishPending` 记录，删除报告成功，任务不存在而 publication 仍存在。对应 `active_job_deletion`。
- 影响：新入口接入后用户可删除正在执行或需要恢复的任务，使持续运行的 Worker 和发布工作区失去任务记录。
- 修复方向：在同一短事务中拒绝活动任务或未核销 publication；仅允许删除可安全退役记录，并保持任务与发布记录一致。清空终态同样需要此保护。
- 置信度：100%。

### R06 · P1 · 完整 Rust 验证不能编译

- 位置：[worker_client.rs](C:/Users/im/Documents/ChatGPT/批量解压缩/crates/core/tests/worker_client.rs:17)。
- 证据：本次完整 `cargo test --workspace --locked -- --test-threads=1` 报 `E0063`。测试构造 `WorkerRequest` 缺少 `candidate_passwords`、`candidates`、`generation`、`operation_id`；Core 生产构造器已补齐，测试尚未补齐。
- 影响：`scripts/verify.ps1` 中断，现有 CI 的核心验证门禁不能通过，完整套件没有执行完成。
- 修复方向：同步测试构造器并重新运行原始完整门禁。单独运行可编译的子集不能代替完整通过。
- 置信度：100%。

### R07 · P2 · 非法状态转移被吞掉，数据库列和 JSON 相互矛盾

- 位置：[task_store.rs](C:/Users/im/Documents/ChatGPT/批量解压缩/crates/infrastructure/src/sqlite/task_store.rs:141)，`mark_job_completed` 第 176 行有相同问题。
- 证据：忽略 `job.set_status` 的错误，仍把请求状态写到 `status` 列，同时序列化未改变状态的聚合。
- 复现：Queued → Completed 本应被拒绝，实际返回成功；SQL 列为 `completed`、JSON 状态为 `queued`，随后 `clear_terminal_jobs` 删除了该 queued 任务。对应 `invalid_state_transition`。
- 影响：状态检查与清理依据不同数据源产生矛盾，未执行的任务可能被清除。找不到任务或 JSON 损坏也不应成功返回。
- 修复方向：传播领域错误，只有合法转移才能写入；缺失任务返回明确错误，保证索引列和记录内容一致。
- 置信度：100%。

### R08 · P2 · 状态在数据库提交前已改变，写入失败后不回滚

- 位置：[coordinator.rs](C:/Users/im/Documents/ChatGPT/批量解压缩/crates/application/src/use_cases/coordinator.rs:148)。
- 证据：先改变 jobs 内存中的状态、阶段、进度、消息，再调用持久化；失败直接返回，没有恢复内存。
- 复现：用 SQLite trigger 注入 UPDATE 失败，接口返回错误；快照仍为 `running`，数据库为 `queued`，revision 仍为 1。对应 `persistence_failure`。
- 影响：同一 revision 已对应不同内存状态，重启前后状态不一致，UI 的失败结果也与快照矛盾。
- 修复方向：对聚合副本校验，成功提交数据库后替换内存，再递增 revision 和发布失效通知。
- 置信度：100%。

### R09 · P2 · JobUseCases 绕过协调器，新增与重试无法同步快照

- 位置：[jobs.rs](C:/Users/im/Documents/ChatGPT/批量解压缩/crates/application/src/use_cases/jobs.rs:30)、[coordinator.rs](C:/Users/im/Documents/ChatGPT/批量解压缩/crates/application/src/use_cases/coordinator.rs:35)。
- 证据：TaskCoordinator 只在构造时读取 jobs；JobUseCases 后续直接写 Store。两者没有共享更新入口、缓存失效或 revision 通知。
- 复现：先构造协调器，再注册任务，数据库 2 条，协调器快照只有 1 条。对应 `registered_job_snapshot`。
- 影响：接入后新建任务不显示；重试、移除、清理也可能继续显示旧状态。“唯一状态权威”实际尚未成立。
- 修复方向：将所有任务写操作统一经过协调器，或取消双份独立状态并从权威仓储生成一致快照。
- 置信度：100%。

### R10 · P2 · 磁盘查询失败伪装成有 100 GiB 空间

- 位置：[environment.rs](C:/Users/im/Documents/ChatGPT/批量解压缩/crates/infrastructure/src/system/environment.rs:59)。
- 证据：`GetDiskFreeSpaceExW` 返回失败时仍返回 `Ok(107374182400)`。
- 复现：查询不存在的临时目录，得到成功的 100 GiB 结果。对应 `disk_probe_error`。
- 影响：资源预检错误放行，网络卷不可达、磁盘移除或权限错误被掩盖。
- 修复方向：传播系统错误，或返回可明确处理的 Unknown；空间安全判断不能用固定成功值代替失败。
- 置信度：100%。

### R11 · P2 · 硬件探测和防休眠仍为占位

- 位置：[environment.rs](C:/Users/im/Documents/ChatGPT/批量解压缩/crates/infrastructure/src/system/environment.rs:27)，防休眠第 73 行。
- 证据：C 盘直接认定 SSD，其余为 Unknown；没有调用已存在的磁盘拓扑探测。`acquire_sleep_lock` 只返回 `NoopLock`，没有获取或释放系统防休眠状态。
- 影响：若以新适配器替换旧路径，机械 C 盘会被错误分类，后台长任务也失去已有防休眠行为。当前旧 Core 的行为未因此改变。
- 修复方向：适配已存在且经过验证的 topology/power 实现；探测失败保留 Unknown，防休眠由实际系统租约管理。
- 置信度：100%，代码证据；没有宣称已进行物理硬盘或休眠验收。

### R12 · P2 · 新排除规则把一个中文字符当成多个 UTF-8 字节

- 位置：[exclusions.rs](C:/Users/im/Documents/ChatGPT/批量解压缩/crates/domain/src/archive/exclusions.rs:22)。
- 证据：新 wildcard 使用 `as_bytes`，`?` 只推进一个字节，旧 Rust 规则按字符推进。非 ASCII 大小写匹配也只处理 ASCII。
- 复现：`?.txt` 匹配 `中.txt`，旧 Core 为 true，新 Domain 为 false。对应 `unicode_exclusion_parity`。旧 scanner 已有针对这种情形的回归测试。
- 影响：迁移后同一排除设置处理不同文件，中文用户尤易遇到。Domain 现有测试没有覆盖这一对等条件。
- 修复方向：明确统一 Unicode 语义，把原回归样本同时运行于 Domain、Rust scanner 和 C# Engine。
- 置信度：100%。

### R13 · P2 · 契约校验不能检测现有漂移，Worker v2 尚未实现

- 位置：[check-contracts.ps1](C:/Users/im/Documents/ChatGPT/批量解压缩/scripts/check-contracts.ps1:26)、[worker.rs](C:/Users/im/Documents/ChatGPT/批量解压缩/crates/protocol/src/worker.rs:74)、[Protocol.cs](C:/Users/im/Documents/ChatGPT/批量解压缩/src/OpenUnpack.Worker/Protocol.cs:22)。
- 证据：脚本仅检查接口或 record 名称是否存在；没有比较字段、类型、可空性、默认值或跨语言 JSON，且 Windows CI 没调用该脚本。Rust 已新增 candidates/operationId/generation/manifest 摘要等字段，C# DTO 没有对应成员；本次脚本仍打印“verified and in sync”。黄金样本仅在 Rust 内往返。
- 影响：使用新 candidates 字段时 C# 会忽略它；WorkerHost 只接受 v1，ready 也不是 WorkerHello。仅加 v2 常量与结构不能提供协商或拒绝旧结果。AppSnapshot 仍没有 sessionId/revision，新 revision 没有传给 UI。
- 修复方向：先明确 v1 冻结边界，再实现真实 v2 协商与 operation/generation 校验；建立 Rust、TS、C# 的真实序列化往返样本并接入 CI，不能用名称存在检查作防漂移验收。
- 置信度：100%。

### R14 · P2 · 实际生产引擎将损坏包错误分类为需要密码

- 位置：[SevenZipEngine.cs](C:/Users/im/Documents/ChatGPT/批量解压缩/src/OpenUnpack.Engine/Native/SevenZipEngine.cs:92)、[旧协调器](C:/Users/im/Documents/ChatGPT/批量解压缩/crates/core/src/coordinator.rs:1128)。
- 证据：`Probe` 捕获 `ArchiveFailure.Corrupt` 后设置 `headerEncrypted=true`，候选全部失败便返回 `WrongPassword`；协调器将其转换为 `NeedsPassword` 并跳过正常提取。未收到密码请求的目录损坏也被视为加密。
- 实际回归：`explicit_partial_delivery_retains_corrupt_inner_archive_and_ordinary_files` 在当前 Worker/Engine 上失败，合成坏内包返回 `needsPassword`，预期为 `failed`。这不是新 Rust 模块接入后的假设，当前生产路径已经包含此逻辑；旧 C# Core 中也有相同实现。
- 影响：损坏归档被提示补充密码，用户无法准确判断失败原因，损坏/部分交付的状态契约不成立。
- 修复方向：保留原生密码请求、目录损坏及 Data/CRC 错误的来源，不把所有 Corrupt 当作加密证据；修复后恢复该原生回归测试，并验证正确加密头仍能正常识别。
- 置信度：100%。

### R15 · P2 · 完整传统 RAR 分卷被可选末卷探测误判为缺卷

- 位置：[SevenZipEngine.cs](C:/Users/im/Documents/ChatGPT/批量解压缩/src/OpenUnpack.Engine/Native/SevenZipEngine.cs:578)，最终校验位于第 480 行。
- 证据：OpenVolumeCallback 对任意不存在的后继文件设置永久 `MissingVolume=true`；即使原生 `Open`、条目读取及解码可以成功，最终 ValidateResult 仍以此标志拒绝结果。官方 26.04 的 RarHandler.cpp 第 1115～1124 行明确区分可选后继卷与 `nextVol_is_Required`，返回 `S_FALSE` 本身不等于真正缺卷。
- 实际回归：`rar_part_volumes_and_traditional_r00_family_accept_middle_volume_selection` 的传统 `Rar2.multi.rar + .r00～.r05` 场景失败，扫描确认 7 卷齐全，最终仍为 `MissingVolume`。其他 partNN RAR 场景此前已经通过。
- 独立确认：同一组 fixture 用当前官方 `7z.exe t` 返回 0、`Everything is Ok`，7 卷、3 个文件；新 Engine 独立诊断成功读取 5 个条目，但成功 Open 后 MissingVolume 已为 true，提取被错误拒绝。
- 影响：当前生产链无法交付合法完整的该类传统 RAR 分卷，补充并不存在的后继卷也无法解决。
- 修复方向：把“未找到请求卷”和“原生确认必需分卷缺失”分开，根据原生归档错误、实际解码与逐项结果判断失败；保留缺少中间卷、未批准卷和路径越界保护，不能简单删除所有缺卷检查。
- 置信度：100%。日志为 `rar2-official-cli.log`、`engine-probe.log`；官方源码摘录保存在本次审查 artifacts 中。

## 三、验证证据

完整验证失败后，仅对可独立执行的子集补充检查；这不会改变 R06 的失败结论。所有测试密码均为合成或公开 fixture 常量，没有读取真实用户密码库。

| 检查 | 本次结果 | 证据 |
|---|---|---|
| `scripts/verify.ps1 -SkipInstall -WorkerProtocol` | 失败：Rust 集成测试 E0063；之前的 Worker 构建、前端构建、C# 与 UI 测试通过 | `artifacts/code-review-2026-10-10/verify.log` |
| .NET Worker + Engine Release 构建及 self-contained publish | 通过，Release build 0 警告、0 错误 | 同上 |
| UI TypeScript/Vite production build | 通过；包大小提示不作为本次正确性阻塞 | 同上 |
| UI Vitest | 15/15，3 个文件 | 同上 |
| C# 测试程序 | 27/27；仍引用旧 C# Core，不能单独证明新 Engine 全格式对等 | 同上与 `tests/OpenUnpack.Tests/OpenUnpack.Tests.csproj` |
| `src/OpenUnpack.Worker/verify-worker.ps1` | 23/23；实际使用新 Engine、认证管道、官方 7z.dll | `artifacts/code-review-2026-10-10/worker-protocol.log` |
| Rust 库单元测试 | 43/43，其中旧 Core 34、Host 2、Domain 7；Application 没有单元测试 | `artifacts/code-review-2026-10-10/rust-libraries.log` |
| Rust 真实桌面管线集成子集 | 17 项中 15 通过、2 失败，详见下文 | `artifacts/code-review-2026-10-10/core-integration.log` |
| 协议黄金样本 | 4/4；首次增量编译出现 rustc 内部 panic，关闭增量编译重试通过 | `artifacts/code-review-2026-10-10/golden-retry.log`，原始失败为 `golden.log` |
| 新仓储已有测试 | 2/2；仅新文件基础生命周期，没有旧库和失败写入覆盖 | `artifacts/code-review-2026-10-10/new-stores.log` |
| 真实格式管线 | 4 项中 3 通过、1 失败：传统 RAR 分卷，见 R15 | `artifacts/code-review-2026-10-10/format-scanner.log` |
| 扫描回归 | 5/5，格式套件中断后单独执行扫描套件 | `artifacts/code-review-2026-10-10/scanner.log` |
| 独立故障与迁移复现程序 | 9/9 假设均复现；这是缺陷证据，不能算产品验收通过 | `artifacts/code-review-2026-10-10/defect-reproductions.json` |
| 新 Engine 独立诊断 | 2/2 缺陷复现：Corrupt → WrongPassword；可选后继卷导致 MissingVolume | `artifacts/code-review-2026-10-10/engine-probe.log` |
| `scripts/check-contracts.ps1` | 返回成功，但未检测 R13 所列字段漂移 | 现场执行输出及脚本源码 |

复现程序入口：`artifacts/code-review-2026-10-10/probe/src/main.rs`。独立运行使用 `cargo run --manifest-path artifacts/code-review-2026-10-10/probe/Cargo.toml --offline`，并使用项目 Windows 构建环境。该程序在临时目录建立旧 Vault/SQLite、注入更新失败并检查通道断开；不修改产品源码或用户数据。

本次补充执行的 Rust 命令：

```powershell
cargo test -p openunpack-domain -p openunpack-application -p openunpack-infrastructure -p openunpack-protocol -p openunpack-core -p openunpack-desktop --lib --locked -- --test-threads=1
cargo test -p openunpack-core --test scanner_regressions --test format_matrix --test desktop_pipeline --locked -- --test-threads=1
# 上述联合集成命令遇到失败会提前中断，后续套件单独执行；首次协议测试的 rustc 内部错误单独重试。
$env:CARGO_INCREMENTAL='0'
cargo test -p openunpack-protocol --test golden_samples --locked -- --test-threads=1
cargo test -p openunpack-infrastructure --test store_tests --locked -- --test-threads=1
cargo test -p openunpack-core --test scanner_regressions --test format_matrix --locked -- --test-threads=1
cargo test -p openunpack-core --test scanner_regressions --locked -- --test-threads=1
dotnet run --project artifacts/code-review-2026-10-10/engine-probe/EngineReview.csproj -c Release
```

关闭增量编译仅是本次工具链重试条件，没有改项目配置或产品代码，也没有据此证明已修复编译器。原始 panic 日志保留。审查期间对关键源文件的 SHA256 再次核对均未改变，记录见 `source-snapshot.json`。

两项桌面管线失败须分开判断：坏内包状态失败已定位到 R14；`renamed_cached_archive_uses_content_association_ahead_of_global_password_scores` 在第 872 行预期 attempts=2，实际为 1。当前 Probe 先选择口令，然后只执行一次 extraction，attempts 仅在 extraction 前递增，因此旧断言不能直接代表候选排序错误。应改为验证实际候选顺序/身份和成功计数，不能仅把数字改成 1 就宣称排序已验证。本次原测试失败仍按失败记录。

已审查入口与可信边界：UI/Tauri 命令、用户选择器路径授权、Host/Worker 双向进程身份校验、请求与响应尺寸限制、压缩条目路径、分卷批准列表、密码库持久化、SQLite 状态、发布与取消。23 项实际 Worker 检查覆盖了路径穿越、符号链接、大小预算、复制摘要、目标回读、分卷、加密头密码、取消和心跳，以及 stdout/stderr 未包含本次请求或秘密。

限制：未进行清洁机器安装、发行 EXE 的窗口操作验收、物理 HDD/SSD 性能、断电或真实系统休眠测试。原生格式测试和库测试属于局部证据。完整 Rust 门禁仍失败，不能据此发布“全部通过”的结论。

## 四、收尾顺序

1. 修复完整验证的编译错误、生产引擎的坏包分类及 RAR 后继卷误判，并准确更新完成度说明；修正候选优先级测试的验证方法。
2. 先用旧 Vault/Repository/WorkerClient 等真实实现完成新端口适配，补齐旧库迁移、失败写入、活动任务保护、取消与退出确认回归。不能把当前新适配器直接切换到用户数据路径。
3. 统一任务写入入口，补齐发布/恢复/outbox 与 UI session/revision 后再切换 Host；保留用户任务选项、节点与阶段计时等已实现行为。
4. 按新运行链重跑完整 Windows 门禁，再完成打包、真实桌面操作和清洁机器安装验收。之后才核销旧 Core 与历史参考代码。

不建议先增加更多分层目录。现在最需要补齐的是生产调用链、数据兼容和失败路径的行为。
