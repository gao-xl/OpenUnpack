# 快照增量协议实施方案（审计 P1）

- 日期：2026-10-12
- 状态：**设计完成，待实施**（审计路线图档位：常规迭代）
- 关联：`docs/audit-report-2026-10-12.md` §B-P1 · 台账 `status-ledger-2026-10-11.md` §1#7（分页与增量未开始）

## 1. 问题与目标

现状：`state-changed` 事件 → 前端 150ms 防抖 → 全量 `snapshot()`。快照 = jobs（含 nodes/transferProgress 全量）+ passwords + settings + 标量。万条队列时每次变更的代价是 O(jobs×nodes) 的克隆、序列化与 IPC 传输；进度事件高峰期约 6 次/秒。

目标：在**不改变最终一致语义**的前提下，把稳态刷新从「全量传输」降为「差异传输」。

## 2. 现状锚点（代码事实）

| 机制 | 位置 | 语义 |
|---|---|---|
| revision | `crates/core/src/coordinator.rs:350` | `view_state` 锁内 `fetch_add(1)`；「一次可见变更恰 +1」有测试（:3875） |
| 广播 | `outputs`（`changed_locked`） | `broadcast::Sender<()>` → desktop-host 转 `state-changed {sessionId, revision}` |
| 防抖拉取 | `apps/desktop-ui/src/api.ts:55-65` | 150ms 防抖 + `wanted`/`staleReads` 门控 |
| 单调去重 | `apps/desktop-ui/src/snapshot-cursor.ts` | `sessionId` + `revision` 严格递增校验 |
| 协议测试 | `crates/protocol/tests/snapshot_revision.rs` | u64 ↔ decimal string wire 约束 |

## 3. 协议草案（三端契约）

### 3.1 设计决策：状态 diff，而非事件日志

服务端保存「上次交付的快照投影」1 份。`since` 与缓存 revision 相等时返回**新旧状态差异**；否则回退全量。

- 差异 = 状态比较（内存内），**不依赖事件不丢失**——apply(delta, base) 恒等于直接全量结果；
- 无需 delta 日志窗口 / backlog 复杂度；内存开销 = 1 份快照投影；
- 高频变更天然合并（防抖后一次 diff 吸收期间全部变化）。

### 3.2 新 DTO（`crates/protocol/src/desktop.rs`）

```rust
#[serde(rename_all = "camelCase")]
pub struct SnapshotUpdate {
    pub session_id: String,
    pub revision: u64,                 // decimal_revision wire（沿用现有机制）
    pub full: Option<AppSnapshot>,     // 首连 / since 失配 / 会话切换
    pub delta: Option<SnapshotDelta>,  // 命中缓存时
}

#[serde(rename_all = "camelCase")]
pub struct SnapshotDelta {
    pub jobs_changed: Vec<JobView>,        // 新增或内容变化的 job
    pub jobs_removed: Vec<String>,
    pub passwords_changed: Vec<PasswordView>,
    pub passwords_removed: Vec<String>,
    pub settings: Option<Settings>,        // 仅变更时下发
    pub state: Option<SnapshotState>,      // 标量字段，仅变更时下发
}

#[serde(rename_all = "camelCase")]
pub struct SnapshotState {
    pub running: bool,
    pub paused: bool,
    pub vault_error: Option<String>,
    pub data_directory: String,
    pub engine_available: bool,
    pub recoverable_count: u32,
    pub cache_used_bytes: u64,
}
```

约束（契约检查器兼容性）：仅使用已建模语义——camelCase、`Option<T>`↔`T | null`、`decimal_revision`；**不引入** tagged enum / untagged / 新大小写规则。`full`/`delta` 二选一由运行时不变量保证（`debug_assert` + 测试），字段本身走 `Option` 精确比对。

TS 侧（`types.ts`）新增同名 interface（`SnapshotUpdate` / `SnapshotDelta` / `SnapshotState`），值域与可选性与 Rust 一一对应；契约检查自动覆盖（`scripts/check-contracts.mjs` 已支持）。

### 3.3 命令面

- 新增：`snapshot_update(sinceRevision: Option<String>) -> SnapshotUpdate`
  - `since = null` → full（首次连接）；
  - `since = Some(r)`：`r == cache.revision` 且 `sessionId` 一致 → delta；否则 full。
- 保留：`snapshot()`（全量兜底；不再更新 delta 缓存，避免多消费者互相挤占）。

## 4. Rust 侧实现要点

- `Coordinator` 新增字段：`snapshot_delta_cache: Mutex<Option<(u64, String, SnapshotProjection)>>`
  （revision、sessionId、上一次交付的快照投影）。
- `snapshot_update(&self, since: Option<u64>)`：
  1. `snapshot()` 构建当前快照（沿用 `view_state` 锁序）；
  2. 命中判定（revision/session 相等）→ 与缓存投影逐字段 diff：
     jobs/passwords 按 `id` 匹配 + `PartialEq`（`JobView` 已 derive）→ `changed`/`removed`；
     settings/state 整体 `PartialEq` → `Option`；
  3. 用当前快照替换缓存；返回 `SnapshotUpdate`。
- 锁顺序：`view_state` → 字段锁 → `delta_cache`（叶子锁），无嵌套新增。
- 兼容性：`DESKTOP_API_VERSION` 不变（新增命令为附加面；旧全量路径保留）。

## 5. 前端接入要点

- `SnapshotCursor` 扩展为「快照合成器」：
  - 维护 `last: AppSnapshot`；
  - `full` → 替换；`delta` → 以 `id` 合并 jobs/passwords（changed 覆盖、removed 删除）+ settings/state 补丁；
  - 合成结果经既有 `accept()` 单调校验后回调。
- `api.subscribe` 的 `update()` 调用 `snapshot_update(cursor.revision)`；连续异常/失配自动退回 `snapshot()`（全量路径本身即回退）。
- preview 模式（非桌面）维持现状。

## 6. 测试计划

| 层 | 用例 |
|---|---|
| Rust 单元 | `apply(delta, base) == full`；removed 检测；settings/state Option 语义；session 切换 → full；since 失配 → full；缓存不因 `snapshot()` 调用被挤占 |
| 协议契约 | 新增 DTO 自动纳入三端比对（desktop↔TS）；`decimal_revision` mutation 覆盖 |
| UI vitest | 合成器：新增/更新/删除/会话替换/异常回退 |
| 基准 | 10k jobs × 平均 3 nodes：全量 vs delta 的序列化字节数与耗时（验收数据入 `artifacts/`） |

## 7. 分阶段实施（可独立提交）

1. **P1-a 协议 + Rust**：DTO + `snapshot_update` + 缓存 diff + Rust 测试 + 契约检查。
2. **P1-b UI 接入**：cursor 合成器 + subscribe 切换 + vitest。
3. **P1-c 验收**：10k 夹具基准 + 原生套件回归 + 台账回写。

回退策略：任阶段异常时前端一键切回 `snapshot()` 全量路径（命令保留，无破坏性迁移）。

## 8. 为什么不在本轮修复轮实施

审计路线图将 P1 列为「常规迭代」；其实施触及核心数据流（快照热路径）与三端契约，需要独立回归预算（原生套件 + 万条基准）。本方案已将其从「既定方向」收敛到「可逐阶段执行的设计」，随下一迭代启动。
