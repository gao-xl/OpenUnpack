## 变更描述 (Description)

请简要描述本 PR 的背景、所解决的核心问题以及引入的实际行为变化。

关联 Issue：Fixes # <!-- 请在此填入关联的 Issue 编号，无关联则填 None -->

---

## 变更类型 (Type of Change)

- [ ] `feat`: 新增功能特性
- [ ] `fix`: 缺陷修复
- [ ] `refactor`: 代码重构（不改变现有外部功能与契约）
- [ ] `perf`: 性能优化
- [ ] `test`: 增加或修改回归测试夹具/测试用例
- [ ] `docs`: 文档变更或规范补充
- [ ] `build` / `ci`: 构建脚本、依赖版本或工作流调整

---

## 跨栈协议与架构一致性核查 (Cross-Stack & Architectural Checks)

如果本 PR 涉及进程间通信、数据模型或关键目录管理，请核实以下项目：

- [ ] **共享契约对齐**：若修改了 IPC 通信消息结构，已同步更新：
  - [ ] Rust 协议定义 (`crates/protocol/src/`)
  - [ ] C# Worker DTO (`src/OpenUnpack.Worker/`)
  - [ ] 前端类型定义 (`apps/desktop-ui/src/`)
- [ ] **单源实现与依赖方向**：底层行为只修改 Infrastructure 唯一实现；Core 兼容路径保持再导出；已运行 `node scripts/check-architecture.mjs --self-test`
- [ ] **单一状态真源恪守**：所有任务状态转换及文件发布决策均在 Rust 核心层完成，UI 与 Worker 未越权判定
- [ ] **异步锁与生命周期**：未持有 `MutexGuard` / `RwLockGuard` 跨越 `.await`；涉及子进程退出的逻辑已确认其完全终止

---

## 数据安全与脱敏检查 (Security & Sanitization Checklist)

- [ ] **无敏感凭据泄露**：未将任何真实密码明文、备份口令、私有路径硬编码至源码、测试夹具或日志输出中
- [ ] **原子化发布与源文件保护**：发布流程严格遵循校验后原子转移；默认绝不删除源文件；发布失败时未盲目回滚删除已发布的成品
- [ ] **路径与权限校验**：所有涉及的磁盘路径均经过规范化校验，阻断重解析点（Junction/Symlink）与路径穿越攻击

---

## 验证证据与测试覆盖 (Validation & Evidence)

请列出本 PR 所执行的本地测试命令、测试样本及实际运行结果：

```powershell
# 示例：执行的测试命令
./scripts/verify.ps1 -SkipInstall -WorkerProtocol
cargo test --workspace --locked
```

- **测试样本说明**：<!-- 说明使用的人工合成归档格式、分卷形式、嵌套情况及测试结果 -->
- **物理介质/平台**：<!-- 例如：Win 11 x64, NVMe SSD -> SATA HDD, 开启工作缓存 -->

---

## 未经验收的边界声明 (Remaining Acceptance Boundaries)

如果存在因硬件条件、格式变体或环境限制未能完全验证的场景，请务必客观如实列出（不掩盖已知局限）：

- [ ] 所有改动均已完成全量验证
- [ ] 存在部分场景待后续验证（请在下方说明具体范围，例如：特定 RAR 变体、极端并发或特定网络驱动器）
  - 说明：
