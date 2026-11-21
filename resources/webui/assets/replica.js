/* ============================================================
   replica.js — 原版 1:1 复刻的交互层（独立于 proto.js，不 import）
   v11: 演示数据开关(默认关) / 浅色主题 / 三步捕获流 / 设备+HID 弹窗
        / 设置弹窗逐行 / 高级区 62+评分10+LR / 音频全卡 / 全职业 14 类
   画布 1180×760（与原版截图同尺寸），fitStage 等比缩放
   ============================================================ */
'use strict';

/* ---------------- mock 数据（默认 = 原版截图空态; 演示开关才填充） ---------------- */
/* 24 槽位表 = SorahkDFO源码/src/gui/gamepad_mapping/model.rs SLOTS（坐标/半径逐字对应） */
var SLOTS = [
  { id: 0,  label: 'LT',        short: 'LT',   x: 0.2826, y: 0.0923, r: 0.038,  kind: 'trigger' },
  { id: 1,  label: 'LB',        short: 'LB',   x: 0.3963, y: 0.1172, r: 0.038,  kind: 'shoulder' },
  { id: 2,  label: 'RT',        short: 'RT',   x: 0.7376, y: 0.0923, r: 0.038,  kind: 'trigger' },
  { id: 3,  label: 'RB',        short: 'RB',   x: 0.6239, y: 0.1172, r: 0.038,  kind: 'shoulder' },
  { id: 4,  label: '十字键·上',  short: '↑',    x: 0.3802, y: 0.4678, r: 0.0284, kind: 'dpad' },
  { id: 5,  label: '十字键·下',  short: '↓',    x: 0.3802, y: 0.5856, r: 0.0284, kind: 'dpad' },
  { id: 6,  label: '十字键·左',  short: '←',    x: 0.3325, y: 0.5267, r: 0.0284, kind: 'dpad' },
  { id: 7,  label: '十字键·右',  short: '→',    x: 0.4279, y: 0.5267, r: 0.0284, kind: 'dpad' },
  { id: 8,  label: '左摇杆·上',  short: '↑',    x: 0.2492, y: 0.2611, r: 0.0251, kind: 'stick' },
  { id: 9,  label: '左摇杆·下',  short: '↓',    x: 0.2492, y: 0.3709, r: 0.0251, kind: 'stick' },
  { id: 10, label: '左摇杆·左',  short: '←',    x: 0.2088, y: 0.3160, r: 0.0251, kind: 'stick' },
  { id: 11, label: '左摇杆·右',  short: '→',    x: 0.2896, y: 0.3160, r: 0.0251, kind: 'stick' },
  { id: 12, label: '右摇杆·上',  short: '↑',    x: 0.6283, y: 0.4696, r: 0.0251, kind: 'stick' },
  { id: 13, label: '右摇杆·下',  short: '↓',    x: 0.6283, y: 0.5644, r: 0.0251, kind: 'stick' },
  { id: 14, label: '右摇杆·左',  short: '←',    x: 0.5934, y: 0.5170, r: 0.0251, kind: 'stick' },
  { id: 15, label: '右摇杆·右',  short: '→',    x: 0.6632, y: 0.5170, r: 0.0251, kind: 'stick' },
  { id: 16, label: 'X 键',       short: 'X',    x: 0.6833, y: 0.3155, r: 0.036,  kind: 'button' },
  { id: 17, label: 'Y 键',       short: 'Y',    x: 0.7516, y: 0.2239, r: 0.036,  kind: 'button' },
  { id: 18, label: 'B 键',       short: 'B',    x: 0.8220, y: 0.3155, r: 0.036,  kind: 'button' },
  { id: 19, label: 'A 键',       short: 'A',    x: 0.7516, y: 0.4067, r: 0.036,  kind: 'button' },
  { id: 20, label: 'Back',       short: 'Back', x: 0.4284, y: 0.3145, r: 0.0257, kind: 'system' },
  { id: 21, label: 'Start',      short: 'Start',x: 0.5723, y: 0.3145, r: 0.0257, kind: 'system' },
  { id: 22, label: '左摇杆·按下', short: '',     x: 0.2492, y: 0.3160, r: 0.020,  kind: 'stickclick' },
  { id: 23, label: '右摇杆·按下', short: '',     x: 0.6283, y: 0.5170, r: 0.020,  kind: 'stickclick' },
];
/* 热点基础色 = model.rs SlotKind::base_rgb */
var KIND_RGB = {
  trigger: [235, 146, 52], shoulder: [175, 122, 224], dpad: [72, 170, 235],
  stick: [80, 200, 130], stickclick: [235, 200, 80], button: [124, 140, 248], system: [150, 165, 180],
};
var KIND_LEGEND = {
  trigger: '扳机', shoulder: '肩键', dpad: '十字键', stick: '摇杆方向',
  stickclick: '摇杆按下', button: 'ABXY 面键', system: '系统键',
};
/* 演示数据（「原型演示」开关打开才填充; 数值与终末地 mod 包一致: 19/24 槽位） */
var DEMO_PAD_TARGETS = {
  LT: 'LSHIFT', LB: 'LCTRL', RT: 'J', RB: 'K',
  'DPad_Up': 'UP', 'DPad_Down': 'DOWN', 'DPad_Left': 'LEFT', 'DPad_Right': 'RIGHT',
  'LS_Up': 'W', 'LS_Down': 'S', 'LS_Left': 'A', 'LS_Right': 'D', 'RS_Up': 'NUMPAD4',
  X: 'U', Y: 'I', B: 'O', A: 'SPACE', Back: 'ESC', Start: 'ENTER',
};
/* 默认态: 空（= 原版截图空态; 页面交互产生的映射照常写入） */
var PAD_TARGETS = {};
/* 连发: 默认 1 条占位（原版截图「映射条目 1」, 该条触发键不在可见键盘行上故键盘无亮键） */
var TURBO_BASE_ROW = { trig: 'NUMPAD0', tgt: 'NUMPAD0', ms: '45ms', flags: ['连发'], note: '' };
var DEMO_TURBO_ROWS = [
  { trig: 'J', tgt: 'J', ms: '45ms', flags: ['连发'], note: '普通攻击' },
  { trig: 'K', tgt: 'K', ms: '60ms', flags: ['连发'], note: '技能1' },
  { trig: 'L', tgt: 'L', ms: '45ms', flags: ['连发'], note: '技能2' },
  { trig: 'U', tgt: 'U', ms: '80ms', flags: ['简易奔跑'], note: '' },
  { trig: 'NUMPAD1', tgt: '1', ms: '100ms', flags: ['连发'], note: '药剂' },
  { trig: 'F1', tgt: 'J', ms: '45ms', flags: ['连发', '锁定'], note: 'buff 连点，按一下锁住一直按' },
];
var TURBO_ROWS = [TURBO_BASE_ROW];
/* 白名单: 默认 0 条空态 */
var DEMO_WL_ROWS = [
  { name: 'dnf.exe', path: 'D:\\Games\\DNF\\dnf.exe' },
  { name: 'WeGame.exe', path: 'C:\\Program Files (x86)\\WeGame\\WeGame.exe' },
  { name: 'DNFLauncher.exe', path: '' },
];
var WL_ROWS = [];
/* 键帽矩阵（首行与原版截图一致） */
var KB_ROWS = [
  ['Esc', 'F1', 'F2', 'F3', 'F4', 'F5', 'F6', 'F7', 'F8', 'F9', 'F10', 'F11', 'F12', 'Print', 'Scroll', 'Pause'],
  ['1', '2', '3', '4', '5', '6', '7', '8', '9', '0', '-', '='],
  ['Q', 'W', 'E', 'R', 'T', 'Y', 'U', 'I', 'O', 'P', '[', ']'],
];
var DEMO_KB_LIT = { J: 1, K: 1, L: 1, U: 1, F1: 1 };
var KB_LIT = {};

var PAGE_NAMES = {
  gamepad: '手柄映射', turbo: '连发映射修改', whitelist: '白名单修改',
  vibration: '通用震动修改', jobs: '全职业预设修改', audio: '音频震动', mod: '外观 MOD',
};

var state = {
  page: 'gamepad', paused: false, vib: true, segJob: false,
  audioOn: false, listenAll: false, motorTestUntil: 0,
  demo: false, light: false,
};

function $(sel) { return document.querySelector(sel); }
function $all(sel) { return Array.prototype.slice.call(document.querySelectorAll(sel)); }
function rgbCss(rgb, a) { return 'rgba(' + rgb[0] + ',' + rgb[1] + ',' + rgb[2] + ',' + (a == null ? 1 : a) + ')'; }
/* theme.rs truncate_chars */
function truncateChars(s, n) {
  var out = '';
  var cnt = 0;
  for (var i = 0; i < s.length && cnt < n; i++) { out += s[i]; cnt++; }
  return cnt < s.length ? out + '…' : out;
}

/* ---------------- 浅色主题（theme.rs Theme::light()） ---------------- */
function isLight() { return document.documentElement.classList.contains('light'); }
function applyTheme(light, save) {
  state.light = !!light;
  document.documentElement.classList.toggle('light', !!light);
  if (save) { try { localStorage.setItem('replica-theme', light ? 'light' : 'dark'); } catch (e) {} }
  var use = document.querySelector('[data-act="toggle-dark"] use');
  if (use) use.setAttribute('href', light ? '#i-sun' : '#i-moon');
  var btn = document.querySelector('[data-act="toggle-dark"]');
  if (btn) btn.title = light ? '切换深浅主题（当前: 浅色）' : '切换深浅主题';
  var dk = $('#set-dark'); if (dk) dk.checked = !light;
  mountPad(); /* 热点玻璃底/文字颜色随主题 */
}

/* ---------------- toast ---------------- */
function toast(msg) {
  var root = $('#toast-root'); if (!root) return;
  var el = document.createElement('div');
  el.className = 'ef-toast';
  el.textContent = msg;
  root.appendChild(el);
  setTimeout(function () { el.classList.add('out'); }, 2200);
  setTimeout(function () { if (el.parentNode) el.parentNode.removeChild(el); }, 2600);
}

/* ---------------- 弹窗 ---------------- */
function openDlg(id) { var el = document.getElementById(id); if (el) el.classList.add('open'); }
function closeDlg(id) { var el = document.getElementById(id); if (el) el.classList.remove('open'); }

/* ---------------- 导航 ---------------- */
function go(pg) {
  if (!PAGE_NAMES[pg]) pg = 'gamepad';
  state.page = pg;
  $all('.nav-item').forEach(function (b) { b.classList.toggle('is-active', b.dataset.pg === pg); });
  $all('.page').forEach(function (p) { p.classList.toggle('is-active', p.dataset.pg === pg); });
  var tb = $('#tb-page'); if (tb) tb.textContent = PAGE_NAMES[pg];
}

/* ---------------- 手柄热点（page.rs render_gamepad_svg: 玻璃底/主环/中心点/短标签） ---------------- */
var gpFlow = { mode: 'idle', slot: null, keys: [], kind: 'target', liveTimer: 0 };
function shortKey(slot) {
  var map = {
    'LT': 'LT', 'LB': 'LB', 'RT': 'RT', 'RB': 'RB',
    '十字键·上': 'DPad_Up', '十字键·下': 'DPad_Down', '十字键·左': 'DPad_Left', '十字键·右': 'DPad_Right',
    '左摇杆·上': 'LS_Up', '左摇杆·下': 'LS_Down', '左摇杆·左': 'LS_Left', '左摇杆·右': 'LS_Right',
    '右摇杆·上': 'RS_Up', '右摇杆·下': 'RS_Down', '右摇杆·左': 'RS_Left', '右摇杆·右': 'RS_Right',
    'X 键': 'X', 'Y 键': 'Y', 'B 键': 'B', 'A 键': 'A', 'Back': 'Back', 'Start': 'Start',
  };
  return map[slot.label] || slot.label;
}
function findSlot(id) {
  for (var i = 0; i < SLOTS.length; i++) if (SLOTS[i].id === id) return SLOTS[i];
  return null;
}
function mountPad() {
  var wrap = $('#pad-art');
  if (!wrap) return;
  var svg = wrap.querySelector('svg');
  if (!svg) return;
  $all('#pad-art g.hot').forEach(function (g) { if (g.parentNode) g.parentNode.removeChild(g); });
  var NS = 'http://www.w3.org/2000/svg';
  var VB = { x: 22, y: 135, w: 545, h: 401 };
  var dark = !isLight();
  SLOTS.forEach(function (s) {
    var rgb = KIND_RGB[s.kind];
    var cx = VB.x + s.x * VB.w, cy = VB.y + s.y * VB.h, r = s.r * VB.w;
    var configured = Object.prototype.hasOwnProperty.call(PAD_TARGETS, shortKey(s));
    var g = document.createElementNS(NS, 'g');
    g.setAttribute('class', 'hot');
    g.dataset.slot = String(s.id);
    var isClick = s.kind === 'stickclick';
    /* 玻璃底 (深机身增透: dark black64 / light black30) */
    var glass = document.createElementNS(NS, 'circle');
    glass.setAttribute('cx', cx); glass.setAttribute('cy', cy); glass.setAttribute('r', r);
    glass.setAttribute('fill', dark ? 'rgba(0,0,0,0.25)' : 'rgba(0,0,0,0.12)');
    g.appendChild(glass);
    /* 主环: 已配置=类色实线 2 / 空槽=类色细线 1.4 */
    var ring = document.createElementNS(NS, 'circle');
    ring.setAttribute('cx', cx); ring.setAttribute('cy', cy); ring.setAttribute('r', r);
    ring.setAttribute('fill', 'none');
    ring.setAttribute('stroke', configured ? rgbCss(rgb, 0.92) : rgbCss(rgb, 0.49));
    ring.setAttribute('stroke-width', configured ? '2' : '1.4');
    g.appendChild(ring);
    /* 中心点（单字符槽位与摇杆按下） */
    if ((s.short.length === 1) || isClick) {
      var dot = document.createElementNS(NS, 'circle');
      dot.setAttribute('cx', cx); dot.setAttribute('cy', cy); dot.setAttribute('r', Math.max(r * 0.15, 1.6));
      dot.setAttribute('fill', rgbCss(rgb, configured ? 1 : 0.55));
      g.appendChild(dot);
    }
    /* 短标签（方向箭头/肩键文字; 面键与系统键留白 —— 与源码一致） */
    if (s.short && !isClick && s.kind !== 'button' && s.kind !== 'system') {
      var fs = s.short.length === 1 ? 13 : 9.5;
      var t = document.createElementNS(NS, 'text');
      t.setAttribute('x', cx); t.setAttribute('y', cy + fs * 0.36);
      t.setAttribute('text-anchor', 'middle');
      t.setAttribute('font-size', fs);
      t.setAttribute('font-family', 'Segoe UI, Microsoft YaHei, sans-serif');
      t.setAttribute('font-weight', '600');
      t.setAttribute('fill', configured ? rgbCss(rgb, 1) : 'rgba(255,255,255,0.75)');
      t.textContent = s.short;
      g.appendChild(t);
    }
    g.addEventListener('click', function () {
      if (gpFlow.mode === 'awaitpad') { gpCapturePad(s); return; }
      gpFlow = { mode: 'selected', slot: s.id, keys: [], kind: 'target' };
      renderGpFlow();
    });
    svg.appendChild(g);
  });
  syncPadFlowVisual();
}
/* 捕获态雷达环 / 演示实体按下亮块 */
function clearPadFx() {
  $all('#pad-art .radar-ring, #pad-art .live-fill, #pad-art .live-ring').forEach(function (el) {
    if (el.parentNode) el.parentNode.removeChild(el);
  });
}
function syncPadFlowVisual() {
  clearPadFx();
  if (gpFlow.mode !== 'awaitkb' && gpFlow.mode !== 'awaitpad') return;
  var s = findSlot(gpFlow.slot); if (!s) return;
  var svg = $('#pad-art svg'); if (!svg) return;
  var NS = 'http://www.w3.org/2000/svg';
  var VB = { x: 22, y: 135, w: 545, h: 401 };
  var cx = VB.x + s.x * VB.w, cy = VB.y + s.y * VB.h, r = s.r * VB.w;
  var radar = document.createElementNS(NS, 'circle');
  radar.setAttribute('class', 'radar-ring');
  radar.setAttribute('cx', cx); radar.setAttribute('cy', cy); radar.setAttribute('r', r);
  radar.setAttribute('fill', 'rgba(255,190,60,0.15)');
  radar.setAttribute('stroke', 'rgba(255,190,60,0.75)');
  radar.setAttribute('stroke-width', '2');
  svg.appendChild(radar);
}
/* 演示：实体按下（v8 A20: 类色亮填充 + 白环 + 雷达扩散 1.4s 一次） */
function demoLivePress() {
  var s = SLOTS[Math.floor(Math.random() * SLOTS.length)];
  var svg = $('#pad-art svg'); if (!svg) return;
  var NS = 'http://www.w3.org/2000/svg';
  var VB = { x: 22, y: 135, w: 545, h: 401 };
  var cx = VB.x + s.x * VB.w, cy = VB.y + s.y * VB.h, r = s.r * VB.w;
  var rgb = KIND_RGB[s.kind];
  var fill = document.createElementNS(NS, 'circle');
  fill.setAttribute('class', 'live-fill');
  fill.setAttribute('cx', cx); fill.setAttribute('cy', cy); fill.setAttribute('r', r);
  fill.setAttribute('fill', rgbCss(rgb, 0.92));
  var halo = document.createElementNS(NS, 'circle');
  halo.setAttribute('class', 'live-fill');
  halo.setAttribute('cx', cx); halo.setAttribute('cy', cy); halo.setAttribute('r', r * 1.45);
  halo.setAttribute('fill', rgbCss(rgb, 0.45));
  var ring = document.createElementNS(NS, 'circle');
  ring.setAttribute('class', 'live-ring');
  ring.setAttribute('cx', cx); ring.setAttribute('cy', cy); ring.setAttribute('r', r);
  ring.setAttribute('fill', 'none');
  ring.setAttribute('stroke', '#FFFFFF'); ring.setAttribute('stroke-width', '2.6');
  var radar = document.createElementNS(NS, 'circle');
  radar.setAttribute('class', 'radar-ring');
  radar.setAttribute('cx', cx); radar.setAttribute('cy', cy); radar.setAttribute('r', r);
  radar.setAttribute('fill', 'rgba(255,190,60,0.15)');
  radar.setAttribute('stroke', 'rgba(255,190,60,0.75)');
  radar.setAttribute('stroke-width', '2');
  radar.style.animationIterationCount = '1';
  svg.appendChild(halo); svg.appendChild(fill); svg.appendChild(ring); svg.appendChild(radar);
  clearTimeout(gpFlow.liveTimer);
  gpFlow.liveTimer = setTimeout(clearPadFx, 1400);
  toast('演示：实体按下「' + s.label + '」— 雷达扩散 1.4 秒');
}

/* ---------------- 捕获流（panel.rs 三步状态机 + GpFlow::MapDone） ---------------- */
function stepHeader(step, title) {
  return '<div class="row"><span class="step-badge">第 ' + step + ' 步</span><span class="slot-step-title">' + title + '</span></div>';
}
function keycapHtml(k) { return '<span class="keycap" style="font-size:11px">' + k + '</span>'; }
function renderGpFlow() {
  var host = $('#gp-slot-panel'), quick = $('#gp-quickcard');
  if (!host || !quick) return;
  syncPadFlowVisual();
  if (gpFlow.mode === 'idle') { host.style.display = 'none'; quick.style.display = ''; return; }
  var s = findSlot(gpFlow.slot);
  if (!s) { gpFlow = { mode: 'idle', slot: null, keys: [], kind: 'target' }; host.style.display = 'none'; quick.style.display = ''; return; }
  quick.style.display = 'none';
  host.style.display = '';
  var key = shortKey(s);
  var cur = PAD_TARGETS[key];
  var rgb = KIND_RGB[s.kind];
  var h = '';
  if (gpFlow.mode === 'selected') {
    h += stepHeader(1, s.label);
    h += '<div class="row mt8"><span class="badge" style="background:' + rgbCss(rgb, 0.9) + ';color:var(--on-emphasis)">' + KIND_LEGEND[s.kind] + '</span></div>';
    h += cur
      ? '<div class="slot-summary good">' + s.label + ' 键  →  键盘 ' + cur + '</div>'
      : '<div class="slot-summary warn">' + s.label + ' 键  →  还没设键盘键</div>';
    h += '<div class="row mt8"><span class="t-12 t-weak">手柄按键</span><span class="badge trigger" title="原型无 HID: 显示槽位键名">' + (s.short || key) + '</span></div>';
    h += '<div class="row mt4" style="flex-wrap:wrap"><span class="t-12 t-weak">键盘按键</span>' +
      (cur ? keycapHtml(cur) : '<span class="t-12 t-hint">还没设 — 点下面的按钮后按键盘</span>') + '</div>';
    h += '<hr class="sep">';
    h += '<button class="btn primary" style="width:100%;height:36px;justify-content:center" data-act="gp-set-kb">' +
      (cur ? '修改键盘键' : '第 ② 步: 按键盘键 (设置要代替的键)') + '</button>';
    h += '<div class="row mt8" style="flex-wrap:wrap">' +
      '<button class="btn secondary small" data-act="gp-set-pad">校对手柄键位</button>' +
      '<button class="btn secondary small" data-act="gp-mouse">鼠标映射…</button></div>';
    h += '<div class="row mt8" style="flex-wrap:wrap">' +
      '<button class="btn secondary small" data-act="gp-clear" ' + (cur ? '' : 'disabled style="opacity:.45;cursor:default"') + '>清空键盘按键</button>' +
      '<button class="btn danger small" data-act="gp-del" ' + (cur ? '' : 'disabled style="opacity:.45;cursor:default"') + ' title="删除需要二次确认">删除该映射</button></div>';
    h += '<button class="btn primary mt12" style="width:100%;height:36px;justify-content:center" data-act="gp-finish">✓ 完成修改</button>';
  } else if (gpFlow.mode === 'awaitkb') {
    h += stepHeader(2, s.label + ': 按键盘上你想让它代替的那个键');
    if (gpFlow.keys.length) {
      h += '<div class="row mt8" style="flex-wrap:wrap"><span class="t-12 t-weak">已捕获</span>' +
        gpFlow.keys.map(keycapHtml).join(' ') + '</div>';
    }
    h += '<div class="t-12 t-hint mt8" style="line-height:1.8">按下后先出现「确认」按钮, 确认后才写入。想设组合键: 在确认页点「＋ 再加一个键」。<br>本步只认键盘键 —— 想换手柄键请先点「取消」回到上一步。</div>';
    h += '<button class="btn secondary mt12" data-act="gp-cancel">✕ 取消</button>';
  } else if (gpFlow.mode === 'awaitpad') {
    h += stepHeader(2, s.label + ': 请按手柄上的一个键');
    h += '<div class="t-12 t-hint mt8" style="line-height:1.8">按一下手柄上的这个键**再松开** (扳机也照扣一次), 然后点「确认」。<br>程序会记住这个槽位对应你手柄上的哪个键 (与连发映射同一套识别)。</div>';
    h += '<div class="t-12 t-hint mt4">按了没反应? 第三方手柄请先在设备管理里激活该手柄。<br>原型: 直接点手柄图上这个键即可模拟按下。</div>';
    h += '<button class="btn secondary mt12" data-act="gp-cancel">✕ 取消</button>';
  } else if (gpFlow.mode === 'confirm') {
    h += stepHeader(3, '确认');
    h += '<div class="mt8 bold" style="font-size:16px;color:var(--accent)">已捕获: ' + gpFlow.keys.join('+') + '</div>';
    h += '<div class="t-12 t-hint mt4">' + (gpFlow.kind === 'trigger' ? '点「确认」把该手柄键记为这个槽位' : '点「确认」把它记成这个键位的键盘映射') + '</div>';
    h += '<div class="row mt12" style="flex-wrap:wrap">' +
      '<button class="btn primary" style="min-width:120px;height:34px;justify-content:center" data-act="gp-confirm">✓ 确认</button>' +
      (gpFlow.kind === 'target' ? '<button class="btn secondary" style="min-width:130px;height:34px;justify-content:center" data-act="gp-more" title="继续按键盘, 组成组合键 (如 CTRL+F6)">＋ 再加一个键</button>' : '') +
      '<button class="btn secondary" style="min-width:90px;height:34px;justify-content:center" data-act="gp-cancel">✕ 取消</button></div>';
  } else if (gpFlow.mode === 'mapdone') {
    h += '<div class="row mt8"><span class="badge" style="background:var(--good);color:var(--on-emphasis)">已完成</span></div>';
    h += '<div class="mt8 bold" style="font-size:16px;color:var(--good)">「' + s.label + '」已更新</div>';
    h += '<div class="t-12 t-hint mt4" style="line-height:1.8">本次按键已完成修改。您可以继续按下其他手柄按键，为其编辑新的映射键位；也可以留在当前页面，继续调整此按键的设置。</div>';
    h += '<button class="btn primary mt12" data-act="gp-edit-again">编辑本映射</button>';
  }
  host.innerHTML = h;
}
/* 真实键盘捕获（浏览器 keydown; Esc 取消 —— 与原版语义一致） */
function normKey(e) {
  var k = e.key;
  if (k === 'Escape') return null;
  if (k === ' ') return 'SPACE';
  if (k === 'ArrowUp') return 'UP';
  if (k === 'ArrowDown') return 'DOWN';
  if (k === 'ArrowLeft') return 'LEFT';
  if (k === 'ArrowRight') return 'RIGHT';
  if (k === 'Shift') return 'LSHIFT';
  if (k === 'Control') return 'LCTRL';
  if (k === 'Alt') return 'LALT';
  if (k === 'Enter') return 'ENTER';
  if (k === 'Tab') return 'TAB';
  if (/^[a-zA-Z]$/.test(k)) return k.toUpperCase();
  if (/^[0-9]$/.test(k)) return k;
  if (/^F\d{1,2}$/.test(k)) return k;
  return k.length === 1 ? k.toUpperCase() : k.toUpperCase();
}
function gpBeginKb() {
  gpFlow.mode = 'awaitkb'; gpFlow.keys = [];
  renderGpFlow();
}
function gpBeginPad() {
  gpFlow.mode = 'awaitpad';
  renderGpFlow();
  toast('请按手柄上的这个键（原型: 点手柄图上该键模拟）');
}
function gpCapturePad(s) {
  gpFlow.mode = 'confirm'; gpFlow.kind = 'trigger'; gpFlow.keys = [s.short || shortKey(s)];
  renderGpFlow();
}
function gpConfirm() {
  var s = findSlot(gpFlow.slot); if (!s) return;
  var key = shortKey(s);
  if (gpFlow.kind === 'trigger') {
    toast('已校对: ' + s.label + ' = ' + gpFlow.keys[0] + ' (原型写入)');
  } else {
    PAD_TARGETS[key] = gpFlow.keys.join('+');
    renderChips();
  }
  mountPad();
  gpFlow.mode = 'mapdone';
  renderGpFlow();
}

/* ---------------- 我设好的映射 chips（truncate_chars 14） ---------------- */
function renderChips() {
  var box = $('#gp-chips'); if (!box) return;
  box.innerHTML = '';
  var keys = Object.keys(PAD_TARGETS);
  var empty = $('#gp-chips-empty'), hint = $('#gp-set-hint'), title = $('#gp-set-title');
  if (empty) empty.hidden = keys.length > 0;
  if (hint) hint.hidden = keys.length === 0;
  if (title) title.textContent = keys.length ? '我设好的映射 (' + keys.length + ')' : '我设好的映射';
  keys.forEach(function (key) {
    var slot = null;
    SLOTS.forEach(function (s) { if (shortKey(s) === key) slot = s; });
    if (!slot) return;
    var full = slot.label + ' → ' + PAD_TARGETS[key];
    var b = document.createElement('button');
    b.className = 'target-chip mapped';
    b.style.fontSize = '11px';
    b.textContent = truncateChars(full, 14);
    b.title = full;
    b.setAttribute('data-act', 'chip-jump');
    box.appendChild(b);
  });
}

/* ---------------- 连发列表（备注截断 14 + 🔒锁定 / ⚡宏徽章, page.rs:631-651） ---------------- */
function renderTurboRows() {
  var box = $('#turbo-rows'); if (!box) return;
  box.innerHTML = '';
  TURBO_ROWS.forEach(function (m, i) {
    var row = document.createElement('div');
    row.className = 'turbo-row';
    var flags = m.flags.map(function (f) {
      return '<span class="badge ' + (f === '锁定' ? 'warn' : 'good') + '" style="font-size:11px;padding:2px 7px">' + (f === '锁定' ? '🔒 ' : '⚡ ') + f + '</span>';
    }).join(' ');
    row.innerHTML =
      '<span class="keycap" style="font-size:11px">' + m.trig + '</span><span class="arrow">→</span>' +
      '<span class="keycap" style="font-size:11px">' + m.tgt + '</span>' + flags +
      '<span class="ms right">' + m.ms + '</span>' +
      '<span class="note">' + truncateChars(m.note || '', 14) + '</span>' +
      '<button class="btn ghost small" data-act="edit-turbo" data-i="' + i + '">编辑</button>';
    box.appendChild(row);
  });
  var cnt = $('#turbo-count'); if (cnt) cnt.textContent = TURBO_ROWS.length + ' 条';
  var hero = $('#hero-count'); if (hero) hero.textContent = String(TURBO_ROWS.length);
}

/* ---------------- 键盘图（点按键 = 批量添加同名连发; 亮键=已有同名映射标绿） ---------------- */
var kbPick = {};
function renderKb() {
  var grid = $('#kb-grid'); if (!grid) return;
  grid.innerHTML = '';
  KB_ROWS.forEach(function (row) {
    var line = document.createElement('div');
    line.className = 'kb-line';
    row.forEach(function (k) {
      var b = document.createElement('button');
      b.className = 'kb-key' + (k.length > 3 ? ' wide' : '');
      if (KB_LIT[k]) b.classList.add('is-lit');
      if (kbPick[k]) b.classList.add('is-pick');
      b.textContent = k;
      b.setAttribute('data-act', 'kb-key');
      b.dataset.k = k;
      line.appendChild(b);
    });
    grid.appendChild(line);
  });
  var pc = $('#kb-pickcount');
  if (pc) pc.textContent = '已选 ' + Object.keys(kbPick).length + ' 键';
}

/* ---------------- 白名单（whitelist_page.rs 空态: 圆点 + 列表为空 —— 当前所有进程都可以使用连发） ---------------- */
function renderWl() {
  var box = $('#wl-rows'); if (!box) return;
  box.innerHTML = '';
  if (!WL_ROWS.length) {
    box.innerHTML = '<div style="text-align:center;padding:14px 0 6px">' +
      '<span class="orb" style="width:9px;height:9px;border-radius:50%;background:var(--text-weak);opacity:.7;display:inline-block"></span>' +
      '<div class="t-12 t-hint mt8">列表为空 —— 当前所有进程都可以使用连发</div></div>';
  }
  WL_ROWS.forEach(function (w, i) {
    var row = document.createElement('div');
    row.className = 'wl-row';
    row.innerHTML =
      '<div><div class="t-13" style="color:var(--text);font-weight:600">' + w.name + '</div>' +
      (w.path ? '<div class="t-11" style="font-size:11px;color:var(--text-weak)">' + w.path + '</div>' : '') + '</div>' +
      '<button class="btn secondary small right" style="width:26px;height:22px;padding:0;justify-content:center" data-act="wl-del" data-i="' + i + '" title="从白名单移除">✕</button>';
    box.appendChild(row);
  });
  var cnt = $('#wl-count'); if (cnt) cnt.textContent = WL_ROWS.length + ' 个';
  renderSetWl();
}
function renderSetWl() {
  var setRows = $('#set-wl-rows'); if (!setRows) return;
  setRows.innerHTML = '';
  if (!WL_ROWS.length) setRows.innerHTML = '<div class="t-12 t-hint" style="padding:6px 0">（空名单 —— 所有进程都会响应连发）</div>';
  WL_ROWS.forEach(function (w, i) {
    var row = document.createElement('div');
    row.className = 'row';
    row.style.cssText = 'padding:6px 0;border-bottom:1px solid var(--stroke)';
    var fn = w.name;
    row.innerHTML = '<div><span class="t-13 bold" style="color:var(--text)">' + fn + '</span>' +
      (w.path ? '<div style="font-size:11px;color:var(--text-weak)">' + w.path + '</div>' : '') + '</div>' +
      '<button class="wl-del-sq right" data-act="set-wl-del" data-i="' + i + '" title="从白名单移除">✕</button>';
    setRows.appendChild(row);
  });
}

/* ---------------- 音频预设宫格（audio_page.rs preset_tile: 名称+手感+推荐星+选中勾） ---------------- */
var AUDIO_PRESETS = [
  { name: '人声清晰', scene: '打游戏 / 看剧情（实测最佳）', feel: '只跟人声频段，杂音不理' },
  { name: '轻柔细腻', scene: '轻声内容（实测次佳）', feel: '细腻微颤，轻声也有反馈' },
  { name: '游戏强劲', scene: '打击感拉满', feel: '招招猛震，压制背景音乐' },
  { name: '低频沉浸', scene: '看电影 / 大场面', feel: '爆炸低音，轰隆感拉满' },
  { name: '全频强劲', scene: '要最猛的裸手感', feel: '所有声音都直接震' },
  { name: '测试版(全0)', scene: '排障用', feel: '排障用：确认马达正常' },
];
var audioPresetSel = 0;
function renderAudioPresets() {
  var box = $('#audio-presets'); if (!box) return;
  box.innerHTML = '';
  AUDIO_PRESETS.forEach(function (p, i) {
    var b = document.createElement('button');
    b.className = 'audio-tile' + (i === audioPresetSel ? ' on' : '');
    b.setAttribute('data-act', 'audio-preset');
    b.title = p.scene + ' —— ' + p.feel;
    b.innerHTML = '<span class="nm">' + p.name +
      (i === 0 ? '<span class="star">★推荐</span>' : '') +
      (i === audioPresetSel ? '<span class="tick">✓</span>' : '') +
      '</span><div class="fd">' + p.feel + '</div>';
    box.appendChild(b);
  });
  var badge = $('#audio-cur-badge');
  if (badge) badge.textContent = '当前：' + AUDIO_PRESETS[audioPresetSel].name;
}

/* ---------------- 实时监测（audio_page.rs: 频谱/马达/统计/判定横幅 + 引擎 6 态） ---------------- */
var AUDIO_STATUS = ['已关闭', '等待白名单游戏', '无音频设备', '等待声音', '运行中', '已暂停'];
var audioStatus = 4;
var audioMode = 0; /* 0 游戏 / 1 音乐 / 2 安静 / 3 人声 */
function renderAudioMonitor() {
  var host = $('#audio-monitor'); if (!host) return;
  if (!state.audioOn) {
    host.innerHTML = '<div class="row">' +
      '<span class="orb" style="width:9px;height:9px;border-radius:50%;background:var(--text-weak);opacity:.7;display:inline-block"></span>' +
      '<span class="t-13 bold" style="color:var(--heading)">音频震动已关闭</span>' +
      '<span class="t-12 t-hint">打开上面的总开关，这里就会显示频谱、两条马达电平和当前判定。</span></div>';
    return;
  }
  var modes = [
    ['game', '🎮', '当前判定：游戏音效段 —— 正常跟手震'],
    ['music', '♪', '当前判定：背景音乐段 —— 连续震动被压住，只有鼓点（节奏力度）和够响的音效会震'],
    ['quiet', '🔇', '当前判定：安静段 —— 手柄保持安静，不会乱震'],
    ['vocal', '🎤', '当前判定：人声/细节段 —— 轻微细腻的反馈，不会吵'],
  ];
  var m = modes[audioMode % 4];
  host.innerHTML =
    '<div class="spec-box"><div class="spec-cols" id="spec-cols">' +
      ['低频', '中低', '中频', '中高', '高频'].map(function (c) {
        return '<div class="spec-col"><div class="bar" style="height:30%"></div><div class="peak" style="bottom:44%"></div><div class="cap">' + c + '</div></div>';
      }).join('') + '</div></div>' +
    '<div class="motor-line"><span class="ml">左马达 低频</span><div class="mbar"><div class="mfill" style="width:0;background:var(--motor-l)"></div></div></div>' +
    '<div class="motor-line"><span class="ml">右马达 高频</span><div class="mbar"><div class="mfill" style="width:0;background:var(--motor-r)"></div></div></div>' +
    '<div class="mini-stats">' +
      '<div class="mini-stat"><div class="v" id="ms-noise">12%</div><div class="l">背景噪声 · 没声音时环境有多吵</div></div>' +
      '<div class="mini-stat"><div class="v" id="ms-thresh">33%</div><div class="l">屏蔽门槛 · 比它小的声音一律不震</div></div>' +
      '<div class="mini-stat"><div class="v" id="ms-trans">61%</div><div class="l">爆发度 · 越大越接近鼓点/重击</div></div>' +
    '</div>' +
    '<div class="mode-banner ' + m[0] + '"><span>' + m[1] + '</span><span>' + m[2] + '</span></div>' +
    '<div class="row mt8" style="flex-wrap:wrap;gap:6px"><span class="t-12 t-hint">引擎状态:</span>' +
      AUDIO_STATUS.map(function (n, i) {
        return '<button class="state-chip' + (i === audioStatus ? ' on' : '') + '" data-act="audio-status" data-i="' + i + '" title="原型演示: 引擎状态机 6 态 (state/audio_state.rs)">' + n + '</button>';
      }).join('') + '</div>';
  tickMonitor();
}
function tickMonitor() {
  if (!state.audioOn) return;
  $all('#spec-cols .spec-col').forEach(function (col) {
    var bar = col.querySelector('.bar'), peak = col.querySelector('.peak');
    if (!bar) return;
    var v = 15 + Math.random() * 80;
    bar.style.height = v.toFixed(0) + '%';
    if (peak) peak.style.bottom = Math.min(v + 8, 97).toFixed(0) + '%';
  });
  $all('.mbar .mfill').forEach(function (f) {
    var w = 8 + Math.random() * 75;
    f.style.width = w.toFixed(0) + '%';
    f.style.opacity = '0.85';
  });
  var n = $('#ms-noise'), t = $('#ms-thresh'), tr = $('#ms-trans');
  if (n) n.textContent = (8 + Math.random() * 10).toFixed(0) + '%';
  if (t) t.textContent = (28 + Math.random() * 12).toFixed(0) + '%';
  if (tr) tr.textContent = (30 + Math.random() * 60).toFixed(0) + '%';
}

/* ---------------- 高级震动调校 62 条（12 组, 口径 = v8 清单 B1 / vibration_page/page.rs:1463-2318） ---------------- */
var VIB_ADV = [
  ['连击密度自适应', 6, [
    ['密度基线', 0, 100, 55, '%'], ['连击窗口', 60, 1200, 320, 'ms'],
    ['密度上限', 100, 400, 220, '%'], ['收敛速度', 1, 20, 8, ''],
    ['断连判定', 80, 2000, 420, 'ms'], ['密度平滑', 0, 100, 40, '%'],
  ]],
  ['命中自适应', 7, [
    ['命中率窗口', 200, 3000, 900, 'ms'], ['高命中阈值', 50, 100, 82, '%'],
    ['高命中压缩', 0, 100, 35, '%'], ['漏击补震', 0, 100, 30, '%'],
    ['补震上限', 0, 200, 80, '%'], ['命中波动容忍', 0, 100, 45, '%'],
    ['自适应恢复', 1, 60, 12, 's'],
  ]],
  ['走路质感', 6, [
    ['步频跟随', 0, 100, 70, '%'], ['步伐幅度', 0, 100, 38, '%'],
    ['起步加重', 0, 100, 45, '%'], ['停步余震', 0, 500, 120, 'ms'],
    ['转身抖动', 0, 100, 25, '%'], ['地面材质系数', 0, 100, 50, '%'],
  ]],
  ['走位能量', 3, [
    ['能量上限', 0, 100, 60, '%'], ['蓄能速度', 1, 20, 6, ''],
    ['释放阈值', 0, 100, 55, '%'],
  ]],
  ['输出曲线', 2, [
    ['曲线形状', 0, 100, 50, '%'], ['曲线强度', 0, 200, 110, '%'],
  ]],
  ['事件衰减', 4, [
    ['事件合并窗口', 0, 200, 45, 'ms'], ['衰减曲线', 0, 100, 50, '%'],
    ['尾震保留', 0, 100, 35, '%'], ['最小可感幅值', 0, 100, 12, '%'],
  ]],
  ['S4 整形', 3, [
    ['起手整形', 0, 100, 60, '%'], ['中段整形', 0, 100, 50, '%'],
    ['收尾整形', 0, 100, 55, '%'],
  ]],
  ['节流', 3, [
    ['最小间隔', 0, 200, 20, 'ms'], ['每秒次数上限', 1, 200, 60, '次'],
    ['超限策略', 0, 2, 1, ''],
  ]],
  ['算法窗口', 7, [
    ['采样窗口', 16, 512, 128, '帧'], ['峰值窗口', 16, 512, 96, '帧'],
    ['平滑窗口', 1, 64, 8, '帧'], ['冷却窗口', 0, 2000, 180, 'ms'],
    ['风暴判定窗口', 100, 3000, 800, 'ms'], ['风暴内上限', 1, 100, 24, '次'],
    ['窗口滑动步长', 1, 128, 16, '帧'],
  ]],
  ['脉冲', 6, [
    ['脉冲幅度', 0, 100, 50, '%'], ['脉冲宽度', 4, 200, 40, 'ms'],
    ['脉冲间隔', 4, 500, 60, 'ms'], ['脉冲抖动', 0, 100, 20, '%'],
    ['双马达相位差', 0, 180, 40, '°'], ['脉冲叠加上限', 1, 16, 4, ''],
  ]],
  ['S1 群怪不打手', 14, [
    ['群怪判定数量', 2, 30, 6, '只'], ['群怪密度抑制', 0, 100, 55, '%'],
    ['单怪权重', 0, 100, 100, '%'], ['群怪权重', 0, 100, 45, '%'],
    ['抑制起效延迟', 0, 1000, 120, 'ms'], ['抑制释放延迟', 0, 2000, 400, 'ms'],
    ['抑制深度', 0, 100, 60, '%'], ['群怪马达分工', 0, 100, 50, '%'],
    ['群怪脉冲抑制', 0, 100, 70, '%'], ['群怪节流倍数', 1, 10, 3, '×'],
    ['群怪风暴阈值', 1, 100, 20, '次'], ['群怪尾震', 0, 100, 25, '%'],
    ['群怪恢复速度', 1, 60, 15, 's'], ['群怪最低保留', 0, 100, 15, '%'],
  ]],
  ['预留', 1, [['预留参数', 0, 100, 0, '%']]],
];
/* 评分细分 10 类（page.rs:1135-1167: 名称 0-100 + 触发次数） */
var RANK_EVENTS = [
  ['评分点', 70, 412, '每次命中累积的评分点'],
  ['极限闪避', 55, 38, '闪避/反击成功瞬间'],
  ['暴击', 62, 906, '暴击命中'],
  ['最终击杀', 80, 12, 'BOSS 击杀/结算'],
  ['凌空追击', 48, 64, '空中追击命中 (槽评分)'],
  ['命中第一击', 45, 210, '命中怪物第一击 (原"破甲"槽, 实测语义)'],
  ['增益叠加', 40, 87, '增益层数叠加'],
  ['释放技能', 52, 634, '释放技能瞬间'],
  ['暴击特写', 66, 41, '暴击/击杀时的镜头特写震屏'],
  ['怪物死亡', 35, 148, '怪物死亡 (OnTargetDie 纯目标死亡信号)'],
];
/* 逐项 L/R 马达权重（item_lr 26 + rank_lr 30, -100..+100, hints.rs:147-281） */
var LR_ITEM = ['总强度', '最大力度', '攻击', '受击', '震撼', '移动', '衰减时长', '命中时加重', '起手脉冲',
  '击杀脉冲', '随机增益', '输出平滑', '飘字强度', '节奏间隔', '节奏', '血量飘字', '特殊飘字', '状态飘字',
  '特效飘字', '攻击飘字', '命中飘字', '召唤兽窗口', '召唤兽次数', '落地', '浮空', '被击'];
var LR_RANK = ['移动持续', '评分 D', '评分 C', '评分 B', '评分 A', '评分 S', '评分 SS', '评分 SSS',
  '评分点', '极限闪避', '暴击', '最终击杀', '凌空追击', '命中第一击', '增益叠加', '释放技能', '暴击特写', '怪物死亡',
  '衰减延迟', '衰减速度', '最小倍率', '最大倍率',
  '连击 2 档', '连击 5 档', '连击 10 档', '连击 20 档',
  '风暴期间', '群怪期间', 'BOSS 战', '测试通道'];

function sliderRow(name, sub, min, max, val, unit) {
  var disp = unit === 'frac' ? (val / 100).toFixed(2) : val + unit;
  return '<div class="slider-row"><span class="lbl">' + name +
    (sub ? '<span style="color:var(--hint);font-size:10.5px"> · ' + sub + '</span>' : '') + '</span>' +
    '<input type="range" class="slider" min="' + min + '" max="' + max + '" value="' + val + '" data-slider>' +
    '<span class="val">' + disp + '</span></div>';
}
function renderVibAdv() {
  var host = $('#vib-adv'); if (!host) return;
  var html = '';
  VIB_ADV.forEach(function (g) {
    html += '<div class="adv-group-title">' + g[0] + ' · ' + g[1] + ' 条</div>';
    g[2].forEach(function (d) { html += sliderRow(d[0], '', d[1], d[2], d[3], d[4]); });
  });
  host.innerHTML = html;
  /* 全职业参数区同款渲染（job_presets: 与通用型震动设定一致） */
  var job = $('#job-adv');
  if (job) {
    var jh = '';
    VIB_ADV.forEach(function (g) {
      jh += '<div class="adv-group-title">' + g[0] + ' · ' + g[1] + ' 条</div>';
      g[2].forEach(function (d) { jh += sliderRow(d[0], '', d[1], d[2], d[3], d[4]); });
    });
    jh += '<div class="adv-group-title">评分细分事件 · 10 类 (触发次数为演示快照)</div>';
    RANK_EVENTS.forEach(function (d) { jh += sliderRow(d[0] + ' (' + d[2] + ' 次)', '', 0, 100, d[1], '%'); });
    jh += '<div class="adv-group-title">逐项 L/R 马达权重 · 通用参数 (item_lr × 26)</div>';
    LR_ITEM.forEach(function (n, i) { jh += sliderRow(n, 'LR-' + ('0' + (i + 1)).slice(-2), -100, 100, 0, ''); });
    jh += '<div class="adv-group-title">逐项 L/R 马达权重 · 评分与衰减 (rank_lr × 30)</div>';
    LR_RANK.forEach(function (n, i) { jh += sliderRow(n, 'LR-' + ('0' + (i + 1)).slice(-2), -100, 100, 0, ''); });
    job.innerHTML = jh;
  }
}
function renderVibRank() {
  var host = $('#vib-rank'); if (!host) return;
  host.innerHTML = RANK_EVENTS.map(function (d) {
    return sliderRow(d[0] + ' (' + d[2] + ' 次)', '', 0, 100, d[1], '%');
  }).join('');
}
function renderLR(hostId, names) {
  var host = $(hostId); if (!host) return;
  host.innerHTML = names.map(function (n, i) {
    return sliderRow(n, 'LR-' + ('0' + (i + 1)).slice(-2), -100, 100, 0, '');
  }).join('');
}

/* ---------------- 全职业名册 14 类（job_presets/mod.rs builtin_jobs, 男女分列+外传） ---------------- */
var JOBS = [
  { base: '鬼剑士（男）', classes: [
    ['剑魂', '光剑连击大师, 极快轻盈, 连击伤害可观 (专属算法: 1)'],
    ['狂战士', '普攻极快但技能沉重, 血气爆发 (专属算法: 1)'],
    ['鬼泣', '鬼神布阵, 中远程持续输出 (专属算法: 1)'],
    ['阿修罗', '波动魔法稳扎稳打, 厚重 (专属算法: 1)'],
  ]},
  { base: '鬼剑士（女）', classes: [
    ['驭剑士', '四武器切换, 干净利落爆发强 (专属算法: 2)'],
    ['暗殿骑士', '暗影强控, 沉稳压迫 (专属算法: 2)'],
    ['契魔者', '蛇腹剑范围巨大, 霸道极重 (专属算法: 2)'],
    ['流浪武士', '双持光剑, 飘逸连招 (专属算法: 2)'],
    ['刃影', '收刀术瞬间倾泻伤害, 犀利 (专属算法: 2)'],
  ]},
  { base: '格斗家（男）', classes: [
    ['气功师', '念气环绕攻防一体, 稳健 (专属算法: 3)'],
    ['散打', '拳脚瞬间爆发, 刚猛直接 (专属算法: 3)'],
    ['街霸', '投掷异常状态, 阴险多变 (专属算法: 3)'],
    ['柔道家', '投技抱摔无敌判定, 极重 (专属算法: 3)'],
  ]},
  { base: '格斗家（女）', classes: [
    ['气功师', '念气护盾群体增益, 攻守兼备 (专属算法: 4)'],
    ['散打', '一脚定乾坤, 凌厉迅捷 (专属算法: 4)'],
    ['街霸', '涂毒投掷, 狡黠多变 (专属算法: 4)'],
    ['柔道家', '空中摔技, 连招流畅厚重 (专属算法: 4)'],
  ]},
  { base: '神枪手（男）', classes: [
    ['漫游枪手', '体术射击, 飘逸炫酷 (专属算法: 5)'],
    ['枪炮师', '重型火器AOE王者, 毁灭性极重 (专属算法: 5)'],
    ['机械师', '脱手机械军团, 从容安逸 (专属算法: 5)'],
    ['弹药专家', '属性弹连射, 灵活多变 (专属算法: 5)'],
    ['合金战士', '战甲硬朗, 厚重 (专属算法: 5)'],
  ]},
  { base: '神枪手（女）', classes: [
    ['漫游枪手', '枪刃凌厉, 爆发突出 (专属算法: 6)'],
    ['枪炮师', '空中重炮, 极重压制 (专属算法: 6)'],
    ['机械师', 'G系列自动攻击, 科技感 (专属算法: 6)'],
    ['弹药专家', '滞空轰炸, 倾泻极快 (专属算法: 6)'],
  ]},
  { base: '魔法师（男）', classes: [
    ['元素爆破师', '四属性连射, 持续压制 (专属算法: 7)'],
    ['冰结师', '冰系冰冻近身, 冷冽干脆 (专属算法: 7)'],
    ['血法师', '吸血强化, 压制 (专属算法: 7)'],
    ['逐风者', '大范围旋风, 极速飘逸 (专属算法: 7)'],
    ['次元行者', '与奈雅丽协同, 诡异多变 (专属算法: 7)'],
  ]},
  { base: '魔法师（女）', classes: [
    ['元素师', '四系元素蓄气, 极重压制 (专属算法: 8)'],
    ['召唤师', '召唤物连击全游戏最高, 震动极限压制防狂震 (高级算法: 连击极高·极限压制)'],
    ['战斗法师', '炫纹系统, 灵动机巧 (专属算法: 8)'],
    ['魔道学者', '装置控场, 古灵精怪 (专属算法: 8)'],
  ]},
  { base: '圣职者（男）', classes: [
    ['圣骑士（辅助）', '祝福护盾, 轻柔稳守 (专属算法: 9)'],
    ['圣骑士（审判）', '光系法术爆发, 沉重 (专属算法: 9)'],
    ['蓝拳圣使', '极速拳击连击, 操作极高 (专属算法: 9)'],
    ['驱魔师（力驱）', '战斧沉重挥砍, 刚猛霸道 (专属算法: 9)'],
    ['驱魔师（法驱）', '符咒阵法, 持续控场 (专属算法: 9)'],
    ['复仇者', '恶魔变身, 狂野持续 (专属算法: 9)'],
  ]},
  { base: '圣职者（女）', classes: [
    ['圣骑士', '增益恢复护盾, 圣洁优雅 (专属算法: 10)'],
    ['异端审判者', '战斧灼烧横扫, 酷烈 (专属算法: 10)'],
    ['巫女', '念珠中远程, 安逸端庄 (专属算法: 10)'],
    ['诱魔者', '原罪变身, 魅惑狂气 (专属算法: 10)'],
    ['女蓝拳', '直拳简单粗暴, 沉重 (专属算法: 10)'],
  ]},
  { base: '暗夜使者（女）', classes: [
    ['刺客', '收招终结技爆炸, 空中连击 (专属算法: 11)'],
    ['死灵术士', '巴拉克亡灵, 中远程统御 (专属算法: 11)'],
    ['忍者', '残影瞬间多重爆发, 凌厉 (专属算法: 11)'],
    ['影舞者', '背击暗杀, 犀利 (专属算法: 11)'],
  ]},
  { base: '守护者（女）', classes: [
    ['精灵骑士', '连锁充能, 骑乘冲锋极重 (专属算法: 12)'],
    ['混沌魔灵', '魔灵军团爆炸, 碾压 (专属算法: 12)'],
    ['帕拉丁', '盾反防御, 沉重稳守 (专属算法: 12)'],
    ['龙骑士', '骑龙立体空战, 厚重 (专属算法: 12)'],
  ]},
  { base: '魔枪士（男）', classes: [
    ['征战者', '战戟刚猛碾压, 极重 (专属算法: 13)'],
    ['决战者', '枪术高速连刺, 灵动飘逸 (专属算法: 13)'],
  ]},
  { base: '外传', classes: [
    ['黑暗武士', '自创连段, 自由度高 (专属算法: 14)'],
  ]},
];
var jobBase = 0;
function renderJobs() {
  var bs = $('#job-base'), cs = $('#job-class');
  if (!bs) return;
  bs.innerHTML = JOBS.map(function (j, i) {
    return '<option' + (i === jobBase ? ' selected' : '') + '>' + j.base + '</option>';
  }).join('');
  renderJobClasses();
}
function renderJobClasses() {
  var cs = $('#job-class'); if (!cs) return;
  cs.innerHTML = JOBS[jobBase].classes.map(function (c, i) {
    return '<option' + (i === 0 ? ' selected' : '') + '>' + c[0] + '</option>';
  }).join('');
  updateJobDesc();
}
function updateJobDesc() {
  var cs = $('#job-class'), desc = $('#job-desc');
  if (!cs || !desc) return;
  var c = JOBS[jobBase].classes[cs.selectedIndex] || JOBS[jobBase].classes[0];
  desc.innerHTML = '开关已关闭: 全职业预设不应用, 使用通用型震动设定参数<br>' + c[1];
}

/* ---------------- 序列宏卡（turbo_page/edit.rs render_sequence_steps_editor） ---------------- */
var SEQ = [];
var seqRecording = false;
function renderSeq() {
  var host = $('#seq-steps'); if (!host) return;
  var bar = $('#seq-bar');
  if (bar) {
    var rec = bar.querySelector('[data-act="seq-rec"]');
    if (rec) rec.outerHTML = seqRecording
      ? '<button class="btn secondary small" data-act="seq-rec">■ 停止并填入</button>'
      : '<button class="btn secondary small" data-act="seq-rec">● 录制按键</button>';
    var recLine = bar.querySelector('.seq-rec');
    if (recLine) recLine.remove();
    if (seqRecording) {
      var s = document.createElement('span');
      s.className = 'seq-rec';
      s.textContent = '● 录制中… (按键盘按键, 完了点右边停止)';
      bar.appendChild(s);
    }
  }
  if (!host) return;
  var hasBad = SEQ.some(function (st) { return st.bad; });
  host.innerHTML = SEQ.length ? SEQ.map(function (st, i) {
    var txt = st.wait ? '等待 ' + st.wait + ' ms'
      : (st.bad ? '引用宏「' + st.name + '」(不存在!)' : (st.mode === 'down' ? '↓ 按下 ' : st.mode === 'up' ? '↑ 抬起 ' : '') + st.k);
    return '<div class="seq-step-row"><span class="idx">' + (i + 1) + '.</span><span>' + txt + '</span>' +
      (st.wait ? '' : '<span class="ms right" style="font-size:11px;color:var(--text-weak)">hold ' + (st.ms || 20) + 'ms</span>') +
      '<button class="btn ghost small" style="height:22px" data-act="seq-del" data-i="' + i + '">删</button></div>';
  }).join('') : '<div class="t-12 t-hint" style="padding:4px 0">还没有通用宏 —— 点上面「＋ 按键步」或「● 录制按键」试试 (原型即时演示, 不落盘)。</div>';
  var fail = $('#seq-fail'); if (fail) fail.hidden = !hasBad;
  var lock = $('#seq-lock'); if (lock) lock.hidden = !(SEQ.length && !hasBad);
}
function seqAddKey(mode) {
  seqCapture = { mode: mode };
  toast('正在捕获中… (按一个键盘键, Esc 取消)');
}
var seqCapture = null;

/* ---------------- 设置弹窗（settings_dialog.rs 逐行） ---------------- */
var SET_MAPPINGS = [
  { trig: 'J', targets: ['J'], turbo: true, dtap: false, run: false, mouse: false, note: '普通攻击' },
  { trig: 'K', targets: ['K'], turbo: true, dtap: false, run: false, mouse: false, note: '技能1' },
  { trig: 'CTRL+F1', targets: ['1'], turbo: false, dtap: false, run: false, mouse: false, note: '' },
];
var setNew = { trig: '', targets: [], turbo: true, dtap: false };
function renderSetMappings() {
  var host = $('#set-mapping-rows'); if (!host) return;
  host.innerHTML = SET_MAPPINGS.map(function (m, i) {
    var chips = m.targets.length > 1
      ? '<div class="form-line" style="margin:4px 0"><span style="font-size:12px;color:var(--text-weak)">🎯</span>' +
        m.targets.map(function (t, ti) {
          return '<button class="evt-chip" data-act="set-chip-del" data-i="' + i + '" data-ti="' + ti + '" title="🗑 点击移除">' + t + ' ✕</button>';
        }).join('') + '</div>'
      : '';
    var mouseRow = m.mouse
      ? '<div class="form-line" style="margin-bottom:6px"><span class="t-13" style="color:var(--text)">间隔:</span><input class="mini-input" value="5"><span class="t-13" style="color:var(--text)">速度:</span><input class="mini-input" value="5"><span class="t-13" style="color:var(--text)">时长:</span><input class="mini-input" value="5"></div>'
      : '';
    return '<div class="set-mapping-row">' +
      '<div class="form-line" style="margin-bottom:6px"><span class="t-14 bold" style="color:var(--text-weak);width:26px">' + (i + 1) + '.</span>' +
      '<span class="t-13 bold" style="color:var(--accent-text)">触发键:</span>' +
      '<button class="cap-btn w170" data-act="set-cap-trigger" data-i="' + i + '">' + truncateChars(m.trig, 9) + '</button></div>' +
      '<div class="form-line" style="margin-bottom:6px"><span style="width:26px"></span>' +
      '<span class="t-13 bold" style="color:var(--accent-text)">目标键:</span>' +
      '<button class="cap-btn w170" data-act="set-cap-target" data-i="' + i + '">' + truncateChars(m.targets.join('+') || '点击设置按键', 9) + '</button>' +
      '<button class="btn-sq accent" data-act="set-cap-target" data-i="' + i + '" title="➕ 添加目标键">＋</button>' +
      '<button class="btn-sq danger" data-act="set-target-clear" data-i="' + i + '" title="🗑 清除所有目标键">✖</button></div>' +
      chips + mouseRow +
      '<div class="form-line" style="margin-bottom:6px">' +
      '<button class="tgl-btn dir" data-act="set-mouse-dir" title="设置鼠标移动方向">⌖ 方向</button>' +
      '<button class="tgl-btn scroll" data-act="set-mouse-scroll" title="设置鼠标滚动方向">🎡 滚动</button>' +
      '<button class="tgl-btn' + (m.turbo ? ' on-accent' : '') + '" data-act="set-tgl" data-i="' + i + '" data-f="turbo" title="' + (m.turbo ? '连发开启 - 自动重复输入' : '连发关闭 - 仅单次输入') + '">' + (m.turbo ? '⚡ 连发' : '○ 连发') + '</button>' +
      '<button class="tgl-btn' + (m.dtap ? ' on-good' : '') + '" data-act="set-tgl" data-i="' + i + '" data-f="dtap" title="' + (m.run ? '已勾选「重推奔跑」, 两者互斥 —— 先关掉重推奔跑' : (m.dtap ? '简易奔跑开启 - 按一次自动补一次敲击 (DFO 跑动)' : '简易奔跑关闭 - 按一次触发一次按键')) + '">' + (m.dtap ? '简易奔跑:开' : '简易奔跑') + '</button>' +
      '<button class="tgl-btn' + (m.run ? ' on-good' : '') + '" data-act="set-tgl" data-i="' + i + '" data-f="run" title="' + (m.dtap ? '已勾选「简易奔跑」, 两者互斥 —— 先关掉简易奔跑' : (m.run ? '重推奔跑: 开\n轻推摇杆=走路, 推过重推阈值=自动补一次松开再按下\n勾选后本条映射的 连发/简易奔跑 不生效' : '重推奔跑: 关\n开启后: 轻推摇杆=走路, 推过重推阈值=自动补一次松开再按下 (游戏判定双击→奔跑)\n仅对摇杆方向映射有效')) + '">' + (m.run ? '🏃 重推奔跑:开' : '🏃 重推奔跑') + '</button>' +
      '<button class="tgl-btn del" data-act="set-mapping-del" data-i="' + i + '">🗑 删除</button></div>' +
      '<div class="form-line" style="margin-bottom:0"><span class="t-12" style="color:var(--text-weak)">备注:</span>' +
      '<input class="slot-note-input" value="' + (m.note || '') + '" placeholder="输入备注文字..."></div>' +
      '</div>';
  }).join('');
  /* 新增表单回显 */
  var nt = $('#set-new-trigger'); if (nt) nt.textContent = setNew.trig || '点击设置按键';
  var tg = $('#set-new-target'); if (tg) tg.textContent = setNew.targets.length ? truncateChars(setNew.targets.join('+'), 9) : '点击设置按键';
  var chips = $('#set-new-chips');
  if (chips) chips.innerHTML = setNew.targets.map(function (t, ti) {
    return '<button class="evt-chip" data-act="set-new-chip-del" data-ti="' + ti + '" title="🗑 点击移除">' + t + ' ✕</button>';
  }).join('');
  var noteRow = $('#set-new-note-row'); if (noteRow) noteRow.hidden = !setNew.trig;
  var tbtn = document.querySelector('[data-act="set-turbo"]');
  if (tbtn) {
    tbtn.className = 'tgl-btn' + (setNew.turbo ? ' on-accent' : '');
    tbtn.textContent = setNew.turbo ? '⚡ 连发' : '○ 连发';
    tbtn.title = setNew.turbo ? '连发开启 - 自动重复输入' : '连发关闭 - 仅单次输入';
  }
  var dbtn = document.querySelector('.set-mapping-row [data-act="set-doubletap"]');
  if (dbtn) {
    dbtn.className = 'tgl-btn' + (setNew.dtap ? ' on-good' : '');
    dbtn.textContent = setNew.dtap ? '简易奔跑:开' : '简易奔跑';
  }
}
function renderSetPresets() {
  var pick = $('#set-preset-pick');
  var mgr = $('#set-preset-mgr-row');
  if (mgr) mgr.hidden = !pick || pick.selectedIndex === 0;
}

/* ---------------- 演示数据开关 ---------------- */
function setDemo(on) {
  state.demo = !!on;
  var t = $('#demo-toggle'); if (t) t.checked = !!on;
  PAD_TARGETS = on ? JSON.parse(JSON.stringify(DEMO_PAD_TARGETS)) : {};
  TURBO_ROWS = on ? JSON.parse(JSON.stringify(DEMO_TURBO_ROWS)) : [JSON.parse(JSON.stringify(TURBO_BASE_ROW))];
  WL_ROWS = on ? JSON.parse(JSON.stringify(DEMO_WL_ROWS)) : [];
  KB_LIT = on ? JSON.parse(JSON.stringify(DEMO_KB_LIT)) : {};
  renderAll();
  toast(on ? '原型演示数据已填充 (19 chips / 6 条连发 / 3 条白名单 / 键盘亮键)' : '已回到原版默认态 (空映射 / 1 条连发 / 0 白名单)');
}
function renderAll() {
  mountPad();
  renderChips();
  renderTurboRows();
  renderKb();
  renderWl();
  renderAudioPresets();
  renderAudioMonitor();
  renderSetMappings();
  renderSetPresets();
}

/* ---------------- 滑杆（trailing fill 着色 + 数值回显） ---------------- */
function initSliders() {
  $all('input[type="range"][data-slider]').forEach(function (el) {
    if (el.dataset.slInit) return;
    el.dataset.slInit = '1';
    var upd = function () {
      var min = Number(el.min || 0), max = Number(el.max || 100);
      var p = ((Number(el.value) - min) / (max - min) * 100).toFixed(1) + '%';
      el.style.setProperty('--p', p);
      var val = el.parentNode.querySelector('.val');
      if (val && !val.hasAttribute('data-static')) val.textContent = el.value;
    };
    el.addEventListener('input', upd);
    upd();
  });
}

/* ---------------- 暂停 / 震动状态 ---------------- */
function setPauseUI() {
  var dot = $('#st-dot'), txt = $('#st-text'), btn = $('#btn-st-pause');
  if (dot) dot.classList.toggle('paused', state.paused);
  if (txt) txt.textContent = state.paused ? '已暂停' : '运行中';
  if (btn) {
    btn.title = state.paused ? '恢复连发' : '暂停连发';
    btn.innerHTML = '<svg><use href="#i-' + (state.paused ? 'play' : 'pause') + '"/></svg>';
  }
  var hd = $('#hero-dot'), hs = $('#hero-state'), hp = $('#btn-hero-pause');
  if (hd) hd.classList.toggle('paused', state.paused);
  if (hs) {
    hs.textContent = state.paused ? '已暂停' : '连发中';
    hs.classList.toggle('t-good', !state.paused);
    if (state.paused) { hs.style.color = 'var(--warn)'; } else { hs.style.color = ''; }
  }
  if (hp) hp.innerHTML = state.paused ? '▶&nbsp; 启动' : '⏸&nbsp; 暂停';
  var run = $('#qc-run');
  if (run) run.textContent = state.paused ? '连发 · 已暂停' : '连发 · 开启中';
}
function setVibUI() {
  var pill = $('#st-pill');
  if (pill) {
    pill.textContent = state.vib ? '震动 开' : '震动 关';
    pill.classList.toggle('off', !state.vib);
  }
  $all('#qc-vib, #hero-vibpill, #vib-pill').forEach(function (el) {
    el.textContent = el.id === 'hero-vibpill' || el.id === 'vib-pill'
      ? (state.vib ? '震动: 开' : '震动: 关')
      : (state.vib ? '震动 · 开启' : '震动 · 关闭');
    el.classList.toggle('on', state.vib);
    el.classList.toggle('mute', !state.vib);
  });
}

/* ---------------- 马达测试动画（1s tick 消费） ---------------- */
function tickMotors() {
  if (Date.now() >= state.motorTestUntil) return;
  $all('.motor-bar').forEach(function (bar, i) {
    var dot = bar.querySelector('.motor-dot');
    if (!dot) return;
    var v = 40 + Math.random() * 50;
    dot.style.left = v + '%';
    dot.style.background = i % 2 === 0 ? 'var(--motor-l)' : 'var(--motor-r)';
    var val = bar.parentNode.querySelector('.motor-val');
    if (val) val.textContent = String(Math.round(v));
  });
}

/* ---------------- fitStage（宽度定标铺满视口, 与 proto.js 同款; H 轮） ---------------- */
function fitStage() {
  /* H 轮（与 proto.js 同款）：宽度定标 + 弹性设计高度 + transform——
     铺满视口零 letterbox；CSS zoom 非整数倍率命中测试失准，弃用。
     1180×760 档 s=1（与 egui 像素对齐基准不变）。 */
  var s = window.innerWidth / 1180;
  var stage = document.getElementById('stage-root');
  if (!stage) return;
  stage.style.width = '1180px';
  stage.style.height = (window.innerHeight / s) + 'px';
  stage.style.transformOrigin = '0 0';
  stage.style.transform = s === 1 ? '' : 'scale(' + s + ')';
  stage.style.zoom = '';
}
window.addEventListener('resize', fitStage);

/* ---------------- data-act 委托 ---------------- */
var ACTS = {
  'open-settings': function () { renderSetMappings(); renderSetPresets(); openDlg('dlg-settings'); },
  'close-dlg': function (el) { var ov = el.closest('.ovl'); if (ov) ov.classList.remove('open'); },
  'settings-save': function () { closeDlg('dlg-settings'); toast('配置已保存并立即生效'); },
  'open-about': function () { openDlg('dlg-about'); },
  'open-devices': function () { openDlg('dlg-devices'); },
  'open-hid': function () { hidState = 'press'; renderHid(); openDlg('dlg-hid'); },
  'toggle-dark': function () { applyTheme(!isLight(), true); },
  'show-guide': function () { toast('原型演示：使用说明向导（DFO 询问 → 版本询问 → 使用说明）'); },
  'classic-mode': function () { toast('原型演示：经典模式为 820×600 独立窗口（老宿主 1.5 三页签布局）'); },
  'minimal-mode': function () { toast('原型演示：极简模式为 430×520 小窗（连发/震动两个总开关）'); },
  'win-min': function () { toast('原型演示：最小化'); },
  'win-max': function () { toast('原型演示：最大化 / 还原'); },
  'win-close': function () { toast('原型演示：关闭 → 确认弹窗（最小化到托盘 / 退出程序 / 取消）'); },
  'win-hide': function () { toast('原型演示：最小化到托盘（任务栏按钮消失, 托盘图标可恢复）'); },
  'exit-app': function () { toast('原型演示：优雅退出（落盘 + 托盘/震动线程清理）'); },
  'switch-key-pick': function (el) { toast('切换键快捷选择：' + el.value + '（实际切换键以「捕获」捕获并保存为准）'); },
  'quit-app': function () { toast('原型演示：优雅退出（落盘 + 托盘/震动线程清理）'); },
  'toggle-pause': function () { state.paused = !state.paused; setPauseUI(); toast(state.paused ? 'Sorahk paused' : 'Sorahk activating'); },
  'toggle-run': function () { ACTS['toggle-pause'](); },
  'toggle-vib': function () { state.vib = !state.vib; setVibUI(); toast(state.vib ? '震动已开启' : '震动已关闭'); },
  'calibrate': function () { toast('原型演示：快速校对手柄按键 —— 24 步向导 (约 20 秒), 中途不能跳过'); },
  'start-identify': function () { toast('原型演示：手柄按键快速映射 —— 读取手柄后按手柄键快速设置'); },
  'chip-jump': function () { toast('原型演示：跳回该槽位编辑'); },
  'add-chord': function () { toast('原型演示：第 1/2 步 · 请同时按住 2 个及以上手柄按键, 然后一起松开'); },
  'demo-live': function () { demoLivePress(); },
  'preset-pick': function (el) { toast('预设切换：' + el.value + '（切换即保存并热重载）'); },
  'seg-general': function () { state.segJob = false; syncSeg(); },
  'seg-job': function () { state.segJob = true; syncSeg(); },
  'vib-preset-pick': function (el) { toast('震动预设「' + el.value + '」已应用'); },
  'toast-s4': function () { toast('S4 震动模块自动注入 · 已启用'); },
  'save-preset': function () { toast('预设已保存（名称留空 = 覆盖当前预设）'); },
  'capture-switch': function (el) {
    el.textContent = '请按任意键...';
    el.classList.add('capturing');
    var done = function (e) {
      if (e.key === 'Escape') { cleanup(); return; }
      var k = normKey(e); if (!k) return;
      cleanup();
      el.textContent = k;
      toast('已捕获：' + k);
    };
    var cleanup = function () {
      el.classList.remove('capturing');
      document.removeEventListener('keydown', done, true);
    };
    setTimeout(function () { document.addEventListener('keydown', done, true); }, 50);
  },
  'del-last': function () { toast('已移除末位键（演示）'); },
  'save-switch': function () { toast('切换键已保存'); },
  'clear-switch': function () { toast('切换键已清除'); },
  'kb-collapse': function (el) {
    var panel = $('#kb-panel'); if (!panel) return;
    var open = panel.style.display !== 'none';
    panel.style.display = open ? 'none' : '';
    el.setAttribute('aria-expanded', String(!open));
    var tri = el.querySelector('.tri'); if (tri) tri.textContent = open ? '▶' : '▼';
  },
  'kb-key': function (el) {
    var k = el.dataset.k;
    if (KB_LIT[k]) {
      el.classList.toggle('is-mark');
      toast('「' + k + '」已有同名连发 —— 标记移除 (再点取消, 确认后才真删)');
      return;
    }
    kbPick[k] = !kbPick[k];
    el.classList.toggle('is-pick', !!kbPick[k]);
    var pc = $('#kb-pickcount');
    if (pc) pc.textContent = '已选 ' + Object.keys(kbPick).filter(function (x) { return kbPick[x]; }).length + ' 键';
  },
  'kb-apply': function () { toast('原型演示：一次性生效（添加同名连发 + 移除标红键）'); kbPick = {}; renderKb(); },
  'kb-clear': function () { kbPick = {}; renderKb(); },
  'add-turbo': function () { toast('原型演示：新增映射置顶 + 行内编辑'); },
  'edit-turbo': function () { toast('原型演示：行内编辑卡（触发键/多目标/抬起键/序列宏/间隔时长…）'); },
  'wl-toggle': function () {
    var on = $('#wl-toggle').classList.contains('on');
    $('#wl-toggle').textContent = on ? '过滤 · 关闭' : '过滤 · 开启';
    $('#wl-toggle').classList.toggle('on', !on);
    $('#wl-sub').textContent = on ? '所有进程都会响应连发 (列表保留, 重开即恢复)' : '只有列表内的进程会响应连发';
  },
  'wl-mode': function () {
    var simple = $('#wl-mode').textContent.indexOf('简易') >= 0;
    $('#wl-mode').textContent = simple ? '匹配 · 精细' : '匹配 · 简易';
    $('#wl-mode-sub').textContent = simple
      ? '路径条目精确到完整路径 —— 同名不同版本的 exe 各归各 (双版本场景)'
      : '只认文件名 —— 条目与进程都按文件名比对, 换目录/换盘不失效 (旧版行为)';
  },
  'wl-add': function () {
    var input = $('#wl-input');
    var name = (input.value || '').trim();
    if (!name) { showWlError('请输入进程名'); return; }
    var dup = WL_ROWS.some(function (w) { return w.name.toLowerCase() === name.toLowerCase(); });
    if (dup) { showWlError('「' + name + '」已在白名单中'); return; }
    WL_ROWS.push({ name: name, path: '' });
    input.value = '';
    showWlError('');
    renderWl();
    toast('已添加：' + name);
  },
  'wl-browse': function () { toast('原型演示：文件选择对话框（Executable *.exe）—— 登记完整路径'); },
  'wl-del': function (el) {
    var i = Number(el.dataset.i);
    if (WL_ROWS[i]) { var n = WL_ROWS[i].name; WL_ROWS.splice(i, 1); renderWl(); toast('已移除：' + n); }
  },
  'test-vib': function () { state.motorTestUntil = Date.now() + 1000; toast('测试震动 1 秒（演示：马达条会动）'); },
  'test-left': function () { state.motorTestUntil = Date.now() + 1000; toast('只测左马达'); },
  'test-right': function () { state.motorTestUntil = Date.now() + 1000; toast('只测右马达'); },
  'apply-vib': function () { toast('震动预设已应用'); },
  'save-cur': function () { toast('已保存为当前预设'); },
  'save-as': function () { toast('预设已另存'); },
  'del-preset': function () { toast('仅「默认」与「测试版(全0)」不可删除 (演示)'); },
  'export-vib': function () { toast('已导出当前设置（原型演示）'); },
  'import-vib': function () { toast('原型演示：从导出文件导入…'); },
  'restore-vib': function () { toast('已还原默认'); },
  's1-engine': function (el) {
    $all('.seg2 [data-act="s1-engine"]').forEach(function (b) { b.classList.toggle('on', b === el); });
    toast('S1 输出引擎模式: ' + el.textContent);
  },
  'apply-job': function () { toast('职业预设已应用：' + JOBS[jobBase].base + ' · ' + ($('#job-class') || {}).value); },
  'export-job': function () { toast('已导出当前职业（原型演示）'); },
  'import-job': function () { toast('原型演示：导入职业设置…'); },
  'job-base-pick': function (el) { jobBase = el.selectedIndex; renderJobClasses(); },
  'job-class-pick': function () { updateJobDesc(); },
  'audio-toggle': function () {
    state.audioOn = !state.audioOn;
    var sw = $('#audio-switch');
    if (sw) { sw.classList.toggle('on', state.audioOn); sw.setAttribute('aria-checked', String(state.audioOn)); }
    var t = $('#audio-title'); if (t) t.textContent = state.audioOn ? '音频震动已开启' : '音频震动已关闭';
    var lab = $('#audio-state-lab'); if (lab) lab.textContent = state.audioOn ? '已开启' : '已关闭';
    renderAudioMonitor();
    toast(state.audioOn ? '音频震动开启：马达跟着声音实时输出' : '音频震动已关闭');
  },
  'motor-self-check': function () { state.motorTestUntil = Date.now() + 500; toast('马达自检 (0.5 秒) —— 排障用：确认马达正常'); },
  'mask-calib': function () { toast('屏蔽校准 (3 秒)：请保持安静 —— 校准完成会把环境噪音测出来'); },
  'ab-classic': function () { syncAb(true); toast('A 经典模式（原手感，逐字节不变）'); },
  'ab-ai': function () { syncAb(false); toast('B AI 智能模式（远近高低自适应）'); },
  /* ★I 轮接线: C 事件模式（与 egui audio_page 三选一对齐; 命令出口在 afterAct 统一发） */
  'ab-c': function () { syncAb(false, true); toast('C 事件模式（BGM 筛选 + 事件包络）'); },
  'listen-all': function (el) {
    state.listenAll = !state.listenAll;
    el.classList.toggle('on', state.listenAll);
    el.setAttribute('aria-checked', String(state.listenAll));
    var badge = $('#audio-wl-badge');
    if (badge) badge.textContent = state.listenAll ? '使用中 · 名单内' : '未启用 · 全局听';
  },
  'src-turbo': function () { toast('与「连发映射」页的白名单共用同一份列表（只读，不会改它）。推荐。'); },
  'src-custom': function () { toast('原型演示：单独填一份（DNF.exe, E:\\Games\\DFO.exe（逗号分隔，可填完整路径））'); },
  'audio-preset': function (el) {
    var tiles = $all('#audio-presets .audio-tile');
    tiles.forEach(function (b) { b.classList.remove('on'); var tk = b.querySelector('.tick'); if (tk) tk.remove(); });
    el.classList.add('on');
    var nm = el.querySelector('.nm');
    if (nm && !nm.querySelector('.tick')) {
      var tk = document.createElement('span'); tk.className = 'tick'; tk.textContent = '✓'; nm.appendChild(tk);
    }
    audioPresetSel = tiles.indexOf(el);
    renderAudioPresets();
    toast('预设「' + AUDIO_PRESETS[audioPresetSel].name + '」已应用');
  },
  'audio-status': function (el) {
    audioStatus = Number(el.dataset.i);
    $all('#audio-monitor .state-chip').forEach(function (b) { b.classList.toggle('on', Number(b.dataset.i) === audioStatus); });
    toast('引擎状态: ' + AUDIO_STATUS[audioStatus]);
  },
  'mod-apply': function (el, ev) {
    if (ev && ev.preventDefault) ev.preventDefault();
    toast('正在应用主题包…');
    setTimeout(function () { location.href = 'endfield.html'; }, 600);
  },
  /* ---- 捕获流 ---- */
  'gp-set-kb': function () { gpBeginKb(); },
  'gp-set-pad': function () { gpBeginPad(); },
  'gp-mouse': function () { toast('原型演示：鼠标映射（滚动 / 点击）→ 结果写入本槽位目标'); },
  'gp-clear': function () {
    var s = findSlot(gpFlow.slot); if (!s) return;
    delete PAD_TARGETS[shortKey(s)];
    mountPad(); renderChips();
    gpFlow.mode = 'selected'; renderGpFlow();
    toast('已清空「' + s.label + '」的键盘按键');
  },
  'gp-del': function () {
    var s = findSlot(gpFlow.slot); if (!s) return;
    gpFlow.mode = 'confirm'; gpFlow.kind = 'delete'; gpFlow.keys = [];
    renderGpFlow();
    var host = $('#gp-slot-panel');
    if (host) host.innerHTML =
      '<div class="mt8 bold" style="font-size:15px;color:var(--warn)">确定删除「' + s.label + '」的映射?</div>' +
      '<div class="t-12 t-hint mt4">删除后需要重新设置才能恢复。</div>' +
      '<div class="row mt12"><button class="btn danger" data-act="gp-del-yes">删除</button>' +
      '<button class="btn secondary" data-act="gp-cancel">取消</button></div>';
  },
  'gp-del-yes': function () {
    var s = findSlot(gpFlow.slot); if (!s) return;
    delete PAD_TARGETS[shortKey(s)];
    mountPad(); renderChips();
    gpFlow = { mode: 'idle', slot: null, keys: [], kind: 'target' };
    renderGpFlow();
    toast('已删除「' + s.label + '」的映射');
  },
  'gp-cancel': function () {
    /* 取消: 只回退一层 —— 确认页/捕获页 → 第 1 步 */
    gpFlow.mode = gpFlow.mode === 'confirm' ? 'selected' : (gpFlow.slot != null ? 'selected' : 'idle');
    if (gpFlow.mode === 'selected' && gpFlow.slot == null) gpFlow.mode = 'idle';
    renderGpFlow();
  },
  'gp-finish': function () {
    gpFlow = { mode: 'idle', slot: null, keys: [], kind: 'target' };
    renderGpFlow();
  },
  'gp-confirm': function () { gpConfirm(); },
  'gp-more': function () { gpFlow.mode = 'awaitkb'; renderGpFlow(); toast('继续按键盘, 组成组合键 (如 CTRL+F6)'); },
  'gp-edit-again': function () { gpFlow.mode = 'selected'; renderGpFlow(); },
  /* ---- 序列宏 ---- */
  'seq-rec': function () {
    if (seqRecording) {
      seqRecording = false;
      SEQ.push({ k: 'J', ms: 20 });
      SEQ.push({ wait: 45 });
      SEQ.push({ k: 'K', ms: 20 });
      renderSeq();
      toast('✓ 已填入并生效 (下方步骤列表)');
    } else {
      seqRecording = true;
      renderSeq();
    }
  },
  'seq-step': function () { seqAddKey('tap'); },
  'seq-press': function () { seqAddKey('down'); },
  'seq-release': function () { seqAddKey('up'); },
  'seq-wait': function () { SEQ.push({ wait: 100 }); renderSeq(); },
  'seq-20ms': function () { SEQ.forEach(function (st) { if (!st.wait) st.ms = 20; }); renderSeq(); toast('全部步骤时长 = 20ms'); },
  'seq-clear': function () { SEQ = []; renderSeq(); toast('已清空序列 (退回普通目标键模式)'); },
  'seq-del': function (el) { SEQ.splice(Number(el.dataset.i), 1); renderSeq(); },
  'seq-broken': function () { SEQ.push({ name: '连招A', bad: true }); renderSeq(); toast('原型演示：注入引用不存在宏的坏步骤 → 解析失败红条 + 🔒接管明示'); },
  /* ---- 设置弹窗 ---- */
  'set-preset-pick': function (el) { renderSetPresets(); if (el.selectedIndex > 0) toast('预设「' + el.value + '」已应用 (空预设合法: 零映射照常生效)'); },
  'set-preset-save': function () { var r = $('#set-preset-name-row'); if (r) r.hidden = false; },
  'set-preset-confirm': function () { var r = $('#set-preset-name-row'); if (r) r.hidden = true; toast('预设已保存'); },
  'set-preset-cancel': function () { var r = $('#set-preset-name-row'); if (r) r.hidden = true; },
  'set-preset-del': function () { renderSetPresets(); toast('预设已删除 (默认与测试版(全0)不可删——此为映射预设, 演示)'); },
  'set-preset-ren': function () { var r = $('#set-preset-ren-row'); if (r) r.hidden = false; },
  'set-preset-ren-ok': function () { var r = $('#set-preset-ren-row'); if (r) r.hidden = true; toast('已重命名'); },
  'set-preset-ren-cancel': function () { var r = $('#set-preset-ren-row'); if (r) r.hidden = true; },
  'set-cap-trigger': function (el) {
    el.textContent = '请按任意键...'; el.classList.add('capturing');
    var done = function (e) {
      if (e.key === 'Escape') { cleanup(); renderSetMappings(); return; }
      var k = normKey(e); if (!k) return;
      var i = Number(el.dataset.i);
      SET_MAPPINGS[i].trig = k; cleanup(); renderSetMappings();
    };
    var cleanup = function () { el.classList.remove('capturing'); document.removeEventListener('keydown', done, true); };
    setTimeout(function () { document.addEventListener('keydown', done, true); }, 50);
  },
  'set-cap-target': function (el) {
    var i = Number(el.dataset.i);
    toast('捕获中: 按键盘键追加目标 (Esc 取消)');
    var done = function (e) {
      if (e.key === 'Escape') { cleanup(); return; }
      var k = normKey(e); if (!k) return;
      if (SET_MAPPINGS[i].targets.indexOf(k) < 0) SET_MAPPINGS[i].targets.push(k);
      cleanup(); renderSetMappings();
    };
    var cleanup = function () { document.removeEventListener('keydown', done, true); };
    setTimeout(function () { document.addEventListener('keydown', done, true); }, 50);
  },
  'set-target-clear': function (el) { SET_MAPPINGS[Number(el.dataset.i)].targets = []; renderSetMappings(); },
  'set-chip-del': function (el) { SET_MAPPINGS[Number(el.dataset.i)].targets.splice(Number(el.dataset.ti), 1); renderSetMappings(); },
  'set-tgl': function (el) {
    var i = Number(el.dataset.i), f = el.dataset.f;
    if (f === 'dtap' && SET_MAPPINGS[i].run) { toast('已勾选「重推奔跑」, 两者互斥 —— 先关掉重推奔跑'); return; }
    if (f === 'run' && SET_MAPPINGS[i].dtap) { toast('已勾选「简易奔跑」, 两者互斥 —— 先关掉简易奔跑'); return; }
    SET_MAPPINGS[i][f] = !SET_MAPPINGS[i][f];
    renderSetMappings();
  },
  'set-mapping-del': function (el) { SET_MAPPINGS.splice(Number(el.dataset.i), 1); renderSetMappings(); },
  'set-mouse-dir': function () { toast('原型演示：鼠标移动方向（8 向 + 5 点击九宫格）'); },
  'set-mouse-scroll': function () { toast('原型演示：鼠标滚动方向'); },
  'set-new-trigger': function (el) {
    el.textContent = '请按任意键...'; el.classList.add('capturing');
    var done = function (e) {
      if (e.key === 'Escape') { cleanup(); renderSetMappings(); return; }
      var k = normKey(e); if (!k) return;
      setNew.trig = k; cleanup(); renderSetMappings();
    };
    var cleanup = function () { el.classList.remove('capturing'); document.removeEventListener('keydown', done, true); };
    setTimeout(function () { document.addEventListener('keydown', done, true); }, 50);
  },
  'set-new-target': function () {
    toast('捕获中: 按键盘键追加目标 (Esc 取消)');
    var done = function (e) {
      if (e.key === 'Escape') { cleanup(); return; }
      var k = normKey(e); if (!k) return;
      if (setNew.targets.indexOf(k) < 0) setNew.targets.push(k);
      cleanup(); renderSetMappings();
    };
    var cleanup = function () { document.removeEventListener('keydown', done, true); };
    setTimeout(function () { document.addEventListener('keydown', done, true); }, 50);
  },
  'set-new-target-clear': function () { setNew.targets = []; setNew.trig = ''; renderSetMappings(); },
  'set-new-chip-del': function (el) { setNew.targets.splice(Number(el.dataset.ti), 1); renderSetMappings(); },
  'set-turbo': function () { setNew.turbo = !setNew.turbo; renderSetMappings(); },
  'set-doubletap': function () { setNew.dtap = !setNew.dtap; renderSetMappings(); },
  'set-mapping-add': function () {
    if (!setNew.trig || !setNew.targets.length) { toast('触发键与目标键都设置后才能添加'); return; }
    var dup = SET_MAPPINGS.some(function (m) { return m.trig === setNew.trig.toUpperCase(); });
    if (dup) { var e2 = $('#set-dup-err'); if (e2) e2.hidden = false; return; }
    var e3 = $('#set-dup-err'); if (e3) e3.hidden = true;
    SET_MAPPINGS.push({ trig: setNew.trig.toUpperCase(), targets: setNew.targets.slice(), turbo: setNew.turbo, dtap: setNew.dtap, run: false, mouse: false, note: '' });
    setNew = { trig: '', targets: [], turbo: true, dtap: false };
    renderSetMappings();
    toast('映射已添加 (该触发键已存在时原版会红字拒绝)');
  },
  'set-wl-add': function () {
    var input = $('#set-wl-input');
    var name = (input.value || '').trim();
    if (!name) return;
    var dup = WL_ROWS.some(function (w) { return w.name.toLowerCase() === name.toLowerCase(); });
    if (dup) { var e4 = $('#set-wl-err'); if (e4) e4.hidden = false; return; }
    var e5 = $('#set-wl-err'); if (e5) e5.hidden = true;
    WL_ROWS.push({ name: name, path: '' });
    input.value = '';
    renderWl();
    toast('已添加并即时保存生效: ' + name);
  },
  'set-wl-del': function (el) { WL_ROWS.splice(Number(el.dataset.i), 1); renderWl(); },
  'set-wl-browse': function () { toast('原型演示：文件选择对话框 —— 登记完整路径'); },
  'open-update-panel': function () { toast('原型演示：更新完整面板（版本行/说明滚动区/基础引擎版与完全版/进度/忽略此版本）'); },
  'check-update': function () { toast('已是最新版本：R2.13（原型演示）'); },
  /* ---- 设备管理 ---- */
  'dev-refresh': function () { toast('已刷新设备列表（原型演示）'); },
  'dev-reset': function () { toast('原型演示：重置手柄连接 (蓝牙重连)'); },
  'dev-toggle-settings': function () { var p = $('#dev-settings-panel'); if (p) p.hidden = !p.hidden; },
  'dev-test': function () { state.motorTestUntil = Date.now() + 1000; toast('测试震动（设备透传演示）'); },
  'dev-stopall': function () { toast('已停止全部马达输出'); },
  'dev-reactivate': function () { toast('原型演示：重新建立 HID 基线'); },
  'dev-hid-filter': function (el) {
    var all = el.textContent.indexOf('全部') >= 0;
    el.textContent = all ? '🎮 仅游戏设备' : '🌸 全部设备';
    el.style.background = all ? 'var(--text-weak)' : 'var(--good)';
    /* 演示: 全部设备 = 显示游戏手柄 + 键盘; 仅游戏设备 = 只显示手柄。?hidempty=1 强制空态。 */
    var qs = new URLSearchParams(location.search);
    var empty = qs.get('hidempty') === '1';
    var list = $('#dev-hid-list'), emp = $('#dev-hid-empty'), empTxt = $('#dev-hid-empty-txt');
    if (list) list.hidden = empty || false;
    if (emp) emp.hidden = !empty;
    if (empTxt) empTxt.textContent = all ? '未检测到 HID 设备' : '未检测到游戏设备';
  },
  /* ---- HID 激活四态 ---- */
  'hid-next': function () {
    if (hidState === 'press') { hidState = 'release'; }
    else if (hidState === 'release') { hidState = 'success'; setTimeout(function () { closeDlg('dlg-hid'); }, 2000); }
    else if (hidState === 'failed') { hidState = 'press'; }
    renderHid();
  },
  'hid-retry': function () { hidState = 'press'; renderHid(); },
  'rename-preset': function () { toast('原型演示：重命名（两次确认防误删）'); },
  'mv-up': function () { toast('原型演示：映射上移'); },
  'mv-down': function () { toast('原型演示：映射下移'); },
  'del-mapping': function () { toast('原型演示：删除（二次确认）'); },
};

function showWlError(msg) {
  var el = $('#wl-error'); if (!el) return;
  el.textContent = msg; el.hidden = !msg;
}
function syncSeg() {
  $all('.seg2 [data-act="seg-general"], .seg2 [data-act="seg-job"]').forEach(function (b) {
    var on = (b.dataset.act === 'seg-job') === state.segJob;
    b.classList.toggle('on', on);
  });
  var row = $('#qc-preset-row');
  if (row) {
    var lab = row.querySelector('span');
    if (lab) lab.textContent = state.segJob ? '职业选择' : '通用预设';
    var sel = row.querySelector('select');
    if (sel) {
      sel.innerHTML = state.segJob
        ? '<option selected>鬼剑士（男）</option><option>格斗家（女）</option><option>神枪手（男）</option>'
        : '<option selected>默认</option><option>低频攻击职业</option><option>高振幅</option><option>节奏律动</option>';
    }
  }
}
/* ★I 轮接线: 三选一 A/B/C（isC=true 时点亮 C tab、隐藏等价开关行、显示 C 专属滑条） */
function syncAb(classic, isC) {
  $all('.ab-tab').forEach(function (b) {
    var on = isC ? b.dataset.act === 'ab-c' : (classic ? b.dataset.act === 'ab-classic' : b.dataset.act === 'ab-ai');
    b.classList.toggle('on', on);
  });
  var cb = $('#ai-on'); if (cb) cb.checked = !classic;
  var cbRow = $('#ai-on-row'); if (cbRow) cbRow.style.display = isC ? 'none' : '';
  var csl = $('#audio-c-sliders'); if (csl) csl.style.display = isC ? '' : 'none';
}

/* ---------------- HID 激活（hid_activation_dialog.rs 四态） ---------------- */
var hidState = 'press';
function renderHid() {
  var host = $('#hid-state'), title = $('#hid-title');
  if (!host || !title) return;
  var next = document.querySelector('[data-act="hid-next"]');
  if (hidState === 'press') {
    title.textContent = '🎮 ✨ 设备激活 ✨ 🎮'; title.className = 'hid-title';
    host.innerHTML = '<div class="hid-prompt">请按下一个按键喵~ (ﾉ◕ヮ◕)ﾉ*:･ﾟ✧</div>' +
      '<div class="hid-warn-box"><div class="wt">⚠️ 注意事项 ⚠️</div>' +
      '<div class="wl">• 只能按一个键哦！</div><div class="wl">• 不要同时按多个键</div><div class="wl">• 按下后记得松开~</div></div>';
    if (next) { next.hidden = false; next.textContent = '下一步（原型演示推进）'; }
  } else if (hidState === 'release') {
    title.textContent = '🎮 ✨ 设备激活 ✨ 🎮'; title.className = 'hid-title';
    host.innerHTML = '<div class="hid-prompt release">很好！现在松开按键吧~ ✧(｡•̀ᴗ-)✧</div>';
    if (next) { next.hidden = false; next.textContent = '下一步（原型演示推进）'; }
  } else if (hidState === 'success') {
    title.textContent = '🎉 激活成功！ 🎉'; title.className = 'hid-title ok';
    host.innerHTML = '<div class="hid-stars">✨ ⭐ 💫 🌟 ✨ ⭐ 💫 🌟</div>' +
      '<div class="t-16" style="color:var(--text)">设备激活完成！</div>' +
      '<div class="t-14 mt8" style="color:var(--text)">现在可以使用连发功能啦~ (｡♥‿♥｡)</div>' +
      '<div class="hid-auto-close">窗口即将自动关闭...</div>';
    if (next) next.hidden = true;
  } else {
    title.textContent = '❌ 激活失败 ❌'; title.className = 'hid-title fail';
    host.innerHTML = '<div class="t-14 mt8" style="color:var(--bad)">错误: 3 秒内未检测到按键差分 (原型演示失败样例)</div>' +
      '<button class="btn mt12" style="background:var(--accent);color:var(--on-emphasis);border-radius:12px;height:36px;min-width:140px;justify-content:center" data-act="hid-retry">🔄 重试</button>';
    if (next) next.hidden = true;
  }
}

function handleAct(act, el, ev) {
  var fn = ACTS[act];
  if (fn) { fn(el, ev); return true; }
  return false;
}

document.addEventListener('click', function (ev) {
  /* 侧栏导航（.nav-item[data-pg]）→ go()。注意 .page 分节也有 data-pg 属性,
     必须限定 .nav-item 前缀, 否则页面内点击会误命中分节元素 */
  var nav = ev.target.closest('.nav-item[data-pg]');
  if (nav) { go(nav.dataset.pg); return; }
  var el = ev.target.closest('[data-act]');
  if (el) {
    if (handleAct(el.dataset.act, el, ev) !== false) {
      if (el.tagName === 'BUTTON' && el.type !== 'submit') ev.preventDefault();
    }
    return;
  }
  var sw = ev.target.closest('[data-switch]');
  if (sw) { sw.classList.toggle('on'); return; }
  var scrim = ev.target.closest('.ovl');
  if (scrim && ev.target === scrim) scrim.classList.remove('open');
});

document.addEventListener('change', function (ev) {
  var el = ev.target;
  if (el.matches('select[data-act]')) handleAct(el.dataset.act, el, ev);
  if (el.id === 'vib-indep') {
    var on = el.checked;
    var badge = $('#vib-mode-badge');
    if (badge) {
      badge.textContent = on ? '调试中' : '正常模式';
      badge.classList.toggle('good', !on);
      badge.classList.toggle('warn', on);
    }
    var hint = $('#vib-mode-hint');
    if (hint) hint.textContent = on
      ? '调试模式已开启: 评分/移动通道不吃全局总调整, 便于逐项单独验证。'
      : '正常模式: 全局总调整统一作用于所有通道。需要单通道调试时再打开。';
  }
  if (el.id === 'job-indep') {
    /* 全职业页调试模式复选框（与震动页同款视觉语义, 本地 UI 行为） */
    var jon = el.checked;
    var jbadge = $('#job-mode-badge');
    if (jbadge) {
      jbadge.textContent = jon ? '调试中' : '正常模式';
      jbadge.classList.toggle('good', !jon);
      jbadge.classList.toggle('warn', jon);
    }
    var jhint = $('#job-mode-hint');
    if (jhint) jhint.textContent = jon
      ? '调试模式已开启: 评分/移动通道不吃全局总调整, 便于逐项单独验证。'
      : '正常模式: 全局总调整统一作用于所有通道。需要单通道调试时再打开。';
    toast(jon ? '职业页调试模式: 评分/移动通道独立于全局总调整' : '职业页回归正常模式: 全局总调整统一作用');
  }
  if (el.id === 'job-on') {
    /* 应用全职业预设开关 → 描述首行联动（本地 UI 行为） */
    var jdesc = $('#job-desc');
    if (jdesc) {
      var jcls = JOBS[jobBase].classes[($('#job-class') || {}).selectedIndex || 0] || JOBS[jobBase].classes[0];
      jdesc.innerHTML = (el.checked
        ? '已开启: 全职业预设应用中, 职业参数覆盖震动设置'
        : '开关已关闭: 全职业预设不应用, 使用通用型震动设定参数') + '<br>' + jcls[1];
    }
    toast(el.checked ? '全职业预设已启用（职业参数覆盖通用震动设置）' : '全职业预设已停用（恢复通用型震动设定参数）');
  }
  if (el.id === 'ai-on') {
    /* AI 开关与 A/B tab 等价联动（syncAb 会同步 tab 高亮与本框勾选态） */
    syncAb(!el.checked);
  }
  if (el.id === 'adv-allow') {
    var dis = !el.checked;
    ['#vib-adv', '#vib-rank', '#vib-lr-item', '#vib-lr-rank'].forEach(function (id) {
      var h = $(id);
      if (h) h.classList.toggle('adv-disabled', dis);
    });
    toast(el.checked ? '允许高级调校: 下方参数参与输出' : '已关闭高级调校: 下方参数全部不生效');
  }
  if (el.id === 'move-indep') toast(el.checked ? '角色移动持续震动已独立于全局强度开关' : '角色移动持续震动回归全局总阀');
  if (el.id === 'set-dark') applyTheme(!el.checked, true);
  if (el.id === 'set-dfo') {
    var row = $('#set-route-row');
    if (row) {
      var combo = row.querySelector('select');
      if (combo) combo.disabled = !el.checked;
      combo && (combo.style.opacity = el.checked ? '1' : '0.45');
    }
    toast(el.checked ? 'DFO 震动功能已开启 (震动中心/全职业预设可用)' : '仅用连发模式: 震动分区隐藏, 震动默认关闭');
  }
  if (el.id === 'demo-toggle') setDemo(el.checked);
});

document.addEventListener('keydown', function (ev) {
  /* 手柄页第 2 步: 等键盘按键（panel.rs AwaitKb —— 累加组合, Esc 取消） */
  if (gpFlow.mode === 'awaitkb') {
    if (ev.key === 'Escape') { gpFlow.mode = 'selected'; renderGpFlow(); ev.preventDefault(); return; }
    var gk = normKey(ev);
    if (gk) {
      gpFlow.keys.push(gk);
      gpFlow.mode = 'confirm';
      renderGpFlow();
      ev.preventDefault();
      return;
    }
    return;
  }
  /* 设置弹窗/序列宏等捕获态由各自 capture:true 监听器处理; 有捕获钮激活时全局快捷键让路 */
  if (document.querySelector('.cap-btn.capturing')) return;
  if (ev.key === 'Escape') {
    $all('.ovl.open').forEach(function (ov) { ov.classList.remove('open'); });
  }
  if (ev.ctrlKey && ev.key.toLowerCase() === 's') { ev.preventDefault(); toast('全部配置已落盘 (原型演示)'); }
  if (seqCapture && ev.key !== 'Escape') {
    var k = normKey(ev);
    if (k) {
      SEQ.push({ k: k, ms: 20, mode: seqCapture.mode });
      seqCapture = null;
      renderSeq();
      toast('已加入步骤: ' + k);
    }
    ev.preventDefault();
    return;
  }
  if (ev.key === 'Escape' && seqCapture) { seqCapture = null; toast('已取消捕获'); return; }
  /* 数字键 1-7 切页（原型便捷; 原版无此快捷键） */
  if (!ev.ctrlKey && !ev.altKey && /^[1-7]$/.test(ev.key) && !ev.target.matches('input, select, textarea')) {
    go(['gamepad', 'turbo', 'whitelist', 'vibration', 'jobs', 'audio', 'mod'][Number(ev.key) - 1]);
  }
});

/* ---------------- 启动 ---------------- */
function tick() {
  tickMotors();
  if (state.audioOn) tickMonitor();
}

function init() {
  var qs = new URLSearchParams(location.search);
  fitStage();
  /* 主题: URL 参数 > localStorage > 深色默认 */
  var t = qs.get('theme');
  var saved = null;
  try { saved = localStorage.getItem('replica-theme'); } catch (e) {}
  applyTheme(t ? t === 'light' : saved === 'light', false);
  renderVibAdv();
  renderVibRank();
  renderLR('#vib-lr-item', LR_ITEM);
  renderLR('#vib-lr-rank', LR_RANK);
  renderJobs();
  renderSeq();
  renderAll();
  initSliders();
  setPauseUI();
  setVibUI();
  setInterval(tick, 1000);
  /* 演示数据开关: URL ?demo=1 → 打开; 否则默认关 (= 原版截图空态) */
  if (qs.get('demo') === '1') setDemo(true);
  go(qs.get('p') || 'gamepad');
  var dlg = qs.get('dlg');
  if (dlg) {
    if (dlg === 'hid') { hidState = qs.get('hidstate') || 'press'; renderHid(); }
    openDlg('dlg-' + dlg);
    if (dlg === 'devices') {
      if (qs.get('hidempty') === '1') {
        var list = $('#dev-hid-list'), emp = $('#dev-hid-empty');
        if (list) list.hidden = true;
        if (emp) emp.hidden = false;
      }
      if (qs.get('devpanel') === '1') { var dp = $('#dev-settings-panel'); if (dp) dp.hidden = false; }
    }
  }
  /* 截图直达: gpflow=selected|kb|confirm|done (第 16 槽 X 键演示) */
  var gf = qs.get('gpflow');
  if (gf === 'selected') { gpFlow = { mode: 'selected', slot: 16, keys: [], kind: 'target' }; renderGpFlow(); }
  if (gf === 'kb') { gpFlow = { mode: 'awaitkb', slot: 16, keys: [], kind: 'target' }; renderGpFlow(); }
  if (gf === 'confirm') { gpFlow = { mode: 'confirm', slot: 16, keys: ['F'], kind: 'target' }; renderGpFlow(); }
  if (gf === 'done') { gpFlow = { mode: 'mapdone', slot: 16, keys: ['F'], kind: 'target' }; renderGpFlow(); }
  /* 截图直达: audioon=1 打开音频开关并渲染监测 */
  if (qs.get('audioon') === '1') {
    state.audioOn = true;
    var asw = $('#audio-switch'); if (asw) { asw.classList.add('on'); asw.setAttribute('aria-checked', 'true'); }
    var at = $('#audio-title'); if (at) at.textContent = '音频震动已开启';
    var al = $('#audio-state-lab'); if (al) al.textContent = '已开启';
    renderAudioMonitor();
  }
  /* 截图直达: seqdemo=1 正常步骤(🔒接管明示) / seqdemo=2 坏步骤(解析失败红条) */
  if (qs.get('seqdemo') === '1') {
    SEQ = [{ k: 'J', ms: 20, mode: 'down' }, { wait: 45 }, { k: 'K', ms: 20, mode: 'up' }];
    renderSeq();
  }
  if (qs.get('seqdemo') === '2') {
    SEQ = [{ k: 'J', ms: 20 }, { name: '连招A', bad: true }];
    renderSeq();
  }
  /* 截图直达: open=<details id 列表> 展开折叠区 */
  var openIds = qs.get('open');
  if (openIds) openIds.split(',').forEach(function (id) {
    var d = document.getElementById(id); if (d && d.tagName === 'DETAILS') d.open = true;
  });
  /* 截图直达: scrollto=<选择器> 滚到指定元素 (页面/弹窗内部滚动); dlgscroll=1 弹窗滚到底 */
  if (qs.get('dlgscroll') === '1') {
    var body = document.querySelector('#dlg-settings .dlg-body');
    if (body) body.scrollTop = body.scrollHeight;
  }
  var st = qs.get('scrollto');
  if (st) {
    try {
      var tgt = document.querySelector(st);
      if (tgt) setTimeout(function () { tgt.scrollIntoView({ block: 'start' }); }, 60);
    } catch (e) {}
  }
}

try {
  init();
} catch (err) {
  var el = document.createElement('pre');
  el.style.cssText = 'position:fixed;top:8px;left:8px;z-index:999;background:#3a1414;color:#fff;padding:10px 14px;border-radius:8px;font:12px Consolas,monospace;max-width:70vw;white-space:pre-wrap';
  el.textContent = 'INIT ERR\n' + (err && err.stack || err);
  document.body.appendChild(el);
}

/* ============================================================
   v12 桥绑定层（双UI桥契约 v1 §2-§4 / §7）
   · 快照 → DOM 单向绑定（按域分函数）；mock 与真桥走同一条渲染路径
   · 命令出口：data-act → SorahkHost.call({t:"cmd", act, params?})
   · cfg.write 动态发现白名单表（cfg.paths，不猜路径）+ 滑杆去抖
   · 纯 mock（file:// 直开）行为不变：绑定层只做状态回显（等值 no-op）
     与快照推送；BIND 模式（真桥 / ?bridge=mock）才启用新交互与动态锚点
============================================================ */
(function () {
  var H = window.SorahkHost;
  if (!H) return;
  var BIND = !!H.binding;
  var REAL = !!H.isReal;
  var lastSnap = null;
  var rawCapActive = false;
  var draggingSlider = null;

  function eid(id) { return document.getElementById(id); }
  function $all(sel) { return Array.prototype.slice.call(document.querySelectorAll(sel)); }
  function escHtml(s) {
    return String(s == null ? '' : s).replace(/[&<>"]/g, function (c) {
      return { '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' }[c];
    });
  }

  /* ---------------- 动态锚点（仅 BIND 模式创建，纯 mock 不动 v11 像素） ---------------- */
  var anchors = null;
  function ensureAnchors() {
    if (!BIND) return null;
    if (anchors) return anchors;
    var side = document.querySelector('.side-status');
    var badge = document.createElement('div');
    badge.id = 'bridge-badge';
    badge.style.cssText = 'margin-top:10px;font-size:10.5px;color:var(--text-weak);letter-spacing:.4px;line-height:1.6';
    badge.hidden = true;
    var stopped = document.createElement('div');
    stopped.id = 'stopped-bar';
    stopped.style.cssText = 'margin-top:6px;font-size:10.5px;color:var(--warn);line-height:1.5';
    stopped.hidden = true;
    if (side) { side.appendChild(badge); side.appendChild(stopped); }
    var upd = document.createElement('div');
    upd.id = 'update-bar';
    upd.style.cssText = 'position:fixed;right:14px;bottom:14px;z-index:900;background:rgba(20,24,34,.92);color:var(--text);border:1px solid var(--stroke);border-radius:8px;padding:8px 12px;font-size:12px;max-width:340px;display:none';
    document.body.appendChild(upd);
    var cap = document.createElement('div');
    cap.id = 'raw-cap-bar';
    cap.style.cssText = 'position:fixed;left:50%;top:12px;transform:translateX(-50%);z-index:900;background:rgba(255,190,60,.92);color:#201400;border-radius:8px;padding:6px 14px;font-size:12px;font-weight:600;display:none';
    document.body.appendChild(cap);
    var st = document.createElement('style');
    st.textContent = '#pad-art g.hot.is-live > circle[stroke]{stroke-width:3.2 !important}';
    document.head.appendChild(st);
    var calib = document.createElement('div');
    calib.id = 'audio-calib-bar';
    calib.style.cssText = 'display:none;margin:8px 0;padding:8px 10px;border:1px solid var(--warn);border-radius:8px;font-size:12px;color:var(--text)';
    var ap = eid('audio-presets');
    if (ap && ap.parentNode) ap.parentNode.insertBefore(calib, ap.nextSibling);
    anchors = { badge: badge, stopped: stopped, upd: upd, cap: cap, calib: calib };
    return anchors;
  }

  /* ---------------- 键名 / 位图映射（照 state/types.rs 注释） ---------------- */
  var VK_NAMES = { 8: 'BACKSPACE', 9: 'TAB', 13: 'ENTER', 16: 'LSHIFT', 17: 'LCTRL', 18: 'LALT', 20: 'CAPSLOCK', 27: 'ESC', 32: 'SPACE', 33: 'PAGEUP', 34: 'PAGEDOWN', 35: 'END', 36: 'HOME', 37: 'LEFT', 38: 'UP', 39: 'RIGHT', 40: 'DOWN', 45: 'INSERT', 46: 'DELETE' };
  (function () { for (var i = 1; i <= 12; i++) VK_NAMES[111 + i] = 'F' + i; })();
  function vkName(vk) {
    vk = Number(vk) | 0;
    if (VK_NAMES[vk]) return VK_NAMES[vk];
    if (vk >= 65 && vk <= 90) return String.fromCharCode(vk);
    if (vk >= 48 && vk <= 57) return String.fromCharCode(vk);
    if (vk >= 96 && vk <= 105) return 'NUMPAD' + (vk - 96);
    return 'VK' + vk;
  }
  /* XInput 按钮位图 → SLOTS label */
  var XI_BITS = { 1: '十字键·上', 2: '十字键·下', 4: '十字键·左', 8: '十字键·右', 16: 'Start', 32: 'Back', 64: '左摇杆·按下', 128: '右摇杆·按下', 256: 'LB', 512: 'RB', 4096: 'A 键', 8192: 'B 键', 16384: 'X 键', 32768: 'Y 键' };
  var RAW_MODE_MAP = { '智能自适应': 'AdaptiveIntelligent', '持续时间最长': 'MostSustained', '最大变化量': 'MaxChangedBits', '最大激活量': 'MaxSetBits', '最终稳定状态': 'LastStable', '摇杆方向优化': 'HatSwitchOptimized', '模拟摇杆优化': 'AnalogOptimized' };
  var XI_MODE_MAP = { '持续时间最长': 'MostSustained', '最终稳定状态': 'LastStable', '斜方向优先': 'DiagonalPriority' };

  /* ---------------- 快照 → DOM（按域分函数） ---------------- */
  function applyCore(s) {
    var c = s.core || {};
    if (c.paused !== undefined && c.paused !== state.paused) { state.paused = c.paused; setPauseUI(); }
    if (c.language !== undefined && BIND) {
      var sel = eid('set-language');
      if (sel && sel.value !== c.language) {
        for (var i = 0; i < sel.options.length; i++) if (sel.options[i].value === c.language) sel.selectedIndex = i;
      }
    }
    if (c.vib_legacy_client !== undefined && BIND) {
      var rt = document.querySelector('[data-act="route-pick"]');
      if (rt) rt.selectedIndex = c.vib_legacy_client ? 1 : 0;
    }
    if (BIND) {
      var a = ensureAnchors();
      if (a && a.badge) {
        var bits = [];
        if (s.version) bits.push(s.version);
        if (c.config_epoch !== undefined) bits.push('配置代次 ' + c.config_epoch);
        if (c.vib_legacy_client !== undefined) bits.push(c.vib_legacy_client ? 'S1 ACT 老方案' : 'S4+ 通用');
        a.badge.textContent = bits.join(' · ');
        a.badge.hidden = !bits.length;
      }
      if (a && a.stopped) {
        var st = (c.stopped_threads || []);
        a.stopped.textContent = st.length ? '⚠ 看护线程已放弃重启: ' + st.join('、') : '';
        a.stopped.hidden = !st.length;
      }
    }
  }

  function pressedLabels(s) {
    var inp = s.input || {};
    var pressed = [];
    if (inp.live_xinput) {
      var bm = Number(inp.live_xinput[1]) || 0;
      for (var b in XI_BITS) if (bm & Number(b)) pressed.push(XI_BITS[b]);
    }
    if (inp.live_hid) {
      var lh = inp.live_hid;
      if (lh.lt) pressed.push('LT');
      if (lh.rt) pressed.push('RT');
      var DP = { 1: '十字键·上', 3: '十字键·右', 5: '十字键·下', 7: '十字键·左' };
      if (DP[lh.dpad]) pressed.push(DP[lh.dpad]);
      var AX = { 1: ['左摇杆·左', '右摇杆·左'], 2: ['左摇杆·右', '右摇杆·右'], 4: ['左摇杆·上', '右摇杆·上'], 8: ['左摇杆·下', '右摇杆·下'] };
      for (var k in AX) {
        if (lh.ls & Number(k)) pressed.push(AX[k][0]);
        if (lh.rs & Number(k)) pressed.push(AX[k][1]);
      }
    }
    return pressed;
  }
  function applyInput(s) {
    var inp = s.input || {};
    rawCapActive = !!inp.raw_capture_active;
    var pressed = pressedLabels(s);
    $all('#pad-art g.hot').forEach(function (g) {
      var slot = findSlot(Number(g.dataset.slot));
      g.classList.toggle('is-live', !!(slot && pressed.indexOf(slot.label) >= 0));
    });
    if (BIND) {
      var a = ensureAnchors();
      if (a && a.cap) {
        if (rawCapActive) {
          var mode = inp.raw_capture_mode === 'MaxSetBits' || inp.raw_capture_mode === 'GenericDevice' ? '手柄键' : '键盘键';
          a.cap.textContent = '● RawInput 捕获中（' + (inp.raw_capture_mode || '') + '）— 按 ' + mode + '，Esc 取消';
          a.cap.style.display = 'block';
        } else a.cap.style.display = 'none';
      }
    }
  }

  function sliderVisual(sl) {
    var min = Number(sl.min || 0), max = Number(sl.max || 100);
    var p = ((Number(sl.value) - min) / (max - min) * 100).toFixed(1) + '%';
    sl.style.setProperty('--p', p);
    var val = sl.parentNode.querySelector('.val');
    if (val && !val.hasAttribute('data-static')) val.textContent = sl.value;
  }
  function echoSliders(v) {
    if (!BIND) return;
    $all('input[type="range"][data-cfg-path]').forEach(function (sl) {
      if (sl === draggingSlider) return;
      var path = sl.dataset.cfgPath, val = null;
      if (path.indexOf('vibration.item_lr.') === 0) { var i = Number(path.slice(18)); val = (v.item_lr || [])[i * 2]; }
      else if (path.indexOf('vibration.rank_lr.') === 0) { var j = Number(path.slice(19)); val = (v.rank_lr || [])[j * 2]; }
      else if (path.indexOf('vibration.rank_type_gain.') === 0) { var k = Number(path.slice(26)); val = (v.rank_type_gain || [])[k]; }
      if (val === null || val === undefined) return;
      var s = String(val);
      if (sl.value !== s) { sl.value = s; sliderVisual(sl); }
    });
  }
  function applyVib(s) {
    var v = s.vibration || {};
    if (v.enabled !== undefined && v.enabled !== state.vib) { state.vib = v.enabled; setVibUI(); }
    if (v.advanced_enabled !== undefined && BIND) {
      var cb = eid('adv-allow');
      if (cb && cb.checked !== v.advanced_enabled) { cb.checked = v.advanced_enabled; cb.dispatchEvent(new Event('change', { bubbles: true })); }
    }
    if (BIND) {
      var pctL = Math.max(0, Math.min(100, (Number(v.out_l) || 0) / 65535 * 100));
      var pctR = Math.max(0, Math.min(100, (Number(v.out_r) || 0) / 65535 * 100));
      $all('.motor-bar').forEach(function (bar, i) {
        var dot = bar.querySelector('.motor-dot'); if (!dot) return;
        var pct = i % 2 === 0 ? pctL : pctR;
        dot.style.left = pct.toFixed(1) + '%';
        dot.style.background = i % 2 === 0 ? 'var(--motor-l)' : 'var(--motor-r)';
        var val = bar.parentNode.querySelector('.motor-val');
        if (val) val.textContent = String(Math.round(pct));
      });
      var testing = !!(v.test_until_ms && v.test_until_ms > (v.now_ms || Date.now()));
      document.body.classList.toggle('vib-testing', testing);
      echoSliders(v);
    }
  }

  var MODE_BANNERS = [
    ['game', '🎮', '当前判定：游戏音效段 —— 正常跟手震'],
    ['music', '♪', '当前判定：背景音乐段 —— 连续震动被压住，只有鼓点（节奏力度）和够响的音效会震'],
    ['quiet', '🔇', '当前判定：安静段 —— 手柄保持安静，不会乱震'],
    ['vocal', '🎤', '当前判定：人声/细节段 —— 轻微细腻的反馈，不会吵'],
  ];
  function applyAudio(s) {
    var a = s.audio || {};
    if (a.enabled !== undefined && a.enabled !== state.audioOn) {
      state.audioOn = a.enabled;
      var sw = eid('audio-switch');
      if (sw) { sw.classList.toggle('on', a.enabled); sw.setAttribute('aria-checked', String(a.enabled)); }
      var t = eid('audio-title'); if (t) t.textContent = a.enabled ? '音频震动已开启' : '音频震动已关闭';
      var lab = eid('audio-state-lab'); if (lab) lab.textContent = a.enabled ? '已开启' : '已关闭';
      renderAudioMonitor();
    }
    if (!BIND || !state.audioOn) return;
    var idx = AUDIO_STATUS.indexOf(a.status || '');
    if (idx >= 0 && idx !== audioStatus) {
      audioStatus = idx;
      $all('#audio-monitor .state-chip').forEach(function (b) { b.classList.toggle('on', Number(b.dataset.i) === idx); });
    }
    var fills = $all('#audio-monitor .mbar .mfill');
    var ml = Math.max(0, Math.min(100, (Number(a.motor_l) || 0) / 65535 * 100));
    var mr = Math.max(0, Math.min(100, (Number(a.motor_r) || 0) / 65535 * 100));
    if (fills[0]) { fills[0].style.width = ml.toFixed(0) + '%'; fills[0].style.opacity = '0.85'; }
    if (fills[1]) { fills[1].style.width = mr.toFixed(0) + '%'; fills[1].style.opacity = '0.85'; }
    var cols = $all('#spec-cols .spec-col');
    (a.bands || []).forEach(function (v, i) {
      var col = cols[i]; if (!col) return;
      var bar = col.querySelector('.bar'), peak = col.querySelector('.peak');
      var pct = Math.max(0, Math.min(97, Number(v) * 100));
      if (bar) bar.style.height = pct.toFixed(0) + '%';
      if (peak) peak.style.bottom = Math.min(pct + 8, 97).toFixed(0) + '%';
    });
    var n = eid('ms-noise'), th = eid('ms-thresh'), tr = eid('ms-trans');
    if (n) n.textContent = ((Number(a.noise_floor) || 0) * 100).toFixed(0) + '%';
    if (th) th.textContent = ((Number(a.threshold_eff) || 0) * 100).toFixed(0) + '%';
    if (tr) tr.textContent = ((Number(a.transient) || 0) * 100).toFixed(0) + '%';
    var banner = document.querySelector('#audio-monitor .mode-banner');
    if (banner && a.mode !== undefined) {
      var m = MODE_BANNERS[(Number(a.mode) || 0) % 4];
      banner.className = 'mode-banner ' + m[0];
      banner.innerHTML = '<span>' + m[1] + '</span><span>' + m[2] + '</span>';
    }
    var anc = ensureAnchors();
    if (anc && anc.calib) {
      var cr = Number(a.calib_result) || 0;
      if (cr > 0) {
        anc.calib.style.display = 'block';
        anc.calib.innerHTML = '校准完成：声音门槛建议 <b>' + cr + '</b>（低于门槛的杂音不震）。' +
          '<button class="btn primary small" data-act="audio-calib-apply">应用建议</button> ' +
          '<button class="btn secondary small" data-act="audio-calib-dismiss">忽略</button>';
      } else anc.calib.style.display = 'none';
    }
  }

  function applyUpdate(s) {
    var u = s.update || {};
    if (!BIND) return;
    var a = ensureAnchors(); if (!a) return;
    var html = '';
    if (u.phase === 'checking') html = '⟳ 检查更新中…';
    else if (u.error) html = '<span style="color:var(--bad)">✗ 更新失败: ' + escHtml(u.error) + '</span>';
    else if (Number(u.total) > 0 && Number(u.done) < Number(u.total)) html = '↓ 下载中 ' + Math.round(Number(u.done) / Number(u.total) * 100) + '%';
    else if (u.latest_version) html = '↑ 发现新版本 ' + escHtml(u.latest_version) + '（可到「关于」查看详情）';
    a.upd.innerHTML = html;
    a.upd.style.display = html ? 'block' : 'none';
  }

  function applyCounts(s) {
    var c = s.counts || {};
    if (c.turbo !== undefined) {
      var tc = eid('turbo-count'); if (tc) tc.textContent = c.turbo + ' 条';
      var hc = eid('hero-count'); if (hc) hc.textContent = String(c.turbo);
    }
    if (c.whitelist !== undefined) {
      var wc = eid('wl-count'); if (wc) wc.textContent = c.whitelist + ' 个';
    }
    if (c.mappings !== undefined && BIND) {
      var tt = eid('gp-set-title');
      if (tt && tt.textContent.indexOf('(') >= 0) tt.textContent = '我设好的映射 (' + c.mappings + ')';
    }
  }

  var applyLog = [];
  function applySnapshot(snap) {
    lastSnap = snap;
    try { applyCore(snap); applyInput(snap); applyVib(snap); applyAudio(snap); applyUpdate(snap); applyCounts(snap); }
    catch (e) { applyLog.push('ERR ' + (e && e.message || e) + ' @ epoch=' + (snap.core || {}).config_epoch); }
    if (window.__BRIDGE_DBG__) {
      applyLog.push('epoch=' + (snap.core || {}).config_epoch + ' ver=' + snap.version + ' paused=' + (snap.core || {}).paused);
      clearTimeout(window.__APPLY_LOG_T__);
      window.__APPLY_LOG_T__ = setTimeout(function () {
        var pre = document.getElementById('apply-log') || document.createElement('pre');
        pre.id = 'apply-log';
        pre.textContent = applyLog.join('\n') + '\n--ACT--\n' + (window.__ACT_PROBE__ || []).join('\n');
        document.body.appendChild(pre);
      }, 100);
    }
  }

  /* ---------------- mock 本地状态机 → 快照推送器（同一渲染路径） ---------------- */
  function pushLocal() {
    if (REAL) return;
    H.pushSnapshot({
      core: { paused: state.paused },
      vibration: { enabled: state.vib },
      audio: { enabled: state.audioOn },
      counts: {
        mappings: Object.keys(PAD_TARGETS).length, turbo: TURBO_ROWS.length,
        whitelist: WL_ROWS.length, macros: SEQ.length, combos: 0, presets: 0,
      },
    });
  }

  /* ---------------- cfg / 命令出口 ---------------- */
  var cfgCache = null;
  function prefetchCfg() {
    H.cfg.get().then(function (r) { if (r && r.data) cfgCache = r.data; }).catch(function () {});
    H.cfg.paths().catch(function () {});
  }
  function findMappingIndex(trig) {
    if (!cfgCache) return null;
    var ms = cfgCache.mappings || [];
    for (var i = 0; i < ms.length; i++) if (ms[i] && ms[i].trigger_key === trig) return i;
    return null;
  }
  function encU32(v) { v = Number(v); return v < 0 ? v + 4294967296 : v; }
  function encVal(path, value) {
    value = Number(value);
    if (path.indexOf('vibration.item_lr.') === 0 || path.indexOf('vibration.rank_lr.') === 0) return encU32(value);
    return value;
  }
  function cfgWrite(path, value, commit) {
    return H.cfg.write([{ path: path, value: value }], commit).catch(function () {});
  }
  function buildMapping(trig, targets, opts) {
    opts = opts || {};
    return {
      trigger_key: trig, target_keys: targets,
      turbo_enabled: !!opts.turbo, double_tap_enabled: !!opts.dtap,
      sequence_text: opts.seq || '', note: opts.note || '',
    };
  }

  /* 滑杆 → cfg.write：拖动 memory 300ms 去抖，松手按族档位补正式 write（§4 映射） */
  var sliderTimers = {};
  function wireSliders() {
    document.addEventListener('input', function (ev) {
      var s = ev.target;
      if (!s.matches || !s.matches('input[type="range"][data-cfg-path]')) return;
      draggingSlider = s;
      var path = s.dataset.cfgPath;
      clearTimeout(sliderTimers[path]);
      sliderTimers[path] = setTimeout(function () {
        sliderTimers[path] = null;
        cfgWrite(path, encVal(path, s.value), 'memory');
      }, 300);
    });
    document.addEventListener('change', function (ev) {
      var s = ev.target;
      if (!s.matches || !s.matches('input[type="range"][data-cfg-path]')) return;
      if (draggingSlider === s) draggingSlider = null;
      var path = s.dataset.cfgPath;
      var val = encVal(path, s.value);
      if (sliderTimers[path]) { clearTimeout(sliderTimers[path]); sliderTimers[path] = null; cfgWrite(path, val, 'memory'); }
      var tier = H.cfg.tier(path) || {};
      cfgWrite(path, val, tier.settle || 'config-only');
    });
    /* 滑杆打标：评分细分 10 / 逐项 LR 26+30（照 hints.rs 语义，快照 item_lr[i*2] = L） */
    $all('#vib-rank input[type="range"][data-slider]').forEach(function (s, i) { s.dataset.cfgPath = 'vibration.rank_type_gain.' + i; });
    $all('#vib-lr-item input[type="range"][data-slider]').forEach(function (s, i) { s.dataset.cfgPath = 'vibration.item_lr.' + i; });
    $all('#vib-lr-rank input[type="range"][data-slider]').forEach(function (s, i) { s.dataset.cfgPath = 'vibration.rank_lr.' + i; });
  }

  /* 设置族复选框：即时 cfg.write（档位按 cfg.paths 表 settle 档, 查不到回退 config-only） */
  function wireCheckboxes() {
    [['set-tray', 'show_tray_icon'], ['set-notif', 'show_notifications'], ['set-top', 'always_on_top'], ['set-dfo', 'dfo_player'],
     /* 音频页屏蔽选项/高级选项 5 个开关（契约 §A.2 bool 全集） */
     ['audio-adaptive', 'audio_haptics.adaptive_threshold'], ['audio-noise', 'audio_haptics.auto_profile'],
     ['audio-quiet', 'audio_haptics.quiet_mask'], ['audio-dual', 'audio_haptics.dual_motor'],
     ['audio-vocal', 'audio_haptics.vocal_only']].forEach(function (pair) {
      var cb = eid(pair[0]);
      if (!cb || cb.dataset.wired) return;
      cb.dataset.wired = '1';
      cb.addEventListener('change', function () {
        var tier = H.cfg.tier(pair[1]);
        cfgWrite(pair[1], cb.checked, (tier && tier.settle) || 'config-only');
      });
    });
  }

  /* 设置弹窗：cfg.get 回显 / 保存 = cfg.write(config-only) */
  function echoSettings(cfg) {
    if (!cfg) return;
    var langSel = eid('set-language'); if (langSel && cfg.language) langSel.value = cfg.language;
    var dk = eid('set-dark'); if (dk && cfg.dark_mode !== undefined) dk.checked = !!cfg.dark_mode;
    var dfo = eid('set-dfo'); if (dfo && cfg.dfo_player !== undefined) dfo.checked = !!cfg.dfo_player;
    var tray = eid('set-tray'); if (tray && cfg.show_tray_icon !== undefined) tray.checked = !!cfg.show_tray_icon;
    var notif = eid('set-notif'); if (notif && cfg.show_notifications !== undefined) notif.checked = !!cfg.show_notifications;
    var top = eid('set-top'); if (top && cfg.always_on_top !== undefined) top.checked = !!cfg.always_on_top;
    var raw = document.querySelector('[data-act="rawinput-mode"]'); if (raw && cfg.rawinput_capture_mode) raw.value = cfg.rawinput_capture_mode;
    var xin = document.querySelector('[data-act="xinput-mode"]'); if (xin && cfg.xinput_capture_mode) xin.value = cfg.xinput_capture_mode;
  }
  function collectSettingsMutations() {
    var muts = [];
    function push(path, v) { if (v !== undefined && v !== null && v !== '') muts.push({ path: path, value: v }); }
    var raw = document.querySelector('[data-act="rawinput-mode"]');
    if (raw) { var rv = RAW_MODE_MAP[raw.value] || raw.value; push('input.rawinput_capture_mode', rv); }
    var xin = document.querySelector('[data-act="xinput-mode"]');
    if (xin) { var xv = XI_MODE_MAP[xin.value] || xin.value; push('input.xinput_capture_mode', xv); }
    var tray = eid('set-tray'); if (tray) push('show_tray_icon', tray.checked);
    var notif = eid('set-notif'); if (notif) push('show_notifications', notif.checked);
    var top = eid('set-top'); if (top) push('always_on_top', top.checked);
    var langSel = eid('set-language'); if (langSel && langSel.value) push('language', langSel.value);
    var dk = eid('set-dark'); if (dk) push('dark_mode', dk.checked);
    var dfo = eid('set-dfo'); if (dfo) push('dfo_player', dfo.checked);
    return muts;
  }

  /* ---------------- UiEvent（外标签枚举） ---------------- */
  function capturedInput(kindName, name) {
    if (gpFlow.mode === 'awaitkb' || gpFlow.mode === 'awaitpad') {
      gpFlow.keys = [name];
      gpFlow.mode = 'confirm';
      renderGpFlow();
    }
    toast('已捕获' + kindName + '：' + name);
  }
  function showHidData(d) {
    if (!d) return;
    openDlg('dlg-hid');
    hidState = 'press'; renderHid();
    var host = eid('hid-state');
    if (host) {
      var bytes = d.data || [];
      var hex = bytes.map(function (b) { return ('0' + Number(b).toString(16)).slice(-2).toUpperCase(); }).join(' ');
      var pre = document.createElement('pre');
      pre.style.cssText = 'font:11px Consolas,monospace;background:rgba(0,0,0,.25);padding:8px;border-radius:6px;margin-top:8px;white-space:pre-wrap;word-break:break-all';
      pre.textContent = 'HID 报文 (handle ' + d.handle + ', ' + bytes.length + ' 字节)\n' + (hex || '（空）');
      host.appendChild(pre);
    }
  }
  function onEvent(ev) {
    /* 壳层字符串事件（契约 §3.1 / 附录 §A.6）：Alt+F4 / 系统关闭 → 退出确认弹窗 */
    if (ev === 'ShellCloseRequested') { openExitDlg(); return; }
    if (!ev || typeof ev !== 'object') return;
    if (ev.RawCaptured !== undefined) {
      var d = ev.RawCaptured;
      if (d && d.Keyboard !== undefined) capturedInput('键盘键', vkName(d.Keyboard));
      else if (d && d.Mouse !== undefined) capturedInput('鼠标键', 'Mouse ' + d.Mouse);
      else if (d && d.KeyCombo !== undefined) capturedInput('组合键', (d.KeyCombo || []).map(vkName).join('+'));
      else if (d && d.XInputCombo !== undefined) capturedInput('手柄键', 'XInput ' + JSON.stringify((d.XInputCombo || {}).button_ids || []));
      else if (d && d.GenericDevice !== undefined) capturedInput('设备键', '#' + ((d.GenericDevice || {}).button_id != null ? d.GenericDevice.button_id : '?'));
      else capturedInput('输入', JSON.stringify(d));
      return;
    }
    if (ev.HidActivationData !== undefined) { showHidData(ev.HidActivationData); return; }
    if (ev.PresetSwitch !== undefined) { toast('输入线程触发预设切换：' + ev.PresetSwitch); return; }
    if (ev.ShowAbout !== undefined) { openDlg('dlg-about'); return; }
    if (ev.ShowWindow !== undefined) { return; }
    if (ev.__hello !== undefined) {
      var hello = ev.__hello || {};
      if (REAL) toast('桥接就绪 · ' + (hello.version || ''));
      setupWindowChrome(hello); /* hello.frameless / hello.dragMode 消费（契约 v1.1 无边框窗口） */
    }
  }

  /* ---------------- data-act → 命令出口（包装 handleAct，先原逻辑后发命令） ---------------- */
  function afterAct(act, el2) {
    switch (act) {
      /* ── 无参命令（词表 §3.2 小写连字符） ── */
      case 'toggle-pause': case 'toggle-run': H.cmd('toggle-pause'); pushLocal(); break;
      case 'quit-app': H.cmd('quit'); break;
      case 'show-guide': H.cmd('show-guide'); break;
      case 'check-update': H.cmd('check-update'); break;
      case 'test-vib': case 'dev-test': H.cmd('test-vibration'); break;
      case 'motor-self-check': H.cmd('audio-self-check'); break;
      case 'mask-calib': H.cmd('audio-calibrate'); break;
      case 'dev-refresh': H.cmd('refresh-devices'); break;
      case 'dev-reset': H.cmd('reset-devices'); break;
      /* ── 带参命令（act = UiCommand 变体名；元组变体 params {value}，结构变体按 serde 字段） ── */
      case 'toggle-vib': H.cmd('SetVibrationEnabled', { value: state.vib }); pushLocal(); break;
      case 'toggle-dark': H.cmd('SetDarkMode', { value: !isLight() }); break;
      case 'preset-pick': if (el2 && el2.value) H.cmd('ApplyMappingPreset', { name: el2.value }); break;
      case 'route-pick': if (el2) H.cmd('SwitchVibEdition', { legacy: String(el2.value).indexOf('S1') >= 0 }); break;
      case 'set-language': if (el2 && el2.value) H.cmd('SetLanguage', { language: el2.value }); break;
      /* ── cfg.write（先 cfg.paths 发现表，档位按表） ── */
      case 'audio-toggle': cfgWrite('audio_haptics.enabled', state.audioOn, 'config-only'); pushLocal(); break;
      /* ★I 轮接线: 音频 A/B/C 三选一命令出口（egui audio_page.rs 同款语义 —— smart_enabled
         与 audio_algorithm_mode 同一批写入; C 内部先跑 B 核心 = smart_enabled=true。
         白名单字段: audio_haptics.audio_algorithm_mode / smart_enabled（cfg_paths.rs §A.2）） */
      case 'ab-classic': H.cfg.write([{ path: 'audio_haptics.audio_algorithm_mode', value: 'a' }, { path: 'audio_haptics.smart_enabled', value: false }], 'config-only').catch(function () {}); break;
      case 'ab-ai': H.cfg.write([{ path: 'audio_haptics.audio_algorithm_mode', value: 'b' }, { path: 'audio_haptics.smart_enabled', value: true }], 'config-only').catch(function () {}); break;
      case 'ab-c': H.cfg.write([{ path: 'audio_haptics.audio_algorithm_mode', value: 'c' }, { path: 'audio_haptics.smart_enabled', value: true }], 'config-only').catch(function () {}); break;
      case 'audio-preset': if (el2) { var nm = el2.querySelector('.nm'); cfgWrite('audio_haptics.preset', (nm ? nm.textContent.replace('★推荐', '').replace('✓', '').trim() : AUDIO_PRESETS[audioPresetSel].name), 'config-only'); } break;
      case 'audio-calib-apply': if (lastSnap && lastSnap.audio) cfgWrite('audio_haptics.mask_threshold', lastSnap.audio.calib_result, 'config-only'); var cbar = eid('audio-calib-bar'); if (cbar) cbar.style.display = 'none'; break;
      case 'audio-calib-dismiss': H.cmd('audio-calib-dismiss'); var cb2 = eid('audio-calib-bar'); if (cb2) cb2.style.display = 'none'; break;
      case 'rawinput-mode': if (el2) cfgWrite('input.rawinput_capture_mode', RAW_MODE_MAP[el2.value] || el2.value, 'config-only'); break;
      case 'xinput-mode': if (el2) cfgWrite('input.xinput_capture_mode', XI_MODE_MAP[el2.value] || el2.value, 'config-only'); break;
      case 'wl-add': case 'set-wl-add': cfgWrite('process_whitelist', WL_ROWS.map(function (w) { return w.name; }), 'reload'); pushLocal(); break;
      case 'wl-del': case 'set-wl-del': cfgWrite('process_whitelist', WL_ROWS.map(function (w) { return w.name; }), 'reload'); pushLocal(); break;
      case 'wl-mode': var wm = eid('wl-mode'); if (wm) cfgWrite('whitelist_simple_mode', wm.textContent.indexOf('简易') >= 0, 'reload'); break;
      case 'settings-save': H.cfg.write(collectSettingsMutations(), 'config-only').catch(function () {}); break;
      /* ── 捕获流（§7）：capture.start / RawCaptured 回填 / Esc 取消 ── */
      case 'gp-set-kb': if (BIND) H.capture.start('target'); break;
      case 'gp-set-pad': if (BIND) H.capture.start('trigger'); break;
      case 'gp-cancel': if (BIND && rawCapActive) H.capture.cancel(); break;
      case 'gp-confirm': {
        var s = findSlot(gpFlow.slot);
        if (s && gpFlow.kind === 'target' && gpFlow.keys.length) {
          var trig = shortKey(s);
          var idx = findMappingIndex(trig);
          var p = { index: idx, mapping: buildMapping(trig, gpFlow.keys.slice()) };
          H.cmd('UpsertMapping', p);
        }
        pushLocal();
        break;
      }
      case 'gp-clear': {
        var s2 = findSlot(gpFlow.slot);
        if (s2) {
          var t2 = shortKey(s2);
          H.cmd('UpsertMapping', { index: findMappingIndex(t2), mapping: buildMapping(t2, []) });
        }
        pushLocal();
        break;
      }
      case 'gp-del-yes': {
        var s3 = findSlot(gpFlow.slot);
        if (s3) { var i3 = findMappingIndex(shortKey(s3)); if (i3 !== null) H.cmd('DeleteMapping', { value: i3 }); }
        pushLocal();
        break;
      }
      /* ── 设置弹窗映射编辑（UpsertMapping / DeleteMapping） ── */
      case 'set-mapping-add': {
        if (setNew.trig && setNew.targets.length) {
          H.cmd('UpsertMapping', { index: null, mapping: buildMapping(setNew.trig, setNew.targets.slice(), { turbo: setNew.turbo, dtap: setNew.dtap }) });
        }
        pushLocal();
        break;
      }
      case 'set-mapping-del': case 'set-target-clear': case 'set-chip-del': case 'set-tgl': case 'set-cap-trigger': case 'set-cap-target': {
        var i4 = el2 && el2.dataset ? Number(el2.dataset.i) : NaN;
        if (!isNaN(i4) && SET_MAPPINGS[i4]) {
          var m4 = SET_MAPPINGS[i4];
          if (act === 'set-mapping-del') {
            var di = findMappingIndex(m4.trig);
            if (di !== null) H.cmd('DeleteMapping', { value: di });
          } else {
            H.cmd('UpsertMapping', { index: findMappingIndex(m4.trig), mapping: buildMapping(m4.trig, m4.targets.slice(), { turbo: m4.turbo, dtap: m4.dtap }) });
          }
        }
        pushLocal();
        break;
      }
      /* ── MOD 页：真桥走 shell.apply（mock 保持页面跳转） ── */
      case 'mod-apply': if (REAL && el2) H.shell.apply(el2.getAttribute('data-mod-id') || 'endfield'); break;
      /* ★K 轮: .dvmod 单文件格式包导入（壳侧原生文件对话框; 文件字节不过 IPC） +
       * 清单刷新。回执形态见 bridge.js H.mods.importDialog 注释（契约 v1.8 §5）。 */
      case 'mod-import': {
        if (!REAL) { toast('（原型演示）导入 .dvmod 需在实机壳内运行'); break; }
        H.mods.importDialog().then(function (r) {
          var d = (r && r.data) || {};
          if (d.cancelled) { toast('已取消导入'); return; }
          if (r && r.ok) {
            var rep = d.report || {};
            toast('已导入「' + (rep.name || rep.id || '?') + '」' + (rep.replaced ? '（覆盖替换旧包）' : '') + '，清单已刷新');
            H.mods.list().then(function (r2) { if (r2 && r2.data) renderModCards(r2.data); }).catch(function () {});
          } else {
            toast('导入失败：' + ((r && r.message) || '未知原因'));
          }
        }).catch(function () { toast('导入失败：桥不可用'); });
        break;
      }
      case 'mods-refresh':
        if (REAL) {
          H.mods.list().then(function (r) {
            if (r && r.data) { renderModCards(r.data); toast('MOD 清单已刷新（' + r.data.length + ' 项）'); }
          }).catch(function () { toast('刷新失败：桥不可用'); });
        } else {
          toast('（原型演示）刷新需在实机壳内运行');
        }
        break;
      default: break;
    }
  }
  var _handleAct = handleAct;
  handleAct = function (act, el2, ev) {
    if (window.__BRIDGE_DBG__ && String(act).indexOf('set-') === 0) {
      (window.__ACT_PROBE__ = window.__ACT_PROBE__ || []).push(act + ' i=' + (el2 && el2.dataset ? el2.dataset.i : 'NO-DS') + ' len=' + SET_MAPPINGS.length);
    }
    var r = _handleAct(act, el2, ev);
    try { afterAct(act, el2); } catch (e) { if (window.__BRIDGE_DBG__) (window.__ACT_PROBE__ = window.__ACT_PROBE__ || []).push('AFTERERR ' + act + ' ' + (e && e.message || e)); }
    return r;
  };
  /* mod-apply：冒烟模式抑制整页跳转，其余保持原演示行为 */
  var _origModApply = ACTS['mod-apply'];
  ACTS['mod-apply'] = function (el2, ev) {
    if (window.__BRIDGE_SMOKE__) {
      if (ev && ev.preventDefault) ev.preventDefault();
      if (REAL && el2) H.shell.apply(el2.getAttribute('data-mod-id') || 'endfield');
      else toast('（冒烟）shell.apply → endfield');
      return;
    }
    _origModApply(el2, ev);
  };
  /* 演示数据开关：包装推送 */
  var _setDemo = setDemo;
  setDemo = function (on) { _setDemo(on); pushLocal(); };
  /* BIND 模式：频带/马达电平由快照驱动，关掉页面随机 tick 避免覆盖快照值（纯 mock 不受影响） */
  var _tickMonitor = tickMonitor;
  tickMonitor = function () { if (BIND) return; _tickMonitor(); };
  var _tickMotors = tickMotors;
  tickMotors = function () { if (BIND) return; _tickMotors(); };
  /* 校准建议条按钮动作（动态生成，词表 §3.2 audio-calib-*） */
  ACTS['audio-calib-apply'] = function () { afterAct('audio-calib-apply'); };
  ACTS['audio-calib-dismiss'] = function () { afterAct('audio-calib-dismiss'); };

  /* ---------------- 窗口控制（契约 v1.1 §3.2 win.* + 无边框窗口契约） ----------------
     · 纯 mock：保持 v11 toast 占位（回归红线）
     · BIND/REAL：win.minimize / win.maximize / win-close → 退出确认弹窗
     · 退出弹窗三选一（照 egui render_close_dialog 语义）：托盘=win.hide / 退出=cmd quit / 取消=关弹窗
     · "ShellCloseRequested" 事件走同一弹窗（onEvent 顶部已接）
     · v1.2 修 3 秒强杀 bug：弹窗打开瞬间回执 win.close-opened → 壳撤销强退兜底；
       弹窗因任何原因关闭（取消/选择完成）不发额外消息（计时器已撤销，无需恢复） */
  function openExitDlg() {
    var d = eid('dlg-exit');
    if (!d) return;
    d.classList.add('open');
    /* 回执真桥门控：BIND = 真桥/调试模式才发（纯 mock 不发，v11 行为不变；
       调试模式经 H.call 记入 __CALLS__ 供冒烟断言，mock 分发默认 resolve 不改状态） */
    if (BIND) H.call({ t: 'win.close-opened' });
  }
  ACTS['win-min'] = function () {
    if (BIND) H.call({ t: 'win.minimize' });
    else toast('原型演示：最小化');
  };
  ACTS['win-max'] = function () {
    if (BIND) H.call({ t: 'win.maximize' });
    else toast('原型演示：最大化 / 还原');
  };
  ACTS['win-close'] = function () {
    if (BIND) openExitDlg();
    else toast('原型演示：关闭 → 确认弹窗（最小化到托盘 / 退出程序 / 取消）');
  };
  ACTS['win-hide'] = function () {
    if (BIND) { closeDlg('dlg-exit'); H.call({ t: 'win.hide' }); }
    else toast('原型演示：最小化到托盘（任务栏按钮消失, 托盘图标可恢复）');
  };
  ACTS['exit-app'] = function () {
    if (BIND) { closeDlg('dlg-exit'); H.cmd('quit'); }
    else toast('原型演示：优雅退出（落盘 + 托盘/震动线程清理）');
  };
  /* ★v1.5 模式切换（契约 §11.6 混合模式语义）：
     · 混合 mod 窗（hello.hybrid=true）里点「典/简」→ shell.stop{mode} 真命令：
       结束 mod 会话（关窗）→ Win32 还原 egui → egui 帧头应用极简/经典模式；
     · 纯 shell / 浏览器演示（无 hello.hybrid）→ 保持 v11 演示 toast（回归红线）。
     mode 词表与 endfield 顶栏三 radio 一致：full/minimal/classic。 */
  function hybridModeWindow() {
    /* 真桥 (REAL) 或冒烟调试 (DBG+smoke 注入 hello, window.__BRIDGE_SMOKE__)
       且 hello.hybrid=true (混合 mod 窗) —— DBG 非冒烟不进真路径 (纯演示) */
    return !!((REAL || window.__BRIDGE_SMOKE__) && window.__SHELL_INFO__ && window.__SHELL_INFO__.hybrid === true);
  }
  ACTS['minimal-mode'] = function () {
    if (hybridModeWindow()) H.call({ t: 'shell.stop', mode: 'minimal' });
    else toast('原型演示：极简模式为 430×520 小窗（连发/震动两个总开关）');
  };
  ACTS['classic-mode'] = function () {
    if (hybridModeWindow()) H.call({ t: 'shell.stop', mode: 'classic' });
    else toast('原型演示：经典模式为 900×600 独立窗口（老宿主三页签布局）');
  };

  /* 标题栏拖动（无边框契约 v1.1/v1.3）：镶边逻辑 v1.3 起全量迁至 bridge.js
     通用安装器（双壳通用：顶栏 mousedown→win.drag、双击→win.maximize、
     四边+四角 8px 热区→win.resize-begin；nc 旧壳兜底 CSS 也在桥内）——
     本壳只委托（幂等）；browser/file:// 下 hello 不到达则桥不安装（演示行为不变）。 */
  function setupWindowChrome(hello) {
    if (!BIND || !hello || !hello.frameless) return;
    if (H.setupWindowChrome) H.setupWindowChrome(hello);
  }

  /* Esc → capture.cancel（§7）；滑杆拖起标记 */
  document.addEventListener('keydown', function (ev) {
    if (ev.key === 'Escape' && rawCapActive && BIND) H.capture.cancel();
  });
  document.addEventListener('mousedown', function (ev) {
    if (ev.target && ev.target.matches && ev.target.matches('input[type="range"][data-cfg-path]')) draggingSlider = ev.target;
  }, true);
  document.addEventListener('mouseup', function () { draggingSlider = null; }, true);

  /* ---------------- 真桥：mods.list 渲染主题卡（ModMeta §3.2） ---------------- */
  function renderModCards(mods) {
    var first = document.querySelector('.mod-card');
    if (!first || !mods || !mods.length) return;
    var hostEl = first.parentNode;
    $all('.mod-card').forEach(function (c) { c.style.display = 'none'; });
    var wrap = eid('mod-cards-dyn');
    if (!wrap) { wrap = document.createElement('div'); wrap.id = 'mod-cards-dyn'; hostEl.insertBefore(wrap, first); }
    wrap.innerHTML = mods.map(function (m) {
      var preview = m.previewUrl
        ? '<img src="' + escHtml(m.previewUrl) + '" alt="" style="width:100%;height:110px;object-fit:cover;border-radius:8px">'
        : '<div style="height:110px;border-radius:8px;background:var(--panel-2);display:flex;align-items:center;justify-content:center;color:var(--text-weak);font-size:12px">' + (m.builtin ? '内置包' : '外部包') + '</div>';
      return '<div class="mod-card" data-mod-id="' + escHtml(m.id) + '">' +
        '<div class="mod-preview">' + preview + '</div>' +
        '<div class="row mt8"><span class="t-14 bold" style="color:var(--heading)">' + escHtml(m.name) + '</span>' +
        '<span class="badge faint right">' + escHtml(m.version) + '</span></div>' +
        '<div class="t-12 t-hint mt4">' + escHtml(m.description || '') + '</div>' +
        '<div class="t-12 mt4" style="color:var(--text-weak)">作者 ' + escHtml(m.author || '') + ' · 引擎 ' + escHtml(m.engine || '') +
        (m.compatible === false ? ' · <span style="color:var(--warn)">不兼容（降级列表）</span>' : '') + '</div>' +
        (m.id === 'classic'
          ? '<div class="badge good mt8" style="display:inline-block">使用中 · 宿主本体（不可卸载）</div>'
          : '<button class="btn primary mt8" style="width:100%;justify-content:center" data-act="mod-apply" data-mod-id="' + escHtml(m.id) + '">应用此 MOD</button>') +
        '</div>';
    }).join('');
  }

  /* ---------------- 启动 ---------------- */
  H.onSnapshot(applySnapshot);
  H.onEvent(onEvent);
  if (BIND) {
    ensureAnchors();
    wireSliders();
    wireCheckboxes();
    prefetchCfg();
    /* 实机口径：真机手柄图显示实时键位高亮（live_xinput/live_hid），「演示：实体按下」胶囊仅 mock 演示保留 */
    var demoBtn = document.querySelector('[data-act="demo-live"]');
    if (demoBtn) demoBtn.style.display = 'none';
  }
  if (REAL) {
    H.mods.list().then(function (r) { if (r && r.data) renderModCards(r.data); }).catch(function () {});
  }
  pushLocal();
  window.__BIND_READY__ = true;
})();
