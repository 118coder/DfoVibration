// 每个分区自己的工作区内容。数字与文案都来自后端读到的真实配置 / 运行态；
// 空的时候写一句完整的话，说明这里将来会出现什么。
//
// 版式约定：概览页是「圆盘 + 左右看板」那一屏；其余页面不再带圆盘，
// 各自占满整个内容区，按这一页要解决的事情排版。
import { esc } from './util.js';

/* ------------------------------------------------------------------ 小部件 */

const numOr = (v, d = 0) => (typeof v === 'number' && Number.isFinite(v) ? v : d);

const knob = (id, label, path, { min = 0, max = 100, step = 1, unit = '%', vib = false } = {}) => {
  const v = numOr(Number(value(path, vib)));
  return `<div class="knob">
    <label for="k-${id}">${esc(label)}</label>
    <output id="o-${id}" for="k-${id}">${esc(v)}${esc(unit)}</output>
    <input type="range" id="k-${id}" min="${min}" max="${max}" step="${step}" value="${esc(v)}"
      data-knob="${esc(path)}" data-unit="${esc(unit)}"${vib ? ' data-vib="1"' : ''} />
  </div>`;
};

const num = (id, label, path, { min = 0, max = 65535, hint = '' } = {}) => {
  const v = value(path);
  return `<label class="field"><span>${esc(label)}</span>
    <input type="number" id="k-${id}" min="${min}" max="${max}" value="${esc(v ?? '')}" data-field="${esc(path)}" />
    ${hint ? `<small>${esc(hint)}</small>` : ''}</label>`;
};

const toggle = (label, path, { vib = false, on = '已开启', off = '已关闭' } = {}) => {
  const v = !!value(path, vib);
  return `<button class="sw" type="button" data-toggle="${esc(path)}" aria-pressed="${v}"${
    vib ? ' data-vib="1"' : ''
  }>${esc(label)} · ${v ? esc(on) : esc(off)}</button>`;
};

const select = (label, path, options, hint = '') => {
  const v = String(value(path) ?? '');
  return `<label class="field"><span>${esc(label)}</span>
    <select data-field="${esc(path)}">${options
      .map(([val, name]) => `<option value="${esc(val)}"${val === v ? ' selected' : ''}>${esc(name)}</option>`)
      .join('')}</select>
    ${hint ? `<small>${esc(hint)}</small>` : ''}</label>`;
};

const head = (zh, en, note) => `<div class="work-head">
  <h1>${esc(zh)} <span class="ef-eyebrow">// ${esc(en)}</span></h1>
  ${note ? `<p>${esc(note)}</p>` : ''}
</div>`;

const empty = (sentence) => `<p class="empty-note">${esc(sentence)}</p>`;

let S = null;
// 震动参数存在 S.vib 里（后端分文件保存）；其余字段都在 S.cfg 上。
// 早先这里直接拿 S 当根，`audio_haptics.*` 这类路径全部取不到值，
// 表现是滑杆只剩一个「%」、开关永远显示关闭 —— 所以默认根必须是配置本体。
function value(path, vib = false) {
  const root = vib ? S.vib : S.cfg;
  return path.split('.').reduce((o, k) => (o == null ? undefined : o[k]), root);
}

/* -------------------------------------------------------------- 键名与标记 */

export function keyLabel(raw) {
  const k = String(raw || '');
  const m = k.match(/^GAMEPAD_[0-9A-F]{4}_(.+)$/i);
  if (m) {
    const rest = m[1];
    // HID 串（带设备序列号）太长，列表里只留最后一段，避免把表格撑爆
    if (/^[0-9A-F]{4}_/i.test(rest)) {
      const seg = rest.split('_').filter(Boolean).pop() || rest;
      return `HID·${seg.slice(-8)}`;
    }
    return rest.toUpperCase();
  }
  return k;
}
const isHid = (raw) => /^GAMEPAD_[0-9A-F]{4}_[0-9A-F]{4}_/i.test(String(raw || ''));
const isPad = (raw) => /^GAMEPAD_/i.test(String(raw || ''));

// 方向位 → 文字（十字键 bit0=上 1=右 2=下 3=左；摇杆 bit0=左 1=右 2=上 3=下）
function dirText(bits, hat = 1) {
  const v = Number(bits) || 0;
  const names = hat ? ['上', '右', '下', '左'] : ['左', '右', '上', '下'];
  const on = names.filter((_, i) => v & (1 << i));
  return on.length ? on.join('+') : '回中';
}

function flagChips(m) {
  const out = [];
  if (m.turbo_enabled) out.push(['连发', 'is-alert']);
  if (m.lock_enabled) out.push(['锁定', '']);
  if (m.sequence_text) out.push(['序列', '']);
  if (m.double_tap_enabled) out.push(['双击', '']);
  if (m.run_enabled) out.push(['奔跑', '']);
  if (m.release_targets && m.release_targets.length) out.push(['抬起', '']);
  return out.map(([t, c]) => `<span class="ef-chip ${c}">${esc(t)}</span>`).join('');
}

export function mappingRows(list, { limit = 0 } = {}) {
  const rows = limit ? list.slice(0, limit) : list;
  const sel = S.sel ?? -1;
  return rows
    .map((m, i) => {
      const hid = isHid(m.trigger_key);
      return `<div class="tbl-row${i === sel ? ' is-sel' : ''}" data-row="${i}" tabindex="0">
        <span class="idx">No.${String(i + 1).padStart(3, '0')}</span>
        <span class="key-in" title="${esc(m.trigger_key)}">${
          hid ? `<span class="ef-field">HID</span> ${esc(keyLabel(m.trigger_key).slice(-9))}` : esc(keyLabel(m.trigger_key))
        }</span>
        <span class="arrow" aria-hidden="true">→</span>
        <span class="targets">${(m.target_keys || [])
          .map((t) => `<span class="ef-tag">${esc(t)}</span>`)
          .join('')}${flagChips(m)}</span>
        <span class="acts"><button class="ef-btn small" type="button" data-act="edit-mapping" data-i="${i}">改</button></span>
      </div>`;
    })
    .join('');
}

/* 手柄槽位：坐标照搬原版 `src/gui/gamepad_mapping/model.rs` 的 SLOTS，
 * 与 resources/gamepad.svg (viewBox 22 135 545 401) 精确对齐。
 * 这样图上热点和真实按键位置是同一套标定，不用再猜。 */
const SLOTS = [
  ['LT', 'LT', 'LT', 0.2826, 0.0923, 0.038, 'trigger'],
  ['LB', 'LB', 'LB', 0.3963, 0.1172, 0.038, 'shoulder'],
  ['RT', 'RT', 'RT', 0.7376, 0.0923, 0.038, 'trigger'],
  ['RB', 'RB', 'RB', 0.6239, 0.1172, 0.038, 'shoulder'],
  ['DPad_Up', '十字键·上', '↑', 0.3802, 0.4678, 0.0284, 'dpad'],
  ['DPad_Down', '十字键·下', '↓', 0.3802, 0.5856, 0.0284, 'dpad'],
  ['DPad_Left', '十字键·左', '←', 0.3325, 0.5267, 0.0284, 'dpad'],
  ['DPad_Right', '十字键·右', '→', 0.4279, 0.5267, 0.0284, 'dpad'],
  ['LS_Up', '左摇杆·上', '↑', 0.2492, 0.2611, 0.0251, 'stick'],
  ['LS_Down', '左摇杆·下', '↓', 0.2492, 0.3709, 0.0251, 'stick'],
  ['LS_Left', '左摇杆·左', '←', 0.2088, 0.316, 0.0251, 'stick'],
  ['LS_Right', '左摇杆·右', '→', 0.2896, 0.316, 0.0251, 'stick'],
  ['RS_Up', '右摇杆·上', '↑', 0.6283, 0.4696, 0.0251, 'stick'],
  ['RS_Down', '右摇杆·下', '↓', 0.6283, 0.5644, 0.0251, 'stick'],
  ['RS_Left', '右摇杆·左', '←', 0.5934, 0.517, 0.0251, 'stick'],
  ['RS_Right', '右摇杆·右', '→', 0.6632, 0.517, 0.0251, 'stick'],
  ['X', 'X 键', 'X', 0.6833, 0.3155, 0.036, 'button'],
  ['Y', 'Y 键', 'Y', 0.7516, 0.2239, 0.036, 'button'],
  ['B', 'B 键', 'B', 0.822, 0.3155, 0.036, 'button'],
  ['A', 'A 键', 'A', 0.7516, 0.4067, 0.036, 'button'],
  ['Back', 'Back', 'BK', 0.4284, 0.3145, 0.0257, 'system'],
  ['Start', 'Start', 'ST', 0.5723, 0.3145, 0.0257, 'system'],
];

const KIND_NAME = {
  trigger: '扳机',
  shoulder: '肩键',
  dpad: '十字键',
  stick: '摇杆方向',
  button: 'ABXY 面键',
  system: '系统键',
};

// 触发器 → 槽位 token：GAMEPAD_045E_LS_Up → LS_Up；HID 串（带序列号）不参与图上定位。
function triggerToken(raw) {
  const m = String(raw || '').match(/^GAMEPAD_[0-9A-F]{4}_(.+)$/i);
  if (!m) return '';
  const rest = m[1];
  if (/^[0-9A-F]{4}_/i.test(rest)) return ''; // GAMEPAD_20BC_5159_DEV..._H...
  return rest;
}

// 找出触发这个槽位的映射（返回在原本 mappings 里的下标）。
function slotIndex(list, token) {
  return list.findIndex((m) => {
    const t = triggerToken(m.trigger_key);
    return t === token || t.endsWith('_' + token);
  });
}

/**
 * 热点图层：与底图共用 viewBox "22 135 545 401"，
 * 所以圆点位置在任何缩放下都和机身严丝合缝。
 */
export function padOverlay(list) {
  const baseX = 22;
  const baseY = 135;
  const unit = 545;
  return SLOTS.map(([token, label, short, x, y, r, kind]) => {
    const i = slotIndex(list, token);
    const m = i >= 0 ? list[i] : null;
    const target = m ? (m.target_keys || []).join('/') : '';
    const cx = (baseX + x * unit).toFixed(1);
    const cy = (baseY + y * 401).toFixed(1);
    const rr = (r * unit).toFixed(1);
    const fs = r > 0.03 ? 13 : 11;
    return `<g class="hot is-${kind}${m ? ' is-set' : ''}" data-slot="${esc(token)}" role="button" tabindex="0">
      <title>${esc(label)}${m ? ` → ${esc(target)}` : ' · 还没设键'}</title>
      <circle cx="${cx}" cy="${cy}" r="${rr}" />
      <text class="k" x="${cx}" y="${cy}" font-size="${fs}">${esc(short)}</text>
      ${
        target
          ? `<text class="v" x="${cx}" y="${(Number(cy) + Number(rr) + 11).toFixed(1)}">${esc(
              target.split(/[+/]/)[0].slice(0, 6),
            )}</text>`
          : ''
      }
    </g>`;
  }).join('');
}

/* -------------------------------------------------------------------- 页面 */

export const PAGES = {
  overview: {
    zh: '概览',
    en: 'Overview',
    dialBig: (s) => (s.status?.paused ? '暂停' : '运行'),
    dialSub: (s) => (s.status?.input?.hid?.present ? '手柄已连接' : '等待手柄'),
    render(s) {
      const c = s.status?.counts || {};
      const turbo = (s.cfg?.mappings || []).filter((m) => m.turbo_enabled);
      const logs = (s.logs || []).slice(-9).reverse();
      const cfg = s.cfg || {};
      return `${head('概览', 'Overview', '左边是这台机器现在在干什么，右边是映射与白名单；下面一条命令、一段日志。')}
    <div class="work-grid ov">
      <section class="ef-panel">
        <h2>映射速览 <span class="ef-field">全部 ${(s.cfg?.mappings || []).length} 条</span>
          <a class="ef-btn small" href="#gamepad">全部</a></h2>
        <div class="scroll">
          ${
            (s.cfg?.mappings || []).length
              ? `<div class="tbl">${mappingRows(s.cfg.mappings, { limit: 6 })}</div>`
              : empty('还没有任何映射。到「手柄映射」里新增第一条。')
          }
        </div>
      </section>
      <section class="ef-panel">
        <h2>最近日志 <a class="ef-btn small" href="#logs">全部</a></h2>
        <ul class="log-list scroll">
          ${
            logs.length
              ? logs
                  .map(
                    (l) =>
                      `<li><span class="t">${esc(l.t || timeOf(l.ms))}</span><span>${esc(l.m)}</span></li>`,
                  )
                  .join('')
              : '<li><span>—</span><span>日志文件还没有内容</span></li>'
          }
        </ul>
      </section>
      <section class="ef-panel">
        <h2>这一周</h2>
        <div class="scroll">
          <ul class="mini-rows">
            <li><span>手柄映射</span><b>${c.mappings ?? 0}</b></li>
            <li><span>连发中</span><b>${turbo.length}</b></li>
            <li><span>预设</span><b>${esc(cfg.current_preset || '默认')}</b></li>
            <li><span>输入超时</span><b>${cfg.input_timeout ?? 0} ms</b></li>
            <li><span>连发节拍</span><b>${cfg.interval ?? 0} / ${cfg.event_duration ?? 0} ms</b></li>
            <li><span>宏库</span><b>${(cfg.universal_macros || []).length} 条</b></li>
          </ul>
          <p class="ef-actions" style="margin-top:.8rem;flex-direction:column;align-items:stretch">
            <button class="ef-btn" type="button" data-act="toggle-pause">${
              s.status?.paused ? '恢复映射' : '暂停映射'
            }</button>
            <button class="ef-btn go" type="button" data-act="save">保存配置</button>
          </p>
        </div>
      </section>
    </div>`;
    },
  },

  gamepad: {
    zh: '手柄映射',
    en: 'Gamepad',
    dialBig: (s) => String((s.cfg?.mappings || []).filter((m) => isPad(m.trigger_key)).length),
    dialSub: () => '手柄映射',
    render(s) {
      const list = s.cfg?.mappings || [];
      const idx = s.sel ?? -1;
      const editing = idx >= 0 && idx < list.length ? list[idx] : null;
      const form = editing ? mappingForm(editing, idx) : s.draft ? mappingForm(s.draft, -1) : null;
      const hid = s.status?.input?.hid;
      const devName = hid?.present
        ? `${String(hid.vid.toString(16)).toUpperCase().padStart(4, '0')}:${String(hid.pid.toString(16))
            .toUpperCase()
            .padStart(4, '0')}`
        : '未连接';

      const padMaps = list
        .map((m, i) => ({ m, i, token: triggerToken(m.trigger_key) }))
        .filter((x) => x.token);
      const setSlots = new Set(padMaps.map((x) => x.token));
      const missing = SLOTS.filter(([token]) => !setSlots.has(token)).length;
      const chords = (s.cfg?.mappings || []).filter((m) => String(m.trigger_key).includes('+'));

      const chips = padMaps
        .map(({ m, i, token }) => {
          const kind = SLOTS.find(([t]) => t === token)?.[6] || 'button';
          return `<button class="map-chip is-${kind}" type="button" data-map="${i}" title="${esc(m.trigger_key)}">
            <b>${esc(token.replace(/^LS_|^RS_|^DPad_/, ''))}</b><i>→</i><em>${esc(
              (m.target_keys || []).join('/') || '—',
            )}</em>
          </button>`;
        })
        .join('');

      const vibOn = !!s.vib?.enabled;
      const paused = !!s.status?.paused;
      const presets = s.cfg?.presets || [];
      const jobMode = !!s.cfg?.minimal_vib_preset_job;

      return `${head('手柄映射', 'Gamepad', '点手柄图上的键就能改它；右边是快速连接，下面是我设好的映射。')}
    <div class="page-grid gp-top">
      <section class="ef-panel pad-panel">
        <h2>手柄 <span class="ef-field">${devName}${hid?.present ? '' : ' · 未连接'}</span>
          <span class="pad-tools">
            <button class="ef-btn small go" type="button" data-wiz="start">快速校对手柄按键</button>
            <button class="ef-btn small" type="button" data-act="pad-focus">手柄按键快速映射</button>
          </span></h2>
        <div class="pad-art-wrap">
          <div class="pad-art${s.padFocus ? ' is-capture' : ''}" id="pad-art" data-pad aria-label="手柄按键位置"></div>
        </div>
        <p class="pad-hint">${s.padFocus ? '点一个键开始校对吧。' : '图上亮着实心圈的是已经设好的键；点任意一个键就能改它。'}</p>
      </section>

      <aside class="quick-card">
        <h2>快速连接</h2>
        <div class="qc-row">
          <button class="qc-btn ${paused ? 'is-off' : 'is-on'}" type="button" data-act="toggle-pause">
            连发 · ${paused ? '已暂停' : '开启中'}
          </button>
          <button class="qc-btn ${vibOn ? 'is-on' : 'is-off'}" type="button" data-act="toggle-vibration">
            震动 · ${vibOn ? '开启' : '关闭'}
          </button>
        </div>
        <div class="qc-line">
          <span>映射选择</span>
          ${
            presets.length
              ? `<select data-preset-select>${presets
                  .map(
                    (p, i) =>
                      `<option value="${i}"${p.name === s.cfg?.current_preset ? ' selected' : ''}>${esc(
                        p.name,
                      )}</option>`,
                  )
                  .join('')}</select>`
              : '<b class="warn">还没有预设</b>'
          }
        </div>
        <div class="qc-seg">
          <button class="${jobMode ? '' : 'is-on'}" type="button" data-job="0">通用预设</button>
          <button class="${jobMode ? 'is-on' : ''}" type="button" data-job="1">全职业预设</button>
        </div>
        <div class="qc-line"><span>当前预设</span><b>${esc(s.cfg?.current_preset || '默认')}</b></div>
        <div class="qc-line"><span>连发节奏</span><b>${s.cfg?.interval ?? 0} / ${
          s.cfg?.event_duration ?? 0
        } ms</b></div>
        <div class="qc-links">
          <a class="ef-btn small" href="#turbo">调连发</a>
          <a class="ef-btn small" href="#vibration">调震动</a>
          <a class="ef-btn small" href="#whitelist">白名单</a>
        </div>
        <div class="qc-line">
          <span>ACT1 自动注入</span>
          <b class="${s.cfg?.vib_legacy_client ? 'good' : ''}">${
            s.cfg?.vib_legacy_client ? '已启用（DLL · 游戏目录）' : '未启用（S4+ 新方案）'
          }</b>
        </div>
        <div class="qc-live">
          <div><span>十字键</span><b>${hid?.present ? dirText(hid.dpad) : '—'}</b></div>
          <div><span>左摇杆</span><b>${hid?.present ? dirText(hid.ls, 0) : '—'}</b></div>
          <div><span>右摇杆</span><b>${hid?.present ? dirText(hid.rs, 0) : '—'}</b></div>
          <div><span>按键位图</span><b>${hid?.present ? '0x' + (hid.buttons >>> 0).toString(16).toUpperCase() : '—'}</b></div>
        </div>
        <div class="qc-foot">
          <span>手柄 ${padMaps.length} 条映射</span>
          <span>${missing ? `${missing} 个键还没设键盘` : '全部设好了'}</span>
        </div>
      </aside>
    </div>

    <div class="page-grid gp-bottom">
      <section class="ef-panel">
        <h2>我设好的映射 <span class="ef-field">${padMaps.length} 条</span>
          ${missing ? `<span class="ef-state">${missing} 个键还没设键盘</span>` : ''}
          <input class="map-search" type="search" data-map-search placeholder="找映射…" aria-label="搜索映射" /></h2>
        <div class="scroll">
          ${padWizard(s)}
          ${chips ? `<div class="chip-wrap">${chips}</div>` : empty('还没有手柄映射。点图上任意一个键就能建第一条。')}
          <div class="chip-legend">
            ${Object.entries(KIND_NAME)
              .map(([k, name]) => `<span><i class="is-${k}"></i>${esc(name)}</span>`)
              .join('')}
          </div>
          ${chordPanel(s)}
        </div>
      </section>
      <section class="ef-panel">
        <h2>${form ? (editing ? '编辑这一条' : '新建一条') : '选一个键'}</h2>
        <div class="scroll">
          ${
            form ||
            `${empty('点手柄图上的键，或者点左边的映射条，这条的触发键、发送键和连发设置会出现在这里。')}
            <div class="sub-h">还没设的键</div>
            <div class="chip-wrap">${SLOTS.filter(([t]) => !setSlots.has(t))
              .map(
                ([t, label, , , , , kind]) =>
                  `<button class="map-chip is-${kind} is-empty" type="button" data-slot="${esc(t)}">${esc(
                    label,
                  )}</button>`,
              )
              .join('')}</div>`
          }
        </div>
      </section>
    </div>`;
    },
  },

  turbo: {
    zh: '连发',
    en: 'Turbo',
    dialBig: (s) => String((s.cfg?.mappings || []).filter((m) => m.turbo_enabled).length),
    dialSub: () => '连发中',
    render(s) {
      const all = s.cfg?.mappings || [];
      const list = all.filter((m) => m.turbo_enabled);
      const keys = [...new Set(list.flatMap((m) => m.target_keys || []))];
      return `${head('连发', 'Turbo', '键盘上亮起来的是这些连发正在发送的键；右边定节奏，单条映射还能覆盖。')}
    <div class="page-grid turbo-grid">
      <section class="ef-panel kb-panel">
        <h2>按键分布 <span class="ef-field">亮 = 连发发送键 · ${keys.length} 个</span></h2>
        <div class="kb-wrap">
          <div class="kb-board" id="kb-board" data-lit="${esc(keys.join(','))}" aria-label="键盘按键分布"></div>
        </div>
        ${kbBatchBar(s)}
        <p class="kb-note">${keys.length ? `当前会连发：${keys.map((k) => `「${k}」`).join('、')}` : '还没有开连发的映射，键盘上不会亮。'}</p>
      </section>
      <div class="turbo-side">
        <section class="ef-panel">
          <h2>全局节奏</h2>
          <div class="scroll">
            ${presetManager(s)}
            <div class="knobs">
              ${knob('interval', '连发间隔', 'interval', { min: 5, max: 500, unit: ' ms' })}
              ${knob('duration', '按压时长', 'event_duration', { min: 1, max: 200, unit: ' ms' })}
              ${knob('combo', '组合键内间隔', 'combo_key_gap_ms', { min: 0, max: 200, unit: ' ms' })}
              ${knob('timeout', '输入超时', 'input_timeout', { min: 100, max: 5000, step: 50, unit: ' ms' })}
            </div>
            <p class="ef-notice" data-flag="手感">间隔太小会让游戏掉输入，按键密集的职业建议 20–40ms。</p>
          </div>
        </section>
        <section class="ef-panel">
          <h2>连发中的映射 <span class="ef-field">${list.length} 条</span></h2>
          <div class="scroll">
            ${
              list.length
                ? `<ul class="ef-rows">${list
                    .map(
                      (m) => `<li>
                  <div>
                    <div><b>${esc(keyLabel(m.trigger_key))}</b> <span class="ef-field">→ ${esc(
                        (m.target_keys || []).join('+'),
                      )}</span></div>
                    <div class="meta"><span>间隔 ${m.interval ? m.interval + ' ms' : '跟随全局'}</span><span>按压 ${
                        m.event_duration ? m.event_duration + ' ms' : '跟随全局'
                      }</span></div>
                  </div>
                  <div class="acts"><button class="ef-btn small" type="button" data-act="turbo-off" data-i="${all.indexOf(
                    m,
                  )}">停止连发</button></div>
                </li>`,
                    )
                    .join('')}</ul>`
                : empty('还没有开连发的映射。到「手柄映射」里选一条，把「连发」打开。')
            }
          </div>
        </section>
      </div>
    </div>`;
    },
  },

  whitelist: {
    zh: '白名单',
    en: 'Whitelist',
    dialBig: (s) => String((s.cfg?.process_whitelist || []).length),
    dialSub: () => '白名单条目',
    render(s) {
      const wl = s.cfg?.process_whitelist || [];
      const on = !!s.cfg?.whitelist_enabled;
      return `${head('白名单', 'Whitelist', '只有名单里的进程会收到模拟按键；名单为空等于全部放行。')}
    <div class="page-grid wl-grid">
      <section class="ef-panel">
        <h2>进程名单 <span class="ef-field">${wl.length} 条</span>
          <span class="ef-state ${on ? 'is-alert' : ''}">${on ? '只放行名单' : '全部放行'}</span></h2>
        <div class="scroll">
          ${
            wl.length
              ? `<div class="tbl"><div class="tbl-row head">
                   <span>编号</span><span>进程</span><span></span><span>比对</span><span></span>
                 </div>${wl
                   .map(
                     (p, i) => `<div class="tbl-row" tabindex="0">
                <span class="idx">No.${String(i + 1).padStart(3, '0')}</span>
                <span class="key-in" title="${esc(p)}">${esc(p)}</span>
                <span class="arrow" aria-hidden="true">→</span>
                <span class="targets">${
                  /[\\/]/.test(p) ? '<span class="ef-chip">完整路径</span>' : '<span class="ef-chip">按进程名</span>'
                }</span>
                <span class="acts"><button class="ef-btn small danger" type="button" data-act="wl-del" data-p="${esc(
                  p,
                )}">移除</button></span>
              </div>`,
                   )
                   .join('')}</div>`
              : empty('名单是空的，现在所有程序都能收到模拟按键。加一条进程名（例如 dnf.exe）就只对它生效。')
          }
          <div class="sub-h">当前状态</div>
          <div class="info-cards">
            <div><span>过滤结果</span><b>${on && wl.length ? '只放行 ' + wl.length + ' 个进程' : '全部放行'}</b></div>
            <div><span>比对规则</span><b>${s.cfg?.whitelist_simple_mode ? '只看文件名' : '完整路径'}</b></div>
            <div><span>输入来源</span><b>${esc(s.status?.input?.source || '手柄未接入')}</b></div>
          </div>
          <div class="sub-h">怎么用</div>
          <ul class="mini-rows">
            <li><span>1 · 写进程名</span><b>dnf.exe</b></li>
            <li><span>2 · 打开过滤</span><b>白名单过滤 · 生效</b></li>
            <li><span>3 · 进游戏</span><b>只有命中时才发送模拟按键</b></li>
          </ul>
          <p class="ef-notice" data-flag="注意">名单为空时即使开了过滤也不限制任何程序；这是为了避免把手柄直接锁死。</p>
          <div class="sub-h">常见进程 <span class="ef-field">在右边点一下就能加进来</span></div>
          <ul class="mini-rows">
            <li><span>dnf.exe</span><b>国服 DFO</b></li>
            <li><span>DNF.exe</span><b>另一个客户端文件名</b></li>
            <li><span>DfoVibration.exe</span><b>本程序（调试时用）</b></li>
          </ul>
          <div class="sub-h">排查小抄</div>
          <ul class="mini-rows">
            <li><span>开了过滤却没拦住</span><b>名单是不是空的</b></li>
            <li><span>游戏里按键没反应</span><b>比对方式换一下试试</b></li>
            <li><span>换过游戏目录</span><b>完整路径那条要重加</b></li>
            <li><span>只想临时全放行</span><b>关掉「白名单过滤」即可</b></li>
          </ul>
        </div>
      </section>
      <section class="ef-panel">
        <h2>匹配方式</h2>
        <div class="scroll">
          <label class="field"><span>添加进程</span>
            <input type="text" id="wl-add" placeholder="dnf.exe 或 D:\\games\\DFO\\dnf.exe" />
            <small>只写文件名匹配所有同名进程；写完整路径只匹配那一个。</small>
          </label>
          <p class="ef-actions" style="margin-top:.6rem">
            <button class="ef-btn go" type="button" data-act="wl-add">加进名单</button>
            <button class="ef-btn small" type="button" data-wl="pick">浏览…选 exe</button>
            <button class="ef-btn small danger" type="button" data-wl="clear">清空名单</button>
            <input type="file" accept=".exe,application/octet-stream" data-wl-file hidden />
          </p>
          <div class="sub-h">常用进程一键加</div>
          <p class="ef-actions" style="flex-wrap:wrap">
            ${['dnf.exe', 'DNF.exe', 'DfoVibration.exe'].map((n) => `<button class="ef-btn small" type="button" data-wl-quick="${esc(n)}">${esc(n)}</button>`).join('')}
          </p>
          <div class="sub-h">开关</div>
          <p class="ef-actions" style="flex-direction:column;align-items:flex-start">
            ${toggle('白名单过滤', 'whitelist_enabled', { on: '生效', off: '全放行' })}
            ${toggle('简易模式', 'whitelist_simple_mode', { on: '只比文件名', off: '按完整路径' })}
          </p>
          <div class="sub-h">当前</div>
          <ul class="mini-rows">
            <li><span>状态</span><b class="${on ? '' : 'warn'}">${on ? '只放行名单' : '全部放行'}</b></li>
            <li><span>比对方式</span><b>${s.cfg?.whitelist_simple_mode ? '进程名' : '完整路径'}</b></li>
            <li><span>条目数</span><b>${wl.length}</b></li>
          </ul>
          <div class="sub-h">判定流程</div>
          <ul class="mini-rows">
            <li><span>1 · 前台进程</span><b>${esc(s.status?.input?.source ? '已识别' : '等待')}</b></li>
            <li><span>2 · 名单比对</span><b>${s.cfg?.whitelist_simple_mode ? '仅文件名' : '完整路径'}</b></li>
            <li><span>3 · 命中后</span><b>${on ? '放行模拟按键' : '不限制'}</b></li>
          </ul>
          <p class="ef-notice" data-flag="提示">白名单只影响模拟按键的投递；震动与音频震动有各自的过滤开关，别混淆。</p>
        </div>
      </section>
    </div>`;
    },
  },

  vibration: {
    zh: '震动',
    en: 'Haptics',
    dialBig: (s) => (s.vib?.enabled ? '开' : '关'),
    dialSub: (s) => (s.vib?.legacy ? 'S1 老方案' : 'S4 新方案'),
    render(s) {
      const v = s.vib || {};
      const m = s.status?.motor || { l: 0, r: 0 };
      return `${head('震动', 'Haptics', '总强度决定整体手感，左右分给两个马达；算法开关一关，那一支就回到旧行为。')}
    <div class="page-grid vib-grid">
      <section class="ef-panel">
        <h2>主控 <span class="ef-field">${v.legacy ? 'S1 ACT 老方案' : 'S4 新方案'}</span></h2>
        <div class="scroll">
          ${vibTools(s)}
          <div class="knobs">
            ${knob('master', '总强度', 'master_gain', { vib: true })}
            ${knob('maxstr', '强度上限', 'max_strength', { vib: true })}
            ${knob('ml', '左马达', 'motor_l_gain', { vib: true })}
            ${knob('mr', '右马达', 'motor_r_gain', { vib: true })}
            ${knob('attack', '攻击频率', 'attack_gain', { vib: true })}
            ${knob('damage', '受击强度', 'damage_gain', { vib: true })}
            ${knob('shake', '震屏反馈', 'shake_gain', { vib: true })}
            ${knob('move', '移动反馈', 'move_gain', { vib: true })}
          </div>
          <div class="sub-h">一键手感</div>
          <p class="ef-actions">
            <button class="ef-btn" type="button" data-vibpatch="heavy">重击</button>
            <button class="ef-btn go" type="button" data-vibpatch="balanced">均衡</button>
            <button class="ef-btn" type="button" data-vibpatch="soft">轻手</button>
          </p>
          <p class="ef-notice" data-flag="说明">一键手感只改总强度、攻击频率、衰减与上限；精细参数留在「时序」里，不会被覆盖。</p>
        </div>
      </section>
      <section class="ef-panel">
        <h2>时序</h2>
        <div class="scroll">
          <div class="knobs">
            ${knob('decay', '衰减', 'decay_ms', { min: 0, max: 1200, step: 10, unit: ' ms', vib: true })}
            ${knob('merge', '命中聚合窗', 'hitmerge_ms', { min: 0, max: 200, unit: ' ms', vib: true })}
            ${knob('out', '输出死区', 'out_threshold', { min: 0, max: 40, unit: '%', vib: true })}
            ${knob('boost', '命中加成', 'hit_boost', { min: 0, max: 100, unit: '%', vib: true })}
            ${knob('start', '起手脉冲', 'start_pulse', { min: 0, max: 100, unit: '%', vib: true })}
            ${knob('kill', '终结脉冲', 'kill_pulse', { min: 0, max: 100, unit: '%', vib: true })}
          </div>
          <div class="sub-h">算法开关</div>
          <p class="ef-actions" style="flex-wrap:wrap">
            ${toggle('命中聚合', 'merge_enabled', { vib: true })}
            ${toggle('命中限频', 'hitcap_enabled', { vib: true })}
            ${toggle('命中风暴', 'storm_enabled', { vib: true })}
            ${toggle('持续压制', 'sustain_enabled', { vib: true })}
            ${toggle('动态衰减', 'decay_enabled', { vib: true })}
            ${toggle('输出重映射', 'remap_enabled', { vib: true })}
          </p>
          <div class="sub-h">参数速查</div>
          <ul class="mini-rows">
            ${[
              ['输出死区', v.out_threshold, '%'],
              ['重映射下限', v.remap_min, '%'],
              ['分工分界', v.split_thr, '%'],
              ['命中限频窗', v.hitcap_win, 'ms'],
              ['命中限频次数', v.hitcap_max, '次'],
              ['风暴阈值', v.storm_thr, '次'],
              ['风暴保留', v.storm_keep_pct, '%'],
              ['衰减延迟', v.rank_decay_delay_ms, 'ms'],
              ['最低倍率', v.rank_decay_min_mul, '%'],
              ['输出平滑', v.out_smooth, ''],
            ]
              .map(([n, val, unit]) => `<li><span>${n}</span><b>${val ?? 0}${unit}</b></li>`)
              .join('')}
          </ul>
        </div>
      </section>
      <section class="ef-panel">
        <h2>实时输出</h2>
        <div class="scroll">
          <p class="ef-actions">${toggle('震动', 'enabled', { vib: true })}</p>
          <div class="motor-bars big" data-live="motor">
            <span><i data-motor="l" style="--v:${m.l?.toFixed?.(3) ?? 0}"></i></span>
            <span><i data-motor="r" style="--v:${m.r?.toFixed?.(3) ?? 0}"></i></span>
          </div>
          <ul class="mini-rows" style="margin-top:.7rem">
            <li><span>左马达</span><b>${Math.round((m.l || 0) * 100)}%</b></li>
            <li><span>右马达</span><b>${Math.round((m.r || 0) * 100)}%</b></li>
            <li><span>事件来源</span><b>${v.legacy ? 'DLL · ACT' : 'DLL · S4+'}</b></li>
            <li><span>引擎路线</span><b>${v.legacy ? 'S1' : 'S4+'}</b></li>
          </ul>
          <div class="sub-h">最近震动事件</div>
          <ul class="log-list">
            ${(s.logs || [])
              .filter((l) => /vib/i.test(l.m))
              .slice(-6)
              .reverse()
              .map(
                (l) =>
                  `<li><span class="t">${esc(l.t || timeOf(l.ms))}</span><span>${esc(l.m)}</span></li>`,
              )
              .join('') || '<li><span>—</span><span>暂时没有震动事件</span></li>'}
          </ul>
          <p class="side-note">马达条跟着游戏里的事件跳：打一下动一下，说明链路是通的；一直不动先看事件来源。</p>
        </div>
      </section>
    </div>`;
    },
  },

  audio: {
    zh: '音频震动',
    en: 'Audio',
    dialBig: (s) => (s.cfg?.audio_haptics?.enabled ? '开' : '关'),
    dialSub: () => '音频通道',
    render(s) {
      const a = s.cfg?.audio_haptics || {};
      const presets = (a.presets || []).map((p) => p.name);
      return `${head('音频震动', 'Audio', '从系统声音里取节奏和力度，和游戏事件震动互相独立，可以只开这一个。')}
    <div class="page-grid audio-grid">
      <section class="ef-panel">
        <h2>频段分配 <span class="ef-field">预设 ${esc(a.preset || '—')}</span></h2>
        <div class="scroll">
          <div class="form-row">
            ${select('预设', 'audio_haptics.preset', presets.map((p) => [p, p]), '预设决定整体的频段分配。')}
            ${knob('again', '总增益', 'audio_haptics.master_gain_pct')}
          </div>
          <div class="knobs" style="margin-top:.8rem">
            ${knob('alow', '低频', 'audio_haptics.low_gain_pct')}
            ${knob('ahigh', '高频', 'audio_haptics.high_gain_pct')}
            ${knob('apunch', '冲击', 'audio_haptics.punch_pct')}
            ${knob('adead', '死区', 'audio_haptics.dead_zone_pct')}
            ${knob('amask', '音乐抑制', 'audio_haptics.music_mask_pct')}
            ${knob('amax', '最大映射', 'audio_haptics.map_max', { min: 0, max: 65535, step: 1000, unit: '' })}
            ${knob('arhythm', '节奏提取', 'audio_haptics.rhythm_pct')}
            ${knob('atexture', '纹理细节', 'audio_haptics.texture_pct')}
          </div>
          <div class="sub-h">当前频段分配</div>
          <div class="band-viz">
            ${[
              ['低频', a.low_gain_pct],
              ['高频', a.high_gain_pct],
              ['冲击', a.punch_pct],
              ['节奏', a.rhythm_pct],
              ['纹理', a.texture_pct],
            ]
              .map(
                ([n, v]) => `<div><span class="bar"><i style="--h:${Math.min(100, Number(v) || 0)}%"></i></span><b>${v ?? 0}%</b><small>${n}</small></div>`,
              )
              .join('')}
          </div>
          <div class="sub-h">这几个旋钮各管什么</div>
          <ul class="mini-rows">
            <li><span>低频</span><b>鼓点、爆炸的推力</b></li>
            <li><span>高频</span><b>技能音效的细碎感</b></li>
            <li><span>冲击</span><b>一瞬间的重击感</b></li>
            <li><span>节奏</span><b>跟着拍子走的脉动</b></li>
            <li><span>纹理</span><b>持续环境音的底噪</b></li>
            <li><span>死区</span><b>低于这个值就不震</b></li>
          </ul>
          <p class="ef-notice" data-flag="顺序">先定总增益，再分低频/高频，最后用死区和音乐抑制压掉不需要的杂音。</p>
        </div>
      </section>
      <section class="ef-panel">
        <h2>开关与增强</h2>
        <div class="scroll">
          ${audioTools(s)}
          <p class="ef-actions" style="flex-direction:column;align-items:flex-start">
            ${toggle('音频震动', 'audio_haptics.enabled')}
            ${toggle('智能增强', 'audio_haptics.smart_enabled')}
            ${toggle('双马达', 'audio_haptics.dual_motor')}
            ${toggle('人声优先', 'audio_haptics.vocal_only')}
            ${toggle('安静环境抑制', 'audio_haptics.quiet_mask')}
          </p>
          <div class="sub-h">智能增强</div>
          <div class="knobs">
            ${knob('asmart', '增强强度', 'audio_haptics.smart_strength_pct')}
            ${knob('adur', '增强时长', 'audio_haptics.smart_duration_pct')}
            ${knob('aattack', '起振', 'audio_haptics.attack_pct')}
            ${knob('arelease', '回落', 'audio_haptics.release_pct')}
          </div>
          <div class="sub-h">链路</div>
          <ul class="mini-rows">
            <li><span>低频分界</span><b>${a.bass_high_hz ?? 0} Hz</b></li>
            <li><span>人声区间</span><b>${a.vocal_low_hz ?? 0}–${a.vocal_high_hz ?? 0} Hz</b></li>
            <li><span>自适应门槛</span><b>${a.adaptive_threshold ? '开' : '关'}</b></li>
            <li><span>自动配置</span><b>${a.auto_profile ? '开' : '关'}</b></li>
            <li><span>音乐抑制</span><b>${a.music_mask_pct ?? 0}%</b></li>
            <li><span>起振 / 回落</span><b>${a.attack_pct ?? 0} / ${a.release_pct ?? 0}%</b></li>
          </ul>
          <p class="ef-notice" data-flag="链路">音频震动吃的是系统环回：游戏没声音、或系统静音时它不会动，这是正常的。</p>
        </div>
      </section>
    </div>`;
    },
  },

  settings: {
    zh: '设置',
    en: 'Settings',
    dialBig: (s) => s.version || '—',
    dialSub: () => '当前版本',
    render(s) {
      const presets = s.cfg?.presets || [];
      return `${head('设置', 'Settings', '改动即时生效并写回 Config.toml。')}
    <div class="page-grid set-grid">
      <section class="ef-panel">
        <h2>常规</h2>
        <div class="scroll">
          ${globalTuning(s)}
          <div class="form-row">
            ${select('语言', 'language', [
              ['SimplifiedChinese', '简体中文'],
              ['TraditionalChinese', '繁體中文'],
              ['English', 'English'],
              ['Japanese', '日本語'],
            ])}
            ${num('workers', '工作线程', 'worker_count', { min: 0, max: 32, hint: '0 = 自动' })}
          </div>
          <div class="sub-h">开关</div>
          <p class="ef-actions" style="flex-direction:column;align-items:flex-start">
            ${toggle('托盘图标', 'show_tray_icon')}
            ${toggle('通知提示', 'show_notifications')}
            ${toggle('窗口置顶', 'always_on_top')}
            ${toggle('DFO 玩家', 'dfo_player')}
          </p>
          ${deviceCard(s)}
          <div class="sub-h">配置摘要</div>
          <ul class="mini-rows">
            <li><span>主配置</span><b>Config.toml</b></li>
            <li><span>震动设定</span><b>Vibration.toml</b></li>
            <li><span>映射数</span><b>${(s.cfg?.mappings || []).length}</b></li>
            <li><span>工作线程</span><b>${s.cfg?.worker_count || '自动'}</b></li>
            <li><span>运行时间</span><b>${esc(uptimeText(s.status?.uptime))}</b></li>
          </ul>
          <div class="sub-h">输入策略</div>
          <div class="form-row">
            ${select('RawInput', 'rawinput_capture_mode', [
              ['MostSustained', '最持续'],
              ['LastStable', '最后稳定'],
              ['DiagonalPriority', '斜向优先'],
            ])}
            ${select('XInput', 'xinput_capture_mode', [
              ['MostSustained', '最持续'],
              ['LastStable', '最后稳定'],
              ['DiagonalPriority', '斜向优先'],
            ])}
          </div>
        </div>
      </section>
      <section class="ef-panel">
        <h2>界面</h2>
        <div class="scroll">
          <p class="ef-actions" style="flex-direction:column;align-items:flex-start">
            ${toggle('深色界面', 'dark_mode', { on: '开', off: '关' })}
            ${toggle('新版控制台', 'web_ui', { on: '启动用它', off: '用旧界面' })}
          </p>
          <p class="empty-note">关掉「新版控制台」后，下次启动回到原来的窗口界面；当前这一页不会立刻关掉自己。</p>
          ${hotkeyCard()}
          <div class="sub-h">切到别的界面</div>
          <div class="mode-cards">
            <button type="button" data-act="set-mode" data-mode="full"><b>完整</b><span>圆盘、映射、震动、日志全都在这</span></button>
            <button type="button" data-act="set-mode" data-mode="minimal"><b>极简</b><span>只留开关与预设，带提示音和轻震</span></button>
            <button type="button" data-act="set-mode" data-mode="turbo"><b>纯连发</b><span>键盘图 + 节奏 + 连发列表</span></button>
          </div>
          <div class="sub-h">配置</div>
          <p class="empty-note">所有改动都会落到工作目录的 Config.toml；震动参数单独写在 Vibration.toml。</p>
          <p class="ef-actions" style="margin-top:.6rem">
            <button class="ef-btn go" type="button" data-act="save">立即保存一次</button>
            <button class="ef-btn" type="button" data-act="show-guide">再看一次引导</button>
          </p>
        </div>
      </section>
      <section class="ef-panel">
        <h2>预设与更新</h2>
        <div class="scroll">
          ${
            presets.length
              ? `<ul class="ef-rows">${presets
                  .map(
                    (p, i) => `<li>
                <div>
                  <div>${esc(p.name)} ${
                      p.name === s.cfg?.current_preset ? '<span class="ef-state is-alert">当前</span>' : ''
                    }</div>
                  <div class="meta"><span>${(p.mappings || []).length} 条映射</span><span>切换键 ${esc(
                      p.switch_key || '未绑定',
                    )}</span></div>
                </div>
                <div class="acts"><button class="ef-btn small" type="button" data-act="apply-preset" data-i="${i}">套用</button></div>
              </li>`,
                  )
                  .join('')}</ul>`
              : empty('还没有保存过预设。')
          }
          ${presetManager(s)}
          ${macroPanel(s)}
          <div class="sub-h">更新</div>
          <ul class="mini-rows">
            <li><span>当前版本</span><b>${esc(s.version || '—')}</b></li>
            <li><span>自动检查</span><b>${s.cfg?.update?.auto_check ? '开' : '关'}</b></li>
            <li><span>最新版本</span><b>${esc(s.status?.update?.latest || '—')}</b></li>
          </ul>
          <p class="ef-actions" style="margin-top:.6rem">
            <button class="ef-btn go" type="button" data-act="check-update">立即检查更新</button>
          </p>
          <div class="sub-h">退出</div>
          <p class="empty-note">退出会停掉键盘鼠标钩子，游戏里就不再收到任何模拟按键。</p>
          <p class="ef-actions" style="margin-top:.5rem">
            <button class="ef-btn danger" type="button" data-act="quit">退出程序</button>
          </p>
        </div>
      </section>
    </div>`;
    },
  },

  /* ---------------------------------------------------------------- 日志 */
  logs: {
    zh: '日志',
    en: 'Log',
    dialBig: () => '—',
    dialSub: () => '日志',
    render(s) {
      const lines = s.logs || [];
      return `${head('日志', 'Log', `运行日志的最后 ${lines.length} 行，出错的行垫了底色。`)}
    <div class="page-grid log-grid">
      <section class="ef-panel">
        <h2>运行日志 <span class="ef-field">${lines.length} 行</span>
          <button class="ef-btn small" type="button" data-act="reload-logs">刷新</button></h2>
        <div class="scroll">
          ${
            lines.length
              ? `<ul class="log-full">${lines
                  .map(
                    (l) =>
                      `<li class="${/error|ERR|失败|panic/i.test(l.m) ? 'err' : ''}"><span class="t">${esc(
                        l.t || timeOf(l.ms),
                      )}</span><span>${esc(l.m)}</span></li>`,
                  )
                  .join('')}</ul>`
              : empty('日志文件还没有内容，程序刚启动时会先写一段初始化记录。')
          }
        </div>
      </section>
    </div>`;
    },
  },
};

// 日志时间：后端给 epoch 毫秒，按本机时区显示；旧格式（自带 t）原样用。
function timeOf(ms) {
  if (!ms) return '';
  return new Date(ms).toLocaleTimeString('zh-CN', { hour12: false });
}

function uptimeText(secs) {
  const v = Math.max(0, Math.floor(Number(secs) || 0));
  const h = Math.floor(v / 3600);
  const m = Math.floor((v % 3600) / 60);
  return h ? `${h} 小时 ${m} 分` : `${m} 分 ${v % 60} 秒`;
}

export function routeOrder() {
  return ['overview', 'gamepad', 'turbo', 'whitelist', 'vibration', 'audio', 'settings', 'logs'];
}

export function render(route, state) {
  S = state;
  const page = PAGES[route] || PAGES.overview;
  return page.render(state);
}

/* -------------------------------------------------------------------- 模式 */

/**
 * 极简模式：一屏只留「现在开没开、连发几条、震动/音频开关、预设快切」。
 * 所有控件都复用同一套保存通道，切回完整界面时看到的状态完全一致。
 */
export function renderMinimal(s) {
  const paused = !!s.status?.paused;
  const hid = s.status?.input?.hid;
  const list = s.cfg?.mappings || [];
  const turbo = list.filter((m) => m.turbo_enabled);
  const presets = s.cfg?.presets || [];
  const vibOn = !!s.vib?.enabled;
  const audioOn = !!s.cfg?.audio_haptics?.enabled;
  const dev = hid?.present
    ? `${String(hid.vid.toString(16)).toUpperCase().padStart(4, '0')}:${String(hid.pid.toString(16))
        .toUpperCase()
        .padStart(4, '0')}`
    : '未连接';
  return `<div class="mini-wrap">
    <section class="mini-card">
      <header class="mini-head">
        <span class="ef-eyebrow">MINIMAL</span>
        <b class="${paused ? 'is-off' : 'is-on'}">${paused ? '已暂停' : '运行中'}</b>
        <span class="ef-field">${dev}</span>
      </header>

      <button class="mini-main${paused ? ' is-off' : ''}" type="button" data-act="mini-toggle">
        ${paused ? '恢复映射' : '暂停映射'}
        <small>${paused ? '手柄按键暂时不发送' : '按一下全部停住'}</small>
      </button>

      <div class="mini-stats">
        <div><b>${list.length}</b><span>映射</span></div>
        <div><b>${turbo.length}</b><span>连发</span></div>
        <div><b>${esc(s.cfg?.current_preset || '默认')}</b><span>预设</span></div>
      </div>

      <div class="mini-switches">
        <button type="button" class="${vibOn ? 'is-on' : ''}" data-act="toggle-vibration">
          震动 <small>${vibOn ? '开' : '关'}</small>
        </button>
        <button type="button" class="${audioOn ? 'is-on' : ''}" data-mini-audio>
          音频 <small>${audioOn ? '开' : '关'}</small>
        </button>
      </div>

      <div class="mini-presets">
        ${
          presets.length
            ? presets
                .map(
                  (p, i) =>
                    `<button type="button" class="${
                      p.name === s.cfg?.current_preset ? 'is-on' : ''
                    }" data-preset="${i}">${esc(p.name)}</button>`,
                )
                .join('')
            : '<span class="ef-field">还没有保存过预设</span>'
        }
      </div>

      <footer class="mini-foot">
        <span>极简模式 · 上方可切回完整界面</span>
        <button class="ef-btn small" type="button" data-mode="full">返回完整</button>
      </footer>
    </section>
  </div>`;
}

/** 纯连发模式：只留连发这一件事，其余页面全部隐藏。 */
export function renderTurboOnly(s) {
  return `<div class="turbo-only">
    <div class="mini-head turbo-head">
      <span class="ef-eyebrow">TURBO ONLY</span>
      <b class="${s.status?.paused ? 'is-off' : 'is-on'}">${s.status?.paused ? '已暂停' : '运行中'}</b>
      <span class="ef-field">${(s.cfg?.mappings || []).filter((m) => m.turbo_enabled).length} 条连发</span>
      <span class="turbo-head-tools">
        <button class="ef-btn small" type="button" data-mode="minimal">极简</button>
        <button class="ef-btn small" type="button" data-mode="full">完整界面</button>
      </span>
    </div>
    ${PAGES.turbo.render(s)}
  </div>`;
}

/** 首次运行的引导。步骤写死在这里，内容都是「第一次打开要知道的事」。 */
export const GUIDE_STEPS = [
  ['接上手柄', '插上 USB 或无线接收器，右上角显示设备号就说明认到了。没手柄也能先用键盘映射。'],
  ['设一条映射', '到「手柄映射」点图上的键，填要发送的键盘键，保存。图上亮实心圈的就是已经设好的。'],
  ['打开连发', '在映射里按一下「连发」，再到「连发」页调间隔与按压时长。键盘图上会亮出正在连发的键。'],
  ['调手感', '震动页管总强度与算法开关；极简模式里一键开关震动和音频提示，切回完整界面状态不变。'],
];

/* ------------------------------------------------------------ 映射编辑表单 */

function mappingForm(m, idx) {
  const isNew = idx < 0;
  const d = S.draft || m;
  const rel = (d.release_targets || []).join('+');
  return `<form class="ef-form" onsubmit="return false">
    <div class="form-row">
      <label class="field"><span>触发键 ${capBtn('trigger_key')}</span>
        <input type="text" value="${esc(d.trigger_key || '')}" data-mfield="trigger_key" placeholder="GAMEPAD_045E_A 或 Q" />
        <small>点「捕获」再按实体键，会自动填；手柄键也可以直接写。</small>
      </label>
      <label class="field"><span>发送键 ${capBtn('target_keys')}</span>
        <input type="text" value="${esc((d.target_keys || []).join('+'))}" data-mfield="target_keys" placeholder="A 或 A+B" />
        <small>多个键用 + 连接，按下时一起发。</small>
      </label>
    </div>
    ${mouseKeyPanel()}
    <div class="form-row" style="margin-top:.5rem">
      <label class="field"><span>抬起键 ${capBtn('release_targets')}</span>
        <input type="text" value="${esc(rel)}" data-mfield="release_targets" placeholder="松手时额外发一组键" />
      </label>
      <label class="field"><span>&nbsp;</span>
        <button class="ef-btn small" type="button" data-mf="clear-release">清空抬起键</button>
      </label>
    </div>
    <div class="sub-h">行为</div>
    <p class="ef-actions" style="flex-wrap:wrap">
      <button class="sw" type="button" data-mflag="turbo_enabled" aria-pressed="${!!d.turbo_enabled}">连发</button>
      <button class="sw" type="button" data-mflag="lock_enabled" aria-pressed="${!!d.lock_enabled}">锁定</button>
      <button class="sw" type="button" data-mflag="double_tap_enabled" aria-pressed="${!!d.double_tap_enabled}">双击</button>
      <button class="sw" type="button" data-mflag="run_enabled" aria-pressed="${!!d.run_enabled}">奔跑</button>
      <button class="sw" type="button" data-mflag="run_recheck" aria-pressed="${!!d.run_recheck}">重推再检测</button>
    </p>
    <div class="form-row" style="margin-top:.6rem">
      <label class="field"><span>间隔覆盖</span>
        <input type="number" min="0" max="1000" value="${d.interval ?? ''}" placeholder="跟随全局" data-mfield="interval" />
      </label>
      <label class="field"><span>按压覆盖</span>
        <input type="number" min="0" max="1000" value="${d.event_duration ?? ''}" placeholder="跟随全局" data-mfield="event_duration" />
      </label>
      <label class="field"><span>移动速度</span>
        <input type="number" min="0" max="1200" value="${d.move_speed ?? 5}" data-mfield="move_speed" />
      </label>
      <label class="field"><span>重推阈值</span>
        <input type="number" min="0" max="100" value="${d.run_threshold ?? 80}" data-mfield="run_threshold" />
      </label>
      <label class="field"><span>双击间隔 ms</span>
        <input type="number" min="0" max="500" value="${d.double_tap_gap_ms ?? 40}" data-mfield="double_tap_gap_ms" />
      </label>
    </div>
    <div class="sub-h">备注</div>
    <label class="field"><span>这条是干什么的</span>
      <input type="text" value="${esc(d.note || '')}" data-mfield="note" placeholder="例如：主力技能，长按连发" />
    </label>
    ${sequenceTools(d, S)}
    <p class="ef-actions" style="margin-top:.8rem">
      <button class="ef-btn go" type="button" data-act="mapping-save">${isNew ? '建好这条' : '保存这一条'}</button>
      <button class="ef-btn" type="button" data-act="mapping-cancel">取消</button>
      ${isNew ? '' : `<button class="ef-btn danger" type="button" data-mfdel="1" data-i="${idx}">${S.delArmed ? '⚠ 再点一次确认删除' : '删除该映射'}</button>`}
    </p>
    <p class="tool-hint">改动要点「保存这一条」才会写进配置；序列宏文本改完也会跟着一起保存。</p>
  </form>`;
}

/* ═══════════════════════════════════════════════════════════════════════════
   原版便利性组件（v3 补回）
   —— 布局不动，只把老用户天天用的「一键 / 批量 / 捕获 / 预设」放回原处。
   这些函数只负责产出 HTML，交互在 app.js 里按 data-* 统一委托。
   ═══════════════════════════════════════════════════════════════════════════ */

export const capBtn = (field, label = '捕获', title = '点一下，然后按一个实体键；支持 Ctrl/Shift/Alt 组合，Esc 取消') =>
  `<button class="cap-btn" type="button" data-cap="${esc(field)}" title="${esc(title)}">${esc(label)}</button>`;

export function presetManager(s) {
  const ps = s.cfg?.presets || [];
  const cur = s.cfg?.current_preset || '';
  const bound = ps.find((p) => p.name === cur)?.switch_key || '';
  return `<div class="tool-card">
    <div class="sub-h">预设管理 <span class="ef-field">选一个就整套套用；留空名称 = 覆盖当前</span></div>
    <div class="pm-row">
      <select data-pm-select aria-label="选择预设">
        ${ps.length
          ? ps.map((p, i) => `<option value="${i}"${p.name === cur ? ' selected' : ''}>${esc(p.name)} · ${(p.mappings || []).length} 条</option>`).join('')
          : '<option value="">还没有预设</option>'}
      </select>
      <button class="ef-btn small go" type="button" data-pm="apply">套用</button>
      <button class="ef-btn small" type="button" data-pm="rename">重命名</button>
      <button class="ef-btn small danger" type="button" data-pm="delete">删除</button>
    </div>
    <div class="pm-row">
      <input type="text" data-pm-name placeholder="预设名称（留空 = 覆盖当前）" aria-label="预设名称" />
      <button class="ef-btn small go" type="button" data-pm="save">保存当前映射</button>
    </div>
    <div class="pm-row">
      <span class="ef-field">切换键：<b>${esc(bound || '未绑定')}</b></span>
      ${capBtn('preset_switch_key', '捕获')}
      <button class="ef-btn small" type="button" data-pm="unbind">清除切换键</button>
    </div>
    <p class="tool-hint">切换键在游戏里也能用，按一下直接切整套映射；总暂停键在「设置 → 常规」里改。</p>
  </div>`;
}

export function sequenceTools(d, s) {
  const text = d?.sequence_text || '';
  const recording = !!s.seqRecording;
  const macros = s.cfg?.universal_macros || [];
  return `<div class="tool-card">
    <div class="sub-h">序列宏 <span class="ef-field">按一下自动打一套连招；填了这里，普通发送键会被接管</span></div>
    <label class="field"><span>序列文本（键,时长 用逗号隔开，段与段用 » 或 ; 分开）</span>
      <textarea rows="2" data-mfield="sequence_text" placeholder="A,80,B,80 » C,120">${esc(text)}</textarea>
    </label>
    <p class="ef-actions" style="flex-wrap:wrap;margin-top:.35rem">
      <button class="ef-btn small" type="button" data-seq="step">＋ 按键步</button>
      <button class="ef-btn small" type="button" data-seq="wait">＋ 等待 200ms</button>
      <button class="ef-btn small" type="button" data-seq="20ms">⏱ 全部 20ms</button>
      <button class="ef-btn small ${recording ? 'go' : ''}" type="button" data-seq="record">${recording ? '■ 停止并填入' : '● 录制按键'}</button>
      <button class="ef-btn small" type="button" data-seq="macro">＋ 引用通用宏</button>
      <button class="ef-btn small danger" type="button" data-seq="clear">✕ 清空</button>
    </p>
    ${s.seqShowMacros ? `<p class="ef-actions" style="flex-wrap:wrap;margin-top:.3rem">
      ${macros.length
        ? macros.map((m) => `<button class="ef-btn small" type="button" data-seq-macro="${esc(m.name)}">宏(${esc(m.name)})</button>`).join('')
        : '<span class="ef-field">还没有通用宏，到「设置」里加一条。</span>'}
    </p>` : ''}
    ${recording ? '<p class="tool-hint is-alert">正在录制：直接在键盘上打一套，再点「停止并填入」。</p>' : ''}
  </div>`;
}

export function mouseKeyPanel() {
  const dirs = [
    ['MOUSE_UP_LEFT', '↖'], ['MOUSE_UP', '↑'], ['MOUSE_UP_RIGHT', '↗'],
    ['MOUSE_LEFT', '←'], ['', '⌖'], ['MOUSE_RIGHT', '→'],
    ['MOUSE_DOWN_LEFT', '↙'], ['MOUSE_DOWN', '↓'], ['MOUSE_DOWN_RIGHT', '↘'],
  ];
  const clicks = [
    ['LBUTTON', '左键'], ['RBUTTON', '右键'], ['MBUTTON', '中键'],
    ['SCROLL_UP', '上滚'], ['SCROLL_DOWN', '下滚'],
    ['XBUTTON1', '侧键1'], ['XBUTTON2', '侧键2'],
  ];
  return `<div class="tool-card">
    <div class="sub-h">鼠标键 <span class="ef-field">点一下加到当前这条的发送键</span></div>
    <div class="mouse-grid">
      ${dirs.map(([k, t]) => k
        ? `<button type="button" class="mouse-cell" data-mouse="${k}" title="${k}">${t}</button>`
        : `<span class="mouse-cell is-mid" aria-hidden="true">${t}</span>`).join('')}
    </div>
    <p class="ef-actions" style="flex-wrap:wrap;margin-top:.4rem">
      ${clicks.map(([k, t]) => `<button class="ef-btn small" type="button" data-mouse="${k}">${t}</button>`).join('')}
    </p>
  </div>`;
}

export function macroPanel(s) {
  const list = s.cfg?.universal_macros || [];
  return `<div class="tool-card">
    <div class="sub-h">通用宏 <span class="ef-field">给多条映射共用的连招片段</span></div>
    ${list.length
      ? `<div class="chip-wrap">${list.map((m, i) => `<button class="map-chip" type="button" data-macro-del="${i}" title="点一下删除">${esc(m.name)} · ${esc(String(m.text || '').slice(0, 24))} ✕</button>`).join('')}</div>`
      : '<p class="empty-note">还没有宏。写一个名字和序列，之后在序列宏里点「引用通用宏」。</p>'}
    <div class="pm-row" style="margin-top:.4rem">
      <input type="text" data-macro-name placeholder="宏名字" aria-label="宏名字" />
      <input type="text" data-macro-text placeholder="A,80,B,80" aria-label="宏序列" />
      <button class="ef-btn small go" type="button" data-macro="add">＋ 添加</button>
    </div>
  </div>`;
}

export function chordPanel(s) {
  const all = s.cfg?.mappings || [];
  const chords = all.map((m, i) => ({ m, i })).filter(({ m }) => String(m.trigger_key || '').includes('+'));
  const q = String(s.chordQuery || '').toLowerCase();
  const visible = chords.filter(({ m }) =>
    !q || `${m.trigger_key} ${(m.target_keys || []).join('+')} ${m.note || ''}`.toLowerCase().includes(q));
  const excl = s.cfg?.chord_exclusive_keys || [];
  const members = [...new Set(chords.flatMap(({ m }) => String(m.trigger_key).split('_').pop().split('+').filter(Boolean)))];
  return `<div class="tool-card">
    <div class="sub-h">手柄组合键 <span class="ef-field">${chords.length} 组${q ? ` · 筛出 ${visible.length}` : ''}</span></div>
    <div class="pm-row">
      <input type="search" data-chord-search value="${esc(s.chordQuery || '')}" placeholder="搜组合名或发送键" aria-label="搜索组合键" />
      <button class="ef-btn small go" type="button" data-chord="add">＋ 添加组合键</button>
      ${q ? `<button class="ef-btn small danger" type="button" data-chord="purge">删除筛选结果 (${visible.length})</button>` : ''}
      ${s.chordPurgeArmed ? '<button class="ef-btn small" type="button" data-chord="purge-cancel">取消</button>' : ''}
    </div>
    ${visible.length
      ? `<ul class="ef-rows compact">${visible.map(({ m, i }) => `<li>
          <div><div><b>${esc(keyLabel(m.trigger_key))}</b> <span class="ef-field">→ ${esc((m.target_keys || []).join('+') || '—')}</span></div>
          <div class="meta"><span>${esc(m.note || '组合键')}</span><span>${m.turbo_enabled ? '连发' : '单发'}</span></div></div>
          <div class="acts">
            <button class="ef-btn small" type="button" data-chord="edit" data-i="${i}">改目标</button>
            <button class="ef-btn small danger" type="button" data-chord="del" data-i="${i}">删除</button>
          </div></li>`).join('')}</ul>`
      : empty(q ? '没有匹配的组合键。' : '还没有组合键。点「添加组合键」，模板会预填 LB+X → SPACE，改键名就能用。')}
    ${members.length ? `<div class="sub-h">独占成员键 <span class="ef-field">勾上后该键不再有自己的单键功能</span></div>
      <p class="ef-actions" style="flex-wrap:wrap">${members.map((t) => `<button class="sw mini" type="button" data-chord-ex="${esc(t)}" aria-pressed="${excl.includes(t)}">${esc(t)}</button>`).join('')}</p>` : ''}
  </div>`;
}

export function padWizard(s) {
  const total = SLOTS.length;
  const i = s.wizStep ?? -1;
  const running = i >= 0 && i < total;
  const done = s.wizDone || [];
  const slot = running ? SLOTS[i] : null;
  return `<div class="tool-card ${running ? 'is-capture' : ''}">
    <div class="sub-h">快速校对手柄按键 <span class="ef-field">${running ? `${i + 1} / ${total}` : `${done.length} / ${total} 已核对`}</span></div>
    ${running
      ? `<p class="wiz-now">现在核对 <b>${esc(slot[1])}</b> <span class="ef-field">${esc(KIND_NAME[slot[6]] || '')}</span></p>
         <p class="ef-actions">
           <button class="ef-btn small go" type="button" data-wiz="use">用图上这个键</button>
           <button class="ef-btn small" type="button" data-wiz="skip">跳过</button>
           <button class="ef-btn small danger" type="button" data-wiz="stop">结束校对</button>
         </p>`
      : `<p class="ef-actions">
           <button class="ef-btn small go" type="button" data-wiz="start">${done.length ? '继续核对' : '开始逐个核对'}</button>
           ${done.length ? '<button class="ef-btn small" type="button" data-wiz="reset">重新开始</button>' : ''}
         </p>`}
    <div class="wiz-bar"><i style="--p:${Math.round(((running ? i : done.length) / total) * 100)}%"></i></div>
    <p class="tool-hint">网页版拿不到手柄的原始报文，所以这里按槽位逐个核对/补键：点「用图上这个键」会直接跳到那个键的映射表单。要一次性按实体键校准全部热点，请用原生窗口的「快速校对手柄按键」。</p>
  </div>`;
}

export function vibTools(s) {
  const names = s.vibPresets || [];
  return `<div class="tool-card">
    <div class="sub-h">试试手感 & 存下来</div>
    <p class="ef-actions" style="flex-wrap:wrap">
      <button class="ef-btn small go" type="button" data-vibtool="test">测试震动 1 秒</button>
      <button class="ef-btn small" type="button" data-vibtool="rank">测试评分震动</button>
      <button class="ef-btn small" type="button" data-vibtool="reset">还原默认</button>
    </p>
    <div class="pm-row" style="margin-top:.4rem">
      <select data-vibpreset-sel aria-label="我的震动预设">
        ${names.length ? names.map((n, i) => `<option value="${i}">${esc(n.name)}</option>`).join('') : '<option value="">还没有保存过</option>'}
      </select>
      <button class="ef-btn small go" type="button" data-vibtool="apply">应用</button>
      <button class="ef-btn small danger" type="button" data-vibtool="del">删除</button>
    </div>
    <div class="pm-row">
      <input type="text" data-vibpreset-name placeholder="给当前这套手感起个名字" aria-label="震动预设名称" />
      <button class="ef-btn small go" type="button" data-vibtool="save">保存当前</button>
    </div>
    <p class="ef-actions" style="flex-wrap:wrap">
      <button class="ef-btn small" type="button" data-vibtool="export">导出当前设置</button>
      <button class="ef-btn small" type="button" data-vibtool="import">导入设置</button>
      <input type="file" accept=".json,.toml,application/json" data-file="vib" hidden />
    </p>
    <p class="tool-hint">网页版预设存在本机浏览器里，导出的是 JSON 文件；原生窗口的 Vibration.toml 预设是另一套，互不覆盖。</p>
  </div>`;
}

export function audioTools(s) {
  const ps = s.cfg?.audio_haptics?.presets || [];
  const calib = s.status?.audio_calib || 0;
  return `<div class="tool-card">
    <div class="sub-h">自检 & 我的预设</div>
    <p class="ef-actions" style="flex-wrap:wrap">
      <button class="ef-btn small go" type="button" data-audiotool="self">马达自检 0.5 秒</button>
      <button class="ef-btn small" type="button" data-audiotool="calib">屏蔽校准 3 秒</button>
      <button class="ef-btn small" type="button" data-audiotool="reset">还原默认</button>
    </p>
    ${calib ? `<p class="ef-notice" data-flag="校准">测出来的噪音门槛建议 <b>${calib}</b>：
      <button class="ef-btn small go" type="button" data-audiotool="calib-apply">应用</button>
      <button class="ef-btn small" type="button" data-audiotool="calib-ignore">忽略</button></p>` : ''}
    <div class="pm-row" style="margin-top:.4rem">
      <select data-audiopreset-sel aria-label="我的音频预设">
        ${ps.length ? ps.map((p, i) => `<option value="${i}">${esc(p.name)}</option>`).join('') : '<option value="">还没有保存过</option>'}
      </select>
      <button class="ef-btn small go" type="button" data-audiotool="apply">应用</button>
      <button class="ef-btn small danger" type="button" data-audiotool="del">删除</button>
    </div>
    <div class="pm-row">
      <input type="text" data-audiopreset-name placeholder="给当前这套起个名字" aria-label="音频预设名称" />
      <button class="ef-btn small go" type="button" data-audiotool="save">保存当前</button>
    </div>
    <p class="ef-actions" style="flex-wrap:wrap">
      <button class="ef-btn small" type="button" data-audiotool="export">导出当前设置</button>
      <button class="ef-btn small" type="button" data-audiotool="import">导入设置</button>
      <input type="file" accept=".json,application/json" data-file="audio" hidden />
    </p>
  </div>`;
}

export function globalTuning(s) {
  const cfg = s.cfg || {};
  return `<div class="tool-card">
    <div class="sub-h">总暂停键 <span class="ef-field">游戏内也能用，不依赖窗口聚焦</span></div>
    <div class="pm-row">
      <input type="text" data-field="switch_key" value="${esc(cfg.switch_key || '')}" placeholder="例如 DELETE 或 CTRL+F8" aria-label="总暂停键" />
      ${capBtn('switch_key', '按一下键')}
    </div>
    <div class="sub-h">全局手感</div>
    <div class="knobs">
      ${knob('g-interval', '默认间隔', 'interval', { min: 5, max: 500, unit: ' ms' })}
      ${knob('g-duration', '默认时长', 'event_duration', { min: 1, max: 200, unit: ' ms' })}
      ${knob('g-gap', '组合键内间隔', 'combo_key_gap_ms', { min: 0, max: 200, unit: ' ms' })}
      ${knob('g-timeout', '输入超时', 'input_timeout', { min: 100, max: 5000, step: 50, unit: ' ms' })}
    </div>
    <p class="tool-hint">拖动即时生效并自动写盘，不用点保存。</p>
  </div>`;
}

export function deviceCard(s) {
  const stopped = s.status?.stopped_threads || [];
  return `<div class="tool-card">
    <div class="sub-h">设备与后台</div>
    <div class="pm-row">
      <button class="ef-btn small" type="button" data-dev="refresh">刷新设备</button>
      <button class="ef-btn small" type="button" data-dev="reset">重置手柄连接</button>
      <button class="ef-btn small go" type="button" data-dev="test">测试震动</button>
    </div>
    ${stopped.length
      ? `<p class="ef-notice" data-flag="后台">有 ${stopped.length} 个后台功能停了（${esc(stopped.join('、'))}）
          <button class="ef-btn small go" type="button" data-dev="restart">一键重启</button></p>`
      : '<p class="tool-hint">后台功能都在跑。掉过线的话，先「重置手柄连接」再按一下手柄键。</p>'}
  </div>`;
}

export function hotkeyCard() {
  return `<div class="tool-card">
    <div class="sub-h">快捷键</div>
    <ul class="mini-rows">
      <li><span>切换分区</span><b>1 – 8</b></li>
      <li><span>暂停 / 恢复</span><b>Delete（总暂停键，可改）</b></li>
      <li><span>立刻写盘</span><b>Ctrl + S</b></li>
      <li><span>取消选中 / 退出捕获</span><b>Esc</b></li>
      <li><span>捕获按键</span><b>点输入框旁「捕获」再按实体键</b></li>
    </ul>
  </div>`;
}

export function kbBatchBar(s) {
  const add = s.kbAdd || [];
  const drop = s.kbRemove || [];
  const conflicts = s.kbConflicts || [];
  const pending = add.length + drop.length;
  return `<div class="kb-batch ${pending ? 'is-on' : ''}">
    <span class="ef-field">点键盘上的键就能多选：
      <b class="pick">待加 ${add.length}</b> · <b class="drop">待移除 ${drop.length}</b>
      ${add.length ? `（${esc(add.slice(0, 8).join('、'))}${add.length > 8 ? '…' : ''}）` : ''}
    </span>
    <span class="kb-batch-acts">
      <button class="ef-btn small go" type="button" data-kb="apply"${pending ? '' : ' disabled'}>确认修改连发</button>
      <button class="ef-btn small" type="button" data-kb="cancel"${pending ? '' : ' disabled'}>取消修改连发</button>
    </span>
    ${conflicts.length ? `<div class="kb-conflict">
      <b>${conflicts.length} 个键已经被别的映射占用：${esc(conflicts.join('、'))}</b>
      <span class="ef-actions">
        <button class="ef-btn small go" type="button" data-kb="overwrite">覆盖并添加</button>
        <button class="ef-btn small" type="button" data-kb="skip">跳过冲突项</button>
        <button class="ef-btn small" type="button" data-kb="cancel">取消</button>
      </span>
    </div>` : ''}
    <p class="tool-hint">黄色实心 = 已有同名连发，点一下会标记为移除；虚线框 = 准备新建；红框 = 待移除。选完点「确认修改连发」一次写入。</p>
  </div>`;
}

/** 给校准向导用：第 i 个槽位的 token。 */
export function padWizardSlot(i) {
  return SLOTS[i] || null;
}
export function padWizardTotal() {
  return SLOTS.length;
}
