# OpenUnpack (解压工坊) 架构重构与系统技术白皮书

> 2026-10-20：本文保留历史设计/迁移背景；当前源码归属及运行链以 [现行架构](architecture-current.md) 为准。Core 已明确为唯一生产编排层，未装配的 TaskCoordinator / SQLite v2 原型退役，底层镜像收敛为 Infrastructure 单源实现。旧的 Core 整体退役及镜像双写要求不再作为当前执行指引。

**文档版本**：v2.2.1（状态与协议校正）  
**核对日期**：2026-10-10  
**架构状态**：分层模块已建立，桌面生产入口仍装配 `openunpack_core::Coordinator`。本轮正在修复生产与迁移问题，最终测试门禁及桌面发行验收待核销；不据历史测试片段宣布 W0～W6 完成或 R01～R15 全部清零。  
**目标读者**：系统架构师、后端/客户端工程师、测试开发工程师、技术决策者

当前问题基线及此前验证见 [项目总结](project-summary-2026-10-10.md)，本轮修复、静态复核与最终证据边界见 [修复记录](issue-fixes-2026-10-11.md)。下文保留分层设计及历史示例；标为目标的机制须完成运行对等、迁移和验收后才能作为实际生产架构。

实际运行链：`React UI → Tauri Host → Rust Core Coordinator → 认证双命名管道 → .NET Worker → OpenUnpack.Engine / 7z.dll`。`TaskCoordinator` 尚未替换生产协调器。开发协作的 Haiku/Sonnet 是 AGENTS 中的目标角色标签；`.pi/agents` 当前配置为 `yarrn/gemini-3.8-flash-high`，本轮 Codex 子 agent 继承可用执行模型，不能按这些角色标签推断实际模型。

---

## 目录

1. [项目背景与重构动因](#1-项目背景与重构动因)
2. [总体系统架构与核心原则](#2-总体系统架构与核心原则)
3. [分层架构与编译边界定义](#3-分层架构与编译边界定义)
   - 3.1 跨语言契约单源 (`crates/protocol`)
   - 3.2 纯业务领域模型 (`crates/domain`)
   - 3.3 应用用例与状态推进权威 (`crates/application`)
   - 3.4 基础设施与外部适配器 (`crates/infrastructure`)
   - 3.5 纯净原生解压引擎 (`src/OpenUnpack.Engine`)
   - 3.6 独立 Worker 进程与宿主装配 (`OpenUnpack.Worker` & `desktop-host`)
4. [核心关键机制设计与实现](#4-核心关键机制设计与实现)
   - 4.1 任务状态机与单状态权威推进
   - 4.2 跨语言契约治理与防漂移机制
   - 4.3 进程隔离、受监督生命周期与 Windows Job Object
   - 4.4 密码安全库与内存租约生命周期
   - 4.5 存储拓扑感知与智能介质路由
5. [跨语言工程结构与构建闭环](#5-跨语言工程结构与构建闭环)
6. [质量保障与测试验证矩阵](#6-质量保障与测试验证矩阵)
7. [生产运维、安全合规与风险防范指南](#7-生产运维安全合规与风险防范指南)

---

## 1. 项目背景与重构动因

### 1.1 项目定位
**OpenUnpack（解压工坊）** 是一款面向 Windows 平台的大规模、高可靠、本地离线批量压缩文件解压系统。系统支持处理复杂的嵌套压缩包、分卷压缩包、加密包（含 ZipCrypto/AES-256、RAR5、7z 等）以及超大文件目录树。前端采用现代 Tauri 2 + React/TypeScript 架构，核心控制由 Rust 编写，底层的物理格式解压依托受监督的独立 .NET/7-Zip 原生引擎 Worker 进程。

### 1.2 历史架构技术债务与重构动因
在项目演进初期，系统采用了紧耦合的原型设计，随着业务边界扩大，暴露出以下核心设计缺陷：

1. **状态写入权威多头混杂**：历史版本中，前端 UI、C# Worker 进度回调与 Rust `Coordinator` 均存在直接修改任务状态的路径，在进程意外中断或异常取消时，极易产生“状态不一致”或“孤儿文件泄漏”。
2. **生产依赖反向渗透与安全风险**：C# Worker 进程原本反向依赖了历史 `OpenUnpack.Core`，而该类库中混合包含了密码库（Vault）、批量任务调度、WPF 遗留原型等上层逻辑，导致仅需原生解压权限的 Worker 进程持有了密码库代码，违反了**最小特权原则**。
3. **跨语言契约漂移（Contract Drift）**：TypeScript 前端、Rust 宿主和 C# Worker 各自独立维护数据结构与枚举定义，缺少统一的权威定义和黄金样本验证，格式兼容性极其脆弱。
4. **单体模块职责膨胀（God Class）**：`coordinator.rs` 承担了状态持久化、路径扫描、解压调度、发布提交、故障恢复与 Win32 IOCTL 探测等超过 2300 行的密集代码，纯业务规则与底层 IO 深度交织，难以实施单元测试。

为解决上述问题，项目按阶段规划（W0 基线建立 → W1 契约单源 → W2 边界与装配 → W3 拆解调度器 → W4 契约硬化 → W5 质量验证与退役）推进基于**模块化单体与端口适配器（Modular Monolith & Ports-and-Adapters）**的重构。新模块存在不代表生产装配、恢复对等和旧实现退役已经完成。

---

## 2. 总体系统架构与核心原则

分层目标遵循六大核心设计原则，实际接入状态以本节说明为准：

1. **单状态机权威（Single State Authority）**：
   当前运行期状态推进由 Core `Coordinator` 负责，目标迁移至 Application 层的 `TaskCoordinator`；持久化权威为 SQLite（开启 WAL + FULL 同步模式）。Worker 的事件通知与前端交互请求作为状态推进输入，不能以 UI 进度直接判定任务完成。
2. **纯领域与零 IO（Zero-IO Domain Core）**：
   `crates/domain` 包含纯业务实体、类型化状态转移矩阵、通配符算法与分卷规律，不依赖文件系统、网络或数据库框架，可进行纯内存单元测试；本节不作全覆盖或耗时保证。
3. **端口与适配器隔离（Ports and Adapters）**：
   应用层定义 Port（Trait），基础设施层提供 SQLite、DPAPI、系统环境与 Worker 适配。过渡期 Infrastructure 的 Worker/Vault 适配仍依赖已验证的旧 Core，不能提前删除该依赖。
4. **跨语言契约单源（Single Source of Truth Wire DTO）**：
   `crates/protocol` 作为 Rust Wire DTO 的权威，桌面端 DTO 与 Worker DTO 分离，采用 `camelCase` 序列化。TS/C# 仍维护对应类型；契约检查的真实覆盖范围须由共享样本及运行结果证明。
5. **Worker 进程级强隔离与最小特权（Worker Isolation & Least Privilege）**：
   剥离纯净的 `OpenUnpack.Engine` 原生引擎类库，Worker 仅链接 Engine，彻底切断对上层密码库与调度库的引用。Worker 生命周期由 Windows Job Object 强绑定，防范孤儿进程。
6. **密码生命周期安全租约（Ephemeral Secret Leases with Zeroize）**：
   密码不写入日志、任务快照或明文数据库。Rust 候选与 IPC 序列化缓冲采用 `Zeroize`/`Zeroizing` 缩短明文驻留；该措施不能保证 C# 托管字符串、用户主动揭示视图或系统内存转储中的所有副本均已清除。

下图描述目标分层；当前 Host 到 Application 的生产连接尚未建立。

```
+-------------------------------------------------------------------------------+
|                       桌面用户界面 (Desktop UI)                               |
|              Tauri 2 + React 19 + TypeScript + Ant Design                     |
+-------------------------------------------------------------------------------+
                                      ▲ (IPC: camelCase JSON / Events)
                                      ▼
+-------------------------------------------------------------------------------+
|                       宿主命令层 (Desktop Host)                               |
|          apps/desktop-host (命令参数校验、Win32 路径提权防护、系统托盘)         |
+-------------------------------------------------------------------------------+
                                      ▲
                                      ▼
+-------------------------------------------------------------------------------+
|                      应用编排层 (OpenUnpack Application)                      |
|      TaskCoordinator (唯一状态推进权威) / JobUseCases / VaultUseCases          |
|                                                                               |
|   [Port: TaskStore]  [Port: VaultStore]  [Port: WorkerSupervisor]  [Port: Env]|
+-------------------------------------------------------------------------------+
         ▲                     ▲                       ▲                  ▲
         |                     |                       |                  |
+--------+--------+   +--------+--------+   +----------+-------+   +------+-----+
|  领域核心层     |   |  基础设施适配器 |   |  跨语言契约单源  |   | 独立 Worker|
| crates/domain   |   | crates/infra    |   | crates/protocol  |   | C# Process |
|                 |   |                 |   |                  |   |            |
| - 纯状态机矩阵  |   | - SQLite (WAL)  |   | - Desktop DTOs   |   | Job Object |
| - 分卷识别规则  |   | - Windows DPAPI |   | - Worker DTOs    |   | 受控会话   |
| - 智能路由决策  |   | - Win32 IOCTL   |   | - WireErrorCode  |   | 仅依赖:    |
| - 候选口令排序  |   | - Named Pipe    |   | - 黄金样本测试   |   | OpenUnpack.|
| - Zeroize 清零  |   |                 |   |                  |   | Engine     |
+-----------------+   +-----------------+   +------------------+   +------------+
```

---

## 3. 分层架构与编译边界定义

系统已划分独立 crate 与程序集。当前依赖为 Application → Domain/Protocol，Infrastructure → Application/Domain/Protocol/Core，Core → Protocol；Protocol 不依赖 Domain。Host 已声明新 crate，但启动仍使用 Core。

### 3.1 跨语言契约单源 (`crates/protocol`)
`crates/protocol` 是整个系统的外围边界类型权威，定义了跨端传输的 Wire DTO。
- **桌面端交互模型 (`desktop.rs`)**：
  - `JobView`：任务在前端视图与列表中的完整投影。
  - `NodeView`：支持多层嵌套压缩包展开的树形节点视图。
  - `AppSnapshot`：按 `sessionId` 与该会话内递增的 `revision` 标识的完整快照；revision 使用十进制字符串传输，避免 JavaScript 数值精度损失。
  - `Settings`：跨端对齐的配置项 DTO。
  - `PasswordView`：脱敏后的密码元数据模型（屏蔽明文口令）。
- **Worker 通信模型 (`worker.rs`)**：
  - `WorkerRequest`：请求 `probe`、`extract`、`copy` 或 `readback`，另有 `cancel`/`ping`/`exit` 控制消息；v2 携带 `operationId`、非零 `generation` 和候选 ID。
  - `WorkerEvent`：进度、心跳、清单产出或错误事件，回传请求的协议版本和身份；v2 返回 `matchedCandidateId`，不回传命中密码明文。
  - `WorkerHello`：Worker 在 v1 `ready` envelope 中附带 v2 能力声明，保持旧客户端兼容。v2 客户端先校验声明，再发送 v2 请求；这不是独立的双向 Hello 交换。后续响应、取消及退出均须匹配请求身份。

### 3.2 纯业务领域模型 (`crates/domain`)
完全遵循**无副作用、零 IO** 原则，任何外部交互均通过数据参数传入。
- **状态转移保护 (`jobs/`)**：
  定义了不可逆的 `JobStatus` 与转移防护矩阵（`can_transition_to`）。终态（`Completed`、`Failed`、`Cancelled`、`Skipped`）必须通过显式重试才能重新转为 `Queued`；正在停止状态（`Stopping`）只能向安全终态收敛。
- **归档规则识别 (`archive/`)**：
  - 通配符排除算法（`matches_exclusion`），支持大小写不敏感的路径与文件名匹配。
  - 自动解包判定（`should_unwrap_single_root`），智能识别顶层单一冗余目录并自动展开。
  - 分卷模式识别（`parse_volume_name`），支持 `.part01.rar`、`.7z.001`、`.r00/.z01` 等经典多卷模式。
- **口令与租约 (`passwords/`)**：
  - `DomainCandidate` 实现底层 `Drop` 特征，自动调用 `Zeroize` 擦除内存明文。
  - 启发式候选排序（`sort_and_dedup_candidates`），基于历史成功命中频次（`success_count`）优先调度，并实施高效去重。
- **资源与路由决策 (`storage/`)**：
  - `decide_route`：基于源盘介质类型（SSD/HDD）与目标盘介质类型，动态决策直通解压或 SSD 缓存加速路由。

### 3.3 应用用例与状态推进权威 (`crates/application`)
编排新分层业务用例，目标成为唯一运行期权威；当前尚未具备替换 Core 全部任务、嵌套、恢复和发布流程的运行对等。
- **端口 Trait 契约 (`ports/`)**：
  - `TaskStore`：定义任务聚合根的持久化操作契约（`load_all_jobs`、`save_job`、`update_job_status` 等）。
  - `VaultStore`：定义安全口令持久化操作契约（`load_decrypt_candidates`、`record_success` 等）。
  - `WorkerSupervisor`：定义 Worker 进程生命周期与管道通信契约。
  - `SystemEnvironment`：定义宿主硬件拓扑探测与防睡眠锁定契约。
- **权威状态协调器 (`use_cases/coordinator.rs`)**：
  `TaskCoordinator` 提供新分层状态与投影接口。目标写入顺序为：**领域状态校验 -> 写入 SQLite -> 更新内存 -> 递增 revision -> 广播前端失效通知**。当前生产 Core 的快照与可见状态写入由统一锁保护；新分层的完整 Worker 驱动与订阅装配仍待实现。

### 3.4 基础设施与外部适配器 (`crates/infrastructure`)
实现应用层定义的各类 Port 接口，隔离 Windows 原生系统调用与外部存储引擎。
- **`SqliteTaskStore`**：
  使用 `rusqlite`，数据库开启 `journal_mode = WAL` 与 `synchronous = FULL`，旧 JobView 迁移以事务执行并保留冻结设置、节点和阶段记录。该设置不能单独证明断电、设备故障或升级回滚零损坏；迁移后记录与旧 Core 的兼容性也须在切换前单独验收。
- **`DpapiVaultStore`**：
  当前为旧 Core `Vault` 的兼容适配，复用 DPAPI 文件格式、原子提交、导入/备份预览、revision 与成功事件去重，保留密码原文。
- **`WindowsSystemEnvironment`**：
  调用 Win32 原生 `GetDiskFreeSpaceExW` 探测可用磁盘容量；通过 IOCTL 查询物理设备号与寻道特征。
- **`LocalWorkerSupervisor`**：
  管理会话线程并调用 Core `WorkerClient`。停止返回成功前必须确认会话线程与子进程退出；退出不确定时保留会话并返回错误，不能据取消标记清理工作区。

### 3.5 纯净原生解压引擎 (`src/OpenUnpack.Engine`)
从原 `OpenUnpack.Core` 彻底剥离出的纯净 .NET 10 类库。
- 仅保留：7-Zip 原生动态链接库包装（`SevenZipEngine.cs`）、COM ABI 交互接口（`SevenZipInterop.cs`）、最小分卷路径开卷解析（`VolumeResolver.cs`）以及引擎专用异常模型（`EngineContracts.cs`）。
- 剔除了任何密码库业务逻辑、批量任务调度器以及上层 UI 逻辑，依赖轻量，无外部框架侵入。

### 3.6 独立 Worker 进程与宿主装配
- **`OpenUnpack.Worker`**：
  作为独立的控制台宿主（`OutputType=Exe`），项目文件仅引用 `OpenUnpack.Engine.csproj`。通过当前用户限定的 control/events 双命名管道传输 UTF-8 JSON-lines，两端校验管道对端 PID。标准输入/输出不承载生产 IPC。大清单写入批准工作目录后封存，Host 验证路径、ID、长度、SHA-256 与有界解析后接收。
- **`apps/desktop-host`**：
  执行前端命令、路径授权、Reparse Point 检查与托盘管理，当前装配 Core `Coordinator`。新分层依赖声明尚不代表宿主已切换。

---

## 4. 核心关键机制设计与实现

### 4.1 任务状态机与单状态权威推进

#### 状态转移全状态矩阵
```
           +----------+
           |  Queued  | <------------------------------------+ (显式重试)
           +----------+                                      |
                 |                                           |
                 v                                           |
           +----------+         (需要用户口令)               |
           | Running  | ---------------------> +---------------+
           +----------+                        | NeedsPassword |
             |   |   |                         +---------------+
 (用户请求停止) |   |   +-------------------+          | (录入口令)
             |   |                       |          |
             v   v                       v          |
     +----------+ +-----------+     +-----------+   |
     | Stopping | | Completed |     |  Failed   | --+
     +----------+ +-----------+     +-----------+   |
          |             ▲                 ▲         |
     (确认退出)          |                 |         |
          v             +-----------------+---------+
     +-----------+
     | Cancelled |
     +-----------+
```

#### 权威推进历史设计示例

下列片段保留早期设计说明，不是当前实现。示例在持久化前修改内存，实际变更应先校验克隆、成功写入后再更新可见状态；不能用此片段替代生产代码核查。
```rust
// crates/application/src/use_cases/coordinator.rs
pub fn transition_job(
    &self,
    job_id: &str,
    next_status: JobStatus,
    stage: ExecutionStage,
    progress: f64,
    message: &str,
) -> Result<(), ApplicationError> {
    let mut jobs_guard = self.jobs.lock().unwrap();
    let job = jobs_guard
        .get_mut(job_id)
        .ok_or_else(|| ApplicationError::JobNotFound(job_id.to_string()))?;

    // 1. 领域层状态机规则转移守卫 (非法转移直接拦截报错)
    job.set_status(next_status)?;
    job.set_stage(stage);
    job.progress = progress;
    job.message = message.to_string();

    // 2. 外部持久化层写入权威 (SQLite WAL + FULL)
    self.task_store
        .update_job_status(job_id, next_status, stage, progress, message)?;

    // 3. 递增快照全局版本号，通知 UI 刷新
    let mut rev = self.snapshot_revision.lock().unwrap();
    *rev += 1;

    Ok(())
}
```

### 4.2 跨语言契约治理与防漂移机制
为检测 TypeScript、Rust 与 C# 的契约漂移，项目维护检查脚本与协议样本。其覆盖范围以实际执行证据为准：
1. **自动化检测脚本 (`scripts/check-contracts.ps1`)**：
   接入 verify/CI，核查类型及共享样本；历史整文件字段存在检查不能证明所属类型、可空性、默认值与真实序列化完全一致。本轮改进须经统一门禁核销。
2. **黄金样本回归测试 (`crates/protocol/tests/golden_samples.rs`)**：
   使用 JSON 样本验证 Rust 序列化及兼容字段。要声称跨语言一致，还需 C#/TS 同一份样本的运行检查。

```rust
// 黄金样本断言规范
#[test]
fn test_worker_request_v1_v2_compat() {
    let raw = r#"{
        "protocolVersion": 2,
        "requestId": "req-99",
        "operation": "extract",
        "archivePath": "C:\\test\\pkg.zip",
        "destinationDirectory": "C:\\test\\out",
        "maxEntries": 1000,
        "maxTotalBytes": 1048576,
        "maxWrittenBytes": 1048576,
        "minFreeBytes": 524288,
        "verification": "standard",
        "exclusions": ["*.tmp"],
        "operationId": "op-99",
        "generation": 1,
        "candidates": [{"candidateId":"cand-1","source":"vault","revision":1,"label":"test","secret":"fixture-only"}]
    }"#;

    let req: WorkerRequest = serde_json::from_str(raw).expect("v2 序列化兼容失败");
    assert_eq!(req.protocol_version, 2);
    assert_eq!(req.candidates[0].candidate_id, "cand-1");
}
```

### 4.3 进程隔离、受监督生命周期与 Windows Job Object
为保障解压极端畸形包或引擎遇到未捕获异常时不拖垮主进程，Worker 运行在受控生命周期模型下：
- **Job Object 树绑定**：
  主进程通过 Win32 API `CreateJobObjectW` 创建专属作业对象，配置 `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`。即使主进程发生不可抗力崩溃，操作系统内核会自动级联终止全部 Worker 子进程，从根本上杜绝孤儿进程占用文件句柄。
- **双向命名管道通信**：
  Worker 创建带 `CurrentUserOnly` 选项的 control/events 两条命名管道；Host 校验服务端 PID，Worker 校验客户端为指定父 PID。v1 ready 附带 Hello；v2 后续事件校验 `requestId`、`operationId` 与 `generation`，不接受其他操作的心跳/进度/终态。
- **受控取消语义 (`Stopping` 状态)**：
  用户取消时，主进程先向 Worker 管道发送友好取消指令；状态机切换至 `Stopping` 状态等待 Worker 清理现场，超时后方由 Job Object 强制接管。

### 4.4 密码安全库与内存租约生命周期
密码是核心敏感资产，系统实施以下深度防御措施：
1. **持久化隔离**：
   利用 Windows DPAPI（`CryptProtectData`），使用当前登录用户主密钥派生的对称密钥加密口令元数据，任何其他系统用户即便读取物理文件也无法解密。
2. **缩短明文驻留与 Rust 候选擦除**：
   Rust 候选封装在带析构擦除的结构体中。以下为字段简化示例，不证明所有托管内存副本均已清除：
   ```rust
   // crates/domain/src/passwords/candidate_policy.rs
   pub struct DomainCandidate {
       pub id: String,
       pub label: String,
       pub secret: String, // 明文承载字段
       pub success_count: u64,
   }

   impl Drop for DomainCandidate {
       fn drop(&mut self) {
           self.secret.zeroize(); // 析构时底层内存写入 0 填充，防止内存转储被窃取
       }
   }
   ```
3. **脱敏视图投影**：
   任何向前端交付的 `PasswordView` 均强制剔除 `secret`，仅暴露 `id`、`name`、`success_count` 与掩码后的元数据。

### 4.5 存储拓扑感知与智能介质路由
针对现代 PC 混合存储环境（NVMe SSD + 机械硬盘 HDD），系统通过 Win32 `IOCTL_STORAGE_QUERY_PROPERTY` 查询寻道惩罚（Seek Penalty）以准确判定介质类型：
```rust
// crates/domain/src/storage/route.rs
pub fn decide_route(
    cache_enabled: bool,
    source_medium: DriveMedium,
    target_medium: DriveMedium,
    cache_input_pref: bool,
) -> ExtractionRoute {
    if !cache_enabled {
        return ExtractionRoute::DirectToTarget;
    }
    match (source_medium, target_medium) {
        // 源盘与目标盘均为机械硬盘：极度寻道竞争，强制走 SSD 临时缓存
        (DriveMedium::Hdd, DriveMedium::Hdd) => {
            if cache_input_pref {
                ExtractionRoute::CacheInputThenExtract
            } else {
                ExtractionRoute::CacheExtractThenMove
            }
        }
        // 源为机械盘，目标为 SSD：仅缓存输入以加速读吞吐
        (DriveMedium::Hdd, DriveMedium::Ssd) => {
            if cache_input_pref {
                ExtractionRoute::CacheInputThenExtract
            } else {
                ExtractionRoute::DirectToTarget
            }
        }
        // 全 SSD 介质环境：无需任何中间缓存，直接直通解压，最大化降低 SSD 写入放大
        _ => ExtractionRoute::DirectToTarget,
    }
}
```

---

## 5. 跨语言工程结构与构建闭环

当前目录包含生产实现与迁移模块；下列目录存在并不表示已完成装配或退役：

```
OpenUnpack/
├── Cargo.toml                    # Rust 工作区统一配置
├── OpenUnpack.sln                # .NET 解决方案 (Engine, Worker, Core, Tests)
├── Directory.Build.props         # .NET 全局编译属性配置
│
├── crates/                       # Rust 核心模块体系
│   ├── protocol/                 # 【契约单源】跨端传输 Wire DTO 定义与黄金样本测试
│   ├── domain/                   # 【纯领域层】零 IO 业务规则、状态机矩阵、路径算法
│   ├── application/              # 【应用用例】TaskCoordinator 权威编排器与端口 Trait
│   ├── infrastructure/           # 【基础设施】SQLite (WAL/FULL)、DPAPI、Win32 资源探测
│   └── core/                     # 【当前生产】协调器、恢复、扫描、Worker/Vault 实现
│
├── src/                          # C# 原生解压引擎与独立 Worker
│   ├── OpenUnpack.Engine/        # 【纯净引擎】7-Zip COM ABI、P/Invoke、最小分卷开卷
│   ├── OpenUnpack.Worker/        # 【独立 Worker】受监督的管道解压进程 (仅依赖 Engine)
│   └── OpenUnpack.Core/          # 【退役过渡】仅支撑历史测试，不再向 Worker 输出依赖
│
├── apps/                         # 桌面客户端表现层
│   ├── desktop-host/             # 【Tauri 宿主】轻量化命令转接、权限防越权校验、托盘
│   └── desktop-ui/               # 【前端界面】React 19 + TypeScript + Ant Design
│
├── scripts/                      # 工程辅助与检查脚本
│   ├── check-contracts.ps1       # 跨端契约一致性静态验证脚本
│   └── run-cargo.sh              # 注入 Windows SDK 与 MSVC 工具链环境的 Cargo 包装器
│
└── tests/                        # 跨语言测试套件
    └── OpenUnpack.Tests/         # 可执行测试程序，以 dotnet run 执行原生回归
```

---

## 6. 质量保障与测试验证矩阵

此前项目盘点的 Rust 全工作区结果为 79 项通过、2 项恢复回归失败；原生混合 ZIP 与伪装后缀诊断也复现了缺陷，见 [项目总结](project-summary-2026-10-10.md)。本轮修复尚需统一运行最终源码的门禁，不宣称全绿。

### 6.1 历史测试报告摘录（非当前验收）

下表保留原白皮书声称的结果，计数与当前测试范围已变化，且曾以局部测试推断缺陷全部清零。它只能用于追溯原文，不能作为当前代码、整版产品或发行包的验收依据。

| 语言/子系统 | 测试套件位置 | 历史报告描述的范围 | 历史报告声称的结果 |
| :--- | :--- | :--- | :--- |
| **Rust Protocol** | `crates/protocol/tests/golden_samples.rs` | 验证 Settings 驼峰序列化、JobView 字段双向映射、Worker v1/v2 协议兼容、错误码枚举对齐 | **4/4 全部通过** |
| **Rust Domain** | `crates/domain/src/lib.rs` | 验证状态转移矩阵阻断、JobAggregate 守卫、通配符排除（含中文字符对齐）、单根展开、分卷解析、密码清零与排序 | **7/7 全部通过** |
| **Rust Infra** | `crates/infrastructure/tests/store_tests.rs` | 真实创建 SQLite (WAL/FULL) 验证状态持久化、旧版 JobView 升级、事务性删除保护；调用 Windows DPAPI 验证真实加解密与整文件升级 | **2/2 全部通过** |
| **Rust Core 单元测试** | `crates/core/src/lib.rs` | 扫描器格式探测、指纹哈希、密码库、备份与并发锁 | **34/34 全部通过** |
| **Rust 桌面管线回归** | `crates/core/tests/desktop_pipeline.rs` | 嵌套归档展开、部分交付保留、重试恢复、损坏包分类 (R14)、内容关联优先级验证 | **12/12 全部通过** |
| **Rust 格式矩阵测试** | `crates/core/tests/format_matrix.rs` | 多卷 ZIP、RAR4/RAR5、分卷 RAR (R15) 传统 `.r00` 兼容支持 | **4/4 全部通过** |
| **Rust 扫描器回归** | `crates/core/tests/scanner_regressions.rs` | 排除规则、目录穿越防护、非归档文件处理、递归扫描拓扑 | **5/5 全部通过** |
| **Rust Worker 集成测试** | `crates/core/tests/worker_client.rs` | 命名管道真实通信、进程生命周期确认、取消响应与句柄释放 (R06) | **3/3 全部通过** |
| **Rust 桌面宿主单元测试** | `apps/desktop-host/src/lib.rs` | 受信任源 URL 校验与越权保护 | **2/2 全部通过** |
| **Rust 架构缺陷复现门禁** | `artifacts/code-review-2026-10-10/probe` | 独立验证 R02~R05, R07~R12 9项缺陷全部清零 | **9/9 缺陷清零 (0 缺陷复现)** |
| **C# 生产引擎独立诊断** | `artifacts/code-review-2026-10-10/engine-probe` | 独立诊断 R14 (坏包误判加密) 与 R15 (传统 RAR 缺卷误判) 修复 | **2/2 缺陷清零 (0 缺陷复现)** |
| **C# Engine** | `src/OpenUnpack.Engine` | 验证纯净解压类库的编译无警告、无错误 | **0 警告 / 0 错误** |
| **C# Worker** | `src/OpenUnpack.Worker` | 验证仅依赖 Engine 的控制台程序编译与协议字段对齐 | **0 警告 / 0 错误** |
| **.NET 集成测试** | `tests/OpenUnpack.Tests` | 验证 7-Zip 原生引擎、ZipCrypto/AES、分卷恢复、路径穿越防范、大文件指纹、RAR5 官方样本 | **27/27 全部通过** |
| **前端 TypeScript** | `apps/desktop-ui` (Vitest) | 验证批量口令删除、备份预览与明确确认、扫描独立取消、子节点独立重试、单根展开规则 | **15/15 全部通过** |
| **跨语言契约防漂移** | `scripts/check-contracts.ps1` | 校验 Rust、C#、TypeScript 接口与 record 字段级一致性 | **字段级 100% 对齐** |

### 6.2 历史输出片段（未作为当前门禁复用）

这些片段未与本轮最终源码和命令绑定；本轮实际日志与失败记录应单独核销。
- **Rust Domain & Infra 测试报告**：
  ```
  running 7 tests
  test archive::exclusions::tests::test_exclusions ... ok
  test archive::naming::tests::test_single_root_detection ... ok
  test archive::naming::tests::test_unique_name ... ok
  test tests::test_archive_volume_and_exclusion ... ok
  test tests::test_job_aggregate_transition_guard ... ok
  test tests::test_job_status_transition_matrix ... ok
  test tests::test_password_candidate_dedup_and_sort ... ok
  test result: ok. 7 passed; 0 failed; finished in 0.00s

  running 2 tests
  test test_dpapi_vault_store_lifecycle ... ok
  test test_sqlite_task_store_lifecycle ... ok
  test result: ok. 2 passed; 0 failed; finished in 1.67s
  ```
- **.NET 27 项集成测试输出**：
  ```
  PASS DPAPI round-trip, encrypted primary/backup and corrupt vault preservation
  PASS encrypted portable backup, wrong passphrase and tamper rejection
  PASS candidate association priority, disabled/group filters and merge
  PASS native ZIP extraction and Unicode paths match original bytes
  PASS native encrypted encrypted.7z wrong/correct password
  PASS native encrypted encrypted.zip wrong/correct password
  PASS native encrypted zipcrypto.zip wrong/correct password
  PASS memory probe: unencrypted archive returns NoPasswordNeeded
  PASS split 7z volumes discovered once and reconstructed
  PASS path traversal, ADS and case duplicate entries rejected
  PASS intelligent cache route decider handles nesting, space gate, sequential bypass
  PASS upstream RAR fixture Rar5.encrypted_filesAndHeader.rar
  Tests: 27 passed, 0 failed.
  ```

---

## 7. 生产运维、安全合规与风险防范指南

### 7.1 生产运维与安全注意事项
1. **Windows DPAPI 运行环境限制**：
   - DPAPI 强依赖当前 Windows 用户的本地安全凭据。密码文件直接拷贝到其他机器或其他用户账户下将无法直接解密。跨设备迁移密码库时，必须引导用户使用系统提供的“受密码保护的加密备份（Export Backup）”功能。
2. **SQLite 长期并发与 WAL 文件治理**：
   - 当前配置为 `WAL` 模式配合 `synchronous = FULL`。长期连接与空闲期 `PRAGMA wal_checkpoint(TRUNCATE)` 属于治理建议，不能据本说明宣称已实现定期 checkpoint 或通过数十万任务规模验收。
3. **路径提权与路径穿越防护**：
   - 任何由压缩包内文件头解析得到的相对路径，在落盘前必须经过 `canonicalize` 与 `verify_no_reparse_ancestors` 校验，拦截任何包含 `..` 或是指向符号链接（Symlink / Reparse Point）的恶意压缩包，防止覆写系统关键目录。

### 7.2 后续演进建议与退役路线
1. **历史 C# Core 的终局退役**：
   - 当前 `src/OpenUnpack.Core` 仅保留用于兼容历史 `OpenUnpack.Tests` 测试用例。随着 Rust 端各端口适配器测试与端到端测试完全成熟，应将残留测试用例逐步转写至 Rust 端口集成测试，随后彻底从解决方案与代码库中删除 `OpenUnpack.Core`。
2. **Worker 会话复用扩展**：
   - 当前 Core 每个请求启动一个 Worker；单节点会话复用与减少 KDF 的收益尚需设计及测量。扩展时应限定任务和候选预算，并保留真实退出确认及内存隔离。
