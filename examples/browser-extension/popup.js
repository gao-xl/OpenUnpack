document.addEventListener('DOMContentLoaded', async () => {
  const data = await chrome.storage.local.get({ port: 18420, token: '' });
  document.getElementById('portInput').value = data.port;
  document.getElementById('tokenInput').value = data.token;

  document.getElementById('saveBtn').addEventListener('click', async () => {
    const port = parseInt(document.getElementById('portInput').value, 10) || 18420;
    const token = document.getElementById('tokenInput').value.trim();
    await chrome.storage.local.set({ port, token });
    alert('配置已保存！');
    checkStatus(port);
  });

  document.getElementById('openWebUiBtn').addEventListener('click', async () => {
    const port = parseInt(document.getElementById('portInput').value, 10) || 18420;
    chrome.tabs.create({ url: `http://127.0.0.1:${port}/` });
  });

  checkStatus(data.port);
});

async function checkStatus(port) {
  const badge = document.getElementById('daemon-badge');
  try {
    const res = await fetch(`http://127.0.0.1:${port}/api/v1/health`);
    if (res.ok) {
      badge.textContent = '在线';
      badge.style.color = '#34d399';
    } else {
      badge.textContent = '异常';
      badge.style.color = '#f87171';
    }
  } catch {
    badge.textContent = '离线';
    badge.style.color = '#94a3b8';
  }
}
