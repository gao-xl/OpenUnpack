# 三语言界面实现记录

## 范围与行为

支持 `zh-CN`（简体中文）、`en-US`（English）、`ja-JP`（日本語），首次使用默认中文。入口为“设置与备份 → 界面与备份 → 界面语言”，选择即时生效，无须保存其他设置，也不会重置未保存的设置草稿。

语言选择以 `openunpack.language` 保存在当前 WebView 的 localStorage，启动时恢复；无效值回退中文，存储不可用时仍允许当前会话切换。文档 `lang`、Ant Design 控件、各功能页、弹窗、状态/阶段标签、前端提示以及托盘/退出提示同步切换。多个窗口通过 storage 事件同步。

文件名、路径、密码、分组、备注与原始后端诊断不翻译。Windows 文件选择器及原生确认按钮沿用系统语言。语言偏好不随密码库备份导出，未增加 Worker 协议或后端业务设置字段。

## 实现位置

- `apps/desktop-ui/src/i18n.ts`：语言读取、持久化、订阅、翻译及插值；中文原文作为稳定查找键，缺失时回退原文。
- `apps/desktop-ui/src/locales/{common,pages,settings,api}.ts`：统一管理英语、日语译文。
- `App.tsx`、`api.ts` 及所有界面组件：渲染时翻译；含翻译的树缓存依赖当前语言。
- `apps/desktop-host/src/lib.rs`：主窗口限定的 `set_ui_language` 命令，更新托盘并保存会话原生界面语言。前端串行发送切换，避免快速切换产生先后顺序错误。
- `build.rs`、`permissions/main.toml`、生成的命令 ACL/schema：注册并授权该命令。
- `README.md`：新增语言入口、持久化方式及边界说明。

## 验证

在 `apps/desktop-ui`：

- `npm run build`：通过 TypeScript 与 Vite 生产构建。
- `npm run lint`：通过，无警告。
- `npm test -- --maxWorkers=2`：165 项中 161 通过，4 项因翻译提取时丢失中文空格失败；已修复，未修改原有断言。
- `npm test -- --maxWorkers=2 tests/i18n.test.tsx tests/i18n-ui.test.tsx tests/archive-browser.test.tsx tests/draft-lifecycle.test.tsx`：修复后 25/25 通过，包含新增整应用语言切换及草稿保留测试。连同上轮其余 16 个通过的测试文件，覆盖当前 166 项测试。
- 新增语言测试覆盖即时切换、偏好恢复/非法值、存储不可用、跨窗口同步、文件名插值保留、静态翻译键/英日插值完整性，以及原生语言更新顺序与失败恢复。

仓库根目录：`cargo check -p openunpack-desktop --offline` 通过；作用文件的 `git diff --check` 通过。Rust 构建自动生成了新命令权限。

沙箱中 Vitest 曾因临时模块文件 ENOENT 无法加载套件，完成验证的测试在沙箱外执行。以上为源码构建及自动化组件证据；本轮未重新打包 EXE/安装包，也未实测原生窗口、托盘、系统对话框或英日界面的不同 DPI 布局。发布前仍应对这些项目做人工验收与译文校对。
