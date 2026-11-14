// 控制台中央的圆盘：七段分区，中间是手柄。
//
//   import { createDial } from './dial.js';
//   const dial = createDial(svg, { sections, onNavigate });
//   dial.update(sections);
//   dial.hub({ device: 'XBOX 360', api: 'XInput', connected: true, live: {...} });
//
// 段的形状、悬停与"当前"填色沿用 endfield.css 的 .ef-dial 规则；
// 中央手柄是本产品自己画的墨线图，按下时那一块被强调色灌满。

const NS = 'http://www.w3.org/2000/svg';
const C = 300; // 圆心
const R_OUT = 286;
const R_IN = 212;

const el = (tag, attrs = {}, text) => {
  const n = document.createElementNS(NS, tag);
  for (const [k, v] of Object.entries(attrs)) n.setAttribute(k, String(v));
  if (text !== undefined) n.textContent = String(text);
  return n;
};

const at = (r, deg) => {
  const a = ((deg - 90) * Math.PI) / 180;
  return [C + r * Math.cos(a), C + r * Math.sin(a)];
};
const f = (n) => Number(n.toFixed(1));

// 环形扇区：外弧顺时针，内弧逆时针
function ring(rIn, rOut, a0, a1) {
  const [x0, y0] = at(rOut, a0);
  const [x1, y1] = at(rOut, a1);
  const [x2, y2] = at(rIn, a1);
  const [x3, y3] = at(rIn, a0);
  const large = a1 - a0 > 180 ? 1 : 0;
  return `M${f(x0)} ${f(y0)}A${rOut} ${rOut} 0 ${large} 1 ${f(x1)} ${f(y1)}L${f(x2)} ${f(y2)}A${rIn} ${rIn} 0 ${large} 0 ${f(x3)} ${f(y3)}Z`;
}

/* ---------------------------------------------------------------- 手柄线稿 */

// 手柄用项目里现成的 360 手柄矢量图（UI部件/360手柄.svg，已裁到机身）。
function padGroup() {
  const g = el('g', { class: 'pad' });
  const w = 300;
  const h = w * (364 / 568);
  for (const theme of ['light', 'dark']) {
    const img = el('image', {
      class: `pad-${theme}`,
      href: `/assets/pad-${theme}.svg`,
      x: 0,
      y: 0,
      width: w,
      height: h,
      preserveAspectRatio: 'xMidYMid meet',
    });
    img.setAttribute('xlink:href', `/assets/pad-${theme}.svg`);
    g.append(img);
  }
  return g;
}

/* ------------------------------------------------------------------ 圆盘主体 */

export function createDial(svg, { sections, onNavigate, hub } = {}) {
  svg.setAttribute('viewBox', '0 0 600 600');
  svg.replaceChildren();

  const refs = new Map(); // id -> { zh, en, plate, beam, lock, flag }
  let current = sections?.[0];

  // 外圈虚线与刻度
  svg.append(el('circle', { class: 'orbit', cx: C, cy: C, r: 296 }));
  const ticks = el('g', { class: 'ticks' });
  for (let d = 0; d < 360; d += 5) {
    const long = d % 45 === 0;
    const [x0, y0] = at(R_OUT + 8, d);
    const [x1, y1] = at(R_OUT + (long ? 18 : 12), d);
    ticks.append(el('line', { class: 'tick', x1: f(x0), y1: f(y0), x2: f(x1), y2: f(y1) }));
  }
  svg.append(ticks);

  // 段
  const segLayer = el('g', { class: 'segs' });
  const step = 360 / sections.length;
  sections.forEach((s, i) => {
    const mid = i * step;
    const gap = 1.1;
    const a0 = mid - step / 2 + gap;
    const a1 = mid + step / 2 - gap;
    const a = el('a', {
      href: `#${s.id}`,
      class: `seg${s.here ? ' is-here' : ''}${s.alert ? ' is-alert' : ''}`,
      style: `--i:${i}`,
      'aria-label': `${s.zh}${s.sub ? '：' + s.sub : ''}`,
    });
    if (s.here) a.setAttribute('aria-current', 'page');

    const [bx0, by0] = at(R_IN - 2, mid);
    const [bx1, by1] = at(126, mid);
    const [l0x, l0y] = at(R_OUT + 9, a0);
    const [l1x, l1y] = at(R_OUT + 9, a1);
    const [tx, ty] = at((R_IN + R_OUT) / 2, mid);

    const plate = el('path', { class: 'plate', d: ring(R_IN, R_OUT, a0, a1) });
    const beam = el('line', {
      class: 'beam',
      x1: f(bx0),
      y1: f(by0),
      x2: f(bx1),
      y2: f(by1),
      pathLength: 100,
    });
    const lock = el('path', {
      class: 'lock',
      d: `M${f(l0x)} ${f(l0y)}A${R_OUT + 9} ${R_OUT + 9} 0 0 1 ${f(l1x)} ${f(l1y)}`,
      pathLength: 100,
    });
    const zh = el('text', { class: 'zh', x: f(tx), y: f(ty - 2), 'text-anchor': 'middle' }, s.zh);
    const en = el('text', { class: 'en', x: f(tx), y: f(ty + 13), 'text-anchor': 'middle' }, s.en || '');

    a.append(lock, beam, plate, zh, en);

    const ref = { zh, en, plate, a, flag: null, section: s };
    if (s.alert) {
      const [fx, fy] = at(R_OUT - 14, a1 - 5);
      const flag = el('rect', { class: 'flag', x: f(fx - 5), y: f(fy - 5), width: 10, height: 10 });
      a.append(flag);
      ref.flag = flag;
    }

    const show = () => showReadout(s);
    a.addEventListener('pointerenter', show);
    a.addEventListener('focus', show);
    a.addEventListener('pointerleave', () => showReadout(current));
    a.addEventListener('blur', () => showReadout(current));
    a.addEventListener('click', (e) => {
      e.preventDefault();
      onNavigate?.(s.id);
    });

    refs.set(s.id, ref);
    segLayer.append(a);
  });
  svg.append(segLayer);

  /* ---- 中央 ---- */
  const hubTagBg = el('rect', { class: 'hub-tag', x: C - 42, y: 126, width: 84, height: 16 });
  const hubTagText = el('text', { class: 'hub-tagtext', x: C, y: 134.5, 'text-anchor': 'middle' }, '未连接');
  const hubTag = el('g', { class: 'hub-tag-g' }, undefined);
  hubTag.append(hubTagBg, hubTagText);

  const stateLine = el('text', { class: 'hub-label', x: C, y: 154, 'text-anchor': 'middle' }, '等待手柄');
  const pad = padGroup();
  pad.setAttribute('transform', 'translate(150 176)');
  const readBig = el('text', { class: 'hub-read', x: C, y: 410, 'text-anchor': 'middle' });
  const readSub = el('text', { class: 'hub-sub', x: C, y: 428, 'text-anchor': 'middle' }, '');
  const readUnit = el('tspan', { class: 'unit', dx: 4 });

  const padHit = el('a', { href: '#gamepad', 'aria-label': '手柄映射' });
  padHit.append(pad);
  padHit.addEventListener('click', (e) => {
    e.preventDefault();
    onNavigate?.('gamepad');
  });

  svg.append(hubTag, stateLine, padHit, readBig, readSub);

  function showReadout(s) {
    if (!s) return;
    readBig.textContent = s.big ?? '';
    if (s.unit) {
      readUnit.textContent = s.unit;
      readBig.append(readUnit);
    }
    readSub.textContent = s.sub ?? s.en ?? '';
  }

  const api = {
    update(next) {
      sections = next;
      current = next.find((s) => s.here) || next[0];
      next.forEach((s) => {
        const r = refs.get(s.id);
        if (!r) return;
        r.zh.textContent = s.zh;
        r.en.textContent = s.en || '';
        r.a.classList.toggle('is-here', !!s.here);
        r.a.classList.toggle('is-alert', !!s.alert);
        if (s.here) r.a.setAttribute('aria-current', 'page');
        else r.a.removeAttribute('aria-current');
      });
      showReadout(current);
    },
    hub(next = {}) {
      const { device, api: apiName, connected, live } = next;
      hubTagText.textContent = device || '未连接';
      stateLine.textContent = connected ? `已连接${apiName ? ' · ' + apiName : ''}` : '等待手柄';
      hubTagBg.setAttribute('width', String(Math.max(56, (device || '未连接').length * 9 + 22)));
      hubTagBg.setAttribute('x', String(C - Math.max(56, (device || '未连接').length * 9 + 22) / 2));
      // 按键
      const bits = live?.buttons ?? 0;
      const on = (k) => (bits >> k) & 1;
      const map = {
        A: on(1),
        B: on(2),
        X: on(3),
        Y: on(4),
        LB: on(5),
        RB: on(6),
        LT: on(7),
        RT: on(8),
        Back: on(9),
        Start: on(10),
        Guide: false,
        LS: (live?.ls ?? 0) !== 0,
        RS: (live?.rs ?? 0) !== 0,
        DPad_Up: (live?.dpad ?? 0) & 1,
        DPad_Right: (live?.dpad ?? 0) & 2,
        DPad_Down: (live?.dpad ?? 0) & 4,
        DPad_Left: (live?.dpad ?? 0) & 8,
      };
      if (live?.lt) map.LT = true;
      if (live?.rt) map.RT = true;
      pad.querySelectorAll('[data-key]').forEach((n) => {
        const k = n.getAttribute('data-key');
        n.classList.toggle('on', !!map[k]);
      });
    },
  };

  if (hub) api.hub(hub);
  showReadout(current);
  return api;
}
