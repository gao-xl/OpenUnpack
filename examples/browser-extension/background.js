// ==============================================================================
// OpenUnpack Browser Extension Background Service Worker (Manifest V3)
// ==============================================================================

const DEFAULT_PORT = 18420;
let ws = null;
let reconnectTimer = null;

async function getConfig() {
  const data = await chrome.storage.local.get({
    port: DEFAULT_PORT,
    token: ''
  });
  return data;
}

async function sendApiCommand(command, args = null) {
  const cfg = await getConfig();
  const url = `http://127.0.0.1:${cfg.port}/api/v1/commands`;
  const headers = { 'Content-Type': 'application/json' };
  if (cfg.token) {
    headers['Authorization'] = `Bearer ${cfg.token}`;
  }

  const payload = { command };
  if (args) payload.args = args;

  const res = await fetch(url, {
    method: 'POST',
    headers,
    body: JSON.stringify(payload)
  });

  if (!res.ok) {
    const errText = await res.text();
    throw new Error(`API error ${res.status}: ${errText}`);
  }
  return await res.json();
}

// Register Context Menu
chrome.runtime.onInstalled.addListener(() => {
  chrome.contextMenus.create({
    id: 'openunpack-download-unpack',
    title: '解压至 OpenUnpack',
    contexts: ['link', 'selection']
  });
});

chrome.contextMenus.onClicked.addListener(async (info, tab) => {
  if (info.menuItemId === 'openunpack-download-unpack') {
    const targetUrl = info.linkUrl || info.selectionText;
    if (targetUrl) {
      console.log('Dispatching URL to OpenUnpack:', targetUrl);
      try {
        // Trigger AddArchive command
        await sendApiCommand('AddArchive', {
          path: targetUrl,
          autoStart: true
        });
        chrome.notifications.create({
          type: 'basic',
          iconUrl: 'icon.png',
          title: 'OpenUnpack',
          message: '已添加解压作业至后台队列'
        });
      } catch (err) {
        console.error('Failed to dispatch:', err);
      }
    }
  }
});

// Auto-detect completed browser archive downloads
chrome.downloads.onChanged.addListener(async (delta) => {
  if (delta.state && delta.state.current === 'complete') {
    const items = await chrome.downloads.search({ id: delta.id });
    if (items && items.length > 0) {
      const filePath = items[0].filename;
      const lower = filePath.toLowerCase();
      if (lower.endsWith('.zip') || lower.endsWith('.7z') || lower.endsWith('.rar') || lower.endsWith('.tar.gz')) {
        console.log('Detected downloaded archive:', filePath);
        try {
          await sendApiCommand('AddArchive', {
            path: filePath,
            autoStart: true
          });
          chrome.notifications.create({
            type: 'basic',
            iconUrl: 'icon.png',
            title: 'OpenUnpack 自动解压',
            message: `已自动调度解压: ${items[0].filename.split(/[\\/]/).pop()}`
          });
        } catch (e) {
          console.warn('Auto-unpack submission error:', e);
        }
      }
    }
  }
});

// Connect to Daemon WebSocket for real-time alerts
async function connectWebSocket() {
  const cfg = await getConfig();
  const wsUrl = `ws://127.0.0.1:${cfg.port}/api/v1/ws${cfg.token ? `?token=${encodeURIComponent(cfg.token)}` : ''}`;
  
  try {
    ws = new WebSocket(wsUrl);
    ws.onopen = () => {
      console.log('Connected to OpenUnpack WebSocket stream');
    };
    ws.onmessage = (event) => {
      try {
        const msg = JSON.parse(event.data);
        if (msg.event === 'need_password' && msg.data) {
          chrome.notifications.create({
            type: 'basic',
            iconUrl: 'icon.png',
            title: 'OpenUnpack 需要解压密码',
            message: `任务 ${msg.data.jobId} 遇到加密包，请前往控制台输入口令。`
          });
        }
      } catch {}
    };
    ws.onclose = () => {
      clearTimeout(reconnectTimer);
      reconnectTimer = setTimeout(connectWebSocket, 5000);
    };
  } catch (err) {
    clearTimeout(reconnectTimer);
    reconnectTimer = setTimeout(connectWebSocket, 5000);
  }
}

connectWebSocket();
