# 隔离文件作业进程

本模块为 Rust 核心提供独立进程中的 7-Zip 原生适配和文件传输；它不运行来源软件、不执行归档内文件、不查询网络，也不删除源包。

开发构建：

```powershell
dotnet build src/OpenUnpack.Worker/OpenUnpack.Worker.csproj -c Release
```

输出 `src/OpenUnpack.Worker/bin/Release/net10.0-windows/OpenUnpack.Worker.exe`。开发环境需要 .NET 10；发行时应使用 `dotnet publish -c Release -r win-x64 --self-contained true` 并将完整输出随应用打包。

启动只接受 `--pipe-prefix`、`--parent-pid`、`--engine`。两个字节模式的异步当前用户命名管道分别为 `<prefix>-control` 和 `<prefix>-events`，同时核对连接客户端 PID。协议由 `Protocol.cs` 实现，与 `crates/core/src/types.rs` 对齐；版本 1，UTF-8 换行 JSON，最大 8 MiB。

- `extract` 读取目录并预检全部路径，再解压一次。写入时计算 SHA-256；原生引擎的 CRC/数据错误不能成功。分卷只能从明确批准列表读取。
- `copy` 在目标临时树使用 `CreateNew` 写入，不覆盖。写入时计算内容摘要，并与非空期望摘要比较。摘要空值用于输入缓存首次复制，返回新摘要。
- `readback` 对目标临时树逐文件读回；要求 sourcePath 与 relativePath 的映射一致并且摘要存在。返回范围 `targetReadback`，不承诺绕过系统缓存或物理掉电持久性。
- `cancel` 和 `ping` 由独立控制接收器处理，进度合并到单事件写入者，不持状态锁等文件 I/O。

只有一项活跃作业。所有失败消息使用固定脱敏文本，不输出请求、口令或异常原文。取消依赖原生回调，不能保证有缺陷的原生库及时响应；Rust `WorkerClient` 为每项请求创建 Windows JobObject，超时终止并确认进程退出，确认前不能清理工作目录或再次发布结果。

安全边界包括归档中的越界、设备名、大小写冲突、链接、备用数据流，来源/目标文件系统 reparse 路径，数量、实际字节和空间预算。常规路径检查不能作为抵抗具有当前 Windows 用户权限的恶意进程的操作系统沙箱；发布目录的所有权和权限仍由主程序管理。

实际集成验证：

```powershell
./src/OpenUnpack.Worker/verify-worker.ps1
```

脚本仅生成项目 `artifacts/worker-verification-*` 下的人工测试包，包含明文 ZIP、加密头 7z、数字分卷、ZIP 符号链接、越界条目、复制和目标篡改，验证进度、独立控制取消、密码使用判定及固定错误。结果保存在同目录 `results.json`；大文件取消夹具在验证完成后删除。没有测试真实用户密码、原软件数据库、实物断电或所有第三方格式样本。
