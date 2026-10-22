# OpenUnpack 代码与后台验证（2026-10-12）

用户选择先完成代码和后台验证，跳过新增设置开关、暂缓原生界面套件。生产任务执行链仍为 Core；本轮没有切换新 TaskCoordinator，也没有批准稳定版发布。

## 已闭合的包级证据

修复后的 NSIS 已实际构建。隔离产品 `OpenUnpackAudit-736d9565aa3f44ec92678910eceb9f92` 使用独立目录和注册表键，没有安装或卸载个人使用的 OpenUnpack。

- 数据目录、尾部分隔符和 `\.` 三种路径写法均收到明确阻断退出码 2。
- 已记录的旧默认目录被跳过；程序实际装进 `%LOCALAPPDATA%\Programs\<审计产品名>`。
- 安装载荷中 Host、Worker EXE/DLL、Engine DLL、7z.dll 五个组件与已验证候选的 SHA-256 一致。
- 重复安装、卸载均保留合成任务库、密码库、WAL 与备份字节；每步核对摘要。
- 卸载后仅清理本次合成目标；任务与密码库的正式卸载语义为保留。卸载勾选项已准确标为“清除界面缓存（保留任务和密码库）”。

权威执行记录：`artifacts/remaining-validation-2026-10-12/installer-final/installer-results.json`、`setup-executions.json`、`data-before.json` 与 `installer-final.log`。第一版四项验证和路径回归记录均保留，不覆盖历史失败证据。

正式产品名普通安装候选：`artifacts/remaining-validation-2026-10-12/OpenUnpack-0.1.0-windows-x64-standard-candidate-setup.exe`。版本/发行者已检查，并独立解开安装载荷核对上述五个组件及 1033 个资料文件。普通包包含 WebView2 引导程序；隔离安装测试使用 skip 模式，所以没有验证缺少 WebView2 时的安装路径。

## 代码与测试

| 范围 | 结果 | 证据 |
|---|---|---|
| 正常 Tauri 发版构建与 UI 构建钩子 | 已通过 | `artifacts/remaining-fixes-2026-10-11/host-build-verified.log` |
| Host 单测，包括规则范围、字节边界和编码 | 5/5 | `artifacts/remaining-validation-2026-10-12/host-unit-tests.log` |
| 最新数据库迁移、租约、WAL 恢复与错误回滚 | 11/11 | `artifacts/remaining-validation-2026-10-12/migration-regressions.log` |
| Rust 工作区后台回归（不含 Host） | 已通过 | `artifacts/remaining-fixes-2026-10-11/rust-workspace.log` |
| 实际 Worker 输出规则跨语言回归 | 1/1 | `artifacts/remaining-fixes-2026-10-11/output-rule-worker-final.log` |
| 原生引擎与 Worker 回归 | 36/36 | `artifacts/remaining-fixes-2026-10-11/dotnet-tests.log` |
| 完整 UI 单测 | 91/91 | `artifacts/remaining-fixes-2026-10-11/ui-tests.log` |
| Rust → C# → Rust 契约样例 | 19/19，兼容差异保留记录 | `artifacts/remaining-fixes-2026-10-11/contracts.log` |

输出试算复用实际 Engine 规划器，解决 emoji 问号计数、终末 sigma 和 dotless i 的差异，保留显式目录继承与原有提取行为。新层取消使用共享退出结果并释放全局锁；快照报告真实错误和统计；迁移使用备份、事务版本门及进程寿命租约。不支持租约的历史二进制仍必须退出后才能迁移。

## 原生与架构边界

权威 `native-qa/` 前置门现在同时检查 WTS、输入桌面名称和同会话 LogonUI。当前探测证据保存在 `artifacts/remaining-validation-2026-10-12/session-probe/native-session.json`；探测结果不是 UI 套件通过记录。A/B/C 和 Rules 的实际原生 Host 验收均按用户选择暂缓，原多选超时根因仍未确证。

仍需完成 TaskCoordinator 完整执行链、速度事件接入、反向依赖拆分、分页/增量/峰值内存、真实元数据路由与探针预算、协议兼容债；发布提交和远端 CI 尚未绑定。独立干净机、实际跨版本升级/回滚、缺 WebView2 的离线安装、第二账户恢复、真实系统 DPI/多屏及故障注入仍缺验收。

遗留 PID 20220、30948 仍在，本轮使用独立构建目录避开争用，没有强制终止这些进程。没有新增设置开关、提交、推送或对外发布。
