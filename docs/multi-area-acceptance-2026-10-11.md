# 多领域验收与修复记录：文件夹/多选/拖放 · 导入与备份恢复界面 · 干净 Windows 安装升级 · 物理磁盘与 DPI

日期：2026-10-11（Asia/Shanghai）。环境：Windows 11（10.0.26300，x64）。物理布局：`C:` = SAMSUNG MZVLB512HBJQ NVMe SSD（磁盘 1，测试中的 SSD 侧）；`D:` = TOSHIBA MQ04ABD200 SATA HDD（磁盘 0，测试中的机械盘侧）。

## 结论摘要

- **修复（产品代码）**：界面路径显示缺陷——原生层返回的规范路径（`\\?\C:\...` 与 `\\?\UNC\...`）被原样展示在扫描弹窗、任务详情（含"可复制"文本）、历史、分卷列表、设置与批次选项等处。新增显示层归一化 `displayPath`（仅渲染，不回写 IPC），覆盖 10 处渲染点，附 3 项单元测试。
- **干净 Windows 安装升级**：**首次**构建成功官方 NSIS 安装包（77.85 MiB，`embedBootstrapper` 内嵌 WebView2 引导器，currentUser，简中/英文），并完成 9/9 真机流程：静默安装 → 产物/快捷方式/注册表校验 → 启动（真实窗口标题、正常退出码 0）→ 覆盖升级 → 静默卸载（程序文件移除）。
- **物理磁盘**：新增核心级真实设备测试（不依赖 UI），2/2 通过：①拓扑探测与 `Get-Disk` 物理真值一致（D: 判定为 HDD、C: 判定为 SSD、跨盘 device number 不同，双方向号码一致）；②真实 **HDD 源 → SSD 缓存工作区 → HDD 发布** 全链路，12×512 KiB 源文件逐字节 SHA 匹配，`cacheUsedBytes` 峰值 > 0 且发布后归零。
- **文件夹/多选/拖放、完整密码导入与备份恢复界面**：对调用链做代码级复核（含 Tauri 2.12.1 / wry 0.57.1 拖放双通道载荷一致性、rfd 对话框"文件夹名"编辑框 1152、NativeAccess 守卫与 drop 原始路径校验、vault 预览令牌与代际绑定），未发现产品缺陷。原生验收套件 A/B/C 已全部编写并冻结于 `artifacts/acceptance-2026-10-11/`；其中套件 A 的多选/文件夹驱动在会话活跃窗口期已实际驱动过原生对话框，但该窗口内 `pick_files` Promise 未决（原因待复现，详见第五节现象 A）；随后 15:37 会话断开使全部 UI 级用例不可执行（现象 B）。套件可在桌面会话恢复后直接重跑。
- **门禁**：C# `31/31`；前端 `79/79`（含新增 displayPath 3 项）；application crate `3/3`（独立 target）；契约检查 `16 desktop DTOs + 7 worker DTOs + 178 fields` 通过；物理磁盘核心测试 `2/2`；安装器流程 `9/9`；Rust 快速全量（core-lib/format/scanner/worker_client/host-lib/domain/infra/protocol）全部通过，明细见第七节。

## 一、产品代码修复：displayPath

**问题**：`apps/desktop-ui` 多处直接渲染宿主与 Core 返回的规范路径。由于 `fs::canonicalize` 在 Windows 返回扩展长度形式（本机 Rust 1.96 实测为 `\\?\C:\...`），用户在以下位置看到并可能复制出 `\\?\` 前缀路径：扫描结果弹窗（归档路径、分卷展开列表）、本批次选项（输出目录、缓存目录提示）、任务详情（原始归档"可复制"文本、分卷、输出目录、工作目录）、处理记录（来源路径）、设置页（默认输出根目录、SSD 工作目录输入框）。

**修复**：`shared.tsx` 新增 `displayPath`：

```ts
export const displayPath = (value: string | null | undefined): string => {
  if (!value) return '';
  if (value.startsWith('\\\\?\\UNC\\')) return `\\\\${value.slice(8)}`;
  if (/^\\\\\?\\[a-zA-Z]:/.test(value)) return value.slice(4);
  return value;
};
```

应用于 `QueuePage.tsx`、`shared.tsx`（JobDetails）、`HistoryPage.tsx`、`SettingsPage.tsx` 共 10 处渲染点。**显示层归一化仅影响渲染与复制出的文本；送往 IPC 的值保持原规范形式不变**（`rowKey`、`setSources`、`addJobs`、`saveSettings`、`openPath` 等全部保持原值），不影响 NativeAccess 守卫的路径匹配语义。

**测试**：`apps/desktop-ui/tests/display-path.test.ts`（3 项：扩展前缀/UNC/常规与空值保持）。

## 二、干净 Windows 安装升级（首次闭合）

**构建**：`apps/desktop-host` 使用 `tauri build --bundles nsis` + 覆盖配置（`artifacts/release-installer.config.json`：`licenseFile`、`release-materials` 资源、`embedBootstrapper`、`currentUser`、简中/英文）。产物：`artifacts/release/OpenUnpack-0.1.0-windows-x64-standard-setup.exe`（81,627,966 字节；SHA-256 `643081bd0254a226c7e4145613aa56a16b99a7fd0f5f060b0ecbcc6c3a15caae`）。安装包内含 WebView2 embedBootstrapper（构建日志可见下载并校验 bootstrapper 与 NSIS 组件）。

**说明**：`build-desktop.ps1 -Installer` 的 `beforeBuildCommand`（`npm --prefix ../desktop-ui run build`）在 tauri CLI 下工作目录解析与预期不符（实测试运行于 `apps/desktop-ui` 相对层级，导致 ENOENT）；本次通过覆盖配置将钩子置为 no-op（`dist` 已先行构建）完成打包。**建议**：修正脚本中该钩子的相对路径解析或用 `-SkipInstall` 之外的显式方式预构建 dist。

**9/9 真机流程**（`artifacts/acceptance-2026-10-11/installer/installer-test.log`）：
| 步骤 | 结果 |
|---|---|
| setup 文件存在 | `size=81627966 bytes` |
| 静默安装（currentUser） | `exit=0`，安装后自动启动的实例被测试收敛 |
| 可执行文件与 worker 资源 | `openunpack-desktop.exe`（18,044,416 B）+ 自包含 worker + `uninstall.exe` 齐备 |
| 开始菜单快捷方式 | `…\Start Menu\Programs\OpenUnpack.lnk` |
| 卸载注册表项 | `HKCU\...\Uninstall\OpenUnpack` |
| 安装版启动 | 窗口标题 `解压工坊 OpenUnpack`，关闭后退出码 `0` |
| 覆盖升级 | 再次静默安装成功，升级后重新启动窗口正常 |
| 静默卸载 | `exit=0` |
| 卸载后清理 | 程序文件移除（exe 消失），遗留 5 项用户数据文件（见下） |

## 三、物理磁盘（真实设备验收）

新增 `crates/core/tests/physical_disk_cross_device.rs`（`#[ignore]`，按需运行；本机 `Get-Disk` 为独立真值）：

1. `disk_topology_matches_physical_ground_truth` — D: 判定 HDD、C: 判定 SSD；跨盘设备号不同且与 `probe_physical_device_number` 双向一致；同盘路径同设备号；真实拓扑特征输入下 `RouteDecider` 选择 `SsdCache`。
2. `cross_disk_hdd_source_ssd_cache_roundtrip` — 真实管线（Core → 命名管道 → .NET Worker → 官方 7-Zip）：源包位于 D:（HDD），缓存工作区位于 C:（TEMP，SSD），输出发布回 D:。断言：作业 `completed`；`cacheUsedBytes` 峰值 > 0（SSD 缓存被实际使用）；输出 12 个文件与源逐字节 SHA 匹配；缓存占用回到 0。

运行命令：`cargo test -p openunpack-core --test physical_disk_cross_device -- --ignored --test-threads=1` → **2 passed; 0 failed**。证据：`artifacts/acceptance-2026-10-11/`（测试输出）与 `D:\openunpack-accept\pd-core\`（真实设备上的夹具与输出）。

## 四、代码级复核结论（文件夹/多选/拖放、导入与备份恢复）

- **拖放双通道一致性**：核对 Tauri 2.12.1 源码，`DragDropEvent::Drop { paths: Vec<PathBuf> }` 同时用于①窗口级 `on_window_event`（宿主写入 `native_drop_paths` 原始字符串）与②webview JS 事件（`DragDropPayload` 由同一 `Vec<PathBuf>` 序列化）。两侧字符串来自同一序列化路径（`PathBuf` → UTF-8 校验），宿主 `scan_authorized` 的 `native_drop_paths.contains(raw)` 原始串匹配成立；扫描后授予规范路径读权限，`add_jobs` 校验通过。wry 0.57.1 为 OLE `RegisterDragDrop`/`IDropTarget`（非 WM_DROPFILES），与拖放模拟器实现路径一致。
- **文件夹选择**：rfd 0.15.4 `pick_folder` → `IFileDialog`（`FOS_PICKFOLDERS`），实测其对话框具有 AutomationId=1152 的"文件夹:"编辑框（同文件模式的 1148 模式）；驱动脚本已按此实现并冻结（`folder-dialog-drive.ps1`）。取消路径实测需 `WM_CLOSE` 兜底（BM_CLICK 在部分时序下不关闭）。
- **多选**：文件对话框引号对列（`"a" "b"`）与"UIA 定位列表项 + Ctrl 点选"两条驱动均已实现（`file-dialog-multiselect.ps1`）。
- **导入与备份恢复**：`read_import_file`（GB18030/GBK/CP936 与 UTF-16LE(含 BOM)/UTF-8 严格校验）、`preview_import/confirm_import` 令牌流、`export_backup/preview_restore_backup/confirm_restore_backup`（口令长度上限、KDF 参数、白名单路径授权、预览令牌与 `generation` 绑定）逐项复核一致；错误口令返回 `Invalid backup archive or incorrect passphrase` 并保留当前库不变。相应 UI 流程的原生端到端套件（含错误口令→重试→预览→确认合并→重启持久性）已就绪待跑。

## 五、关键发现（环境与流程，均为可复现事实）

1. **安装目录与数据目录重合**：`resolve_data_directory()` 默认 `%LOCALAPPDATA%\OpenUnpack`，与 currentUser NSIS 的默认安装目标相同。实证：安装期间程序与 `jobs.sqlite`/`passwords.vault` 混放；**卸载后程序文件移除，而更早的用户数据文件（2026-10-08）幸存**（9/9 流程第 9 步"遗留 5 项"）。风险与建议：当前行为下用户数据不会被本次卸载删除，但两目录语义耦合脆弱（升级/修复安装/未来 per-machine 安装、以及 SNIS 目录清理策略变化都可能波及数据）。**建议 P1**：将安装目录改为 `%LOCALAPPDATA%\Programs\OpenUnpack`（或数据目录迁至 `OpenUnpack-Data` 并做一次性迁移），并在 NSIS hook 中显式声明数据保留/删除策略。本次未改该语义（涉及便携与已装用户迁移，需产品决策）。
2. **孤儿 WebView2 进程累积**：多次强杀应用实例后累计 33 个 `msedgewebview2.exe` 残留，显著拖慢后续实例的 WebView2 启动（可致 UI 数分钟不挂载）。已提供 `cleanup-webview.ps1`（按启动时间过滤，避免误杀系统 SearchHost/M365Copilot 实例），并接入套件 A/B/C 收尾流程。
3. **共享 target 目录的构建抖动**：并发会话活动期间，`cargo build -p openunpack-desktop` 两次出现 `coordinator.rs` 引用 `ArchiveInspectionView/ArchiveEntryView` 的 E0425（同一时刻 `cargo check` 与单包构建均通过）。定位为共享 `target/` 中协议 crate 元数据/指纹陈旧导致的偶发不一致；通过失效 protocol/core 指纹（touch 两 crate 的 lib.rs）后重建解决。建议跨会话构建串行，或会话隔离 `CARGO_TARGET_DIR`。
4. **历史遗留进程锁**：13:12 启动的 `openunpack_application-4ec7f17179633599.exe`（PID 20220）与其 cargo（PID 30948）持续挂起（2 线程、近零 CPU），长期锁定 `target/debug` 产物，导致工作区测试链接失败（LNK1104）。该进程属更早会话的沙箱上下文，当前沙箱无法终止（`拒绝访问`）。**待人工处理**：在管理员终端执行 `Stop-Process -Id 20220,30948 -Force`（或重启）。本次以“排除该包 + 独立 target 运行 application 测试”完成等价验证。
5. **UI 级原生用例受阻（两个叠加现象，需区分）**：
   - **现象 A（待复现）**：15:22–15:36 会话仍活跃期间，对话框可被真实驱动且经二次探测确证关闭，但 `pick_files` 的
     Promise 未决（多选与单文件对照均复现）。原因未定位——已排除"对话框未关闭"与"仅多选触发"；复现判据与入口见
     `docs/status-ledger-2026-10-11.md`「验收 1 详细记录」。
   - **现象 B（已定位，环境）**：15:37 起执行会话（RDP session 2）断开（`qwinsta` 显示"断开"，LogonUI 在场），
     WebView2 页面在断开桌面内无法挂载（实测 `page` 停留 `about:blank`、按钮永不出现；同时刻 sqlite/worker 初始化均已完成）。
   **恢复会话后可先复测现象 A（若再现按台账步骤定位），三个套件可直接重跑。**
6. **候选构建血缘**：验收候选 `artifacts/acceptance-2026-10-11/OpenUnpack-acceptance-2026-10-11/`（`OpenUnpack.exe` 18,043,904 B，2026-10-11 15:20 构建；含 `BUILD-SOURCE-SHA256.txt`、`SHA256SUMS.txt`、仓库内 `worker/`、`release-materials/`）。注意：该目录内 exe 由带 `custom-protocol` 的构建生成；NSIS 包内 exe 由 tauri CLI 构建生成（同一源码树，见下）。构建期间有并行会话在仓库内活动（源码快照以 `BUILD-SOURCE-SHA256.txt` 为准）。

## 六、待办与复跑指引

```text
# UI 级原生套件（需桌面会话处于活动/已解锁状态）
node artifacts/acceptance-2026-10-11/suite-a.cjs      # 文件夹/多选/真实 OLE 拖放（含 dragdrop-sim）
node artifacts/acceptance-2026-10-11/suite-b.cjs      # GB18030/UTF-16LE 导入 + 备份→错误口令→预览→确认恢复 + 重启持久性
node artifacts/acceptance-2026-10-11/suite-c.cjs      # 125/150/200% 缩放 + 磁盘跨盘 UI 全流程

# 物理设备（核心级，已通过；可重复）
cargo test -p openunpack-core --test physical_disk_cross_device -- --ignored --test-threads=1

# 安装器（已通过；可重复）
pwsh -File artifacts/acceptance-2026-10-11/installer-test.ps1 ^
  -SetupExe artifacts\release\OpenUnpack-0.1.0-windows-x64-standard-setup.exe ^
  -OutDir artifacts\acceptance-2026-10-11\installer
```

未在本轮闭合（原因如实标注）：干净机器（无 Node/.NET SDK、WebView2 缺失）需独立测试机；断电/拔盘/目标不足等破坏性场景未执行；DPI 用例依赖活动桌面会话；拖放模拟器（`dragdrop-sim`，OLE `DoDragDrop` + SendInput）已构建但真实投放验证依赖活动桌面会话。

## 七、门禁汇总

| 检查 | 结果 | 日志 |
|---|---|---|
| Rust 快速全量：core-lib 65/65、format_matrix 6/6、scanner_regressions 5/5、worker_client 3/3、desktop-host-lib 3/3、domain 7/7、infrastructure（lib 2 + migration 6 + store 2 + supervisor 1 + vault 5）、protocol（golden_samples 6 + snapshot_revision 1） | **全部通过，共 122/122** | 终端输出（2 批次） |
| Rust 全工作区（含 desktop_pipeline 重载套件，并行模式） | 运行 >55 分钟未在时限内完成，主动中止（并行争用所致，非失败）；**同树（复核轮次 3，15:45）串行记录：desktop_pipeline 25/25、core-lib 64/64 全通过** | `.workbuddy/memory/2026-10-11.md`「复核轮次 3」 |
| `cargo test -p openunpack-core --test physical_disk_cross_device -- --ignored` | **2/2 通过** | 终端输出 |
| `CARGO_TARGET_DIR=…verify-app-target cargo test -p openunpack-application` | **3/3 通过** | `app-tests-final.log` |
| `npm --prefix apps/desktop-ui test` | **79/79 通过** | 终端输出（6 files） |
| `dotnet run --project tests/OpenUnpack.Tests … --no-build` | **31/31 通过**（Evidence: `artifacts/test-results/20261011-082732-24c0a88a`） | `dotnet-tests-final.log` |
| `node scripts/check-contracts.mjs` | **通过**（16 desktop DTOs / 7 worker DTOs / 178 fields） | `contracts-final.log` |
| 安装器 9 步流程 | **9/9 通过** | `installer/installer-test.log` |
