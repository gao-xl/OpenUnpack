# S06 目录身份修复与候选包验证报告

执行日期：2026-10-14 至 2026-10-15。范围：当前生产链 React/Tauri → `crates/core::Coordinator` → WorkerClient → .NET Worker → Engine。遵循 [执行方案](s06-directory-identity-plan-2026-10-14.md)，保留全部既有未提交修改；未切换生产架构，未提交或推送代码。

## 1. 结论与威胁模型

S06 在下述本机 NTFS 威胁模型范围内完成代码整改和自动验证。关键文件操作从“检查路径后重新打开”改为“持有父目录和目标对象句柄，再对同一对象操作”。修复覆盖发布、回读、源清理、工作区恢复、工具箱、清单消费、分卷输入和来源标记；另外修复缓存等待分类与打包取旧文件风险。候选包仍须经过原生桌面、干净机器、安装升级/卸载及真实设备验收，不能由后台测试替代。

保护对象包括原始归档、已校验输出、应用拥有的工作目录及恢复依据。归档名称、内容和元数据均不可信；其他账户对已批准目录的修改纳入本轮威胁模型。不建立对抗当前用户恶意进程、管理员、内核失陷或历史已授予修改权限句柄的隔离边界。私有 ACL 证明当前访问边界，不能撤销旧句柄已授予的属性或安全修改权限；新工作目录因此在创建时原子赋予私有 ACL。

原始 S06 风险等级 Medium，根因置信度 High。下面等级表示修复前风险；通过测试表示在注明范围内有证据，不表示所有设备和文件系统均已验收。

## 2. 实施结果

| 问题 / 风险与置信度 | 影响 | 修复及失败行为 |
|---|---|---|
| 祖先目录替换、junction 竞态 / Medium、High | 写入、读取或删除偏离原批准对象 | Rust 与 C# 从根到叶持有目录对象，按父句柄逐组件打开、创建和枚举；拒绝 reparse 点。只固定末端句柄或缓存祖先检查均不足以满足要求。 |
| 发布回读与 rename 分离 / Medium、High | 校验结果和最终发布对象不同 | 回读前固定整树，按同一对象计算 SHA-256、核对清单、归属及取消状态。NTFS 根目录改名必须先释放子句柄，因此先证明根/继承边界及全部子项私有 ACL，再释放子项，根句柄保留到禁止覆盖的提交。普通发布和恢复发布共用此路径。 |
| 工作目录在独立请求间失去保护 / Medium、High | 下一次 Worker 请求可能写入替换目录 | 创建时原子设置受保护的当前用户/SYSTEM/Administrators ACL；旧工作目录在恢复写入前验证私有权限。租约跨 Worker 请求及间隙保留，清理前显式释放。发布临时目录也覆盖复制到整树回读的生命周期。 |
| 源清理重新解析路径 / Medium、High | 校验后误删替换归档或错误分卷 | 先固定全部源卷，再复核卷号、文件号、大小与修改时间；使用原句柄删除。任一对象变化即停止源清理。 |
| 回收站交接存在路径和永久删除歧义 / Medium、High | 清理错误对象、误永久删除或无法恢复原位置 | 先排除既有删除/内容写句柄，验证单链接源、owner/DACL 及父链，保持同对象租约到 Shell 返回。独立 STA 调用 IFileOperation，要求回收标志、未中止和非空成功回收条目；没有永久删除回退。多主体可写、未知 ACL、多硬链接或歧义路径均保留源。 |
| 清理失去 ownership 标记 / Medium、High | 删除一部分后无法安全重试 | 预先固定整树，ownership 最后删除；最后根删除失败时通过同一目录句柄恢复原标记，不覆盖新文件。锁冲突保持工作目录关联及可识别错误。 |
| 工具箱预览凭据只绑定大小/时间 / Medium、High | 同大小、伪造相同时间的替换文件可能沿用批准 | 摘要加入原生卷号和文件号；执行先固定所有待操作源，再复核预览。大小写改名的两个步骤、跨卷临时复制/提交/源删除都保持同一对象；目标冲突不覆盖。 |
| Worker 输入、输出、ADS 与复制分离 / Medium、High | 读取替换输入或写入错误输出 | 分卷输入租约覆盖整个操作；输出句柄保持到来源标记、提交和显式 Keep，失败回滚删除同一对象。复制先校验全部相对名称和文件/目录冲突，再创建目标。 |
| 清单或未确认退出记录的读取与删除分离 / Low、High | 删除替换记录，或丢失恢复依据 | 有界解析及 SHA-256 检查期间保留末端对象与父租约，再删除同一对象；进程创建时间不匹配、记录无效或无法锁定时保持停止/待核对状态。 |
| 极深路径和超大租约树 / Medium、High | 构造祖先链时放大内存/IO，先耗尽资源再拒绝 | 相对路径 128 层、绝对目录链 256 层、整树 200,000 对象，上限检查前置。GNU `./` 前缀按线性方式处理。归档条目预算仍独立生效，未提高现有解压上限。 |
| 缓存阶段预留释放后错误判为永久超限 / Medium、High | 并发小任务即使能依次完成，也可能停在错误状态 | 实际缓存仍有活动同缓存任务时，只有单任务本身满足配额/展开上限的请求才转为等待并自动重试。没有活动任务的静态占用及本身超大的请求仍返回容量错误；不扩大配额。 |
| Core 回读耗时遗漏 / Low、High | 完成状态缺少 verifying 阶段记录 | 用实际回读起止时间记录耗时，避免为填充数据重复一次 Worker 回读。 |
| 相对构建目录随工作路径改变 / Medium、High | Tauri 在另一个目录构建，脚本可能从原目录取到旧版同版本安装包 | 以 Cargo metadata 的绝对输出目录统一 Cargo/Tauri 环境，结束后恢复原环境变量。最终候选必须再解包核对实际 Host，不能只检查产品名和版本。 |

主要代码：`crates/core/src/{paths,saferecycle,recycle_shell,coordinator,toolbox,worker_manifest}.rs`、`src/OpenUnpack.Engine/PathIdentityLease.cs`、Worker 文件传输与引擎调用路径。构建脚本统一显式启用 `custom-protocol`，便携包在安装包最终 Host 构建后冻结。

## 3. 原生行为证据与验证方法

先运行旧行为基线，再增加负面夹具，最后使用冻结代码执行完整验证。所有密码、归档、目录替换和删除均使用隔离合成数据，不接触个人密码库。

- Windows 实测仅持有共享锁不能阻止 `FILE_WRITE_ATTRIBUTES` 建立 junction；因此关键操作必须使用父句柄相对打开，而不能只依靠共享锁。
- 原生目录枚举需要 `FILE_LIST_DIRECTORY`，只申请属性权限会返回拒绝访问。独立 QA 的最小原生探针复现并验证修正。
- 此路径中 Win32 RootDirectory 相对 rename 返回错误 87；Rust 和 C# 均改用原生 NtSetInformationFile，相同对象禁止覆盖提交。
- Windows 枚举保留原始大小写，故回读的期望/实际清单使用一致的路径分隔符与大小写规范化；独立 QA 探针验证了原生行为。
- 测试包含属性型 junction 并发转换、目录/文件改名阻断、清理失败重试、只读文件、同名保护、ACL 拒绝、硬链接、NT 路径歧义、大小写改名、清单摘要和取消。

独立 QA 对已冻结的八个关键文件完成只读审查并记录摘要；此后新增的跨请求工作目录租约、工具箱对象身份、缓存分类及构建目录修复由主任务复核并补充验证，不冒称已经再次独立审查。C# 最终 payload 的 Worker/Engine 摘要也由 QA 独立复核。

## 4. 验证记录

最终完整工作区回归为 **292 通过、0 失败、3 默认忽略**；忽略项中的跨卷专项已另行通过，另两项需要真实物理硬件。使用冻结代码、真实自包含 Worker，命令为 `cargo test --workspace --locked --offline -- --test-threads=2`。严格静态检查及格式门禁均通过，220 个冻结代码文件在验证后摘要无漂移。实际安装包和便携包均已解包并运行其中的 Worker。

已完成的专项证据：

| 验证 | 结果 | 证据 |
|---|---|---|
| 最终 Rust 完整工作区 | 292 通过、0 失败、3 默认忽略 | `workspace-final-bounded.log`、`workspace-final-counts.json` |
| 严格静态检查 / 格式 | 工作区所有目标 Clippy `-D warnings` 通过；fmt 通过 | `clippy-final-frozen.log`、`fmt-final-status.log` |
| 源码冻结 | 220 个代码文件无漂移 | `code-freeze-final.json`、`source-freeze-check.log` |
| 真实 Core → Worker → Engine 管道 | 36/36，通过 16,000 文件传输与回读，组内用时 230.93 秒 | 同上，不与完整套件重复计数 |
| C → D 跨卷移动 | 1 通过，原对象固定及冲突目标保留 | `cross-volume-final-frozen.log` |
| Rust / TS / C# 结构与策略门禁 | 22 桌面 DTO、8 Worker DTO、240 字段、6 个变异负例；4 组镜像；15 个报告/Schema 用例通过 | `contracts-final-frozen.log` |
| 修改前工具箱 / C# Release 基线 | 7 通过 / 70 通过 | 执行方案及 `artifacts/test-results/20261014-110323-9f0cd1b3` |
| C# 最终 Release harness | 80 通过、0 失败、0 跳过 | `artifacts/test-results/20261014-114519-1af73704` |
| 最终 C# junction 夹具 | 89 次转换，22 次安全成功、228 次安全拒绝，0 个外部目录文件系统事件 | 上述 harness 结果 |
| 冻结 Worker 的真实 v2 双管道 | 14 项断言通过 | `artifacts/issue-fixes-2026-10-10/v2-bdfa48b82bc3404188c27f7a822b9f7c` |
| Windows 目录路径专项 | 18 通过 | `paths-final.log` |
| 独立请求之间目录身份保留 | 1 通过，释放租约后可正常移动 | `workspace-lifetime.log` |
| 工具箱对象身份及回归 | 9 通过，跨卷专项默认忽略另跑 | `toolbox-identity-final.log` |
| 清理/调度恢复回归 | 5 通过 | `recovery-final.log` |
| 清单消费 | 7 通过，4 组镜像门禁通过 | `manifest-final.log`、`mirrors-final.log` |
| 缓存状态分类 | 修复前确定性失败，修复后通过 | `cache-budget-before-fix.log`、`cache-budget-after-fix.log` |
| 两个 24 MiB 任务共用 36 MiB 缓存 | 自动完成，峰值 25,173,105 字节 < 37,748,736 字节 | `cache-wait-after-fix.log` |
| 在线依赖与 .NET 安全分析 | .NET / npm / Windows Rust 依赖图通过，安全分析 0 警告、0 错误 | `security-audit-final-frozen.log`、同名 JSON |
| 新候选构建 / 相对目录回归 | 普通 NSIS、便携和源码构建成功；Cargo/Tauri 使用同一绝对目录，调用者环境恢复 | `build-final-absolute.log`、`build-target-restore.log` |
| 实际 NSIS / 便携 payload | 每包 195 个冻结 Worker 文件一致；1,331 个便携文件校验和通过 | `candidate-validation-final.log`、`candidate-validation.json` |
| 实际包内 Worker 双管道 | NSIS、便携各 14 项真实断言通过 | `v2-ca67a1c8efc548649b4fc42de08009d3`、`v2-f6c10e30fffc4968983815c6b424efb6`，均在 `artifacts/issue-fixes-2026-10-10/` |
| 源码发布白名单 | 377 个源文件通过审计；冻结代码及最终文档的 ZIP 一致性证据单独保存 | `source-package-final.log`、`source-package-validation.json` |

除特别注明，日志位于 `_workspace/s06-directory-identity-2026-10-14/`。历史中间失败日志保留，最终结论以最终冻结回归为准，不把专项重复执行的次数加到完整套件总数中。与 Release 编译同时运行的首轮完整管道为 34 通过、2 个 90 秒超时；停止编译负载后，同样两个用例分别在 21.32 秒和 6.23 秒通过，未修改超时或功能断言。最终完整套件限定两个测试并发，并与 Release 编译分开执行。

在线 Rust 检查载入 1,329 条公告并扫描 Cargo.lock 的 513 个 crate；两个公告仅在证明不属于 Windows 工作区可达依赖后按现有策略排除（`RUSTSEC-2024-0370`、`RUSTSEC-2024-0429`）。扫描通过只表示该时点、该依赖图和公告库的覆盖结果。

## 5. 候选包与来源核对

生成本地 Windows x64 便携包、普通 NSIS 候选安装包及源码包；包内 Worker 使用自包含 .NET Runtime 10.0.12。普通安装包带 WebView2 引导程序，缺少 WebView2 时仍需网络。本轮未制作新的完全离线安装包，也未公开发布或签发稳定版。

最终文件位于 `artifacts/release/`：

- `OpenUnpack-0.1.0-s06-20261015-windows-x64-setup.exe`
- `OpenUnpack-0.1.0-s06-20261015-windows-x64-portable.zip`
- `OpenUnpack-0.1.0-s06-20261015-source.zip`
- `SHA256SUMS-s06-20261015.txt`：上述三个交付文件的 SHA-256；避免把该文件的旧副本与新包混用。

实际包核对结果：NSIS 与便携包各有 195 个 Worker 文件与冻结版本逐字节一致，运行时均为 10.0.12；便携目录 1,331 项校验和全部一致。包内 Worker 使用各自包内的 `7z.dll` 通过真实命名管道完成协议控制、取消、有限密码探测、实际解压、独立内容比对及无秘密标准流检查，各 14 项断言通过。

Host 比对只允许 Tauri 的既定包类型差异：在同一已确认偏移，安装版为 `__TAURI_BUNDLE_TYPE_VAR_NSS`，便携/最终构建为 `__TAURI_BUNDLE_TYPE_VAR_UNK`。实测仅 3 个字节不同，已与本地 `tauri-utils 2.10.1` 源码核对；固定位置规范化后整文件 SHA-256 相同，其他任何差异都拒绝。实际 Host 摘要、标记偏移与包摘要保存在 `candidate-validation.json`，不会以产品名/版本相同代替来源核对。

冻结 Worker 位于 `artifacts/worker-s06-final2/win-x64/`，完整清单见其中 `s06-payload-manifest.json`：

| 文件 | SHA-256 |
|---|---|
| OpenUnpack.Worker.dll | `e80f4af0d401b08030d2448c37c6013e07a387f7ae893b5fb5c136898c26ff40` |
| OpenUnpack.Engine.dll | `818248e4996d2710660428e1a802a8717ea0730ad5b27d2875ccd44cf2431070` |
| 7z.dll | `899cd73d7fc60e1c1c35c7de0f5bed3dc69bfe2cecbd48014d868a347970a0b1` |
| coreclr.dll | `128aee8c62a673d64739e585e3876b61133571cec83a46240a62581d5465639b` |

源码包按白名单排除个人数据库、密码库、密钥、工具缓存和工作目录；ZIP 与 220 个冻结代码文件及 5 份最终文档逐项核对的结果见 `source-package-validation.json`。Host 与 NSIS 的 Authenticode 状态实测为 NotSigned，本轮不是已签名稳定版。

## 6. 兼容性与未关闭事项

| 事项 | 状态与后续验收 |
|---|---|
| 本机 NTFS | 本轮实际验证范围；C/D 跨卷不等于两块不同物理介质。 |
| FAT/exFAT、NAS、网络共享及特殊 ACL | 没有完成验收；无法证明私有权限时安全拒绝，保留源或工作目录。不能为兼容性跳过 ACL/对象身份检查。 |
| 历史已授予属性/安全修改权限的句柄 | 私有 ACL 不能撤销旧句柄已授予权限（包括 WRITE_DAC/WRITE_OWNER）；不作隔离保证。当前用户恶意进程、管理员和内核同样在边界之外。 |
| A07 架构迁移 | 生产仍使用 Core。Infrastructure 清单解析主体保持镜像，但其迁移辅助只固定末端对象，尚不具备生产 Core 完整父链保护；切换前须单独补齐并验证。 |
| 只读扫描和容量/卷识别 | 部分仍按字符串路径读取，不声称所有文件系统 IO 已改为对象句柄。不能据此绕过关键写入、发布或清理路径的保护。 |
| UH04 快照开销 | 完整快照成本仍需后续架构优化和压力验证，本轮未混入改造。 |
| 原生界面套件 | 按此前约定暂缓；原生选择器、拖放、托盘、密码操作和真实窗口交互仍须人工/桌面自动化验收。 |
| 干净机器和安装生命周期 | 仍需 Windows 10/11 上首次安装、缺少 WebView2、升级保留数据、卸载保留数据及回滚验证。解包和哈希一致不能替代安装验收。 |
| 真实介质和故障 | 仍需 HDD/SSD/USB/NAS、网络断连、拔盘、磁盘满、掉电/强杀后的恢复及长期性能测量。 |
| 签名与公开发行 | 本轮是本地候选；尚未完成可信代码签名、稳定版审批和公开发布。 |
| Cargo PDB 文件名碰撞提示 | 当前 Host 的库和可执行目标生成同名调试符号文件，Release 构建仍有此提示；应用目标重命名及调试符号归档需后续独立处理，不声称整个打包过程无警告。 |

下一阶段先在隔离 Windows 环境验证候选安装生命周期；原生界面验收恢复后执行完整用户操作流，再决定稳定版发布。A07/UH04 分别立项，不能用本轮 S06 关闭状态代替架构和性能验收。

## 7. 参考

Windows 权限/共享语义参见 [CreateFileW](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-createfilew) 和 [GetSecurityInfo](https://learn.microsoft.com/en-us/windows/win32/api/aclapi/nf-aclapi-getsecurityinfo)；Shell 标志及回收结果参见 [IFileOperation.SetOperationFlags](https://learn.microsoft.com/en-us/windows/win32/api/shobjidl_core/nf-shobjidl_core-ifileoperation-setoperationflags) 和 [PostDeleteItem](https://learn.microsoft.com/en-us/windows/win32/api/shobjidl_core/nf-shobjidl_core-ifileoperationprogresssink-postdeleteitem)。具体安全结论以本轮原生探针和真实生产调用链验证为依据。
