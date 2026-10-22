# 解压速度与阶段剩余时间

日期：2026-10-11。用户选择的新功能：解压速度与剩余时间显示。

## 当前状态

**功能代码、后端回归与真实 Windows 成品的速度流程已通过；整个项目的稳定版仍未放行。** 后续已定位选择器测试驱动的 AutomationId 冲突并修正：原生文件选择/取消/界面添加4项通过，速度/详情/暂停/输出验证3项通过，无前端异常、宿主退出0。使用原 Release EXE 和包内真实 Worker/7z.dll，没有降低访问授权或改变路径断言。详见 [选择器修复与原生验收补充](native-picker-fix-2026-10-11.md)。完整稳定版仍以 [发版前审核](release-audit-2026-10-11.md) 的其余未闭合项为准。

## 使用行为

- 队列的“当前阶段”和任务详情显示同一组速度、预计剩余时间。
- 外包解压和内包处理显示实际输出速度；缓存/写回显示传输速度，严格验证显示回读速度。以 1024 为单位显示 KiB/s、MiB/s 等。
- 标签明确为“预计本阶段剩余”或“预计当前内包剩余”。后续内包数量与大小尚未确定，因此不计算整个任务或整个队列的预计完成时间。
- 至少观察 0.5 秒后计算速度；使用单调时钟和时间自适应 EWMA，平滑时间常数为 2 秒。初始、零输出、未知总量及非传输阶段显示正在估算或暂无法估算。
- 5 秒没有新采样时显示“等待新进度”。持续收到相同字节计数满 5 秒也清除旧估计；恢复输出后重新观察。系统时钟回拨不会复活已过期采样，前端不伪造倒计时。
- 每次请求、密码尝试、内包、重试和阶段转换独立测速；完成、失败及停止时清理。队列的“暂停新任务”允许活动 Worker 完成本次请求，因此它继续显示实际速度。
- 输出排除使用统一预计算表，进度总量只包含保留文件。排除目录后代按精确祖先路径匹配，规划过程支持逐项取消；原有输出/解码/路径安全预算保持。

## 实现范围

新增 `TransferProgress` 桌面 DTO 和 `JobView.transferProgress`。旧 JSON 缺字段仍能读取；运行观测保存在 Core 内存中，只在原子快照中投影，不随每条进度写数据库，也不在重启后恢复旧速度。

Core 以 protocol/request/operation/generation/阶段/活动节点校验事件身份。`TransferOwner` 在正常、错误、取消和异常展开路径清理本次观测，旧请求不能删除新请求的观测。现有生产 Host 仍装配 Core；没有切换 TaskCoordinator。

主要文件：`crates/core/src/transfer_progress.rs`、`crates/core/src/coordinator.rs`、`crates/protocol/src/desktop.rs`、`apps/desktop-ui/src/components/TransferMetrics.tsx`、队列/详情组件，以及 Engine 的排除规划。

## 验证与固定源码

证据目录：`artifacts/feature-speed-eta-2026-10-11/`；实施与独立复核：`_workspace/feature-speed-eta-2026-10-11/`。

| 检查 | 已确认结果 |
|---|---|
| 本轮开始前 UI 基线 | 28/28，`ui-feature-baseline.log` 位于上一轮审核目录 |
| 修正时钟回拨后的 UI | 76/76，TypeScript/Vite 构建通过，`ui-test-final.log`、`ui-build-final.log` |
| 当前 Core 单元套件 | 63/63，`rust-core-final.log`；包含本轮算法/身份/生命周期回归 |
| 其他 Rust crate | 35 个用例通过，`rust-rest-final.log` |
| C# 当前原生套件 | 31/31，nativeAvailable=true，当前源码与首次固定源各跑一遍；`dotnet-final.log`、`dotnet-frozen.log` |
| 第三方材料 | 488 个组件、1030 文件、52132400 字节一致，`materials-frozen.log` |
| 最终专属源跨语言协议 | 14 Desktop/7 Worker DTO、169 字段及4个变异检查；19组真实 Rust→C#→Rust 全通过，`contracts-isolated-final.log` |
| 最终专属源追加回归 | 协议黄金5、Core transfer16、Worker spill manifest3全部通过，`contracts-isolated-final.log` |
| 实际优化 EXE | `release-build-r2.log` 成功，Cargo退出0，用时6分57秒；首次构建异常退出且没有具体编译错误，保留 `release-build.log` |
| 包内真实 Worker | v2 原生控制14断言通过，`worker-v2-packaged.log` |
| 新 EXE 界面大包流程 | **后续3/3通过**，见 `artifacts/picker-fix-2026-10-11/native-button-fix-r5/results.json`：速度/阶段ETA、暂停时详情、写回/回读、输出/原包SHA、排除、目标任务行完成与观测清空；无前端异常、退出0。原始失败记录保留，不能把早期 picker 的 `normalExit=null` 改称正常退出 |
| 原生选择与界面添加 | 后续4/4通过，见 `native-picker-regression-r2/results.json`：不同目录中文/空格路径精确返回、取消返回空、真实添加文件→扫描→登记；无前端异常、退出0 |

本轮工作树同时有其他归档浏览功能开发。第一次固定源缺少同期 C# 新增的 `FileRecord.isEncrypted` Rust 对应字段，契约检查明确失败；第二次固定源捕获了尚未同步 TS 的 Archive*View，契约检查再次失败。这两份失败证据保留，均未用于交付。

最终功能专属源由 `source-path.txt` 指向：`source-speed-only-b619487fc71a485f919aa353a8d465cc/`。302 个源码/配置/原生引擎/样本文件绑定摘要，`SOURCE-LINEAGE.json` 记录来源。它在第一次固定源上仅同步 Worker Rust `is_encrypted`、相应契约样例及两处测试字面量，除此 299 个文件相同。`CopyFile` 未改，旧封存/发布的文件映射及摘要语义保留。其他正在开发的 Archive*View/API 完全保留在原工作树，没有装入本次候选。

新字段同步之后，最终专属源的真实 Rust→C#→Rust、协议黄金样例、Core transfer 与 spill manifest 追加回归全部通过，并从该源成功构建 EXE。上述 63/35 是此前分套结果，不声明最终专属源一次全 workspace 都跑过；完整 Core 集成大清单/所有格式套件本轮不重复执行。302 个绑定文件最终核对零不一致；其他同期开发继续留在原工作树。

关键复现入口：`build-worker.ps1`、`verify-contracts.ps1`、`build-release.ps1` 均位于本轮证据目录。前端使用 `npm --prefix apps/desktop-ui test` 和 `run build`；Core 使用 `cargo test -p openunpack-core --lib --locked --offline -- --test-threads=1`，其余 crate 使用 `cargo test --workspace --exclude openunpack-core --locked --offline -- --test-threads=1`。最终追加回归的 `--manifest-path` 指向上述专属源，`--target-dir` 使用已有项目 target。构建前从原项目导入已验证 MSVC/SDK 环境，self-contained Worker runtime 固定为10.0.9。

## 交付边界

真实成品验证使用隔离的数据目录、512 MiB 保留内容和64 MiB排除内容的合成归档、实际 Host/Worker/7z.dll 和 WebView2。此前选择器驱动按 AutomationId=1 首先匹配许可证 ListItem，而非真正的 Open Button；旧的错误路径因此已得到解释。修正控件定位并严格核对路径后，实际成品流程通过，已有速度/详情截图、完整字节摘要、复制/回读采样、完成清理证据。原始未授权读取仍正确拒绝，所有失败记录保留；没有访问个人密码库，没有提交或发布稳定版。

本功能的队列→速度/阶段ETA→暂停→详情→写回/回读→完成及输出SHA、原包保留已闭合。关闭详情后仍保持批量解压页面，最终完成断言限定到目标任务行。独立 QA 复算输出和原包摘要，302绑定源码没有漂移。完整稳定版仍须完成原审核剩余场景。

更新后的内部便携候选：`artifacts/picker-fix-2026-10-11/OpenUnpack-speed-eta-2026-10-11-windows-x64-native-verified-portable.zip`；摘要见同目录 `PACKAGE-SHA256.txt` 和 `package.json`。同一组原程序二进制附最新报告与关键原生证据，明确速度与文件选择场景通过、StableReleaseApproved=false。原功能 ZIP 保留为此前验收未完成时的历史快照。

文件夹/多选/拖放、完整备份恢复界面、干净 Windows 安装/升级及物理磁盘/DPI 验收仍按原审核报告保留。本功能通过也不意味着项目所有功能已获完整发版批准。
