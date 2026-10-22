# OpenUnpack 下载器自动化集成指南 (Integrations Guide)

本目录包含主流 BT / HTTP 下载客户端与 OpenUnpack 自动化联动的标准脚本。当下载客户端完成下载后，自动触发解压，实现 **“下载即解压、无感自动化”**。

---

## 目录结构

| 脚本文件 | 适配下载客户端 | 操作系统 |
| :--- | :--- | :--- |
| `qbittorrent.bat` | qBittorrent | Windows |
| `qbittorrent.sh` | qBittorrent | Linux / macOS |
| `aria2-on-complete.bat` | aria2 | Windows |
| `aria2-on-complete.sh` | aria2 | Linux / macOS |
| `transmission-done.sh` | Transmission | Linux / macOS |

---

## 1. qBittorrent 配置

### Windows
1. 打开 qBittorrent -> **工具 (Tools)** -> **选项 (Options)** -> **下载 (Downloads)**。
2. 勾选 **“Torrent 完成时运行外部程序” (Run external program on torrent finished)**。
3. 填入如下命令（请将路径替换为脚本实际所在绝对路径）：
   ```bat
   "C:\path\to\scripts\integrations\qbittorrent.bat" "%F" "%D" "%N" "%L"
   ```

### Linux / macOS
1. 赋予脚本执行权限：
   ```bash
   chmod +x /path/to/scripts/integrations/qbittorrent.sh
   ```
2. 在 qBittorrent 下载选项中填入：
   ```bash
   /path/to/scripts/integrations/qbittorrent.sh "%F" "%D" "%N" "%L"
   ```

---

## 2. aria2 配置

在 `aria2.conf` 配置文件中添加如下选项：

### Windows
```ini
on-download-complete=C:\path\to\scripts\integrations\aria2-on-complete.bat
```

### Linux / macOS
```bash
chmod +x /path/to/scripts/integrations/aria2-on-complete.sh
```
在 `aria2.conf` 中填入：
```ini
on-download-complete=/path/to/scripts/integrations/aria2-on-complete.sh
```

---

## 3. Transmission 配置

在 Transmission 守护进程配置文件 `settings.json` 中配置：
```json
{
  "script-torrent-done-enabled": true,
  "script-torrent-done-filename": "/path/to/scripts/integrations/transmission-done.sh"
}
```
保存后重载 Transmission 服务即可。

---

## 工作原理与容灾回退

1. **优先 CLI 调度**：若已安装 `openunpack` 命令行工具并在 `PATH` 中（或默认安装目录），脚本直接调用 `openunpack unpack`，享有完整的参数校验与多卷识别。
2. **REST API 回退**：若未安装 CLI，脚本自动读取 `%LOCALAPPDATA%/OpenUnpack/daemon.info` 或 `$XDG_DATA_HOME/openunpack/daemon.info` 中的端口与授权 Token，通过 `curl` 直接与 OpenUnpack 本地守护进程通信提交解压。
