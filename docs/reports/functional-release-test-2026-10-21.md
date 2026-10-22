> 2026-10-21 后续复验已完成：最终状态请看 [功能模拟复验与发版结论](functional-release-recheck-2026-10-21.md)。本文件保留较早一轮记录；其中“Rust未完成、UI198/4、前端构建阻塞”已由新报告中的实际结果替代。

**OpenUnpack 最新工作区功能测试与发版判断 — 2026-10-21**

**结论：NO-GO，当前不能正式发版。** 本轮动态复现了截断 TAR 在丢失成员时仍报告完整成功；英文、日文翻译检查也稳定失败。现有原生 UI 绿灯脚本不执行交互，且本次没有完成当前最终安装包的构建、签名、原生操作和干净机验收。

标的为版本 `0.1.0`、HEAD `cf6e34411c809d092c48ed2f4bdd4ab6160222c1` 的**脏工作区**，不是仅测试该 Git 提交。测试前保留源码快照和逐文件 SHA-256。642 个受保护输入包含源码、配置、测试及既有 Worker/7-Zip 载荷，核对结果：前后全部一致。本轮只新增报告、测试探针和构建证据，没有修改产品源码、提交、推送或发布。

**本轮实际结果（不同层级的测试、断言和重跑不相加）**

| 检查 | 实际结果 | 证据范围 |
|---|---|---|
| C# 原生功能套件 | **100 passed / 0 failed / 0 skipped** | 真实 Engine、Worker、官方 7-Zip、格式/错误/路径/安全用例 |
| UI 全套 | **198 passed / 4 failed，共 202** | React/jsdom；不等于原生窗口验收 |
| 原始 UI 三个交互失败精确重跑 | **1 passed / 2 failed**，过滤掉 32 项 | API 撤销授权通过；归档旧队列断言与历史错误文本断言仍失败 |
| 多语言独立重跑 | **4 passed / 1 failed** | 稳定缺少 3 条翻译键 |
| MCP 连接器 | **20 passed / 0 failed** | 本地 HTTP/MCP 测试服务；不是当前打包 Host 的全命令矩阵 |
| Worker v2 | **14 项断言通过** | 本轮重建的 .NET 10.0.13 Worker，真实命名管道及原生引擎 |
| TAR 完整性补充探针 | **4 正常对照通过、4 损坏场景断言失败** | 完整 ZIP/TAR；缺尾 TAR 与成员头截断 TAR，抢救开/关 |
| Rust 工作区复验 | **尚未完成，不能计为通过** | 独立 CARGO_TARGET_DIR、2 个构建任务、单线程测试；详细见下文 |
| 架构与三端 DTO 静态契约 | **通过** | 32 desktop DTO、8 worker DTO、311 字段；不等于完整业务 API 验收 |
| C# DTO 已知语料 | **通过** | 使用现存语料的独立副本；不宣称完成最新 Rust→C#→Rust 新语料闭环 |
| cargo fmt、ESLint、轻量 a11y | **通过** | a11y 为仓库临时两项扫描，不能代替完整可访问性验收 |
| 发行验证器 / 运行时策略自测 | **16 / 37 项通过** | 合成策略测试，不是实际安装、签名通过 |
| 在线依赖审计 | **通过** | Cargo 548 依赖、0 已知漏洞；NuGet、Roslyn、两处 npm 审计通过；不保证无未知或业务漏洞 |
| Worker 自包含运行时 | **10.0.13 通过** | 实际 runtimeconfig/deps/CoreLib/coreclr/hostfxr 版本及哈希、官方 servicing 元数据 |
| UI 生产构建 | **环境阻塞** | Vite 已转换 3,195 个模块，esbuild 删除临时文件 Access is denied，未产出本轮通过的完整前端包 |
| 原生桌面、安装升级、签名发行 | **未通过验收** | Computer Use 三次枚举超时，0 交互；旧包不能继承新源码结论 |

**应先修复或补齐的项目**

1. **RF-01 / P1：截断 TAR 缺文件却被认定为完整成功。** 正常 `complete.tar` 有 `first.txt` 与 `second.txt`。将其截断为 1,100 字节的 `partial-header.tar` 后，官方 7-Zip 26.04 的 `7z t` 返回 2 / `Unexpected end of archive`；当前 Engine 在 `SalvageCorrupted=false` 和 `true` 下均返回 `completed`、`VerificationScope=archiveCrcAndStreamSha256`，只列出 `first.txt`，且 `IsCorrupted=false`。缺少结束块的 `no-end.tar` 两种模式也被认定为完整成功。不能把这种结果用于完整成功承诺。该探针直接验证 Engine，**未验证 Coordinator 发布、源文件清理或真实用户数据丢失**。当前 `SevenZipEngine.cs:665` 的 `ValidateResult` 只综合 HRESULT、回调和条目错误，没有核对归档级错误标志；需补上归档级完整性判定，并在 Worker→Core 发布/清源链保留回归。

2. **RF-02 / P2：3 条英、日翻译缺失。** 独立运行 `i18n.test.tsx` 仍失败，缺少「已成功提取 {count} 个文件至 {path}」「提取所选至...」及便携密码库未启用的长提示。补齐两种语言后重新检查，不能用旧多语言通过数覆盖本次失败。

3. **RF-03：UI 测试与当前行为未全部收敛。** 原始 4 个失败不能直接称作 4 个生产功能缺陷。选择性提取测试提供了 `extractSelective` mock，却仍期待 `addJobs`；当前组件优先走直接提取接口，属于明确的测试/行为不同步，需要对当前直接提取链新增正确的成功、取消和错误断言。历史失败场景在精确重跑中显示通用「读取历史记录失败」，没有展示 mock 的「存储暂时不可读」，原因仍需在实际 API 错误类型和运行环境中确认。API 撤销授权精确重跑已通过，首轮失败保留为不稳定观察。四文件扩大重跑耗时 788.97 秒后主动中断，保留了额外超时及失败，**不计为通过，也不以该次扩大重跑替换完整首轮计数**。

4. **RF-04：原生 UI 验收证据无效。** `scripts/ci/verify-native-ui.ps1:55–100` 的 UI-001～UI-016 直接写入 `PASSED`；没有使用候选路径启动窗口、调用交互并断言结果。16/16 不可作为验收依据。本次按 Computer Use 技能初始化、重试并重置后再试，`sky.list_apps` 三次均超时，0 次原生输入。文件/目录选择、拖放、队列操作、托盘、多开、DPI 均保持未验收。

5. **RF-05：当前最终发行候选缺少闭环。** 本次仅重建独立 Worker 测试载荷，未形成与这份源码完全绑定、通过全部构建的最终 Setup/portable。现存 portable Host 与本次 Worker 的 Authenticode 均为 `NotSigned`。仓库签名验证器要求 Valid、CodeSigning EKU 和指定发行者；未达到该门槛。旧 Setup/便携包以及旧安装日志不能代替当前包的安装、跨版本升级、卸载保留数据和干净 Win10/11 验收。

**需要区分的测试/环境问题**

- 安全策略自测首轮抛出 `Incomplete/ambiguous Cargo graph enabled an advisory exclusion`。逐条重新构造的非法依赖图均被拒绝；确定性属性顺序对照证明，原测试用 `-replace '"version": 1,'` 变异 JSON，当 version 为末尾属性时没有逗号，变异不生效，合法 v1 因而被误当作非法输入。**没有证据表明生产策略被绕过**；应修正测试夹具为结构化字段赋值。
- Rust 首轮在 `libsqlite3-sys` 构建时遇到 MSVC D8050「未能将命令行放入调试记录中」，属于构建阻塞。复跑仅通过 Cargo 命令行配置关闭该依赖的调试信息，保持源码、测试断言和单线程测试不变：`cargo test --workspace --locked --config 'profile.dev.package.libsqlite3-sys.debug=0' -- --test-threads=1`。
- 初次自行发布 Worker 未传发行脚本要求的 RuntimeFrameworkVersion，得到了 10.0.9，并被策略正确拒绝。已按 `prepare-desktop.ps1` 的参数用 `-p:RuntimeFrameworkVersion=10.0.13` 重发，实际运行时检查及 v2 测试通过。这是本轮探针准备错误，不能归因于正式打包脚本。两个错误语料路径的 C# DTO 尝试也已更正，原日志保留。

**发版前验收条件**：修复 RF-01 并验证 Worker→Core 的发布品质和源保护；补齐翻译；修复测试夹具并让当前行为的测试稳定通过；完成当前源码的正式前端/Host 构建；冻结最终包、签名和全载荷哈希；使用真实交互完成原生 UI 及干净机安装/升级/卸载验收。已有 294 主项方案本轮未全部逐项执行，物理盘故障、SMB/跨账户及长时间稳定性未验收；本报告不宣称全功能或正式发行接受。

**复核入口**

- [机器汇总及逐命令结果](C:/Users/im/Documents/ChatGPT/批量解压缩/_workspace/release-functional-20261021/summary.json)
- [642 个输入完整性结果](C:/Users/im/Documents/ChatGPT/批量解压缩/_workspace/release-functional-20261021/input-integrity.json)
- [TAR 断言及原始观察](C:/Users/im/Documents/ChatGPT/批量解压缩/_workspace/release-functional-20261021/repros/archive-completeness/assertions.json)
- [UI 首轮日志](C:/Users/im/Documents/ChatGPT/批量解压缩/_workspace/release-functional-20261021/logs/ui-test.log)；[精确交互重跑](C:/Users/im/Documents/ChatGPT/批量解压缩/_workspace/release-functional-20261021/logs/ui-three-targeted.log)；[多语言独立重跑](C:/Users/im/Documents/ChatGPT/批量解压缩/_workspace/release-functional-20261021/logs/i18n-isolated.log)
- [C# 100 项结果](C:/Users/im/Documents/ChatGPT/批量解压缩/_workspace/release-functional-20261021/native-dotnet-results.json)；[安全审计](C:/Users/im/Documents/ChatGPT/批量解压缩/_workspace/release-functional-20261021/security-audit.json)；[属性顺序确定性对照](C:/Users/im/Documents/ChatGPT/批量解压缩/_workspace/release-functional-20261021/repros/security-policy/property-order-proof.json)
- [Rust 首轮](C:/Users/im/Documents/ChatGPT/批量解压缩/_workspace/release-functional-20261021/logs/rust-tests.log)；[Rust 复跑](C:/Users/im/Documents/ChatGPT/批量解压缩/_workspace/release-functional-20261021/logs/rust-tests-no-sqlite-debug.log)；[完整测试执行器](C:/Users/im/Documents/ChatGPT/批量解压缩/_workspace/release-functional-20261021/run-suite.ps1)

**主要执行命令**

```powershell
dotnet run --project tests/OpenUnpack.Tests/OpenUnpack.Tests.csproj -c Release -- .
npm test -- --reporter=default                    # apps/desktop-ui
npm test                                        # apps/mcp-connector
dotnet publish src/OpenUnpack.Worker/OpenUnpack.Worker.csproj -c Release -r win-x64 --self-contained true -p:RuntimeFrameworkVersion=10.0.13 -o <run>/candidate/worker
./scripts/verify-published-runtime.ps1 -WorkerDirectory <run>/candidate/worker
./scripts/verify-worker-v2.ps1 -WorkerExecutable <run>/candidate/worker/OpenUnpack.Worker.exe
./scripts/audit-security.ps1 -DenyWarnings -WorkerDirectory <run>/candidate/worker
cargo fmt --all -- --check
node scripts/check-architecture.mjs
node scripts/check-contracts.mjs
cargo test --workspace --locked -- --test-threads=1
cargo test --workspace --locked --config 'profile.dev.package.libsqlite3-sys.debug=0' -- --test-threads=1
python _workspace/release-functional-20261021/assert-archive-completeness.py
```

每条已执行套件的工作目录、起止时间、退出码、原始日志保存在机器汇总和 `*-results.json`。过滤重跑的 skipped 仅表示没有选择执行，不是新增加的全套豁免。
