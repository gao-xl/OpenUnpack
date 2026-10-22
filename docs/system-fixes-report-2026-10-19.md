# OpenUnpack 检测问题修复记录

日期：2026-10-19。范围：2026-10-18 检测确认的四项问题、过期 UI audit 和 .NET 发布运行时基线。此轮修复候选已冻结；后续本机 API 开发使用独立运行目录，不能把此报告视为 API 验收。

## 修复结果

| 问题 | 修改和行为 | 直接回归证据 |
| --- | --- | --- |
| F01 密码库可超过备份读取上限 | 新增、导入、恢复、预览及备份共用 100000 活动条目上限；已有超限库存量可打开、编辑和删除，拒绝继续新增和导出。写入前验证本地 DPAPI 和可移植备份容量，失败保留原内存、文件和目标。 | 两份 Vault count/size/DPAPI/reopen/原子性测试；独立公共入口探针检查 100001 库拒绝新增和导出，保持哨兵与源库。 |
| F02 CRC 抢救掩盖 Unsupported | native 回调累计全部失败类型，任何 Unsupported 在返回抢救结果前拒绝，继续沿 Worker→Core 映射为 failed。 | 新 C# native 矩阵 20 例；真实 Core/新 Worker 六种条目顺序×两种模式 12/12，均无错误发布，保留源包和哨兵。 |
| F03 Unicode 大小写分卷错误分组 | Windows 分组、去重及兄弟卷发现统一使用 CompareStringOrdinal 大小写比较；保留不等价 Unicode 序列。 | 四类分卷的大小写选择矩阵与不合并反例；三种真实 Unicode 7z 分卷独立提取验证。 |
| F04 CSV 畸形引号改变密码 | 严格五态解析，拒绝字段内非法引号、闭合后的多余字符和未闭合字段，报告原始起始行；合法转义、多行和空白保留。错误仍整批拒绝。 | 合法 CSV 写入后重开比对、畸形导入原子拒绝、物理行号与 TSV 对照。 |
| UI audit 过期 | 适配实际分页 DTO 与序列化请求时序；保留迟到响应不可见及纯进度不重新查询断言。 | 原 4 通过/2 失败 → 6/6 通过。 |
| DS01/DS02 发布运行时与审计 | 自包含 Worker 更新为 .NET 10.0.13；发布、法律材料和审计使用共享精确版本策略，核对实际运行时文件并检查官方后续安全服务更新。 | 新载荷版本/构建身份/哈希通过；37 项 runtime policy 与 15 项解析用例通过；新 Worker v2 安全探针通过。 |

## 验证汇总

- C# 原生测试：100 通过、0 失败、0 跳过；构建 0 warning / 0 error。
- Rust library：Core 188 通过、1 ignored；Infrastructure 54 通过。ignored 不记作通过。
- Core 集成：8 组共 65/65；独立混合归档全链 12/12；导入、备份、Unicode 和存储门禁 27/27。
- UI：默认测试 148/148，独立 audit 6/6，lint/build 通过。
- 镜像自检：5/5；Worker v2 新载荷探针 6/6。
- 安全审计：实际 .NET 10.0.13 身份、官方在线服务基线、4 个 NuGet 项目、Roslyn、Windows Cargo 图及 npm 检查通过。Cargo 保留 3 项已允许的 unmaintained 警告（zbus_names、zvariant、zvariant_derive），不代表零警告或不存在未知漏洞。
- 新 Host release 构建通过；法律材料收集通过，580 components / 1125 files；修复候选清单 1332 files。

## 证据及候选绑定

根目录：`_workspace/repair-20261018/run-20261018-fixes-01/`。

- `before/`、`source-manifest-before.json`：改动前快照。
- `source-manifest-after-fixes.json`：修复阶段冻结的 341 个输入；`results/changed-source-files.json` 列出其中 13 个变化。新增 runtime policy/helpers/tests 单独见 `02_runtime.md`。
- `logs/` 和 `results/`：真实命令退出码、日志、独立断言和载荷身份。`03_qa.md` 为读取上述证据后的独立审查，修复范围 approved。
- `candidate/OpenUnpack/OpenUnpack.exe`：修复候选，SHA-256 `20ADFA8AB1E24A7448DE7B6DB2249394ECA06277ABB4E3C1AF2B93DDC56C8593`。完整文件身份在 `results/candidate-manifest.json`。
- `payload/`：实际用于新 Worker 回归的自包含 .NET 10.0.13 载荷，身份在 `results/published-runtime.json` 和 `payload-manifest.json`。

候选绑定修复阶段源码，后续 API 更改另行构建验证；工作区继续保留既有修改，未暂存、提交或推送。

## 保留边界

十万条测试覆盖上限、备份编码/解码和拒绝路径；没有执行十万条完整 O(n²) merge 性能验收。Windows 文件名比较结论不扩展为所有文件系统及大小写敏感目录支持。

此报告不将自动测试等同于正式发行验收：原生窗口交互、剪贴板/选择框、干净 Windows 安装/升级/卸载、签名、回滚、真实故障磁盘/SMB/USB/VHD、长时间负载，以及 S06 未闭合的真实目录替换竞争仍需各自证据。此轮未构建或安装新的 NSIS 包；Clippy/format 等后续门禁不得倒填为已执行。
