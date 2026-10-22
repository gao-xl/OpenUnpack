# 未决事项登记册（OPEN-DECISIONS）

> 规则：只追加 + 就地关闭（OPEN → RESOLVED），关闭时补 Resolution 列。每条都应能被一次检查或一次发布动作关闭。

| ID | Date | Source | Open Item | Current Leaning | Blocked By | Resolves When | Status |
|----|------|--------|-----------|-----------------|------------|---------------|--------|
| OD-001 | 2026-10-21 | QUAL-002 | 用自研 `scripts/check-a11y.mjs` 过渡替代 `eslint-plugin-jsx-a11y` | 过渡方案保留到插件支持 ESLint 10 | `eslint-plugin-jsx-a11y` 的 peerDependencies 仅到 `eslint@^9`（本项目 ESLint 10.12） | 发布版本声明 `eslint@^10` 后，删除脚本与 `lint:a11y`，CI 合并回 `npm run lint` | OPEN |
| OD-002 | 2026-10-21 | P2-09 | 7-Zip 口令能否不经命令行传递 | 26.04 实测不支持 stdin，默认回退 argv；`OPENUNPACK_7Z_STDIN=1` 保留为可选通道 | 上游 7-Zip 若支持从 stdin/文件读口令 | 上游支持后，把 stdin 设为默认并删除 argv 回退 | OPEN |
| OD-003 | 2026-10-21 | P2-12 | 既有便携库用户的启用引导 | 当前在后端 fail-closed + UI 文案提示 | 需要产品决定是否加"一键迁移到 DPAPI" | 产品确认迁移策略后实现 | OPEN |

## 关闭记录

（暂无）
