# OpenUnpack 新功能规划建议

## Context

截至 2026-10-13，生产调用链是 React/Tauri → crates/core → Worker → Engine。domain/application/infrastructure 是迁移中的模块；尚未具备发布事务、恢复与重启行为的完整对等性，不能切换生产入口。

批量队列、嵌套展开、密码库、TXT/TSV/CSV 导入、SSD 缓存、分卷识别、归档浏览、密码派生和磁盘仲裁已有代码。实现状态与原生桌面、真实磁盘、干净机器和安装升级验收分别记录；全面审计整改与验证见 docs/comprehensive-fixes-report-2026-10-13.md。

下面保留早期功能建议以便追溯。其中归档浏览、密码派生和基础磁盘识别已经实现，后续工作限于选择性解压、缓存策略、行为对等和真实设备验收；不应将这些条目当作全部尚未实现的功能。

基于对 `software-spec.md`（软件规范）与 `functional-design.md`（功能设计说明书）中 P1/P2 路线图的交叉比对，以及对当前代码库实际实现能力的深度审计，以下是按**投入产出比从高到低**排序的功能建议。

---

## 建议一：监听文件夹自动入队（Watched Folder / Hot Folder）

**推荐指数：⭐⭐⭐⭐⭐（最高）**

### 为什么推荐

这是用户使用频率最高、体验提升最明显、且实现成本相对可控的功能。核心场景：用户将下载目录（如 `D:\Downloads`）设为监听文件夹，新下载的压缩包自动进入解压队列并尝试密码解压，无需手动拖拽。

### 涉及层次与改动范围

| 层 | 改动 |
|---|---|
| **Rust Protocol** | `Settings` 新增 `watched_folders: Vec<String>` |
| **Rust Core** | 新增 `watcher.rs` 模块，使用 `notify` crate 监听文件系统事件，变更时调用 `scanner::scan` → `coordinator::add_jobs` |
| **TypeScript UI** | `SettingsPage.tsx` 增加监听文件夹列表管理（添加/移除/暂停） |
| **C# Worker** | 无改动 |

### 关键设计决策（需用户确认）

- 监听粒度：仅新增文件 vs 新增+修改完成？
- 是否支持递归监听子目录？
- 文件锁检测：下载中的文件如何判断已完成？（稳定性窗口：如 5 秒内大小不变则入队）
- 自动入队后是否立即开始解压还是仅排队等待手动启动？

---

## 建议二：资源管理器右键菜单集成（Shell Context Menu）

**推荐指数：⭐⭐⭐⭐⭐（最高）**

### 为什么推荐

桌面解压工具的核心交互入口。用户在任意文件夹中右键点击压缩包 → "使用 OpenUnpack 解压"，无需先打开软件再拖拽。与 WinRAR / 7-Zip 竞品体验对齐。

### 涉及层次与改动范围

| 层 | 改动 |
|---|---|
| **Rust Desktop Host** | 新增 Tauri 插件或独立注册表/COM 注册模块，处理 Shell Extension 或 IPC 转发 |
| **Rust Core** | 接收外部 IPC 路径参数，复用现有 `scan_sources` + `add_jobs` |
| **Windows Installer** | 安装/卸载时注册/注销右键菜单（Registry `HKCU\Software\Classes` 或 `IContextMenu` COM） |
| **TypeScript UI** | 设置页增加"启用/禁用右键菜单集成"开关 |

### 关键设计决策

- 使用轻量级注册表方案（`shell\open\command`）还是完整的 Shell Extension COM？
- 右键菜单项数量：仅一个"使用 OpenUnpack 解压"还是提供子菜单（解压到此处 / 解压到指定目录 / 添加到队列）？
- 是否支持多文件右键选中批量发送？

---

## 建议三：压缩包内容预览（Archive Content Preview）

**推荐指数：⭐⭐⭐⭐**

### 为什么推荐

用户在大量待解压文件中，经常需要先"看看里面有什么"再决定是否解压、解压哪些。当前 Worker 已具备 `Inspect` 操作（列出归档目录树），但 UI 层未暴露此功能。

### 涉及层次与改动范围

| 层 | 改动 |
|---|---|
| **Rust Protocol** | `desktop.rs` 新增 `ArchiveEntry` DTO（路径、大小、是否加密、修改时间） |
| **Rust Core** | 新增 `inspect` Tauri 命令，调用 Worker 的 Inspect 操作 |
| **Rust Desktop Host** | 注册 `inspect_archive` 命令 |
| **TypeScript UI** | 任务行增加"预览"按钮/抽屉，展示可搜索的虚拟化目录树（`antd Tree`） |
| **C# Worker** | 已有 `InspectAsync` — 仅需确认 DTO 对齐 |

### 关键设计决策

- 加密归档在未输入密码前能否列出文件名？（7z 加密头不行，ZIP 可以）
- 是否支持选择性解压（勾选部分文件仅解压所选）？— 这会显著增加复杂度，建议作为后续迭代
- 预览结果是否缓存？

---

## 建议四：编码智能识别与乱码自愈（Encoding Auto-Detection）

**推荐指数：⭐⭐⭐⭐**

### 为什么推荐

这是 `functional-design.md` 中 F4 功能域的核心设计，解决中文用户解压老旧 ZIP 包时出现乱码的高频痛点。功能设计文档已有详细的探测矩阵与评分算法规格。

### 涉及层次与改动范围

| 层 | 改动 |
|---|---|
| **C# Engine** | `SevenZipEngine` 读取 ZIP Local File Header 的原始字节 → 多编码候选评分 → 选用最高分编码重建文件名 |
| **Rust Protocol** | `WorkerEvent` 扩展 `detected_encoding: Option<String>` 字段 |
| **Rust Core** | 记录检测到的编码到任务元数据，支持"切换编码重新写回" |
| **TypeScript UI** | 任务详情中显示检测到的编码，提供手动切换编码的下拉选项 |

### 关键设计决策

- 支持的编码范围：GBK / GB18030 / Shift-JIS / Big5 / EUC-KR？
- 切换编码后是否需要重新解压还是仅重命名已解压文件？
- 置信度低于阈值时是否提示用户手动选择？

---

## 建议五：密码候选规则派生引擎（Candidate Derivation Rules）

**推荐指数：⭐⭐⭐**

### 为什么推荐

`functional-design.md` F2.2 已详细设计了候选流水线的分层优先级与规则派生机制。当前实现仅做库内原文匹配，未实现去空格变体、大小写变体、文件名嵌入密码提取等派生规则。

### 涉及层次与改动范围

| 层 | 改动 |
|---|---|
| **Rust Domain** | `crates/domain/src/passwords/candidate_policy.rs` 扩展派生规则引擎 |
| **Rust Core** | `coordinator.rs` 中的候选构建阶段接入派生管线 |
| **Rust Protocol** | `CandidateItem` 扩展 `origin: String` 字段（Original / Trimmed / CaseVariant / FromFileName） |
| **TypeScript UI** | 设置页"密码匹配策略"区域增加规则开关 |
| **C# Worker** | `WorkerEvent` 返回命中的 `candidate_id` + `origin`，无需修改解压逻辑 |

---

## 建议六：物理磁盘拓扑感知调度（Device Topology Provider）

**推荐指数：⭐⭐⭐**

### 为什么推荐

`functional-design.md` F5.1 已有完整设计。当前并发调度（`concurrency: 1~4`）未区分物理磁盘，可能导致同一块机械硬盘上多个任务并发读写引发磁头寻道风暴。

### 涉及层次与改动范围

| 层 | 改动 |
|---|---|
| **Rust Infrastructure** | 新增 `system/topology.rs`，调用 Win32 `DeviceIoControl(IOCTL_STORAGE_GET_DEVICE_NUMBER)` 探测物理盘映射与介质类型 |
| **Rust Core** | `coordinator.rs` 的调度信号量从全局改为按物理盘隔离 |
| **TypeScript UI** | 设置页展示探测到的物理磁盘拓扑（可选） |

---

## 优先级排序与依赖关系总结

```text
第一波（高价值 × 低风险，建议优先实施）：
  ├── 建议一：监听文件夹 ── 独立实施，无跨层协议变更
  └── 建议二：右键菜单集成 ── 独立实施，需 Windows 注册表交互

第二波（中等复杂度，显著提升体验）：
  ├── 建议三：内容预览 ── 需 3-Way 协议扩展 + UI 新组件
  └── 建议四：编码自愈 ── 主要在 C# Engine 层，需协议小幅扩展

第三波（深度增强，建议有余力时推进）：
  ├── 建议五：密码派生引擎 ── 主要在 Rust Domain 层
  └── 建议六：磁盘拓扑调度 ── 主要在 Rust Infrastructure 层
```

---

## 验证计划

每项新功能均需通过：
- 协议同步检查：`node scripts/check-contracts.mjs`
- Rust 单元与集成测试：`cargo test --workspace`
- 前端测试与构建：`npm test && npm run build`（`apps/desktop-ui`）
- .NET 引擎测试：`dotnet run --project tests/OpenUnpack.Tests`
- 跨边界 QA 审计：qa-inspector 全量验收
