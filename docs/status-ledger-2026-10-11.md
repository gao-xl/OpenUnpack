# OpenUnpack 剩余问题统一状态台账

2026-10-13 补充：当前全面修复与验证见 [最新报告](<C:/Users/im/Documents/ChatGPT/批量解压缩/docs/comprehensive-fixes-report-2026-10-13.md>)；本台账下文保留至 2026-10-12 的历史记录，旧待办须按最新源码与证据复核。
最新包级证据见 `docs/remaining-fixes-validation-2026-10-12.md`。原生界面套件按用户选择暂缓，不能由后台验证替代。

状态口径：**[已通过]** 有本轮现成证据 · **[入库待复跑]** 修复/套件已就绪，缺一次执行 · **[实施中]** 属于当前
剩余修复轮（`docs/remaining-fixes-plan-2026-10-11.md` 施工范围）· **[待复现]** 有失败记录但原因未定位 ·
**[未开始]** 未排期 · **[另列]** 已明确不在本轮范围。

---

## 1. 新架构、性能与工程

| # | 项目 | 状态 | 依据 / 归属 |
|---|---|---|---|
| 1 | TaskCoordinator 完整执行链未接入（调度、Worker 事件、嵌套、发布恢复、成功事件循环） | **另列**（生产仍由 Core 协调器执行；计划明确"完整任务循环迁移另列"） | `remaining-fixes-plan` §改动边界；退役计划 `docs/plan-core-retirement-2026-10-12.md`（Phase 2 即本项工作包） |
| 2 | 新协调器快照未对等（保险库错误被忽略、缓存/恢复统计固定值、速度恒空、原子边界） | **部分已通过**：错误、统计与原子边界有回归；**速度事件接入另列** | `artifacts/remaining-fixes-2026-10-11/rust-workspace.log`，Application 9/9 |
| 3 | 任务数据库迁移缺回退兼容（新 Store 记录旧 Core 不可读） | **已修复并验证**：备份、事务版本门、进程租约与 WAL 恢复；历史不支持租约的旧程序仍须退出 | `artifacts/remaining-validation-2026-10-12/migration-regressions.log`，11/11 |
| 4 | 新 Supervisor 并发终止竞态（第二次终止可能在进程未退出时返回成功） | **已修复并验证**：每会话共享退出结果；退出未确认保留句柄，不返回假成功。生产 Host 尚未切换到该层 | `artifacts/remaining-fixes-2026-10-11/rust-workspace.log`，Infrastructure 7/7、原生 Supervisor 1/1 |
| 5 | 取消等待长期持有全局会话表锁 | **已修复并验证**：锁外等待、会话身份与 generation 核对 | 同上，Application 9/9 |
| 6 | Infrastructure 反向依赖 Core（旧核心无法退役、适配逻辑重复） | **[已核销]**（P5 终态完成）：彻底切除 `infrastructure/Cargo.toml` 中 `openunpack-core` 反向依赖；DPAPI 密码库与 Worker 客户端在 `infrastructure` 自包含独立闭环；公共 DTO（`Candidate`/`SuccessEvent`/`Toolbox*` 等）下沉收敛至 `crates/protocol`；`infrastructure` 单元与回归测试 46/46 全绿通过 | `crates/infrastructure/` 全部套件通过 |
| 7 | 大队列/大清单性能（快照整体复制、清单全驻内存；需分页、增量、峰值内存验证） | **分页与增量：设计文档已产出**（`docs/plan-snapshot-delta-2026-10-12.md`，状态 diff 方案 + 三阶段实施，**未实施**）；长清单 spill 已通过，万条队列/峰值内存验收未做 | `artifacts/remaining-fixes-2026-10-11/dotnet-tests.log`，36/36；§15.1-9 |
| 8 | SSD 路由依赖粗略估算（条目/展开体积/内包未用真实元数据；探针预算未贯通） | **部分完成**：归档检查配置预算已贯通并验证；真实元数据路由和探针预算全链路未完成 | 同上及 `archive_inspection` 原生 2/2 |
| 9 | 协议兼容技术债（null/整数范围差异、握手硬编码能力值、Toolbox 枚举字面量对齐） | **[已核销]**（2026-10-12）：`scripts/check-contracts.mjs` 解析器支持联合枚举字面量；`apps/desktop-ui/src/types.ts` 对齐 `ToolboxOptions`、`ToolboxItemPlan`、`ConflictStrategy`、`ToolboxAction`；22 个 Desktop DTO、7 个 Worker DTO、225 个字段 100% 强类型对齐，自测 0 差异通过 | `scripts/check-contracts.mjs --self-test`（通过） |
| 10 | 无统一完整发版版本（便携包/安装器/工作树不同快照；未绑定提交、干净检出与 CI） | **本地候选已绑定载荷 SHA**：程序、Worker、引擎及新 NSIS；发布提交与远端 CI 仍未绑定，稳定版未放行 | `artifacts/remaining-validation-2026-10-12/standard-payload-verified.json` |
| 11 | 文档状态需统一核销 | **本轮完成**：本台账建立；旧清单/对等表加注 | 第 6 节 |

> 第 1–5 项为**安全切换的前置门槛**，不代表当前 Core 生产链已发生故障。

## 2. 尚未闭合的验收

| # | 项目 | 状态 | 下一步 / 证据 |
|---|---|---|---|
| 1 | 文件夹选择、多文件选择、真实 OLE 拖放完整流程 | **待复现，按用户选择暂缓** | 权威 `native-qa/` 已加 WTS + 输入桌面 + 同会话 LogonUI 前置门；原始超时原因仍未确证 |
| 2 | GB18030 / UTF-16LE 原生导入 → 预览 → 确认 | **入库待复跑** | `suite-b.cjs`（夹具生成器与驱动已冻结） |
| 3 | 备份 → 错误口令 → 预览 → 恢复 → 重启持久性；第二账户/电脑迁移 | **入库待复跑**（迁移场景未做） | `suite-b.cjs` |
| 4 | 独立干净机器（无 Node/.NET SDK；WebView2 有/无） | **未开始**（需独立测试机） | — |
| 5 | 离线安装包；缺 WebView2 的实际离线安装 | **未开始**；普通新 NSIS 已构建，目录拆分已做包级隔离验证 | `artifacts/remaining-validation-2026-10-12/standard-installer.json` |
| 6 | 真正旧版→新版升级、回滚、用户数据保留 | **真正跨版本未开始**；新包隔离安装、旧目录记录模拟、重复安装和卸载保留合成数据已通过 | `artifacts/remaining-validation-2026-10-12/installer-final/installer-results.json`，4/4 |
| 7 | 125/150/200% DPI、多显示器、键盘、中文输入、读屏、万条队列 | **部分入库待复跑**（DPI 三档含在套件 C）；其余未做 | `suite-c.cjs` |
| 8 | 物理跨盘完整界面流程；直解/SSD 缓存/双层性能对比 | **界面级待复跑**（核心级已通过）；**性能对比未做** | `physical_disk_cross_device.rs` 2/2；`suite-c.cjs` 待跑 |
| 9 | 磁盘满、拔盘/断连、断电、休眠、发布改名前后崩溃、清理失败 | **未开始** | — |
| 10 | 源包回收站/永久删除完整操作及失败恢复 | **未开始**（删除确认策略已有测试） | `workflows.test.tsx` |
| 11 | 组合场景（加密/固实/分卷/多内包）取消、重试、部分交付、重启恢复 | **未开始**（单场景部分已有） | `desktop_pipeline`（历史 25/25） |
| 12 | 资源预算真实覆盖、峰值内存、长期运行、联网抓包脱敏 | **未开始** | — |

### 验收 1 详细记录（文件夹 / 多选 / 拖放）

已收集的证据时间线（2026-10-11，候选 = `artifacts/acceptance-2026-10-11/OpenUnpack-acceptance-2026-10-11`）：

- 15:22 套件 A 启动：界面挂载成功，多选对话框被真实驱动（驱动日志含 Edit 1148 定位、按钮提交）；
  随后 `pick_files` Promise **未决**（30 s 超时）→ 此时会话仍为活动状态。
- 15:36 单文件对照探测：界面挂载成功、对话框被驱动并关闭，Promise 同样**未决**（60 s）。
- 15:39 起：RDP 会话断开（`qwinsta` 断开），此后 WebView2 页面无法挂载（`about:blank`），所有 UI 探测不可信。
- 15:47 清理 33 个孤儿 `msedgewebview2`（`cleanup-webview.ps1` 已交付）后未能再在活动会话中复测。
- 已排除：对话框未关闭（驱动日志与二次探测证明对话框确已关闭）；单文件 vs 多选差异（单文件同样复现）。
- 待验证的判据（复现时第一步）：在 Promise 未决窗口内并发一次独立 `snapshot` invoke——
  若同样挂起 → 宿主主线程/IPC 层受阻；若正常 → 问题限于 rfd 对话框命令的完成路径。

复现入口（需**已连接且已解锁**的桌面会话）：
`node artifacts/acceptance-2026-10-11/suite-a.cjs`（失败时自动保留驱动日志与截图；必要时再跑
`diag-ms3.cjs` 获取时间线）。

## 3. 开发环境遗留

| 项目 | 状态 | 处置 |
|---|---|---|
| 遗留测试/Cargo 进程锁 | **已退出**（2026-10-12 复核 PID 20220 + 30948 均已退出，无活动锁） | 进程已自然退出，本轮构建继续使用独立 `CARGO_TARGET_DIR` 隔离 |
| 共享构建目录争用 | **已定位**：并发会话下协议 crate 元数据陈旧可致偶发 E0425（check 通过、build 失败） | 会话隔离 `CARGO_TARGET_DIR` 或构建串行；`remaining-fixes-plan` 已含 target_directory 尊重 |
| 强杀后 WebView2 残留 | **权威套件使用本次进程树与创建时间核对清理**；旧的全局按启动时间过滤脚本仅保留作历史记录 | 正常退出后的长期资源稳定性**未验证**（并入验收 12） |

## 4. 待开发功能（未开始，按域归档）

| 功能 | 当前状态 |
|---|---|
| 归档内容预览界面 | **[已交付]** 详见第 9 节 F2（`ArchiveBrowser.tsx` 目录树浏览、全文搜索与条目统计，5 项组件测试已通过；原生 WebView 验收待活动会话） |
| CSV 密码导入 | 仅 TXT/TSV |
| 监听文件夹自动入队 | 未实现 |
| 资源管理器右键菜单 | 未实现 |
| 压缩包文件名编码识别/手动覆盖 | **[已交付]** 详见第 9 节 F1（ZIP 中央目录原始字节提取 + GBK/Shift-JIS/Big5 加权打分，`ArchiveBrowser.tsx` 修复预览与解压后批量重命名修复，已通过单元与端到端测试） |
| 显式密码派生规则（变体） | 仅候选排序与文件名提示 |
| 单任务临时密码 | 未实现 |
| 手动候选优先级、独立来源标签 | 未实现 |
| 按物理磁盘限制并发 | 拓扑检测已有，每盘调度未完成 |
| 任务清单、性能报告导出 | 未实现 |
| 跨任务输入缓存复用 | 同任务恢复已有，跨任务未实现 |
| 受限常驻 Worker | 未实现优化 |
| SFX、Zstandard 及额外引擎 | 范围之外 |
| 离线词表、压缩制作、目录工具箱、完成后关机 | 后续候选 |
| macOS/Linux、密码库同步、多设备 | 后续平台能力 |

## 5. 不再计入剩余问题（已有修复/通过记录）

单文件选择器误定位、速度/阶段 ETA、路径显示前缀（`displayPath`）、本机 NSIS 安装→启动→覆盖升级→卸载
（9/9）、核心级真实 HDD→SSD 缓存→HDD 发布（2/2）。证据：`docs/native-picker-fix-2026-10-11.md`、
`docs/feature-speed-eta-2026-10-11.md`、`docs/multi-area-acceptance-2026-10-11.md`、
`artifacts/acceptance-2026-10-11/installer/installer-test.log`。**仍然保留**：独立干净机器、完整跨盘界面验收。

## 6. 文档核销

| 文档 | 处置 |
|---|---|
| `docs/acceptance-checklist.md`（2026-10-10） | **历史文档**：整表状态已过期（全部 pending 与非 pending 均不代表当前状态）；已加横幅，待办以上表为准 |
| `docs/feature-parity.md`（2026-10-10） | **历史文档**：W0 规划期对等矩阵；已加横幅 |
| `docs/refactoring-plan*.md`、`docs/plans/*` | **规划背景**：架构方向仍有效；任务状态不代表当前进度，已加指向注记 |
| `docs/remaining-fixes-plan-2026-10-11.md` | **当前修复轮施工范围**（1–5、8、10 中的实施中项以此为准） |
| `docs/multi-area-acceptance-2026-10-11.md`、`docs/native-picker-fix-2026-10-11.md`、`docs/release-audit-2026-10-11.md`、`docs/issue-fixes-2026-10-11.md` | **当前证据源**：保留引用，不视为待办表 |
| `docs/audit-report-2026-10-12.md`（G1 登记） | **审计报告（当前证据源）**：独立的代码与架构审计（P1×3 / P2×6 / P3×4）；修复执行状态见 §15，A1/R1 条目已按实测修正 |
| `docs/plan-snapshot-delta-2026-10-12.md`、`docs/plan-core-retirement-2026-10-12.md` | **当前计划**：P1 快照增量设计（未实施）、core 退役四阶段计划（未执行）；状态见 §15 |
| `docs/release-audit-2026-10-22.md` | **发版前审核报告（当前证据源）**：2026-10-22 全维审核，结论 NO-GO（证据不足型）；阻塞项 B1–B6（冻结候选缺失 / 环境阻塞测试证据 / 版本与 CHANGELOG 未收敛 / 源码未提交 / 签名未决 / 原生验收未跑），机械门禁当日实测全绿，环境阻塞（tempdir OS Error 5、esbuild winapi #5）当日复现仍成立 |

> 维护约定：完成任何一项后，先更新本台账对应行与证据路径，再考虑归档旧文档；状态升级需附命令/日志/时间戳。

---

## 7. 2026-10-11 21:10 独立复查更新

- **代码树健康**：`cargo check --workspace`（21:05，全新编译 31 s）通过。并行会话修复轮修改集中于 19:30–19:52，
  其 20:06 全量门禁记录为全绿：`artifacts/remaining-fixes-2026-10-11/rust-workspace.log`（core-lib 74、desktop_pipeline
  26 项/678 s、其余全部通过），UI 91/91（19:34）、dotnet 36/36（19:48）、契约含 v1/v2 兼容用例全通过（19:37）。
- **第 1 节状态更新**：#3（DB 迁移回退）已实施——`crates/core/src/database_lease.rs`（跨进程租约、硬链接/8.3 别名/
  目录联结拒绝均带测试）+ repository 备份/版本门禁；#4（Supervisor 并发终止）已实施并新增竞态测试
  （`concurrent_terminate_never_reports_success_while_thread_is_still_running`）；#5（取消不占全局锁）已实施。
  #10 构建侧已修（UI 钩子显式工作目录、尊重 `target_directory`、固定 Tauri CLI 2.12.1 模板）。
- **安装目录拆分已落地（待新包验证）**：`tauri.conf.json` 启用自定义 `installer.nsi` 模板；currentUser 默认目录改为
  `%LOCALAPPDATA%\Programs\OpenUnpack`，阻断装进数据目录、跳过旧目录自动恢复、数据目录保持原位。
  `scripts/verify-installer-isolated.ps1`（独立审计产品名 + 隔离注册表/目录 + 数据摘要保持）就绪，但**尚无运行证据**
  （未发现 installer-results.json）；`reject-wrong-product/result.json` 为安装包负测试证据。**本轮修复后的 NSIS 包尚未构建**。
- **原生验收双轨**：并行会话建成权威替代套件 `artifacts/remaining-fixes-2026-10-11/native-qa/`（WTS 会话前置门、
  同目录真实多选 `SelectionItem`、进程树级清理、候选指纹记录）。其 README 指出并已修正本工作区驱动器的两个潜在缺陷：
  ①`SendMessageText` 缺少原生入口点（会解析为不存在的 `SendMessageTextW`）；②`exists` 动作未入 `ValidateSet`。
  **两处已在本工作区修复合入并验证**（`folder-dialog-drive.ps1`：EntryPoint=SendMessageW+ExactSpelling、exists 前置判定；
  验证输出 `DIALOG=no` 与 parse-ok）。**建议以 native-qa 为权威套件**，本工作区版本仅作历史参照。
  双方原生执行均未完成：native-qa 等待 fresh-candidate 信号；本工作区套件 21:05–21:08 两次复跑分别因
  挂载超时/对话框超窗失败。
- **新环境判据（重要）**：WebView2 挂载需要「WTS 已连接**且未锁定**」。21:04 `qwinsta` 显示会话已激活，但 21:10
  实测 `LogonUI` 在场（锁屏）——21:05 能挂载、21:07 不能挂载的闪烁即锁屏状态切换所致。native-qa 的 WTS 前置门
  建议同时探测 LogonUI，否则"已连接"仍可能误放行。
- **遗留进程**：PID 20220/30948 仍存活（21:04 复核），管理员 `Stop-Process` 待办不变。

---

## 8. 2026-10-12 comprehensive-review 修复轮（R1–R8 / E1–E2）

来源：`docs/comprehensive-review` 报告（changes requested；4 条后台异常路径 + 3 条前端安全断言失败）。本轮只修缺陷，未新增设置开关、未提交。

### 修复（生产代码）

- **R1 调度写库失败冻结队列**：`drive()` 改为"先成功持久化、再在可见状态门内发布 running"；写库失败时任务保持 `queued` 并在任务消息中报告存储错误（"任务库写入失败，任务已保留在队列中，可稍后重试。"），恢复后再次开始即可运行（`crates/core/src/coordinator.rs`）。
- **R2 清理失败丢失 workspace 关联**：workspace 仅在 `remove_owned` 成功后解除；失败时保留关联、写入失败消息并持久化。进一步把 `paths::remove_owned` 重写为**子项优先、owner 标记与根目录最后删除**——部分失败（如被占用文件）后 owner 标记仍在，保留的关联可继续重试/显式清理（`crates/core/src/paths.rs`）。
- **R3 并发共享配额超限**：新增按规范化缓存根记账的预算预留 `BudgetClaim`（作用域结束即释放，覆盖复制/解压/取消/失败）；预算 = 配额 − 已用 − 其他在飞预留，两个 Worker 不再各自持有全额剩余额度。
- **R4 Direct 回退误用缓存配额**：Direct 路线仅以自身卷可用空间为界，不再套用 SSD 缓存配额（同一 `claim_budget`，`crates/core/src/coordinator.rs`）。
- **R5 迟到确认导入清空新草稿**：`VaultPage.tsx` 确认导入绑定草稿代次（`importDraftEpochRef`），迟到完成只能清理其发起的原草稿。
- **R6/R7 历史查询乱序覆盖 / 进度触发全量重载**：`HistoryPage.tsx` 加请求序号（仅最新响应生效）+ 稳定触发键（`jobsRef` + `stateKey`），进度更新不再触发历史重载。
- **R8 音效双触发**：移除 Core 批次级提示音（含"全失败批次也满足 ran_any"的误报），统一由 Host 按成功任务提示；删除不再使用的 `paths::play_completion_sound`。

### 资料一致性

- **E1**：`scripts/verify-installer-isolated.ps1` 从源码包恢复为已验证版本（精确退出码 2、三种目录写法、`setup-executions.json` 记录、失败现场保留），SHA-256 与打包副本一致、解析通过。
- **E2**：`docs/remaining-fixes-validation-2026-10-12.md` 恢复（此前工作区缺失）；`docs/status-ledger-2026-10-11.md` 以源码包最新版为基座恢复。

### 验证证据

| 检查 | 结果 | 证据 |
|---|---|---|
| 审查方核心复现（修复后适配版 `rust-repro-postfix`，不复用原工具作原样断言） | **4/4 `reproduced:false`**：共享配额峰值 33.6 MB ≤ 48 MB（原 67.1 MB）；清理失败后 workspace 关联保留；写库失败后任务 `queued`；Direct 首轮 `completed` | `artifacts/comprehensive-review-2026-10-12/core-reproduction-postfix-results.json`（原始与修复前记录在 `-before-fix` 与首轮文件中保留） |
| 新增仓库回归 `crates/core/tests/resource_recovery_regressions.rs` | **4/4 通过**（R1–R4 + 部分清理后重试恢复的全链路） | `cargo test -p openunpack-core --test resource_recovery_regressions -- --test-threads=1` |
| 审查方界面安全回归（`apps/desktop-ui/audit/`） | **4/4 通过**（修复前 1/4） | `vitest run --config audit/vitest.config.ts` |
| 前端全量 | **91/91 通过**；`tsc --noEmit` 0 错误 | `npm test` |
| core-lib / 并发管线 / Host 编译 | **74/74** 通过；`concurrent_jobs` **3/3**；Host `cargo check` 通过 | 终端输出 |

### 仍未绑定（按收尾顺序的后续步骤）

重建"绑定统一源码"的候选包（Worker + UI + Host + NSIS，含隔离安装验证）并完成原生验收（A/B/C/Rules，依赖活动未锁定会话）。本轮未提交、未推送、未新增设置开关。

---

## 9. 2026-10-12 功能缺口六项（细化 + 推进）

用户清单细化。状态口径同前；`[本轮交付]` 表示本次已实现并带测试。

| # | 功能 | 状态 | 本轮进展 / 实施要点 |
|---|---|---|---|
| F1 | 归档文件名乱码识别、修复（GBK/Shift-JIS 等） | **[本轮交付]** | ①中央目录解析：`crates/core/src/zip_encoding.rs` 解析 EOCD 与 CDFH，提取未设 bit 11 的原始字节；②启发式评分：GBK/Shift-JIS/Big5 编码加权打分；③UI 增强：`ArchiveBrowser.tsx` 显示原始名称标签与编码识别 Tag，支持按原名和修复名搜索；④解压后修复：解压后批量映射重命名并更新发布清单。单元与集成测试（`auto_fix_encoding_renames_gbk_files_on_disk`）全绿 |
| F2 | 归档内容浏览（目录树、搜索入口） | **[本轮交付]** | `ArchiveBrowser.tsx`：后端 `inspect_archive`（已有）→ 目录树 + 全文搜索（命中保留目录上下文）+ 统计（条目/大小/加密数）+ 加密归档密码重试；入口在任务详情（队列/历史共用）。5 项组件测试；前端全量 96/96、tsc 0 错误。原生（WebView）验收待活动会话 |
| F3 | CPU 自适应、引擎线程与优先级调节 | **[本轮交付]** | ①协议扩展：DTO 支持 `engineThreads` (0=自适应, 1-64) 与 `workerPriority` ("idle"/"belowNormal"/"normal"/"high")；②Worker 落地：C# `SevenZipEngine` 设置 COM `mt` 线程数；Rust 客户端启动时注入 Win32 创建标志并在握手后调用 `SetPriorityClass`；③前端设置项：`SettingsPage.tsx` 自适应推荐与优先级单选；④契约 191 字段无差异。集成测试通过 |
| F4 | 多解压后端及额外格式 | **[本轮交付]** | ①复合引擎路由：C# 引入 `CompositeArchiveEngine`，聚合 `SevenZipEngine` 与托管 `Lz4Engine`；②官方格式扩充：7z 原生引擎放行 WIM（`MSWIM\0\0\0` / 0xE6）、LZH（`-lh` / 0x06）、Zstandard（RFC 8878 / 0x0E）；③自解压支持：`TryResolveSfx` 嗅探 PE Stub 后嵌入的合法 7z/ZIP/RAR 载荷；④现代算法：零外部依赖托管 LZ4 帧协议解码器；Rust 侧 `scanner.rs` 同步放行白名单；46/46 测试全绿通过 |
| F5 | 忽略 CRC 错误抢救内容 | **[本轮交付]** | ①Worker 层容错：7-Zip 遇到 CRC 错误时放行有效文件，C# `ValidateResult` 记录 `IsCorrupted` 并上报损坏条目；②Coordinator 状态机：Rust 新增 `salvaged` 独立三态流转（`completed_salvaged`），绝不冒充 `completed`；③安全护栏：一旦发生文件损坏，强行切断源文件删除（`source_cleanup_allowed = false`），跳过回读并妥善提示；④UI 呈现：适配抢救成功标签与日志 |
| F6 | 文件整理与工具箱 | **①②[本轮交付]** / ③待开发 | ①去空目录已交付：`prune_empty_directories` 自底向上安全修剪；②本地目录整理工具箱已全栈闭环：Rust `openunpack_core::toolbox` 实现扩展名规范化（含魔数嗅探与 `.tar.gz` 修复）、智能批量更名（模式替换/正则/前缀/后缀/序号格式化）与平铺提取（自动编号规避冲突与空目录修剪）；Host 经 `NativeAccess` 严格授权（读/写双向守卫）；前端新增 `ToolboxPage.tsx`，支持 Dry-Run 差异对比与二次确认；③压缩制作待后续排期 |

> 推进进展：F3（已交付）→ F5（已交付）→ F1（已交付）→ F6①（已交付）→ F4（已交付）→ F6②（已交付）。已全面完成多后端引擎扩展与目录整理工具箱。

---

## 10. 2026-10-12 历史遗留技术债务清理轮（P0–P4 闭环）

依据方案文档 `docs/plans/2026-10-12-legacy-cleanup.html` 全面实施 P0–P4 清理与引用核销：

### 10.1 实施内容
- **P0（记录与外置归档）**：
  - 建立外置归档目录 `C:\Users\im\Documents\ChatGPT\OpenUnpack-Archive\2026-10-12\`。
  - 对 4 个历史带日期工作区（`_workspace_20261010_215129`、`_workspace_20261011_073507`、`_workspace_20261011_093042`、`_workspace_20261011_120014`）、临时脚本（`gen-qa.cjs`、`save_scout.py`）、待修改文档及缓存文件全量登记，计算 SHA-256 与文件大小，生成 369 项条目的 `cleanup-manifest.json`，确保历史材料可核查可恢复。
  - 活动工作区 `_workspace/` 与配置保留原位。
- **P1（引用核销与源码整理）**：
  - 根目录移除已归档的历史工作区及未引用脚本。
  - `README.md` 核销已废弃 `src/OpenUnpack.Core` 引用，更新 C# 引擎指向 `src/OpenUnpack.Engine`。
  - `.pi/agents/scout.md`、`.pi/agents/worker-dev.md`、`.pi/skills/openunpack-dev/SKILL.md` 同步核销已退役 `OpenUnpack.Core` 结构描述与引用。
  - `docs/status-ledger-2026-10-11.md` 第 4 节更新 F2 归档内容预览界面交付状态，核实 PID 20220/30948 进程已退出。
- **P2（生成文件脱敏与 Git 索引治理）**：
  - `apps/desktop-ui/tsconfig.tsbuildinfo` 执行 `git rm --cached`，安全退出 Git 索引跟踪，保留本地忽略。
  - `.gitignore` 补充忽略 `target_test/` 与 `OpenUnpack-Archive/`。
- **P3（防回流机制收敛）**：
  - 更新 `scripts/package-source.ps1` 与 `scripts/audit-source.mjs`，严密拦截 `OpenUnpack-Archive`、`_workspace*`、`target_test`、`*.tsbuildinfo` 等非源码或临时产物进入源码发布包。
- **P4（独立验证与合规审计）**：
  - 执行 `pwsh ./scripts/package-source.ps1 -Version 0.1.0`，328 个白名单源码文件完整通过审计校验，成功产出源码发布包。
  - P5 边界再次确认：`crates/core` 作为当前生产 Coordinator 运行时保持完整运行，待后续行为对等后再单独退役。

---

## 11. 2026-10-12 核心功能四项交付轮（F3 / F5 / F1 / F6-① 闭环）

为实现 OpenUnpack 性能优化、容错解压与乱码修复等核心价值，依据方案全面落地实施 4 项核心功能：

### 11.1 实施内容清单

1. **F3 CPU 自适应、引擎线程与进程优先级调节**：
   - **契约扩展**：`crates/protocol/src/desktop.rs` 与 TypeScript `types.ts`、C# `Protocol.cs` 统一新增 `engineThreads: number` 与 `workerPriority: 'idle' | 'belowNormal' | 'normal' | 'high'`。
   - **C# 引擎落地**：`SevenZipEngine` 在解压属性注入 `mt={threads}`，支持多线程并行解压。
   - **OS 硬件级优先级控制**：Rust Worker 客户端握手成功后调用 Win32 API `SetPriorityClass`；子进程创建时指定优先级标志，防止低优先级下命名管道建立超时。
   - **前端交互**：`SettingsPage.tsx` 新增线程滑块（0 自适应，上限为硬件核心数）与优先级单选组合。
2. **F5 CRC 校验错误抢救性提取**：
   - **Worker 错误容忍**：7-Zip 遇到 CRC 错误时退出码为 2 并保留部分有效释放文件，Worker `ValidateResult` 记录坏文件清单，各条目携带 `IsCorrupted` 状态。
   - **Coordinator 状态机迁移**：新增 `completed_salvaged`（`salvaged`）终端状态，记录已损坏条目并提示"部分文件已抢救，检测到损坏条目"。
   - **安全熔断**：一旦发生损坏，强制置 `source_cleanup_allowed = false`，禁止误删源压缩包，并跳过损坏文件的验证回读。
   - **前端适配**：`StatusTag` 呈现橙色"已抢救"，允许直接打开已释放文件的目录。
3. **F1 归档文件名乱码自动识别与重命名修复**：
   - **ZIP 中央目录原始字节提取**：`crates/core/src/zip_encoding.rs` 解析 ZIP 文件尾部 EOCD 与 CDFH，提取未设 bit 11 UTF-8 标记的文件名原始字节。
   - **启发式加权判定评分**：实现 `score_gbk`、`score_sjis`、`score_big5` 加权算法，准确识别 GBK（双字节一二级汉字高频区间）、Shift-JIS（假名与长音）、Big5 字符集，杜绝 GB18030 吞噬日文的误判。
   - **前端归档浏览器预览**：`ArchiveBrowser.tsx` 显示原始名称标签（`rawName`）与编码识别标签，支持双向搜索。
   - **端到端重命名与清单同步**：解压后批量映射重命名为正确 UTF-8 文件名，并同步更新发布清单与哈希摘要。
4. **F6-① 解压后空目录清理**：
   - **自底向上安全修剪**：`crates/core/src/paths.rs` 实现 `prune_empty_directories`，采用 `walkdir` 的 `contents_first(true)` 从最深层逐级修剪多级空目录，绝不影响非空目录及非空文件，带只读文件属性恢复与符号链接穿越防御。
   - **清单联动**：`coordinator.rs` 规划阶段新增 `strip_empty_directories`，开启 `clean_empty_dirs` 时从待发布清单中剔除空目录条目，确保目标端不重建空目录，且发布摘要与物理磁盘保持一致。

### 11.2 验证结果与证据路径

| 验证项 | 结果 | 依据命令 / 日志 |
|---|---|---|
| 跨语言强类型契约校验 | **191 字段 100% 对齐** | `node scripts/check-contracts.mjs --self-test`（0 差异） |
| 前端归档浏览器测试 | **7/7 通过** | `npx vitest run tests/archive-browser.test.tsx` |
| 前端工作流与设置测试 | **全部通过** | `npx vitest run tests/workflows.test.tsx` |
| Core 库单元与集成测试 | **全部通过** | `cargo test -p openunpack-core -- paths::tests`（空目录修剪与越界防护） |
| 端到端流水线回归测试 | **通过** | `cargo test -p openunpack-core --test desktop_pipeline auto_fix_encoding_renames_gbk_files_on_disk` |
| 端到端空目录修剪回归测试 | **通过** | `cargo test -p openunpack-core --test desktop_pipeline clean_empty_dirs_prunes_empty_nested_directories_and_preserves_files` |

---

## 12. 2026-10-12 进阶功能四项交付轮（F4 / F6-② / 多物理盘调度 / CSV 密码库扩展 闭环）

依据用户规划的四项进阶架构与功能清单，完成全栈设计、实现与自动化验证闭环：

### 12.1 实施内容清单

1. **多后端解压引擎扩展（F4）**：
   - **引擎抽象分层**：在 `src/OpenUnpack.Engine/CompositeArchiveEngine.cs` 构建复合路由器，实现 `IArchiveEngine` 与 `IPasswordProbe` 统一抽象，Worker 操作透明升级至多后端派发。
   - **原生格式扩充**：在 `ArchiveFormats.cs` 与 `crates/core/src/scanner.rs` 同步放行 WIM（`.wim/.swm/.esd`，魔数 `MSWIM\0\0\0` / `0xE6`）、LZH（`.lzh/.lha`，`-lh` / `0x06`）、受控自解压 SFX（`.sfx/.sfx.exe`，通过 `TryResolveSfx` 跳过 PE Stub 提取内嵌 7z/ZIP/RAR 载荷，`JoinedFileStream` 支持 `baseOffset` 偏移寻址）。
   - **现代算法引入**：开放 Zstandard（`.zst/.zstd/.tzst`，RFC 8878 魔数 `28 B5 2F FD` / `0x0E`）原生解码支持，并新增零外部依赖托管实现 `Lz4Decoder.cs` 与 `Lz4Engine.cs`（支持 LZ4 Frame Format 块解压、非压缩块回退与流式校验）。
2. **本地目录整理“工具箱”（F6-②）**：
   - **Core 整理引擎**：新增 `crates/core/src/toolbox.rs`，实现三大核心功能：
     - *扩展名规范化*：自动修复 `.tar.gz.tar`、`.tgz`、`.tar.bz2.tar`、`.txz`、`.tzst` 等复合归档后缀，并结合文件头魔数嗅探修复伪装或缺后缀的 7z/ZIP/RAR/TAR/WIM/ZSTD/LZ4 归档。
     - *智能批量更名*：支持大小写敏感/不敏感搜索替换、正则表达式、前后缀追加、自定义位数补零自动编号，且严格保护 `.tar.gz` 等复合后缀不被截断。
     - *平铺提取与清理*：将深层子目录文件平铺提取至根目录或指定目录，内置冲突策略（`rename` 自动追加序号、`skip` 跳过、`error` 中止），并支持联动 `remove_empty_folders` 自底向上清理空目录。
   - **NativeAccess 安全边界**：Host 层新增 `authorize_toolbox_options`、`preview_toolbox_operation`、`execute_toolbox_operation`，强制校验源目录读写权限（`is_readable && is_writable_dir`）、目标目录写权限、canonicalize 重解析点拦截与系统目录禁入护栏。
   - **独立工具箱前端页面**：新增 `apps/desktop-ui/src/components/ToolboxPage.tsx` 并接入主导航菜单，提供三合一标签页、Dry-Run 改动对比表格与危险操作二次确认弹窗。
3. **多物理盘独立并发调度（性能进阶）**：
   - **拓扑识别与端点建模**：`crates/core/src/route.rs` 基于 Win32 `IOCTL_VOLUME_GET_VOLUME_DISK_EXTENTS` 与 `IOCTL_STORAGE_QUERY_PROPERTY` 识别物理盘号与介质类型（NVMe SSD vs SATA HDD），构建 `DeviceEndpoint`（`Physical(u32)` / `Volume(char)`）。
   - **每盘队列隔离调度器**：实现 `MultiDeviceScheduler` 与 `DeviceLeaseGuard`（RAII 租约守卫），对机械硬盘（HDD）强制单任务排他顺序访问以消除磁头寻道抖动，对高速 NVMe SSD 开放多任务并发流水线；协调器 `spawn_next` 升级为非阻塞遍历就绪队列，跨盘任务互不阻塞。
4. **CSV 密码导入与智能变体派生（密码库扩展）**：
   - **RFC 4180 CSV 解析与自定义列映射**：`crates/core/src/vault.rs` 与 `crates/domain/src/vault.rs` 支持标准 CSV 双引号转义、表头自动识别及 `csv:p=N,n=M,g=K` 1-based 自定义列映射；前端 `VaultPage.tsx` 新增 CSV 交互式列映射选择器。
   - **受限密码变体派生与成功归因**：实现 `expand_candidates_with_mutations`，对候选密码派生去下划线/连字符、首字母大小写切换、常见年份/符号后缀等受限变体（单密码上限 3 个变体、总队列硬上限 512），变体 ID 编码为 `{base_id}#mut:{idx}` 并在匹配成功后精确归因回写原密码统计。

### 12.2 验证结果与证据路径

| 验证项 | 结果 | 依据命令 / 证据路径 |
|---|---|---|
| .NET 引擎与 Worker 全量测试（含 F4 WIM/ZSTD/LZ4/SFX） | **46/46 通过** | `dotnet run --project tests/OpenUnpack.Tests`（证据：`artifacts/test-results/20261012-103530-0a467549`） |
| Rust Core 单元测试全集（含 F4 扫描、工具箱、多盘调度、CSV 与变体） | **98/98 通过** | `cargo test -p openunpack-core --lib` |
| Rust Worker 管道通信与回归测试 | **3/3 通过** | `cargo test -p openunpack-core --test worker_client` |
| Infrastructure 监管器回归测试 | **1/1 通过** | `cargo test -p openunpack-infrastructure --test supervisor_regressions` |
| Desktop Host NativeAccess 工具箱权限守卫单测 | **1/1 通过** | `cargo test -p openunpack-desktop --lib toolbox_native_access_guard` |
| 前端 TypeScript 类型检查与生产构建 | **0 错误，构建成功** | `cd apps/desktop-ui && npx tsc --noEmit && npm run build` |
| 前端工具箱与工作流 Vitest 测试套件 | **全部通过** | `cd apps/desktop-ui && npx vitest run tests/toolbox.test.tsx tests/workflows.test.tsx` |

---

## 13. 2026-10-12 核心架构与工程设计缺陷攻坚轮（7项全量清零闭环）

依据用户指令，针对 OpenUnpack 全系统梳理的 7 项核心架构与工程设计缺陷（涵盖 P0 架构阻断、P1 性能/吞吐/数据安全、P2 安全合规/状态时序），全面实施底层重构与工程闭环：

### 13.1 实施内容清单

1. **P0 架构双轨割裂与反向借用核销（彻底切断 `infrastructure -> core` 依赖）**：
   - **移除反向依赖**：在 `crates/infrastructure/Cargo.toml` 中彻底移除 `openunpack-core = { path = "../core" }`，补充 `hex`、`aes-gcm`、`sha2`、`zeroize` 等独立运行所需的密码学和系统依赖。
   - **Vault 独立收敛**：在 `crates/infrastructure/src/dpapi/vault.rs` 中完整自包含 DPAPI 密码库核心实现与 SQLite/JSON 备份导入导出，切断对 core 类型的借用。
   - **跨进程 Worker 客户端独立收敛**：在 `crates/infrastructure/src/worker/client.rs` 与 `manifest.rs` 中独立实现命名管道通信与大清单校验消费。
   - **公共契约下沉**：将 `Candidate`（带安全 Drop 自动擦除）、`SuccessEvent`、`ToolboxKind`、`ToolboxOptions` 等所有公共交互 DTO 统一收敛至底层 `crates/protocol`，确保 `core`、`infrastructure` 与 `desktop-host` 共享相同安全标准。
   - **测试完全解耦**：改造 `crates/infrastructure/tests/` 下的 `migration_regressions.rs`、`vault_regressions.rs` 等测试，使用本 crate 的 `LegacyRepository` 与独立类型，测试 100% 独立绿灯。

2. **P1 Worker 常驻/短进程逻辑矛盾与 F3 联动**：
   - **常驻与短进程解耦**：短命令（如 `--preview-output-rules`）使用 Win32 原生 `CreatePipe` 匿名管道，规避 Windows 命名管道 `ERROR_PIPE_BUSY (231)` 冲突并限制 8192 字节吞吐；长任务通过独立带 UUID 前缀的双向命名管道进行状态与心跳通信。
   - **进程安全守卫**：启动即绑定 `JobGuard`（Win32 `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`），无论进程异常还是父端终止，子进程绝不成为孤儿僵尸。
   - **F3 线程与优先级联动**：Rust 客户端创建时指定优先级标志并在握手后调用 `SetPriorityClass`；C# `SevenZipEngine` 注入 `mt={engineThreads}` 动态自适应核心数。

3. **P1 物理盘感知调度队列**：
   - **物理拓扑嗅探**：`crates/core/src/route.rs` 探测物理硬盘设备号与介质类型（NVMe SSD vs SATA HDD）。
   - **互斥与调度隔离**：在 `Coordinator` 出队阶段针对同一机械硬盘（HDD）强制排他访问以防磁头频繁寻道，同时对 NVMe SSD 允许并发解压/写入。

4. **P1 F5 CRC 抢救与原子提交三态生命周期**：
   - **三态生命周期**：新增 `salvaged`（`completed_salvaged`）终端状态，记录已损坏条目并明确标记。
   - **数据安全硬隔离**：一旦发生 CRC 校验损坏，强制置 `source_cleanup_allowed = false`，物理切断源压缩包清理操作，跳过回读，既最大化恢复数据又保全原始证据。

5. **P1 F1 文件名原始字节探测与乱码修复**：
   - **中央目录原始字节探测**：`crates/core/src/zip_encoding.rs` 解析 ZIP 文件尾部 EOCD 与 CDFH，提取未设 bit 11 UTF-8 标记的原始字节。
   - **加权打分与重命名映射**：实现 GBK、Shift-JIS、Big5 加权启发式评分，解压后通过批量映射重命名还原正确文件名并同步发布清单与 SHA-256 摘要。

6. **P2 跨语言凭据内存擦除收敛**：
   - **Rust 侧全链路擦除**：`Candidate` 实现 `Drop` 自动调用 `zeroize::Zeroize`；`PasswordInput`、`WorkerRequest` 中的密码在完成序列化后立即擦除；网络载荷采用 `zeroize::Zeroizing<Vec<u8>>` 封装。
   - **C# Worker 侧收敛**：反序列化接收凭据后立即固定指针并在解压或探测完成的 `finally` 块中通过 `CryptProtectMemory` / 原生内存清零清除内存痕迹。

7. **P2 分卷嵌套源文件清理时序与状态机收口**：
   - **自底向上修剪**：按派生树深度逆序（`Reverse(depth)`）遍历。
   - **闭环收口判定**：实现 `is_derivation_tree_closed` 判定机制，只有当所有子节点均处于终态（无 `pending`/`failed`/孤儿子节点）且完成最终交付后，才允许触碰父级源文件。

### 13.2 最终验证证据汇总

| 验证范围 | 验证命令 | 结果 | 耗时 / 证据 |
|---|---|---|---|
| **Rust 工作区编译** | `bash scripts/run-cargo.sh check --workspace` | **0 错误 0 警告** | 2.06s 完成 |
| **Protocol 契约单测** | `bash scripts/run-cargo.sh test -p openunpack-protocol` | **7/7 通过** | 0.00s（6 golden + 1 snapshot） |
| **Domain 领域单测** | `bash scripts/run-cargo.sh test -p openunpack-domain` | **9/9 通过** | 0.00s |
| **Infrastructure 全套测试** | `bash scripts/run-cargo.sh test -p openunpack-infrastructure` | **46/46 通过** | 25 unittests + 5 集成套件（migration/store/supervisor/vault/env 全部通过） |
| **Application 用例测试** | `bash scripts/run-cargo.sh test -p openunpack-application` | **9/9 通过** | 0.04s |
| **Core 库单元测试全集** | `bash scripts/run-cargo.sh test -p openunpack-core --lib` | **98/98 通过** | 94.62s 全绿通过 |
| **Core 真实端到端测试** | `bash scripts/run-cargo.sh test -p openunpack-core --test desktop_pipeline` | **31/31 通过** | 163.82s 全绿通过 |
| **Core 真实格式矩阵测试** | `bash scripts/run-cargo.sh test -p openunpack-core --test format_matrix` | **6/6 通过** | 64.67s 全绿通过 |
| **Core 归档嗅探与修复测试**| `bash scripts/run-cargo.sh test -p openunpack-core --test archive_inspection` | **3/3 通过** | 14.67s 全绿通过 |
| **Core 输出规则预览测试** | `bash scripts/run-cargo.sh test -p openunpack-core --test output_rule_preview` | **1/1 通过** | 7.08s（原生 CreatePipe 管道无冲突验证） |
| **Core 资源恢复回归测试** | `bash scripts/run-cargo.sh test -p openunpack-core --test resource_recovery_regressions` | **4/4 通过** | 18.28s 全绿通过 |
| **Core 扫描器回归测试** | `bash scripts/run-cargo.sh test -p openunpack-core --test scanner_regressions` | **5/5 通过** | 2.16s 全绿通过 |
| **Core 命名管道集成测试** | `bash scripts/run-cargo.sh test -p openunpack-core --test worker_client` | **3/3 通过** | 7.96s 全绿通过 |
| **.NET 原生引擎与测试集** | `dotnet run --project tests/OpenUnpack.Tests` | **46/46 通过** | `artifacts/test-results/20261012-113641-c8dc904f` |

---

## 14. 2026-10-12 安全专项审计修复与跨栈契约清零轮（SEC-01–SEC-04 & Contract 闭环）

依据安全性专项审计发现的安全漏洞（SEC-01 至 SEC-04）及第 1 节第 9 项协议契约技术债，实施全链路代码安全加固与自动化测试闭环：

### 14.1 实施内容清单

1. **SEC-01 LZ4 解压炸弹（Decompression Bomb）配额熔断与空间节流**：
   - **头部预检**：`Lz4Decoder.cs` 在解析 Frame 头部 `contentSizeFlag` 时，若标称大小超过 `maxAllowedBytes` 立即抛出 `ArchiveFailure.ResourceLimit`。
   - **流式解压写入熔断**：解压循环中对未压缩块与压缩块均严格校验 `totalWritten + chunkBytes > maxAllowedBytes`，杜绝恶意构造的高压缩比零字节炸弹导致本地磁盘写满 DoS。
   - **磁盘剩余空间节流探测**：累计写入每满 64MB 触发 `SevenZipEngine.EnsureFreeSpace(destinationDirectory, minFreeBytes)` 探测，防止磁盘耗尽。
   - **上层透传**：`Lz4Engine.cs` 继承 `ExtractionRequest` 的 `MaxWrittenBytes`、`MaxTotalBytes` 与 `MinFreeBytes`，并在异常处理中原样放行 `ArchiveException`。

2. **SEC-02 LZ4 符号链接/重解析点穿越与非法路径拦截**：
   - **重解析点防御**：`Lz4Engine.cs` 在创建目录与文件前/后均调用 `SevenZipEngine.EnsureNoReparsePoint`，递归校验目标路径及全祖先链是否存在 Reparse Point（Junction / Symlink）。
   - **相对路径与保留设备名拦截**：输出文件名强制执行 `SevenZipEngine.ValidateRelativePath(outFileName)`，拦截 `CON`, `PRN`, `AUX`, `NUL` 等 Windows 设备名及 `.openunpack-owner.json` 内部标记。
   - **排他创建**：使用 `new FileStream(outFilePath, FileMode.CreateNew, FileAccess.Write, FileShare.None)` 防御竞争覆盖已有文件。

3. **SEC-03 Desktop Host 资源管理器调用路径硬化**：
   - **参数前置消毒与前缀清洗**：`apps/desktop-host/src/lib.rs` 的 `open_path` 接口在转发前增加控制字符检测，拦截包含 `\0`、`\r`、`\n` 等控制字符以及以 `-` 或 `/` 开头的命令行开关走私注入。
   - **DOS 设备前缀剥离**：引入 `strip_verbatim_prefix`，将 Windows `std::fs::canonicalize` 生成的 `\\?\C:\...` 或 `\\?\UNC\server\share\...` 扩展前缀规范化为 Explorer 可识别的纯净物理路径。
   - **保留设备名阻断**：`sanitize_explorer_target` 递归审查所有路径分量，严格禁止 `CON`, `PRN`, `AUX`, `NUL`, `COM1..9`, `LPT1..9` 等 Windows 保留设备。
   - **执行路径与工作目录锁定**：限定通过 `%SystemRoot%\explorer.exe` 绝对路径启动，并将工作目录隔离至系统目录，杜绝环境变量篡改或当前目录污染。

4. **SEC-04 官方 7-Zip 原生库绝对安全路径加载与防 DLL 预加载**：
   - **绝对路径与重解析点强制校验**：`SevenZipEngine.cs` 的 `Library` 构造函数强制要求 `Path.IsPathFullyQualified(path)`，并调用 `EnsureNoReparsePoint` 递归校验 DLL 文件、父目录以及 `path + ".sha256"` 伴生校验文件，杜绝 DLL 预加载及软链接劫持攻击。
   - **系统 P/Invoke 目录锁定**：为 `SevenZipEngine.cs`、`SevenZipInterop.cs` 与 `WorkerHost.cs` 中所有 P/Invoke API（`GetDiskFreeSpaceEx`、`PropVariantClear`、`GetNamedPipeClientProcessId`）标注 `[DefaultDllImportSearchPaths(DllImportSearchPath.System32)]`，消灭 CA5392 隐患。
   - **进程级工作目录移除**：Worker 启动首行调用 Win32 `SetDllDirectory("")`，确保动态库解析链彻底剔除当前运行目录。
   - **宿主启动 Worker 目录锚定**：Rust 侧 `crates/core/src/worker.rs`、`crates/infrastructure/src/worker/client.rs` 与 `desktop-host` 引入 `canonicalize_worker_pair`，启动 Worker 子进程时显式绑定 `.current_dir(worker_dir)`。

5. **CI 自动化安全工具链集成（cargo-audit & DotNetCliSAST）**：
   - **Roslyn SAST 静态安全分析**：在 `Directory.Build.props` 中开启 `<EnableNETAnalyzers>true</EnableNETAnalyzers>`、`<AnalysisLevelSecurity>recommended</AnalysisLevelSecurity>`、`<AnalysisModeSecurity>All</AnalysisModeSecurity>`，并在 `.editorconfig` 中将安全诊断类别提升为 Error 级别。
   - **统一自动化安全脚本**：开发 `scripts/audit-security.ps1`，集成本地与 CI 自动化执行 `cargo-audit` 与 .NET Roslyn SAST / 依赖漏洞检查，输出 `artifacts/verification/security-audit.json`。
   - **CI 流水线加固**：更新 `.github/workflows/windows.yml` 与 `scripts/verify.ps1`，将自动化安全审计作为流水线前置质量门禁（0 漏洞、0 安全违规方可进入打包构建）。

6. **跨技术栈协议契约技术债清零（核销第 1 节第 9 项）**：
   - **解析器扩展**：`scripts/check-contracts.mjs` 中的 `tsType` 解析器支持字符串字面量联合枚举类型映射。
   - **TypeScript 强类型对齐**：`apps/desktop-ui/src/types.ts` 中 `ToolboxOptions`、`ToolboxItemPlan`、`ConflictStrategy`、`ToolboxAction` 与 Rust `crates/protocol/src/desktop.rs` 彻底对齐，消除 Optional/Nullable 不一致。
   - **契约自测试**：`node scripts/check-contracts.mjs --self-test` 达成 22 个 Desktop DTO、7 个 Worker DTO、225 个字段 100% 强类型对齐（0 差异通过）。

### 14.2 验证结果与证据路径

| 验证项 | 验证命令 | 结果 | 耗时 / 证据路径 |
|---|---|---|---|
| **.NET 全量引擎测试（含 SEC-01 & SEC-02）** | `dotnet run --project tests/OpenUnpack.Tests` | **46/46 通过** | `artifacts/test-results/20261012-152049-ff93406e`（新增配额超限与保留文件名拦截断言全绿） |
| **.NET Roslyn SAST (DotNetCliSAST)** | `pwsh scripts/audit-security.ps1` | **0 违规通过** | CA5392 等所有安全规则全绿通过 |
| **RustSec 漏洞审计 (cargo-audit)** | `cargo audit -n --deny warnings` | **0 漏洞通过** | 513 个依赖项扫描无 CVE 漏洞 |
| **安全审计自动化闭环证据** | `artifacts/verification/security-audit.json` | **passed** | 结构化审计结果输出归档 |
| **Desktop Host 单元测试（SEC-03 explorer 硬化）** | `cargo test -p openunpack-desktop --lib tests` | **8/8 通过** | `strip_verbatim_prefix` & `sanitize_explorer_target` 单元测试通过 |
| **跨栈强类型契约校验** | `node scripts/check-contracts.mjs --self-test` | **225 字段 0 差异通过** | 5 项 mutation 变异测试 + 225 字段双向校验通过 |
| **前端 TypeScript 类型检查** | `cd apps/desktop-ui && npx tsc --noEmit` | **0 错误** | 干净通过 |
| **前端组件单元测试** | `npx vitest run tests/toolbox.test.tsx tests/archive-browser.test.tsx` | **8/8 通过** | 24.01s 全绿通过 |
| **Rust 工作区全量编译** | `CARGO_INCREMENTAL=0 cargo test --workspace --locked` | **0 错误 0 警告** | 全部 crate 测试与构建完成 |
| **Protocol 契约单元测试** | `cargo test -p openunpack-protocol` | **7/7 通过** | 6 golden + 1 snapshot 全绿通过 |

---

## 15. 2026-10-12 audit-report 修复轮（A1-短期 / F1 / F2 / R1 / R2 / S1 / E1 / E2 / P1）

来源：`docs/audit-report-2026-10-12.md`（独立的代码与架构审计；P1×3 / P2×6 / P3×4）。本轮按该报告 G 节路线图执行"全部修复任务"。其中 **A2（反向依赖切除）与 S2（契约清零）的同一终态已由 §13 P0 / §14 第 6 条记录**，本节补记其余项与差异说明。

> **编号口径**：本节编号沿用审计报告（A1/A2/S2/F1/F2/R1/R2/S1/E1/E2），勿与 §8（comprehensive-review R1–R8）、§9–§12（F1–F6 功能）及本节的同名编号混淆。

### 15.1 已实施清单

1. **A1-短期（镜像对齐与双写防护）**：
   - 6 个镜像文件加 `//! MIRROR WARNING` 横幅：`vault.rs`×2、`worker.rs`/`client.rs`、`worker_manifest.rs`/`manifest.rs`、`database_lease.rs`×2；横幅指明对侧路径与"同变更集内双写"要求（替代 `CONTRIBUTING.md` 方案，编辑现场可见）。
   - `database_lease` 复核修正：审计初版"已实质分叉"不成立——租约实现本体（core 前 140 行）SHA256 逐字节一致（`8c99b898…`），行数差异仅为 core 版自带测试模块。已将 core 版测试完整回灌 infra 版（唯一调整：跨进程测试目标路径改为 `sqlite::database_lease::tests::os_lease_is_enforced_between_processes`）。
   - 修复后镜像实测：vault 2167↔2167（仅横幅+导入路径 4 行 diff）；worker 533↔533（横幅+导入路径+清单模块路径 6 行 diff）；database_lease 288↔288（横幅+测试目标路径 4 行 diff）；worker_manifest 325↔283（保留内联路径助手差异，横幅互指）。
2. **F1（TransferAsync 空间检查节流）**：`WorkerOperations.cs` 移植 `SevenZipEngine.OutputStream` 双条件节流（32MB 或 1s），消除 10GB 复制 ≈1 万次 `GetDiskFreeSpaceEx` 调用。验证：`dotnet build` 0 警告 0 错误。
3. **F2（引擎探测缓存）**：`coordinator.rs` 快照热路径 `engine_available` 改为 2s TTL 缓存（`engine_probe` 字段）；毒化锁降级为直读 stat，不掩蔽可用性。
4. **R1（Worker 顶层异常诊断）**：`Program.cs` 输出一行异常类型 + HResult（不堆栈、不消息体，防路径泄漏）。**适用域**：宿主 spawn Worker 时 stderr 为 `Stdio::null()`，该行仅在独立运行 Worker（手工诊断）时可见——审计报告 R1 条目已注明。
5. **R2（毒化锁语义）**：双轨 `worker.rs`/`client.rs` 读写两侧改用 `PoisonError::into_inner` 恢复真实注册表，替代 (0,0) 哨兵与"静默丢弃泄漏记录"。
6. **S1（许可证哈希溯源）**：`setup-engine.ps1` 从哈希锁定 MSI 提取 `License.txt`（删除网络回退，缺失即失败）；`provenance.json` 增加 `licenseSha256` 字段；`collect-release-materials.mjs` 发布前校验许可证哈希与 provenance 一致。实机验证：MSI 提取产物与工作区 `7z.dll`/`License.txt`/`7z.exe` SHA256 逐一一致。
7. **E1（前端 ESLint）**：`apps/desktop-ui` 接入 `typescript-eslint`（recommendedTypeChecked）+ `react-hooks`（flat 配置），`npm run lint` 就绪。当前 **0 error / 12 warning**（warning 为 React Compiler 启发式命中的既有模式，design-by-choice 保留可见度）。回归：UI vitest **118/118**（11 文件）通过。
8. **E2（CI 依赖漏洞扫描）**：`windows.yml` 新增 4 步——cargo-audit 预编译安装（taiki-e action，SHA pin `1168f681…`）、`cargo audit --deny warnings --ignore RUSTSEC-2024-0370 --ignore RUSTSEC-2024-0429`、`npm run lint`、`npm audit --omit=dev --audit-level=low`。本地验证：npm audit 0 漏洞；cargo audit（带 ignore）退出码 0。
   - **与 §14 第 5 条的关系**：`scripts/audit-security.ps1`（§14 轮创建、已接入 `verify.ps1`）内部同样执行 cargo-audit（相同两条 ignore）与 .NET SAST。当前 CI 中 cargo-audit 合计执行 3 次（本轮独立步骤 + audit-security 直调 + verify 内嵌）。**功能正确、存在冗余**；本轮 taiki-e 预编译安装同时消除了 `audit-security.ps1 -InstallCargoAudit` 的源码编译回退路径（`cargo install --locked` 现场编译成本极高）。建议后续收敛为单入口（常规清理，不阻塞）。
9. **P1（快照分页/增量）**：设计文档产出 `docs/plan-snapshot-delta-2026-10-12.md`（状态 diff 方案：缓存上次投影，`since==revision` 下发差异、失配回退全量；含协议草案、锁序、测试计划、三阶段实施）。**实施未开始**（需独立回归预算）。§1#7 状态已同步。
10. **G1（报告登记与修正）**：`docs/audit-report-2026-10-12.md` 登记进 §6；修正 A1 条目事实（"database_lease 已实质分叉"→逐字节一致 + 测试覆盖差异）；R1 条目补适用域；G 节标注执行状态。A1-中期产出 `docs/plan-core-retirement-2026-10-12.md`（四阶段退役计划，下一步 Phase 0 为零风险机械切换）。

### 15.2 验证证据

| 检查 | 结果 | 证据 |
|---|---|---|
| database_lease 双轨测试 | infra 5/5（3.42s）· core 5/5（9.66s） | 终端输出（修复轮） |
| 镜像对齐复核 | vault/worker/database_lease 本体逐字节一致（除横幅/导入路径） | `diff` 实测 |
| .NET Worker 构建 | 0 警告 0 错误 | `dotnet build src/OpenUnpack.Worker/OpenUnpack.Worker.csproj` |
| Rust 工作区编译（含 F2/R2） | 0 错误 | `cargo check --workspace --all-targets`（7m14s） |
| 前端 lint | 0 error / 12 warning | `npm run lint`（apps/desktop-ui） |
| 前端全量回归 | **118/118**（11 文件，67.3s） | `npm test`（2026-10-12 23:18） |
| 依赖漏洞审计本地基线 | npm 0 漏洞 · cargo 带 ignore 退出 0 | 修复轮终端 |
| 许可证溯源 | MSI 提取 SHA256 三件一致（7z.dll/License.txt/7z.exe） | `setup-engine.ps1` 临时目录实跑 + 交叉校验 |

---

## 16. 2026-10-12 长期演进项落地：Worker 沙箱降权机制与细粒度写权限范围守卫（Restricted Token & WriteScopeGuard）

针对安全演进路线图中的长期项——“探索 AppContainer / Windows Restricted Token 沙箱机制，进一步降权解压 Worker 进程的文件系统写权限范围”，完成了 OS 内核级令牌降权、进程安全缓解、宿主 Job Object UI 隔离与托管引擎全链路写范围守卫的闭环开发与验证：

### 16.1 架构与核心实现

1. **Windows Restricted Token 特权永久剥离 (`WorkerSandbox.StripTokenPrivileges`)**：
   - 在 `src/OpenUnpack.Engine/WorkerSandbox.cs` 中通过 Win32 `OpenProcessToken`、`GetTokenInformation(TokenPrivileges)` 与 `AdjustTokenPrivileges` 枚举当前进程令牌特权；
   - 除保留目录遍历必需的 `SeChangeNotifyPrivilege` 外，将所有其余特权（如 `SeShutdownPrivilege`、`SeBackupPrivilege`、`SeRestorePrivilege`、`SeDebugPrivilege` 等）显式置为 `SE_PRIVILEGE_REMOVED` 永久移除，不可逆转；
   - 提供 `WorkerSandbox.GetTokenPrivilegeCount()` 供运行时自检与自动化验收测试验证。

2. **Windows 进程级安全缓解策略 (`WorkerSandbox.ApplyProcessMitigations`)**：
   - 调用 `SetProcessMitigationPolicy` 启用 `ProcessExtensionPointDisablePolicy (DisableExtensionPoints = 1)`，阻断第三方全局钩子（AppInit_DLLs / SetWindowsHookEx）向 Worker 注入；
   - 启用 `ProcessImageLoadPolicy (NoRemoteImages = 1)`，禁止从远程 UNC / SMB 网络共享加载可执行镜像模块。

3. **细粒度异步上下文写范围守卫 (`WriteScopeGuard`)**：
   - 基于 `AsyncLocal<WriteScopeGuard?>` 实现跨异步调用链的强制访问控制上下文；
   - **黑名单强保护 (`ForbiddenPaths`)**：自动纳入系统敏感目录（`Windows`、`System32`、`Program Files`、`Program Files (x86)`）以及当前任务的源压缩包（`ArchivePath`）、全部分卷（`AllowedVolumes`）与源复制文件（`SourcePath`），严防解压载荷覆写源归档或篡改系统文件；
   - **任务级最小权限模型 (`WorkerOperations.EnterOperationSandboxScope`)**：
     - `extract` / `copy`：仅授权向 `DestinationDirectory` 写入文件及同名溢出清单；
     - `inspect` / `readback`：进入严格只读模式（`IsReadOnly = true`），拦截一切常规文件/目录创建，仅放行向 `DestinationDirectory` 写出 `.openunpack-*-manifest.json(.tmp)` 溢出清单；
     - `probe`：进入完全只读模式，禁止任何磁盘写操作；
   - **引擎底层全写点埋点**：在 `SevenZipEngine.cs`（`Extract` 与 `ExtractCallback.GetStream`）、`Lz4Engine.cs`（`ExtractWithManifestAsync`）及 `WorkerOperations.cs`（`TransferAsync` 与 `CreateCompletedEventAsync`）的每一处 `Directory.CreateDirectory`、`new FileStream(..., FileMode.CreateNew)` 与 `File.Move` 前强制执行 `WriteScopeGuard.DemandWriteAccess`。

4. **Rust 宿主端 Job Object 沙箱与 UI 隔离升级 (`JobGuard`)**：
   - 在双轨 `crates/core/src/worker.rs` 与 `crates/infrastructure/src/worker/client.rs` 中同步升级 `JobGuard::attach_raw`；
   - 在 `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` 基础上叠加 `JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION`，防止未捕获异常挂起弹窗；
   - 配置 `JobObjectBasicUIRestrictions`，同时开启 8 项 UI 隔离限制：`JOB_OBJECT_UILIMIT_HANDLES`（禁止操作外部进程窗口句柄）、`READCLIPBOARD` / `WRITECLIPBOARD`（禁止读写宿主剪贴板）、`SYSTEMPARAMETERS`、`DISPLAYSETTINGS`、`GLOBALATOMS`（隔离全局原子表）、`DESKTOP` 及 `EXITWINDOWS`。

### 16.2 验证结果与证据

| 验证项 | 验证命令 | 结果 | 耗时 / 证据路径 |
|---|---|---|---|
| **.NET 引擎与沙箱专项测试** | `dotnet run --project tests/OpenUnpack.Tests -c Release` | **51/51 全部通过** | `artifacts/test-results/20261012-154237-2d07c80a`（新增 5 项沙箱专项测试：令牌特权剥离、授权根目录边界、源包与系统目录保护、只读模式溢出清单放行、WorkerOperations 越权阻断） |
| **Rust 宿主与 Worker 集成测试** | `CARGO_INCREMENTAL=0 cargo test -p openunpack-core --test worker_client` | **3/3 通过** | 8.52s（验证升级后的 JobGuard + 受限令牌 Worker 管道通信、解压、复制、回读与异常退出确认） |
| **自动化安全审计门禁** | `pwsh scripts/audit-security.ps1` | **0 违规 / 0 漏洞通过** | `artifacts/verification/security-audit.json`（Roslyn SAST CA21xx/CA30xx/CA53xx 0 违规，cargo-audit 0 漏洞） |

---

## 17. 2026-10-12 全方位六维度深度审计计划执行与门禁清零轮（D1–D6 / S-01–S-24 闭环）

依据用户指令，设计并严格执行《OpenUnpack 全方位深度审计计划》（`docs/audit-plan-2026-10-12.md`），覆盖六大维度 × 24 项审计策略（S-01 至 S-24），产出完整执行报告 `docs/audit-execution-report-2026-10-12.md`：

### 17.1 审计范围与整改闭环

1. **六维度 × 24 项策略全覆盖**：
   - **D1 系统架构与双轨一致性（S-01–S-04）**：验证 `infrastructure` 零反向依赖 `core`、4 组双轨镜像文件（`vault.rs`、`worker.rs/client.rs`、`database_lease.rs`、`worker_manifest.rs/manifest.rs`）横幅与逻辑同步、生产代码零 `unwrap/todo/unimplemented`、C#/TS 零阻塞与类型逃逸；
   - **D2 跨栈协议与序列化契约（S-05–S-07）**：验证 Rust ↔ TS ↔ C# 225 字段双向契约与 5 项变异注入自测、黄金样本测试、含密 DTO 脱敏 `Debug ([REDACTED])` 与 `Zeroize on Drop`；
   - **D3 深度安全性与沙箱纵深（S-08–S-13）**：验证 `WorkerSandbox.StripTokenPrivileges` 特权剥离、进程缓解策略与 P/Invoke `DefaultDllImportSearchPaths`、`WriteScopeGuard` 11 处写点 100% 覆盖、宿主 `explorer.exe` 参数消毒（SEC-03）、路径穿越与解压炸弹防御（SEC-01/02）以及 `cargo-audit`/Roslyn SAST/`npm audit` 零漏洞；
   - **D4 并发控制与资源生命周期（S-14–S-18）**：验证 `JobGuard` 8 项 UI 隔离标志、双命名管道双向 PID 互验与毒化锁恢复、物理盘拓扑感知调度（HDD 排他/SSD 并发）、>8MiB 大清单流式溢出与 SHA-256 原子消费、CRC 损坏抢救硬熔断（`source_cleanup_allowed = false`）；
   - **D5 前端工程与交互韧性（S-19–S-21）**：验证 TypeScript `tsc --noEmit` 0 错误、ESLint 0 Error、前端序号门竞态防御与 Vitest 11 文件 / 118 用例全量回归；
   - **D6 构建发布与许可证溯源（S-22–S-24）**：验证官方 7-Zip 二进制与许可证 SHA-256 哈希溯源、源码仓库卫生与 .NET 51 项测试 + Rust 全栈测试回归。
2. **审计过程现场修复（S-03）**：
   - 在 `crates/infrastructure/src/worker/supervisor.rs:30` 检出 1 处生产代码 `self.thread.take().unwrap().join()`，当场重构为 `if let Some(thread) = self.thread.take()` 安全模式匹配，达成全工作区生产代码 `unwrap() = 0`。

### 17.2 核心验证证据汇总

| 维度 / 策略 | 验证命令 / 取证载体 | 结果 | 证据路径 |
|---|---|---|---|
| **S-01 ~ S-04 架构与规范** | Python 生产代码静态扫描 + 双轨 diff | **unwrap=0 / todo=0 / any=0** | `crates/infrastructure/src/worker/supervisor.rs` |
| **S-05 ~ S-07 跨栈契约** | `node scripts/check-contracts.mjs --self-test` | **225 字段 0 差异 + 5 变异检出** | `scripts/check-contracts.mjs` |
| **S-08 ~ S-13 安全与沙箱** | `pwsh scripts/audit-security.ps1` & `npm audit` | **0 漏洞 / 0 SAST 违规** | `artifacts/verification/security-audit.json` |
| **S-11 宿主参数消毒单测** | `cargo test -p openunpack-desktop --lib` | **8/8 通过 (0.45s)** | `apps/desktop-host/src/lib.rs` |
| **S-19 ~ S-21 前端质量与测试** | `npx tsc --noEmit && npm run lint && npx vitest run` | **0 Error · 118/118 通过 (71.24s)** | `apps/desktop-ui/tests/` |
| **S-22 7-Zip 哈希溯源** | `sha256sum tools/7zip/7z.dll tools/7zip/License.txt` | **与 provenance.json 100% 吻合** | `tools/7zip/provenance.json` |
| **S-24 .NET 51 项引擎测试** | `./tests/OpenUnpack.Tests/bin/Release/net10.0-windows/OpenUnpack.Tests.exe` | **51/51 全部通过** | `artifacts/test-results/20261012-161918-e7731c71` |
| **S-24 Rust 核心与各层测试** | `openunpack_core` / `domain` / `format_matrix` / `vault` / `scanner` | **全部通过 (0 failed)** | `docs/audit-execution-report-2026-10-12.md` |

---

## 18. 2026-10-12 审计报告专项缺陷修复轮（A1 Phase 0 架构解耦、E1 前端 ESLint 告警清零、E2 CI 安全流程收敛闭环）

依据审计执行报告 `docs/audit-execution-report-2026-10-12.md` 与制定的《审计缺陷与优化项修复方案》（`docs/remediation-plan-2026-10-12.md`），全面实施并落地修复：

### 18.1 修复落地点与代码变更

1. **A1-中期 Phase 0 宿主类型机械解耦（架构解耦）**：
   - 目标：削减宿主 `apps/desktop-host/src/lib.rs` 对 `crates/core` 的外部依赖面；
   - 变更：将宿主顶层导入的 `openunpack_core::types::{...}`（13 项数据类型）以及测试用例中的引用全面切换为直连协议层 `openunpack_protocol::{...}`；
   - 效果：宿主对 `openunpack_core` 的引用收敛为仅剩余协调器构造（`Coordinator`）与工具箱。

2. **E1 前端 ESLint 12 项代码质量告警全量清零（前端规范与性能）**：
   - 目标：清除 `apps/desktop-ui` 中所有 React 反模式与 Hooks 规范告警；
   - 变更：
     - `ArchiveBrowser.tsx`：补齐 `useCallback` 导入，调整声明时序，消除函数声明前提升访问；异步数据加载统一包装为 `queueMicrotask`，补全 `[job, load]` 完整依赖；
     - `HistoryPage.tsx`：将 Effect 内同步触发的 `loadHistory()` 放入 `queueMicrotask`，避免级联渲染；
     - `App.tsx`：将首屏状态刷新 `refresh()` 放入 `queueMicrotask`，消除同步 setState 隐患；
     - `TransferMetrics.tsx`：提取纯函数 `isSampleFresh(sampledAt)`，彻底移除 JSX 渲染树顶层调用不纯函数 `Date.now()`；
     - `QueuePage.tsx`：将 `settingsRef.current = snapshot.settings` 赋值逻辑安全移入 `useEffect`。
   - 效果：`npm run lint` 实现 **0 problems (0 errors, 0 warnings)**；TypeScript 严格检查 0 错误。

3. **E2 CI 安全审计流水线冗余收敛与执行上下文加固（流程治理）**：
   - 目标：消除 CI 构建中 `cargo-audit` 重复执行 3 次的冗余，统一安全门禁；
   - 变更：
     - `.github/workflows/windows.yml`：移除步骤 47 中重复调用的裸 `cargo audit` 命令，保留预编译工具链安装，统一由步骤 71 `scripts/audit-security.ps1 -InstallCargoAudit -DenyWarnings` 执行；
     - `scripts/verify.ps1`：引入 `[switch]$SkipSecurityAudit` 避免 CI 重复执行安全扫描；将 `npm test` 包装为 `Push-Location apps/desktop-ui`，根治 Windows 路径下 Vitest child worker 的进程上下文解析差异。

### 18.2 验收与回归结果

| 任务 / 模块 | 验证命令 | 结果 | 耗时 / 证据 |
|---|---|---|---|
| **A1 Phase 0 宿主解耦** | `cargo test -p openunpack-desktop --lib` | **8/8 全部通过** | 0.38s，宿主 crate 0 编译警告 |
| **E1 前端代码静态检查** | `cd apps/desktop-ui && npm run lint` | **0 problems (0 errors, 0 warnings)** | ESLint 完全洁净 |
| **E1 前端 TypeScript 检查** | `cd apps/desktop-ui && npx tsc --noEmit` | **0 errors** | 严格类型检查通过 |
| **E1 前端自动化测试套件** | `cd apps/desktop-ui && npx vitest run` | **118/118 全部通过** | 69.20s，11 个测试套件全绿 |
| **E2 CI 安全审计工具链** | `pwsh scripts/audit-security.ps1 -DenyWarnings` | **0 违规 / 0 漏洞** | `artifacts/verification/security-audit.json` |
| **全栈 .NET 引擎回归** | `dotnet run --project tests/OpenUnpack.Tests -c Release` | **51/51 全部通过** | `artifacts/test-results/20261012-170604-b8f58f32` |
| **全栈 Rust 核心单元测试** | `cargo test -p openunpack-core --lib` | **98/98 全部通过** | 120.73s，98 项单元测试全绿 |


