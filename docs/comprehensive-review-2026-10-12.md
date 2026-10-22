# OpenUnpack 全面检查报告（2026-10-12）

审查结论：**changes requested，当前候选不批准稳定版发布**。现有自动化回归通过，但新增定向检查复现了4条后台异常路径，并发现3条前端安全断言失败。另有音效职责重复和验收资料退回旧版本的问题。

本次按 AGENTS.md 的 `/review` 流程完成生产路径与跨语言审查，主代理复核子代理结论并实际运行定向验证。测试均使用合成压缩包、独立任务库/密码库和临时目录，没有读取个人数据库，没有安装正式产品，也没有强杀其他会话的遗留进程。本轮新增审计报告与隔离复现用例，**未修改业务代码、未新增设置开关**。

## 1. 当前真实实现与候选身份

- 生产 Host 仍使用 Core `Coordinator`。当前 Core 通过 `JoinSet` 支持 **1–4个归档并发**；单个 Worker 进程处理一个活动请求。这不是单归档内部线程数或密码候选并行度。
- `TaskCoordinator` 完整执行链未接入；不能把新层修复当成生产已切换，也不能再把当前 Core 说成只能串行。
- 184个已记录源码/配置文件与前次候选源码摘要比较：183个相同，只有 `scripts/verify-installer-isolated.ps1` 不同。生产 Rust、TS、C# 实现没有发现相对该摘要的变化。
- 便携候选、源码压缩包、普通 NSIS 候选的字节数与 SHA-256 全部符合 `package-lineage.json`。详情：`artifacts/comprehensive-review-2026-10-12/package-checks.json`。
- 候选第三方材料复核：488个组件，1030个清单内文件摘要通过；此前安装载荷统计的1033个资料文件还包含清单/说明文件，统计口径不同。

上述一致性验证不表示功能没有缺陷。下面的 Core 复现使用当前生产库和候选的真实 Worker/官方7-Zip；前端复现为组件层，不能替代原生 WebView 操作。

## 2. 已复现的问题

以下问题的优先级均为 **P2**；其中 R1–R6 是本次发版阻塞项。置信度为本次证据下的判断，不能据此推断所有场景均失败。

| ID | 问题与影响 | 位置 | 证据 / 置信度 |
|---|---|---|---|
| R1 | 调度首次写库失败后，队列停止，任务仍显示 running，正常重试被拒绝 | `crates/core/src/coordinator.rs:914`、`:922` | 隔离SQLite第二连接持写锁超过2秒；`globalRunning=false`、任务`running`、无workspace、retry报错。100% |
| R2 | 规则变化后重试，旧目录清理失败会丢失workspace关联 | `crates/core/src/coordinator.rs:1335` | 合成旧工作区文件被无删除共享的句柄占用；清理报错后目录和文件仍存在，任务workspace却为null，界面失去清理入口。100% |
| R3 | 多任务同时使用同一缓存根会超出共享缓存配额 | `crates/core/src/coordinator.rs:1207`、`:1620` | 两个32MiB输出、48MiB共享配额；真实峰值67,116,140字节，两个任务均completed。100% |
| R4 | 缓存盘不可用而回退Direct时，直接解压仍错误受缓存配额限制 | `crates/core/src/coordinator.rs:1207` | 未挂载盘符仅作为缓存配置，未向该盘写入；输出盘Direct工作区已观察，2MiB输出被1MiB缓存配额阻止；关闭缓存后同任务completed。100% |
| R5 | 确认导入仍在等待时，取消并重开新的导入草稿，旧请求完成会关闭并清空新草稿 | `apps/desktop-ui/src/components/VaultPage.tsx:197` | deferred确认请求→取消→重开并输入→旧请求返回；预期保留弹窗的安全断言失败。100%，组件级 |
| R6 | 历史查询乱序时，旧结果会覆盖较新的记录列表 | `apps/desktop-ui/src/components/HistoryPage.tsx:21` | 第二次请求先返回newer，第一次随后返回older，最终newer消失。100%，组件级 |
| R7 | 仅进度变化也会重新读取完整历史，增加大队列复制和IPC负担 | `apps/desktop-ui/src/components/HistoryPage.tsx:28`、`:31` | 相同任务状态、只改progress后history调用从1次变2次；未测长期/万条队列性能。100%，组件级，warning |

后台结果：`artifacts/comprehensive-review-2026-10-12/core-reproduction-results.json`，4/4为`reproduced=true`。第一轮3项结果也保留，不覆盖原始证据。配额复现采用后端允许的缩小合成数值；设置界面的GiB步进并不能消除同一竞争窗口。

前端结果：`artifacts/comprehensive-review-2026-10-12/ui-regression-final-results.json`。4项定向测试中 **1通过、3失败**；失败断言分别对应R5、R6、R7。这些用例单独放在 `apps/desktop-ui/audit/`，不混入默认91项基线。它们断言应有的安全行为，退出码1是已定位产品缺陷，不是测试环境故障。

修复方向：

1. R1：先成功持久化再发布running；写入失败保留可重试状态并明确报告存储错误。调度状态与revision也应使用同一可见状态门，避免新状态携带旧revision。
2. R2：删除成功后才清空workspace；清理可能部分完成时仍保留目录关联和清理失败状态。
3. R3/R4：按实际路由和实际缓存根管理共享预算预留，覆盖复制、解压、取消、失败及释放；仅锁住预算读取不能解决两个Worker各持相同额度。Direct不应扣独立SSD缓存配额。
4. R5：确认导入绑定弹窗/预览代次，旧操作完成只能关闭原操作对应的弹窗。
5. R6/R7：历史查询只接受最新请求，按稳定的状态键触发；进度刷新不应触发完整历史读取。

原始压缩包在这些复现中仍保留。R2是临时结果与记录关联丢失，不应表述为已发生源包丢失。

## 3. 静态警告与证据一致性问题

| ID | 级别 / 置信度 | 发现 | 建议 |
|---|---|---|---|
| R8 | P3 / warning / 95% | Host `lib.rs:1387` 在任务完成时调用音效，Core `coordinator.rs:1023` 在整批结束再次调用；全失败批次也满足Core的`ran_any`条件。未进行听感验收，不能宣称已听到两次 | 统一通知责任层，明确按任务尝试或按批次通知 |
| E1 | P2 / warning / 100% | 当前安装验证脚本相对已发源码包退回旧版：只检查一次数据目录、把任意非零退出码视为阻断、缺少setup-executions记录；finally可能清掉失败现场 | 恢复已验证的精确退出码2、三种目录写法、执行记录和失败现场保留；回归后重新绑定源码摘要 |
| E2 | P2 / warning / 100% | 当前状态台账仍标注2026-10-11 19:50；最新2026-10-12验证报告从工作区缺失，但源码包中均有更新版本。导致Supervisor/迁移/包级验证结论失真 | 以源码包和现存执行证据合并恢复最新文档，保持唯一状态来源；不能推断由谁覆盖 |

E1对比副本：`artifacts/comprehensive-review-2026-10-12/packaged-verify-installer.ps1`。E2两份源码包文档副本也在该目录，供核对。**已留存的安装4/4与退出码[2,2,2,0,0]证据仍有效；本轮没有重装正式产品，当前退回的脚本不能自动承接旧证据。**

`git diff --check`另有3处文件末尾空行提示，属于格式清理项；没有把换行风格警告当成构建失败。

## 4. 本次实际运行的检查

| 范围 | 结果 | 日志（均在`artifacts/comprehensive-review-2026-10-12/`） |
|---|---|---|
| 前端TypeScript与生产构建 | 通过 | `ui-build.log` |
| 现有界面自动化基线 | 91/91通过 | `ui-tests.log` |
| Rust库单测，排除Host | 98/98通过：Application9、Core74、Domain8、Infrastructure7 | `rust-lib-tests.log` |
| Host发版模式单测 | 5/5通过 | `host-tests.log` |
| 实际Worker/Engine测试 | 36/36通过，含加密/分卷/RAR/路径/大清单/取消 | `dotnet-tests.log` |
| 跨语言契约 | 16桌面DTO、7WorkerDTO、178字段；19实际往返+6金样通过 | `contracts.log` |
| 当前生产并发集成 | 3/3通过：双任务输出、单任务取消、同名发布 | `concurrent-pipeline-tests.log` |
| 新增后台异常复现 | 4/4复现缺陷 | `core-reproductions-final.log`与JSON |
| 新增界面安全回归 | 1通过、3失败，问题已定位 | `ui-regressions-final.log`与JSON |
| 候选3包摘要、材料 | 全部匹配 | `package-checks.json`、`materials-audit.log` |

主要执行命令（独立target，无竞争构建）：

```powershell
npm --prefix apps/desktop-ui run build
npm --prefix apps/desktop-ui test
./scripts/check-contracts.ps1 -RunCargoTest
cargo test --workspace --exclude openunpack-desktop --lib --locked --offline -- --test-threads=1
cargo test -p openunpack-desktop --release --lib --locked --offline -- --test-threads=1
dotnet run --project tests/OpenUnpack.Tests/OpenUnpack.Tests.csproj -c Release --artifacts-path artifacts/comprehensive-review-2026-10-12/dotnet -- .
cargo test -p openunpack-core --test desktop_pipeline concurrent_jobs --locked --offline -- --test-threads=1
cargo run --manifest-path artifacts/comprehensive-review-2026-10-12/rust-repro/Cargo.toml --offline
# 在apps/desktop-ui目录运行，独立审计套件，不替代默认基线：
./node_modules/.bin/vitest.cmd run --config audit/vitest.config.ts --reporter=json --outputFile=../../artifacts/comprehensive-review-2026-10-12/ui-regression-final-results.json
node scripts/audit-release-materials.mjs artifacts/remaining-fixes-2026-10-11/candidate/release-materials
```

Rust运行前导入项目MSVC/SDK环境。非Host构建复用本会话`remaining-inspect`独立target，Host使用`remaining-fixes`独立target；同一target按序运行。复现工具需要`OPENUNPACK_REVIEW_ROOT`，并从明确的candidate目录选择Worker。日志保留实际路径/退出码。原有完整26项Core管线及迁移11项证据经摘要核对保留，本轮没有无理由重复所有耗时套件。

## 5. 已排除的误判

- 普通密码保存期间“取消→重开→迟到结果清空草稿”的假设被排除：当前Ant Design 6的`Modal.handleCancel`在`confirmLoading=true`时直接返回，本次实际取消点击未关闭弹窗，保护测试通过。导入使用自定义footer，仍存在R5。
- “当前生产仅单活动作业”已过时；现有3项并发集成本次实际重跑通过。
- 不能把未切换TaskCoordinator、原生待验收、未知超时原因，计为已复现的生产崩溃。

## 6. 仍未完成的架构与发版验收

架构/功能范围仍需另行完成：完整TaskCoordinator执行与新层速度事件、Infrastructure反向Core依赖、队列分页/增量与峰值内存、真实归档元数据路由及探针预算、协议能力硬编码与null/整数兼容债、遗留inspect临时清单清理。归档内容预览UI、CSV导入、监听目录、Explorer菜单等待开发功能不属于本次检查新增缺陷。

按用户此前“先完成代码与后台验证”的选择，**当前候选的原生套件A/B/C/Rules未执行**。文件夹、多选、真实OLE拖放、导入/恢复/重启、物理跨盘界面与缩放验收仍待补。历史选择器Promise超时根因仍未确证，不能仅凭后来的锁屏/断开状态宣称已定位。

干净Windows机器、WebView2缺失及实际离线安装、真正跨版本升级/回滚、另一账户、多显示器/系统DPI、源删除完整UI、故障注入组合、长期运行、万条队列/峰值内存仍缺验收。既有隔离安装验证只证明同机器审计产品安装、重复安装、卸载保留合成数据，不证明上述外部条件。

本轮开始时遗留PID20220/30948仍存在，未操作这些其他会话进程。所有新构建和夹具均按独立所有权隔离。

## 7. 收尾顺序

先修复R1–R6并让新增安全回归通过，再处理R7/R8及E1/E2，重建绑定统一源码的候选包，最后补当前候选原生验收和独立安装/升级环境。当前基线通过不能覆盖新增失败，本报告不能用作稳定版放行证据。
