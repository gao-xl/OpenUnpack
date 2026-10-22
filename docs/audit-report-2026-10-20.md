# OpenUnpack 全量代码审计报告

> **审计日期**：2026-10-20
> **审计对象**：`codex/open-unpack` 分支工作区（Rust 7 crate + Tauri 宿主 + React 前端 + MCP 连接器）
> **审计维度**：安全漏洞、逻辑 Bug、边界条件、异常与资源管理、性能隐患
> **方法**：危险模式全仓扫描 → 关键函数逐行复核 → `cargo audit` 实跑 → 与历史 18 轮审计交叉核对
> **结论**：未发现 P0（可远程利用 / 数据损坏）级缺陷；确认 8 项 P1、13 项 P2、5 项 P3。安全基线显著高于同类桌面项目，风险集中在**本地 API 的授权模型**与**远程连接凭据处理**，而非传统的注入类漏洞。

---

## 0. 结论摘要

### 0.1 风险矩阵

| 等级 | 数量 | 主题分布 |
|------|------|----------|
| P0 | 0 | — |
| P1 | 8 | 授权/作用域 3 / 权限模型 1 / 凭据与传输 2 / 依赖供应链 1 / 安全控制失效 1 |
| P2 | 13 | 命令注入 1 / 路径判定 1 / 信任开关 1 / 解释器劫持 1 / 凭据暴露 1 / 数据完整性 1 / 资源上限 1 / 前端逻辑 4 / 引擎一致性 1 / 便携密钥 1 |
| P3 | 5 | 配额核算 1 / 性能 1 / 无障碍 1 / 死代码与架构 2 |

### 0.2 总体判断

**做得好的地方（不是客套，是审计结论）**：

- 路径安全采用 **handle-based** 方案（`DirectoryGuard` / `LockedEntry` / `LockedTree`），而非字符串拼接校验；重解析点（junction/symlink）在每一级祖先上拒绝，规避了 TOCTOU 与 8.3 短名绕过。
- 本地 API：仅绑 `127.0.0.1`、校验 Host 唯一性、非 WS 端点拒绝 query 参数、Token 用恒定时间比较（`fold(|all,(l,r)| all|(l^r))`）、作用域在 HTTP 层集中强制。
- 设置的危险字段有 SEC-03 注入守卫（`post_extraction_action` / `watched_folders` 被钉死为现值）。
- 前端无 XSS 面：全目录无 `dangerouslySetInnerHTML` / `innerHTML` / `eval`；队列列表已虚拟化；快照有 revision + epoch 双层守卫。
- 全仓无 SQL 字符串拼接（唯一的 `format!("SELECT ...")` 出现在测试代码 `architecture_convergence.rs:179`）。

**真正的薄弱点**：

1. **本地 API 的授权模型存在结构性缺口**：WebSocket 通道绕过集中式作用域校验；`submit_input` 缺少任务归属校验；`add_jobs` 对路径"先扩权后使用"而非校验既有授权。
2. `settings.write` 作用域的**实际权限**被低估——它能写入 `on_complete` 钩子，等价于任意命令执行；
3. 远程守护进程模式把**整个权限凭据**放在 localStorage 与 WebSocket query 里，且允许明文 HTTP；
4. `zip` 依赖携带 1 条 9.1 critical 漏洞，且 CI 的 `cargo audit` 门禁当前必然失败（已实测退出码 1）；
5. 引擎行为不一致：输出排除规则只在原生 ZIP 引擎生效，7z/RAR 路径**完全忽略**。

---

## 1. 审计范围与方法

### 1.1 覆盖范围

| 批次 | 模块 | 规模 |
|------|------|------|
| 1 | `crates/core`（解压协调器 / 扫描 / 工具箱 / 编码探测） | 34 文件 / 16,074 行 |
| 2 | `crates/infrastructure` + `crates/local-api` | 21 + 4 文件 / 14,014 行 |
| 3 | `apps/desktop-host` + `crates/protocol`/`domain`/`application`/`cli` | 4 + 32 文件 / 8,365 行 |
| 4 | `apps/desktop-ui` + `apps/mcp-connector` | 27 + 4 文件 / 7,292 行 |

### 1.2 执行的关键命令

```bash
cargo audit                    # 退出码 1，3 条漏洞 + 5 条 unmaintained
cargo audit --json             # advisory DB: 1338 条，last-updated 2026-10-17，依赖 546 个
# 危险模式扫描：unsafe(183 处) / Command::new(24 处) / .unwrap()(783 处，逐文件区分 src 与 #[cfg(test)])
# 路径判定扫描：starts_with / canonicalize / symlink_metadata / remove_dir_all / set_permissions
```

### 1.3 与历史审计的关系

仓库已有 `docs/audit-report-2026-10-12.md`（A1–A3 / S2 / S1 / F1–F2 / R1–R2 / E1–E2）与台账 §14–§18（SEC-01–04、D1–D6、S-01–S-24）的闭环记录。本报告**只登记新发现或复发项**，已确认修复的历史项不重复计入（见 §7）。

### 1.4 等级定义

| 等级 | 判定标准 |
|------|----------|
| P0 | 可远程无凭据利用，或导致用户数据损坏/丢失 |
| P1 | 本地权限提升、凭据泄露、核心交付链阻断 |
| P2 | 特定前置条件下的注入/绕过、功能死路、资源无上限 |
| P3 | 健壮性、性能与体验层面的改进项 |

---

## 2. P1 级问题

### P1-01 `settings.write` 作用域可写入完成钩子，等价于任意命令执行

**位置**

- `apps/desktop-host/src/local_api.rs:757-803`（SaveSettings 守卫）
- `crates/local-api/src/command.rs:369`（SaveSettings 要求 `SettingsWrite`）
- `crates/core/src/coordinator/scheduler.rs:319-324`（终态触发钩子）
- `crates/core/src/coordinator/scheduler.rs:801-844`（命令构造）
- `crates/core/src/coordinator/integrity.rs:12-56`（`validate_settings` 未校验 `on_complete`）

**证据**：SEC-03 守卫只钉死了两个字段，第三个更危险的字段被漏掉：

```rust
// local_api.rs:757-767
SaveSettings { mut settings } => {
    // SEC-03: Settings Injection Guard
    // API clients are strictly forbidden from modifying system-privileged fields:
    // post_extraction_action (shutdown/sleep/hibernate injection) and watched_folders.
    settings.post_extraction_action = current_settings.post_extraction_action;
    settings.watched_folders = current_settings.watched_folders;
    // ❌ on_complete 未还原、未校验
```

而该字段在任务终态时会被直接执行，`else` 分支是**任意 shell 命令**：

```rust
// scheduler.rs:824-831
} else if std::path::Path::new(&script_cmd).is_file() {
    let mut c = tokio::process::Command::new(&script_cmd);
    c.arg(&job_id).arg(&status).arg(&output_path);
    c
} else {
    let mut c = tokio::process::Command::new("cmd.exe");
    c.arg("/C").arg(&script_cmd);   // 任意命令字符串
    c
};
```

`validate_settings`（integrity.rs:12-56）校验了 max_depth / max_entries / 并发数 / 目录合法性 / 排除规则长度，**没有对 `on_complete` 做任何校验**。

**触发场景**：第三方工具（MCP 连接器、脚本）被授予 `settings.write` 作用域 → 调用 `save_settings` 把 `onComplete` 设为 `powershell -enc <base64>` → 任意任务进入终态（含 failed）时执行。

**影响**：权限模型被击穿。用户授予的是"修改设置"，实际获得的是"以当前用户身份执行任意命令"，可越过 `read_roots`/`write_roots` 的文件作用域限制读写任意文件、读取凭据库。作用域名与实际权限严重不匹配。

**修复建议**（两步，缺一不可）：

1. 在 SEC-03 守卫中一并钉死：

```rust
settings.on_complete = current_settings.on_complete;
```

2. 若确需允许 API 配置钩子，则改为受限语义：只接受**绝对路径且必须是已存在的可执行文件**，拒绝含空格拼接的裸命令，并在 UI 侧强制经文件选择器授权：

```rust
if let Some(hook) = settings.on_complete.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
    let p = Path::new(hook);
    if !p.is_absolute() || !p.is_file() {
        return Err("完成钩子必须是已存在的绝对路径可执行文件。".into());
    }
    // 拒绝 .ps1/.bat 之外的解释器裸命令
}
```

同时把 `scheduler.rs:829-831` 的 `cmd /C <任意字符串>` 兜底分支删除——它让"脚本路径"字段退化为"命令行"字段。

---

### P1-02 守护进程 Token 明文持久化，并经 WebSocket query 传输

**位置**：`apps/desktop-ui/src/api.ts:143-159`、`160-163`、`204-209`；`apps/desktop-ui/src/components/ApiSettingsPanel.tsx:116-120`

**证据**

```ts
// api.ts:143-151
setDaemonConnection:(url:string|null,token:string|null)=>{
  if(url){
    const cleanUrl = url.trim().replace(/\/+$/, '');
    const cleanToken = (token || '').trim();
    activeDaemon = { url: cleanUrl, token: cleanToken };
    try {
      localStorage.setItem('openunpack_daemon_url', cleanUrl);
      localStorage.setItem('openunpack_daemon_token', cleanToken);  // 明文落盘
    } catch {}
```

```ts
// api.ts:207-209
const wsUrl = activeDaemon.url.replace(/^http/i, 'ws');
const tokenParam = activeDaemon.token ? `?token=${encodeURIComponent(activeDaemon.token)}` : '';
ws = new WebSocket(`${wsUrl}/api/v1/ws${tokenParam}`);   // 凭据进 URL
```

**触发场景**：用户配置并连接远程/本地守护进程后，Token 常驻 `localStorage`；每次建立 WS 都把它拼进查询串。

**影响**

- 该 Token 是全权凭据：可调用 `RevealPassword`（`local_api.rs:717`）读取**明文密码**、增删任务、修改设置。
- `localStorage` 在 WebView2 profile 下是磁盘明文文件；崩溃转储、漫游配置同步、备份工具、同机低权限进程均可读取，且重启后仍然有效。
- Token 进入 URL query 后，会残留在服务端访问日志、反向代理日志、崩溃报告与 `Referer` 类记录中。
- 对照组：`apps/mcp-connector/src/api.ts:62` 只用 `Authorization` 头传 Token，且 `redact()` 做了脱敏——同一项目里已有正确范式，说明这是前端实现遗漏而非设计取舍。

**修复建议**

1. 桌面端改存系统凭据（DPAPI / `keyring` / Tauri stronghold），或仅保存在内存、每次启动重输；至少不要写 `localStorage`。
2. WS 鉴权改为**首帧消息**或 `Sec-WebSocket-Protocol` 子协议传 Token，不要放 query。
3. `getDaemonConnection()`（api.ts:160-163）不要回传明文 Token，改为 `{ url, hasToken: boolean }`，避免它进入 React state 被 DevTools 读取。

---

### P1-03 远程守护进程允许明文 HTTP 传输密码与 Token

**位置**：`apps/desktop-ui/src/api.ts:143-147`（URL 仅 trim + 去尾斜杠）；`ApiSettingsPanel.tsx:89-101`（文案明确支持"远程服务器"）

**证据**

```ts
const cleanUrl = url.trim().replace(/\/+$/, '');   // 无 scheme 校验、无主机白名单
activeDaemon = { url: cleanUrl, token: cleanToken };
```

界面文案：`"支持将界面与后台解压引擎解耦，连接远程服务器或本地无头守护进程。"`（ApiSettingsPanel.tsx:90）。填入 `http://<公网IP>:47980` 即可成立，无任何警告。

**触发场景**：用户在内网/公网跨机使用守护进程模式（产品明确支持的场景）。

**影响**：`save_password` / `reveal_password` / `retry_job(password)` 的**明文密码**与 `Authorization: Bearer <token>` 全程明文传输，同网段嗅探即可获取整个凭据库。这是本产品最敏感数据的直接泄露路径。对照：本地 HTTP 服务端本身只绑 `127.0.0.1`（`server.rs:267,278`），风险完全来自前端允许用户把它指向远端。

**修复建议**：在 `setDaemonConnection` 中强制协议策略：

```ts
const u = new URL(cleanUrl);
const loopback = ['127.0.0.1', 'localhost', '::1'].includes(u.hostname);
if (u.protocol !== 'https:' && !loopback) {
  throw new Error(t('非本机地址的守护进程必须使用 HTTPS。'));
}
```

并在面板上对 `http:` + 非回环地址渲染 `Alert type="error"`。

---

### P1-04 `zip` 依赖存在 1 条 critical 漏洞，且 CI 依赖门禁当前必然失败

**位置**：`Cargo.lock`（`zip 8.6.0`）；`crates/infrastructure/Cargo.toml:24`；`crates/infrastructure/src/engine/zip_engine.rs`；`.github/workflows/windows.yml:78`；`scripts/security-audit-policy.ps1:81-87`

**证据**（实跑 `cargo audit`，退出码 **1**）

| 编号 | 严重度 | 标题 | 修复版本 |
|------|--------|------|----------|
| RUSTSEC-2026-0369 | **9.1 critical** | `extract()` 沿符号链接应用 Unix 权限且不限制目标 | ≥ 9.0.0 |
| RUSTSEC-2026-0367 | 7.3 high | 提取时保留 SUID/SGID 位 | ≥ 9.0.0 |
| RUSTSEC-2026-0368 | — | `extract()` 创建指向目标目录外的符号链接 | ≥ 9.0.0 |

**可达性分析（重要，避免夸大）**：`zip_engine.rs` 的 `extract_zip` 采用**手动逐条目解压**（`by_index` / `by_index_decrypt` + `File::create` + 自行 `validate_relative_path`），**不调用** `ZipArchive::extract()` / `ZipStreamReader::extract()`，也不创建符号链接。因此三条告警描述的攻击路径**当前不可达**，不构成本报告中的路径穿越缺陷。但 `extract_zip` 已接入调度（`engine/mod.rs:156`），是处理不可信压缩包的主路径之一，依赖解析器本身的任何缺陷都直接面对攻击者构造的归档。

**交付链影响（确定性结论）**：CI 步骤 `./scripts/audit-security.ps1 -DenyWarnings`（windows.yml:78）会执行 `cargo audit --deny warnings`；该脚本的豁免白名单（security-audit-policy.ps1:81-87）只覆盖 5 条 unmaintained 告警，**不含上述 3 条 zip 漏洞**，且 `zip` 在 Windows 依赖图中可达。故本地实测退出码为 1，CI 在同一 lockfile 下必然失败——即当前分支不具备可发布状态。

**修复建议**：

1. 升级 `zip` 至 `>= 9.0.0`（`crates/infrastructure/Cargo.toml:24`），确认 `by_index_decrypt` 等 API 在 9.x 的签名变化并同步测试。
2. 升级后在 `zip_engine.rs` 增加回归用例：构造含 symlink 条目（`external_attributes` 置 `S_IFLNK`）与 SUID 位的 ZIP，断言解压产物为普通文件且权限位被掩码。
3. 若短期无法升级，必须在 `security-audit-policy.ps1` 中登记带**可达性论证**的豁免，而不是让 CI 静默失败或跳过。

---

### P1-05 WebSocket 通道绕过集中式作用域校验，`jobs.read` 令牌可执行 `jobs.write` 操作

**位置**：`crates/local-api/src/server.rs:1013-1027`（准入）、`1121-1134`（Snapshot）、`1149-1162`（StartJobs）、`1089-1119`（SubmitInput）、`893-906`（被绕过的校验）

**证据**：作用域强制只存在于 HTTP 的 `dispatch()` 中：

```rust
// server.rs:899-906 —— 唯一的集中式作用域闸门
if command.scopes().iter().any(|scope| !client.scopes.contains(scope))
    || command.requires_file_delete() && !client.scopes.contains(&Scope::FilesDelete)
{
    return Err(ApiError::denied());
}
```

而 WS 通道**直接调用 `backend.execute`**，不经过 `dispatch`：

```rust
// server.rs:1018 —— 只要求 JobsRead 即可建立连接
if !client.scopes.contains(&Scope::JobsRead) { return Err(ApiError::denied()); }

// server.rs:1122
let res = backend.execute(ApiCommand::Snapshot, client_exec.clone()).await;   // 实际要求 JobsRead+VaultRead+SettingsRead
// server.rs:1150
let res = backend.execute(ApiCommand::StartJobs, client_exec.clone()).await;  // 实际要求 JobsWrite
// server.rs:1102
let res = backend.submit_input(job_id, input.to_string()).await;              // 实际要求 JobsWrite
```

`command.rs:333` 明确 `Snapshot => &[JobsRead, VaultRead, SettingsRead]`，`command.rs:347` 明确 `StartJobs/PauseJobs => JobsWrite`，`command.rs:378` 明确 `SubmitInput => JobsWrite`——三条全部被 WS 绕过。

**触发场景**：为第三方工具创建只读令牌（仅勾选 `jobs.read`，这是最容易被用户接受的权限）→ 该工具改用 WS 连接 → 发送 `{"jsonrpc":"2.0","method":"openunpack.snapshot"}` 或 `openunpack.resume`。

**影响**：

- 只读令牌可**启动/暂停全部任务**，并获得**完整 `Snapshot`**（含设置、路径、密码库条目元数据），而这些在 HTTP 侧需要额外两个作用域；
- 可向任意 `jobId` 提交输入（见 P1-06）。作用域模型在 WS 上整体失效，"最小权限"承诺不成立。

**修复建议**：把作用域校验下沉到唯一入口，让 `backend.execute` 内部强制，WS 只做转发：

```rust
// 在 ApiBackend::execute 实现的最外层
if command.scopes().iter().any(|s| !client.scopes.contains(s)) {
    return Err(ApiError::denied());
}
```

并让 WS 的 `submit_input` 走 `ApiCommand::SubmitInput`（享受同一套校验与 `validate()`），不要直接调用 backend 方法。补测试：仅持 `JobsRead` 的 client 通过 WS 调 `resume` 必须返回 `-32000`。

---

### P1-06 `submit_input` 缺少任务归属校验：`jobs.write` 令牌可向任意任务注入密码并写入全局密码库

**位置**：`apps/desktop-host/src/local_api.rs:926-959`

**证据**

```rust
SubmitInput { job_id, input_type, value: input_value } => {
    let handled = if input_type == "password" {
        state.coordinator.submit_input(&job_id, input_value.clone()).unwrap_or(false)  // ← 无归属校验
    } else { false };
    if !handled && input_type == "password" {
        let pwd_input = PasswordInput { id: None, name: format!("Job {job_id} Password"), secret: Some(input_value.clone()), .. };
        let _ = state.vault_use_cases.upsert_password(pwd_input);        // ← 写全局密码库
        let _ = super::core_retry_job(&state, job_id.clone(), false, Some(input_value)).await;  // ← 重试任意任务
    }
```

同文件其他涉及任务的命令都会先构造 `ClientJobPolicy` 并调用 `policy.authorize(&job_id)`（例如 `RemoveJob` 分支，`local_api.rs:686`），`SubmitInput` 是唯一绕过的写操作。

**触发场景**：客户端 A 持有 `jobs.write`（用于提交自己的解压任务），枚举/猜测到客户端 B 或 GUI 任务的 `jobId`（`jobId` 形如 `job-1759...`，可从快照或历史推断），向该任务提交密码。

**影响**：

- 密码被写入**全局密码库**（`upsert_password`），成为持久化污染，后续所有任务都会尝试该候选；
- `core_retry_job` 会重启不属于该客户端的任务，破坏他人任务状态；
- 结合 P1-05，只读令牌也能触发本条。

**修复建议**：进入分支前先做归属校验，并只允许"该客户端创建且处于 needsPassword"的任务：

```rust
let policy = ClientJobPolicy { client: client.clone(), .. };
policy.authorize(&job_id).map_err(|_| ApiError::denied())?;
// input_type 仅接受 password；不满足条件时返回 400 而非静默写库
```

删除 `upsert_password` 这个副作用——API 提交的解压密码不应自动进入用户密码库。

---

### P1-07 `add_jobs` 对任意路径"先扩权后使用"，`read_roots` / `write_roots` 形同虚设

**位置**：`apps/desktop-host/src/local_api.rs:875-924`；`crates/local-api/src/command.rs:377-389`

**证据**

```rust
if let Some(out) = output_dir {
    let canon_out = super::canonicalize_existing(&out)...;
    settings.output_root = canon_out.to_string_lossy().to_string();   // 任意输出根
}
...
{
    let mut access = state.access.lock()...?;
    access.grant_read(canon_path, false);                            // 给自己授权
    if !settings.output_root.is_empty() {
        access.grant_write_dir(PathBuf::from(&settings.output_root)); // 给自己授权
    }
}
```

全段没有 `is_client_path_authorized(...)`（该函数存在于 `local_api.rs:216`，`add_jobs` 未调用）。

**触发场景**：客户端持 `jobs.write + files.read + files.write`（提交任务的常规组合），传入 `path = C:\Users\other\机密.zip`、`output_dir = C:\Users\other\`。

**影响**：文件作用域的**审批模型被整体跳过**——`api_create_client` 阶段要求目录经本机选择器批准（`local_api.rs:148-173`），但 `add_jobs` 反而把客户端带来的任意路径加入授权集合。若同时持 `files.delete` 且 `delete_source=true`（`command.rs:388` 会要求该作用域），源文件可被回收/删除，作用域外亦然。

**修复建议**：`add_jobs` 必须先校验再授权，且不得反向扩权：

```rust
if !is_client_path_authorized(&canon_path.to_string_lossy(), &client)
    || (output_dir.is_some() && !is_client_path_authorized(&canon_out.to_string_lossy(), &client)) {
    return Err(ApiError::permission_denied("路径不在已批准的目录范围内。"));
}
```

`access.grant_*` 只应保留给 GUI 的本机目录选择器，不应出现在 API 命令路径。

---

### P1-08 输出排除规则只在原生 ZIP 引擎生效，7z / RAR 路径完全忽略

**位置**：`crates/infrastructure/src/engine/cli_engine.rs:172-250`；对照 `zip_engine.rs:171-179`；请求体 `crates/core/src/coordinator/pipeline_helpers.rs:50-58`

**证据**：请求体确实携带了用户的排除/包含规则：

```rust
// pipeline_helpers.rs:50-58
exclusions: if operation == "extract" { s.output_exclusions.clone() } else { vec![] },
inclusions: if operation == "extract" { s.output_inclusions.clone() } else { vec![] },
```

原生引擎会逐条过滤：

```rust
// zip_engine.rs:172-179
if matches_exclusion(&relative_path, &request.exclusions) { continue; }
if !matches_inclusion(&relative_path, is_dir, &request.inclusions) { continue; }
```

而 CLI 引擎整段没有任何引用：

```rust
// cli_engine.rs:180-192 —— 只有 -o / -y / -p 与归档路径，exclusions 未使用
cmd.arg("x").arg(format!("-o{}", destination.display())).arg("-y");
cmd.arg(&request.archive_path);
```

`extract_cli` 之后的 `WalkDir` 仅负责生成 `FileRecord`（含哈希），不做删除或过滤；`matches_exclusion` 在 core 中也只被扫描器调用，没有任何后置补偿。

**触发场景**：用户在设置中配置 `outputExclusions = ["*.exe", "*.scr"]`（把排除规则当安全检查用），然后解压一个 `.rar` / `.7z`——这是产品的主力格式路径。

**影响**：UI 的排除预览（`preview_exclusions`）会显示"这些文件将被跳过"，实际全部落盘。**同一份设置在不同引擎下行为不同**，且失败方向是"多写了用户明确排除的文件"，属安全预期落空而非单纯功能缺失。

**修复建议**：二选一，不要维持现状：

1. 解压后按 `request.exclusions` / `inclusions` 清理已写出的文件（需在 core 的 readback 之前，并同步修正 `FileRecord` 清单）；
2. 或把排除规则翻译为 7-Zip 的 `-x!<pattern>` / `-i!<pattern>` 参数，由引擎在执行时跳过：

```rust
for pattern in &request.exclusions { cmd.arg(format!("-x!{pattern}")); }
for pattern in &request.inclusions { cmd.arg(format!("-i!{pattern}")); }
```

并补跨引擎一致性测试：同一归档 + 同一规则，`zip` 与 `7z` 的 `FileRecord` 集合必须相等。

---

## 3. P2 级问题

### P2-01 完成钩子以 `cmd.exe /C` 拼接参数，输出路径可注入命令

**位置**：`crates/core/src/coordinator/scheduler.rs:814-831`

**证据**

```rust
} else if script_cmd.to_lowercase().ends_with(".bat")
    || script_cmd.to_lowercase().ends_with(".cmd")
{
    let mut c = tokio::process::Command::new("cmd.exe");
    c.arg("/C")
        .arg(&script_cmd)
        .arg(&job_id)
        .arg(&status)
        .arg(&output_path);   // ← 来自压缩包名/输出根，可含 '&'
```

`output_path` 的来源（scheduler.rs:780-789）是 `job.output_path` → `options.output_root` → **压缩包所在目录**。压缩包文件名由外部提供者可控制，目录名可含 `&`。`Command` 的引号规则只对含空白的参数加引号；`a&calc` 这类不含空格的参数会**裸传**给 `cmd.exe`，由 cmd 二次解析时在 `&` 处断句。

**触发场景**：用户配置了 `.bat` / `.cmd` 完成钩子；随后处理一个位于 `D:\in\x&calc\` 目录（或输出到名为 `x&calc` 的目录）的压缩包，任务结束时执行注入命令。

**影响**：钩子调用者（用户）被归档提供者借道执行任意命令——跨信任边界。

**修复建议**：不要经 `cmd.exe` 传递数据参数。`.bat/.cmd` 分支改为：

```rust
let mut c = tokio::process::Command::new("cmd.exe");
c.arg("/C").arg(&script_cmd);            // 仅脚本本身
c.env("OPENUNPACK_JOB_ID", &job_id)      // 数据一律走环境变量
 .env("OPENUNPACK_STATUS", &status)
 .env("OPENUNPACK_OUTPUT", &output_path);
```

同时删除 `else { cmd.exe /C script_cmd }` 兜底（见 P1-01）。非 Windows 分支 `format!("{} \"$@\"", script_cmd)`（scheduler.rs:838）同样应改为只传脚本路径 + 环境变量。

---

### P2-02 循环陷阱判定大小写敏感，Windows 下可漏判导致自激解压

**位置**：`crates/core/src/intake_guard.rs:12-36`、`201-209`

**证据**

```rust
let out_norm = normalize_path_for_comparison(output_dir);
...
if out_norm.starts_with(&watched_norm) { return Err("...自激死循环解压...") }

fn normalize_path_for_comparison(p: &Path) -> PathBuf {
    p.canonicalize().unwrap_or_else(|_| { /* 仅拼接 components，未归一化大小写 */ })
}
```

`Path::starts_with` 是**纯词法、按组件、区分大小写**的比较（std 文档明确说明不解析符号链接、不归一化大小写）。Windows 文件系统大小写不敏感，因此：

- 监听目录 `C:\Downloads`（已存在 → canonicalize 返回真实大小写 `C:\Downloads`）
- 输出目录 `c:\downloads\unpacked`（**尚不存在** → canonicalize 失败 → 走 fallback，保留用户输入的小写）

`starts_with` 返回 `false` → 守卫放行 → 解压产物回灌监听目录。这是一条常见的真实路径：用户在设置里手输/粘贴小写输出路径，而目录尚未创建。

**影响**：监听目录内新产生的归档被再次入队，形成自激解压循环，CPU/磁盘持续占用，队列无限增长。这正是 G-01 守卫要防的唯一场景。

**修复建议**：归一化时同时处理大小写与 `..`，并显式比较组件而不是依赖 `Path::starts_with` 的平台语义：

```rust
fn normalize_path_for_comparison(p: &Path) -> Vec<String> {
    let base = p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
    base.components()
        .filter_map(|c| match c {
            Component::Normal(s) => Some(s.to_string_lossy().to_lowercase()),
            Component::Prefix(pre) => Some(pre.as_os_str().to_string_lossy().to_lowercase()),
            Component::RootDir => Some(String::new()),
            _ => None,
        })
        .collect()
}
// 前缀判定：a.len() <= b.len() && a == b[..a.len()]
```

并补测试：`check_loop_trap(&["C:\\Downloads"], Path::new("c:\\downloads\\out"))` 必须返回 `Err`。

---

### P2-03 `OPENUNPACK_TRUSTED_ALL_PATHS` 以进程环境变量承载，可被父进程注入并整体放行路径授权

> **定性说明**：该开关是无头/自动化模式的**有意设计**（`--trust-all` / `-y`，`lib.rs:2634-2638`；见 CHANGELOG「全路径信任模式」），不是遗留调试代码。本条针对的是它的**承载方式**，而非功能本身。

**位置**：`crates/local-api/src/lib.rs:16-20`；`apps/desktop-host/src/lib.rs:2633-2638`；`apps/desktop-host/src/local_api.rs:217-219`（另有 148/770/1206/1242/1357 五处）

**证据**

```rust
// crates/local-api/src/lib.rs:16
pub fn is_trusted_all_paths() -> bool {
    std::env::var("OPENUNPACK_TRUSTED_ALL_PATHS")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}
```

```rust
// apps/desktop-host/src/lib.rs:2634-2638
if args.iter().any(|a| a == "--trust-all" || a == "-y") {
    unsafe { std::env::set_var("OPENUNPACK_TRUSTED_ALL_PATHS", "1"); }
}
```

```rust
// apps/desktop-host/src/local_api.rs:216-219
fn is_client_path_authorized(path_str: &str, client: &ApiClient) -> bool {
    if openunpack_local_api::is_trusted_all_paths() { return true; }   // 任意路径直接放行
```

**触发场景**：任何能以该环境变量拉起 OpenUnpack 的本地程序（快捷方式的目标字符串、计划任务、安装脚本、父进程注入）。用户未必知道 `-y` 这个别名意味着"关闭全部路径审批"。

**影响**：文件作用域模型被一行环境变量整体旁路，且：

- 该开关**同时**关闭 `api_create_client` 的目录审批（:148）、`save_settings` 的输出/缓存根目录边界（:770）与 `is_client_path_authorized`（:217）；
- 无任何审计日志或 UI 标识，事后无法判断本次会话是否处于全信任模式；
- GUI 模式与无头模式共用同一判定，缺少"仅无头可用"的约束。

**修复建议**：把"信任"从环境变量改为**解析后的显式状态**，并加约束与留痕：

```rust
// 解析参数后写入 AppState，而不是 set_var
let trust_all = args.iter().any(|a| a == "--trust-all");
if trust_all && !headless { return Err("--trust-all 仅允许在 --headless 模式下使用。"); }
tracing::warn!(target: "security", "--trust-all 已启用：本机 API 路径授权被整体跳过");
```

`is_trusted_all_paths()` 改为读取注入的状态值，不再读环境变量；同时把 `-y` 别名移除（语义不明显）。

---

### P2-04 目录扫描无条目上限，超大目录可致内存线性膨胀

**位置**：`crates/core/src/scanner.rs:814-963`

**证据**：`scan_with_cancel` 只校验**排除规则**的数量与长度（:807-813），对 `collected_files` / `seen_canonical` / 后续 `dir_map` **没有任何条目上限**：

```rust
let mut collected_files = Vec::new();
let mut seen_canonical = BTreeSet::new();
...
if seen_canonical.insert(key) { collected_files.push(cpath); }   // 无上限
```

对照：工具箱模块有 `MAX_TOOLBOX_FILES = 50_000`、`MAX_TOOLBOX_ENTRIES = 100_000` 与时间预算（toolbox.rs:44-52,52-60），扫描模块没有对应约束。

**触发场景**：拖入/选择包含数十万文件的目录（如 `node_modules` 父目录、下载盘根目录、被 P2P 客户端预分配大量文件的目录），且 `recursive = true`。

**影响**：`PathBuf` + `BTreeSet<String>` 双份结构常驻，百万级条目可达数百 MB；同时 `dir_map` 再复制一份分组关系，UI 侧还要序列化整份扫描结果。低内存机器会出现长时间卡顿或 OOM，且扫描阶段无进度反馈。

**修复建议**：引入与工具箱一致的硬上限并提前失败，给出可操作的提示：

```rust
const MAX_SCAN_FILES: usize = 200_000;
if collected_files.len() >= MAX_SCAN_FILES {
    return Err(format!("扫描条目超过上限（{MAX_SCAN_FILES}），请缩小目录范围或增加排除规则。"));
}
```

同时把扫描结果改为增量回传（分批事件）而非一次性构造。

---

### P2-05 「指定密码重试」是功能死路，点击后界面无任何反应

**位置**：`apps/desktop-ui/src/components/QueuePage.tsx:49-54`、`205-221`、`412`

**证据**：按钮对非 `needsPassword` 的可重试任务可见，但弹窗开关要求任务**必须**是 `needsPassword`：

```tsx
const passwordOwnerPresent = !passwordJob
  || snapshot.jobs.some(job => job.id === passwordJob.id && job.status === 'needsPassword');
useEffect(() => {
  if (passwordOwnerPresent) return;
  const epoch = ++passwordDraftEpoch.current;
  queueMicrotask(() => { if (passwordDraftEpoch.current === epoch) closePassword(); });
}, [passwordOwnerPresent, closePassword]);
...
open={!!passwordJob && passwordOwnerPresent}     // 恒为 false
```

而按钮的触发条件恰好相反（`retryable(job) && job.status !== 'needsPassword'`，:205），点击后 `setPasswordJob(job)` 立刻被上面的 effect 清空。

**触发场景**：对 `failed` / `cancelled` / `interrupted` / `waitingResource` 任务点击"指定密码重试"。

**影响**：该恢复路径完全失效，且无报错、无 loading、无任何反馈，用户会判断为程序卡死；界面却持续渲染这个无效按钮，前后自相矛盾。

**修复建议**：守卫只用于"任务从快照中消失"的场景，不绑定状态迁移（重试成功后的关闭已由 `onOk` 负责）：

```tsx
const passwordOwnerPresent = !passwordJob
  || snapshot.jobs.some(job => job.id === passwordJob.id);
```

---

### P2-06 全部任务被取消后仍触发关机/休眠倒计时

**位置**：`apps/desktop-ui/src/App.tsx:171-186`

**证据**

```tsx
const allFinished = snapshot.jobs.length > 0 && snapshot.jobs.every(j =>
  ['completed', 'partial', 'salvaged', 'failed', 'cancelled', 'skipped'].includes(j.status)
);
if (allFinished) {
  ...
  } else if (action === 'sleep' || action === 'shutdown') {
    setTimeout(() => { setCountdownState({ action, seconds: 30 }); }, 0);
  }
```

`cancelled` 与 `skipped` 被计入"全部完成"。用户主动停止整批任务后，`snapshot.running` 由 true→false，条件成立。

**触发场景**：设置 `postExtractionAction = shutdown`，添加一批任务后意识到选错，逐个/整批取消 → 30 秒倒计时开始，机器关机。

**影响**：与用户意图直接相反的高影响行为（未完成解压就关机，且取消的语义是"别做了"而非"做完了"）。虽有 30 秒倒计时可中断，但属于危险默认。

**修复建议**：终态集合区分"成功完成"与"用户终止"：

```tsx
const successful = ['completed', 'partial', 'salvaged'];
const allFinished = snapshot.jobs.length > 0
  && snapshot.jobs.every(j => [...successful, 'failed'].includes(j.status));
const anyCancelled = snapshot.jobs.some(j => j.status === 'cancelled');
if (allFinished && !anyCancelled) { /* 才进入倒计时 */ }
```

---

### P2-07 三处关机动作裸 `void` 调用，失败完全静默

**位置**：`apps/desktop-ui/src/App.tsx:180`、`197`、`358-360`

**证据**

```tsx
if (prev.seconds <= 1) {
  void api.executePostAction(prev.action);   // 无 catch
  return null;
}
```

`executePostAction` 走 `call()`，失败时 `invoke` 的 rejection 直接抛出；`main.tsx` 未注册全局 `unhandledrejection` 处理。

**触发场景**：倒计时归零或点"立即执行"时，`execute_post_action` 失败（权限不足、系统策略禁用关机、目标目录已被删除）。

**影响**：弹窗消失、界面回到"就绪"、机器既不关机也不报错。用户离席后以为已关机，实际仍在运行——这是本报告中对用户欺骗性最强的失败模式。

**修复建议**：统一走 `run()` 包裹，或至少补 catch 并展示错误：

```tsx
void api.executePostAction(act).catch((err: unknown) =>
  setLocalError(err instanceof Error ? err.message : String(err)));
```

---

### P2-08 「打开输出目录」打开的是最早任务而非最后完成的任务

**位置**：`apps/desktop-ui/src/App.tsx:177-181`

**证据**

```tsx
const lastOutput = snapshot.jobs.find(j => j.outputPath)?.outputPath
  || snapshot.settings.outputRoot;
```

`Array.prototype.find` 返回**首个**匹配项，而变量名与语义都要求"最后一个输出"。

**触发场景**：批量解压 ≥2 个任务、`outputRoot` 为空（各自输出到源目录旁）、`postExtractionAction = openOutput`，全部完成后。

**影响**：资源管理器被打开到队列中最早那个任务的目录，多批次场景下几乎必然指错，且无提示，用户可能误以为后续任务输出丢失。

**修复建议**：按完成时间取最新：

```tsx
const lastOutput = [...snapshot.jobs]
  .filter(j => j.outputPath)
  .sort((a, b) => (b.finishedAt ?? b.createdAt).localeCompare(a.finishedAt ?? a.createdAt))[0]
  ?.outputPath || snapshot.settings.outputRoot;
```

---

### P2-09 解压密码经 7-Zip 命令行参数传递，可被同机进程读取

**位置**：`crates/infrastructure/src/engine/cli_engine.rs:69-71`、`154-158`、`185-189`

**证据**

```rust
if let Some(pwd) = &request.password {
    cmd.arg(format!("-p{}", pwd));    // 密码成为 argv 的一部分
} else {
    cmd.arg("-p-");
}
```

`inspect_cli`、`probe_cli`、`extract_cli` 三处都是同一写法。7-Zip 没有提供"从 stdin 读密码"的稳定接口，因此这不是随手可改的编码问题，但风险是真实的。

**触发场景**：同机低权限进程枚举进程命令行（`NtQueryInformationProcess` / WMI / ETW），或在崩溃转储、进程审计日志、EDR 遥测中留存。

**影响**：解压密码（常常是用户密码库中的复用密码）以明文出现在进程命令行，生命周期覆盖整个解压过程。

**修复建议**：在风险说明中如实标注，并优先考虑：① 对 `-p` 参数做一次性化处理——仅在 spawn 的极短窗口内存在；② 长期方案是优先走 native 引擎（进程内解密，不落 argv）；③ 至少在 UI 的"密码库"说明中提示"7z 引擎会将密码传给子进程"。

---

### P2-10 `find_7z_binary` 接受 PATH 中的相对名称，存在解释器劫持面

**位置**：`crates/infrastructure/src/engine/cli_engine.rs:16-55`

**证据**

```rust
if let Ok(custom) = std::env::var("OPENUNPACK_7Z_PATH") { ... }   // 任意路径，无签名校验
let candidates = ["7z", "7za", "7zr", "p7zip", ...];
for candidate in candidates {
    let p = Path::new(candidate);
    if p.is_file() { return Some(p.to_path_buf()); }              // 相对路径 → 受 CWD 影响
    if let Ok(output) = std::process::Command::new(candidate).arg("--help").output() { ... }  // PATH 查找
}
```

Windows 的 `CreateProcess` 搜索顺序包含**当前工作目录**；`is_file()` 对相对名同样以 CWD 为基准。

**触发场景**：用户从"下载目录"里双击打开一个归档（应用 CWD 跟随该目录），目录中存在攻击者放置的 `7z.exe`；或 `OPENUNPACK_7Z_PATH` 被外部启动器设置。

**影响**：应用以当前用户身份执行攻击者二进制，等同于代码执行。虽然需要"用户在被污染目录中启动应用"这一前置条件，但对"解压下载来的压缩包"这一主场景而言并不苛刻。

**修复建议**：只接受绝对路径，并优先使用随包分发的引擎：

```rust
let p = Path::new(candidate);
if !p.is_absolute() { continue; }        // 拒绝 "7z"/"7za"
if !p.is_file() { continue; }
// 校验同目录下 7z.dll 的签名/哈希，或仅允许 app dir 下的副本
```

删除对裸名称的 `Command::new(candidate)` 探测（它本身就会执行候选程序）。

---

### P2-11 CLI 引擎计算文件哈希时吞掉读取错误，可能把前缀哈希当作完整校验值

**位置**：`crates/infrastructure/src/engine/cli_engine.rs:223-236`

**证据**

```rust
let mut file = File::open(path)...?;
let mut hasher = Sha256::new();
let mut buf = [0u8; 32 * 1024];
while let Ok(n) = file.read(&mut buf) {     // ← 读取错误被吞，循环直接结束
    if n == 0 { break; }
    hasher.update(&buf[..n]);
}
hex::encode(hasher.finalize())              // 得到的是"已读前缀"的哈希
```

同一 `FileRecord` 的 `size` 取自 `metadata.len()`（完整长度），于是出现"size 是全长、sha256 只是前缀"的不一致记录。

**触发场景**：文件被其他进程锁定/截断、网络盘瞬时断连、磁盘读取错误（`ERROR_CRC`）——7-Zip 已成功写出，随后哈希阶段读失败。

**影响**：清单中记录了一个错误的校验值。若后续 readback 用该值比对，会报"校验失败"并可能触发重试/降级；若仅作记录，则完整性校验形同虚设。属于"静默产生错误证据"，比直接失败更危险。

**修复建议**

```rust
loop {
    match file.read(&mut buf) {
        Ok(0) => break,
        Ok(n) => hasher.update(&buf[..n]),
        Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
        Err(e) => return Err(format!("IoError: {e}")),
    }
}
```

---

### P2-12 便携密码库的密钥可被明文文件或环境变量替换

**位置**：`crates/infrastructure/src/dpapi/vault.rs:2001-2016`

**证据**

```rust
fn resolve_vault_key(path: &Path) -> Result<Zeroizing<Vec<u8>>, String> {
    if let Ok(env_key) = std::env::var("OPENUNPACK_VAULT_KEY") { ... return Ok(...); }   // 环境变量优先
    let key_file = parent.join(".vault.key");
    if key_file.exists() {
        if let Ok(bytes) = fs::read(&key_file) { if !bytes.is_empty() { return Ok(...); } }  // 明文密钥文件
    }
```

**触发场景**：攻击者具备应用数据目录写权限（同用户恶意程序、同步盘、备份还原），投放一个自选内容的 `.vault.key`；应用随后以该密钥加密新写入的密码条目。

**影响**：DPAPI 的"绑定当前用户"保护被静默降级为"目录内明文密钥"，攻击者可离线解密此后新增/更新的全部条目。这是设计上的便携模式，但缺少**密钥来源绑定**（不校验文件权限/属主、不与 DPAPI 记录的指纹比对）与 UI 提示。与 P2-03 同属"环境变量可改变安全语义"家族。

**修复建议**：便携模式改为显式开关（UI 勾选"创建可移植密码库"才允许 `.vault.key`），并对密钥文件校验 ACL（仅当前用户可读）；普通模式下若发现该文件存在，应报错而非静默采用。

---

### P2-13 CLI 列表输出按字节截断文件名，中文名必崩

**位置**：`crates/cli/src/main.rs:271-275`

**证据**

```rust
let mut name = j.source.display_name.clone();
if name.len() > 28 {          // 字节长度
    name.truncate(25);        // 按字节截断 → 落在 UTF-8 字符中间就 panic
    name.push_str("...");
}
```

`String::truncate` 在非字符边界处会 **panic**（不是返回错误）。

**触发场景**：`openunpack list`（非 `--json`）且任一任务的文件名含中文/日文/emoji，且字节长度 > 28。中文名 10 个字即 30 字节，极易触发。

**影响**：CLI 直接 panic 退出，退出码异常，脚本化调用中断。这是本项目面向中文用户的默认场景，不是边缘情况。

**修复建议**

```rust
let mut name: String = j.source.display_name.chars().take(25).collect();
if name.chars().count() < j.source.display_name.chars().count() { name.push_str("..."); }
```

并补一条含中文文件名的 CLI 快照测试。



## 4. P3 级问题

### P3-01 Worker 退出未确认时，缓存配额预留永久占用直至重启

**位置**：`crates/core/src/coordinator/cache_budget.rs:250-268`

**证据**

```rust
impl Drop for BudgetClaim {
    fn drop(&mut self) {
        if let Some((owner, job_id)) = &self.owner
            && owner.retain_if_unconfirmed(job_id)
        {
            // 保留配额，等待进程确认退出
            return;          // ← reservation 未被 take，也没有其他释放入口
        }
        if let Some((root, amount)) = self.reservation.take() { /* 扣减 */ }
```

`retain_if_unconfirmed` 返回 true 时，`BudgetClaim` 随即析构，但 `reservation` 字段未被取出，账本中的额度再无人扣减。全仓仅 `release_unknown_owner`（resources.rs:241）清理 `unknown_owners`，**不触碰 ledger**。

**影响**：Worker 异常退出（崩溃/强杀）后，该笔额度在当前进程生命周期内永久计入 `reserved`，导致后续 SSD 缓存任务被误判为 `CacheQuotaContention` / `ResourceLimit` 而拒绝，直到重启应用。

**修复建议**：为"保留"路径登记一个可回收的延期释放项（例如把 `(root, amount)` 移入 `pending_reclaim`，由 worker 退出确认回调或任务清理时扣减）；或在 `claim` 计算 `reserved` 时以 `min(ledger, 实际 tree_size)` 做一次对账。

---

### P3-02 归档浏览器搜索未 defer，每次按键重建整棵树

**位置**：`apps/desktop-ui/src/components/ArchiveBrowser.tsx:244-260`

**证据**：`search` 直接参与 `useMemo` 依赖，单键触发 `filter`（≤10,000 条）→ `buildTree` 全量重建 → `collectKeys` 再遍历全树。同项目 `QueuePage.tsx:18`、`VaultPage.tsx:24`、`HistoryPage.tsx:28` 均已使用 `useDeferredValue`，此处遗漏。

**修复建议**：`const deferredSearch = useDeferredValue(search);`，输入框仍绑定即时值以保持回显。

---

### P3-03 空队列拖拽区键盘不可达

**位置**：`apps/desktop-ui/src/components/QueuePage.tsx:303-318`

**证据**：`<div role="region" onClick={...}>`，无 `tabIndex`、无 `onKeyDown`，不进入 Tab 序。

**修复建议**：改为 `<button type="button">` 或补 `role="button" tabIndex={0}` + Enter/Space 处理（WCAG 2.1.1）。

---

### P3-04 完成钩子存在第二份死代码实现，且从环境变量取命令

**位置**：`crates/core/src/coordinator/pipeline_helpers.rs:505-555`

**证据**：`#[allow(dead_code)] pub(super) fn trigger_on_complete_hook` 与 `scheduler.rs:768` 的同名函数重复实现，且命令来源是 `std::env::var("OPENUNPACK_ON_COMPLETE")`，直接 `cmd /C <整个环境变量>`。

**影响**：当前无调用方，不构成可利用漏洞；但一旦被误接回调用链，就是"环境变量 → 任意命令执行"。两套实现并存也违反项目自身的 MIRROR/去重约定。

**修复建议**：删除该副本，只保留 scheduler 中的实现，并在其中使用环境变量传**数据**而非命令。

---

### P3-05 状态迁移校验器无生产调用点，非法迁移缺少统一把关

**位置**：`crates/domain/src/jobs/state_machine.rs`；引用面 `crates/domain/src/jobs/model.rs:2`

**证据**：全仓检索 `StateMachine|state_machine`，除定义文件本身外，只有 `jobs/mod.rs` 的再导出与 `model.rs` 对 `ExecutionStage`/`JobStatus` **类型**的引用：

```
crates/domain/src/jobs/mod.rs:3:pub mod state_machine;
crates/domain/src/jobs/mod.rs:7:pub use state_machine::*;
crates/domain/src/jobs/model.rs:2:use super::state_machine::{ExecutionStage, JobStatus};
```

`crates/core` 的 `Cargo.toml` 并不依赖 `openunpack-domain`（依赖方为 `application` / `infrastructure` / `desktop-host`），实际状态迁移由 core 的 `commands.rs` / `scheduler.rs` 各自在命令实现中把关。

**影响**：不存在统一的"状态 + 事件 → 新状态"判定表，新增迁移点时容易漏校验（例如对 `cleanupPending` 任务重复触发 retry）。当前未见具体越权迁移，属结构性风险而非已确认缺陷。

**修复建议**：把迁移表下沉到 `core` 实际使用的位置（或在 core 中引入 domain 并改为唯一入口），并补表驱动测试：对每个 (状态, 事件) 组合断言允许/拒绝。

---

## 5. 复核后未采纳的候选（避免误报）

审计过程中有 4 条候选经复核被降级或否决，记录于此以便复现判断过程：

| 候选 | 初判 | 复核结论 |
|------|------|----------|
| 前端 `invoke` 错误被通用文案覆盖（api.ts:83） | P2 | **不成立**。`apps/desktop-host/src/lib.rs` 中 92 个 `#[tauri::command]` 的错误类型全部是 `String`（`grep -oE "Result<[^>]*, \w+>"` 结果：92/92 为 `String>`），Tauri 以字符串拒绝，`typeof e === 'string'` 分支必然命中，真实原因可正常透出。仅建议保留结构化错误的兜底提取。 |
| `zip` 三条 advisory → 路径穿越 | P1 | **降级为依赖问题**。`extract_zip` 走手动 `by_index` + `File::create`，不调用 `ZipArchive::extract()`、不创建符号链接，`validate_relative_path`（path_safety.rs:4-52）已拒绝 `..`、绝对路径、`:`、保留设备名、尾随点/空格、深度 >128。攻击路径不可达，但必须升级（见 P1-04）。 |
| SQL 注入 | P1 | **不成立**。全仓唯一的 `format!("SELECT ...")` 在 `crates/core/tests/architecture_convergence.rs:179`（测试代码，表名来自常量）。仓储层全部走预编译参数。 |
| `zip_encoding.rs` 解析不可信字节可能 panic | P2 | **不成立**。EOCD 搜索、46 字节头、name/extra/comment 长度均有边界校验（:43-45、:96-98、:113-115），`cd_offset + cd_size` 由上界约束不溢出，未发现可越界切片。 |

---

## 6. 已确认的防护（本轮检查未见问题）

| 面 | 结论 | 证据 |
|----|------|------|
| Zip Slip / 路径穿越 | 双栈纵深：原生引擎 `validate_relative_path` + 协调器 handle-based 目录守卫 | `path_safety.rs:4-52`、`paths.rs:43-544`、`info.rs` 重解析点逐级拒绝 |
| 符号链接 / Junction | 扫描、工具箱、路径校验、数据库锁、环境探测全部显式拒绝 `FILE_ATTRIBUTE_REPARSE_POINT` | `scanner.rs:264`、`toolbox.rs:223`、`paths.rs:902`、`database_lease.rs:38` |
| 本地 API 暴露面 | 仅 `127.0.0.1`；Host 唯一且必须匹配端口；非 WS 端点拒绝全部 query；Origin 白名单；CORS 头不携带凭据 | `server.rs:267,278,657-700,761-770` |
| Token 校验 | 格式/长度预校验 + SHA-256 摘要 + 恒定时间 XOR 累加 + 失败节流 | `store.rs:275-338` |
| 本地 API 配置持久化 | 拒绝符号链接/重解析点、1 MiB 上限、schema 与 id 唯一性校验 | `store.rs:125-182` |
| 权限作用域 | 命令→作用域映射集中定义，HTTP 层统一强制 | `command.rs:330-369`、`server.rs:900` |
| 7-Zip 参数注入 | 参数以独立 argv 传入，目标目录 `-o<path>` 与密码 `-p<pwd>` 不经 shell | `cli_engine.rs:67-74,154-158,180-191` |
| 重命名规则 | 结果经 `relative()` 二次校验，拒绝分隔符、保留名、尾随点 | `toolbox.rs:533-546`、`paths.rs:913-955` |
| 前端 XSS | 全目录无 `dangerouslySetInnerHTML` / `innerHTML` / `eval` / `srcdoc`；用户数据均走文本节点 | 全量 grep 零命中 |
| 前端监听与定时器 | `matchMedia` / `blur` / `visibilitychange` / `setInterval` / Tauri `listen` 均有 cleanup | 逐文件核对 |
| 快照竞态 | revision + session 单调游标 + epoch 守卫 | `App.tsx` refresh、`QueuePage` scanEpoch |
| MCP 连接器 | URL 严格白名单（仅 `http://127.0.0.1[:port]`）、拒绝重定向、20 MiB 响应上限、日志脱敏 | `mcp-connector/src/api.ts:16-29,67-118` |

---

## 7. 与历史审计的交叉核对

| 历史项 | 状态 | 本轮结论 |
|--------|------|----------|
| A1 双轨镜像 ~2,900 行复制 | 台账 §18 已闭环 | 未复发；`database_lease` 双写带 MIRROR 横幅 |
| A2 `openunpack-core` 残留依赖 | 已切除 | 未复发 |
| SEC-01 全局清空历史需管理员 | 已修 | 复核通过（`local_api.rs:699-707`） |
| SEC-02 picker 授予持久化 | 已废弃 | 复核通过（`store.rs:344-381` 标注 deprecated） |
| SEC-03 设置注入守卫 | 已修但**覆盖不全** | **本轮 P1-01**：漏掉 `on_complete`；**P1-06**：`submit_input` 未复用 `ClientJobPolicy` |
| SEC-04 / D1–D6 / S-01–S-24 | 已闭环 | 未发现复发 |
| R2 毒化锁返回伪条目 | 已修 | `cache_budget.rs:67,75` 改用 `PoisonError::into_inner`；但 `Drop` 路径仍有 P3-01 的额度滞留 |

---

## 8. 修复路线图

### 立即（阻断发布）

1. **P1-04** 升级 `zip >= 9.0.0` 并补 symlink/SUID 回归用例——当前 CI 必红，属发布硬阻断。
2. **P1-05** 作用域校验下沉到 `backend.execute`，WS 不再直连 backend——这是整套令牌模型的地基。
3. **P1-06** `submit_input` 前置 `policy.authorize(job_id)`，删除写全局密码库的副作用。
4. **P1-07** `add_jobs` 改为"先校验后使用"，不得 `grant_*` 反向扩权。
5. **P1-01** `on_complete` 加入 SEC-03 钉死列表；删除 `cmd /C <任意字符串>` 兜底分支。
6. **P2-03** 信任开关从环境变量改为显式启动参数（仅 `--headless` 可用 + 审计日志 + 移除 `-y` 别名）。

### 本周（安全与正确性）

7. **P1-02 / P1-03** 守护进程 Token 移出 `localStorage` 与 URL query；非回环地址强制 HTTPS 并给危险提示。
8. **P1-08** 排除规则在 CLI 引擎补齐（`-x!` / `-i!` 或后置清理），并加跨引擎一致性测试。
9. **P2-01** 钩子参数改环境变量传递，消除 `cmd.exe` 二次解析。
10. **P2-02** 循环陷阱判定归一化大小写 + 补单测。
11. **P2-05 / P2-06 / P2-07** 前端三处逻辑缺陷（密码重试死路、取消后关机、静默失败）。
12. **P2-04** 扫描条目硬上限；**P2-10** 7z 解析改为绝对路径 + 签名/哈希校验。

### 下个迭代（健壮性）

13. **P2-08 / P2-09 / P2-11 / P2-12 / P2-13** 输出目录选择、密码 argv 风险、哈希吞错、便携密钥绑定、CLI 中文截断。
14. **P3-01** 配额账本对账；**P3-02 / P3-03 / P3-04 / P3-05** 性能、无障碍、死代码与状态机收敛。

---

## 9. 附录：关键证据

```bash
# 依赖漏洞（退出码 1 = CI 门禁会失败）
$ cargo audit >/dev/null 2>&1; echo $?
1

# 告警明细
$ cargo audit 2>/dev/null | grep -E "^(Crate|ID|Severity|Solution)"
Crate: zip  8.6.0   ID: RUSTSEC-2026-0369   Severity: 9.1 (critical)
Crate: zip  8.6.0   ID: RUSTSEC-2026-0367   Severity: 7.3 (high)
Crate: zip  8.6.0   ID: RUSTSEC-2026-0368

# 豁免白名单不含 zip
$ sed -n '81,87p' scripts/security-audit-policy.ps1
@{ id = 'RUSTSEC-2024-0370'; package = 'proc-macro-error'; ... }
@{ id = 'RUSTSEC-2024-0429'; package = 'glib'; ... }
# （无 zip 相关条目）

# Tauri 命令错误类型全为 String（用于否决"错误被吞"候选）
$ grep -oE "Result<[^>]*, [A-Za-z_]+>" apps/desktop-host/src/lib.rs | sed 's/.*, //' | sort | uniq -c
     92 String>

# 作用域闸门只在 HTTP dispatch 中出现一次，WS 直连 backend
$ grep -n "command" crates/local-api/src/server.rs | grep -n "scopes()"     # 900 (dispatch 内)
$ grep -n "backend.execute" crates/local-api/src/server.rs                  # 1122 / 1136 / 1150

# 排除规则：ZIP 引擎有，CLI 引擎没有
$ grep -c "matches_exclusion\|request.inclusions" crates/infrastructure/src/engine/zip_engine.rs   # 2
$ grep -c "exclusions\|inclusions" crates/infrastructure/src/engine/cli_engine.rs                 # 0

# CLI 非字符边界截断
$ grep -n "truncate(25)" crates/cli/src/main.rs                                    # 273

# 状态机无生产调用点
$ grep -rn "StateMachine\|state_machine" --include="*.rs" crates apps | grep -v state_machine.rs
crates/domain/src/jobs/mod.rs:3:pub mod state_machine;
crates/domain/src/jobs/mod.rs:7:pub use state_machine::*;
crates/domain/src/jobs/model.rs:2:use super::state_machine::{ExecutionStage, JobStatus};
```

---

*本报告只登记有代码证据的问题；每条结论均可按"位置"列直接定位复核。*

---

## 11. 修复执行情况

修复记录、变更影响面与验证证据见 `docs/audit-fix-log-2026-10-20.md`：

- **P1：8/8 已修复**；
- **P2：11/13 已修复**（仅 P2-09 密码传入 argv、P2-12 便携密钥绑定遗留，原因见修复日志）；
- 另修复 1 项修复轮中新发现的缺陷：`--on-complete` 只写环境变量、无读取方，导致无头钩子完全不生效；
- 验证：`cargo check` 4 包通过；`intake_guard` 5/5、`cli_engine` 2/2、`authorization_tests` 2/2 单测通过；前端 `eslint` 与 `tsc -b` 通过；依赖审计告警清零（zip 9.0.5，`cargo audit` exit 0）。
