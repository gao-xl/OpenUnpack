# OpenUnpack (解压工坊) 全维度代码审计与安全评估报告

**评估基准时间**：2026-10-20  
**报告版本**：v1.0.0 (Release Candidate Audit)  
**审计范畴**：跨语言全栈架构（Rust 核心与宿主、C# .NET 10 执行引擎、React 19 / TypeScript 5 交互端、Local API 与 MCP 连接器、构建验证脚本）  
**审计标准**：遵循通用安全威胁建模（STRIDE）规范与代码质量工程准则  
**最终审计裁决**：**通过（附带非阻塞整改项，Pass with Non-Blocking Actions）**  

---

## 目录

1. [执行摘要与审计裁决](#一执行摘要与审计裁决)
2. [系统架构与威胁模型审查](#二系统架构与威胁模型审查)
3. [分层代码质量与安全性审查](#三分层代码质量与安全性审查)
   - 3.1 本机 HTTP API 与鉴权网关
   - 3.2 Rust 核心协调中心与状态机
   - 3.3 C# 原生执行进程与 7-Zip 沙盒
   - 3.4 桌面 UI 前端与 WebView2 通信
   - 3.5 供应链与第三方依赖合规
4. [关键问题与缺陷深度剖析](#四关键问题与缺陷深度剖析)
5. [缺陷与技术债务台账清单](#五缺陷与技术债务台账清单)
6. [整改实施建议与演进路线](#六整改实施建议与演进路线)

---

## 一、执行摘要与审计裁决

### 1.1 总体评价
OpenUnpack 项目展现了极其严谨的跨语言工业级系统工程水准。系统构建了由“**Win32 进程隔离（Job Object + 令牌特权剥离） + 目录句柄固定（PathIdentityLease 防 TOCTOU） + 本地凭据硬件级保护（DPAPI + 内存即时清零 Zeroize） + 严格状态单源（SQLite WAL + 租约锁）**”组成的多重防御体系。

经深度代码审查，未发现任何可利用的高危远程代码执行（RCE）、非受控越权读写或凭据明文泄露风险。代码库各层边界清晰，数据流与异常处理完备。

### 1.2 审计判定状态
- **安全性判定**：**PASS（通过）**  
  核心安全机制设计健全，无阻断上线的 Critical / High 级安全漏洞。
- **发布准备判定**：**CHANGES REQUESTED（需整改后发布）**  
  存在 3 项影响自动化打包与 CI 严格门禁的已知问题（FT-001、FT-002、FT-004），需在正式发布前核销。

---

## 二、系统架构与威胁模型审查

```
[ 外部非受信域 (Untrusted Input) ]
  ├── 恶意压缩包 (ZIP / 7z / RAR: 路径遍历、压缩炸弹、符号链接劫持、Zone.Identifier 逃逸)
  ├── 批量密码文件 (TXT / TSV / CSV: 畸形格式、超长字段缓冲区溢出)
  └── 本机 HTTP 请求 / MCP 智能体输入 (未授权调用、NTFS ADS 逃逸、暴力枚举、DoS)
        │
        ▼ (第一道边界：Host 鉴权与前置校验)
[ 宿主进程 (Desktop Host & Local API) ]
  ├── 独立 API Token 认证 + 滑动退避限流 + RFC 7807 统一错误响应
  ├── 细粒度 Scope 鉴权 (11 项权限) + NTFS ADS 冒号逃逸拦截 + 路径大小写规范化
  └── 统一状态推进 (SQLite WAL 事务 + 进程独占锁租约)
        │
        ▼ (第二道边界：双向命名管道 + Windows Job Object)
[ 独立沙盒执行进程 (OpenUnpack.Worker) ]
  ├── 令牌特权显式剥离 (AdjustTokenPrivileges)
  ├── 进程加固策略 (禁止网络镜像加载 + 禁用扩展点)
  ├── 运行时写范围守卫 (WriteScopeGuard 强制仅限 Workspace 写入)
  └── 目录句柄固定防竞争 (PathIdentityLease 阻止中途目录替换)
        │
        ▼ (第三道边界：COM / P-Invoke 互操作)
[ 底层解码引擎 (官方未修改 7z.dll 26.04) ]
```

### 2.1 资产与防护对策
1. **密码库与核心凭据**：
   - 存储：受 Windows DPAPI 保护，与当前 Windows 用户 SID 强绑定；
   - 内存生命周期：Rust 敏感结构使用 `Zeroizing<T>`，C# 端在解压完成后立即调用 `CredentialSecurity.ZeroizeString` 擦除非托管内存；
   - 日志脱敏：全链路控制台与文件日志严禁输出密码原文、备份口令或用户绝对路径，Worker 使用固定结构的 `SafeDiagnostics`。
2. **本地文件系统完整性**：
   - 路径限制：相对路径严格限制 128 层，目录租约限制 256 层，超限直接阻断；
   - 绝不回滚误删：解压失败或取消时，源文件与半成品现场独立保全，杜绝误伤用户已有数据。

---

## 三、分层代码质量与安全性审查

### 3.1 本机 HTTP API 与鉴权网关 (`crates/local-api` & `desktop-host/src/local_api.rs`)
- **优点**：
  - 全面支持 RFC 7807 标准错误模型与细粒度字段级校验（`FieldError`）；
  - `check_client_path` 严格拦截盘符之外的冒号（`:`），彻底防御 NTFS 备用数据流（ADS）越权；
  - 针对 Windows 路径大小写不敏感特性，实现了大小写折叠规范化匹配（`path_starts_with_case_insensitive`）。
- **待改进点**：
  - 未认证请求在丢弃请求体时的并发缓冲较小（`rejected_bodies` 仅 16 许可），且丢弃超时较短（250ms），在高并发测试下易触发 Winsock 10053 异常。

### 3.2 Rust 核心协调中心与状态机 (`crates/core`, `crates/domain`, `crates/application`)
- **优点**：
  - 单一状态机权威设计，所有任务状态跃迁严格由 SQLite 事务驱动；
  - 包含完整的物理磁盘介质感知能力（Win32 `IOCTL_STORAGE_GET_DEVICE_NUMBER`），智能降级机械硬盘并发度；
  - 零 IO 领域模型设计完备，包含严格的去空格、大小写与文件名内嵌密码派生规则。
- **待改进点**：
  - `crates/core/src/coordinator.rs` 达 8,728 行代码，承担了调度、持久化、拓扑、文件监控、垃圾回收等多重职责，维护成本较高；
  - 存在 5 组双轨镜像代码（依赖 `scripts/check-mirrors.mjs` 防漂移），需加速向分层架构收敛。

### 3.3 C# 原生执行进程与 7-Zip 沙盒 (`src/OpenUnpack.Worker` & `OpenUnpack.Engine`)
- **优点**：
  - `WorkerSandbox.cs` 实现了系统级纵深防御，显式移除非必要令牌特权并启用防注入策略；
  - `PathIdentityLease.cs` 在创建各级目录时持有只读共享句柄，锁死重命名与删除，有效切断了利用 Junction / 软链接实现的 TOCTOU 竞争覆盖攻击；
  - 纯托管 LZ4 解码器实现健全，支持帧校验与内容校验。
- **待改进点**：
  - 打包暂存物料中的 .NET 运行时版本为 10.0.12，未及时更新至基线要求的 10.0.13。

### 3.4 桌面 UI 前端与 WebView2 通信 (`apps/desktop-ui`)
- **优点**：
  - 声明式 React JSX 渲染，完全无 `dangerouslySetInnerHTML` 滥用，XSS 风险为零；
  - 响应式遥测指标卡与等宽字体设计（`.tabular-nums`），杜绝频繁刷新引发的 DOM 抖动；
  - 静态检查严密：`eslint` 0 警告 0 错误通过，TypeScript 严格类型检查通过。

### 3.5 供应链与第三方依赖合规 (`Cargo.lock`, `npm`, 7-Zip)
- **优点**：
  - 7-Zip 动态链接库使用官方未经修改的二进制，严格遵从 LGPL 2.1 规范与 unRAR 限制条款；
  - 具备自动化物料审计脚本（`audit-source.mjs`, `collect-release-materials.mjs`）。
- **待改进点**：
  - Cargo 依赖树存在 3 项 Linux D-Bus 相关的非活跃依赖（`zbus_names`、`zvariant` 等），虽在 Windows 下不可达，但影响严格 `cargo audit` 门禁。

---

## 四、关键问题与缺陷深度剖析

### 4.1 FT-004：未认证请求连接间歇性中止 (Winsock 10053)
- **级别**：`Medium`（可用性与网络健壮性）
- **影响位置**：`crates/local-api/src/server.rs:486-531` (`reject_request`)
- **根本原因**：
  当调用方发起未携带有效 Token 的 POST 请求（如 `/api/v1/commands/job_details`）且正文较多时，若 `state.rejected_bodies` 信号量耗尽或在 250ms 内未能排空 Body，服务端会在丢弃未竟数据后立即关闭 TCP 套接字。
  Windows Winsock 协议栈在有未读入数据时执行关闭操作会向对端发出 RST 报文，导致客户端接收缓冲区被刷空，抛出 `WSAECONNABORTED (10053)`，使得客户端未能正常解析服务端返回的 HTTP 401 响应头。
- **修复方案**：
  放宽丢弃请求体的并发限制与等待时间，确保在发送 401 响应后对连接执行标准的 FIN 挥手流程；或在客户端针对 10053 错误增加幂等重试。

### 4.2 FT-002：代码格式门禁差异
- **级别**：`Nit / Blocker for CI`
- **影响位置**：`apps/desktop-host/src/lib.rs`
- **根本原因**：
  近期新增的休眠/关机命令及 Windows 资源管理器上下文菜单注册代码未按照 Rust 2024 默认代码格式规范换行。
- **修复方案**：
  在代码库根目录运行 `cargo fmt --all` 即可一键恢复合规。

---

## 五、缺陷与技术债务台账清单

| 编号 | 类别 | 严重程度 | 置信度 | 涉及模块 | 核心问题描述 | 处理建议 |
|:---:|:---:|:---:|:---:|:---|:---|:---|
| **FT-004** | 网络与协议 | `Medium` | 95% | `crates/local-api` | 丢弃未认证 Body 过急导致 Winsock 10053 连接中止 | 平滑处理丢弃正文并标准挥手 |
| **FT-001** | 构建与物料 | `Low` | 100% | `artifacts/worker` | 暂存载荷为 .NET 10.0.12，未升级至 10.0.13 | 重新执行 `prepare-desktop.ps1` |
| **FT-002** | 静态检查 | `Nit` | 100% | `desktop-host` | `apps/desktop-host/src/lib.rs` 存在格式差异 | 运行 `cargo fmt --all` |
| **FT-003** | 供应链合规 | `Low` | 100% | `Cargo.lock` | 3 项 Linux 平台的 unmaintained 依赖告警 | 评估豁免策略或升级依赖 |
| **ARCH-01**| 架构可维护性| `Warning`| 90% | `crates/core` | `coordinator.rs` 超过 8700 行，单体职责过重 | 推进拆解至 `domain`/`application` |
| **ARCH-02**| 架构可维护性| `Warning`| 95% | `infrastructure` | 5 处双轨镜像文件依靠脚本硬性核验 | 迁移完成后退役旧镜像 |

---

## 六、整改实施建议与演进路线

### 6.1 阶段一：发布前阻塞项核销 (预计耗时 < 1 工作日)
1. 运行 `cargo fmt --all` 自动格式化全部 Rust 源码，解除 FT-002；
2. 运行 `./scripts/prepare-desktop.ps1` 重新打包包含 .NET 10.0.13 的 Worker 产物，解除 FT-001；
3. 调整 `server.rs` 中的 `reject_request` 逻辑，消除 10053 异常，解除 FT-004；
4. 重新执行 `./scripts/verify.ps1 -SkipInstall -WorkerProtocol` 进行全量门禁验收。

### 6.2 阶段二：架构解耦与核心退役 (预计在 0.2.0 版本落地)
1. 按照既定重构计划，将 `crates/core/src/coordinator.rs` 拆分为 `crates/domain`（纯业务规则）与 `crates/application`（协调编排）；
2. 消除 `scripts/check-mirrors.mjs` 中的 5 组双轨镜像，统一使用 `crates/infrastructure` 中的标准实现。

---

**报告编制人**：OpenUnpack 自动化审计与安全架构团队  
**归档位置**：`docs/reports/2026-10-20-code-audit-report.md`
