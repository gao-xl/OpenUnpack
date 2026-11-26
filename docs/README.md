# OpenUnpack 文档

当前产品面向 Windows x64。功能设计和测试方案描述目标与验收要求，不能作为当前版本全部通过的证明。

| 主题 | 文档 |
|---|---|
| 安装、使用与构建 | [项目说明](../README.md) |
| 贡献与验证 | [贡献指南](../CONTRIBUTING.md) |
| 安全边界 | [安全策略](../SECURITY.md) |
| 生产链与源码归属 | [现行架构](architecture-current.md) |
| 模块与文件总览 | [详细技术文档（文件与模块索引）](technical-documentation.md) |
| 本机 API | [调用与授权指南](local-api.md) |
| 产品需求 | [软件规格](software-spec.md)、[功能设计](functional-design.md) |
| 验收准则 | [版本验收清单](acceptance-checklist.md)、[完整功能测试规范](plans/2026-10-19-full-functional-testing.md) |
| 专项验证方法 | [目录身份与竞态](s06-directory-identity-plan-2026-10-14.md)、[磁盘负载](load-optimization-plan-2026-10-15.md) |
| 优化问题 | [优化问题清单](optimization-issues-2026-11-17.md)、[性能优化方案](performance-optimization-plan-2026-11-17.md) |
| 阶段总结与报告汇总 | [阶段总结与历史报告汇总（2026-11-25）](reports/phase-summary-2026-11-25.md) |
| 技术决策 | [未决事项](decisions/OPEN-DECISIONS.md) |
| 已发布版本 | [v0.1.0-beta.1](release-notes-0.1.0-beta.1.md)、[变更记录](../CHANGELOG.md) |

公开文档列表由 `scripts/source-release-policy.json` 管理。新增文档时同步维护该列表与 Git 忽略规则。内部会话笔记、历史执行现场、反编译材料及未维护的部署原型不属于公开源码。
