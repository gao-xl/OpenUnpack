# 更新日志 (Changelog)

本文件记录 OpenUnpack 项目的重要更新与变更。

本日志格式基于 [Keep a Changelog 1.1.0](https://keepachangelog.com/zh-CN/1.1.0/) 构建，并遵循 [语义化版本 2.0.0 (Semantic Versioning)](https://semver.org/lang/zh-CN/) 规范。

---

## [Unreleased]

（暂无）

---

## [0.1.0-beta.1] - 2026-10-22

> 首个**公开预发布（Beta）**。本节为相对 2026-10-15 内部候选里程碑（见下方 `[0.1.0-rc]`，该候选未对外分发）的完整增量记录；基础能力（桌面体验、解压调度、密码库、校验与发布保护）见 `[0.1.0-rc]` 条目。
>
> Beta 说明：核心解压、密码库、监听文件夹与本机 API 已可正常使用；界面细节与部分边界场景仍在打磨，请勿用于不可替代的唯一副本。

### Added
- **本机 HTTP API 2.0（Local API）深度成熟化**：
  - 错误体系升级为 RFC 7807 Problem Details 标准模型（`type`/`title`/`code`/`detail`/`requestId`/`invalidFields`/`retryAfterSecs`），统一 `ApiResponse<T>` 响应信封与服务端端到端耗时元数据。
  - 全量命令防御性前置校验：路径合法性、控制字符、绝对路径、标识符字符集白名单（`^[a-zA-Z0-9_\-]{1,64}$`）、批量上限（≤1000）与口令长度边界；拦截 NTFS 备用数据流（ADS）冒号逃逸；路径鉴权采用大小写折叠规范化匹配。
  - 认证与限流：静态凭据 TTL 过期与 `last_used_at` 活动审计；认证失败滑动退避（非阻塞异步化，防工作线程饥饿）；并发排队缓冲与 429 优雅重试（`Retry-After` 标头），暂停/取消保留独立快速通道。
  - 新增聚合指令 `add_archive`（路径规范化、分卷探测、输出与清理策略、临时密码注入、自动排队单条完成）与归档内容结构化审阅工具、动态临时密码注入工具。
- **WebSocket 双向通道**：`/api/v1/ws` 端点（Header/Query Token 认证、单次消费票据、帧大小与并发资源限制），`state_changed` / `job_progress`（吞吐、ETA、百分比实时推流）/ `need_password` 实时事件；命令与 HTTP 共用统一授权闸门。
- **交互式缺密码挂起**：解压遇密码未命中时 60 秒异步挂起，可经 WebSocket 或 `submit_input` 即时回传唤醒，超时自动回退密码库链路。
- **无头守护进程模式**：`--headless` / `-d` / `--daemon` 独立后台启动，支持 `--data-dir`、`--port`、`--trust-all`、`--on-complete`、`--read-root` / `--write-root`（最小权限，可重复参数）与 `--init-token`（完整令牌仅显式打印，默认脱敏输出）；默认客户端作用域收敛并默认 30 天过期；兼容 POSIX 环境。
- **命令行客户端**：新增 `openunpack-cli`，提供 `status` / `unpack` / `list` / `pause` / `resume` / `cancel` / `retry` 子命令与 `--json` 输出。
- **监听文件夹自动入队**：五层防抢读漏斗（扩展名过滤、内核排他读锁探针、3 秒稳定窗口状态机、ZIP/7z/RAR 头尾元数据轻量预检、分卷协同就绪判定）+ 独立 `intake_guard` 六项防护（环路自激、OneDrive/iCloud 脱机占位符、P2P 全零预分配、磁盘爆盘/解压炸弹 1.1x 膨胀校验、杀软扫描退避、孤儿分卷超时收尾）；支持多卷分卷完备性检查与解压后源文件回收站安全处置。
- **任务终态外部钩子**：`--on-complete` 与 `settings.on_complete` 在任务终态触发外部脚本，注入 `OPENUNPACK_JOB_ID` / `OPENUNPACK_STATUS` / `OPENUNPACK_SOURCE` / `OPENUNPACK_OUTPUT` 等环境变量；数据不经位置参数传递，含 shell 元字符的脚本路径直接拒绝。
- **冲突决策与语义版控引擎**：发布冲突支持 `rename`（加序号）/ `skip` / `timestamp`（时间戳版本）/ `keepNewer` / `keepOlder` / `keepLarger` / `overwrite`（原子暂存备份与失败回滚）七种策略；目标文件大小与 SHA-256 均一致时自动 `SkipIdentical` 跳过，消除冗余 I/O。
- **归档内选择性解压**：归档审阅支持按文件/目录树勾选与局部提取，`zip` 流式与 7-Zip CLI 双引擎，支持 `flatten` 扁平化与目标重定向。
- **损坏归档抢救模式**：Zip 引擎在条目读取失败 / CRC 失败 / 分卷截断时不终止流程，标记损坏条目并继续提取，任务置 `salvaged` 状态并在界面/API 明示已抢救与损坏文件数；7-Zip CLI 非零返回码含有效输出时同样可提取已落盘健康文件。
- **NTFS 硬链接空间优化**：同分区输出按「大小预筛选 + SHA-256 分组」去重，以原子「改名 → 硬链接 → 回收」机制转换，零数据丢失风险；默认启用（`deduplicate_hardlinks`）。
- **物理磁盘拓扑感知调度**：Win32 IOCTL 双通道探针（`IOCTL_STORAGE_GET_DEVICE_NUMBER` + `IOCTL_VOLUME_GET_VOLUME_DISK_EXTENTS` 回退）识别逻辑盘对应物理磁盘；同一机械硬盘最多 1 个高 I/O 任务，NVMe/SSD 保持多任务并发；HDD 任务自动压制解压引擎线程数 ≤2。
- **智能密码提取**：文件名密码特征挖掘（中英文前缀、发布站域名、尾部扩展名）、同级伴随文件嗅探（`<stem>.txt/.nfo/.md`，64 KiB 上限，UTF-8/GB18030 兼容）、规则派生增强与调度链即时注入。
- **冗余嵌套自动展平**：过滤 `__MACOSX`/`.DS_Store`/`Thumbs.db`/`desktop.ini` 系统噪音，Windows 大小写无关拓扑聚合，连续展开最多 3 层无意义包裹。
- **ZIP 编码流式探测自愈**：UTF-8 / GBK / Shift-JIS / Big5 / EUC-KR 多语言编码探测评分矩阵，解耦文件系统依赖。
- **资源管理器右键菜单**：Windows 注册表 Context Menu 一键注册/注销，单实例命令行唤醒自动入队外部归档。
- **单任务临时密码重试**：在飞任务即时密码重试与挽救交互，支持局部交付与可选密码库同步。
- **解压完成自动化动作**：`none` / `openOutput` / `sleep` / `shutdown` 配置，关机与睡眠前 30 秒带音效、可撤销的安全倒计时窗口。
- **大队列快照增量协议**：`SnapshotDelta` 增量投影与服务端缓存，前端 `SnapshotCursor` 局部合成与版本单调比较，显著降低大队列 IPC 序列化开销。
- **守护进程远程接管**：WebUI 静态产物服务（含无构建产物时的内置回退控制台）、CORS 预检；前端支持远程/本地守护进程无感切换。
- **生态扩展**：下载器集成脚本（qBittorrent / Aria2 / Transmission）、Manifest V3 浏览器插件接入原型（`examples/browser-extension/`）、Python 异步事件驱动客户端、官方《OpenAPI & 开发者生态接入指南》。
- **跨平台密码库规范**：OPENVLT1 格式（AES-256-GCM + PBKDF2-HMAC-SHA256 100,000 次派生），支持环境变量/本地主密钥注入，向下透明兼容 Windows DPAPI 旧库。
- **纯 Rust 原生解压引擎接管**：内置 Zip 流式扫描、密码探针、包含/排除通配符、严格 Zip Slip 防御与 7-Zip CLI 桥接；无 Worker 进程时全自动双轨接管。
- **容器化与跨平台构建**：官方多阶段 `Dockerfile`、`docker-compose.yml` 与 `build-crossplatform.sh` / `build-headless.ps1` 构建脚本。

### Changed
- **架构分层重构**：工作区扩展为 `protocol` / `domain` / `application` / `infrastructure` / `core` / `local-api` / `cli` / `desktop` 八 crate 分层；六大双轨镜像模块（vault、worker client、worker manifest、database_lease、paths、route）收敛为 infrastructure 单一真源 + core 兼容转发，彻底消除 MIRROR 双写风险。
- **Local API 认证默认值收紧**：无头模式默认客户端不再授予全部作用域，收敛为读写任务与文件所需最小集，默认 30 天过期；完整 Token 仅 `--init-token` 显式输出，默认脱敏。
- **桌面 UI 现代化**：系统化 Design Token 体系（Ant Design 6 ConfigProvider 深度定制）、队列遥测看板与格式色彩徽标、空状态磁吸拖拽靶区、等宽数字防抖（进度/吞吐/ETA）、沉浸式标题栏与侧边栏心跳卡片；TSX 内硬编码色值清零。
- **CSP 策略**：`connect-src` 增加对 `127.0.0.1` / `localhost` 回环 HTTP/WS 任意端口的放行，仅用于本机守护进程远程接管，不含任何非回环地址。
- **依赖升级**：`zip` 8.6.0 → 9.0.5（修复 RUSTSEC-2026-0369 等 3 条漏洞）、`@tauri-apps/cli` 钉定 2.12.1；新增 `axum` / `tokio-tungstenite` / `notify` / `utoipa` / `zopfli` 等依赖。
- **CI 门禁加固**：新增 cargo-audit（含安全策略脚本负向自测）、前端 ESLint + 过渡性 a11y 门禁、npm/MCP 生产依赖审计、`cargo clippy --workspace --all-targets --locked -- -D warnings` 严格检查。
- **无头模式信任开关**：`OPENUNPACK_TRUSTED_ALL_PATHS` 环境变量不再生效，改为进程内状态，仅无头入口 `--trust-all` 显式启用。

### Fixed
- **Local API 越权与注入防护**：共享任务队列水平越权（BOLA）隔离——`CancelJobs` / `RemoveJob` / `ClearCompleted` / `PauseJobs` / `ClearHistory` 全部接入客户端归属与权限校验；WebSocket 通道统一走 `dispatch()` 授权闸门（此前仅需 `jobs.read`）；`submit_input` 补任务归属校验；`add_jobs` 移除任意路径反向扩权；设置写入对系统特权字段（关机/休眠注入、监听目录）强制免疫，`on_complete` 钉死为当前配置。
- **多租户元数据脱敏**：任务元数据按客户端授权目录执行可见性沙箱，未授权路径脱敏为 `[REDACTED]`。
- **桌面宿主写授权**：原地解压对源文件父目录强制写授权校验；原生对话框与拖拽仅对用户明确选择的目录授予白名单；`execute_post_action` 增加队列运行中、活动任务存在、配置一致三重前置守卫。
- **7-Zip 排除/包含规则全引擎对齐**：CLI 命令行映射 `-xr!` / `-x!` / `-ir!` / `-i!`，提取后与发布规划阶段双通道物理清洗，达成 100% 规则对齐；封堵以 `/` / `\` 开头的绝对路径注入（Zip Slip）。
- **无头钩子失效缺陷**：`--on-complete` 此前仅写入环境变量且读取方为死代码，现改为启动时持久化到设置并校验必须是存在的绝对路径。
- **其他**：CLI 中文名按字符（非字节）截断导致的 panic；`intake_guard` 循环陷阱判定改大小写无关组件比较；哈希读取 `while let Ok` 吞错；前端 `JobDetails` 缺失 `inputDisabled` 声明导致的 `tsc` 构建阻断；异步 Promise 悬空调用与 React Hooks 级联渲染治理；并行任务取消后关机动作误触发；密码重试死路与静默失败。

### Security
- **依赖漏洞清零**：Rust（cargo audit）、npm 生产依赖、NuGet 三链审计 0 漏洞；`zip` 9.1 级 RUSTSEC 漏洞（RUSTSEC-2026-0369）随升级消除。
- **便携密码库 fail-closed**：移除"`.vault.key` 文件存在即切换"的隐式分支，便携密钥必须经 `OPENUNPACK_PORTABLE_VAULT` / `OPENUNPACK_VAULT_KEY` 显式启用，密钥必须是普通文件（拒绝符号链接）；已有便携库需显式设置变量方可打开，界面提供原因与两条解决路径提示。
- **7-Zip 口令通道定性**：经字节精确实测确认 7-Zip 26.04 CLI 不从重定向 stdin 读取口令，默认回退 argv 传递，`OPENUNPACK_7Z_STDIN=1` 保留为可选通道（决策登记 OD-002）。
- **WS 未认证请求连接中止（Winsock 10053）**：未认证请求体平滑排空后标准 FIN 挥手，消除连接复位。
- 全链路日志与错误追踪脱敏基线维持，敏感凭证、备份口令、数据库原始内容与个人隐私绝对路径不进入任何输出。

---

## [0.1.0-rc] - 2026-10-15（内部候选里程碑，未对外分发；公开首发为 0.1.0-beta.1）

### Added
- **核心桌面体验**：
  - 基于 Tauri + Windows WebView2 架构的原生桌面客户端（`OpenUnpack.exe`），搭配 React + Ant Design 前端界面。
  - 内建多语言支持：提供简体中文、English、日本語，支持即时动态切换，首选语言与界面偏好持久化。
  - 主题与交互：支持浅色模式、深色模式及跟随系统主题，支持窗口置顶、系统托盘常驻与自定义提示音。
- **解压与调度引擎**：
  - 基于双命名管道的进程级隔离架构：Rust 协调器与独立的 C#/.NET 10 Worker 进程通信，动态集成官方可替换 `7z.dll`。
  - 队列与并发管控：支持任务顺序队列、多任务并发（上限 5 个），支持机械盘与未知设备按实际介质仲裁并发度。
  - 暂存与缓存体系：支持可选的 SSD 工作缓存与中转目录，内置容量/余量限制与自动直写决策机制。
  - 深度嵌套处理：支持多层嵌套归档的递归自动展开，具备嵌套层数与累计解压上限保护。
- **密码库与安全存储**：
  - 本地密码库：由 Windows DPAPI（绑定当前 Windows 账户）提供加密保护，支持密码分组、快速搜索与启停控制。
  - 批量导入导出：支持 TXT（单行密码）与 TSV/CSV（含名称、密码、分组、备注）批量导入、语法预览与自动去重。
  - 独立加密备份：支持使用独立备份口令导出全量加密备份文件，恢复导入前支持结构化预览与合并确认。
- **校验与完整性保护**：
  - 双级校验语义：提供标准流式校验（解压流与目标写入流实时 SHA-256 比对）与严格模式（逻辑目标重读校验）。
  - 文件发布保护：支持同名目录自动编号或跳过；原子化发布流程，发布失败保留独立恢复现场，杜绝成品文件回滚删除。
  - 权限安全绑定：临时与发布目录基于当前 Windows 账户专属访问控制列表（ACL）和对象句柄绑定，防御路径遍历与重解析点注入。
- **生态与自动化扩展**：
  - 本机 HTTP API：基于 Axum + Utoipa OpenAPI，内置 43 项受控接口，支持独立密钥认证、目录权限授权与任务自动化管理。
  - 本地 MCP 连接器：基于 Node.js 24 开发的 stdio MCP 服务，支持外部智能体（Agent）客户端安全调用计划编排、任务轮询与重试。
- **工程化与打包体系**：
  - 自动化构建与验证管线：提供完整的 PowerShell 7 自动化脚本（`prepare-desktop.ps1`, `verify.ps1`, `build-desktop.ps1`）。
  - 多形态发行物料：支持便携免安装包（Portable）、标准 NSIS 安装包以及集成离线 WebView2 运行时的完整安装包。
  - 第三方物料与源码审计：自动化收集并审计依赖库许可协议，生成 `release-materials/manifest.json` 与 SHA-256 校验清单。

### Changed
- 安装路径规范调整：NSIS 默认安装路径重构为 `%LOCALAPPDATA%\Programs\OpenUnpack`，与数据目录 `%LOCALAPPDATA%\OpenUnpack` 实行物理分离，杜绝升级卸载误伤用户数据。
- 线程与内存分配策略优化：Worker 内存预算在多任务并发、归档预读和规则预览间动态共享，大字典任务遇资源瓶颈时支持优雅降级与提示。

### Deprecated
- 早期基于 WPF 的桌面试验原型（`OpenUnpack.Desktop`）及旧版 C# 单体参考实现（`OpenUnpack.Core`）正式退役归档，源码全面迁移至 Tauri + Rust 架构。

### Fixed
- 修复并发文件操作中的目录句柄锁冲突问题，增强在密集 I/O 状态下的发布可靠性。
- 修复极端场景下的路径长度边界判断逻辑，严格限制相对路径最大 128 层、目录租约最大 256 层。
- 修复重入解压时特定状态下内包密码重试的状态机流转断裂缺陷。

### Security
- 强化密码库与临时工作目录权限安全基线，确保临时中转目录仅限当前用户拥有完全控制权，禁止非特权继承权限。
- 全链路日志与错误追踪强制脱敏，确保敏感凭证、备份口令、数据库原始内容以及个人隐私绝对路径不进入任何控制台输出与日志文件。
- 明确威胁模型边界：声明 DPAPI 无法抵御同一用户身份运行的恶意代码注入，明确标准与严格校验的系统缓存语义。
