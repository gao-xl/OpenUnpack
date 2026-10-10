# 贡献指南

先阅读 README 与 `docs/acceptance-checklist.md`。历史设计文档标有草案状态，不能用它们替代当前代码、测试和验收证据。提交说明应列出行为变化、人工样本测试结果和尚未验证的范围。

## 本地验证

Windows x64、64 位 PowerShell 7、Node 24、Rust MSVC 工具链、C++ Build Tools / Windows SDK、`global.json` 对应 .NET SDK。保留 `Cargo.lock` 和 UI 的 `package-lock.json`；依赖升级同时更新许可证、漏洞检查和原生回归。

```powershell
./scripts/prepare-desktop.ps1
cargo fetch --locked --target x86_64-pc-windows-msvc
./scripts/collect-release-materials.ps1
./scripts/verify.ps1 -SkipInstall -WorkerProtocol
```

测试只能使用人工归档、人工密码和临时目录。不要读取原软件数据库或个人密码库，也不要把真实口令、归档内容、用户目录、网关密钥或账户配置放进 issue、源码或输出日志。`OPENUNPACK_DATA_DIR` 可指定隔离数据目录；它只改变数据位置，不取消 Windows 账户绑定。

## 模块约定

- Rust 核心决定任务完成与发布，UI 不能自行宣布成功；Worker 不拥有 SQLite 或整个密码库。
- 共享类型修改时同步 Rust / TypeScript / Worker DTO，并验证命名管道协议版本与消息大小边界。
- 不持状态锁跨 `await`。取消、进程监督和进度传输保持独立；清理须确认 Worker 实际退出。
- 不覆盖旧成品，不凭目录存在推断发布成功；已发布文件不能因后处理失败被回滚删除。
- 密码原文、备份口令和底层异常原文不得进入日志；普通/严格验证的 UI 描述要与真实范围一致。
- 默认保留源包。新增删除、后台联网或运行外部文件功能须有明确产品需求与独立验收。

## 提交与发行

改动保持单一目的，附通过的命令与样本范围；未验证的 RAR 变体、真实拖放、DPI、设备故障和性能注明 pending。新编写的代码按项目 MIT 许可贡献；引用第三方代码需保留版权和对应许可证，不复制来源软件界面资产、私有数据库或 DLL。

`build-desktop.ps1` 仅产出本地可发布材料，不自动创建 GitHub 仓库或外部 release。公开发布前重新核对验收清单、源码白名单、第三方源码和告知、签名策略及干净机器安装。当前没有公开维护邮箱；安全问题按 SECURITY.md 的方式联系现有维护者。
