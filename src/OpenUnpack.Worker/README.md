# 隔离文件作业进程

本模块为 Rust 核心提供独立进程中的 7-Zip 原生适配和文件传输；它不运行来源软件、不执行归档内文件、不查询网络，也不删除源包。

开发构建：

```powershell
dotnet build src/OpenUnpack.Worker/OpenUnpack.Worker.csproj -c Release
```

输出 `src/OpenUnpack.Worker/bin/Release/net10.0-windows/OpenUnpack.Worker.exe`。开发环境需要 .NET 10；发行时应使用 `dotnet publish -c Release -r win-x64 --self-contained true` 并将完整输出随应用打包。

启动只接受 `--pipe-prefix`、`--parent-pid`、`--engine`。两个字节模式的异步当前用户命名管道分别为 `<prefix>-control` 和 `<prefix>-events`，同时核对连接客户端 PID。协议由 `Protocol.cs` 实现，与 `crates/protocol/src/worker.rs` 对齐；支持版本 1 / 2，UTF-8 换行 JSON，最大 8 MiB。ready 的 Hello 广告版本 2，同时保留旧版能力字段。backends 分别声明 DLL 实际版本的 7-Zip 和托管 LZ4 后端。

- `extract` 读取目录并预检全部路径，再解压一次。写入时计算 SHA-256；原生引擎的 CRC/数据错误不能成功。分卷只能从明确批准列表读取。
- `copy` 在目标临时树使用 `CreateNew` 写入，不覆盖。写入时计算内容摘要，并与非空期望摘要比较。摘要空值用于输入缓存首次复制，返回新摘要。
- `readback` 对目标临时树逐文件读回；要求 sourcePath 与 relativePath 的映射一致并且摘要存在。返回范围 `targetReadback`，不承诺绕过系统缓存或物理掉电持久性。
- `inspect` 只读取目录/帧头。没有展开量的条目以 `size=0,sizeKnown=false` 返回，`totalBytesKnown=false`；缺省两个 known 字段均为 true 以兼容旧清单。未知大小只用于 `metadataOnly`，复制与回读请求拒绝 `sizeKnown=false`。
- `probe` 根据可靠的未加密目录属性直接返回 `NoPasswordNeeded`，未知属性返回 `Failed` 供常规提取回退；结果不代表内容完整性。加密候选写入有限 discard stream 并验证原生结果，最终提取仍完整校验。
- `cancel` 和 `ping` 由独立控制接收器处理，进度合并到单事件写入者，不持状态锁等文件 I/O。

只有一项活跃作业。所有失败消息使用固定脱敏文本，不输出请求、口令或异常原文。取消依赖原生回调，不能保证有缺陷的原生库及时响应；Rust `WorkerClient` 为每项请求创建 Windows JobObject，超时终止并确认进程退出，确认前不能清理工作目录或再次发布结果。

安全边界包括归档中的越界、设备名、大小写冲突、链接、备用数据流，来源/目标文件系统 reparse 路径，数量、实际字节和空间预算。常规路径检查不能作为抵抗具有当前 Windows 用户权限的恶意进程的操作系统沙箱；发布目录的所有权和权限仍由主程序管理。

托管 LZ4 支持独立块、无预设字典的单帧，包括块/内容校验和；拼接帧、可跳过帧和尾随数据返回 `Unsupported`。提取范围为 `frameValidationAndStreamSha256`，不把内容未携带的校验和描述为归档 CRC。Zone.Identifier 读取上限 16 KiB；受控来源标记无法传播时操作失败，不返回可发布的 completed 清单，源包保持原状。失败暂存内容可能需要主程序在确认 Worker 退出后清理。

stderr 只输出有限 JSON 诊断：`kind=openunpackDiagnostic,version=1`，字段为 `operation,phase,errorCode,exceptionType,hResult,nativeCode`，没有路径、请求 ID、密码、原始消息或堆栈。每进程最多 8 行，每行最多 512 UTF-8 字节。主程序应按 schema/长度白名单解析并忽略其他内容。operation 为 `startup/extract/inspect/probe/copy/readback/unknown`；phase 为 `request/metadata/decode/transfer/sourceZone/targetZone/mitigation/cleanup/unknown`。安全策略在系统支持时必须设置并回读成功；旧系统未提供的策略以 `HardeningUnavailable` 诊断标明，真实设置/回读失败以 `HardeningFailed` 拒绝启动。

实际集成验证：

```powershell
./src/OpenUnpack.Worker/verify-worker.ps1
```

脚本仅生成项目 `artifacts/worker-verification-*` 下的人工测试包，包含明文 ZIP、加密头 7z、数字分卷、ZIP 符号链接、越界条目、复制和目标篡改，验证进度、独立控制取消、密码使用判定及固定错误。结果保存在同目录 `results.json`；大文件取消夹具在验证完成后删除。没有测试真实用户密码、原软件数据库、实物断电或所有第三方格式样本。
