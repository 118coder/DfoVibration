// 和 Rust 后端说话。所有请求都带一个只在本次启动有效的口令，
// 后端校验口令 + 只监听 127.0.0.1，别的程序拿不到这个页面就用不了接口。

const token = document.querySelector('meta[name="sorahk-token"]')?.content || '';

async function call(path, { method = 'GET', body } = {}) {
  const res = await fetch(path, {
    method,
    headers: {
      'Content-Type': 'application/json',
      'X-Sorahk-Token': token,
    },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  let data = null;
  try {
    data = await res.json();
  } catch {
    data = null;
  }
  if (!res.ok || (data && data.ok === false)) {
    throw new Error((data && data.error) || `请求失败 (${res.status})`);
  }
  return data ?? {};
}

export const api = {
  bootstrap: () => call('/api/bootstrap'),
  state: () => call('/api/state'),
  logs: (n = 200) => call(`/api/logs?n=${n}`),
  action: (act) => call('/api/action', { method: 'POST', body: { act } }),
  patchConfig: (patch) => call('/api/config', { method: 'POST', body: { patch } }),
  patchVibration: (patch) => call('/api/vibration', { method: 'POST', body: { patch } }),
  saveMappings: (mappings) => call('/api/mappings', { method: 'POST', body: { mappings } }),
};

// 状态实时推送（服务端每 500ms 采一次）
export function streamStatus(onStatus, onError) {
  const es = new EventSource(`/api/events?t=${encodeURIComponent(token)}`);
  es.addEventListener('status', (e) => {
    try {
      onStatus(JSON.parse(e.data));
    } catch {
      /* 一次坏包丢掉就好 */
    }
  });
  es.addEventListener('error', () => onError?.());
  return es;
}
