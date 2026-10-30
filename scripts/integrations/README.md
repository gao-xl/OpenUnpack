# 下载器完成回调

这些脚本把已完成下载的本地文件或目录交给独立 CLI：`openunpack unpack <path>`。脚本直接返回 CLI 退出码；缺少 CLI 时非零退出，不调用 HTTP 回退，也不读取或输出 API 密钥。提交成功只表示已提交任务，不代表后台解压已经完成。

当前生产 Worker 仅支持 Windows x64。先按主 README 配置 Windows 无头服务、API 密钥和目录授权，再在下载器运行账户中单独配置 CLI。桌面安装包的 `OpenUnpack.exe` 是 Host；不要将它当成独立 CLI。Windows 文件名不区分大小写，不能靠 `openunpack.exe` 的大小写区分二者。

可用 `scripts/build-headless.ps1` 构建本地 Windows 无头目录；其中 `openunpack-daemon.exe` 是 Host，`openunpack.exe` 是 CLI，必须保留同级 `worker/`。不应将该脚本描述为 Linux/macOS 服务构建入口。

通过下载器继承的 `OPENUNPACK_CLI` 指定独立 CLI 的完整路径，例如 `C:\Tools\OpenUnpackHeadless\openunpack.exe`；也可把实际 CLI 所在目录加入 `PATH`。CLI 与服务的连接和授权遵循主 README 与 CLI 配置，回调脚本不猜测安装目录或端口。

| 脚本 | 下载器 | 运行条件 |
|---|---|---|
| `qbittorrent.bat` | qBittorrent | Windows |
| `aria2-on-complete.bat` | aria2 | Windows |
| `qbittorrent.sh` | qBittorrent | Bash、可用 CLI、已配置路径映射 |
| `aria2-on-complete.sh` | aria2 | Bash、可用 CLI、已配置路径映射 |
| `transmission-done.sh` | Transmission | Bash、可用 CLI、已配置路径映射 |

Shell 脚本只是 CLI 调用示例，不提供 Linux/macOS 解压服务。只有下载器、CLI 与 Windows 服务能访问并正确解释同一目标路径时才使用；脚本不自动转换 WSL、容器或远程路径。

## qBittorrent

在“工具 → 选项 → 下载 → Torrent 完成时运行外部程序”填入实际脚本位置：

```bat
"C:\Tools\OpenUnpackHeadless\integrations\qbittorrent.bat" "%F" "%D" "%N" "%L"
```

具备上述 Bash/CLI/路径条件时，也可使用：

```bash
/path/to/integrations/qbittorrent.sh "%F" "%D" "%N" "%L"
```

## aria2

在 `aria2.conf` 中设置回调：

```ini
on-download-complete=C:\Tools\OpenUnpackHeadless\integrations\aria2-on-complete.bat
```

具备 Bash/CLI/路径条件时，把值换为 `aria2-on-complete.sh` 的绝对路径。aria2 提供 GID、文件数和第一个文件路径；多文件下载提交其父目录。

## Transmission

具备 Bash/CLI/路径条件时，配置实际脚本位置：

```json
{
  "script-torrent-done-enabled": true,
  "script-torrent-done-filename": "/path/to/integrations/transmission-done.sh"
}
```

使用 shell 脚本前赋予执行权限。首次接入请用合成归档检查目标路径和 CLI 退出码，并在 OpenUnpack 队列中核对最终任务状态。不要用唯一副本验证清理行为。
