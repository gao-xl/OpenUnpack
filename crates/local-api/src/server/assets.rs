//! 本机 API 嵌入式管理页与静态前端资源安全分发。

use super::*;

/// working directory is deliberately excluded so that an HTML file dropped in
/// the launch directory cannot inherit the API origin and reach the token.
pub(super) fn web_root_candidates(
    env_root: Option<&str>,
    exe_dir: Option<&std::path::Path>,
) -> Vec<std::path::PathBuf> {
    let mut candidates = Vec::new();
    if let Some(dir) = env_root {
        let trimmed = dir.trim();
        if !trimmed.is_empty() {
            candidates.push(std::path::PathBuf::from(trimmed));
        }
    }
    if let Some(parent) = exe_dir {
        candidates.push(parent.join("web"));
    }
    #[cfg(debug_assertions)]
    {
        let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        candidates.push(manifest_dir.join("../../apps/desktop-ui/dist"));
    }
    candidates
}

pub(super) fn is_safe_web_dir(path: &std::path::Path) -> Option<std::path::PathBuf> {
    let meta = std::fs::symlink_metadata(path).ok()?;
    if meta.file_type().is_symlink() || !meta.is_dir() {
        return None;
    }
    let canon = path.canonicalize().ok()?;
    canon.is_dir().then_some(canon)
}

pub(super) fn is_safe_web_file(
    root: &std::path::Path,
    file: &std::path::Path,
) -> Option<std::path::PathBuf> {
    let meta = std::fs::symlink_metadata(file).ok()?;
    if meta.file_type().is_symlink() || !meta.is_file() {
        return None;
    }
    let canon = file.canonicalize().ok()?;
    if canon.starts_with(root) && canon.is_file() {
        Some(canon)
    } else {
        None
    }
}

pub(super) fn resolve_web_root() -> Option<std::path::PathBuf> {
    let env_root = std::env::var("OPENUNPACK_WEB_ROOT").ok();
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(std::path::Path::to_path_buf));
    web_root_candidates(env_root.as_deref(), exe_dir.as_deref())
        .into_iter()
        .find_map(|candidate| {
            let root = is_safe_web_dir(&candidate)?;
            let index = root.join("index.html");
            is_safe_web_file(&root, &index)?;
            Some(root)
        })
}

pub(super) async fn serve_index() -> Result<Response, ApiError> {
    if let Some(root) = resolve_web_root() {
        let index_file = root.join("index.html");
        if let Some(safe_index) = is_safe_web_file(&root, &index_file)
            && let Ok(content) = std::fs::read_to_string(&safe_index)
        {
            let content = content.replacen(
                "<head>",
                "<head><meta name=\"openunpack-web-console\" content=\"1\">",
                1,
            );
            return Ok((
                [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
                content,
            )
                .into_response());
        }
    }

    let fallback_html = r#"<!DOCTYPE html>
<html lang="zh-CN">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <title>OpenUnpack Web Console</title>
  <style>
    :root { --bg: #0b0f19; --card: #151d30; --text: #f1f5f9; --muted: #94a3b8; --accent: #38bdf8; --border: #1e293b; --green: #10b981; }
    * { box-sizing: border-box; }
    body { font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif; background: var(--bg); color: var(--text); margin: 0; padding: 2rem; display: flex; justify-content: center; }
    .container { max-width: 860px; width: 100%; }
    .header { display: flex; align-items: center; justify-content: space-between; margin-bottom: 2rem; }
    .brand { display: flex; align-items: center; gap: 0.75rem; }
    .brand svg { width: 32px; height: 32px; fill: var(--accent); }
    h1 { font-size: 1.5rem; margin: 0; font-weight: 700; letter-spacing: -0.025em; }
    .status-badge { display: inline-flex; align-items: center; gap: 0.4rem; padding: 0.35rem 0.75rem; border-radius: 9999px; font-size: 0.8rem; font-weight: 600; background: rgba(16, 185, 129, 0.15); color: var(--green); border: 1px solid rgba(16, 185, 129, 0.3); }
    .status-dot { width: 8px; height: 8px; border-radius: 50%; background: var(--green); box-shadow: 0 0 8px var(--green); }
    .card { background: var(--card); border: 1px solid var(--border); border-radius: 12px; padding: 1.5rem; margin-bottom: 1.5rem; box-shadow: 0 10px 15px -3px rgba(0,0,0,0.3); }
    .card-title { font-size: 1.1rem; font-weight: 600; margin-top: 0; margin-bottom: 0.5rem; color: #fff; }
    .card-desc { color: var(--muted); font-size: 0.9rem; margin-bottom: 1rem; line-height: 1.5; }
    .grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(240px, 1fr)); gap: 1rem; margin-bottom: 1.5rem; }
    .nav-box { background: rgba(255,255,255,0.03); border: 1px solid var(--border); border-radius: 8px; padding: 1rem; text-decoration: none; color: inherit; transition: all 0.2s; display: block; }
    .nav-box:hover { border-color: var(--accent); transform: translateY(-2px); }
    .nav-box-title { font-weight: 600; color: var(--accent); margin-bottom: 0.25rem; }
    .nav-box-desc { font-size: 0.8rem; color: var(--muted); }
    .token-box { display: flex; gap: 0.5rem; margin-top: 1rem; }
    input[type="text"] { flex: 1; background: #0b0f19; border: 1px solid var(--border); border-radius: 6px; padding: 0.5rem 0.75rem; color: #fff; font-family: monospace; font-size: 0.85rem; }
    input[type="text"]:focus { outline: none; border-color: var(--accent); }
    button { background: var(--accent); color: #0b0f19; border: none; border-radius: 6px; padding: 0.5rem 1rem; font-weight: 600; cursor: pointer; transition: opacity 0.2s; }
    button:hover { opacity: 0.9; }
    pre { background: #070a12; border: 1px solid var(--border); border-radius: 8px; padding: 1rem; overflow-x: auto; font-family: ui-monospace, SFMono-Regular, Menlo, monospace; font-size: 0.82rem; color: #7dd3fc; margin: 0; }
  </style>
</head>
<body>
  <div class="container">
    <div class="header">
      <div class="brand">
        <svg viewBox="0 0 24 24"><path d="M21 16.5c0 .38-.21.71-.53.88l-7.9 4.44c-.16.12-.36.18-.57.18s-.41-.06-.57-.18l-7.9-4.44A.991.991 0 0 1 3 16.5v-9c0-.38.21-.71.53-.88l7.9-4.44c.16-.12.36-.18.57-.18s.41.06.57.18l7.9 4.44c.32.17.53.5.53.88v9zM12 4.15L6.04 7.5 12 10.85l5.96-3.35L12 4.15z"/></svg>
        <h1>OpenUnpack Daemon</h1>
      </div>
      <div class="status-badge"><span class="status-dot"></span><span>Daemon Running</span></div>
    </div>
    <div class="card">
      <div class="card-title">开发者与第三方接入生态</div>
      <div class="card-desc">OpenUnpack Daemon 提供完整的无头后台服务。您可以通过 REST API、WebSocket 双向通信或 CLI 远程调度解压集群。</div>
      <div class="grid">
        <a class="nav-box" href="/docs/">
          <div class="nav-box-title">Swagger API 文档</div>
          <div class="nav-box-desc">实时交互式 OpenAPI 接口在线调试与参数预览。</div>
        </a>
        <a class="nav-box" href="/openapi.json">
          <div class="nav-box-title">OpenAPI 规范</div>
          <div class="nav-box-desc">标准 JSON 契约文件，一键导入 Postman 或生成 SDK。</div>
        </a>
        <a class="nav-box" href="/api/v1/health">
          <div class="nav-box-title">服务健康检查</div>
          <div class="nav-box-desc">GET /api/v1/health 快速探测服务存活状态。</div>
        </a>
      </div>
    </div>
    <div class="card">
      <div class="card-title">实时状态探查 (Snapshot)</div>
      <div class="card-desc">输入您的 API Client Token，即可在线读取解压队列与系统状态：</div>
      <div class="token-box">
        <input type="password" autocomplete="off" id="tokenInput" placeholder="输入 API Token (如 openunpack-token-...)">
        <button id="fetchSnapshotBtn" type="button">查询状态</button>
      </div>
      <div style="margin-top: 1rem;">
        <pre id="output">// 点击“查询状态”读取当前活动任务...</pre>
      </div>
    </div>
  </div>
  <script src="/assets/daemon-fallback.js"></script>
</body>
</html>"#;

    Ok((
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        fallback_html.as_bytes(),
    )
        .into_response())
}

pub(super) const DAEMON_FALLBACK_JS: &str = r#"const saved = sessionStorage.getItem('openunpack_token');
if (saved) {
  const input = document.getElementById('tokenInput');
  if (input) input.value = saved;
}
async function fetchSnapshot() {
  const input = document.getElementById('tokenInput');
  const token = input ? input.value.trim() : '';
  if (token) sessionStorage.setItem('openunpack_token', token);
  const out = document.getElementById('output');
  if (!out) return;
  out.textContent = '正在获取状态...';
  try {
    const headers = token ? { Authorization: 'Bearer ' + token } : {};
    const res = await fetch('/api/v1/commands', {
      method: 'POST',
      redirect: 'error',
      headers: { ...headers, 'Content-Type': 'application/json' },
      body: JSON.stringify({ command: 'queue_snapshot' }),
    });
    const data = await res.json();
    out.textContent = JSON.stringify(data, null, 2);
  } catch (e) {
    out.textContent = '请求失败: ' + (e && e.message ? e.message : String(e));
  }
}
document.addEventListener('DOMContentLoaded', () => {
  const btn = document.getElementById('fetchSnapshotBtn');
  if (btn) btn.addEventListener('click', fetchSnapshot);
});
"#;

pub(super) async fn serve_asset(Path(asset): Path<String>) -> Result<Response, ApiError> {
    if asset.contains("..") || asset.contains('/') || asset.contains('\\') || asset.contains(':') {
        return Err(ApiError::new(400, "invalid_path", "非法的资源路径。"));
    }
    if asset == "daemon-fallback.js" {
        return Ok((
            [
                (
                    header::CONTENT_TYPE,
                    "application/javascript; charset=utf-8",
                ),
                (header::CACHE_CONTROL, "public, max-age=3600"),
            ],
            DAEMON_FALLBACK_JS,
        )
            .into_response());
    }

    if let Some(root) = resolve_web_root() {
        let file_path = root.join("assets").join(&asset);
        if let Some(safe_file) = is_safe_web_file(&root, &file_path)
            && let Ok(bytes) = std::fs::read(&safe_file)
        {
            let mime = match asset.rsplit('.').next() {
                Some("js") | Some("mjs") => "application/javascript; charset=utf-8",
                Some("css") => "text/css; charset=utf-8",
                Some("svg") => "image/svg+xml",
                Some("png") => "image/png",
                Some("jpg") | Some("jpeg") => "image/jpeg",
                Some("ico") => "image/x-icon",
                Some("woff") => "font/woff",
                Some("woff2") => "font/woff2",
                Some("json") => "application/json; charset=utf-8",
                _ => "application/octet-stream",
            };
            return Ok((
                [
                    (header::CONTENT_TYPE, mime),
                    (header::CACHE_CONTROL, "public, max-age=31536000, immutable"),
                ],
                bytes,
            )
                .into_response());
        }
    }

    Err(ApiError::new(404, "not_found", "资源文件不存在。"))
}

pub(super) async fn serve_favicon() -> Result<Response, ApiError> {
    if let Some(root) = resolve_web_root() {
        let ico = root.join("favicon.ico");
        if let Some(safe_ico) = is_safe_web_file(&root, &ico)
            && let Ok(bytes) = std::fs::read(&safe_ico)
        {
            return Ok(([(header::CONTENT_TYPE, "image/x-icon")], bytes).into_response());
        }
    }
    const EMPTY_ICO: &[u8] = &[
        0x00, 0x00, 0x01, 0x00, 0x01, 0x00, 0x01, 0x01, 0x00, 0x00, 0x01, 0x00, 0x20, 0x00, 0x68,
        0x04, 0x00, 0x00, 0x16, 0x00, 0x00, 0x00,
    ];
    Ok(([(header::CONTENT_TYPE, "image/x-icon")], EMPTY_ICO).into_response())
}

pub(super) async fn swagger_asset(Path(asset): Path<String>) -> Result<Response, ApiError> {
    let (mime, bytes): (&str, &'static [u8]) = match asset.as_str() {
        "swagger-ui.css" => (
            "text/css; charset=utf-8",
            include_bytes!("../../assets/swagger-ui/swagger-ui.css"),
        ),
        "swagger-ui-bundle.js" => (
            "text/javascript; charset=utf-8",
            include_bytes!("../../assets/swagger-ui/swagger-ui-bundle.js"),
        ),
        "swagger-ui-standalone-preset.js" => (
            "text/javascript; charset=utf-8",
            include_bytes!("../../assets/swagger-ui/swagger-ui-standalone-preset.js"),
        ),
        "swagger-initializer.js" => (
            "text/javascript; charset=utf-8",
            include_bytes!("../../assets/swagger-ui/swagger-initializer.js"),
        ),
        _ => return Err(ApiError::new(404, "not_found", "文档资源不存在。")),
    };
    Ok(([(header::CONTENT_TYPE, mime)], bytes).into_response())
}
