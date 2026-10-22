## Harness: OpenUnpack

**Goal:** Coordinate cross-stack development (Rust / TypeScript / C#) for the OpenUnpack batch archive extraction desktop app.

**Triggers:**
- Feature development, bug fixes, refactoring → `/dev <task>`
- Code review, protocol sync audit, architecture check → `/review <scope>`
- Simple questions about the codebase can be answered directly without orchestration.

**Team:**
| Agent | Role | Model |
|-------|------|-------|
| scout | Read-only codebase reconnaissance | haiku |
| rust-dev | Rust backend (domain → Tauri host) | sonnet |
| ui-dev | React/TS frontend (Ant Design) | sonnet |
| worker-dev | C#/.NET worker + 7-Zip engine | sonnet |
| qa-inspector | Cross-boundary verification | sonnet |

**Change Log:**
| Date | Change | Target | Reason |
|------|--------|--------|--------|
| 2026-10-10 | Initial harness build | All | New harness for 5-agent team covering Rust/TS/C# cross-stack dev |
