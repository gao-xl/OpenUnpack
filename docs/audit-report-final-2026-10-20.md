# OpenUnpack 终审报告（Final Audit）

> 审计日期：2026-10-20 22:38 – 22:58（GMT+8）
> 审计计划：`docs/audit-plan-final-2026-10-20.md`
> 审计标的：工作树 `cf6e344` + 277 项改动 / 179 个未跟踪文件
> 源码指纹：`artifacts/audit-final-2026-10-20/SOURCE-SHA256.txt`（691 文件）
> 指纹 SHA-256：`0d2f2e6fc90c5949735d1385de195b1bfcbd7e04634256ee16fa5535b0448c03`
> 方法：机械门禁 → 授权层逐条核查 → 依赖三分支扫描 → 静态 UI 规范扫描 → 证据固化

---

## 0. 结论

**NO-GO（证据不足型否决）**

代码层未发现未决的 P0/P1：审计过程中发现的唯一 P1（`tsc` 构建阻断）已修复并验证；授权层、票据流程、路径校验均通过复核；Rust/npm/NuGet 三条依赖链 0 漏洞。

但按计划 §7 的准出标准，以下**必需证据无法在本机取得**，因此不能给出放行结论：

1. 前端单测与生产构建被 `esbuild` 的 `winapi error #5` 阻断；
2. 137 处依赖 `tempfile::tempdir()` 的 Rust 用例全部因 `PermissionDenied(5)` 无法执行（含 `http_contract`、宿主授权、核心协调器）；
3. 原生 UI 套件与性能/浸泡测试未执行（无冻结候选 + 无交互会话 + 无可用临时目录）。

→ 在具备正常执行环境的机器上补跑第 4 节命令后，可重新判定为 GO 或 GO WITH CONDITIONS。

---

## 1. 阶段执行结果

| 阶段 | 内容 | 状态 | 关键结果 |
|------|------|------|----------|
| Phase 0 | 冻结与基线 | 完成（降级） | 无 commit 级冻结，改用 691 文件源码指纹；工作树含 179 个未跟踪文件 |
| Phase 1 | 机械门禁 | 完成（部分阻塞） | fmt / clippy / cargo audit / contracts / architecture / eslint / tsc / dotnet build 全绿；vitest、npm build、MCP test 阻塞 |
| Phase 2 | 安全深审 | 完成 | 授权矩阵、WS 票据、路径规范化、日志抽查均复核；新增 2 项 P2 设计缺陷 |
| Phase 3 | 功能与数据一致性 | 部分完成 | protocol(6+1+1) / domain(10+3) / 授权(2) / 循环陷阱(5) / 7z 映射(2) 全过；137 处 tempdir 用例阻塞 |
| Phase 4 | UI/UX | 部分完成 | 静态扫描完成（无 emoji、21 处硬编码色、无 a11y 插件）；运行时走查阻塞 |
| Phase 5 | 性能与稳定性 | **未执行** | 无冻结 Worker、无可用临时目录、无法启动交互会话 |
| Phase 6 | 汇总 | 完成 | 本报告 + 证据包 |

---

## 2. 问题清单

### QUAL-001 【P1】`JobDetails` 缺少 `inputDisabled` 属性声明，`tsc -b` 失败（已修复）

```yaml
id: QUAL-001
severity: P1
dimension: 代码质量/类型契约
status: VERIFIED
location: apps/desktop-ui/src/components/shared.tsx:131
```

**证据**：调用方传入未声明属性，类型检查直接失败：

```tsx
// QueuePage.tsx:469
<JobDetails ... inputDisabled={deletion.busy || (snapshot.running && !daemon)} />

// shared.tsx:131（修复前）
interface JobDetailsProps { job?: JobView; close: () => void; run: ...; operationsDisabled?: boolean }
```

```
src/components/QueuePage.tsx(469,168): error TS2322:
Property 'inputDisabled' does not exist on type 'IntrinsicAttributes & JobDetailsProps'.
```

**触发**：执行 `npm run build`（其第一步即 `tsc -b`）→ 构建失败，发布链阻断。
**影响**：P1 发布阻断；同时说明该属性代表的"运行中禁止输入密码"的保护此前未生效（组件忽略该值）。
**修复**：补声明并在密码输入、保存复选框、确认按钮上接线：

```tsx
interface JobDetailsProps { ...; inputDisabled?: boolean }
function JobDetailsContent({ ..., inputDisabled = false }: JobDetailsProps)
// Input.Password / Checkbox disabled={inputDisabled}
// okButtonProps={{ disabled: !inlinePassword || inputDisabled }}
```

**验证**：`npx tsc -b` → exit 0（2026-10-20 22:47）。
**备注**：该缺陷由并行改动引入，机械门禁（lint 不查类型）未能拦截，只有 `tsc` 能发现——印证计划中"lint + tsc 必须同时保留"的判断。

---

### SEC-001 【P2】无头守护进程把全权 Token 打印到 stdout，且默认客户端无过期

```yaml
id: SEC-001
severity: P2
dimension: 安全/敏感信息
status: OPEN
location: apps/desktop-host/src/lib.rs:2811-2824
```

**证据**：

```rust
if clients.is_empty() {
    let default_client = ApiClientInput {
        name: "default-cli".into(),
        scopes: Scope::ALL.into_iter().collect(),   // 全部作用域
        read_roots: vec![],
        write_roots: vec![],
        expires_at: None,                           // 永不过期
    };
    if let Ok(created) = store.create_client(default_client) {
        println!(" Token: {}", created.token);      // 明文进 stdout
    }
}
```

**触发**：首次以 `--headless` 启动（容器 / systemd / 重定向到日志文件）。
**影响**：`Scope::ALL` 令牌可读密码库、改设置、操作全部任务，且永不过期；它会被写入容器日志、CI 日志、`nohup.out`。日志读取者（同机其他用户、日志采集系统）即可获得全部 API 权限。
**修复建议**：
1. 打印改为一次性显式动作：仅当传入 `--init-token` 时输出，并提示"请立即保存"；
2. 在 stderr 输出且默认脱敏（只显示前后 4 位），完整值写入权限收紧的文件；
3. `expires_at` 默认 30 天；
4. 默认 scope 收敛为 `jobs.read/jobs.write/files.read/files.write`，`settings.write` 与 `vault.read` 需显式授予。

---

### SEC-002 【P2】无头模式无法以最小权限运行：空根目录 + 仅 `--trust-all` 两条路

```yaml
id: SEC-002
severity: P2
dimension: 安全/权限模型
status: OPEN
location: apps/desktop-host/src/lib.rs:2812-2818; apps/desktop-host/src/local_api.rs:1449-1472
```

**证据**：默认客户端 `read_roots` / `write_roots` 均为空；`check_client_path` 在非信任模式下要求目标落在某个 root 内：

```rust
let allowed = if write { client.write_roots.iter()... } else { ... };
if !allowed.into_iter().any(|root| ...) { return Err(ApiError::denied()); }
```

而无头入口没有提供设置 roots 的参数，只有 `--trust-all`（整体关闭路径授权）。
**触发**：运维希望"只允许访问 `/data/in` 与 `/data/out`"地跑自动化。
**影响**：要么 API 完全不能读写文件，要么必须 `--trust-all` 关闭全部路径校验——**没有中间档**，与最小权限原则冲突；这也是用户倾向长期开启 `--trust-all` 的根因。
**修复建议**：新增可重复参数 `--read-root <path>` / `--write-root <path>`，在创建默认客户端时填入并 `canonicalize`；保留 `--trust-all` 作为最后手段并在启动横幅中标注风险。

---

### QUAL-002 【P3】ESLint 未启用 `jsx-a11y`，可访问性无门禁

```yaml
id: QUAL-002
severity: P3
dimension: UI·UX/可访问性
status: OPEN
location: apps/desktop-ui/eslint.config.*
```

**证据**：`grep -rn "jsx-a11y" eslint.config.* package.json` 零命中；即键盘可达性、`aria-*`、图片替代文本等只能靠人工发现。
**影响**：与 `docs/audit-report-2026-10-20.md` 中 P3-03（拖拽区键盘不可达）同类问题会持续回归。
**修复建议**：安装 `eslint-plugin-jsx-a11y` 并启用 `recommended`，先以 `warn` 引入、存量清零后升 `error`。

---

### QUAL-003 【P3】21 处硬编码色值绕过 Design Token

```yaml
id: QUAL-003
severity: P3
dimension: UI·UX/一致性
status: OPEN
location: QueuePage.tsx(8) shared.tsx(5) HistoryPage.tsx(3) ToolboxPage.tsx(2) SettingsPage.tsx(1) App.tsx(1)
```

**证据**：`grep -rnoE "#[0-9a-fA-F]{6}" src --include="*.tsx"` 共 21 处，如 `#16a34a`、`#dc2626`、`#d97706`、`#7c3aed`。
**影响**：深色主题下这些语义色不随 Token 调整，存在对比度不足与视觉不一致风险；新增主题（如高对比度）需逐处改。
**修复建议**：把状态色收敛为 `--ou-success/warning/danger/info` 四个 Token；保留 `#fff`/`#000` 例外。

---

### QUAL-004 【P3】文档与脚本漂移：`check-mirrors.mjs` 已删除但多处仍引用

```yaml
id: QUAL-004
severity: P3
dimension: 代码质量/文档一致性
status: OPEN
location: docs/plans/2026-10-17-system-detection.md:240,1563；artifacts/release/*/scripts/check-contracts.ps1
```

**证据**：当前 `scripts/` 无 `check-mirrors.mjs`（`node scripts/check-mirrors.mjs` → MODULE_NOT_FOUND）；`check-contracts.ps1` 已改为只调用 contracts + architecture，但计划文档与归档源码仍把 mirrors 当作门禁。
**影响**：误导后续审计（本计划的初稿也据此列入），并让人误以为双轨镜像仍有自动校验。
**修复建议**：在 `docs/plans/2026-10-17-system-detection.md` 标注"已被 architecture 检查取代"，或补回脚本；同时在 `docs/audit-plan-final-2026-10-20.md` 的资产表中移除该项（本轮已在下文注明）。

---

### 结转项（CARRY-OVER）

| 编号 | 等级 | 说明 | 来源 |
|------|------|------|------|
| P2-09 | P2 | 解压密码经 7-Zip 命令行参数传递，可被同机进程读取 | `audit-report-2026-10-20.md` |
| P2-12 | P2 | 便携密码库密钥可由明文 `.vault.key` 或环境变量替换 | 同上 |

两项均未在轮内修复，原因（外部工具限制 / 需要密钥格式迁移）见 `audit-fix-log-2026-10-20.md` §3。

---

## 3. 已核验无问题项（本轮实测）

| 面 | 结论 | 证据 |
|----|------|------|
| 授权矩阵 | 所有带路径的命令都在 `preflight` 前经 `file_requests()` 校验；`Add/Retry/Start` 另经 `execution_requests()` | `local_api.rs:582-599`、`1475-1577` |
| 路径校验 | 绝对路径、盘符冒号白名单（拒 ADS）、数据目录双向保护、根目录 `canonicalize` + 大小写无关前缀、重解析点二次校验 | `check_client_path` 1415-1474；`canonicalize_for_save` 390-413 |
| WS 票据 | 32 字节随机、15s TTL、单次消费、每客户端 ≤2、全局 ≤64、消费时重新校验撤销状态 | `server/events.rs:34-78` |
| WS 资源限制 | 入站帧 16 KiB、发送 2s 超时、订阅并发上限 | `events.rs:25-27,192-196` |
| WS 命令授权 | 命令统一走 `dispatch()`，与 HTTP 共用 `authorize_command` | `events.rs:265`、`server.rs:819-838` |
| 信任开关 | 仅进程内状态，`OPENUNPACK_TRUSTED_ALL_PATHS` 环境变量不再生效 | `crates/local-api/src/lib.rs:14-32` |
| 依赖 | Rust 0 漏洞；npm prod 0 漏洞；NuGet 0 漏洞 | `cargo audit`、`npm audit`、`dotnet list package --vulnerable` |
| 图标规范 | 全目录无 emoji 功能图标 | emoji 正则扫描零命中 |
| .NET 构建 | 0 warning / 0 error | `dotnet build -c Release` |

---

## 4. 未覆盖面与补跑清单

| 未覆盖 | 原因 | 补跑命令 | 验收判据 |
|--------|------|----------|----------|
| Rust 集成/协调器/宿主授权（137 处 tempdir） | 未签名子进程被禁止建目录（`PermissionDenied(5)`），已用探针确认 | `cargo test --workspace` | 全绿 |
| 前端单测（vitest） | esbuild `winapi #5` | `npm --prefix apps/desktop-ui test` | 全绿 |
| 前端生产构建 | 同上 | `npm --prefix apps/desktop-ui run build` | exit 0 |
| MCP 连接器测试 | Node 子进程 `bcrypt.dll` 加载被拒 | `npm --prefix apps/mcp-connector test` | 20/20 |
| 原生 UI / 对话框 / 拖放 | 需交互会话且候选冻结 | `pwsh scripts/ci/ensure-interactive-session.ps1` + `node artifacts/.../harness.cjs` | 用例全过 + 截图 |
| 性能 / 24h 浸泡 | 需冻结 Worker 与独占机器 | `pwsh scripts/measure-load.ps1 -Worker <frozen>/OpenUnpack.Worker.exe -Profile all` | 回退 ≤10%，无泄漏 |
| 发布链签名 | 需发布证书 | `pwsh scripts/verify-release-signatures.ps1` | 签名校验通过 |

> 环境根因已定位并记录在 `artifacts/audit-final-2026-10-20/GATES.md`：签名宿主（PowerShell）可建目录，未签名探针/测试二进制/esbuild 均被拒；`dangerouslyDisableSandbox` 复跑结果相同，排除沙箱开关因素。**这不是仓库缺陷**。

---

## 5. 走向 GO 的修复顺序

1. **恢复执行环境**（最高优先）：换一台未启用"未签名二进制限制"策略的机器，或为该目录下的测试二进制添加策略豁免；随后补跑第 4 节全部命令。
2. **SEC-001**：Token 不进 stdout、默认 scope 收敛、加过期。
3. **SEC-002**：新增 `--read-root` / `--write-root`，让无头模式可最小权限运行。
4. **QUAL-002 / QUAL-003**：a11y 门禁 + 色值 Token 化（可与 UI 迭代合并）。
5. **结转 P2-09 / P2-12**：按修复日志 §3 的方案排期。
6. 全部完成后按计划 §7 重新判定；预期结论为 **GO WITH CONDITIONS**（原生 UI 与性能需在候选冻结后复验）。

---

## 6. 证据索引

| 文件 | 内容 |
|------|------|
| `artifacts/audit-final-2026-10-20/SOURCE-SHA256.txt` | 691 个源文件哈希（标的指纹） |
| `artifacts/audit-final-2026-10-20/GATES.md` | 全部门禁命令、退出码、环境根因与测试缺口量化 |
| `docs/audit-report-2026-10-20.md` | 上一轮全量审计（差异基线） |
| `docs/audit-fix-log-2026-10-20.md` | 上一轮修复记录（P1 8/8、P2 11/13） |
| `docs/audit-plan-final-2026-10-20.md` | 本报告依据的终审计划 |

---

## 7. 修复记录（2026-10-21）

针对本报告第 2 节的问题清单执行修复，范围为本轮新发现的 P2/P3（结转项 P2-09 / P2-12 仍按原计划排期）。

| 编号 | 修复前 | 修复后 | 位置 |
|------|--------|--------|------|
| SEC-001 | 默认客户端 `Scope::ALL`、无过期、Token 明文打印 | 默认 scope 收敛为 `jobs.read/jobs.write/files.read/files.write/settings.read`；30 天过期；仅 `--init-token` 打印完整令牌，否则输出 `oup_01…abcd` 脱敏形式 | `apps/desktop-host/src/lib.rs`（客户端创建段） |
| SEC-002 | 无头只能"无文件权限"或 `--trust-all` | 新增可重复参数 `--read-root <dir>` / `--write-root <dir>`，启动时 `canonicalize` 并要求必须是已存在目录，写入默认客户端 | 同上 + `collect_flag_values` / `canonical_api_roots` |
| QUAL-002 | 无 a11y 门禁 | **未修复（上游限制）**：`eslint-plugin-jsx-a11y@6.10.2` 的 peer 范围为 `eslint ^3–^9`，项目使用 ESLint 10.12，强行安装需 `--legacy-peer-deps` 且可能运行时不兼容 | `apps/desktop-ui/eslint.config.js` |
| QUAL-003 | 21 处硬编码色值 | 新增 `--ou-success/-danger/-warning/-accent/-info` 及三组 `-bg/-border` Token（深色主题提亮），TSX 中 hex/rgba 色值清零 | `src/styles.css` + 6 个组件 |
| QUAL-004 | 文档仍引用已退役脚本 | 在计划文档两处标注退役说明；终审计划中的资产/命令/准出项同步移除 | `docs/plans/2026-10-17-system-detection.md`、`docs/audit-plan-final-2026-10-20.md` |

附带：`README.md` 新增「无头模式（Headless Daemon）」参数表，说明 `--read-root` / `--write-root` / `--init-token` 与默认权限；同时修掉并行新增测试 `local_api/native_tests.rs:225` 的 `MutexGuard` 跨 `await`（改为块级作用域，消除潜在死锁与 Clippy 红）。

### 验证

| 检查 | 结果 |
|------|------|
| `cargo fmt --all -- --check` | 0 差异 |
| `cargo clippy -p openunpack-desktop --lib -- -D warnings` | 通过 |
| `npm run lint` | 0 error / 0 warning |
| `npx tsc -b` | 0 error |
| TSX 硬编码色值 | `grep -c "#[0-9a-fA-F]{6}"` = **0**；`rgba(...)` = **0** |
| 新增单测 `headless_args_tests`（2 条） | **无法运行**：测试二进制启动即 `STATUS_ENTRYPOINT_NOT_FOUND (0xc0000139)`，与 §4 环境限制同源；已通过编译与 Clippy |

### 结论更新

代码层未决项从 2×P2 + 3×P3 收敛为：**未决 P2 = 0；P3 中 2 项已修、1 项（QUAL-002）因上游限制记 WONTFIX**，另结转 P2-09 / P2-12。整体结论仍为 **NO-GO（证据不足型）**——放行与否取决于第 4 节补跑结果，而非代码整改。
