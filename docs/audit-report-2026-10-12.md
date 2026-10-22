# OpenUnpack 全面代码与架构审计报告

**审计日期**：2026-10-12 · **审计方案**：`docs/audit-plan-2026-10-12.md` · **审计方式**：全量源码人工审查（主审逐文件阅读 + 定向静态分析），所有发现附文件：行号证据，并与 `docs/status-ledger-2026-10-11.md` 交叉核对去重。

---

## 0. 总体结论

**未发现 P0 级问题。** 本项目的安全工程与并发纪律显著高于同类桌面应用的常见水平：路径穿越防护是双栈纵深（Rust `paths.rs` + C# `SevenZipEngine` 各自独立拒绝符号链接/目录联结/设备名/重复路径/越界写入）、密码全链路 zeroize、Worker 进程治理完备（CurrentUserOnly 管道 + Job 对象 + 父进程监视 + 客户端身份校验）、SQL 全参数化、前端 strict 模式下 0 处类型逃逸、全仓 0 处 TODO/FIXME、生产代码 0 处 `unwrap()`（coordinator 生产段 3359 行）。

**主要风险集中在架构层面**：新旧双轨迁移产生了约 2,900 行近乎逐字复制的代码，其中 `database_lease` 已发生实质分叉——这是当前唯一"已经兑现"的漂移风险。其次是台账口径与代码事实存在一处偏差（详见 A2）。

### 风险矩阵

| 等级 | 数量 | 条目 |
|---|---|---|
| P0 严重 | 0 | — |
| P1 高 | 3 | A1 双轨大规模复制且已分叉 · A2 infra 残留 core 依赖声明（台账口径失真）· P1 快照全量复制协议（台账已知，审计复核确认） |
| P2 中 | 6 | A3 Coordinator 巨石对象 · S2 协议 DTO 派生 Debug 未脱敏与内存覆零 · F1 复制循环逐 MB 查盘 · E1 前端无 lint 工具链 · E2 CI 缺依赖漏洞扫描与签名 · G1 台账治理 |
| P3 低 | 4 | R1 Worker 静默退出 · S1 许可证下载无校验 · R2 毒化锁假条目 · F2 快照引擎探测 |

---

## A. 系统架构

### A1 【P1】新旧双轨约 2,900 行近乎逐字复制（`database_lease` 分叉表述已修正，见条目内注）

**位置**：`crates/core/src/vault.rs`（2163 行）vs `crates/infrastructure/src/dpapi/vault.rs`（2163 行）；`crates/core/src/worker.rs`（502 行）vs `crates/infrastructure/src/worker/client.rs`（502 行）；`crates/core/src/database_lease.rs`（284 行）vs `crates/infrastructure/src/sqlite/database_lease.rs`（140 行）；`crates/core/src/worker_manifest.rs`（312 行）vs `crates/infrastructure/src/worker/manifest.rs`（280 行）。

**证据**（实测 diff 行数）：

| 模块 | core 行数 | infra 行数 | diff 行数 | 重复率 |
|---|---|---|---|---|
| vault | 2163 | 2163 | 4 | ≈99.8% |
| worker client | 502 | 502 | 8 | ≈98% |
| database_lease | 284 | 140 | 0（前 140 行 SHA256 一致，修正见下注） | 本体 100% |
| worker_manifest | 312 | 280 | 部分分叉 | — |

**影响**：台账 §1#6 将"适配逻辑重复"列为已核销项的代价描述，但实际形态是**整文件复制**。此后任何密码库/Worker 客户端的安全修复都必须双写。若生产 core 侧修了一个 DPAPI 边界 bug 而 infra 副本漏改，切换新架构之日就是回归之时。

> **修正（2026-10-12 修复轮实测复核）**：本条目初版"`database_lease` 已实质分叉"及"行数相差一倍证明漂移"的判断**不成立**。实测：两份文件租约实现本体（core 版前 140 行）**SHA256 完全一致**（`8c99b898220077b8b0c6f56740471a61438758bd5c36dffcf80bbc8ab9c4b630`），行数差异源于 core 版自带 141-284 行测试模块——属**测试覆盖分布差异**，不是语义漂移。真实缺口是 infra 侧缺测试覆盖（已于本轮补齐，见下条 2 的实际落地）。

**修复建议与落地**（2026-10-12 修复轮）：
1. 立即确立**唯一真源**：在迁移完成前，安全敏感模块（vault、worker client）的修改必须同步双写，并在 `CONTRIBUTING.md` 或模块头注释中明文规定。
   → **已落地（模块头方案）**：6 个镜像文件（`vault.rs`×2、`worker.rs`/`client.rs`、`worker_manifest.rs`/`manifest.rs`、`database_lease.rs`×2）均加 `//! MIRROR WARNING` 横幅，指明对侧路径与"同变更集内双写"要求。未写 `CONTRIBUTING.md`——模块头横幅在编辑现场（打开文件即可见），防错更直接。
2. 短期（当前修复轮）：对齐两份 `database_lease`，以 core 版（284 行，含跨进程租约与硬链接/8.3 拒绝测试，台账 §7 有 11/11 验证记录）为准回灌 infra 版。
   → **已完成（选择回灌测试而非重写）**：复核确认租约实现本体逐字节一致，实际差距仅为测试覆盖。已将 core 版测试模块完整回灌 infra 版（唯一修改：跨进程测试的 `--exact` 目标路径改为 `sqlite::database_lease::tests::os_lease_is_enforced_between_processes`）。验证：infra 5/5 passed（3.42s）、core 基线 5/5 passed（9.66s）。
3. 中期：切换宿主到新分层后**物理删除** `crates/core` 对应模块；在删除前给 `crates/core/src/vault.rs`、`worker.rs` 文件头加 `// DEPRECATED: 真源在 crates/infrastructure` 横幅。
   → **计划已产出**：`docs/plan-core-retirement-2026-10-12.md`（宿主剩余使用面 = toolbox 2 命令 + `types::` 再导出 5 处 + 生产 `Coordinator`；工具箱为新分层最后待迁移项）。

**验证方式**：`diff crates/core/src/vault.rs crates/infrastructure/src/dpapi/vault.rs | wc -l` 应收敛到 0 或文件不存在。

### A2 【P1】`infrastructure/Cargo.toml` 残留 `openunpack-core` 依赖声明，与台账"已彻底切除"口径不符

**位置**：`crates/infrastructure/Cargo.toml:11` — `openunpack-core = { path = "../core" }`。

**证据**：`crates/infrastructure/src/` 全部源码中 `openunpack_core` 出现 0 次（Grep 全仓确认）——源码层面依赖确已切除，但清单声明残留。台账 §1#6 声称"彻底切除 `infrastructure/Cargo.toml` 中 `openunpack-core` 反向依赖……46/46 全绿"，与当前工作树事实不符。

**影响**：① 构建图仍然串联 core → 切除了编译单元的使用却未切除编译依赖，`cargo build -p openunpack-infrastructure` 仍会编译 core，违背"独立闭环"目标；② 台账作为唯一真源的可信度受损（台账维护约定要求"状态升级需附证据"，此行属于状态超前于代码）。

**修复建议**：删除 `crates/infrastructure/Cargo.toml:11`，跑 `cargo check --workspace` + infrastructure 全套测试确认无隐藏使用，然后修正台账 §1#6 表述为"源码依赖已切除；清单声明于 2026-10-12 审计后清除"。

**验证方式**：`cargo tree -p openunpack-infrastructure | grep core` 无输出。

### A3 【P2】Coordinator 巨石对象：3359 行生产代码 + 14 个细粒度互斥锁

**位置**：`crates/core/src/coordinator.rs:43-75`（结构体），生产段 1–3359 行。

**证据**：`Coordinator` 持有 `jobs / settings / active_cancel / publish_gate / cancelled_jobs / partial_requests / node_retry_requests / mutations / job_lifecycle / deleting_jobs / recovering_jobs / budget_reservations / view_state / transfers` 共 14 个锁，锁序纪律仅靠注释约定（如 70-71 行 "Snapshot readers and visible-state writers acquire this before field locks"）。`pub fn` 达 25 个，涵盖调度、恢复、发布、扫描、设置、密码库授权六大职责。

**正面事实**：审计逐点核查了全部 62 处 `.await` 与锁的邻近关系，**未发现任何 std::Mutex 守卫跨 await 持有**；RAII 守卫（`DeleteIntent`/`RecoveryOwner` 的 Drop）模式正确。这是"高风险结构被高纪律约束住"的状态。

**影响**：当前正确，但每新增一个命令/状态都要人肉维护锁序不变量；锁数量与文件规模都已超过可安全演进的阈值。

**修复建议**：中期重构为子模块（scheduler / recovery / publication / scan），每子模块自带状态与锁；`view_state` 与 `mutations`/`job_lifecycle` 的进入顺序写成 `const` 级文档断言（或 lock-order 测试）。不急于动——在双轨迁移（A1）收敛前，core 侧只做冻结性维护。

### 架构已检查未见问题项
- **crate 分层方向**：protocol 无上游依赖；domain 仅依赖 serde/thiserror/zeroize；application 依赖 protocol+domain；infrastructure 依赖前三者（外加 A2 的残留）。方向正确。
- **契约同步**：`scripts/check-contracts.mjs` 是真实的跨语言 DTO 结构化比对（Rust struct / TS interface / C# record 三向解析、serde casing/Option 可空/decimal 转换建模），非表面文本检查；含 v1/v2 兼容用例。
- **进程间协议**：双管道 + 版本握手 + 能力声明校验（`worker.rs:13-22` 对 hello 的逐项断言含"拒绝回传明文密码"）、请求/操作/代际三重身份匹配、orphan 防护（父进程监视 + `KILL_ON_JOB_CLOSE`）。
- **前端桥**：36 个 Tauri command 全部经 `verify_frontend_window` 校验事件来源；`api.ts` 统一封装错误映射与快照游标（session+revision 防乱序、150ms 防抖）。

---

## B. 安全漏洞与潜在风险

**本节结论：未发现可利用的安全漏洞。** 以下为检查记录与纵深防御建议。

### B-OK-1 路径穿越（zip-slip）：双栈纵深，防护完整
- C# 侧 `SevenZipEngine.ValidateRelativePath`（`Native/SevenZipEngine.cs:383-401`）：拒绝绝对路径、`..`、ADS（`:`）、NUL、尾随空格/点、保留设备名（含 COM¹²³ 上标变体）、`.openunpack-owner.json`；`ReadEntries`（:336-381）额外拒绝符号链接/硬链接/ADS/anti 条目与重复/大小写冲突路径；`GetStream`（:777-779）落盘前再做 `StartsWith(destination)` 包含性校验 + `FileMode.CreateNew` 禁止覆盖 + 写入前后两次 `EnsureNoReparsePoint`。
- Rust 侧 `paths.rs:13-31`（联接/符号链接祖先链拒绝）、`relative()`（:32-71，同类规则）、分卷 `GetStream`（:671-694）要求文件名无目录成分且必须在已批准集合内。
- **残余风险（P3 理论级）**：TOCTOU——`EnsureNoReparsePoint` 检查与 `FileMode.CreateNew` 之间，同机其他进程可在目标父目录换入联接。单机单用户场景攻击面极低；如需彻底闭合，需改用 `CreateFile` + `FILE_FLAG_OPEN_REPARSE_POINT` 拒绝语义。

### B-OK-2 命令/参数注入：无暴露面
引擎为进程内 COM 调用官方 `7z.dll`（`Native/SevenZipEngine.cs:605-640`，加载时校验 `.sha256` 伴生哈希），密码经 `ICryptoGetTextPassword` 回调传递（:655-660），**从不进入命令行**。全仓唯一 `Command::new` 是宿主 `open_path`（`apps/desktop-host/src/lib.rs:1052`），使用解析自 `%SystemRoot%` 的 explorer.exe + 参数数组（无 shell），且前置 `authorize_open` 授权门。

### B-OK-3 密码与密钥材料
- DPAPI 用户作用域 + `CRYPTPROTECT_UI_FORBIDDEN`（`vault.rs:1526-1585`）；备份为 PBKDF2-HMAC-SHA256 **60 万迭代**（`vault.rs:33`，恢复时下限 10 万校验 :711）+ AES-256-GCM + 随机 16B 盐/12B nonce（:673-681）。
- 内存卫生：`zeroize`/`Zeroizing` 贯穿 Rust 侧；C# 侧 `CryptographicOperations.ZeroMemory` 清请求缓冲区、`ZeroizeRequest` 清请求对象、v2 协议禁止 Worker 回传明文（`WorkerHost.cs:140`、`worker.rs:27` 断言 `matched_password.is_none()`）。
- 剪贴板：`clipboard.rs` 序列号条件清理（用户已复制他物则不清）+ 失败路径显式清零。
- 日志泄漏面：Grep 全仓无密码入日志；错误消息为固定枚举文案（`WorkerOperations.cs:391-402`），不回显输入。

### B-OK-4 SQL 注入 / 整数溢出 / XSS / 密钥
- `repository.rs` 全部 `params!` 参数化（10 处），无字符串拼接 SQL。
- 溢出：大小累加均有 `maxBytes - size` 预检（`SevenZipEngine.cs:365-367`、`WorkerOperations.cs:203-207,266`）与 `ulong` 边界校验。
- 前端：0 处 `dangerouslySetInnerHTML`；CSP `script-src 'self'`、`object-src 'none'`（`tauri.conf.json:5`）。
- 硬编码密钥扫描：仅测试夹具（`"super secret"` 等 3 处，均在 `#[cfg(test)]` 或测试模块）。
- 供应链：7-Zip MSI 下载后强制 SHA256 比对 pinned 值（`setup-engine.ps1:66-67`）；release-materials 下载带预期哈希（`collect-release-materials.mjs:22-38`）；CI action 全部 SHA pin + `permissions: contents: read`。

### S2 【P2】跨语言协议 DTO（`CandidateItem` / `WorkerRequest` / `WorkerEvent`）派生 `Debug` 未脱敏且未统一实现 `Drop` Zeroize
**位置**：`crates/protocol/src/worker.rs:36-43, 89-131, 161-203`；`src/OpenUnpack.Worker/Protocol.cs:23-30, 50-73`；`crates/protocol/src/desktop.rs:6-15`（`PasswordInput`）。
**证据**：同 crate 下的 `Candidate`（`crates/protocol/src/lib.rs:19-24`）显式实现了 `Drop` 调用 `self.secret.zeroize()`，但 IPC 协议层的 `CandidateItem`（含 `secret`）、`WorkerRequest`（含 `password`、`candidate_passwords`、`candidates`）、`WorkerEvent`（含 `matched_password`）及桌面端 `PasswordInput`（含 `secret`）均直接派生了 `#[derive(Debug)]` 且未实现 `Drop` 擦除；C# 侧 `public sealed record` 自动生成的 `ToString()` 亦会直接拼接明文属性。
**影响**：当前生产代码虽未将上述对象直接打入日志，且 C# 侧有 `ZeroizeRequest` 手动清理，但缺乏类型级防御（Defense-in-Depth）——一旦后续维护者在调试或异常分支中使用 `{:?}` 或 `$"{request}"`，明文密码将直接泄漏到日志或崩溃转储；Rust 端 DTO 释放后明文仍残留于堆内存。
**修复建议**：
1. Rust 侧：移除上述含密 DTO 的派生 `Debug`，手动实现 `fmt::Debug` 将敏感字段格式化为 `"[REDACTED]"`；为 `CandidateItem`、`WorkerRequest`、`WorkerEvent`、`PasswordInput` 实现 `Drop` 调用 `zeroize`。
2. C# 侧：在 `CandidateItem`、`WorkerRequest`、`WorkerEvent` record 中重写 `public override string ToString()` 屏蔽密码字段。

### S1 【P3】`license.txt` 下载无哈希校验
**位置**：`scripts/setup-engine.ps1:83`。纯文本许可证、不参与执行，仅作记录；可并入 collect-release-materials 的带哈希下载清单统一管理。

---

## C. 性能瓶颈

### P1 【P1·台账已知复核确认】快照全量复制协议：万条队列场景缺乏保障
**位置**：`crates/core/src/coordinator.rs:360-394`。
**证据**：每次 `snapshot()` 在 `view_state` 锁内 `jobs.lock()?.clone()`（:362）复制全部任务 + `vault.views()` 全量密码视图；前端每次变更通知后 150ms 防抖拉取全量快照（`api.ts` subscribe）。进度事件高频期间快照频率可达 ~6 次/秒。
**缓解现状**：QueuePage 已使用 antd `Table virtual` 虚拟滚动（`QueuePage.tsx:150`），渲染层不是瓶颈；瓶颈在序列化与 IPC 传输量。台账 §1#2/#7 已列"分页与增量未开始"，本审计复核确认代码事实与台账一致，维持 P1。
**修复建议**：按台账既定方向实现快照分页/增量（revision + 变更集下发）；`engine_available` 的 `is_file()` 双检查（:388）可缓存为启动期一次性判定（见 F2）。

### F1 【P2】`TransferAsync` 复制循环每 1MB 调用一次 `GetDiskFreeSpaceEx`
**位置**：`src/OpenUnpack.Worker/WorkerOperations.cs:270`（写循环内逐块调用 `EnsureFreeSpace`）。
**对比**：同代码库 `ExtractCallback.GetStream` 已做 1 秒节流（`SevenZipEngine.cs:784-789`），此处未节流。10GB 复制 ≈ 1 万次 Win32 调用，估计开销 0.1–0.5 秒 + 元数据 IO 抖动；机械盘上影响可测量但不致命。
**修复建议**：移植 ExtractCallback 的 `TickCount64` 节流模式（1s/次），保持语义不变。

### F2 【P3】快照内每次调用 `is_file()` 探测引擎存在性
**位置**：`coordinator.rs:388`。Worker/引擎路径生命周期内不变，可在 `open()` 时缓存；仅在外部工具被替换时需要刷新——配合现有 `engineAvailable` 展示语义可接受，属微优化。

### 性能已检查未见问题项
- IO 缓冲：C# 复制/解压均 1MB 缓冲 + `ArrayPool` 复用 + `SequentialScan` 提示（`WorkerOperations.cs:225,248-258`）；管道消息上限 8MB、大清单自动 spill 到带哈希的清单文件（:302-372，含 128MB 上限与不可覆盖提交）。
- 并发：JoinSet 调度 + 按预算预留（`budget_reservations`）防并发worker 各自独占配额；暂停时 select 等待不空转（`coordinator.rs:1197-1209`）。
- 无持锁跨 await（逐点核查 62 处 await）。

---

## D. 异常处理与可靠性

### D-OK 检查结论：错误传播链完整、恢复语义严谨
- **错误码保真**：C# 异常 → 9 类结构化错误码（`WorkerOperations.cs:374-389`）→ 协议事件 → Rust `WorkerEvent` → UI 固定文案；「缺密码 / 密码错误或损坏 / 损坏 / 缺分卷 / 不支持 / 路径不安全 / 资源限制 / IO」用户可区分。
- **取消语义**：取消令牌贯穿 Rust→管道→C#→每个原生 COM 回调（`SetTotal/SetCompleted/Read/GetStream` 均检查）；Worker 侧原生编解码器可能忽略回调的情形有兜底（`WorkerHost.cs:106-109` 2 秒等待 + 父进程最终终止期限 + 未确认退出标记 `UNCONFIRMED_FILE` 与 PID+创建时间双重身份核对，`coordinator.rs:267-287`）。
- **崩溃恢复**：启动重放（:254-287）区分 `running→interrupted`、发布未决→`publishPending`、进程未确认退出→`stopping`；发布提交不可覆盖（`paths.rs:159-170`）；工作目录归属标记 + 清单哈希双重校验（`owned()`）。
- **资源清理**：Worker 管道 `CurrentUserOnly` + 30s 连接超时 + 父进程死亡监视自退；`OpenedArchive`/`OpenCallback`/`InputStream` 全链路 Dispose；COM 对象 `FinalReleaseComObject`。

### R1 【P3】Worker 进程顶层异常静默退出
**位置**：`src/OpenUnpack.Worker/Program.cs:103-106` — `catch { return 1; }`，无任何诊断输出。父进程侧只能看到非零退出码。
**修复建议**：向 stderr 写一行不含敏感信息的异常类型 + HResult（不堆栈、不消息体，防路径泄漏），便于现场诊断。
**落地（2026-10-12 修复轮）**：已实现 `Console.Error.WriteLine($"OpenUnpack.Worker startup failure: {ex.GetType().Name} (0x{ex.HResult:X8})")`。**适用域备注**：宿主 spawn Worker 时 stderr 已被设为 `Stdio::null()`（`crates/core/src/worker.rs`、`crates/infrastructure/src/worker/client.rs` 的 spawn 构造处），故该诊断行仅在**独立运行 Worker**（手工诊断、现场排查）时可见；宿主托管路径下仍以退出码为准。这是与"stdout 专供线协议、stderr 不进宿主管道"边界一致的取舍。

### R2 【P3】`unconfirmed_processes` 毒化锁返回伪条目 `(0,0)`
**位置**：`crates/core/src/worker.rs:86`。语义含混但后果保守（PID 0 永不"已退出"→任务停留 `stopping`，不会误判为可清理）。可加注释或改为返回 Err 让调用方显式处理。

---

## E. 工程化与依赖治理

### E1 【P2】前端无 lint 工具链
**证据**：`apps/desktop-ui/` 无 eslint 配置文件，`package.json` scripts 仅 dev/build/test/tauri/desktop，无 lint；无 eslint 依赖。tsconfig `strict: true` 且实测 0 处 `any`/`@ts-ignore`/非空断言滥用——当前干净靠自律，无机制防退化。
**修复建议**：接入 `eslint` + `typescript-eslint`（推荐 `recommended-type-checked`）+ `react-hooks` 插件，加入 CI。

### E2 【P2】CI 缺依赖漏洞扫描与产物签名
**位置**：`.github/workflows/windows.yml`。
**证据**：现有 CI 质量高（action 全 SHA pin、只读权限、锁定依赖、跑 verify.ps1 原生套件 + 构建三件套）。缺口：无 `cargo audit` / `npm audit` 步骤；安装器无代码签名（README 已声明本地构建无公开分发，属可接受现状，但发版前必须补）。
**修复建议**：CI 增加 `cargo audit --deny warnings` 与 `npm audit --omit=dev`（容忍度按当前基线设定）；发版流程规划 Authenticode 签名。

### 工程化已检查未见问题项
- TODO/FIXME/HACK 全仓 0 处；生产 `unwrap()` 0 处（167 处全在 `#[cfg(test)]`）；`panic!` 命中均为测试故意行为。
- 依赖精简无重复功能库；`Cargo.lock`/`package-lock.json` 锁定；CI 用 `cargo fetch --locked`。
- 测试资产：Rust 集成测试 8 个套件（含 `resource_recovery_regressions.rs` R1–R4 永久回归）、UI vitest 12 文件、C# 协议/单元测试、契约检查 v1/v2 用例。
- THIRD_PARTY_NOTICES 覆盖 7-Zip（LGPL/BSD/unRAR 限制说明）、.NET、Tauri NSIS 模板（含上游源码与 SHA-256 溯源）。

---

## F. 与状态台账交叉核对

| 台账条目 | 审计复核结果 |
|---|---|
| §1#2 快照不对等（速度恒空等） | 代码确认 `snapshot()` 不含速度字段，传输进度由 `transfers` 投影（:364-368）——与台账"部分已通过，速度事件另列"一致 |
| §1#5 取消持全局锁 | 确认已修复：`active_cancel` 均为临时守卫，锁外等待 |
| §1#6 infra 切除 core 依赖 | **部分不符**：源码已切除，清单声明残留（见 A2） |
| §1#7 大清单全驻内存 | 确认仍存在：snapshot 全量 clone（见 P1） |
| §1#9 协议兼容技术债 | 契约检查器覆盖面良好，null/整数范围差异属已知记录项 |

**新增于台账之外的发现**：A1（复制规模量化与 database_lease 分叉）、A2（清单残留）、A3、S2（协议 DTO Debug/Zeroize 缺失）、F1、E1、E2、R1、R2、S1、F2。

---

## G. 修复路线图

> **执行状态（2026-10-12 修复轮）**：立即项 1–4 全部完成；下一迭代 5–8 中 A1-中期已产出退役计划文档、F1/E1/E2 已完成；常规迭代 11（R1/R2/S1/F2）已完成；常规迭代 9（P1）已产出设计文档 `docs/plan-snapshot-delta-2026-10-12.md`（实施未开始，需独立回归预算）；常规迭代 10（A3）按报告自身建议推迟至双轨收敛后。逐项落地证据见各条目"落地"注与 `docs/status-ledger-2026-10-11.md` §15。

**立即（当前修复轮内，均 < 1 天）**
1. A2：删除 `crates/infrastructure/Cargo.toml:11` 残留依赖 → `cargo check --workspace` + infra 测试 → 修正台账 §1#6 表述。
2. A1-短期：对齐两份 `database_lease`（以 core 284 行版为准）；给 core 侧 vault.rs / worker.rs 加双写警示横幅。
3. S2：协议 DTO（`CandidateItem` / `WorkerRequest` / `WorkerEvent` / `PasswordInput`）实现脱敏 Debug 与 Drop Zeroize。
4. G1：将本报告登记进台账证据源（第 6 节）。

**下一迭代**
5. A1-中期：制定 core→新分层切换与 core 物理删除计划（台账 #1 的"另列"项排期）。
6. F1：TransferAsync 自由空间检查节流（移植 ExtractCallback 模式）。
7. E1：接入 ESLint + react-hooks，纳入 CI。
8. E2：CI 加 cargo audit / npm audit。

**常规迭代**
9. P1：快照分页/增量协议（台账 #7 既定方向）。
10. A3：Coordinator 子模块拆分（在 A1 收敛后）。
11. R1/R2/S1/F2：Worker stderr 诊断、毒化锁语义、license 下载入哈希清单、引擎探测缓存。

---

## 附录：审计统计

| 指标 | 数值 |
|---|---|
| 审计代码量 | Rust ≈21,600 行 · TS/TSX 3,151 行 · C# 2,744 行 · 脚本 ≈1,080 行 |
| 人工精读文件 | 24 个（含全部安全关键路径：paths/vault/worker/SevenZipEngine/WorkerHost/WorkerOperations/clipboard/repository/coordinator/protocol 热点） |
| 生产代码 unwrap() | 0（测试内 167 处不计） |
| TODO/FIXME | 0 |
| 硬编码密钥 | 0（测试夹具除外） |
| 前端 any/@ts-ignore | 0 |
| P0/P1/P2/P3 | 0 / 3 / 6 / 4 |
