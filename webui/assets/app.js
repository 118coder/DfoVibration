// 控制台的骨架：整块 1600×900 舞台按窗口等比缩放 → 取一次全量数据 → 画圆盘和看板
// → 起实时推送 → 按分区换工作区。所有改动都从 saveConfig / saveVibration 出去。
import { api, streamStatus } from './api.js';
import { createDial } from './dial.js';
import {
  GUIDE_STEPS, PAGES, kbBatchBar, mappingRows, padOverlay, padWizardSlot, padWizardTotal,
  render, renderMinimal, renderTurboOnly, routeOrder,
} from './pages.js';
import { esc, uptime } from './util.js';

const $ = (sel, root = document) => root.querySelector(sel);
const $$ = (sel, root = document) => [...root.querySelectorAll(sel)];

const S = {
  version: '',
  status: null,
  cfg: null,
  vib: null,
  logs: [],
  route: 'overview',
  sel: -1,
  draft: null,
  updateLine: '',
  padFocus: false,
  mode: 'full',
  guideStep: 0,
  /* 下面这些是「便利性」的暂存状态：切换页面/重渲染都留在同一份 S 里 */
  capture: null,          // 正在等一个实体键
  kbAdd: [],              // 键盘图上准备新建同名连发的键
  kbRemove: [],           // 准备移除的同名连发键
  kbConflicts: [],        // 被别的映射占住的键
  chordQuery: '',
  chordPurgeArmed: false,
  presetArmed: false,
  delArmed: false,
  wizStep: -1,
  wizDone: [],
  seqRecording: false,
  seqKeys: [],
  seqShowMacros: false,
  vibPresets: [],
};

let dial = null;
let saveTimer = null;

/* ------------------------------------------------------------ 16:9 舞台 */

function fitStage() {
  const s = Math.min(window.innerWidth / 1600, window.innerHeight / 900);
  document.documentElement.style.setProperty('--s', String(s));
  // 舞台外的底色和舞台相同，窗口比例不对时就是一圈纸边
  document.body.style.setProperty('--edge', s < 1 ? '0' : '0');
}

/* ------------------------------------------------------------------ 小工具 */

function toast(text, kind = 'ok') {
  const t = $('#toast');
  t.textContent = text;
  t.className = `toast show${kind === 'err' ? ' err' : ''}`;
  clearTimeout(t._timer);
  t._timer = setTimeout(() => (t.className = 'toast'), 2200);
}

const setPath = (obj, path, val) => {
  const keys = path.split('.');
  let o = obj;
  for (let i = 0; i < keys.length - 1; i++) {
    if (typeof o[keys[i]] !== 'object' || o[keys[i]] === null) o[keys[i]] = {};
    o = o[keys[i]];
  }
  o[keys[keys.length - 1]] = val;
  return obj;
};

const getPath = (obj, path) => path.split('.').reduce((o, k) => (o == null ? undefined : o[k]), obj);

function countOf(cfg) {
  const list = cfg.mappings || [];
  return {
    mappings: list.length,
    turbo: list.filter((m) => m.turbo_enabled).length,
    combos: list.filter((m) => String(m.trigger_key).includes('+')).length,
    presets: (cfg.presets || []).length,
    whitelist: (cfg.process_whitelist || []).length,
    macros: (cfg.universal_macros || []).length,
  };
}

function mergeCfg(next) {
  S.cfg = next;
  if (S.status) {
    S.status.counts = countOf(next);
    renderDial();
    renderPanels();
  }
}

/* ------------------------------------------------------------------ 保存 */

async function saveConfig(patch, { quiet = true } = {}) {
  try {
    const res = await api.patchConfig(patch);
    mergeCfg(res.config);
    if (!quiet) toast('已保存到 Config.toml');
    return true;
  } catch (e) {
    toast(e.message, 'err');
    return false;
  }
}

async function saveVibration(patch) {
  try {
    const res = await api.patchVibration(patch);
    S.vib = res.vibration;
    renderPanels();
    toast('已保存震动设定');
    return true;
  } catch (e) {
    toast(e.message, 'err');
    return false;
  }
}

const scheduleSave = (path, val, vib) => {
  clearTimeout(saveTimer);
  saveTimer = setTimeout(() => {
    if (vib) saveVibration(setPath({}, path, val));
    else saveConfig(setPath({}, path, val));
  }, 350);
};

async function saveMappings(mappings) {
  try {
    const res = await api.saveMappings(mappings);
    mergeCfg(res.config);
    toast('映射已保存');
    return true;
  } catch (e) {
    toast(e.message, 'err');
    return false;
  }
}

/* ------------------------------------------------------------------ 圆盘 */

function dialSections() {
  const here = PAGES[S.route] ? S.route : 'overview';
  return routeOrder().map((id) => {
    const p = PAGES[id];
    return {
      id,
      zh: p.zh,
      en: p.en.toUpperCase(),
      big: p.dialBig(S),
      sub: p.dialSub(S),
      here: id === here,
      alert: id === 'whitelist' && !S.cfg?.whitelist_enabled,
    };
  });
}

// 圆盘按中间栏的短边取正方形，窗口缩放时跟着重算
function sizeDial() {
  const hub = $('.hub');
  const svg = $('#dial');
  if (!hub || !svg) return;
  const side = Math.max(220, Math.min(hub.clientWidth, hub.clientHeight));
  svg.style.width = `${side}px`;
  svg.style.height = `${side}px`;
}

function renderDial() {
  const sections = dialSections();
  if (!dial) {
    dial = createDial($('#dial'), { sections, onNavigate: (id) => go(id) });
  } else {
    dial.update(sections);
  }
  dial.hub(hubState());
  sizeDial();
}

const hex = (n, pad = 4) => Number(n).toString(16).toUpperCase().padStart(pad, '0');

function hubState() {
  const hid = S.status?.input?.hid;
  return {
    device: hid?.present ? `${hex(hid.vid)}:${hex(hid.pid)}` : '未连接',
    api: hid?.present ? 'RAW HID' : '',
    connected: !!hid?.present,
  };
}

/* ------------------------------------------------------------------ 看板 */

function renderPanels() {
  const st = S.status;
  if (!st) return;
  const c = st.counts || countOf(S.cfg);
  const cfg = S.cfg || {};

  $('#cmd-ver').textContent = S.version || '—';
  const rs = $('#runstat');
  rs.classList.toggle('is-paused', !!st.paused);
  rs.querySelector('b').textContent = st.paused ? '已暂停' : '运行中';

  $('#figs-runtime').innerHTML = `
    <a href="#gamepad"><b>${c.mappings}</b><span>映射</span></a>
    <a href="#settings"><b>${c.presets}</b><span>预设</span></a>
    <a href="#turbo"${c.turbo ? ' class="is-alert"' : ''}><b>${c.turbo}</b><span>连发</span></a>`;

  $('#ledger-run').innerHTML = `
    <li><span>已运行</span><span class="ef-field">${uptime(st.uptime)}</span></li>
    <li><span>宏库</span><span class="ef-field">${c.macros} 条</span></li>
    <li><span>工作线程</span><span class="ef-field">${st.workers ?? cfg.worker_count ?? 0} 个</span></li>`;

  $('#ledger-input').innerHTML = `
    <li><span>输入来源</span><span class="ef-field">${st.input?.source ? esc(st.input.source) : '手柄未接入'}</span></li>
    <li><span>切换键</span><span class="ef-field">${esc(cfg.switch_key || '未设置')}</span></li>
    <li><span>组合键</span><span class="ef-field">${c.combos} 组</span></li>`;

  $('#figs-wl').innerHTML = `
    <a href="#whitelist"><b>${c.whitelist}</b><span>条目</span></a>
    <a href="#whitelist"${cfg.whitelist_enabled ? '' : ' class="is-alert"'}><b class="is-text">${
      cfg.whitelist_enabled ? '只放行' : '全放行'
    }</b><span>当前</span></a>`;

  $('#ledger-wl').innerHTML = `
    <li><span>比对方式</span><span class="ef-field">${cfg.whitelist_simple_mode ? '进程名' : '完整路径'}</span></li>
    <li><span>游戏进程</span><span class="ef-field">${esc((cfg.process_whitelist || [])[0] || '未登记')}</span></li>`;

  $('#ledger-hap').innerHTML = `
    <li><span>游戏震动</span><span class="ef-field ${S.vib?.enabled ? '' : 'warn'}">${S.vib?.enabled ? '开' : '关'}</span></li>
    <li><span>总强度</span><span class="ef-field">${S.vib?.master_gain ?? 0}%</span></li>
    <li><span>音频震动</span><span class="ef-field">${cfg.audio_haptics?.enabled ? '开' : '关'}</span></li>`;

  $('#foot-upd').textContent = `更新 · ${updateText(st.update)}`;

  const list = S.cfg?.mappings || [];
  const ov = $('#tbl-overview');
  if (ov) {
    ov.innerHTML = list.length
      ? `<div class="tbl">${mappingRows(list, { limit: 5 })}</div>`
      : '<p class="empty-note">还没有映射。到「手柄映射」点图上的键就能建第一条。</p>';
  }
  const lo = $('#log-overview');
  if (lo) {
    const logs = (S.logs || []).slice(-7).reverse();
    lo.innerHTML = logs.length
      ? logs.map((l) => `<li><span class="t">${esc(l.t || fmtLogTime(l.ms))}</span><span>${esc(l.m)}</span></li>`).join('')
      : '<li><span>—</span><span>日志还没有内容</span></li>';
  }
  const qo = $('#quick-overview');
  if (qo) {
    qo.innerHTML = `
      <button class="cmd-btn" type="button" data-act="new-mapping">新增映射</button>
      <button class="cmd-btn" type="button" data-act="check-update">检查更新</button>
      <button class="cmd-btn" type="button" data-act="save">写入配置</button>
      <button class="cmd-btn" type="button" data-act="show-guide">上手引导</button>`;
  }
  const qp = $('#quick-preset');
  if (qp && document.activeElement !== qp) {
    const ps = S.cfg?.presets || [];
    qp.innerHTML = ps.length
      ? ps.map((p, i) => `<option value="${i}"${p.name === S.cfg?.current_preset ? ' selected' : ''}>${esc(p.name)}</option>`).join('')
      : '<option value="">无预设</option>';
  }
  updateMotor(st.motor);
}

function fmtLogTime(ms) {
  if (!ms) return '';
  return new Date(ms).toLocaleTimeString('zh-CN', { hour12: false });
}

function updateText(u) {
  if (!u) return '未检查';
  if (u.error) return '检查失败';
  if (u.phase === 'Checking') return '检查中…';
  if (u.latest && u.latest !== S.version) return `有新版 ${u.latest}`;
  return '已是最新';
}

function updateMotor(m) {
  if (!m) return;
  const l = Math.max(0, Math.min(1, m.l || 0));
  const r = Math.max(0, Math.min(1, m.r || 0));
  $$('[data-motor]').forEach((n) => n.style.setProperty('--v', n.dataset.motor === 'l' ? l : r));
  $$('[data-live="motor"]').forEach((g) => g.classList.toggle('is-idle', l < 0.01 && r < 0.01));
}

/* ------------------------------------------------------------------ 键盘图 */

// 连发页的键盘是外部 SVG：取一次原样塞进 DOM，之后只切按键的 is-lit。
// 之所以 inline 而不是 <img>，是为了能用主题变量给键帽上色、并高亮发送键。
let kbLoaded = false;
async function mountKeyboard() {
  const host = $('#kb-board');
  if (!host) return;
  if (!kbLoaded || !host.querySelector('svg')) {
    try {
      const res = await fetch('/assets/keyboard.svg');
      host.innerHTML = await res.text();
      kbLoaded = true;
    } catch {
      host.innerHTML = '<p class="empty-note">键盘图没加载出来。</p>';
      return;
    }
  }
  const lit = new Set(String(host.dataset.lit || '').split(',').filter(Boolean));
  const pick = new Set(S.kbAdd || []);
  const drop = new Set(S.kbRemove || []);
  host.querySelectorAll('.kb-key').forEach((g) => {
    g.classList.toggle('is-lit', lit.has(g.id));
    g.classList.toggle('is-pick', pick.has(g.id));
    g.classList.toggle('is-drop', drop.has(g.id));
  });
}

/* ------------------------------------------------------------------ 路由 */

function go(route) {
  if (location.hash === `#${route}`) renderWork(true);
  else location.hash = `#${route}`;
}

function currentRoute() {
  const id = location.hash.replace('#', '') || 'overview';
  return PAGES[id] || id === 'logs' ? id : 'overview';
}

/* ------------------------------------------------------------------ 手柄图 */

// 手柄底图和热点在同一个 SVG 里，共用 viewBox "22 135 545 401"：
// 圆点坐标就是原版标定的那套，缩放交给 SVG 自己，不用再算像素。
let padTheme = '';
async function mountPad() {
  const host = $('#pad-art');
  if (!host) return;
  const theme = document.documentElement.dataset.theme === 'dark' ? 'dark' : 'light';
  const overlay = padOverlay(S.cfg?.mappings || []);
  if (padTheme !== theme || !host.querySelector('svg')) {
    try {
      const txt = await fetch(`/assets/pad-map-${theme}.svg`).then((r) => r.text());
      host.innerHTML = txt.replace('</svg>', `<g class="pad-hot">${overlay}</g></svg>`);
      padTheme = theme;
    } catch {
      host.innerHTML = '<p class="empty-note">手柄图没加载出来。</p>';
    }
  } else {
    const g = host.querySelector('.pad-hot');
    if (g) g.innerHTML = overlay;
  }
}

function renderWork(animate = true) {
  S.route = currentRoute();
  document.body.dataset.route = S.route;
  const work = $('#work');
  if (S.mode === 'minimal') work.innerHTML = renderMinimal(S);
  else if (S.mode === 'turbo') work.innerHTML = renderTurboOnly(S);
  else work.innerHTML = render(S.route, S);
  if (S.mode === 'turbo' || (S.mode === 'full' && S.route === 'turbo')) mountKeyboard();
  if (S.route === 'gamepad') mountPad();
  if (animate) {
    work.classList.remove('swap');
    void work.offsetWidth;
    work.classList.add('swap');
  }
  $$('.scroll', work).forEach((n) => (n.scrollTop = 0));
  if (dial) dial.update(dialSections());
}

/* ------------------------------------------------------------------ 模式 */

function modeOf(cfg) {
  if (cfg?.classic_mode) return 'turbo';
  if (cfg?.minimal_mode) return 'minimal';
  return 'full';
}

// 三套界面共用同一份 S：切模式只是换 renderer，不重建状态，
// 所以「极简里关了震动」回到完整界面看到的就是关着的。
function applyMode() {
  document.body.dataset.mode = S.mode;
  const nav = $('#nav');
  if (nav) nav.hidden = S.mode !== 'full';
  $$('.mode-switch button').forEach((b) => {
    b.setAttribute('aria-pressed', String(b.dataset.mode === S.mode));
  });
}

async function setMode(mode) {
  if (!['full', 'minimal', 'turbo'].includes(mode)) return;
  const patch = { minimal_mode: mode === 'minimal', classic_mode: mode === 'turbo' };
  if (!(await saveConfig(patch))) return;
  S.mode = mode;
  applyMode();
  if (mode !== 'full') S.route = 'overview';
  renderWork(false);
  if (mode === 'minimal') blip('on');
}

/* ------------------------------------------------------------ 提示音与震动 */

// 极简模式下每次切换给一点即时反馈：短促的提示音 + 一次很轻的震动。
// 声音用 WebAudio 现合成，不额外带音频文件；震动走原生测试脉冲。
let audioCtx = null;
function blip(kind = 'on') {
  try {
    audioCtx = audioCtx || new (window.AudioContext || window.webkitAudioContext)();
    const o = audioCtx.createOscillator();
    const g = audioCtx.createGain();
    o.type = 'square';
    o.frequency.value = kind === 'off' ? 320 : 720;
    g.gain.setValueAtTime(0.0001, audioCtx.currentTime);
    g.gain.exponentialRampToValueAtTime(0.06, audioCtx.currentTime + 0.01);
    g.gain.exponentialRampToValueAtTime(0.0001, audioCtx.currentTime + 0.12);
    o.connect(g).connect(audioCtx.destination);
    o.start();
    o.stop(audioCtx.currentTime + 0.13);
  } catch {
    /* 浏览器不给放声音就算了 */
  }
  if (S.mode === 'minimal' && S.vib?.enabled) api.action('test-pulse').catch(() => {});
}

/* ------------------------------------------------------------------ 引导 */

function showGuide() {
  const host = $('#guide');
  if (!host) return;
  const i = S.guideStep;
  const [title, body] = GUIDE_STEPS[i] || GUIDE_STEPS[0];
  host.hidden = false;
  host.innerHTML = `<div class="guide-card" role="dialog" aria-modal="true" aria-label="上手引导">
    <div class="guide-step">${i + 1} / ${GUIDE_STEPS.length}</div>
    <h2>${esc(title)}</h2>
    <p>${esc(body)}</p>
    <div class="guide-actions">
      <button class="ef-btn" type="button" data-guide="skip">跳过</button>
      <span class="guide-dots">${GUIDE_STEPS.map((_, k) => `<i class="${k === i ? 'is-on' : ''}"></i>`).join('')}</span>
      ${
        i < GUIDE_STEPS.length - 1
          ? '<button class="ef-btn go" type="button" data-guide="next">下一步</button>'
          : '<button class="ef-btn go" type="button" data-guide="done">开始使用</button>'
      }
    </div>
  </div>`;
}

async function closeGuide() {
  $('#guide').hidden = true;
  if (!S.cfg?.guide_seen) {
    S.cfg.guide_seen = true;
    await saveConfig({ guide_seen: true });
  }
}

function renderNav() {
  const short = {
    overview: '总览', gamepad: '手柄', turbo: '连发', whitelist: '白名单',
    vibration: '震动', audio: '音频', settings: '设置', logs: '日志',
  };
  $('#nav').innerHTML = routeOrder()
    .map((id, i) => {
      const p = PAGES[id];
      return `<a href="#${id}"${id === S.route ? ' aria-current="page"' : ''} title="${esc(p.zh)}">
        <i>${String(i + 1).padStart(2, '0')}</i><b>${esc(short[id] || p.zh)}</b></a>`;
    })
    .join('') + '<div class="rail-foot"><span>1–8</span><span>切换</span></div>';
}

const applyTheme = (dark) => {
  document.documentElement.dataset.theme = dark ? 'dark' : 'light';
  padTheme = '';
  if (S.route === 'gamepad') mountPad();
};


/* ═══════════════════════════════════════════════════════════════════════════
   便利性交互的实现（原版那些「一键 / 批量 / 捕获 / 预设」）
   ═══════════════════════════════════════════════════════════════════════════ */

const VIB_STORE = 'sorahk.web.vibpresets';
function loadVibPresets() {
  try { return JSON.parse(localStorage.getItem(VIB_STORE) || '[]') || []; } catch { return []; }
}
function storeVibPresets(list) {
  S.vibPresets = list;
  try { localStorage.setItem(VIB_STORE, JSON.stringify(list)); } catch { /* file:// 下可能不给存 */ }
}

function downloadJson(filename, obj) {
  try {
    const blob = new Blob([JSON.stringify(obj, null, 2)], { type: 'application/json' });
    const a = document.createElement('a');
    a.href = URL.createObjectURL(blob);
    a.download = filename;
    a.click();
    setTimeout(() => { try { URL.revokeObjectURL(a.href); } catch { /* ignore */ } }, 800);
  } catch (e) { toast('导出失败：' + e.message, 'err'); }
}
function readJson(file) {
  return new Promise((resolve, reject) => {
    const r = new FileReader();
    r.onload = () => { try { resolve(JSON.parse(String(r.result))); } catch (e) { reject(e); } };
    r.onerror = () => reject(new Error('读文件失败'));
    r.readAsText(file);
  });
}

/** KeyboardEvent → 项目里用的键名（A / F8 / SPACE / CTRL+F6 …）。 */
function keyNameFromEvent(e) {
  const k = e.key;
  const map = {
    ' ': 'SPACE', Escape: 'ESC', Enter: 'ENTER', Backspace: 'BACKSPACE', Tab: 'TAB',
    Delete: 'DELETE', Insert: 'INSERT', Home: 'HOME', End: 'END',
    ArrowUp: 'UP', ArrowDown: 'DOWN', ArrowLeft: 'LEFT', ArrowRight: 'RIGHT',
    PageUp: 'PAGEUP', PageDown: 'PAGEDOWN', CapsLock: 'CAPSLOCK',
    Control: 'CTRL', Shift: 'SHIFT', Alt: 'ALT', Meta: 'LWIN',
    '`': 'OEM_3', '-': 'MINUS', '=': 'EQUAL', '[': 'LBRACKET', ']': 'RBRACKET',
    '\\': 'BACKSLASH', ';': 'SEMICOLON', "'": 'QUOTE', ',': 'COMMA', '.': 'PERIOD', '/': 'SLASH',
  };
  if (map[k]) return map[k];
  if (/^F\d{1,2}$/i.test(k)) return k.toUpperCase();
  if (/^[a-z]$/i.test(k)) return k.toUpperCase();
  if (/^\d$/.test(k)) return k;
  if (e.code && /^Numpad/.test(e.code)) return e.code.replace('Numpad', 'NUMPAD').toUpperCase();
  return String(k).toUpperCase().replace(/\s+/g, '_');
}

function beginCapture(field, btn) {
  if (S.capture && S.capture.field === field) { cancelCapture(); return; }
  $$('.cap-btn.is-armed').forEach((b) => b.classList.remove('is-armed'));
  S.capture = { field };
  if (btn) btn.classList.add('is-armed');
  toast('请按一个实体键（Esc 取消）');
}
function cancelCapture() {
  S.capture = null;
  $$('.cap-btn.is-armed').forEach((b) => b.classList.remove('is-armed'));
  toast('已取消捕获');
}
function appendSeq(text, step) {
  const t = String(text || '').trim();
  return t ? `${t} » ${step}` : step;
}
async function applyCaptured(name) {
  const cap = S.capture;
  if (!cap) return;
  S.capture = null;
  $$('.cap-btn.is-armed').forEach((b) => b.classList.remove('is-armed'));

  if (cap.field === '__seq_step__') {
    const t = $('[data-mfield="sequence_text"]');
    if (t) { t.value = appendSeq(t.value, `${name},80`); t.dispatchEvent(new Event('change', { bubbles: true })); }
    toast(`已加入按键步 ${name}`);
    return;
  }
  if (cap.field === 'preset_switch_key') {
    const sel = $('[data-pm-select]');
    const ps = [...(S.cfg.presets || [])];
    const i = Number(sel?.value ?? 0);
    if (!ps[i]) { toast('先选一个预设', 'err'); return; }
    ps[i] = { ...ps[i], switch_key: name };
    if (await saveConfig({ presets: ps }, { quiet: false })) { renderWork(false); toast(`「${ps[i].name}」的切换键已设为 ${name}`); }
    return;
  }
  const input = $(`[data-mfield="${cap.field}"]`) || $(`[data-field="${cap.field}"]`);
  if (!input) { toast('找不到要填的输入框', 'err'); return; }
  const multi = cap.field === 'target_keys' || cap.field === 'release_targets';
  input.value = multi && input.value ? `${input.value}+${name}` : name;
  input.dispatchEvent(new Event('change', { bubbles: true }));
  toast(`已捕获 ${name}`);
}

/* ---------- 键盘图批量连发 ---------- */

const sameNameMapping = (name) =>
  (S.cfg?.mappings || []).findIndex((m) =>
    String(m.trigger_key || '').toUpperCase() === name &&
    (m.target_keys || []).length === 1 &&
    String(m.target_keys[0]).toUpperCase() === name &&
    m.turbo_enabled);

function resetKb() { S.kbAdd = []; S.kbRemove = []; S.kbConflicts = []; }

function kbPick(id) {
  const name = String(id || '').toUpperCase();
  if (!name) return;
  S.kbAdd = S.kbAdd || []; S.kbRemove = S.kbRemove || [];
  if (sameNameMapping(name) >= 0) {
    const at = S.kbRemove.indexOf(name);
    if (at >= 0) S.kbRemove.splice(at, 1); else S.kbRemove.push(name);
  } else {
    const at = S.kbAdd.indexOf(name);
    if (at >= 0) S.kbAdd.splice(at, 1); else S.kbAdd.push(name);
  }
  renderWork(false);
}

function newSameName(name) {
  return {
    trigger_key: name, target_keys: [name], turbo_enabled: true,
    lock_enabled: false, double_tap_enabled: false, run_enabled: false,
    interval: null, event_duration: null, move_speed: 5, run_threshold: 80,
    double_tap_gap_ms: 40, run_recheck: true, sequence_text: '',
    release_targets: [], note: '键盘快捷连发',
  };
}

async function kbApply(mode = 'normal') {
  if (mode === 'cancel') { resetKb(); renderWork(false); toast('已撤销全部暂存'); return; }
  let add = [...(S.kbAdd || [])];
  const drop = [...(S.kbRemove || [])];
  const conflicts = add.filter((n) =>
    (S.cfg?.mappings || []).some((m) => String(m.trigger_key || '').toUpperCase() === n) &&
    sameNameMapping(n) < 0);
  if (mode === 'normal' && conflicts.length) {
    S.kbConflicts = conflicts;
    renderWork(false);
    toast(`${conflicts.length} 个键已被别的映射占用，选一下怎么处理`, 'err');
    return;
  }
  if (mode === 'skip') add = add.filter((n) => !conflicts.includes(n));

  const list = [...(S.cfg?.mappings || [])];
  for (const n of drop) {
    const i = sameNameMapping(n);
    if (i >= 0) list.splice(i, 1);
  }
  if (mode === 'overwrite') {
    for (const n of conflicts) {
      const i = list.findIndex((m) => String(m.trigger_key || '').toUpperCase() === n);
      if (i >= 0) list[i] = { ...list[i], target_keys: [n], turbo_enabled: true, sequence_text: '' };
    }
    add = add.filter((n) => !conflicts.includes(n));
  }
  for (const n of add) list.push(newSameName(n));
  if (await saveMappings(list)) {
    const a = add.length, d = drop.length;
    resetKb();
    renderWork(false);
    toast(`批量完成：新建 ${a} 条，移除 ${d} 条`);
  }
}

/* ---------- 预设管理 ---------- */

async function presetOp(op, btn) {
  const ps = [...(S.cfg?.presets || [])];
  const sel = $('[data-pm-select]');
  const i = Number(sel?.value ?? 0);
  const typed = ($('[data-pm-name]')?.value || '').trim();
  const cur = S.cfg?.current_preset || '';

  if (op === 'apply') {
    const p = ps[i];
    if (!p) return toast('还没有预设', 'err');
    if (!confirm(`套用预设「${p.name}」？当前的映射会被它覆盖。`)) return;
    if (await saveConfig({ mappings: p.mappings, current_preset: p.name }, { quiet: false })) renderWork(false);
    return;
  }
  if (op === 'save') {
    const name = typed || cur || `预设 ${ps.length + 1}`;
    const old = ps.findIndex((p) => p.name === name);
    const next = { name, mappings: [...(S.cfg?.mappings || [])], switch_key: old >= 0 ? ps[old].switch_key : '' };
    if (old >= 0) ps[old] = next; else ps.push(next);
    if (await saveConfig({ presets: ps, current_preset: name }, { quiet: false })) {
      renderWork(false); toast(old >= 0 ? `已覆盖「${name}」` : `已保存「${name}」`);
    }
    return;
  }
  if (op === 'rename') {
    if (!typed) return toast('先在名称框里写新名字', 'err');
    if (!ps[i]) return toast('先选一个预设', 'err');
    const from = ps[i].name;
    ps[i] = { ...ps[i], name: typed };
    const patch = { presets: ps };
    if (cur === from) patch.current_preset = typed;
    if (await saveConfig(patch, { quiet: false })) { renderWork(false); toast(`已改名为「${typed}」`); }
    return;
  }
  if (op === 'delete') {
    if (!ps[i]) return toast('还没有预设', 'err');
    if (btn.dataset.armed !== '1') {
      btn.dataset.armed = '1';
      btn.textContent = '⚠ 再点一次确认删除';
      return toast('再点一次才真的删除');
    }
    const name = ps[i].name;
    ps.splice(i, 1);
    if (await saveConfig({ presets: ps, current_preset: cur === name ? '' : cur }, { quiet: false })) {
      renderWork(false); toast(`已删除「${name}」`);
    }
    return;
  }
  if (op === 'unbind') {
    if (!ps[i]) return toast('先选一个预设', 'err');
    ps[i] = { ...ps[i], switch_key: '' };
    if (await saveConfig({ presets: ps }, { quiet: false })) { renderWork(false); toast('切换键已清除'); }
  }
}

/* ---------- 序列宏 ---------- */

function seqOp(op, btn) {
  const t = $('[data-mfield="sequence_text"]');
  const set = (v) => { if (t) { t.value = v; t.dispatchEvent(new Event('change', { bubbles: true })); } };
  if (op === 'clear') { set(''); toast('序列已清空，回到普通发送键'); return; }
  if (op === '20ms') { set(String(t?.value || '').replace(/,\d+/g, ',20')); toast('所有步骤时长已统一为 20ms'); return; }
  if (op === 'wait') { set(appendSeq(t?.value, 'NONE,200')); toast('已加一个等待步'); return; }
  if (op === 'step') { beginCapture('__seq_step__', btn); return; }
  if (op === 'macro') { S.seqShowMacros = !S.seqShowMacros; renderWork(false); return; }
  if (op === 'record') {
    if (!S.seqRecording) {
      S.seqRecording = true; S.seqKeys = [];
      renderWork(false); toast('开始录制：打一套连招，再点「停止并填入」');
    } else {
      S.seqRecording = false;
      const keys = S.seqKeys.slice();
      if (!keys.length) { renderWork(false); return toast('没录到按键', 'err'); }
      const parts = [];
      keys.forEach((k, n) => {
        const dt = n + 1 < keys.length ? keys[n + 1].t - k.t : 80;
        parts.push(k.name, String(Math.max(20, Math.min(1000, Math.round(dt)))));
      });
      set(parts.join(','));
      renderWork(false); toast(`录到 ${keys.length} 个按键`);
    }
  }
}

/* ---------- 组合键 ---------- */

async function chordOp(op, btn) {
  if (op === 'add') return act('add-chord', btn);
  const list = [...(S.cfg?.mappings || [])];
  if (op === 'edit') { S.sel = Number(btn.dataset.i); S.draft = null; renderWork(false); return; }
  if (op === 'del') {
    list.splice(Number(btn.dataset.i), 1);
    if (await saveMappings(list)) { S.sel = -1; renderWork(false); toast('组合键已删除'); }
    return;
  }
  if (op === 'purge' || op === 'purge-cancel') {
    if (op === 'purge-cancel') { S.chordPurgeArmed = false; renderWork(false); return; }
    if (!S.chordPurgeArmed) {
      S.chordPurgeArmed = true;
      btn.textContent = '⚠ 再点一次确认批量删除';
      return toast('再点一次才真的删除筛选结果');
    }
    const q = String(S.chordQuery || '').toLowerCase();
    const keep = list.filter((m) => !(
      String(m.trigger_key || '').includes('+') &&
      `${m.trigger_key} ${(m.target_keys || []).join('+')} ${m.note || ''}`.toLowerCase().includes(q)));
    S.chordPurgeArmed = false;
    if (await saveMappings(keep)) { renderWork(false); toast('筛选结果已删除'); }
    return;
  }
}

async function toggleExclusive(token, btn) {
  const list = [...(S.cfg?.chord_exclusive_keys || [])];
  const at = list.indexOf(token);
  const on = at < 0;
  if (on) list.push(token); else list.splice(at, 1);
  if (await saveConfig({ chord_exclusive_keys: list }, { quiet: false })) {
    btn.setAttribute('aria-pressed', String(on));
    toast(on ? `${token} 已设为独占成员键` : `${token} 已取消独占`);
  }
}

/* ---------- 手柄逐步校对向导 ---------- */

function wizOp(op) {
  const total = padWizardTotal();
  if (op === 'reset') { S.wizStep = 0; S.wizDone = []; renderWork(false); return; }
  if (op === 'stop') { S.wizStep = -1; renderWork(false); toast('校对已结束，已核对的键都留着'); return; }
  if (op === 'start') {
    // 「继续核对」要从第一个没核对过的槽位接着走，不是每次从头来
    const done = S.wizDone || [];
    let from = 0;
    for (let k = 0; k < total; k++) {
      const sl = padWizardSlot(k);
      if (sl && !done.includes(sl[0])) { from = k; break; }
    }
    S.wizStep = from;
    renderWork(false);
    return;
  }
  const i = S.wizStep;
  const slot = padWizardSlot(i);
  if (!slot || i < 0 || i >= total) return;
  if (op === 'use') {
    const token = slot[0];
    if (!S.wizDone.includes(token)) S.wizDone.push(token);
    const hot = $(`[data-slot="${token}"]`);
    if (hot) hot.dispatchEvent(new MouseEvent('click', { bubbles: true }));
    else toast('图上找不到这个键', 'err');
  }
  S.wizStep = Math.min(total, i + 1);
  renderWork(false);
  if (S.wizStep >= total) toast('全部槽位都过了一遍');
}

/* ---------- 震动 / 音频：测试、预设、导入导出 ---------- */

const VIB_DEFAULT = {
  enabled: true, master_gain: 90, max_strength: 90, attack_gain: 70,
  damage_gain: 70, shake_gain: 60, move_gain: 40, decay_ms: 180,
  motor_l_gain: 100, motor_r_gain: 85, hit_boost: 20, start_pulse: 30, kill_pulse: 40,
};
const AUDIO_DEFAULT = {
  enabled: true, master_gain_pct: 85, low_gain_pct: 70, high_gain_pct: 45,
  punch_pct: 60, mask_threshold: 40, rhythm_pct: 50, texture_pct: 40,
  attack_pct: 65, release_pct: 35, dead_zone_pct: 12, music_mask_pct: 55,
  smart_strength_pct: 60, smart_duration_pct: 55,
};

async function vibTool(op, btn) {
  if (op === 'test' || op === 'rank') {
    await api.action(op === 'rank' ? 'test-rank' : 'test-vibration').catch(() => {});
    toast(op === 'rank' ? '已触发评分震动' : '试震 1 秒');
    return;
  }
  if (op === 'reset') {
    if (await saveVibration(VIB_DEFAULT)) { renderWork(false); toast('震动参数已还原默认'); }
    return;
  }
  if (op === 'import') { $('[data-file="vib"]')?.click(); return; }
  if (op === 'export') { downloadJson(`sorahk-vibration-${Date.now()}.json`, { kind: 'vibration', params: S.vib }); return; }
  const list = loadVibPresets();
  const sel = $('[data-vibpreset-sel]');
  const i = Number(sel?.value ?? 0);
  const typed = ($('[data-vibpreset-name]')?.value || '').trim();
  if (op === 'save') {
    if (!typed) return toast('先给这套手感起个名字', 'err');
    const at = list.findIndex((p) => p.name === typed);
    const next = { name: typed, params: { ...S.vib } };
    if (at >= 0) list[at] = next; else list.push(next);
    storeVibPresets(list); renderWork(false); toast(`已存「${typed}」`);
    return;
  }
  if (!list[i]) return toast('还没有保存过预设', 'err');
  if (op === 'apply') {
    if (await saveVibration(list[i].params)) { renderWork(false); toast(`已套用「${list[i].name}」`); }
    return;
  }
  if (op === 'del') {
    if (btn.dataset.armed !== '1') { btn.dataset.armed = '1'; btn.textContent = '⚠ 再点一次'; return; }
    list.splice(i, 1); storeVibPresets(list); renderWork(false); toast('已删除');
  }
}

async function audioTool(op, btn) {
  if (op === 'self' || op === 'calib') {
    await api.action(op === 'self' ? 'audio-self-check' : 'audio-calibrate').catch(() => {});
    toast(op === 'self' ? '马达自检已触发' : '保持安静 3 秒，正在测环境噪音');
    return;
  }
  if (op === 'calib-apply') {
    const v = Number(S.status?.audio_calib || 0);
    if (await saveConfig({ audio_haptics: { mask_threshold: v } })) {
      await api.action('audio-calib-dismiss').catch(() => {});
      S.status.audio_calib = 0; renderWork(false); toast(`屏蔽门槛已设为 ${v}`);
    }
    return;
  }
  if (op === 'calib-ignore') {
    await api.action('audio-calib-dismiss').catch(() => {});
    S.status.audio_calib = 0; renderWork(false); return;
  }
  if (op === 'reset') {
    if (await saveConfig({ audio_haptics: AUDIO_DEFAULT })) { renderWork(false); toast('音频参数已还原默认'); }
    return;
  }
  if (op === 'import') { $('[data-file="audio"]')?.click(); return; }
  if (op === 'export') { downloadJson(`sorahk-audio-${Date.now()}.json`, { kind: 'audio', params: S.cfg.audio_haptics }); return; }
  const a = { ...(S.cfg?.audio_haptics || {}) };
  delete a.presets;
  const ps = [...(S.cfg?.audio_haptics?.presets || [])];
  const sel = $('[data-audiopreset-sel]');
  const i = Number(sel?.value ?? 0);
  const typed = ($('[data-audiopreset-name]')?.value || '').trim();
  if (op === 'save') {
    if (!typed) return toast('先给这套起个名字', 'err');
    const next = { ...a, name: typed, user_modified: true };
    const at = ps.findIndex((p) => p.name === typed);
    if (at >= 0) ps[at] = next; else ps.push(next);
    if (await saveConfig({ audio_haptics: { presets: ps } }, { quiet: false })) { renderWork(false); toast(`已存「${typed}」`); }
    return;
  }
  if (!ps[i]) return toast('还没有保存过预设', 'err');
  if (op === 'apply') {
    const p = { ...ps[i] }; delete p.name; delete p.user_modified; delete p.presets;
    if (await saveConfig({ audio_haptics: p })) { renderWork(false); toast(`已套用「${ps[i].name}」`); }
    return;
  }
  if (op === 'del') {
    if (btn.dataset.armed !== '1') { btn.dataset.armed = '1'; btn.textContent = '⚠ 再点一次'; return; }
    ps.splice(i, 1);
    if (await saveConfig({ audio_haptics: { presets: ps } }, { quiet: false })) { renderWork(false); toast('已删除'); }
  }
}

async function macroOp(op, btn) {
  const list = [...(S.cfg?.universal_macros || [])];
  if (btn.dataset.macroDel !== undefined) {
    list.splice(Number(btn.dataset.macroDel), 1);
    if (await saveConfig({ universal_macros: list }, { quiet: false })) { renderWork(false); toast('宏已删除'); }
    return;
  }
  const name = ($('[data-macro-name]')?.value || '').trim();
  const text = ($('[data-macro-text]')?.value || '').trim();
  if (!name || !text) return toast('名字和序列都要填', 'err');
  const at = list.findIndex((m) => m.name.toLowerCase() === name.toLowerCase());
  const next = { name, text };
  if (at >= 0) list[at] = next; else list.push(next);
  if (await saveConfig({ universal_macros: list }, { quiet: false })) { renderWork(false); toast(`宏「${name}」已保存`); }
}

async function devOp(op) {
  const map = { refresh: 'refresh-devices', reset: 'reset-devices', test: 'test-vibration', restart: 'restart-threads' };
  await api.action(map[op] || op).catch(() => {});
  toast(({ refresh: '设备列表已刷新', reset: '手柄连接已重置', test: '试震已触发', restart: '后台功能已重启' })[op] || '已完成');
}

/* ------------------------------------------------------------------ 事件 */

async function act(name, btn) {
  switch (name) {
    case 'toggle-pause': {
      const r = await api.action('toggle-pause');
      toast(r.paused ? '已暂停：手柄按键不再发送' : '已恢复');
      break;
    }
    case 'save': {
      const res = await api.patchConfig({});
      mergeCfg(res.config);
      toast('已写入 Config.toml');
      break;
    }
    case 'quit': {
      if (!confirm('退出后游戏里不再收到模拟按键，确定退出？')) return;
      await api.action('quit');
      document.body.innerHTML = '<p style="font-family:sans-serif;padding:2rem">程序正在退出，可以关掉这个页面。</p>';
      break;
    }
    case 'check-update': {
      btn?.setAttribute('aria-busy', 'true');
      try {
        const r = await api.action('check-update');
        toast(r.message || '正在检查更新…');
        setTimeout(async () => {
          try {
            const st = await api.state();
            S.status = st.status;
            renderPanels();
          } catch {
            /* 忽略一次失败 */
          }
        }, 4000);
      } catch (e) {
        toast(e.message, 'err');
      } finally {
        btn?.removeAttribute('aria-busy');
      }
      break;
    }
    case 'reload-logs': {
      const r = await api.logs(300);
      S.logs = r.lines || [];
      renderWork(false);
      toast(`读到 ${S.logs.length} 行`);
      break;
    }
    case 'wl-add': {
      const v = ($('#wl-add')?.value || '').trim();
      if (!v) return toast('先写一个进程名', 'err');
      const list = [...(S.cfg.process_whitelist || [])];
      if (list.includes(v)) return toast('名单里已经有这个进程了', 'err');
      list.push(v);
      if (await saveConfig({ process_whitelist: list }, { quiet: false })) renderWork(false);
      break;
    }
    case 'wl-del': {
      const list = (S.cfg.process_whitelist || []).filter((x) => x !== btn.dataset.p);
      if (await saveConfig({ process_whitelist: list }, { quiet: false })) renderWork(false);
      break;
    }
    case 'new-mapping': {
      S.sel = -1;
      S.draft = {
        trigger_key: '',
        target_keys: [],
        turbo_enabled: false,
        lock_enabled: false,
        double_tap_enabled: false,
        run_enabled: false,
        interval: null,
        event_duration: null,
        note: '',
      };
      renderWork(false);
      break;
    }
    case 'mapping-save': {
      const d = S.sel >= 0 ? S.cfg.mappings[S.sel] : S.draft;
      if (!d) return;
      if (!d.trigger_key || !(d.target_keys || []).length) return toast('触发键和发送键都要填', 'err');
      const list = [...(S.cfg.mappings || [])];
      if (S.sel >= 0) list[S.sel] = { ...list[S.sel], ...d };
      else list.push({ ...d });
      if (await saveMappings(list)) {
        S.draft = null;
        renderWork(false);
      }
      break;
    }
    case 'mapping-del': {
      if (!confirm('删除这条映射？游戏里这个键就不再有功能。')) return;
      const i = Number(btn.dataset.i);
      const list = (S.cfg.mappings || []).filter((_, k) => k !== i);
      if (await saveMappings(list)) {
        S.sel = -1;
        renderWork(false);
      }
      break;
    }
    case 'mapping-cancel': {
      S.draft = null;
      S.sel = -1;
      renderWork(false);
      break;
    }
    case 'set-mode': {
      await setMode(btn.dataset.mode);
      break;
    }
    case 'show-guide': {
      S.guideStep = 0;
      showGuide();
      break;
    }
    case 'mini-toggle': {
      const r = await api.action('toggle-pause');
      blip(r.paused ? 'off' : 'on');
      toast(r.paused ? '已暂停：手柄按键不再发送' : '已恢复');
      renderWork(false);
      break;
    }
    case 'pad-focus': {
      S.padFocus = true;
      S.sel = -1;
      S.draft = null;
      renderWork(false);
      toast('点手柄图上要设的那个键');
      break;
    }
    case 'add-chord': {
      S.padFocus = false;
      S.sel = -1;
      S.draft = {
        trigger_key: 'GAMEPAD_045E_RB+X',
        target_keys: ['SPACE'],
        turbo_enabled: false,
        lock_enabled: false,
        double_tap_enabled: false,
        run_enabled: false,
        interval: null,
        event_duration: null,
        note: '',
      };
      renderWork(false);
      toast('组合键已填好模板：改键名就行');
      break;
    }
    case 'toggle-vibration': {
      const on = !S.vib?.enabled;
      if (await saveVibration({ enabled: on })) toast(on ? '震动已开启' : '震动已关闭');
      break;
    }
    case 'turbo-off': {
      const list = [...(S.cfg.mappings || [])];
      const i = Number(btn.dataset.i);
      list[i] = { ...list[i], turbo_enabled: false };
      if (await saveMappings(list)) renderWork(false);
      break;
    }
    case 'apply-preset': {
      const p = (S.cfg.presets || [])[Number(btn.dataset.i)];
      if (!p) return;
      if (!confirm(`套用预设「${p.name}」？当前的映射会被它覆盖。`)) return;
      if (await saveConfig({ mappings: p.mappings, current_preset: p.name }, { quiet: false })) renderWork(false);
      break;
    }
    case 'edit-mapping': {
      S.sel = Number(btn.dataset.i);
      S.draft = null;
      renderWork(false);
      break;
    }
    default:
      break;
  }
}

function wire() {
  document.addEventListener('click', async (e) => {
    const btn = e.target.closest('[data-act]');
    if (btn) {
      e.preventDefault();
      return act(btn.dataset.act, btn);
    }

    /* ── 便利性：捕获 / 预设 / 批量 / 序列宏 / 鼠标键 / 组合键 / 向导 / 测试 / 宏 / 设备 ── */
    const cap = e.target.closest('[data-cap]');
    if (cap) { beginCapture(cap.dataset.cap, cap); return; }

    const kb = e.target.closest('[data-kb]');
    if (kb) { await kbApply(kb.dataset.kb); return; }
    const kbg = e.target.closest('#kb-board .kb-key');
    if (kbg) { kbPick(kbg.id); return; }

    const pm = e.target.closest('[data-pm]');
    if (pm) { await presetOp(pm.dataset.pm, pm); return; }

    const seq = e.target.closest('[data-seq]');
    if (seq) { seqOp(seq.dataset.seq, seq); return; }
    const sm = e.target.closest('[data-seq-macro]');
    if (sm) {
      const t = $('[data-mfield="sequence_text"]');
      if (t) { t.value = appendSeq(t.value, `宏(${sm.dataset.seqMacro})`); t.dispatchEvent(new Event('change', { bubbles: true })); }
      return;
    }

    const mk = e.target.closest('[data-mouse]');
    if (mk) {
      const input = $('[data-mfield="target_keys"]');
      if (input) {
        input.value = input.value ? `${input.value}+${mk.dataset.mouse}` : mk.dataset.mouse;
        input.dispatchEvent(new Event('change', { bubbles: true }));
        toast(`已加入 ${mk.dataset.mouse}`);
      } else toast('先选中一条映射，再加鼠标键', 'err');
      return;
    }
    const mfclr = e.target.closest('[data-mf]');
    if (mfclr) {
      const input = $('[data-mfield="release_targets"]');
      if (input) { input.value = ''; input.dispatchEvent(new Event('change', { bubbles: true })); toast('抬起键已清空'); }
      return;
    }
    const mfdel = e.target.closest('[data-mfdel]');
    if (mfdel) {
      if (!S.delArmed) { S.delArmed = true; renderWork(false); toast('再点一次才真的删除'); }
      else { S.delArmed = false; await act('mapping-del', mfdel); }
      return;
    }

    const chord = e.target.closest('[data-chord]');
    if (chord) { await chordOp(chord.dataset.chord, chord); return; }
    const ex = e.target.closest('[data-chord-ex]');
    if (ex) { await toggleExclusive(ex.dataset.chordEx, ex); return; }

    const wiz = e.target.closest('[data-wiz]');
    if (wiz) { wizOp(wiz.dataset.wiz); return; }

    const vt = e.target.closest('[data-vibtool]');
    if (vt) { await vibTool(vt.dataset.vibtool, vt); return; }
    const atool = e.target.closest('[data-audiotool]');
    if (atool) { await audioTool(atool.dataset.audiotool, atool); return; }

    const mc = e.target.closest('[data-macro], [data-macro-del]');
    if (mc) { await macroOp('add', mc); return; }
    const dv = e.target.closest('[data-dev]');
    if (dv) { await devOp(dv.dataset.dev); return; }

    const wl = e.target.closest('[data-wl]');
    if (wl) {
      if (wl.dataset.wl === 'pick') $('[data-wl-file]')?.click();
      if (wl.dataset.wl === 'clear') {
        if (!confirm('清空整份白名单？之后所有程序都会收到模拟按键。')) return;
        if (await saveConfig({ process_whitelist: [] }, { quiet: false })) { renderWork(false); toast('白名单已清空'); }
      }
      return;
    }

    // 只认操作条里的模式按钮：body 自己也带 data-mode，用裸属性选择器
    // 会把页面里几乎所有点击都当成「切回完整模式」，其它交互全被吞掉。
    const modeBtn = e.target.closest('.mode-switch button[data-mode]');
    if (modeBtn) {
      e.preventDefault();
      await setMode(modeBtn.dataset.mode);
      return;
    }

    const gbtn = e.target.closest('[data-guide]');
    if (gbtn) {
      const what = gbtn.dataset.guide;
      if (what === 'next') {
        S.guideStep = Math.min(GUIDE_STEPS.length - 1, S.guideStep + 1);
        showGuide();
      } else if (what === 'done' || what === 'skip') {
        closeGuide();
      }
      return;
    }

    const vp = e.target.closest('[data-vibpatch]');
    if (vp) {
      const sets = {
        heavy: { master_gain: 100, attack_gain: 90, decay_ms: 120, max_strength: 100 },
        balanced: { master_gain: 90, attack_gain: 70, decay_ms: 180, max_strength: 90 },
        soft: { master_gain: 60, attack_gain: 40, decay_ms: 260, max_strength: 65 },
      };
      const patch = sets[vp.dataset.vibpatch];
      if (patch && (await saveVibration(patch))) {
        toast('手感已套用');
        renderWork(false);
      }
      return;
    }

    const wq = e.target.closest('[data-wl-quick]');
    if (wq) {
      const input = $('#wl-add');
      if (input) input.value = wq.dataset.wlQuick;
      const add = $('[data-act="wl-add"]');
      if (add) add.dispatchEvent(new MouseEvent('click', { bubbles: true }));
      return;
    }

    const presetBtn = e.target.closest('[data-preset]');
    if (presetBtn) {
      const i = Number(presetBtn.dataset.preset);
      const p = (S.cfg?.presets || [])[i];
      if (!p) return;
      if (await saveConfig({ mappings: p.mappings, current_preset: p.name }, { quiet: false })) {
        blip('on');
        renderWork(false);
      }
      return;
    }

    if (e.target.closest('[data-mini-audio]')) {
      const on = !S.cfg?.audio_haptics?.enabled;
      if (await saveConfig({ audio_haptics: { enabled: on } })) {
        blip(on ? 'on' : 'off');
        renderWork(false);
      }
      return;
    }

    const slot = e.target.closest('[data-slot]');
    if (slot) {
      const token = slot.dataset.slot;
      const list = S.cfg?.mappings || [];
      const hit = list.findIndex((m) => {
        const m2 = String(m.trigger_key || '').match(/^GAMEPAD_[0-9A-F]{4}_(.+)$/i);
        if (!m2 || /^[0-9A-F]{4}_/i.test(m2[1])) return false;
        return m2[1] === token || m2[1].endsWith('_' + token);
      });
      S.padFocus = false;
      if (hit >= 0) {
        S.sel = hit;
        S.draft = null;
      } else {
        S.sel = -1;
        S.draft = {
          trigger_key: `GAMEPAD_045E_${token}`,
          target_keys: [],
          turbo_enabled: false,
          lock_enabled: false,
          double_tap_enabled: false,
          run_enabled: false,
          interval: null,
          event_duration: null,
          note: '',
        };
      }
      renderWork(false);
      setTimeout(() => $('[data-mfield="target_keys"]')?.focus(), 40);
      return;
    }

    const chip = e.target.closest('[data-map]');
    if (chip) {
      S.padFocus = false;
      S.sel = Number(chip.dataset.map);
      S.draft = null;
      renderWork(false);
      return;
    }

    const job = e.target.closest('[data-job]');
    if (job) {
      const want = job.dataset.job === '1';
      if (await saveConfig({ minimal_vib_preset_job: want })) {
        toast(want ? '震动预设切到全职业' : '震动预设切到通用');
        renderWork(false);
      }
      return;
    }

    const row = e.target.closest('.tbl-row[data-row]');
    if (row && S.route === 'gamepad') {
      S.sel = Number(row.dataset.row);
      S.draft = null;
      renderWork(false);
      return;
    }

    const sw = e.target.closest('[data-toggle]');
    if (sw) {
      const path = sw.dataset.toggle;
      const vib = sw.dataset.vib === '1';
      const on = sw.getAttribute('aria-pressed') !== 'true';
      sw.setAttribute('aria-pressed', String(on));
      const label = sw.textContent.split(' · ')[0];
      sw.textContent = `${label} · ${on ? '已开启' : '已关闭'}`;
      if (vib) {
        S.vib[path] = on;
        await saveVibration(setPath({}, path, on));
      } else {
        setPath(S.cfg, path, on);
        await saveConfig(setPath({}, path, on));
        if (path === 'dark_mode') applyTheme(on);
      }
      return;
    }

    const mf = e.target.closest('[data-mflag]');
    if (mf) {
      const on = mf.getAttribute('aria-pressed') !== 'true';
      mf.setAttribute('aria-pressed', String(on));
      const d = S.sel >= 0 ? S.cfg.mappings[S.sel] : S.draft;
      if (d) d[mf.dataset.mflag] = on;
    }
  });

  document.addEventListener('input', (e) => {
    const cq = e.target.closest('[data-chord-search]');
    if (cq) {
      S.chordQuery = cq.value;
      S.chordPurgeArmed = false;
      renderWork(false);
      setTimeout(() => { const n = $('[data-chord-search]'); if (n) { n.focus(); n.setSelectionRange(n.value.length, n.value.length); } }, 20);
      return;
    }
    const q = e.target.closest('[data-map-search]');
    if (q) {
      const key = q.value.trim().toLowerCase();
      $$('.chip-wrap .map-chip, .tbl .tbl-row[data-row]').forEach((n) => {
        n.hidden = key ? !n.textContent.toLowerCase().includes(key) : false;
      });
      return;
    }
    const r = e.target.closest('[data-knob]');
    if (r) {
      const out = document.getElementById(`o-${r.id.slice(2)}`);
      if (out) out.textContent = `${r.value}${r.dataset.unit || ''}`;
      scheduleSave(r.dataset.knob, Number(r.value), r.dataset.vib === '1');
    }
  });

  document.addEventListener('change', async (e) => {
    const ff = e.target.closest('[data-file]');
    if (ff && ff.files && ff.files[0]) {
      try {
        const obj = await readJson(ff.files[0]);
        if (ff.dataset.file === 'vib' && obj.params && await saveVibration(obj.params)) { renderWork(false); toast('震动设置已导入'); }
        if (ff.dataset.file === 'audio' && obj.params && await saveConfig({ audio_haptics: obj.params })) { renderWork(false); toast('音频设置已导入'); }
      } catch (err) { toast('导入失败：' + err.message, 'err'); }
      ff.value = '';
      return;
    }
    const wlf = e.target.closest('[data-wl-file]');
    if (wlf && wlf.files && wlf.files[0]) {
      const input = $('#wl-add');
      if (input) input.value = wlf.files[0].name;
      toast('浏览器只给得出文件名；要按完整路径过滤，请把路径粘进输入框');
      wlf.value = '';
      return;
    }
    const f = e.target.closest('[data-field]');
    if (f) {
      const path = f.dataset.field;
      const vib = f.dataset.vib === '1';
      let v = f.value;
      if (f.type === 'number') v = v === '' ? 0 : Number(v);
      if (vib) {
        S.vib = { ...S.vib };
        setPath(S.vib, path, v);
        await saveVibration(setPath({}, path, v));
      } else {
        setPath(S.cfg, path, v);
        await saveConfig(setPath({}, path, v));
        if (path === 'language' || path === 'dark_mode') toast('已保存');
      }
      return;
    }
    const ps = e.target.closest('[data-preset-select]');
    if (ps) {
      const i = Number(ps.value);
      const p = (S.cfg?.presets || [])[i];
      if (!p) return;
      if (!confirm(`套用预设「${p.name}」？当前的映射会被它覆盖。`)) {
        renderWork(false);
        return;
      }
      if (await saveConfig({ mappings: p.mappings, current_preset: p.name }, { quiet: false })) {
        renderWork(false);
        toast(`已切到「${p.name}」`);
      }
      return;
    }

    const m = e.target.closest('[data-mfield]');
    if (m) {
      const d = S.sel >= 0 ? S.cfg.mappings[S.sel] : S.draft;
      if (!d) return;
      const k = m.dataset.mfield;
      let v = m.value;
      if (k === 'target_keys' || k === 'release_targets') v = v.split(/[+,]/).map((x) => x.trim()).filter(Boolean);
      else if (['interval', 'event_duration', 'move_speed', 'run_threshold', 'double_tap_gap_ms'].includes(k)) v = v === '' ? null : Number(v);
      else if (k === 'sequence_text') v = String(v);
      d[k] = v;
    }
  });

  window.addEventListener('hashchange', () => {
    S.draft = null;
    S.padFocus = false;
    renderNav();
    renderWork(true);
  });

  window.addEventListener('resize', () => {
    fitStage();
    sizeDial();
  });

  $('#btn-theme').addEventListener('click', async () => {
    const dark = document.documentElement.dataset.theme !== 'dark';
    applyTheme(dark);
    setPath(S.cfg, 'dark_mode', dark);
    await saveConfig({ dark_mode: dark });
  });
  $('#btn-pause').addEventListener('click', () => act('toggle-pause'));
  $('#btn-quit').addEventListener('click', () => act('quit'));

  document.addEventListener('keydown', (e) => {
    const typing = e.target instanceof Element && e.target.matches('input, select, textarea');

    // 捕获态最优先：连输入框里打字也算「按实体键」，Esc 取消。
    if (S.capture) {
      e.preventDefault();
      if (e.key === 'Escape') { cancelCapture(); return; }
      if (['Control', 'Shift', 'Alt', 'Meta'].includes(e.key)) return;
      const mods = [];
      if (e.ctrlKey) mods.push('CTRL');
      if (e.shiftKey) mods.push('SHIFT');
      if (e.altKey) mods.push('ALT');
      applyCaptured([...mods, keyNameFromEvent(e)].join('+'));
      return;
    }
    if ((e.ctrlKey || e.metaKey) && String(e.key).toLowerCase() === 's') {
      e.preventDefault(); act('save'); return;
    }
    if (S.seqRecording) {
      if (e.key === 'Escape') { S.seqRecording = false; renderWork(false); return; }
      if (!['Control', 'Shift', 'Alt', 'Meta'].includes(e.key)) {
        S.seqKeys.push({ name: keyNameFromEvent(e), t: performance.now() });
      }
      e.preventDefault();
      return;
    }
    if (typing) return;
    if (e.key === 'Escape') {
      S.sel = -1; S.draft = null; S.delArmed = false;
      renderWork(false);
      return;
    }
    const order = routeOrder();
    if (/^[1-8]$/.test(e.key)) { go(order[Number(e.key) - 1]); return; }
    if (e.key === 'Delete') act('toggle-pause');
  });
}

/* ------------------------------------------------------------------ 启动 */

async function boot() {
  fitStage();
  wire();
  try {
    const data = await api.bootstrap();
    S.version = data.version;
    S.status = data.status;
    S.status.counts = data.status.counts || countOf(data.config);
    S.cfg = data.config;
    S.vib = data.vibration;
    S.vibPresets = loadVibPresets();
    S.logs = data.logs || [];
    applyTheme(!!data.config.dark_mode);
    S.mode = modeOf(data.config);
    S.route = currentRoute();
    document.body.dataset.route = S.route;
    applyMode();
    renderNav();
    renderDial();
    // 先渲染工作区：pages.js 的模块级状态是在 render() 里设置的，
    // renderPanels 会用到它画「映射速览」，顺序反了会直接抛空指针。
    renderWork(true);
    renderPanels();
    // 调试/演示用：?mode=minimal|turbo|full 直接看对应界面
    const qs = new URLSearchParams(location.search || location.hash.split('?')[1] || '');
    const forced = qs.get('mode');
    if (forced && ['full', 'minimal', 'turbo'].includes(forced)) {
      S.mode = forced;
      applyMode();
      renderWork(false);
    }
    if (qs.get('guide') === '1') {
      S.guideStep = 0;
      showGuide();
    }
    if (!data.config.guide_seen) {
      S.guideStep = 0;
      showGuide();
    }
  } catch (e) {
    $('#work').innerHTML = `<p class="ef-notice" data-flag="离线">连不上本地服务：${esc(
      e.message,
    )}。请确认程序还在运行。</p>`;
    return;
  }

  streamStatus((st) => {
    S.status = st.status;
    if (st.status?.counts) S.status.counts = st.status.counts;
    if (st.vibration) S.vib = st.vibration;
    renderPanels();
    if (dial) dial.hub(hubState());
  });
}

boot();
