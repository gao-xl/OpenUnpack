# OpenUnpack OpenAPI & 开发者生态接入指南

OpenUnpack 从桌面单一工具全面演进为**跨平台无头解压守护进程 (Daemon) 与微服务生态**。任何第三方客户端、自动化下载器、浏览器插件、NAS 操作系统及企业工作流系统均可通过统一的 OpenAPI 与 WebSocket 双向协议进行无缝接入。

---

## 1. 架构总览

```
  ┌────────────────────────────────────────────────────────┐
  │                   接入生态层 (Clients)                 │
  │  React WebUI  │  openunpack-cli  │ 浏览器插件 │ 第三方服务 │
  └───────┬───────────────┬─────────────────┬──────────────┬─┘
          │ (HTTP/WS)     │ (IPC/CLI)       │ (REST)       │ (RPC)
  ┌───────▼───────────────▼─────────────────▼──────────────▼─┐
  │               OpenUnpack Daemon (Axum Local API)         │
  │  - 静态 WebUI 路由与安全沙箱 (/ & /assets/*)               │
  │  - OpenAPI 3.0 / Swagger UI (/docs/ & /openapi.json)     │
  │  - WebSocket 实时流与决策挂起 (/api/v1/ws)               │
  │  - 跨平台便携密码库 (AES-256-GCM Vault)                   │
  └───────────────────────────┬──────────────────────────────┘
                              │
  ┌───────────────────────────▼──────────────────────────────┐
  │                 解压调度执行层 (Execution)               │
  │   - 纯 Rust 原生解压引擎 (内置流式解压、Zip Slip 防御)    │
  │   - 外部 7-Zip CLI 桥接引擎 (7z / 7za / p7zip)           │
  │   - 目录监视引擎 (Watch Directory Engine)                │
  └──────────────────────────────────────────────────────────┘
```

---

## 2. 快速开始与在线文档

守护进程启动后，即可通过浏览器直接访问内置接口：
* **Web 控制台首页**：`http://127.0.0.1:18420/`（自动挂载 React 独立静态 WebUI，若无构建产物则自动呈现轻量 Web 探查控制台）
* **Swagger UI 在线调试**：`http://127.0.0.1:18420/docs/`
* **OpenAPI 3.0 规范定义**：`http://127.0.0.1:18420/openapi.json`
* **服务存活探测**：`GET http://127.0.0.1:18420/api/v1/health`

---

## 3. 认证授权机制

为了防御浏览器恶意页面跨站请求伪造（CSRF）与 DNS 重绑定攻击，OpenUnpack 采用基于 Token 的请求认证与严格的 Host 校验：

1. **获取 API 密钥**：
   - 守护进程首次无头启动时会自动生成 `default-cli` 令牌并输出在标准日志中；
   - 也可以通过 `openunpack-cli` 或桌面端界面创建指定作用域（`read_jobs`, `write_jobs`, `vault_read`, `vault_write` 等）的独立 Token。
2. **请求头注入**：
   在所有 HTTP 请求中附带标准 Bearer 请求头：
   ```http
   Authorization: Bearer <YOUR_API_TOKEN>
   ```
3. **跨域支持 (CORS)**：
   Daemon 现已原生支持全量 CORS 预检请求（`OPTIONS`）及跨域访问头（`Access-Control-Allow-Origin: *`），方便浏览器扩展和第三方前端面板直接调用。

---

## 4. 核心 REST API

所有业务动作统一由 `/api/v1/commands` 接收调度。

### 4.1 提交解压作业 (`AddArchive`)
```http
POST /api/v1/commands
Content-Type: application/json
Authorization: Bearer <TOKEN>

{
  "command": "AddArchive",
  "args": {
    "path": "/downloads/sample.zip",
    "outputDir": "/extracted/sample",
    "password": "optional-secret",
    "deleteSource": false,
    "autoStart": true
  }
}
```

### 4.2 获取队列快照 (`Snapshot`)
```http
POST /api/v1/commands
Content-Type: application/json
Authorization: Bearer <TOKEN>

{
  "command": "Snapshot"
}
```

### 4.3 队列生命周期控制
* **启动解压**：`{ "command": "StartJobs" }`
* **暂停解压**：`{ "command": "PauseJobs" }`
* **取消指定作业**：`{ "command": "CancelJobs", "args": { "jobId": "job-uuid" } }`
* **密码重试**：`{ "command": "RetryJob", "args": { "jobId": "job-uuid", "password": "new-password" } }`

---

## 5. WebSocket 实时事件与交互式决策 (`/api/v1/ws`)

### 5.1 建立连接
```javascript
const ws = new WebSocket('ws://127.0.0.1:18420/api/v1/ws?token=' + encodeURIComponent(token));
```

### 5.2 接收实时进度事件 (`job_progress`)
```json
{
  "event": "job_progress",
  "data": {
    "jobId": "4a7138b2-...",
    "bytesExtracted": 10485760,
    "totalBytes": 52428800,
    "currentFile": "videos/part1.mp4",
    "bytesPerSecond": 5242880,
    "remainingSeconds": 8
  }
}
```

### 5.3 响应缺密挂起决策 (`need_password`)
当遇到未知密码的加密压缩包时，Daemon 会保持作业为挂起等待状态，并通过 WebSocket 广播 `need_password` 事件：
```json
{
  "event": "need_password",
  "data": {
    "jobId": "4a7138b2-...",
    "archivePath": "/downloads/secret.rar"
  }
}
```
客户端或用户输入口令后，通过 `SubmitInput` 提交决策，作业自动唤醒并继续解压：
```http
POST /api/v1/commands
Content-Type: application/json
Authorization: Bearer <TOKEN>

{
  "command": "SubmitInput",
  "args": {
    "jobId": "4a7138b2-...",
    "inputType": "password",
    "value": "MySecretPassword123"
  }
}
```

---

## 6. 生态集成套件

* **浏览器插件原型**：参见 `examples/browser-extension/`（支持 Chrome / Edge / Firefox 监听下载完成自动转交解压与缺密通知）。
* **Python 异步客户端**：参见 `examples/third-party-client/python_async_client.py`（基于 aiohttp 实现事件驱动作业解压与交互决策）。
* **BT/下载器集成脚本**：参见 `scripts/integrations/`（已开箱即用支持 qBittorrent、Aria2、Transmission）。
* **Docker 部署模板**：参见根目录 `Dockerfile` 与 `docker-compose.yml`。
