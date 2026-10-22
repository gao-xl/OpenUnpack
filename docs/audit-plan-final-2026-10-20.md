# OpenUnpack 最终全面审计计划

> 版本：v1.0（2026-10-20）
> 适用对象：`codex/open-unpack` 工作区当前候选，覆盖 Rust 7 crate + Tauri 宿主 + React 前端 + C# Worker + MCP 连接器 + 构建发布链
> 计划性质：**终审（Final Audit）**——不是从零重审，而是在既有 20+ 份报告与 18 轮整改之上做「差异审计 + 全量回归 + 发布准入判定」
> 预计投入：单线 9 人日 / 三轨并行 4.5 工作日

---

## 0. 计划摘要

| 项 | 内容 |
|----|------|
| 审计目标 | 对冻结候选给出一份**可复核、可回归、可据此放行或否决**的终审结论 |
| 覆盖维度 | 安全 / 代码质量与可维护性 / UI·UX / 功能完整性 / 性能与稳定性 |
| 审计对象 | `crates/{core,infrastructure,domain,application,protocol,local-api,cli}`、`apps/desktop-host`、`apps/desktop-ui`、`apps/mcp-connector`、`src/OpenUnpack.{Worker,Engine}`、`scripts/`、`.github/workflows` |
| 方法组合 | 机械门禁 + 威胁建模驱动的手工深审 + 场景矩阵与故障注入 + 五源对齐的 UI 走查 + 冻结候选上的负载与浸泡 |
| 产出 | ① 终审报告（分级问题清单）② 修复日志 ③ 证据包 ④ 台账更新 ⑤ GO / NO-GO 结论 |
| 判定基线 | P0 = 0；P1 = 0（或全部有书面缓解与补偿控制）；机械门禁 100% 绿；关键路径证据可复现 |

**本计划的三条原则**

1. **先机械、后人工**：能在 5 分钟内用工具判定的，绝不占用人工时间；机械门禁红着的时候不做深审。
2. **先信任边界、后代码整洁**：安全与数据一致性优先于命名与注释；UI 与性能放在功能稳定之后，避免在会变的实现上做无效测量。
3. **结论必须可复现**：每条问题附「位置 + 证据 + 复现步骤」，每个"已修复"附「验证命令 + 输出摘要 + 时间戳」。

---

## 1. 审计边界与基线

### 1.1 组件与风险面

| 组件 | 技术 | 规模（约） | 信任边界 | 关键资产 |
|------|------|-----------|----------|----------|
| `crates/core` | Rust | 16.1k 行 | 编排不可信归档与本地文件 | 输出目录、源文件、任务状态 |
| `crates/infrastructure` | Rust | 11.4k 行 | 子进程 / DPAPI / SQLite / 文件系统 | 密码库、Worker 进程、清单 |
| `crates/local-api` | Rust (axum) | 2.6k 行 | 本机 HTTP + WebSocket | API 令牌、作用域、事件流 |
| `apps/desktop-host` | Rust (Tauri) | 5.2k 行 | WebView2 ↔ 原生桥 | 路径授权、NativeAccess、设置 |
| `apps/desktop-ui` | React + TS | 6.8k 行 | 用户与桌面进程之间 | 密码输入、任务操作 |
| `apps/mcp-connector` | TS (stdio) | 0.5k 行 | 外部 Agent ↔ 本机 API | API 令牌、批次计划 |
| `src/OpenUnpack.Worker` | C#/.NET | — | 命名管道 ↔ 7-Zip 引擎 | 解压结果、清单 |
| 构建发布链 | PS1 / CI | 30+ 脚本 | 供应链 | 安装包、签名、SBOM |

### 1.2 范围

**纳入**：上述全部源码与脚本；`Cargo.lock`、`package-lock.json`、`.csproj` 依赖图；CI 工作流；原生 QA 套件；安装包与签名流程。

**不纳入**：7-Zip 本体源码审计（按第三方组件处理，只做版本/签名/已知漏洞核对）；外部网络渗透；法务合规审查；云服务（本项目无后端服务）。

### 1.3 可直接复用的既有资产（禁止重复劳动）

| 资产 | 路径 | 用法 |
|------|------|------|
| 上一轮全量审计 | `docs/audit-report-2026-10-20.md` | 0 P0 / 8 P1 / 13 P2 / 5 P3；作为**差异基线**，只审变更与未覆盖面 |
| 修复执行记录 | `docs/audit-fix-log-2026-10-20.md` | P1 8/8、P2 11/13 已修；未修 2 项需重新定级 |
| 状态台账 | `docs/status-ledger-2026-10-11.md` §8–§18 | 已闭环项清单，避免重复登记 |
| 依赖复核 | `docs/dependency-security-report-2026-10-17.md` | 供应链结论与缺口 |
| 契约/架构门禁 | `scripts/check-contracts.mjs`、`check-architecture.mjs` | 机械一致性（`check-mirrors.mjs` 已退役，见终审报告 QUAL-004） |
| 安全审计策略 | `scripts/audit-security.ps1`、`security-audit-policy.ps1` | cargo-audit + NuGet + npm，含豁免白名单 |
| 原生 QA 套件 | `artifacts/remaining-fixes-2026-10-11/native-qa/` | 原生对话框/拖放/多选驱动 |
| 负载测量 | `scripts/measure-load.ps1`（ssd/hdd/cpu/controls/snapshots/all） | 性能基线 |
| 发布校验 | `scripts/verify-release-signatures.ps1`、`test-release-validation.ps1`、`verify-installer-isolated.ps1` | 发布链 |

### 1.4 环境前置（不满足则结论自动降级为「有条件通过」）

| 前置 | 要求 | 不满足的后果 |
|------|------|--------------|
| 交互会话 | WTS 会话**已连接且未锁定**（`LogonUI` 不在场） | WebView2 静默不挂载，全部 UI/原生用例失效 |
| 冻结候选 | 构建产物复制到独立目录 + `SHA256SUMS.txt` + `BUILD-SOURCE-SHA256.txt` | 证据无法绑定版本 |
| 独占构建 | 同一时刻只允许一个 `cargo`；无并行会话写工作区 | 元数据陈旧型编译错误、锁竞争（`os error 5`） |
| 临时目录 | 修复 `tempfile::tempdir()` 的 `PermissionDenied(5)` | 约 30 条集成用例不可执行（当前实测阻塞） |
| 前端构建 | 解决 esbuild 读 `package.json` 的 `winapi #5` | 无法产出构建证据，仅能依赖 `tsc`/`lint` |
| 测试夹具盘 | 准备 SSD 与 HDD 两个物理盘路径 + 大文件/分卷夹具 | 跨盘路由、物理盘调度无法验证 |

> 当前实测：`tempdir` 与 `esbuild` 两项在本机被策略拦截（详见 `audit-fix-log-2026-10-20.md` 第 4 节），**必须换机或提权后再执行 Phase 3/5**。

---

## 2. 五个维度的检查要点

### 2.1 安全性

**目标**：证明「不可信归档 + 不受信本机调用方」两条攻击路径都无法越权、越界或泄露凭据。

| 子域 | 检查要点 | 方法 / 证据 | 通过判据 |
|------|----------|-------------|----------|
| A. 依赖与供应链 | ① Rust 漏洞与 unmaintained；② npm（prod + dev）；③ NuGet；④ 7-Zip 版本与签名；⑤ 锁文件漂移；⑥ 构建可复现 | `cargo audit --deny warnings`、`npm audit --omit=dev`、`dotnet list package --vulnerable`、`verify-release-signatures.ps1`、`git diff Cargo.lock` | 0 条可利用漏洞；豁免项必须有**可达性论证**；签名校验通过 |
| B. 鉴权与权限 | ① `scope → command` 映射是否全覆盖；② HTTP / WS 双通道是否同一闸门；③ 路径作用域（read/write roots）是否可绕过；④ 任务级 BOLA；⑤ 令牌生命周期与撤销 | 生成**权限矩阵**（scope × command），逐格构造请求；重点覆盖 `submit_input`、`add_archive`、`save_settings`、`recover_job` | 每个 scope 只能触达其映射命令；越权请求 100% 返回 403 |
| C. 敏感信息 | ① 密码是否进入 argv / 日志 / 崩溃转储 / 前端持久化；② 保险库加密与密钥来源；③ 日志脱敏；④ 剪贴板与导出；⑤ 远程模式传输加密 | 代码检索 + 进程命令行抓取 + `%LOCALAPPDATA%` 落盘检查 + 抓包（远程守护） | 明文密码不出现在 argv、磁盘、日志、URL；远程必须 TLS |
| D. 输入校验 | ① 归档条目路径（`..`、绝对路径、ADS、保留设备名、尾点/空格、超长、深嵌套）；② JSON 反序列化边界（超大字符串、未知字段、数值溢出）；③ 通配符/正则复杂度；④ 文件名编码（UTF-8/GBK/SJIS/Big5） | 构造恶意归档样本 + `validate_relative_path` 单测 + fuzz（可选 cargo-fuzz） | 全部恶意样本被拒绝或安全降级；无 panic |
| E. 注入与越权 | ① `cmd.exe` / `powershell` 参数拼接；② 7-Zip 参数注入；③ SQL 拼接；④ 路径拼接（字符串前缀 vs 组件比较）；⑤ URL scheme / 重定向 | 全仓模式扫描 + 手工 PoC + 回归用例 | 无 shell 二次解析面；数据只经 env/argv 单层传递 |

**安全审计的固定动作**

1. 画信任边界图（4 条：归档→Core、WebView→Host、Token→Local API、Worker→文件系统）。
2. 对每条边界列出「输入 → 校验点 → 消费点」，缺失校验点即为候选缺陷。
3. 每个候选缺陷写**最小 PoC**（可执行命令或测试用例），无 PoC 不入清单（可标"待验证"）。

### 2.2 代码质量与可维护性

| 检查要点 | 方法 | 通过判据 |
|----------|------|----------|
| 分层与依赖方向 | `check-architecture.mjs` + 人工核对 `core → infrastructure` 是否反向依赖 | 无循环依赖；domain 不被 core 依赖（现状：core 未依赖 domain） |
| 单文件规模 | `tokei` / `wc -l` 排序 | 生产文件 ≤ 300 行为目标；> 800 行列入重构清单（现状有多处） |
| 重复代码 | MIRROR 双轨人工核对（脚本已退役）+ 人工比对 | 双轨文件行为一致；同逻辑仅一处实现 |
| 命名与一致性 | 人工走查 + 术语表 | 同一概念（job/task、vault/password）全仓唯一叫法 |
| 错误处理 | `grep "unwrap()|expect("` 区分 `#[cfg(test)]`；错误码表 | 生产路径 0 处裸 unwrap；错误信息可操作 |
| 日志 | 级别使用、是否含敏感数据、是否带 requestId/jobId | 无密码/路径泄露；关键状态迁移有日志 |
| 死代码与开关 | `#[allow(dead_code)]` 清单、环境变量开关清单 | 每项有归属或删除计划（如 `--trust-all` 已改为显式状态） |
| 文档一致性 | README / CONTRIBUTING / 台账 / 报告交叉核对 | 命令与行为描述与实际一致 |

### 2.3 UI/UX

| 子域 | 检查要点 | 方法 | 通过判据 |
|------|----------|------|----------|
| 一致性 | Design Token 覆盖（颜色/间距/圆角/字号）；图标单一来源且非 emoji；深浅色一致；中英日文案完整 | 五源对齐：设计变量 / 元数据 / 设计截图 / 代码 / 渲染截图 | Token 外硬编码色值 = 0；三语无缺失键 |
| 响应式 | 1024 / 1280 / 1600 宽度；最小窗口；DPI 125%/150%/200%；超长路径与超长文件名 | 逐分辨率截图 + 手工缩放 | 无横向滚动条、无截断错位、无重叠 |
| 可访问性 | 键盘可达与焦点顺序；`aria-*`；对比度 ≥ 4.5:1；`prefers-reduced-motion`；屏幕阅读器语义 | `eslint-plugin-jsx-a11y` + axe-core（可用时）+ 纯键盘走查 | 无键盘陷阱；交互元素均有可读名称 |
| 交互反馈 | 加载/禁用/进度/取消四态；空态；危险操作二次确认；错误提示可操作；Toast 不堆叠 | 场景脚本逐条执行 | 每个异步动作都有可见反馈；失败不静默 |
| 状态一致性 | 快照刷新时弹窗/选中态/滚动位置是否丢失；语言切换后状态保持 | 手工 + `snapshot-cursor` 单测 | 无状态错乱、无"点了没反应" |

### 2.4 功能完整性

| 子域 | 检查要点 | 方法 | 通过判据 |
|------|----------|------|----------|
| 核心流程 | 添加 → 扫描 → 识别密码 → 解压 → 校验 → 发布 → 清理（含分卷、嵌套、加密、损坏、超大） | 场景矩阵逐格执行（见附录 C） | P0/P1 场景 100% 通过 |
| 边界与异常 | 磁盘满、权限拒绝、文件被占用、源被移动、分卷缺失、密码错误、取消/暂停/恢复 | 故障注入：ACL、文件句柄锁、稀疏文件占满、拔盘 | 失败路径有明确提示且可恢复 |
| 数据一致性 | 快照 revision 单调；增量协议正确；状态机合法迁移；历史分页；密码库并发写；设置原子落盘 | 表驱动状态机测试 + 并发脚本 + 断电模拟 | 无非法状态迁移；无丢失更新 |
| 引擎一致性 | ZIP 原生引擎 vs 7z CLI：排除/包含规则、清单、哈希结果一致 | 同一归档 + 同一规则跑双引擎对比 | `FileRecord` 集合与哈希完全一致 |
| 跨端一致性 | UI / Local API / CLI / MCP 对同一命令的语义一致 | 契约测试 + 三端脚本 | 行为差异为 0 或有文档化说明 |

### 2.5 性能与稳定性

| 子域 | 检查要点 | 方法 | 通过判据 |
|------|----------|------|----------|
| 渲染 | 万条队列、万节点归档树、每秒多次快照更新 | React Profiler + 手工压数据 | 单次更新 < 16 ms；无掉帧 |
| 加载 | 冷启动到可交互；10 万文件扫描；10 GB 归档检查 | 计时脚本 × 3 次取中位数 | 与基线相比回退 ≤ 10% |
| 资源 | 内存峰值、句柄数、线程数、缓存目录增长、WebView2 子进程 | Process Explorer / `Get-Process` 采样 | 长时运行无单调增长 |
| 吞吐 | SSD / HDD / 跨盘 / 分卷 / 小文件密集 | `measure-load.ps1 -Profile all` | 达基线；CPU 未打满成为瓶颈 |
| 崩溃点 | panic 路径、unsafe（183 处）、递归深度、锁顺序、OOM | `cargo test --release`、`load_profile`、24h soak | 0 崩溃；无死锁；无 OOM |

---

## 3. 执行顺序

```
Phase 0  准备与冻结            0.5d
   │
Phase 1  机械门禁（快失败）     0.5d  ── 任一红 → 先修，不进入后续
   │
Phase 2  安全深审（最高风险）   2.0d  ── 轨道 A
   │                                      ╲
Phase 3  功能与数据一致性       2.0d  ── 轨道 B（依赖 Phase 1 构建可用）
   │                                      ╱
Phase 4  UI/UX 走查             1.5d  ── 轨道 C（需交互会话）
   │
Phase 5  性能与稳定性           1.5d  ── 轨道 C（需独占机器，必须最后）
   │
Phase 6  汇总、复验、定级       1.0d
```

**为什么是这个顺序**

- **Phase 1 前置**：`fmt` / `clippy` / `audit` / `contracts` 是确定性信号，5 分钟出结果；带着红灯做人工审计会浪费判断力。
- **Phase 2 次之**：安全缺陷的修复可能改动结构（如本次 `local-api` 授权层重构），越早发现越省返工。
- **Phase 3 在安全之后**：功能用例需要稳定构建；且部分功能缺陷（如跨租户任务操作）本质是安全问题，放在一起判定。
- **Phase 4 UI 在功能之后**：UI 走查依赖可操作的真实流程，流程还在变时截图无意义。
- **Phase 5 性能最后**：负载与浸泡要求机器独占、候选冻结、无后台构建，且前面的修复会改变性能特征。

**并行建议**

| 时段 | 轨道 A（安全/静态） | 轨道 B（功能） | 轨道 C（UI/性能） |
|------|--------------------|----------------|-------------------|
| D1 | Phase 1 + 威胁建模 | 场景矩阵准备 | 设计 Token 盘点 |
| D2–D3 | Phase 2 深审 + PoC | Phase 3 核心流程 | 静态 UI 走查（可离线） |
| D4 | Phase 2 复核 | Phase 3 异常注入 | Phase 4 交互走查 |
| D5 | Phase 6 汇总 | Phase 6 汇总 | Phase 5 负载 + soak |

> UI 与性能共用交互会话，二者串行；安全与功能可完全并行（不同工具链）。

---

## 4. 工具与方法矩阵

| 维度 | 工具 | 具体命令 / 用法 | 产出 |
|------|------|-----------------|------|
| 依赖 | cargo-audit | `cargo audit --deny warnings`；`scripts/audit-security.ps1 -DenyWarnings` | 漏洞清单 + 豁免论证 |
| 依赖 | npm / NuGet | `npm audit --omit=dev`；`dotnet list package --vulnerable` | 同上 |
| 静态 | clippy | `cargo clippy --workspace --all-targets -- -D warnings` | 0 error |
| 静态 | rustfmt | `cargo fmt --all -- --check` | 0 差异 |
| 静态 | 自定义规则库 | `rg` 模式：`unwrap\(|expect\(|unsafe|Command::new|format!\(.*SELECT|starts_with\("` | 候选清单 |
| 契约 | 仓库脚本 | `node scripts/check-contracts.mjs`、`check-architecture.mjs` | 一致性报告 |
| 前端 | ESLint / tsc / vitest | `npm run lint`、`npx tsc -b`、`npm test` | 0 error + 用例通过 |
| 前端 | a11y | `eslint-plugin-jsx-a11y`；如可安装则 `@axe-core/react` | 严重项清单 |
| 原生 | QA 套件 | `artifacts/remaining-fixes-2026-10-11/native-qa/harness.cjs` + 各 dialog 脚本 | 截图 + 日志 |
| 性能 | 负载脚本 | `scripts/measure-load.ps1 -Profile all -Levels 1,2,3,5` | CSV + 对比曲线 |
| 性能 | 进程采样 | `Get-Process -Id … | Select WorkingSet,HandleCount,Threads` 定时采样 | 资源曲线 |
| 稳定性 | soak | 24h 循环任务 + 崩溃恢复注入 | 无崩溃/无泄漏结论 |
| 安全 | 手工 | STRIDE 四象限 + 权限矩阵脚本化 + 最小 PoC | PoC 用例 |
| 数据 | 故障注入 | `icacls` 拒权、独占句柄、`fsutil file createnew` 占盘、拔盘 | 异常路径证据 |

**建议新增（当前缺口）**

1. 权限矩阵自动化脚本：遍历 `command.rs::scopes()` 与命令清单，生成 scope × command 表并断言越权拒绝。
2. UI 截图基线：固定窗口尺寸 + 三语 + 深浅色，生成可 diff 的截图集（回归时自动比对）。
3. 性能基线入库：把 `measure-load.ps1` 结果按候选版本存档，形成趋势而非单点。

---

## 5. 问题清单格式

### 5.1 字段定义（唯一 schema）

| 字段 | 必填 | 说明 |
|------|------|------|
| `id` | 是 | `SEC-001` / `QUAL-003` / `UX-007` / `FUNC-012` / `PERF-004`（按维度前缀 + 三位序号） |
| `severity` | 是 | `P0` / `P1` / `P2` / `P3`（见 5.2） |
| `dimension` | 是 | 五维度之一 + 子域（如 `安全/鉴权`） |
| `title` | 是 | 一句话说明"哪里错了"，不写修复方案 |
| `location` | 是 | `文件:行号` 或 `组件/接口`；跨栈问题列主责位置 |
| `evidence` | 是 | 3–8 行原始代码 / 命令输出 / 截图路径 |
| `trigger` | 是 | 具体触发链（谁、以什么输入、走哪条路径） |
| `impact` | 是 | 对资产的实际影响（数据、权限、可用性） |
| `repro` | 是 | 可执行复现步骤或测试用例路径 |
| `fix` | 是 | 具体改法（代码片段优先） |
| `verification` | 否 | 修复后的验证命令与期望输出 |
| `status` | 是 | `OPEN` / `FIXED` / `VERIFIED` / `RISK-ACCEPTED` / `WONTFIX` |
| `owner` | 否 | 责任模块 |

### 5.2 严重度定义

| 等级 | 判定 | 典型例子 |
|------|------|----------|
| **P0** | 可远程无凭据利用；或导致用户数据损坏/丢失；或绕过全部授权 | 未鉴权的文件写入；解压覆盖用户原目录 |
| **P1** | 本地权限提升；凭据泄露；发布链阻断；核心功能不可用 | 令牌越权读写；密码明文落盘；CI 必红 |
| **P2** | 特定前置条件下的注入/绕过；功能死路；资源无上限；显著性能退化 | 钩子参数注入；取消后仍关机；扫描无上限 |
| **P3** | 健壮性、体验、可维护性改进 | 截断 panic 边缘场景；a11y 键盘不可达 |

**升降级规则**

- 需要"攻击者已具备同用户代码执行"才能成立 → 降一级，并在影响里写明前提。
- 涉及金钱、不可逆数据、跨用户数据 → 升一级。
- 仅影响测试/开发路径 → 最高 P3。
- 同一根因在多处出现 → 合并为一条，位置列全部受影响点。

### 5.3 状态机

```
OPEN ──修复──> FIXED ──验证通过──> VERIFIED
  │                │
  │                └──验证失败──> OPEN（附失败证据）
  ├──书面接受──> RISK-ACCEPTED（需记录补偿控制与复审日期）
  └──确认不修──> WONTFIX（需说明理由）
```

**去重规则**

1. 同根因、同修复 → 合并；`location` 列全部点。
2. 跨栈同一缺陷（如 Rust 与 UI 都未校验）→ 按**最大影响**定级，修复建议分栈给出。
3. 历史已登记项（台账 §8–§18）→ 只登记"复发"或"未修完"，标注 `REGRESSION` / `CARRY-OVER`。

### 5.4 输出模板

**Markdown（人读，报告主体）**

```markdown
### SEC-003 【P1】WebSocket 通道绕过作用域校验

- **位置**：`crates/local-api/src/server.rs:1018`、`server/events.rs:265`
- **证据**：
  ```rust
  if !client.scopes.contains(&Scope::JobsRead) { return Err(ApiError::denied()); }
  let res = backend.execute(ApiCommand::StartJobs, client_exec.clone()).await;
  ```
- **触发**：持 `jobs.read` 令牌的客户端连接 `/api/v1/ws`，发送 `openunpack.resume`。
- **影响**：只读令牌可启动/暂停全队列并读取完整快照（含设置与密码库元数据）。
- **复现**：`cargo test -p openunpack-local-api --test websocket_security`
- **修复**：WS 命令统一走 `dispatch()`；`authorize_command()` 作为唯一闸门。
- **验证**：`cargo test -p openunpack-local-api --lib authorization_tests::` → 2 passed
- **状态**：VERIFIED（2026-10-20）
```

**JSONL（机器读，供看板与统计）**

```json
{"id":"SEC-003","severity":"P1","dimension":"安全/鉴权","title":"WS 绕过作用域校验","location":["crates/local-api/src/server.rs:1018"],"trigger":"jobs.read 令牌经 WS 调 resume","impact":"只读令牌可控制队列并读取完整快照","repro":"cargo test -p openunpack-local-api --test websocket_security","fix":"WS 统一走 dispatch()","status":"VERIFIED","verified_at":"2026-10-20T22:10:00+08:00"}
```

---

## 6. 交付物与节奏

| 交付物 | 路径 | 内容 |
|--------|------|------|
| 终审报告 | `docs/audit-report-final-2026-10-XX.md` | 结论摘要 + 风险矩阵 + 五维问题清单 + 未覆盖面声明 |
| 修复日志 | `docs/audit-fix-log-final-2026-10-XX.md` | 逐项改动 + 验证命令 + 输出摘要 + 影响面 |
| 证据包 | `artifacts/audit-final-2026-10-XX/` | 命令输出、截图、PoC、性能 CSV、soak 日志 |
| 台账更新 | `docs/status-ledger-2026-10-11.md` §19 | 本轮闭环项与遗留项 |
| 发布结论 | 报告首页 | `GO` / `NO-GO` / `GO WITH CONDITIONS` + 条件清单 |

**节奏**：每阶段结束提交一次「阶段小结」（3–5 行 + 问题计数 + 阻塞项），不写长文；终审报告只在 Phase 6 一次性产出。

---

## 7. 准出标准（Definition of Done）

- [ ] **P0 = 0**，且不存在"待确认是否 P0"的悬置项。
- [ ] **P1 = 0**，或每项均有书面 `RISK-ACCEPTED`（含补偿控制、复审日期、责任人）。
- [ ] 机械门禁全绿：`clippy --workspace --all-targets -D warnings`、`fmt --check`、`cargo audit`、`npm audit`、`dotnet` 漏洞扫描、`check-contracts`、`check-architecture`。
- [ ] 原生 QA 套件在**冻结候选**上全部通过（含截图/日志）。
- [ ] 性能无 >10% 回退；24h soak 无崩溃、无内存单调增长。
- [ ] 每条 `VERIFIED` 都附可复现命令与输出摘要。
- [ ] 报告、修复日志、台账三者对同一编号的状态一致。
- [ ] 未覆盖面（如环境阻断项）在报告中**显式声明**，不得默认视为通过。

---

## 8. 风险登记

| 风险 | 概率 | 影响 | 应对 |
|------|------|------|------|
| 测试进程无法创建临时目录（当前已发生） | 高 | 30+ 用例不可跑，功能维度证据不足 | 换机执行；或修复 `%TEMP%` ACL；报告中降级声明 |
| esbuild 无法读工程文件（当前已发生） | 高 | 无生产构建证据 | 同上；至少保留 `tsc` + `lint` 证据 |
| 并行会话改同一文件 | 高 | 审计对象漂移，结论失效 | 审计期冻结写入；每阶段记录 `git status` 与文件哈希 |
| 交互会话被锁屏打断 | 中 | UI/原生用例批量失败 | 每轮 UI 前执行 `scripts/ci/ensure-interactive-session.ps1` |
| 审计范围蔓延 | 中 | 周期失控 | 严格按 §1.2 边界；新发现只登记不深挖 |
| 修复引入回归 | 中 | 需要反复 | 每批修复后必跑 Phase 1 门禁 + 相关回归用例 |

---

## 9. 附录

### A. 安全检查表（逐条打勾）

| 编号 | 检查项 | 判据 |
|------|--------|------|
| SEC-C1 | 每个 IPC 命令都有 window/label/origin 校验 | 0 遗漏 |
| SEC-C2 | 每个 API 命令都有 scope 映射且 HTTP/WS 一致 | 矩阵全覆盖 |
| SEC-C3 | `add_*` 类命令不自行扩权 | 先校验后使用 |
| SEC-C4 | 路径比较按组件、大小写无关（Windows） | 无字符串前缀判定 |
| SEC-C5 | 归档条目拒绝 `..`/绝对/ADS/设备名/尾点 | 恶意样本 100% 拒绝 |
| SEC-C6 | 子进程参数不经 shell 二次解析 | 数据走 env 或独立 argv |
| SEC-C7 | 7z 可执行文件来源为绝对路径 | 无 PATH/CWD 劫持面 |
| SEC-C8 | 密码不出现在 argv/日志/磁盘/URL | 抓取验证 |
| SEC-C9 | 远程守护强制 TLS | 非回环 http 被拒 |
| SEC-C10 | 令牌恒定时间比较 + 失败节流 | 代码核对 |
| SEC-C11 | 设置类命令钉死特权字段 | 逐字段核对 |
| SEC-C12 | 依赖漏洞为 0 或已论证豁免 | `cargo audit` 绿 |
| SEC-C13 | 危险操作（删除源/清空历史）二次确认 | UI + API 双查 |
| SEC-C14 | 崩溃转储/日志不含敏感数据 | 抽样检查 |
| SEC-C15 | 信任开关不可由环境变量开启 | 代码核对 |

### B. UI 走查场景（每条截三张图：浅色 / 深色 / 窄窗）

1. 空队列 + 拖拽区（键盘可达）
2. 批量添加 1000 条后的队列滚动与筛选
3. 密码弹窗（错误密码 → 提示 → 重试）
4. 指定密码重试（失败任务）
5. 归档树 1 万节点 + 搜索
6. 历史分页（翻页、跳页、空页）
7. 设置页（含远程守护、HTTPS 拒绝提示）
8. 任务完成动作倒计时（可取消）
9. 错误态（磁盘满 / 权限拒绝 / 引擎缺失）
10. 三语切换后的长文案与按钮换行

### C. 功能场景矩阵（行 = 格式，列 = 场景）

| 格式 \ 场景 | 正常 | 加密 | 分卷 | 嵌套 | 损坏 | 超大(>10G) | 含恶意路径 |
|-------------|------|------|------|------|------|-----------|-----------|
| ZIP | ✓ | ✓ | — | ✓ | ✓ | ✓ | ✓ |
| 7Z | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| RAR | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| TAR/GZ | ✓ | — | — | ✓ | ✓ | ✓ | ✓ |

> 每格记录：耗时、峰值内存、结果状态、清单哈希是否与预期一致。

### D. 命令速查

```bash
# 机械门禁
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo audit --deny warnings
node scripts/check-contracts.mjs && node scripts/check-architecture.mjs
npm --prefix apps/desktop-ui run lint && npm --prefix apps/desktop-ui test
npm --prefix apps/mcp-connector test

# 原生与集成
./scripts/verify.ps1 -SkipPrepare -WorkerProtocol
pwsh -File scripts/ci/ensure-interactive-session.ps1
node artifacts/remaining-fixes-2026-10-11/native-qa/harness.cjs

# 性能
pwsh -File scripts/measure-load.ps1 -Worker <frozen>/OpenUnpack.Worker.exe -Profile all

# 发布链
pwsh -File scripts/verify-release-signatures.ps1
pwsh -File scripts/test-release-validation.ps1
```
