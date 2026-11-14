export const esc = (v) =>
  String(v ?? '').replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));

export const pct = (v) => Math.round(Math.max(0, Math.min(1, v)) * 100);

export const uptime = (secs) => {
  const s = Math.max(0, Math.floor(secs || 0));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  return h ? `${h} 小时 ${String(m).padStart(2, '0')} 分` : `${m} 分 ${String(s % 60).padStart(2, '0')} 秒`;
};
