# OpenUnpack 已知问题修复与复验

> 2026-10-22 补充：普通用户同 SHA 部署及完整套件中的原始回收正向用例均已通过；新候选包、当前复验与剩余发行门禁见 [10-22 发版审查](release-readiness-2026-10-22.md)。以下保留 10-21 当次失败与未完成状态，不回写为历史通过。

日期：2026-10-21T23:25:58.362362+08:00。版本：0.1.0；HEAD `cf6e34411c809d092c48ed2f4bdd4ab6160222c1`，现有脏工作区。

**发版结论：NO-GO，当前仍不能正式发版。** 数据完整性、选择提取、英日翻译及密码/冲突契约问题已修复并验证。真实回收站正向流程仍失败；最终安装包、签名、真实窗口和干净机验收尚未完成。以下结果用于评估本轮修复，不等于整版验收通过。

## 修复结果

| 问题 | 当前状态 | 行为与证据 |
|---|---|---|
| RR-01 缺成员/缺结束块 TAR 被报告为完整成功 | 已修复并通过跨层回归 | Engine 检查归档级 ErrorFlags/WarningFlags。普通模式拒绝损坏；显式抢救保留 salvagedWithCrcWarning，Core 普通/恢复发布保持降级状态，不记成功密码、不清源。5 个真实 Worker→Core 用例核对官方 7z t、输出字节与源哈希。 |
| RR-02 提取所选 IPC/API 缺接线 | 已修复并通过后端与 UI 检测 | 补应用命令/ACL/API；改用有限 Worker、字面选择器、私有工作区、读回及唯一目录发布。读写授权、分卷授权、发布前再次授权/源见证、flatten 冲突拒绝。真实 HTTP 验证缺密码重试、提取成功及未授权目录拒绝。 |
| RR-03 回收站失败 | 部分修复，未关闭 | 已修 Shell 正斜杠解析和扩展/UNC 路径，错误带 COM 阶段/HRESULT。真实流程仍没有合格回收凭据，安全中止并保留源包；原始正向断言未跳过。 |
| RR-04 英日翻译缺键 | 已修复 | 补齐各3条键及正确的目录打开错误提示；UI 全套通过。 |
| RR-05 密码候选/测试契约不一致 | 已修复并复验 | 文件名/旁注线索仅排序已授权候选，不另行派生口令。overwrite 仍为支持策略，未知策略仍拒绝；重名回归同时验证同内容去重、不同内容另存和 skip 保护；便携 Vault 显式清除 opt-in 并恢复环境。 |
| RR-06 原生 UI 脚本固定绿灯 | 门禁修复，实际验收未完成 | 16个未执行项目现在标 BLOCKED，返回 exit2；不会把常量结果当 GUI 证据。仍须完成真实候选操作和包验收。 |

选择提取的 UI 还覆盖了取消目录选择不提交、错误保留选择、临时密码重试、服务/任务切换后的迟到响应隔离。远程连接不调用本机打开目录。API 使用说明已同步到 [本机 API 文档](../local-api.md)。

## 实际验证结果

| 范围 | 最终结果 | 范围限制 |
|---|---|---|
| 完整 Rust 工作区 | **468 passed / 1 failed / 9 ignored**；exit 101 | --workspace、--no-fail-fast、单线程；保留真实回收失败和条件忽略项 |
| Worker→Core 归档完整性 | 5 passed / 0 failed / 0 ignored | 完整归档与两类截断，普通/抢救发布及源清理保护 |
| Worker→Core 选择提取 | 5 passed / 0 failed / 0 ignored | 精确选择、目录/flatten、冲突拒绝、默认输出授权和执行中撤销授权 |
| 真实桌面后端流程 | 37 passed / 1 failed / 0 ignored | 38项；包括16,000真实文件的大清单批次复制/完整读回，失败项为回收站 |
| C# 原生全套 | **109 passed / 0 failed / 0 skipped** | 实际 Engine/Worker/7-Zip；不是安装包 |
| 原始归档差分 | **8/8 通过**，修复前4/8 | 完整 ZIP/TAR、两种截断、抢救开/关；与官方7-Zip对照 |
| UI 全套 | **214/214 通过** | React/jsdom；31个定向用例为其中子集，不重复累计 |
| TypeScript/Vite、ESLint、轻量 a11y | 均通过 | 独立 dist；不代表原生辅助功能验收 |
| API/CLI/Protocol 定向 | 56 passed / 0 failed / 0 ignored | 与完整工作区重叠，不累计 |
| Host 单元定向 | 28 passed / 0 failed / 1 ignored | 测试副本嵌入 Common Controls v6 清单；原 Cargo 二进制不改，不能替代正式 Host |
| 真实 HTTP→Host→Core→Worker | 1 passed / 0 failed / 0 ignored | 显式执行原条件忽略项，含普通/临时密码/等待输入和所选提取；源保留、库为空、无关队列不启动 |
| MCP 构建与测试 | **20/20 通过**；exit 0 | 本地连接器服务测试 |
| 自包含 Worker v2 | **14项断言通过** | 198文件独立候选，实际 .NET10.0.13 运行时身份验证 |
| 原生 UI 门禁 | 0 passed / 16 blocked / 0 failed；exit2 | 只证明门禁拒绝未验收状态，没有执行真实窗口操作 |

工作区失败用例：

- `recycle_bin_cleanup_when_enabled_and_verified`

条件忽略用例保持原状，不计作通过：

- `toolbox::tests::cross_volume_transfer_uses_locked_source_and_preserves_existing_target`
- `synthetic_cpu_format_case`
- `synthetic_hdd_cache_matrix`
- `synthetic_load_matrix`
- `synthetic_pause_and_cancel`
- `synthetic_snapshot_history`
- `cross_disk_hdd_source_ssd_cache_roundtrip`
- `disk_topology_matches_physical_ground_truth`
- `local_api::native_tests::native_http_extraction_keeps_passwords_temporary_and_gui_jobs_queued`

## 构建和策略门禁

| 命令 | 结果 | 日志 |
|---|---|---|
| rust-format-final | 通过 / exit 0 | [rust-format-final.log](../../_workspace/release-fixes-20261021/logs/rust-format-final.log) |
| clippy-final | 通过 / exit 0 | [clippy-final.log](../../_workspace/release-fixes-20261021/logs/clippy-final.log) |
| contracts-roundtrip | 通过 / exit 0 | [contracts-roundtrip.log](../../_workspace/release-fixes-20261021/logs/contracts-roundtrip.log) |
| security-policy | 通过 / exit 0 | [security-policy.log](../../_workspace/release-fixes-20261021/logs/security-policy.log) |
| runtime-policy | 通过 / exit 0 | [runtime-policy.log](../../_workspace/release-fixes-20261021/logs/runtime-policy.log) |
| release-policy | 通过 / exit 0 | [release-policy.log](../../_workspace/release-fixes-20261021/logs/release-policy.log) |

真实 DTO 往返有19个 Rust→C#→Rust 场景，并另跑6个 golden samples。安全/运行时/发行校验器测试是验证校验器行为，不能替代当前最终成品的漏洞扫描、签名或安装验收。严格 Clippy 前两次分别在 cli_engine 的排序写法和 Host 的可合并嵌套条件处失败，均做等价修正后全目标通过；原日志 clippy.log、clippy-attempt-2.log 保留，未放宽 -D warnings。

## 快照、候选及证据可靠性

动态文件用例使用仓库 fixtures 或本轮合成数据，未对用户归档执行解压、回收或删除。“源包保留”指产品在失败/抢救时保留输入；临时合成夹具可在测试结束后由 tempfile 正常释放。

本轮初始 642 项受保护输入留存 source/；这些输入中有意修改 31 项，清单和对应补丁见 [input-integrity.json](../../_workspace/release-fixes-20261021/input-integrity.json) 与 [changes.patch](../../_workspace/release-fixes-20261021/changes.patch)。API 说明与报告另行更新。原有脏工作区、原失败日志及首次编译/夹具错误保留，没有重置、提交或推送。

最终整工作区执行前另外冻结源码、配置、官方7-Zip、共享暂存、独立 Worker 候选及固定 Release Worker 依赖；**1051 项执行前后全部一致**。UI 70 项、C# 27 项及候选 198 项与其各自最终测试快照一致。摘要见 [final-integrity.json](../../_workspace/release-fixes-20261021/final-integrity.json)；最终源码为 final-source/。

Worker 候选为 `_workspace/release-fixes-20261021/candidate/worker/OpenUnpack.Worker.exe`。Engine SHA-256 `9228d87d495569cdf85cd3928e6e00214fcb4f84afb9223213ebdf72aef53e82`，实际 runtimeconfig/deps/CoreLib/coreclr/hostfxr 均核验10.0.13。共享 artifacts/worker 和最终 Setup/portable 没有用该候选重新封装，不能将独立候选验证写成最终包通过。

生成 ACL 已确认 allow-extract-selective 和 main-commands 都允许 extract_selective；清单哈希 `00e568dc9446731be6643b98c2bd8a3fd589a82ae171d95b39a78413b16e3425`。这仍是接线/构建证据，原生窗口点击未验收。

运行使用隔离 Cargo target、单构建作业、私有 TEMP 和锁定离线依赖；保留已记录的 libsqlite3-sys debug=0 MSVC 构建绕行，没有改变产品配置。Host 测试清单适配脚本为 [host-runner.ps1](../../_workspace/release-fixes-20261021/host-runner.ps1)。

## 剩余发版条件

1. 在可用回收站的普通 Windows 用户环境完成原始真实回收正向用例，并核对源身份与回收凭据；当前仍失败。独立探针也没有取得凭据，不能仅归因为环境就豁免。
2. 冻结并重新封装最终 Setup/portable，核对完整载荷摘要和可信签名，完成 Win10/11 干净机安装、升级、卸载和真实窗口功能（选择、拖放、托盘、DPI等）。
3. 补齐仍按条件忽略的物理盘/长时及相关环境验收；动态撤销测试通过只能覆盖本次运行中的授权撤销，不构成全部取消/异常生命周期竞态保证。

## 证据入口

- [机器汇总](../../_workspace/release-fixes-20261021/summary.json)、[完整 Rust 命令](../../_workspace/release-fixes-20261021/rust-full-results.json)、[完整 Rust 日志](../../_workspace/release-fixes-20261021/logs/rust-workspace-final.log)。
- [定向命令](../../_workspace/release-fixes-20261021/rust-focused-results.json)、[静态门禁](../../_workspace/release-fixes-20261021/rust-lint-results.json)、[策略及真实契约](../../_workspace/release-fixes-20261021/rust-gates-results.json)。
- [UI 修复与验证](../../_workspace/release-fixes-20261021/ui/report.md)、[Worker 修复与验证](../../_workspace/release-fixes-20261021/worker/report.md)、[回收站根因与未决证据](../../_workspace/release-fixes-20261021/recycle/report.md)。
- [独立只读审查](../../_workspace/release-fixes-20261021/qa-review.md)：发布前复核授权的明确 await 窗口已在代码关闭；该审查本身没有运行测试。
- [修复前复验报告](functional-release-recheck-2026-10-21.md)，保留历史失败状态。

早期定向运行的 TAR 断言误把内部恢复工作区当成用户发布目录，以及自定义策略的普通/扩展路径格式差异，均作为夹具问题修正并保留日志；完整工作区结果以上述最终冻结运行计。测试没有通过跳过回收站失败或放宽完整性/授权/源保护判据变绿。
