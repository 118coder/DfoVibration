/* ============================================================
   DfoVibration 手柄界面原型 · 交互脚本（全部为演示数据）
   data-act 命令名对齐后端 UiContract action id，便于将来接线
   ============================================================ */
'use strict';
window.addEventListener('error', function (e) {
  var t = document.getElementById('toast-root');
  if (t) { var d = document.createElement('div'); d.className = 'ef-toast';
    d.textContent = 'ERR ' + e.message + ' @line ' + e.lineno; t.appendChild(d); }
});

const $  = (s, r) => (r || document).querySelector(s);
const $$ = (s, r) => Array.from((r || document).querySelectorAll(s));
const SVGNS = 'http://www.w3.org/2000/svg';

/* ---------------- 震动参数定义（通用震动 / 全职业预设共用） ---------------- */
const ALGO_LABELS = {
  '专属算法 1 · 攻速-力度': [['攻速权重', 60], ['力度权重', 55], ['连击加深', 48], ['沉稳回馈', 30]],
  '专属算法 2 · 节奏-连击': [['节奏密度', 70], ['连击间隔', 40], ['连击加深', 52], ['收势缓冲', 25]],
  '专属算法 3 · 重击-蓄力': [['蓄力深度', 65], ['爆发强度', 80], ['后摇消减', 35], ['震感收敛', 40]],
};
const VIB_PARAMS = {
  global: [
    ['总强度', 'Master', 0, 100, 72, '%'], ['最大力度', 'Max', 0, 100, 90, '%'],
    ['攻击', 'Attack', 0, 100, 66, '%'], ['受击', 'Damage', 0, 100, 58, '%'],
    ['震撼', 'Shake', 0, 100, 40, '%'], ['移动', 'Move', 0, 100, 24, '%'],
    ['衰减时长', 'Decay', 0, 2000, 420, 'ms'], ['命中时加重', 'Hit Boost', 0, 100, 30, '%'],
    ['起手脉冲', 'Start Pulse', 0, 100, 35, '%'], ['击杀脉冲', 'Kill Pulse', 0, 100, 45, '%'],
    ['随机增益', 'Random', 0, 100, 20, '%'], ['输出平滑', 'Smooth', 0, 100, 40, '%'],
  ],
  fonts: [
    ['飘字强度', 'Strength', 0, 100, 55, '%'], ['节奏间隔', 'Interval', 10, 500, 90, 'ms'],
    ['节奏', 'Rhythm', 0, 100, 50, '%'], ['血量飘字', 'HP', 0, 100, 48, '%'],
    ['特殊飘字', 'Special', 0, 100, 40, '%'], ['状态飘字', 'State', 0, 100, 35, '%'],
    ['特效飘字', 'Effect', 0, 100, 36, '%'], ['攻击飘字', 'Attack', 0, 100, 42, '%'],
    ['命中飘字', 'Hit', 0, 100, 38, '%'],
  ],
  rank: [ ['等级增益', 'Level Gain', 0, 100, 70, '%'], ['评分时长', 'Duration', 50, 2000, 480, 'ms'] ],
  summon: [ ['召唤兽震动强度', 'Summon', 0, 100, 50, '%'], ['频率值', 'Abs Freq', 1, 60, 22, 'Hz'] ],
  decay: [
    ['延迟', 'Delay', 0, 1000, 120, 'ms'], ['变弱速度', 'Speed', 1, 100, 34, ''],
    ['最小倍率', 'Min', 0, 100, 20, 'frac'], ['最大倍率', 'Max', 0, 200, 150, 'frac'],
  ],
  algo: [
    ['攻速权重', 'AP1', 0, 100, 60, '%'], ['力度权重', 'AP2', 0, 100, 55, '%'],
    ['连击加深', 'AP3', 0, 100, 48, '%'], ['沉稳回馈', 'AP4', 0, 100, 30, '%'],
  ],
};
function paramRow(attr, d) {
  const [zh, en, min, max, val, unit] = d;
  const disp = unit === 'frac' ? (val / 100).toFixed(2) : val + unit;
  return '<div class="slider-row"><div class="lbl">' + zh + '<small>' + en + '</small></div>' +
    '<input type="range" class="ef-range" data-' + attr + '="' + en.toLowerCase().replace(/ /g, '') + '" min="' + min + '" max="' + max + '" value="' + val + '">' +
    '<span class="val">' + disp + '</span></div>';
}
function renderParamRows(id, attr, key) {
  const el = document.getElementById(id);
  if (el) el.innerHTML = VIB_PARAMS[key].map(d => paramRow(attr, d)).join('');
}
renderParamRows('vib-global', 'vib', 'global');
renderParamRows('vib-fonts', 'vib', 'fonts');
renderParamRows('vib-rank', 'vib', 'rank');
renderParamRows('vib-decay', 'vib', 'decay');
renderParamRows('job-global', 'vibj', 'global');
renderParamRows('job-fonts', 'vibj', 'fonts');
renderParamRows('job-rank', 'vibj', 'rank');
renderParamRows('job-decay', 'vibj', 'decay');
renderParamRows('job-summon', 'vibj', 'summon');
renderParamRows('job-algo-sliders', 'vibj', 'algo');

/* ---------------- v8：高级区 62 条 + 评分 10 类 + 左右权重 ---------------- */
const VIB_ADV = [
  ['连击密度自适应', 6, [
    ['密度基线', 'Density Base', 0, 100, 55, '%'], ['连击窗口', 'Combo Window', 60, 1200, 320, 'ms'],
    ['密度上限', 'Density Cap', 100, 400, 220, '%'], ['收敛速度', 'Converge', 1, 20, 8, ''],
    ['断连判定', 'Break Gap', 80, 2000, 420, 'ms'], ['密度平滑', 'Smooth', 0, 100, 40, '%'],
  ]],
  ['命中自适应', 7, [
    ['命中率窗口', 'Hit Window', 200, 3000, 900, 'ms'], ['高命中阈值', 'Hi Thresh', 50, 100, 82, '%'],
    ['高命中压缩', 'Compress', 0, 100, 35, '%'], ['漏击补震', 'Miss Fill', 0, 100, 30, '%'],
    ['补震上限', 'Fill Cap', 0, 200, 80, '%'], ['命中波动容忍', 'Tolerance', 0, 100, 45, '%'],
    ['自适应恢复', 'Recover', 1, 60, 12, 's'],
  ]],
  ['走路质感', 6, [
    ['步频跟随', 'Step Sync', 0, 100, 70, '%'], ['步伐幅度', 'Step Amp', 0, 100, 38, '%'],
    ['起步加重', 'Start Boot', 0, 100, 45, '%'], ['停步余震', 'Stop Tail', 0, 500, 120, 'ms'],
    ['转身抖动', 'Turn Shake', 0, 100, 25, '%'], ['地面材质系数', 'Material', 0, 100, 50, '%'],
  ]],
  ['走位能量', 3, [
    ['能量上限', 'Energy Cap', 0, 100, 60, '%'], ['蓄能速度', 'Charge', 1, 20, 6, ''],
    ['释放阈值', 'Release', 0, 100, 55, '%'],
  ]],
  ['输出曲线', 2, [
    ['曲线形状', 'Curve Shape', 0, 100, 50, '%'], ['曲线强度', 'Curve Gain', 0, 200, 110, '%'],
  ]],
  ['事件衰减', 4, [
    ['事件合并窗口', 'Merge Win', 0, 200, 45, 'ms'], ['衰减曲线', 'Decay Curve', 0, 100, 50, '%'],
    ['尾震保留', 'Tail Keep', 0, 100, 35, '%'], ['最小可感幅值', 'Min Feel', 0, 100, 12, '%'],
  ]],
  ['S4 整形', 3, [
    ['起手整形', 'Attack Shape', 0, 100, 60, '%'], ['中段整形', 'Sustain Shape', 0, 100, 50, '%'],
    ['收尾整形', 'Release Shape', 0, 100, 55, '%'],
  ]],
  ['节流', 3, [
    ['最小间隔', 'Min Gap', 0, 200, 20, 'ms'], ['每秒次数上限', 'Rate Cap', 1, 200, 60, '次'],
    ['超限策略', 'Overflow', 0, 2, 1, ''],
  ]],
  ['算法窗口', 7, [
    ['采样窗口', 'Sample Win', 16, 512, 128, '帧'], ['峰值窗口', 'Peak Win', 16, 512, 96, '帧'],
    ['平滑窗口', 'Smooth Win', 1, 64, 8, '帧'], ['冷却窗口', 'Cooldown', 0, 2000, 180, 'ms'],
    ['风暴判定窗口', 'Storm Win', 100, 3000, 800, 'ms'], ['风暴内上限', 'Storm Cap', 1, 100, 24, '次'],
    ['窗口滑动步长', 'Slide Step', 1, 128, 16, '帧'],
  ]],
  ['脉冲', 6, [
    ['脉冲幅度', 'Pulse Amp', 0, 100, 50, '%'], ['脉冲宽度', 'Pulse Width', 4, 200, 40, 'ms'],
    ['脉冲间隔', 'Pulse Gap', 4, 500, 60, 'ms'], ['脉冲抖动', 'Pulse Jitter', 0, 100, 20, '%'],
    ['双马达相位差', 'Phase', 0, 180, 40, '°'], ['脉冲叠加上限', 'Stack Cap', 1, 16, 4, ''],
  ]],
  ['S1 群怪不打手', 14, [
    ['群怪判定数量', 'Mob Count', 2, 30, 6, '只'], ['群怪密度抑制', 'Mob Suppress', 0, 100, 55, '%'],
    ['单怪权重', 'Solo Weight', 0, 100, 100, '%'], ['群怪权重', 'Mob Weight', 0, 100, 45, '%'],
    ['抑制起效延迟', 'On Delay', 0, 1000, 120, 'ms'], ['抑制释放延迟', 'Off Delay', 0, 2000, 400, 'ms'],
    ['抑制深度', 'Depth', 0, 100, 60, '%'], ['群怪马达分工', 'L/R Split', 0, 100, 50, '%'],
    ['群怪脉冲抑制', 'Pulse Cut', 0, 100, 70, '%'], ['群怪节流倍数', 'Throttle x', 1, 10, 3, '×'],
    ['群怪风暴阈值', 'Storm Th', 1, 100, 20, '次'], ['群怪尾震', 'Mob Tail', 0, 100, 25, '%'],
    ['群怪恢复速度', 'Recover', 1, 60, 15, 's'], ['群怪最低保留', 'Floor', 0, 100, 15, '%'],
  ]],
  ['预留', 1, [['预留参数', 'Reserved', 0, 100, 0, '%']]],
];
const RANK_EVENTS = [
  ['评分点', 'Score Point', 70, 412], ['极限闪避', 'Perfect Dodge', 55, 38],
  ['暴击', 'Critical', 62, 906], ['最终击杀', 'Final Kill', 80, 12],
  ['凌空追击', 'Air Chase', 48, 64], ['命中第一击', 'First Hit', 45, 210],
  ['增益叠加', 'Buff Stack', 40, 87], ['释放技能', 'Skill Cast', 52, 634],
  ['暴击特写', 'Crit Cut-in', 66, 41], ['怪物死亡', 'Mob Death', 35, 148],
];
const LR_ITEM = ['总强度', '最大力度', '攻击', '受击', '震撼', '移动', '衰减时长', '命中时加重', '起手脉冲',
  '击杀脉冲', '随机增益', '输出平滑', '飘字强度', '节奏间隔', '节奏', '血量飘字', '特殊飘字', '状态飘字',
  '特效飘字', '攻击飘字', '命中飘字', '召唤兽窗口', '召唤兽次数', '落地', '浮空', '被击'];
const LR_RANK = ['移动持续', '评分 D', '评分 C', '评分 B', '评分 A', '评分 S', '评分 SS', '评分 SSS',
  '评分点', '极限闪避', '暴击', '最终击杀', '凌空追击', '命中第一击', '增益叠加', '释放技能', '暴击特写', '怪物死亡',
  '衰减延迟', '衰减速度', '最小倍率', '最大倍率',
  '连击 2 档', '连击 5 档', '连击 10 档', '连击 20 档',
  '风暴期间', '群怪期间', 'BOSS 战', '测试通道'];

function lrRow(name, i) {
  return '<div class="slider-row"><div class="lbl">' + name + '<small>LR-' + String(i + 1).padStart(2, '0') + '</small></div>' +
    '<input type="range" class="ef-range" min="-100" max="100" value="0">' +
    '<span class="val">0</span></div>';
}
function renderAdv(host, attr) {
  if (!host) return;
  let html = '';
  VIB_ADV.forEach(function (g) {
    html += '<p class="slider-group-title">' + g[0] + ' · ' + g[1] + ' 条</p>';
    g[2].forEach(function (d) {
      html += '<div class="slider-row"><div class="lbl">' + d[0] + '<small>' + d[1] + '</small></div>' +
        '<input type="range" class="ef-range" data-' + attr + '="' + d[1].toLowerCase().replace(/ /g, '') +
        '" min="' + d[2] + '" max="' + d[3] + '" value="' + d[4] + '">' +
        '<span class="val">' + d[4] + (d[5] === '' ? '' : d[5]) + '</span></div>';
    });
  });
  host.innerHTML = html;
}
function renderRankEvents(host, attr) {
  if (!host) return;
  host.innerHTML = RANK_EVENTS.map(function (d) {
    return '<div class="slider-row rank-evt"><div class="lbl">' + d[0] + '<small>' + d[1] + ' · 触发 ' + d[3] + ' 次</small></div>' +
      '<input type="range" class="ef-range" data-' + attr + '="rk' + d[1].toLowerCase().replace(/[^a-z]/g, '') +
      '" min="0" max="100" value="' + d[2] + '">' +
      '<span class="val">' + d[2] + '%</span></div>';
  }).join('');
}
function renderLR(host, list) {
  if (!host) return;
  host.innerHTML = list.map(function (n, i) { return lrRow(n, i); }).join('');
}
const advHost = document.getElementById('vib-adv');
if (advHost) advHost.className = 'slider-cols';
renderAdv(document.getElementById('vib-adv'), 'vibadv');
const jobAdv = document.getElementById('job-adv');
if (jobAdv) jobAdv.className = 'slider-cols';
renderAdv(document.getElementById('job-adv'), 'vibjadv');
renderRankEvents(document.getElementById('vib-rank-events'), 'vib');
renderRankEvents(document.getElementById('job-rank-events'), 'vibj');
renderLR(document.getElementById('vib-lr'), LR_ITEM);
renderLR(document.getElementById('vib-lr-rank'), LR_RANK);
renderLR(document.getElementById('job-lr'), LR_RANK);


/* ---------------- 演示数据 ---------------- */
const SLOTS = [
  ['LT', 0.2826, 0.0923, 0.038, 'trigger', '左扳机'],
  ['LB', 0.3963, 0.1172, 0.038, 'shoulder', '左肩键'],
  ['RT', 0.7376, 0.0923, 0.038, 'trigger', '右扳机'],
  ['RB', 0.6239, 0.1172, 0.038, 'shoulder', '右肩键'],
  ['DPad_Up', 0.3802, 0.4678, 0.0284, 'dpad', '十字上'],
  ['DPad_Down', 0.3802, 0.5856, 0.0284, 'dpad', '十向下'],
  ['DPad_Left', 0.3325, 0.5267, 0.0284, 'dpad', '十字左'],
  ['DPad_Right', 0.4279, 0.5267, 0.0284, 'dpad', '十字右'],
  ['LS_Up', 0.2492, 0.2611, 0.0251, 'stick', '左摇杆上'],
  ['LS_Down', 0.2492, 0.3709, 0.0251, 'stick', '左摇杆下'],
  ['LS_Left', 0.2088, 0.316, 0.0251, 'stick', '左摇杆左'],
  ['LS_Right', 0.2896, 0.316, 0.0251, 'stick', '左摇杆右'],
  ['RS_Up', 0.6283, 0.4696, 0.0251, 'stick', '右摇杆上'],
  ['RS_Down', 0.6283, 0.5644, 0.0251, 'stick', '右摇杆下'],
  ['RS_Left', 0.5934, 0.517, 0.0251, 'stick', '右摇杆左'],
  ['RS_Right', 0.6632, 0.517, 0.0251, 'stick', '右摇杆右'],
  ['X', 0.6833, 0.3155, 0.036, 'button', 'X 键'],
  ['Y', 0.7516, 0.2239, 0.036, 'button', 'Y 键'],
  ['B', 0.8220, 0.3155, 0.036, 'button', 'B 键'],
  ['A', 0.7516, 0.4067, 0.036, 'button', 'A 键'],
  ['Back', 0.4284, 0.3145, 0.0257, 'system', 'Back'],
  ['Start', 0.5723, 0.3145, 0.0257, 'system', 'Start'],
  ['LS_Click', 0.2492, 0.316, 0.0251, 'stick', '左摇杆按下'],
  ['RS_Click', 0.6283, 0.517, 0.0251, 'stick', '右摇杆按下'],
];
const SLOT_CAT = { trigger: '扳机', shoulder: '肩键', dpad: '十字键', stick: '摇杆方向', button: 'ABXY 面键', system: '系统键' };
const SLOT_HID = { LT: 'H629989901', LB: 'H629989902', RT: 'H629989903', RB: 'H629989904',
  'DPad_Up': 'H629989911', 'DPad_Down': 'H629989912', 'DPad_Left': 'H629989913', 'DPad_Right': 'H629989914',
  'LS_Up': 'H629989921', 'LS_Down': 'H629989922', 'LS_Left': 'H629989923', 'LS_Right': 'H629989924',
  'RS_Up': 'H629989931', 'RS_Down': 'H629989932', 'RS_Left': 'H629989933', 'RS_Right': 'H629989934',
  X: 'H629989941', Y: 'H629989942', B: 'H629989943', A: 'H629989944',
  Back: 'H629989951', Start: 'H629989952',
  LS_Click: 'H629989961', RS_Click: 'H629989962' };
const WIZ_ORDER = ['LT','RT','LB','RB','A','B','X','Y','Back','Start','LS_Click','RS_Click',
  'LS_Up','LS_Down','LS_Left','LS_Right','RS_Up','RS_Down','RS_Left','RS_Right',
  'DPad_Up','DPad_Down','DPad_Left','DPad_Right'];

const DB = {
  logs: [
    { t: '11.16 09:12:33', lvl: 'info', msg: '程序启动 · R2.14 · Web 控制台就绪' },
    { t: '11.16 09:12:33', lvl: 'info', msg: '手柄已连接 Xbox 360 Controller (045E:028E)' },
    { t: '11.16 09:12:34', lvl: 'info', msg: '加载预设 PVE通用 · 映射 8 条 · 连发 5 条' },
    { t: '11.16 09:14:02', lvl: 'warn', msg: '有按键被中文输入法接管了，已自动诊断记录（建议游戏内切换英文输入）' },
    { t: '11.16 09:15:47', lvl: 'info', msg: '连发总开关 DELETE · 暂停' },
    { t: '11.16 09:15:51', lvl: 'info', msg: '连发总开关 DELETE · 恢复' },
    { t: '11.16 09:17:20', lvl: 'error', msg: '注入保护拦截 1 次：名单外程序尝试写入，已放行游戏' },
    { t: '11.16 09:18:05', lvl: 'info', msg: '音频震动：当前声音判定 → 战斗' },
    { t: '11.16 09:19:44', lvl: 'info', msg: '前台探测缓存重建完成，一切正常' },
    { t: '11.16 09:21:10', lvl: 'info', msg: '震动预设「重手感」已应用' },
    { t: '11.16 09:23:37', lvl: 'info', msg: '手柄设备变动 · 已自动重新枚举完成' },
    { t: '11.16 09:25:01', lvl: 'info', msg: '配置已自动落盘（1 秒去抖）' },
    { t: '11.16 09:26:18', lvl: 'info', msg: '职业预设叠加：鬼剑士 · 狂战士' },
    { t: '11.16 09:27:40', lvl: 'info', msg: 'dnf.exe 在白名单里，连发已放行' },
  ],
  padSlots: {
    LT: { tgt: 'LSHIFT', chips: [] }, LB: { tgt: 'LCTRL', chips: [] },
    RT: { tgt: 'J', chips: ['连发'] }, RB: { tgt: 'K', chips: ['连发'] },
    DPad_Up: { tgt: 'UP' }, DPad_Down: { tgt: 'DOWN' }, DPad_Left: { tgt: 'LEFT' }, DPad_Right: { tgt: 'RIGHT' },
    LS_Up: { tgt: 'W' }, LS_Down: { tgt: 'S' }, LS_Left: { tgt: 'A' }, LS_Right: { tgt: 'D' },
    RS_Up: { tgt: 'NUMPAD4' }, RS_Down: null, RS_Left: null, RS_Right: null,
    X: { tgt: 'U', chips: ['连发'] }, Y: { tgt: 'I' }, B: { tgt: 'O' }, A: { tgt: 'SPACE' },
    Back: { tgt: 'ESC' }, Start: { tgt: 'ENTER' },
  },
  kbMappings: [
    { trig: 'Q', tgt: 'J', flags: ['连发'], note: '普通攻击' },
    { trig: 'E', tgt: 'K', flags: ['连发'], note: '技能1' },
    { trig: 'R', tgt: 'L', flags: [], note: '技能2' },
    { trig: 'CTRL+F1', tgt: '1', flags: [], note: '换装' },
    { trig: 'ALT+2', tgt: 'HOME', flags: [], note: '回城' },
    { trig: '鼠标↑', tgt: 'W', flags: ['简易奔跑'], note: '' },
    { trig: '滚轮下', tgt: 'X', flags: [], note: '拾取' },
    { trig: 'G', tgt: 'N', flags: [], note: '' },
  ],
  turbo: [
    { trig: 'J', tgt: 'J', interval: 45, dur: 30, flags: ['连发'], note: '普通攻击' },
    { trig: 'K', tgt: 'K', interval: 60, dur: 30, flags: ['连发'], note: '技能1' },
    { trig: 'L', tgt: 'L', interval: 45, dur: 30, flags: ['连发'], note: '技能2' },
    { trig: 'U', tgt: 'U', interval: 80, dur: 40, flags: ['简易奔跑'], note: '简易奔跑' , seq: '' },
    { trig: 'NUMPAD1', tgt: '1', interval: 100, dur: 30, flags: ['连发'], note: '药剂' },
    { trig: 'F1', tgt: 'J', interval: 45, dur: 30, flags: ['连发', '锁定'], note: 'buff 连点，按一下锁住一直按', seq: 'J[40] 等待[60] K[40] 等待[60] L[40]' },
  ],
  chords: [
    { combo: 'LT+LB', tgt: '2', members: ['LT', 'LB'], seq: '1[30] 2[30] 3[30]' },
    { combo: 'LB+A', tgt: '3', members: ['LB', 'A'] },
    { combo: 'RT+X', tgt: 'J', members: ['RT'] },
    { combo: 'Y+B', tgt: 'HOME', members: [], seqBad: true, seq: 'UP[40 UP[40] ENTER' },
    { combo: 'A+B+X', tgt: 'F5', members: [] },
  ],
  macros: [
    { name: '三连', text: '1[30] 2[30] 3[30]' },
    { name: '开门', text: 'UP[40] UP[40] ENTER' },
  ],
  presets: ['PVE通用', '搬砖-风法', '决斗专用', '测试-全零', '旧版备份'],
  currentPreset: 'PVE通用',
  vibPresets: ['默认', '低频攻击职业', '高频攻击职业', '高振幅', '节奏律动', '极简轻巧', '实战竞技', '测试版(全0)'],
  audioMine: ['搬砖-夜班', '剧场模式'],
  audioUsing: '搬砖-夜班',
  whitelist: [
    { name: 'dnf.exe', path: 'D:\Games\DNF\dnf.exe' },
    { name: 'WeGame.exe', path: 'C:\Program Files (x86)\WeGame\WeGame.exe' },
    { name: 'DNFLauncher.exe', path: '' },
  ],
};
const OCC_KEYS = ['Q', 'E', 'R', 'G'];

/* ---------------- 全局状态 ---------------- */
const state = {
  paused: false, route: 'overview', mode: 'full',
  kbLit: new Set(DB.turbo.map(m => m.trig)),
  kbPick: new Set(), kbDrop: new Set(),
  gpSlot: null, gpFlow: 'idle',
  wizNo: 3, chordDraft: null, logLevel: 'all', logSearch: '',
  captures: {}, waitingCap: null, seqRecording: false,
  seqSteps: [{ k: 'J', d: 40 }, { k: 'J', d: 40 }, { k: 'K', d: 40 }],
  editTurbo: -1, addTurbo: false,
  mousePick: null, guideNo: 1, t0: Date.now(), events: 1284,
  wizDone: ['LT', 'RT', 'A'], wizErr: false, chordGroup: 'all',
  lang: 'zh', dfo: true, moveIndep: true, advEnabled: true,
};

/* ---------------- 工具 ---------------- */
const esc = s => String(s).replace(/[&<>"]/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' }[c]));

/* 舞台缩放（H 轮机制 + J 轮 1180 基准）：纯宽度定标 + 弹性设计高度，铺满视口无黑框 */
function fitStage() {
  /* ★J 轮：设计画布 1920×1080 → 1180×760（与原生 egui 默认窗同尺寸）。
     1180×760 档 s=1 —— 字号/元素按 1180 宽直接设计、一比一呈现（不是 1920 缩 0.61 倍）；
     更大窗口等比放大。同时把 H 轮文档口径落实为**纯宽度定标**（旧代码残留 Math.min，
     纵横比宽于设计稿时会侧向 letterbox —— 2560×1400 审计档实测暴露），任何纵横比零黑框。 */
  const s = window.innerWidth / 1180;
  const stage = document.getElementById('stage-root');
  if (!stage) return;
  /* H 轮终案（宽度定标 + 弹性设计高度 + transform）：
     ① 缩放 = innerWidth/1180（宽度铺满，字号手感与设计稿同源；窗口越宽字越大）;
     ② 舞台设计高度 = innerHeight/s（弹性——顶栏吸顶、.shell 弹性伸展、页内滚动兜底），
        任何纵横比都铺满视口 → 零 letterbox 黑框（修「上下大黑框」）;
     ③ 一律 transform:scale(origin 0 0) —— CSS zoom 在非整数倍率下 Chromium 命中测试
        失准（实测 zoom=0.61 时顶栏/镶边热区收不到任何 mousedown），transform 命中精确，
        视觉与 zoom 一致。1180×760 档 s=1 时 transform 置空（原样位图路径）。 */
  stage.style.width = '1180px';
  stage.style.height = (window.innerHeight / s) + 'px';
  stage.style.transformOrigin = '0 0';
  stage.style.transform = s === 1 ? '' : 'scale(' + s + ')';
  stage.style.zoom = '';
}
window.addEventListener('resize', fitStage);

/* 覆盖层弹窗（画布内，随舞台缩放；原生 dialog 在顶层不参与 transform） */
function openDlg(id) { const el = document.getElementById(id); if (el) el.hidden = false; }
function closeDlg() { $$('.ovl').forEach(o => { o.hidden = true; }); }

function toast(msg) {
  const root = $('#toast-root');
  const el = document.createElement('div');
  el.className = 'ef-toast'; el.textContent = msg; el.setAttribute('role', 'status');
  root.appendChild(el);
  setTimeout(() => { el.classList.add('out'); setTimeout(() => el.remove(), 180); }, 2200);
}
function confirmDialog(title, body, opts) {
  opts = opts || {};
  $('#dlg-confirm-title').textContent = title;
  $('#dlg-confirm-body').textContent = body;
  const ok = $('#dlg-confirm-ok');
  ok.className = 'ef-btn small' + (opts.danger ? ' danger' : ' go');
  ok.textContent = opts.okText || '确认';
  ok.onclick = () => { closeDlg(); if (opts.onOk) opts.onOk(); };
  openDlg('dlg-confirm');
}
function setPauseUI() {
  const lt = $('#lamp-text'); if (lt) lt.textContent = state.paused ? '已暂停' : '运行中';
  const sl0 = $('#status-lamp'); if (sl0) sl0.classList.toggle('is-paused', state.paused);
  const bp = $('#btn-pause'); if (bp) bp.textContent = state.paused ? '恢复' : '暂停';
  const ob = $('#ov-pause-btn'); if (ob) ob.textContent = state.paused ? '恢复映射' : '暂停映射';
  const sp = $('#side-pause'); if (sp) sp.textContent = state.paused ? '恢复' : '暂停';
  const sl = $('#side-lamp');
  if (sl) { sl.classList.toggle('is-paused', state.paused); const st = $('#side-lamp-text'); if (st) st.textContent = state.paused ? '已暂停' : '运行中'; }
  const tl = $('#tb-lamp');
  tl.classList.toggle('is-paused', state.paused);
  tl.querySelector('i').style.background = state.paused ? 'var(--ef-signal)' : '#00FFA2';
  tl.querySelector('span').textContent = state.paused ? '已暂停' : '运行中';
}

/* ---------------- 路由 ---------------- */
/* ★J 轮: 外观 MOD 进左侧栏（与原生 egui Page::TABS 同序放最后）；CLASSIC_OK 不含 mods
   —— 经典模式与原生口径一致，不提供 MOD 管理（回完整模式再切） */
const ROUTES = ['overview', 'gamepad', 'turbo', 'whitelist', 'vibration', 'jobs', 'audio', 'settings', 'logs', 'mods'];
const PAGE_NAMES = { overview: ['概览', 'Overview'], gamepad: ['手柄映射', 'Gamepad Keys'], turbo: ['连发映射', 'Turbo'], whitelist: ['白名单', 'Whitelist'], vibration: ['通用震动', 'Vibration'], jobs: ['全职业预设', 'Job Presets'], audio: ['音频震动', 'Audio Haptics'], settings: ['设置', 'Settings'], logs: ['日志', 'Logs'], mods: ['外观 MOD', 'Appearance Mods'] };
function go(route) {
  if (!ROUTES.includes(route)) route = 'overview';
  const CLASSIC_OK = ['overview', 'gamepad', 'turbo', 'vibration', 'whitelist', 'jobs'];
  if (state.mode === 'classic' && !CLASSIC_OK.includes(route)) route = 'turbo';
  state.route = route;
  $$('.page').forEach(p => p.classList.toggle('is-active', p.dataset.page === route));
  $$('#main-nav a, .side nav a').forEach(a => {
    if (a.dataset.route === route) a.setAttribute('aria-current', 'page');
    else a.removeAttribute('aria-current');
  });
  const tp = document.getElementById('topbar-page');
  if (tp && PAGE_NAMES[route]) tp.textContent = PAGE_NAMES[route][0];
  const tpe = document.getElementById('topbar-page-en');
  if (tpe && PAGE_NAMES[route]) tpe.textContent = PAGE_NAMES[route][1];
  // 新手动线：进入手柄页时若没选中任何按键，自动选中第一个未设键
  if (route === 'gamepad' && !state.gpSlot) {
    const firstUnset = SLOTS.find(s => !DB.padSlots[s[0]]);
    if (firstUnset) selectSlot(firstUnset[0]);
  }
  window.location.hash = route;
}
window.addEventListener('hashchange', () => go(window.location.hash.slice(1)));

/* ---------------- 数字键切分区 / Ctrl+S ---------------- */
document.addEventListener('keydown', e => {
  const tag = (e.target.tagName || '').toLowerCase();
  const typing = tag === 'input' || tag === 'textarea' || tag === 'select';
  if (state.waitingCap) { handleCaptureKey(e); return; }
  if (e.key === 'Escape') {
    if (document.querySelector('.ovl:not([hidden])')) { closeDlg(); return; }
    if (state.waitingCap) { cancelCapture(); return; }
    if (state.gpSlot) { selectSlot(null); }
    return;
  }
  if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 's') { e.preventDefault(); doSave(); return; }
  if (typing || document.querySelector('.ovl:not([hidden])')) return;
  const n = parseInt(e.key, 10);
  if (n >= 1 && n <= 9) go(ROUTES[n - 1]);
});
function doSave() { toast('已全部落盘：Config.toml + 路线震动文件'); }

/* 极简模式五态句 + 预设类型切换 */
function renderMinimalState() {
  const turboOn = !state.paused;
  const vibOn = (function () { const s2 = $('#side-vib'); return s2 ? s2.checked : true; })();
  const dfoOn = state.dfo;
  const txt = !dfoOn ? 'DFO 震动已关闭（只用连发）'
    : turboOn && vibOn ? '连发与震动均已就绪'
      : turboOn && !vibOn ? '连发运行中 · 震动关闭'
        : !turboOn && vibOn ? '震动开启 · 连发已暂停'
          : '连发与震动全部关闭';
  ['#mv-state', '#mv-five'].forEach(function (sel) {
    const el = $(sel); if (el) el.textContent = txt;
  });
  const run = $('#mv-run');
  if (run) run.textContent = (state.paused ? '已暂停' : '运行中') + ' · 线程 4/4';
}

/* ---------------- 主题 / 皮肤 / 模式 ---------------- */
/* v10 双 UI 架构：本文件只服务「终末地风格」外观 MOD 包（原版 1:1 宿主在 index.html，不引本文件）。
   皮肤两态循环（恢复 v8 行为）：终末地·深色 ⇄ 终末地·浅色。
   持久化：proto-theme（沿用 v8 的 key；v9 的 dfo-skin 已随换皮方案撤销）。 */
var SKIN_STATES = [
  { skin: 'ef', theme: 'dark',  btn: '切浅色' },
  { skin: 'ef', theme: 'light', btn: '切深色' },
];
function skinStateIndex() {
  const s = document.documentElement.dataset.skin || 'ef';
  const t = document.documentElement.dataset.theme || 'dark';
  for (let i = 0; i < SKIN_STATES.length; i++) {
    if (SKIN_STATES[i].skin === s && SKIN_STATES[i].theme === t) return i;
  }
  return 0;
}
function skinStateLabel(st) {
  return '终末地 MOD · ' + (st.theme === 'dark' ? '深色' : '浅色');
}
/* 设置页「界面模式」单选回显（当前页面就是终末地 MOD 包本身，恒选「使用中」） */
function syncSkinRadios() {
  $$('input[name="skinpick"]').forEach(r => { r.checked = r.value === 'mod-ef'; });
}
/* 应用外观：写 data-theme、按钮文字、持久化、单选回显（皮肤恒为 ef） */
function applyLook(skin, theme) {
  if (theme !== 'dark' && theme !== 'light') theme = 'dark';
  document.documentElement.dataset.skin = 'ef';
  document.documentElement.dataset.theme = theme;
  const btnText = SKIN_STATES[skinStateIndex()].btn;
  const btn = $('#btn-theme'); if (btn) btn.textContent = btnText;
  const btnMini = $('#btn-theme-mini'); if (btnMini) btnMini.textContent = btnText;
  try { localStorage.setItem('proto-theme', theme); } catch (e) {}
  syncSkinRadios();
}
function applyMode(m) {
  state.mode = m;
  document.body.dataset.mode = m;
  $$('input[name="mode"], input[name="mode2"]').forEach(r => { r.checked = r.value === m; });
  if (m === 'classic') go('turbo');
  toast(m === 'full' ? '完整模式：九个分区全都在' : m === 'minimal' ? '极简模式：小窗常驻，右上角可回完整界面' : '经典模式：只留原版三页签（另加白名单 / 全职业）');
}

/* ---------------- 圆盘 ---------------- */
function polar(cx, cy, r, deg) {
  const a = (deg - 90) * Math.PI / 180;
  return [cx + r * Math.cos(a), cy + r * Math.sin(a)];
}
function arcPath(r1, r2, a0, a1) {
  const [x0, y0] = polar(300, 300, r2, a0), [x1, y1] = polar(300, 300, r2, a1);
  const [x2, y2] = polar(300, 300, r1, a1), [x3, y3] = polar(300, 300, r1, a0);
  const large = (a1 - a0) > 180 ? 1 : 0;
  return `M${x0.toFixed(1)},${y0.toFixed(1)} A${r2},${r2} 0 ${large} 1 ${x1.toFixed(1)},${y1.toFixed(1)} L${x2.toFixed(1)},${y2.toFixed(1)} A${r1},${r1} 0 ${large} 0 ${x3.toFixed(1)},${y3.toFixed(1)} Z`;
}
function drawDial() {
  const svg = $('#dial'); if (!svg) return;
  svg.innerHTML = '';
  const uptimeMin = Math.floor((Date.now() - state.t0) / 60000);
  const sections = [
    { zh: '概览', en: 'OVERVIEW', big: uptimeMin, unit: '分', sub: '系统已运行', route: 'overview' },
    { zh: '手柄映射', en: 'GAMEPAD', big: setSlotCount(), unit: '槽', sub: '已设 / 22', route: 'gamepad' },
    { zh: '连发', en: 'TURBO', big: state.kbLit.size, unit: '条', sub: '节奏 45ms', route: 'turbo' },
    { zh: '震动', en: 'VIBRATION', big: 72, unit: '%', sub: '总强度', route: 'vibration' },
    { zh: '音频', en: 'AUDIO', big: 75, unit: '%', sub: '总增益', route: 'audio' },
    { zh: '白名单', en: 'WHITELIST', big: DB.whitelist.length, unit: '条', sub: '文件名比对', route: 'whitelist' },
    { zh: '设置', en: 'SETTINGS', big: 6, unit: '区', sub: '六分区', route: 'settings' },
    { zh: '日志', en: 'LOGS', big: DB.logs.length, unit: '条', sub: '本会话', route: 'logs' },
  ];
  const N = sections.length, GAP = 1.6, step = 360 / N;
  sections.forEach((s, i) => {
    const a0 = i * step + GAP / 2, a1 = (i + 1) * step - GAP / 2;
    const mid = (a0 + a1) / 2;
    const [tx, ty] = polar(300, 300, 258, mid);
    const [ex, ey] = polar(300, 300, 292, mid);
    const g = document.createElementNS(SVGNS, 'g');
    g.setAttribute('class', 'sectg' + (s.route === state.route ? ' is-here' : ''));
    g.dataset.route = s.route;
    const p = document.createElementNS(SVGNS, 'path');
    p.setAttribute('d', arcPath(228, 288, a0, a1));
    p.setAttribute('class', 'sect');
    p.setAttribute('tabindex', '0');
    p.setAttribute('role', 'link');
    p.setAttribute('aria-label', s.zh + ' ' + s.sub);
    const beam = document.createElementNS(SVGNS, 'line');
    beam.setAttribute('x1', ex); beam.setAttribute('y1', ey);
    beam.setAttribute('x2', polar(300, 300, 200, mid)[0]); beam.setAttribute('y2', polar(300, 300, 200, mid)[1]);
    beam.setAttribute('class', 'beam');
    const zh = document.createElementNS(SVGNS, 'text');
    zh.setAttribute('x', tx); zh.setAttribute('y', ty - 6); zh.setAttribute('text-anchor', 'middle');
    zh.setAttribute('class', 't-zh'); zh.textContent = s.zh;
    const en = document.createElementNS(SVGNS, 'text');
    en.setAttribute('x', tx); en.setAttribute('y', ty + 10); en.setAttribute('text-anchor', 'middle');
    en.setAttribute('class', 't-en'); en.textContent = s.en;
    const big = document.createElementNS(SVGNS, 'text');
    big.setAttribute('x', tx); big.setAttribute('y', ty + 38); big.setAttribute('text-anchor', 'middle');
    big.setAttribute('class', 't-big'); big.textContent = s.big;
    const unit = document.createElementNS(SVGNS, 'text');
    unit.setAttribute('x', tx + (String(s.big).length * 11) + 14); unit.setAttribute('y', ty + 38);
    unit.setAttribute('class', 't-unit'); unit.textContent = s.unit;
    const sub = document.createElementNS(SVGNS, 'text');
    sub.setAttribute('x', tx); sub.setAttribute('y', ty + 54); sub.setAttribute('text-anchor', 'middle');
    sub.setAttribute('class', 't-en'); sub.textContent = s.sub;
    g.appendChild(p); g.appendChild(beam);
    g.appendChild(zh); g.appendChild(en); g.appendChild(big); g.appendChild(unit); g.appendChild(sub);
    svg.appendChild(g);
    p.addEventListener('click', () => go(s.route));
    p.addEventListener('keydown', ev => { if (ev.key === 'Enter') go(s.route); });
  });
  // 内圈 gauge：L / R 马达
  [['bar-vl', 140], ['bar-vr', 118]].forEach(([id, r]) => {
    const share = parseFloat(($(('#' + id)) || { style: {} }).style.getPropertyValue('--p') || 0);
    const track = document.createElementNS(SVGNS, 'circle');
    track.setAttribute('cx', 300); track.setAttribute('cy', 300); track.setAttribute('r', r);
    track.setAttribute('class', 'gauge-track');
    const arc = document.createElementNS(SVGNS, 'circle');
    arc.setAttribute('cx', 300); arc.setAttribute('cy', 300); arc.setAttribute('r', r);
    arc.setAttribute('class', 'gauge-arc');
    const c = 2 * Math.PI * r;
    arc.setAttribute('stroke-dasharray', `${(c * share).toFixed(1)} ${c.toFixed(1)}`);
    arc.setAttribute('transform', 'rotate(-90 300 300)');
    svg.appendChild(track); svg.appendChild(arc);
  });
  // 中央读数
  const cx = document.createElementNS(SVGNS, 'text');
  cx.setAttribute('x', 300); cx.setAttribute('y', 292); cx.setAttribute('text-anchor', 'middle');
  cx.setAttribute('class', 't-center'); cx.textContent = state.paused ? '暂停' : String(Math.floor((Date.now() - state.t0) / 60000));
  const cs = document.createElementNS(SVGNS, 'text');
  cs.setAttribute('x', 300); cs.setAttribute('y', 318); cs.setAttribute('text-anchor', 'middle');
  cs.setAttribute('class', 't-center-sub'); cs.textContent = state.paused ? 'PAUSED' : 'UPTIME · MIN';
  svg.appendChild(cx); svg.appendChild(cs);
  // 存引用：tick 只更新读数，不重建整盘（避免悬停态被打断）
  window._dialRefs = {
    cx, cs,
    arcs: $$('.gauge-arc', svg),
    shares: () => [barShare('bar-vl'), barShare('bar-vr')],
  };
}
function barShare(id) {
  const el = $('#' + id);
  return el ? parseFloat(el.style.getPropertyValue('--p') || 0) : 0;
}
function tickDial() {
  const refs = window._dialRefs, svg = $('#dial');
  if (!refs || !svg || !refs.cx.isConnected) { drawDial(); return; }
  refs.cx.textContent = state.paused ? '暂停' : String(Math.floor((Date.now() - state.t0) / 60000));
  refs.cs.textContent = state.paused ? 'PAUSED' : 'UPTIME · MIN';
  const shares = refs.shares();
  refs.arcs.forEach((arc, i) => {
    const r = +arc.getAttribute('r'), c = 2 * Math.PI * r;
    arc.setAttribute('stroke-dasharray', `${(c * (shares[i] || 0)).toFixed(1)} ${c.toFixed(1)}`);
  });
  $$('.sectg', svg).forEach(g => {
    g.classList.toggle('is-here', g.dataset.route === state.route);
  });
}

/* ---------------- 手柄热点 ---------------- */
function slotXY(s) { return { cx: 22 + s[1] * 545, cy: 135 + s[2] * 401, r: s[3] * 545 }; }
function setSlotCount() { return SLOTS.filter(s => DB.padSlots[s[0]]).length; }
function mountPad() {
  const svg = $('#pad-art svg'); if (!svg) return;
  svg.id = 'pad-svg';
  const g = document.createElementNS(SVGNS, 'g');
  g.id = 'pad-hot';
  const DIR = { Up: '↑', Down: '↓', Left: '←', Right: '→' };
  const PRE = { LS: 'L', RS: 'R', DPad: 'D' };
  SLOTS.forEach(s => {
    const [token, , , , kind] = s;
    const { cx, cy, r } = slotXY(s);
    const data = DB.padSlots[token];
    const small = kind === 'stick' || kind === 'dpad';
    const grp = document.createElementNS(SVGNS, 'g');
    grp.setAttribute('class', 'hot' + (data ? ' is-set' : '') + (small ? ' sm' : ''));
    grp.dataset.slot = token;
    const title = document.createElementNS(SVGNS, 'title');
    title.textContent = s[5] + (data ? ' → ' + data.tgt : '（未设置）');
    const c = document.createElementNS(SVGNS, 'circle');
    c.setAttribute('cx', cx.toFixed(1)); c.setAttribute('cy', cy.toFixed(1)); c.setAttribute('r', (r * 0.62).toFixed(1));
    const k = document.createElementNS(SVGNS, 'text');
    k.setAttribute('x', cx.toFixed(1)); k.setAttribute('y', (cy - (data && !small ? 5 : 4)).toFixed(1));
    k.setAttribute('class', 'k');
    let label = token;
    if (small) {
      const m = token.match(/^(LS|RS|DPad)_(\w+)$/);
      label = m ? PRE[m[1]] + (DIR[m[2]] || '') : token;
    }
    k.textContent = label;
    grp.appendChild(title); grp.appendChild(c); grp.appendChild(k);
    if (data && !small) {
      const v = document.createElementNS(SVGNS, 'text');
      v.setAttribute('x', cx.toFixed(1)); v.setAttribute('y', (cy + 12).toFixed(1));
      v.setAttribute('class', 'v'); v.textContent = data.tgt;
      grp.appendChild(v);
    }
    g.appendChild(grp);
  });
  svg.appendChild(g);
  svg.addEventListener('click', e => {
    const hot = e.target.closest('[data-slot]');
    if (hot) selectSlot(hot.dataset.slot);
  });
}
function selectSlot(token) {
  state.gpSlot = token;
  state.captures.gp = [];
  const gcap = $('#gp-cap'); if (gcap) gcap.classList.remove('is-waiting');
  $$('#pad-hot .hot').forEach(h => h.classList.toggle('is-here', h.dataset.slot === token));
  $$('#gp-unset .slot-chip').forEach(h => h.classList.toggle('is-here', h.dataset.slot === token));
  const idle = $('#gp-flow-idle'), edit = $('#gp-flow-edit'), done = $('#gp-flow-done');
  if (!token) { idle.hidden = false; edit.hidden = true; done.hidden = true; state.gpFlow = 'idle'; return; }
  idle.hidden = true; edit.hidden = false; done.hidden = true;
  state.gpFlow = 'selected';
  const s = SLOTS.find(x => x[0] === token);
  $('#gp-slot-name').textContent = s[5];
  const tk = $('#gp-slot-token'); if (tk) tk.textContent = token;
  const cat = $('#gp-slot-cat'); if (cat) cat.textContent = SLOT_CAT[s[4]] || '';
  const hid = $('#gp-slot-hid'); if (hid) hid.textContent = '键位 ' + (SLOT_HID[token] || 'H629989976');
  const kb = $('#gp-slot-kbbadge');
  if (kb) kb.innerHTML = DB.padSlots[token] ? '<span class="kbd">' + esc(DB.padSlots[token].tgt) + '</span>' : '<span class="kbd">未设置</span>';
  const sum = $('#gp-slot-summary');
  if (sum) {
    const d0 = DB.padSlots[token];
    sum.textContent = d0 ? (s[5] + ' → 键盘 ' + d0.tgt) : (s[5] + ' · 还没有设置目标键，点下方「修改键盘键」开始');
    sum.style.color = d0 ? 'var(--ef-ink)' : 'var(--ef-ink-2)';
  }
  const data = DB.padSlots[token];
  $('#gp-slot-cur').innerHTML = data
    ? `<span class="kbd is-signal">${esc(data.tgt)}</span>` + (data.chips || []).map(c => ` <span class="ef-state">${esc(c)}</span>`).join('')
    : '<span class="ef-state is-draft">未设置</span>';
  setFlowStep(1);
  const capVal = $('#gp-cap-val'); capVal.innerHTML = '';
  $('#gp-cap .cap-hint').textContent = '点击后按下键盘键';
}
function setFlowStep(n) {
  $$('#gpt-slot .flow-steps .step').forEach(st => {
    const i = parseInt(st.dataset.step, 10);
    st.classList.toggle('is-now', i === n);
    st.classList.toggle('is-done', i < n);
  });
}
function renderUnsetChips() {
  const host = $('#gp-unset');
  host.innerHTML = SLOTS.filter(s => !DB.padSlots[s[0]])
    .map(s => `<button class="slot-chip" data-slot="${s[0]}">＋ ${s[5]}</button>`).join('');
  $('#n-unset').textContent = host.children.length;
  $('#n-set').textContent = setSlotCount();
  const sc = $('#gp-set-count'); if (sc) sc.textContent = setSlotCount();
}

/* ---------------- 键盘批量连发 ---------------- */
function mountKeyboard() {
  const svg = $('#kb-board svg'); if (!svg) return;
  $$('g[id]', svg).forEach(g => g.classList.add('kb-key'));
  refreshKb();
}
function refreshKb() {
  $$('#kb-board .kb-key').forEach(g => {
    let tip = '这个键可以建连发：点一下选中';
    if (state.kbLit.has(g.id)) tip = state.kbDrop.has(g.id) ? '待移除：再点一下还原' : '已在连发中：点一下标记移除';
    else if (state.kbPick.has(g.id)) tip = '已选中：确认后新建连发';
    else if (OCC_KEYS.includes(g.id)) tip = '已被普通映射占用：新建时会提示冲突';
    let t = g.querySelector('title');
    if (!t) { t = document.createElementNS(SVGNS, 'title'); g.insertBefore(t, g.firstChild); }
    t.textContent = g.id + ' — ' + tip;
    g.classList.toggle('is-lit', state.kbLit.has(g.id));
    g.classList.toggle('is-pick', state.kbPick.has(g.id));
    g.classList.toggle('is-drop', state.kbDrop.has(g.id));
    g.classList.toggle('is-occ', OCC_KEYS.includes(g.id) && !state.kbLit.has(g.id) && !state.kbPick.has(g.id));
  });
  const applyBtn = $('#kb-apply-btn');
  if (applyBtn) applyBtn.disabled = state.kbPick.size === 0 && state.kbDrop.size === 0;
  const hint = $('#kb-hint');
  if (hint) hint.textContent = (state.kbPick.size || state.kbDrop.size)
    ? `已选 ${state.kbPick.size} 键 · 待移除 ${state.kbDrop.size} 键（再点一下可取消）`
    : '在键盘图上点一个键开始（再点一下可取消）';
  const chips = [];
  state.kbPick.forEach(k => chips.push({ k, kind: 'pick' }));
  state.kbDrop.forEach(k => chips.push({ k, kind: 'drop' }));
  $('#kb-chips').innerHTML = chips.map(c =>
    `<span class="slot-chip ${c.kind === 'pick' ? 'is-set' : ''}" style="${c.kind === 'drop' ? 'outline:2px solid var(--ef-danger)' : ''}" data-kbchip="${c.k}">${c.k} ✕</span>`).join('');
  renderTurboList();
}
function kbToggle(key) {
  if (state.kbLit.has(key)) {
    state.kbDrop.has(key) ? state.kbDrop.delete(key) : state.kbDrop.add(key);
  } else if (OCC_KEYS.includes(key)) {
    toast(key + ' 已被普通映射占用，确认修改时会提示冲突');
    state.kbPick.has(key) ? state.kbPick.delete(key) : state.kbPick.add(key);
  } else {
    state.kbPick.has(key) ? state.kbPick.delete(key) : state.kbPick.add(key);
  }
  refreshKb();
}
function kbApply() {
  if (!state.kbPick.size && !state.kbDrop.size) { toast('没有待确认的修改'); return; }
  const conflicts = [...state.kbPick].filter(k => OCC_KEYS.includes(k));
  if (conflicts.length) {
    $('#kb-conflict-text').textContent = '触发键 ' + conflicts.join('、') + ' 已被普通映射占用。覆盖将替换原映射的目标键。';
    openDlg('dlg-conflict');
    return;
  }
  state.kbPick.forEach(k => { state.kbLit.add(k); DB.turbo.push({ trig: k, tgt: k, interval: 45, dur: 30, flags: ['连发'], note: '批量新建' }); });
  state.kbDrop.forEach(k => { state.kbLit.delete(k); DB.turbo = DB.turbo.filter(m => m.trig !== k); });
  const n = state.kbPick.size + state.kbDrop.size;
  state.kbPick.clear(); state.kbDrop.clear();
  refreshKb(); renderNavCounts(); drawDial();
  toast('连发已更新（现 ' + DB.turbo.length + ' 条），配置已热重载');
}
function kbCancel() { state.kbPick.clear(); state.kbDrop.clear(); refreshKb(); toast('已撤销全部暂存修改'); }

/* ---------------- 记录行渲染 ---------------- */
function chipHtml(flags) { return (flags || []).map(f => `<span class="ef-state">${esc(f)}</span>`).join(' '); }
function renderOvMappings() {
  const host = $('#ov-mappings');
  if (!host) return;   // 概览改版后已移除映射速览卡
  const rows = DB.kbMappings.slice(0, 6).map((m, i) =>
    `<li><div class="ef-record"><span class="ef-tag">No.${String(i + 1).padStart(3, '0')}</span>
     <span class="name">${esc(m.trig)} → ${esc(m.tgt)}</span><span>${chipHtml(m.flags)}</span>
     <span class="ef-field">${esc(m.note || '')}</span></div></li>`);
  host.innerHTML = rows.join('');
}
function renderOvLogs() {
  const rows = DB.logs.slice(-6).reverse().map(l =>
    `<li><div class="ef-record"><span class="ef-tag">${l.t}</span>
     <span class="name">${esc(l.msg)}</span>
     <span class="lvl"><span class="ef-state ${l.lvl === 'error' ? 'is-alert' : l.lvl === 'warn' ? 'is-draft' : ''}">${l.lvl === 'error' ? '错误' : l.lvl === 'warn' ? '警告' : '信息'}</span></span></div></li>`);
  $('#ov-logs').innerHTML = rows.join('');
}
function turboEditCard(m, i) {
  const cb = (name) => `<label class="ef-field" style="display:inline-flex;align-items:center;gap:.4rem"><input type="checkbox" data-tflag-cb="${name}" ${m.flags.includes(name) ? 'checked' : ''}>${name}</label>`;
  return `<li><div class="ef-record" style="display:block" data-tedit-card="${i}">
    <div class="ef-actions" style="margin-bottom:12px">
      <span class="ef-tag is-current">编辑映射 No.${String(i + 1).padStart(3, '0')}</span>
      <span style="flex:1"></span>
      <button class="ef-btn small go" data-tedit-save="${i}">保存修改</button>
      <button class="ef-btn small ghost" data-tedit-cancel>取消</button>
      <button class="ef-btn small danger" data-tdelete="${i}">删除该映射</button>
    </div>
    <div class="ef-form">
      <div class="ef-actions">
        <span class="ef-field">触发键</span><span class="kbd">${esc(m.trig)}</span>
        <button class="ef-btn small ghost" data-tcap-trig="${i}">捕获触发键</button>
        <span class="ef-field" style="margin-left:12px">目标键</span><span class="kbd is-signal">${esc(m.tgt)}</span>
        <button class="ef-btn small ghost" data-tadd-tgt="${i}">＋添加目标</button>
      </div>
      <div class="ef-actions" style="align-items:center">
        <span class="ef-field">抬起时发送（可选）</span>
        <span class="ef-field">当前：${esc(m.up || '无')}</span>
        <button class="ef-btn small ghost" data-tadd-up="${i}">＋添加抬起键</button>
        <span class="ef-field">松手那一瞬额外按一下（收招 / 取消用）</span>
      </div>
      <div>
        <div class="ef-actions" style="flex-wrap:wrap">
          <span class="ef-field">序列宏</span>
          <button class="ef-btn small ghost" data-seq-record>● 录制</button>
          <button class="ef-btn small ghost" data-seq-add-key>＋按键步</button>
          <button class="ef-btn small ghost" data-seq-add-down>＋按下步 ↓</button>
          <button class="ef-btn small ghost" data-seq-add-upstep>＋抬起步 ↑</button>
          <button class="ef-btn small ghost" data-seq-add-wait>＋等待步</button>
          <button class="ef-btn small ghost" data-seq-20ms>⏱ 全部 20ms</button>
        </div>
        <p class="ef-field" id="seq-live-inline" hidden><span class="ef-state is-alert">● 录制中</span> 按顺序敲键，Esc 停止并填入。</p>
        <p class="ef-field" id="seq-parse-err" hidden><span class="ef-state is-alert">✗ 解析失败</span> 序列写法有问题，保存后本条不生效。</p>
        <p class="ef-field">${m.seq ? '🔒 已由序列宏接管——按下触发键时目标键不单独发送，连发 / 锁定 / 简易奔跑停用。' : '未挂载序列宏：正常按「触发键 → 目标键」发送。'}</p>
        <details class="adv" style="margin-top:6px"><summary>高级：直接编辑序列文本</summary><div class="inner">
          <textarea data-tseq="${i}" rows="2" style="width:100%" placeholder="J[40] J[40] 等待[200] K">${esc(m.seq || '')}</textarea>
        </div></details>
      </div>
      <div class="ef-actions" style="flex-wrap:wrap;row-gap:8px">
        <label class="ef-field">间隔 <input type="number" value="${m.interval}" min="1" max="5000" style="width:5.5rem" data-tedit="interval"> ms</label>
        <label class="ef-field">按住 <input type="number" value="${m.dur}" min="1" max="5000" style="width:5.5rem" data-tedit="dur"> ms</label>
        ${cb('连发')}${cb('锁定')}${cb('简易奔跑')}
        <span class="ef-field">锁定与奔跑互斥，同时勾会提示</span>
      </div>
      <div class="ef-actions" style="flex-wrap:wrap">
        <span class="ef-field">鼠标</span>
        <button class="ef-btn small ghost" data-act="mouse-dialog">⌖ 方向</button>
        <button class="ef-btn small ghost" data-act="mouse-dialog">🎡 滚动</button>
        <label class="ef-field">移动速度 <input type="number" value="${m.speed || 5}" min="1" max="1200" style="width:5rem" data-tspeed="${i}"></label>
      </div>
      <div class="ef-actions" style="flex-wrap:wrap">
        <span class="ef-field">重推奔跑</span>
        <input type="checkbox" class="ef-switch" data-trerun="${i}" ${m.flags.includes('重推奔跑') ? 'checked' : ''}>
        <label class="ef-field">重推阈值 <input type="range" class="ef-range" data-trerun-th="${i}" min="50" max="95" value="${m.rerunTh || 62}" style="width:8rem"></label>
        <label class="ef-field">二次敲击间隔 <input type="number" value="${m.rerunGap || 120}" min="20" max="200" style="width:5rem" data-trerun-gap="${i}"> ms</label>
        <label class="ef-field" style="display:inline-flex;align-items:center;gap:.4rem"><input type="checkbox" data-trerun-again="${i}" ${m.rerunAgain ? 'checked' : ''}>再检测</label>
      </div>
      <div class="ef-actions"><span class="ef-field">备注</span><input type="text" value="${esc(m.note || '')}" data-tnote="${i}" style="flex:1;min-width:12rem" placeholder="给自己看的标注，列表里显示前 14 个字"></div>
    </div>
  </div></li>`;
}
function renderTurboList() {
  const host = $('#turbo-list');
  const rows = DB.turbo.map((m, i) => {
    if (state.editTurbo === i) return turboEditCard(m, i);
    const note = m.note ? ' · ' + esc(String(m.note).slice(0, 14)) + (String(m.note).length > 14 ? '…' : '') : '';
    return `<li><div class="ef-record" data-tedit-open="${i}">
      <span class="ef-tag">No.${String(i + 1).padStart(3, '0')}</span>
      <span class="name">${esc(m.trig)} → ${esc(m.tgt)}</span>
      <span>${chipHtml(m.flags)}${m.flags.includes('锁定') ? '<span class="ef-state">🔒 锁定</span>' : ''}${m.seq ? '<span class="ef-state">⚡ 宏</span>' : ''}<span class="ef-field" title="${esc(m.note || '')}">每 ${m.interval}ms 一次（约每秒 ${Math.round(1000 / m.interval)} 次）· 按住 ${m.dur}ms${note}</span></span>
      <span class="acts"><button class="ef-btn small ghost" data-tedit-open="${i}">调速</button>
      <button class="ef-btn small danger" data-tdelete="${i}">删除</button></span>
    </div></li>`;
  }).join('');
  const wrap = host.closest('.tb-list');
  if (wrap) wrap.classList.toggle('is-editing', state.editTurbo >= 0);
  host.innerHTML = rows || '<li><div class="ef-record"><span class="ef-field">还没有连发。在上方键盘图点键批量新建，或点卡片标题右侧「＋手动新建」。</span></div></li>';
}
function chordGroupOf(c) {
  const k = c.combo.split('+');
  if (k.some(x => x === 'LT' || x === 'RT')) return 'trig';
  if (k.some(x => x === 'LB' || x === 'RB')) return 'shoulder';
  return 'other';
}
function renderChords() {
  const q = ($('#chord-search') ? $('#chord-search').value : '').toLowerCase();
  const g = state.chordGroup;
  const shown = DB.chords.filter(c =>
    (c.combo.toLowerCase().includes(q) || c.tgt.toLowerCase().includes(q)) &&
    (g === 'all' || chordGroupOf(c) === g));
  const rows = shown.map(c => {
    const i = DB.chords.indexOf(c);
    return `<li><div class="ef-record"><span class="ef-tag">No.${String(i + 1).padStart(3, '0')}</span>
     <span class="name">${esc(c.combo)} → ${esc(c.tgt)}</span>
     <span>${(c.members || []).map(k => `<span class="ef-state">${esc(k)} 让位</span>`).join(' ')}${c.seq ? '<span class="ef-state">⚡ 宏</span>' : ''}<span class="ef-field">${c.seq ? '按下组合键执行脚本' : '组合键 → 单键'}</span></span>
     <span class="acts"><button class="ef-btn small ghost" data-chord-retarget="${i}">改目标</button>
     <button class="ef-btn small danger" data-chord-del="${i}">删除</button></span></div>
     ${c.seq ? `<div class="chord-seq${c.seqBad ? ' is-bad' : ''}">⚡ 序列宏：${c.seqBad ? '<b>✗ 解析失败 · 本条组合键当前不生效</b>' : esc(c.seq)}</div>` : ''}</li>`;
  });
  $('#chord-list').innerHTML = rows.join('') || '<li><div class="ef-record"><span class="ef-field">这个分组下还没有组合键。</span></div></li>';
  $('#n-combos').textContent = DB.chords.length;
  const gc = $('#chord-group-count');
  if (gc) gc.textContent = '显示 ' + shown.length + ' / 共 ' + DB.chords.length + ' 条';
}
function renderMacros() {
  const host = $('#macros-list');
  if (!DB.macros.length) {
    host.innerHTML = '<li><div class="ef-record"><span class="ef-field">还没有通用宏——在下面填个名字和序列就能存一个，之后任意映射都能引用。</span></div></li>';
    return;
  }
  host.innerHTML = DB.macros.map((m, i) =>
    `<li><div class="ef-record" data-macro-use="${i}" title="点整行引用到当前序列">
     <span class="ef-tag">No.${String(i + 1).padStart(3, '0')}</span>
     <span class="name">宏(${esc(m.name)})</span><span class="ef-field">${esc(m.text)}</span>
     <span class="acts"><button class="ef-btn small ghost" data-macro-use="${i}">引用</button>
     <button class="ef-btn small danger" data-macro-del="${i}">删除</button></span></div></li>`).join('');
}
function renderSeq() {
  $('#seq-steps').innerHTML = state.seqSteps.map((s, i) =>
    `<div class="seq-step"><span class="no">${String(i + 1).padStart(2, '0')}</span>
     <span>${s.wait ? '等待' : '按键'} <span class="kbd">${esc(s.k)}</span></span>
     <input type="number" value="${s.d}" min="0" max="5000" style="width:4.5rem" data-seqd="${i}" aria-label="时长">
     <button class="ef-btn small danger" data-seq-del="${i}">✕</button></div>`).join('')
    || '<p class="ef-field">空序列。点击「＋按键步」或「● 录制按键」开始。</p>';
  $('#seq-warn').hidden = state.seqSteps.length === 0;
}
function wlRows(delAttr) {
  return DB.whitelist.map((w, i) =>
    `<li><div class="ef-record"><span class="ef-tag">No.${String(i + 1).padStart(3, '0')}</span>
     <span class="name">${esc(w.name)}</span><span class="ef-field">${esc(w.path || '按文件名登记（宽松比对）')}</span>
     <span class="acts"><button class="ef-btn small danger" ${delAttr}="${i}">移除</button></span></div></li>`).join('');
}
function renderWl() {
  $('#wl-list').innerHTML = wlRows('data-wl-del')
    || '<li><div class="ef-record"><span class="ef-field">名单是空的 —— 空名单 = 所有程序都能用连发（等于没开白名单）。加一个 dnf.exe 最省事。</span></div></li>';
  const sw = $('#sw-wl');
  const setHost = $('#set-wl');
  if (setHost) setHost.innerHTML = wlRows('data-setwl-del')
    || '<li><div class="ef-record"><span class="ef-field">还没有进程。上面填一个加进来。</span></div></li>';
  const wc = $('#wl-count'); if (wc) wc.textContent = DB.whitelist.length;
  const sc = $('#set-wl-count'); if (sc) sc.textContent = DB.whitelist.length;
  const st = $('#wl-state');
  if (st && sw && !sw.checked) {
    st.dataset.flag = '注意';
    st.textContent = '已关闭：所有程序都会触发连发，可能误触其他软件。名单仍然保留在配置里，重新打开开关就立刻恢复。';
  }
  renderNavCounts();
}
function renderSetMappings() {
  $('#set-mappings').innerHTML = DB.kbMappings.map((m, i) =>
    `<li><div class="ef-record"><span class="ef-tag">No.${String(i + 1).padStart(3, '0')}</span>
     <span class="name">${esc(m.trig)} → ${esc(m.tgt)}</span>
     <span>${chipHtml(m.flags)}<span class="ef-field">${esc(m.note || '无备注')}</span></span>
     <span class="acts"><button class="ef-btn small ghost" data-set-dir="${i}">方向</button>
     <button class="ef-btn small ghost" data-set-scroll="${i}">滚动</button>
     <button class="ef-btn small danger" data-set-del="${i}">删除</button></span></div>
     <details class="adv" style="margin:0 0 10px"><summary>行内编辑：目标键 / 鼠标三参数 / 连发 / 奔跑 / 重推 / 备注</summary>
       <div class="inner">
         <div class="ef-actions" style="row-gap:10px">
           <span class="ef-field">目标键</span><span class="kbd is-signal">${esc(m.tgt)}</span>
           <button class="ef-btn small ghost" data-set-retgt="${i}">改目标键</button>
           <span class="ef-field">多个目标可叠加（chips）</span>
         </div>
         <div class="ef-actions mt" style="row-gap:10px">
           <span class="ef-field">鼠标参数</span>
           <label class="ef-field">方向 <select aria-label="鼠标方向"><option>（无）</option><option>上</option><option>下</option><option>左</option><option>右</option></select></label>
           <label class="ef-field">滚动 <select aria-label="滚动"><option>（无）</option><option>滚轮上</option><option>滚轮下</option></select></label>
           <label class="ef-field">移动速度 <input type="number" value="5" min="1" max="1200" style="width:5rem"></label>
         </div>
         <div class="ef-actions mt" style="row-gap:10px">
           <label class="ef-field" style="display:inline-flex;align-items:center;gap:.4rem"><input type="checkbox" ${m.flags.includes('连发') ? 'checked' : ''}>⚡ 连发</label>
           <label class="ef-field" style="display:inline-flex;align-items:center;gap:.4rem"><input type="checkbox" ${m.flags.includes('简易奔跑') ? 'checked' : ''}>简易奔跑（与连发互斥）</label>
           <label class="ef-field" style="display:inline-flex;align-items:center;gap:.4rem"><input type="checkbox" ${m.flags.includes('重推奔跑') ? 'checked' : ''}>重推奔跑</label>
           <label class="ef-field">备注 <input type="text" value="${esc(m.note || '')}" style="width:10rem"></label>
         </div>
       </div>
     </details></li>`).join('');
}
function renderAudioMine() {
  const host = $('#audio-mine');
  if (!DB.audioMine.length) { host.innerHTML = '<li><div class="ef-record"><span class="ef-field">还没有自己的音频预设。调好手感后，在下面输入名字保存。</span></div></li>'; return; }
  host.innerHTML = DB.audioMine.map((n, i) =>
    `<li><div class="ef-record" data-audio-apply="${i}" title="点击整行应用"><span class="ef-tag">No.${String(i + 1).padStart(3, '0')}</span>
     <span class="name">${esc(n)}</span>
     <span class="ef-field">自定义预设 · 点击整行应用</span>
     <span class="acts">${DB.audioUsing === n ? '<span class="ef-state is-alert">使用中</span>' : ''}
     <button class="ef-btn small ghost" data-audio-apply="${i}">应用</button>
     <button class="ef-btn small danger" data-audio-del="${i}">删除</button></span></div></li>`).join('');
}
function renderLogs() {
  let list = DB.logs.filter(l =>
    state.logLevel === 'all' ? true :
    state.logLevel === 'attention' ? l.lvl !== 'info' : l.lvl === state.logLevel);
  if (state.logSearch) list = list.filter(l => l.msg.toLowerCase().includes(state.logSearch.toLowerCase()));
  $('#log-list').innerHTML = list.map(l =>
    `<li><div class="ef-record"><span class="ef-tag">${l.t}</span>
     <span class="name">${esc(l.msg)}</span>
     <span class="lvl"><span class="ef-state ${l.lvl === 'error' ? 'is-alert' : l.lvl === 'warn' ? 'is-draft' : ''}">${l.lvl === 'error' ? '需要处理' : l.lvl === 'warn' ? '需要注意' : '信息'}</span></span></div></li>`).join('');
  $('#log-total').textContent = DB.logs.length;
  $('#n-log-all').textContent = DB.logs.length;
  $('#n-log-warn').textContent = DB.logs.filter(l => l.lvl !== 'info').length;
  $('#n-log-err').textContent = DB.logs.filter(l => l.lvl === 'error').length;
}
function renderSwitchChips() {
  const host = $('#tb-switch-chips');
  if (!host) return;
  const keys = state.switchKeys || [];
  host.innerHTML = keys.length
    ? keys.map((k, i) => `<span class="slot-chip is-set" data-swkey-del="${i}">${esc(k)} ✕</span>`).join('')
      + `<span class="ef-field">共 ${keys.length} / 3 个键</span>`
    : '<span class="ef-field">还没设置切换键（点上面的捕获框按键，或直接用默认 DELETE）</span>';
}
function renderSelects() {
  const opts = DB.presets.map(p => `<option${p === DB.currentPreset ? ' selected' : ''}>${esc(p)}</option>`).join('')
    + '<option value="">（无预设 · 只清当前预设名，保留现有映射）</option>';
  ['#quick-preset', '#top-preset', '#tb-preset', '#set-preset', '#m-preset'].forEach(sel => {
    const el = $(sel); if (el) el.innerHTML = opts;
  });
  const vp = $('#vib-preset');
  if (vp) vp.innerHTML = DB.vibPresets.map(p => `<option>${esc(p)}</option>`).join('');
}
function renderNavCounts() {
  $$('[data-count]').forEach(el => {
    const k = el.dataset.count;
    el.textContent = k === 'mappings' ? setSlotCount() : k === 'turbo' ? DB.turbo.length : k === 'whitelist' ? DB.whitelist.length : DB.logs.length;
  });
  $('#fig-mappings').textContent = setSlotCount();
  $('#fig-turbo').textContent = DB.turbo.length;
  $('#fig-combos').textContent = DB.chords.length;
  $('#ov-macros').textContent = DB.macros.length + ' 个通用宏';
  const op = $('#ov-preset'); if (op) op.textContent = DB.currentPreset;
  const tbn = $('#tb-n-mapping'); if (tbn) tbn.textContent = DB.kbMappings.length;
  const tbw = $('#tb-n-worker'); if (tbw) tbw.textContent = '4';
}

/* ---------------- 捕获组件 ---------------- */
const KEYMAP = { ShiftLeft: 'LSHIFT', ShiftRight: 'RSHIFT', ControlLeft: 'LCTRL', ControlRight: 'RCTRL', AltLeft: 'LALT', AltRight: 'RALT',
  MetaLeft: 'LWIN', MetaRight: 'RWIN', Space: 'SPACE', Enter: 'ENTER', Escape: 'ESC', Backspace: 'BACKSPACE', Tab: 'TAB',
  CapsLock: 'CAPSLOCK', Delete: 'DELETE', Insert: 'INSERT', Home: 'HOME', End: 'END', PageUp: 'PAGEUP', PageDown: 'PAGEDOWN',
  ArrowUp: 'UP', ArrowDown: 'DOWN', ArrowLeft: 'LEFT', ArrowRight: 'RIGHT',
  Semicolon: 'OEM_1', Quote: 'OEM_7', Backquote: 'OEM_3', Minus: 'OEM_MINUS', Equal: 'OEM_PLUS',
  BracketLeft: 'OEM_4', BracketRight: 'OEM_6', Backslash: 'OEM_5', Comma: 'OEM_COMMA', Period: 'OEM_PERIOD', Slash: 'OEM_2',
  PrintScreen: 'SNAPSHOT', ScrollLock: 'SCROLL', Pause: 'PAUSE', NumpadDivide: 'DIVIDE', NumpadMultiply: 'MULTIPLY',
  NumpadSubtract: 'SUBTRACT', NumpadAdd: 'ADD', NumpadDecimal: 'DECIMAL', ContextMenu: 'MENU' };
function keyName(e) {
  if (e.code.startsWith('Key')) return e.code.slice(3);
  if (e.code.startsWith('Digit')) return e.code.slice(5);
  if (e.code.startsWith('Numpad') && /Numpad\d/.test(e.code)) return 'NUMPAD' + e.code.slice(6);
  if (/^F\d+$/.test(e.code)) return e.code;
  return KEYMAP[e.code] || e.key.toUpperCase();
}
function startCapture(name) {
  if (state.waitingCap) cancelCapture();
  const el = $(`.capture[data-cap="${name}"]`);
  if (!el) { toast('（原型）请按任意键…'); state.waitingCap = name; return; }
  state.waitingCap = name;
  el.classList.add('is-waiting');
  el.querySelector('.cap-hint').textContent = '等待按键… Esc 取消';
}
function cancelCapture() {
  if (!state.waitingCap) return;
  const el = $(`.capture[data-cap="${state.waitingCap}"]`);
  if (el) {
    el.classList.remove('is-waiting');
    el.querySelector('.cap-hint').textContent = '点击后按下键盘键';
  }
  state.waitingCap = null;
}
function handleCaptureKey(e) {
  e.preventDefault(); e.stopPropagation();
  if (e.key === 'Escape') { cancelCapture(); return; }
  const name = state.waitingCap;
  const isMod = e.key === 'Shift' || e.key === 'Control' || e.key === 'Alt' || e.key === 'Meta';
  const arr = state.captures[name] || (state.captures[name] = []);
  const kn = keyName(e);
  if (isMod) { if (!arr.includes(kn)) arr.push(kn); renderCapture(name); return; }
  if (!arr.includes(kn)) arr.push(kn);
  state.waitingCap = null;
  const el = $(`.capture[data-cap="${name}"]`);
  if (el) {
    el.classList.remove('is-waiting');
    const h = el.querySelector('.cap-hint'); if (h) h.textContent = '已捕获';
  }
  renderCapture(name);
  if (name === 'gp') {
    state.gpFlow = 'done';
    const slot = state.gpSlot || 'A';
    const tgt = arr.join('+');
    if (!DB.padSlots[slot]) DB.padSlots[slot] = {};
    DB.padSlots[slot].tgt = tgt;
    const kb2 = $('#gp-slot-kbbadge');
    if (kb2) kb2.innerHTML = '<span class="kbd">' + esc(tgt) + '</span>';
    const sum2 = $('#gp-slot-summary');
    if (sum2) { const s2 = SLOTS.find(x => x[0] === slot); sum2.textContent = s2[5] + ' → 键盘 ' + tgt; sum2.style.color = 'var(--ef-ink)'; }
    mountPadRefresh();
    setFlowStep(3);
    const qs = document.getElementById('gp-qs'); if (qs) qs.hidden = true;
    $('#gp-flow-edit').hidden = true;
    $('#gp-flow-done').hidden = false;
    $('#gp-done-text').innerHTML = `✓ ${esc(slot)} 键 → 键盘 <span class="kbd is-signal">${esc(tgt)}</span>`;
    renderUnsetChips(); renderNavCounts(); drawDial();
  }
  if (name === 'chord') {
    $('#chord-cap-val').innerHTML = `<span class="kbd">${esc(arr.join('+'))}</span>`;
    $$('#gpt-combo .flow-steps .step').forEach(st => {
      st.classList.toggle('is-done', st.dataset.step === '1');
      st.classList.toggle('is-now', st.dataset.step === '2');
    });
  }
  if (name === 'tb-switch') {
    state.switchKeys = state.switchKeys || [];
    if (state.switchKeys.length >= 3) { toast('最多 3 个键：先删掉一个再捕获'); }
    else { arr.forEach(k => { if (!state.switchKeys.includes(k)) state.switchKeys.push(k); }); renderSwitchChips(); }
  }
  if (name === 'chord-retarget') {
    if (DB.chords[0]) DB.chords[0].tgt = arr.join('+');
    renderChords();
  }
  toast('已捕获：' + arr.join('+'));
}
function renderCapture(name) {
  const arr = state.captures[name] || [];
  const host = $(`.capture[data-cap="${name}"]`);
  if (!host) return;
  let val = host.querySelector('span[id$="-val"]') || host.querySelector('.cap-val');
  if (!val) { val = document.createElement('span'); val.className = 'cap-val'; host.appendChild(val); }
  val.innerHTML = arr.map(k => `<span class="kbd">${esc(k)}</span>`).join(' ');
}
function mountPadRefresh() {
  $$('#pad-hot .hot').forEach(h => {
    const d = DB.padSlots[h.dataset.slot];
    const small = h.classList.contains('sm');
    h.classList.toggle('is-set', !!d);
    const v = h.querySelector('.v');
    if (d && !v && !small) {
      const t = document.createElementNS(SVGNS, 'text');
      t.setAttribute('class', 'v');
      const s = SLOTS.find(x => x[0] === h.dataset.slot);
      const { cx, cy } = slotXY(s);
      t.setAttribute('x', cx.toFixed(1)); t.setAttribute('y', (cy + 12).toFixed(1));
      t.textContent = d.tgt; h.appendChild(t);
    } else if (d && v) v.textContent = '→ ' + d.tgt;
    else if (!d && v) v.remove();
    const title = h.querySelector('title');
    if (title) title.textContent = h.dataset.slot + (d ? ' → ' + d.tgt : '（未设置）');
  });
}

/* ---------------- 滑杆 ---------------- */
function initSliders() {
  $$('.ef-range').forEach(r => {
    const row = r.closest('.slider-row');
    const val = row ? row.querySelector('.val') : null;
    const min = +r.min || 0, max = +r.max || 100;
    let unit = '';
    if (val) {
      const m = val.textContent.match(/(Hz|ms|%|×|次)$/);
      unit = m ? m[1] : (/^\d+\.\d+$/.test(val.textContent.trim()) ? 'frac' : '');
    }
    r._unit = unit;
    updateSlider(r);
    r.addEventListener('input', () => { updateSlider(r); scheduleSave(); });
  });
}
function updateSlider(r) {
  const min = +r.min || 0, max = +r.max || 100;
  const p = (r.value - min) / (max - min);
  r.style.setProperty('--p', p.toFixed(3));
  const row = r.closest('.slider-row');
  const val = row ? row.querySelector('.val') : null;
  if (val) {
    val.textContent = r._unit === 'frac' ? (r.value / 100).toFixed(2)
      : r._unit ? r.value + r._unit : r.value;
  }
}
let saveTimer = null;
function scheduleSave() {
  clearTimeout(saveTimer);
  saveTimer = setTimeout(() => toast('滑杆已生效，自动保存（1s 去抖 · 原型模拟）'), 1000);
}

/* ---------------- 实时读数 tick ---------------- */
function tick() {
  const up = Math.floor((Date.now() - state.t0) / 1000);
  const mm = String(Math.floor(up / 60)).padStart(2, '0'), ss = String(up % 60).padStart(2, '0');
  const ov = $('#ov-uptime'); if (ov) ov.textContent = mm + ':' + ss;
  if (!state.paused) {
    state.events += Math.floor(Math.random() * 3);
    const ev = $('#fig-vib-events'); if (ev) ev.textContent = state.events;
    const wobble = (base, amp) => Math.max(0, Math.min(1, base + (Math.random() - .5) * amp));
    [['bar-vl', 'num-vl', .12], ['bar-vr', 'num-vr', .08]].forEach(([b, n, base]) => {
      const v = wobble(base, .06);
      const bar = $('#' + b); bar.style.setProperty('--p', v.toFixed(3));
      $('#' + n).textContent = String(Math.round(v * 100)).padStart(3, '0') + '%';
    });
    [['vbar-l', 'vnum-l', .35], ['vbar-r', 'vnum-r', .22]].forEach(([b, n, base]) => {
      const v = wobble(base, .1);
      $('#' + b).style.setProperty('--p', v.toFixed(3));
      $('#' + n).textContent = String(Math.round(v * 100)).padStart(3, '0') + '%';
    });
    [['abar-l', 'anum-l', .28], ['abar-r', 'anum-r', .18]].forEach(([b, n, base]) => {
      const v = wobble(base, .12);
      $('#' + b).style.setProperty('--p', v.toFixed(3));
      $('#' + n).textContent = String(Math.round(v * 100)).padStart(3, '0') + '%';
    });
    $$('#audio-bands li').forEach((li, i) => {
      li.style.setProperty('--v', wobble(.58 - i * .12, .14).toFixed(2));
    });
    $$('#ov-bands li').forEach((li, i) => {
      li.style.setProperty('--v', wobble(.62 - i * .13, .16).toFixed(2));
    });
    $('#a-noise').textContent = (-52.3 + (Math.random() * 2 - 1)).toFixed(1) + ' dB';
    $('#a-trans').textContent = (0.412 + (Math.random() * .2 - .1)).toFixed(3);
  }
  if (!state.paused) {
    const et = $('#vib-evt-total');
    if (et) et.textContent = (state.events = state.events + Math.floor(Math.random() * 3));
    const live = $('#ai-live');
    if (live && state.route === 'audio') {
      const far = Math.random() > .5 ? '近' : '远';
      const pitch = Math.random() > .45 ? '低' : '中高';
      const ms = 260 + Math.floor(Math.random() * 520);
      const l = 40 + Math.floor(Math.random() * 40), r = 100 - l;
      live.textContent = '距离 ' + far + ' · 音高 ' + pitch + ' · 预计持续 ' + ms + 'ms · 左 ' + l + '% / 右 ' + r + '%';
    }
    if (Math.random() > .8) {
      const v = $('#a-verdict');
      if (v) {
        const pool = [['战斗声', true], ['环境音', false], ['音乐', false], ['安静', false]];
        const pick = pool[Math.floor(Math.random() * pool.length)];
        v.textContent = '判定：' + pick[0];
        v.className = 'ef-state' + (pick[1] ? ' is-alert' : '');
      }
    }
  }
  tickDial();
}

/* ---------------- 引导 ---------------- */
const GUIDE = [
  '<p>你在玩 DNF 吗？</p><p style="margin:6px 0 0">选「是」= 联动开启震动引擎与 DLL 通道；选「只用连发」= 隐藏全部震动分区（之后可在设置 02 改回来）。</p>'
    + '<div class="ef-actions" style="margin-top:12px"><button class="ef-btn small go" data-act="guide-dfo-yes">是，我在玩 DNF</button>'
    + '<button class="ef-btn small" data-act="guide-dfo-no">只用连发</button></div>'
    + '<p class="ef-field" id="guide-pick1" style="margin:10px 0 0">当前选择：震动 + 连发（默认）</p>',
  '<p>客户端是哪一版？</p><p style="margin:6px 0 0"><b>S1 · ACT 注入版</b> — 旧版客户端，要 DfoVibration.dll 在程序目录；<br><b>S4+ · 通用版</b> — 当前正式服，不依赖注入，参数体系独立。</p>'
    + '<div class="ef-actions" style="margin-top:12px"><button class="ef-btn small" data-act="guide-route-s1">S1 ACT 注入版</button>'
    + '<button class="ef-btn small go" data-act="guide-route-s4">S4+ 通用版（正式服）</button></div>'
    + '<p class="ef-field" id="guide-pick2" style="margin:10px 0 0">当前选择：S4+ 通用版</p>',
  '<p>连发：在「连发」分区点键盘图批量创建，行内可调间隔与按压时长；游戏内按 <span class="kbd">DELETE</span> 一键暂停 / 恢复全部连发。</p>',
  '<p>手柄映射：点击手柄图上的槽位热点 → 按下键盘键 → 完成。支持 <span class="kbd">LT</span>+<span class="kbd">LB</span> 这类组合键与序列宏挂载。</p>',
  '<p>模式与托盘：极简模式小窗常驻适合挂机；关闭窗口可选择最小化到托盘。全部修改实时生效，自动落盘。</p>',
];
function renderGuide() {
  $('#guide-no').textContent = state.guideNo;
  $('#guide-body').innerHTML = GUIDE[state.guideNo - 1];
  $('#guide-back').disabled = state.guideNo === 1;
  $('#guide-next').textContent = state.guideNo === GUIDE.length ? '开始使用' : '下一步';
}

/* ---------------- 事件委托 ---------------- */
document.addEventListener('click', e => {
  if (e.target.closest('[data-close]')) { closeDlg(); return; }
  const nav = e.target.closest('[data-route]');
  if (nav && !nav.closest('.ef-dial')) { go(nav.dataset.route); return; }
  const actEl = e.target.closest('[data-act]');
  if (actEl) { handleAct(actEl.dataset.act, actEl, e); return; }
  const slotChip = e.target.closest('.slot-chip[aria-pressed]');
  if (slotChip && !slotChip.closest('#audio-tiles')) {
    const on = slotChip.getAttribute('aria-pressed') !== 'true';
    slotChip.setAttribute('aria-pressed', on);
    slotChip.classList.toggle('is-set', on);
    return;
  }
  const cap = e.target.closest('.capture[data-cap]');
  if (cap) { startCapture(cap.dataset.cap); return; }
  const hot = e.target.closest('#kb-board .kb-key');
  if (hot) { kbToggle(hot.id); return; }
  const kbchip = e.target.closest('[data-kbchip]');
  if (kbchip) {
    const k = kbchip.dataset.kbchip;
    state.kbPick.delete(k); state.kbDrop.delete(k); refreshKb(); return;
  }
  const anchor = e.target.closest('[data-anchor]');
  if (anchor) {
    const t = document.querySelector(anchor.dataset.anchor);
    if (t) t.scrollIntoView({ block: 'start' });
    return;
  }
  const unset = e.target.closest('#gp-unset .slot-chip');
  if (unset) { selectSlot(unset.dataset.slot); return; }
  const gpTab = e.target.closest('[data-gptab]');
  if (gpTab) {
    $$('#gp-tabs .ef-chip').forEach(c => { c.classList.toggle('is-active', c === gpTab); c.setAttribute('aria-pressed', c === gpTab); });
    $('#gpt-slot').hidden = gpTab.dataset.gptab !== 'slot';
    $('#gpt-combo').hidden = gpTab.dataset.gptab !== 'combo';
    $('#gpt-wizard').hidden = gpTab.dataset.gptab !== 'wizard';
    const grid = document.querySelector('.gp-grid');
    if (grid) { grid.classList.toggle('is-tabbed', gpTab.dataset.gptab !== 'slot'); grid.dataset.gptab = gpTab.dataset.gptab; }
    return;
  }
  const gpFilter = e.target.closest('[data-gpfilter]');
  if (gpFilter) {
    $$('#page-gamepad [data-gpfilter]').forEach(c => c.setAttribute('aria-pressed', c === gpFilter));
    toast('筛选：' + gpFilter.dataset.gpfilter);
    return;
  }
  const chordGroup = e.target.closest('[data-chordgroup]');
  if (chordGroup) {
    $$('#gpt-combo [data-chordgroup]').forEach(c => c.setAttribute('aria-pressed', c === chordGroup));
    state.chordGroup = chordGroup.dataset.chordgroup;
    renderChords();
    return;
  }
  const aiMode = e.target.closest('[data-act="ai-mode-a"], [data-act="ai-mode-b"]');
  if (aiMode) {
    const toB = aiMode.dataset.act === 'ai-mode-b';
    const a = $('#ai-mode-a'), b = $('#ai-mode-b'), sw = $('#sw-ai'), st = $('#ai-state');
    if (b) { b.classList.toggle('is-set', toB); b.setAttribute('aria-pressed', toB); }
    if (a) { a.classList.toggle('is-set', !toB); a.setAttribute('aria-pressed', !toB); }
    if (sw) sw.checked = toB;
    if (st) st.textContent = toB ? '当前 B 智能模式 · 监听中——智能层参与输出' : '当前 A 经典模式 / 静音中——智能层未参与输出';
    toast(toB ? '已切换 B AI 智能模式（远近高低自适应）' : '已切换 A 经典模式（原手感，逐字节不变）');
    return;
  }
  const tile = e.target.closest('#audio-tiles .slot-chip, #vib-tiles .slot-chip');
  if (tile) {
    $$('#' + tile.parentElement.id + ' .slot-chip').forEach(c => {
      const on = c === tile;
      c.classList.toggle('is-set', on);
      c.setAttribute('aria-pressed', on);
      const base = c.dataset.name || c.textContent;
      c.textContent = (on ? '✓ ' : '') + base;
    });
    toast('已应用「' + (tile.dataset.name || tile.textContent) + '」');
    return;
  }
  const lvl = e.target.closest('[data-loglevel]');
  if (lvl) {
    $$('#page-logs [data-loglevel]').forEach(c => c.setAttribute('aria-pressed', c === lvl));
    state.logLevel = lvl.dataset.loglevel; renderLogs(); return;
  }
  const mk = e.target.closest('[data-mk]');
  if (mk && !mk.disabled) {
    $$('#dlg-mouse [data-mk]').forEach(b => b.classList.remove('is-here'));
    mk.classList.add('is-here');
    state.mousePick = mk.dataset.mk;
    return;
  }
  const newflag = e.target.closest('[data-newflag]');
  if (newflag) {
    newflag.setAttribute('aria-pressed', newflag.getAttribute('aria-pressed') !== 'true');
    newflag.classList.toggle('is-active');
    return;
  }
  const macroUse = e.target.closest('[data-macro-use]');
  if (macroUse) {
    const m = DB.macros[+macroUse.dataset.macroUse];
    if (m) { state.seqSteps.push({ k: '宏(' + m.name + ')', d: 0 }); renderSeq(); toast('已引用通用宏「' + m.name + '」到当前序列'); }
    return;
  }
  const swDel = e.target.closest('[data-swkey-del]');
  if (swDel) { state.switchKeys.splice(+swDel.dataset.swkeyDel, 1); renderSwitchChips(); return; }
  const setWlDel = e.target.closest('[data-setwl-del]');
  if (setWlDel) {
    const i = +setWlDel.dataset.setwlDel;
    DB.whitelist.splice(i, 1); renderWl(); toast('已移除，该进程立即恢复响应连发');
    return;
  }
  const upkg = e.target.closest('[data-updpkg]');
  if (upkg) {
    $$('#dlg-update [data-updpkg]').forEach(b => { const on = b === upkg; b.classList.toggle('is-set', on); b.setAttribute('aria-pressed', on); });
    toast('下载包：' + upkg.textContent.trim());
    return;
  }
  const teditOpen = e.target.closest('[data-tedit-open]');
  if (teditOpen) {
    state.editTurbo = +teditOpen.dataset.teditOpen; renderTurboList();
    const card = document.querySelector('[data-tedit-card]');
    if (card) card.scrollIntoView({ block: 'center' });
    return;
  }
  const teditCancel = e.target.closest('[data-tedit-cancel]');
  if (teditCancel) { state.editTurbo = -1; renderTurboList(); return; }
  const teditSave = e.target.closest('[data-tedit-save]');
  if (teditSave) {
    const i = +teditSave.dataset.teditSave, row = teditSave.closest('.ef-record');
    DB.turbo[i].interval = +row.querySelector('[data-tedit="interval"]').value;
    DB.turbo[i].dur = +row.querySelector('[data-tedit="dur"]').value;
    const flags = [];
    row.querySelectorAll('[data-tflag-cb]').forEach(cb => { if (cb.checked) flags.push(cb.dataset.tflagCb); });
    if (row.querySelector('[data-trerun]') && row.querySelector('[data-trerun]').checked) flags.push('重推奔跑');
    DB.turbo[i].flags = flags;
    const noteEl = row.querySelector('[data-tnote]'); if (noteEl) DB.turbo[i].note = noteEl.value;
    const speedEl = row.querySelector('[data-tspeed]'); if (speedEl) DB.turbo[i].speed = +speedEl.value;
    const seqEl = row.querySelector('[data-tseq]'); if (seqEl) DB.turbo[i].seq = seqEl.value;
    state.editTurbo = -1; renderTurboList(); toast('映射 #' + (i + 1) + ' 已保存并热重载');
    return;
  }
  const tcapTrig = e.target.closest('[data-tcap-trig]');
  if (tcapTrig) { toast('捕获中：按下手柄或键盘上的触发键…（原型模拟，正式版实时捕获）'); return; }
  const taddTgt = e.target.closest('[data-tadd-tgt]');
  if (taddTgt) { toast('捕获下一个键盘键作为目标（原型模拟）；多个目标可叠加'); return; }
  const taddUp = e.target.closest('[data-tadd-up]');
  if (taddUp) {
    const i = +taddUp.dataset.taddUp;
    DB.turbo[i].up = DB.turbo[i].up || 'SPACE';
    renderTurboList(); toast('已添加抬起键 SPACE：松开 ' + DB.turbo[i].trig + ' 时自动按一下');
    return;
  }
  const seqAddDown = e.target.closest('[data-seq-add-down]');
  if (seqAddDown) { state.seqSteps.push({ k: '?', d: 20, down: true }); renderSeq(); toast('已加按下步（↓）'); return; }
  const seqAddUpstep = e.target.closest('[data-seq-add-upstep]');
  if (seqAddUpstep) { state.seqSteps.push({ k: '?', d: 20, up: true }); renderSeq(); toast('已加抬起步（↑）'); return; }
  const tflag = e.target.closest('[data-tflag]');
  if (tflag) {
    const f = tflag.dataset.tflag;
    tflag.setAttribute('aria-pressed', tflag.getAttribute('aria-pressed') !== 'true');
    tflag.classList.toggle('is-active');
    return;
  }
  const tdel = e.target.closest('[data-tdelete]');
  if (tdel) {
    const i = +tdel.dataset.tdelete, m = DB.turbo[i];
    confirmDialog('删除这条连发', `触发键 ${m.trig} → ${m.tgt} 将被移除，该操作会立即热重载。`, { danger: true, okText: '删除', onOk() {
      state.kbLit.delete(m.trig); DB.turbo.splice(i, 1);
      refreshKb(); renderNavCounts(); drawDial(); toast('连发已删除');
    } });
    return;
  }
  const chordDel = e.target.closest('[data-chord-del]');
  if (chordDel) {
    const i = +chordDel.dataset.chordDel;
    confirmDialog('删除组合键', `${DB.chords[i].combo} → ${DB.chords[i].tgt} 将被移除。`, { danger: true, okText: '删除', onOk() {
      DB.chords.splice(i, 1); renderChords(); renderNavCounts(); drawDial(); toast('组合键已删除');
    } });
    return;
  }
  const chordRetarget = e.target.closest('[data-chord-retarget]');
  if (chordRetarget) {
    startCapture('chord-retarget');
    toast('按新的目标键盘键…');
    return;
  }
  const macroDel = e.target.closest('[data-macro-del]');
  if (macroDel) {
    DB.macros.splice(+macroDel.dataset.macroDel, 1);
    renderMacros(); renderNavCounts(); toast('通用宏已删除');
    return;
  }
  const seqDel = e.target.closest('[data-seq-del]');
  if (seqDel) { state.seqSteps.splice(+seqDel.dataset.seqDel, 1); renderSeq(); return; }
  const wlDel = e.target.closest('[data-wl-del]');
  if (wlDel) {
    DB.whitelist.splice(+wlDel.dataset.wlDel, 1);
    renderWl(); toast('已移除，该进程立即恢复响应连发');
    return;
  }
  const setDel = e.target.closest('[data-set-del]');
  if (setDel) {
    const i = +setDel.dataset.setDel;
    confirmDialog('删除这条映射', `${DB.kbMappings[i].trig} → ${DB.kbMappings[i].tgt} 将被移除。`, { danger: true, okText: '删除', onOk() {
      DB.kbMappings.splice(i, 1); renderSetMappings(); renderOvMappings(); toast('映射已删除');
    } });
    return;
  }
  const audioApply = e.target.closest('[data-audio-apply]');
  if (audioApply) { toast('已应用「' + DB.audioMine[+audioApply.dataset.audioApply] + '」'); return; }
  const audioDel = e.target.closest('[data-audio-del]');
  if (audioDel) {
    DB.audioMine.splice(+audioDel.dataset.audioDel, 1);
    renderAudioMine(); toast('预设已删除');
    return;
  }
});
function handleAct(act, el, e) {
  const A = {
    'toggle-pause'() { state.paused = !state.paused; setPauseUI(); toast(state.paused ? '映射已暂停' : '映射已恢复'); },
    'toggle-vib'() { const s = $('#tb-vib-state'); const on = s.textContent.includes('开'); s.textContent = on ? '震动 关' : '震动 开'; toast(on ? '震动引擎已关闭' : '震动引擎已开启'); },
    'toggle-theme'() {
      // 顶栏按钮两态循环（v8 行为）：终末地·深色 ⇄ 浅色
      const next = SKIN_STATES[(skinStateIndex() + 1) % SKIN_STATES.length];
      applyLook(next.skin, next.theme);
      toast('界面已切换：' + skinStateLabel(next) + '（终末地外观 MOD 生效中）');
    },
    /* 双 UI 架构：停用 MOD = 跳回原版经典宿主（index.html）；本页即终末地包，无需再「应用」 */
    'mod-disable'() { toast('正在停用终末地 MOD · 返回原版…'); setTimeout(function () { location.href = 'index.html'; }, 300); },
    'mod-apply-self'() { toast('终末地风格 MOD 已经是使用中'); },
    'save'() { doSave(); },
    'show-guide'() { state.guideNo = 1; renderGuide(); openDlg('dlg-guide'); },
    'test-vibration'() { pulseTest(); toast('测试震动 1 秒（L 70 / R 55）'); },
    'test-rank'() { pulseTest(); toast('测试评分震动：SS 档 480ms'); },
    'check-update'() {
      const f = $('#update-float');
      if (f) { f.hidden = false; clearTimeout(window._updT); window._updT = setTimeout(() => { f.hidden = true; }, 8000); }
      const err = $('#upd-err'); if (err) err.hidden = true;
      toast('检查完成：发现新版本 R2.15（右下角可查看）');
    },
    'refresh-devices'() { toast('设备已刷新：XInput ×1 · RawInput HID ×1'); },
    'reset-devices'() { toast('手柄连接已重置（XInput 蓝牙握手）'); },
    'device-dialog'() { openDlg('dlg-device'); },
    'mouse-dialog'() { openDlg('dlg-mouse'); },
    'mouse-confirm'() {
      if (!state.mousePick) { toast('请先在九宫格或键位中选择一个鼠标事件'); return; }
      DB.kbMappings.push({ trig: state.mousePick, tgt: 'Q', flags: [], note: '鼠标映射' });
      renderSetMappings(); renderOvMappings();
      closeDlg(); toast('已添加映射：' + state.mousePick + ' → Q');
    },
    'dlg-cancel'() { closeDlg(); },
    'kb-apply'() { kbApply(); },
    'kb-cancel'() { kbCancel(); },
    'kb-conflict-overwrite'() {
      [...state.kbPick].forEach(k => { state.kbLit.add(k); if (!DB.turbo.find(m => m.trig === k)) DB.turbo.push({ trig: k, tgt: k, interval: 45, dur: 30, flags: ['连发'], note: '覆盖添加' }); });
      state.kbPick.clear(); state.kbDrop.clear(); refreshKb(); renderNavCounts(); drawDial();
      closeDlg(); toast('冲突已覆盖，连发已更新');
    },
    'kb-conflict-skip'() {
      OCC_KEYS.forEach(k => state.kbPick.delete(k));
      A['kb-apply']();
      closeDlg();
    },
    'gp-tab-combo'() { $('[data-gptab="combo"]').click(); },
    'gp-tab-wizard'() { $('[data-gptab="wizard"]').click(); },
    'gp-await-kb'() { setFlowStep(2); startCapture('gp'); },
    'gp-clear-kb'() { if (state.gpSlot) { $('#gp-cap-val').innerHTML = ''; toast('已清空键盘按键'); } },
    'gp-done'() {
      const cur = state.gpSlot;
      toast(cur + ' 键映射已保存');
      const next = SLOTS.find(s => !DB.padSlots[s[0]]);
      if (next) { selectSlot(next[0]); toast('继续：' + next[5]); }
      else { selectSlot(null); toast('全部按键都设好了！'); }
    },
    'gp-apply-recommended'() {
      const rec = { RS_Down: 'NUMPAD2', RS_Left: 'NUMPAD3', RS_Right: 'NUMPAD4' };
      Object.entries(rec).forEach(([k, v]) => { DB.padSlots[k] = { tgt: v }; });
      mountPadRefresh(); renderUnsetChips(); renderNavCounts(); drawDial();
      const next = SLOTS.find(s => !DB.padSlots[s[0]]);
      if (next) selectSlot(next[0]);
      toast('已套用推荐键位，只差最后几个了');
    },
    'onb-calibrate'() { go('gamepad'); $('[data-gptab="wizard"]').click(); },
    'onb-finish'() { const onb = $('#ov-onb'); if (onb) onb.hidden = true; toast('引导完成！随时点「新手引导」重看'); },
    'onb-collapse'() { const onb = $('#ov-onb'); if (onb) onb.hidden = true; },
    'vib-weaker'() { stepVibStrength(-6); },
    'vib-stronger'() { stepVibStrength(6); },
    'log-copy'() { toast('已复制 14 条日志 · R2.14 · Xbox 360 已连接'); },
    'log-zip'() { toast('已导出诊断包：日志 + 配置 + 设备信息（原型模拟）'); },
    'guide-skip'() { closeDlg(); try { localStorage.setItem('proto-guide', '1'); } catch (err) {} toast('已跳过引导，随时点「新手引导」重看'); },
    'vib-test-l'() { pulseTest(.6); toast('只测左马达 0.6 秒'); },
    'vib-test-r'() { pulseTest(.6); toast('只测右马达 0.6 秒'); },
    'log-folder'() { toast('已打开日志文件夹（原型模拟）'); },
    'gp-continue'() { $('#gp-flow-done').hidden = true; $('#gp-flow-idle').hidden = false; state.gpSlot = null; $$('#pad-hot .hot').forEach(h => h.classList.remove('is-here')); },
    'gp-delete'() {
      const slot = state.gpSlot;
      confirmDialog('删除该映射', `槽位 ${slot} 的映射将被移除（二次确认）。`, { danger: true, okText: '删除', onOk() {
        delete DB.padSlots[slot]; mountPadRefresh(); renderUnsetChips(); renderNavCounts(); drawDial(); selectSlot(null); toast('槽位映射已删除');
      } });
    },
    'gp-attach-seq'() { toast('已挂载序列宏：' + ($('#gp-seq').value || '（无）')); },
    'gp-recal'() { toast('校对手柄键位：请把「' + (state.gpSlot || '该键') + '」推到底再松开（原型模拟）'); },
    'chord-rerecord'() { toast('重新按住手柄键（原型演示）'); },
    'chord-save'() {
      const tgt = (state.captures.chord || []).join('+');
      if (!tgt) { toast('请先捕获目标键盘键'); return; }
      const combo = 'LT+LB';
      if (DB.chords.find(c => c.combo === combo)) { toast('这个组合已经存在：' + combo + ' → ' + DB.chords.find(c => c.combo === combo).tgt + '，先删掉或改成员键'); return; }
      const swKey = ($('#set-switch-val') || {}).textContent || '';
      if (swKey && tgt === swKey.trim()) { toast('目标键和「连发切换键」撞了：' + tgt + '，换一个'); return; }
      DB.chords.push({ combo, tgt, members: ['LT', 'LB'] });
      state.captures.chord = []; $('#chord-cap-val').innerHTML = '';
      renderChords(); renderNavCounts(); toast('组合键已保存：' + combo + ' → ' + tgt + '；成员键 LT / LB 单独按时让位给组合键');
    },
    'chord-cancel'() { state.captures.chord = []; $('#chord-cap-val').innerHTML = ''; },
    'chord-batch-delete'() {
      confirmDialog('删除筛选结果', '将删除当前筛选出的全部组合键（二次确认）。', { danger: true, okText: '全部删除', onOk() {
        const q = ($('#chord-search').value || '').toLowerCase();
        DB.chords = DB.chords.filter(c => !(c.combo.toLowerCase().includes(q) || c.tgt.toLowerCase().includes(q)));
        renderChords(); renderNavCounts(); drawDial(); toast('筛选结果已删除');
      } });
    },
    'wiz-prev'() { if (state.wizNo > 1) { state.wizNo--; renderWiz(); } },
    'wiz-next'() {
      const tok = WIZ_ORDER[(state.wizNo - 1) % WIZ_ORDER.length];
      if (!state.wizDone.includes(tok)) state.wizDone.push(tok);
      if (state.wizNo < 24) { state.wizNo++; renderWiz(); }
      else { renderWiz(); toast('校对完成：24 / 24，全部按键都记下了'); }
    },
    'wiz-skip'() { A['wiz-next'](); },
    'wiz-end'() { toast(`校对已保存到第 ${state.wizNo} 步，下次可继续`); },
    'turbo-add'() {
      DB.turbo.push({ trig: '（捕获）', tgt: '（捕获）', interval: 45, dur: 30, flags: ['连发'], note: '' });
      state.editTurbo = DB.turbo.length - 1;
      renderTurboList();
    },
    'seq-record'() { state.seqRecording = true; toast('录制中…按 Esc 停止并填入（原型模拟）'); },
    'seq-stop'() { state.seqSteps.push({ k: 'A', d: 20 }, { k: 'B', d: 20 }); renderSeq(); toast('录制停止，2 步已填入'); },
    'seq-add-key'() { state.seqSteps.push({ k: '?', d: 20 }); renderSeq(); },
    'seq-add-wait'() { state.seqSteps.push({ k: '200', d: 200, wait: true }); renderSeq(); },
    'seq-20ms'() { state.seqSteps.forEach(s => { s.d = 20; }); renderSeq(); toast('全部步骤已设为 20ms'); },
    'seq-clear'() { state.seqSteps = []; renderSeq(); toast('已清空，退回普通目标键模式'); },
    'macro-add'() {
      const n = $('#macro-name').value.trim(), t = $('#macro-text').value.trim();
      if (!n || !t) { toast('需要宏名字和序列文本'); return; }
      const same = DB.macros.find(m => m.name.toLowerCase() === n.toLowerCase());
      if (same) {
        confirmDialog('同名宏已存在', '「' + same.name + '」已有脚本：' + same.text + '。要用新内容覆盖它吗？', { okText: '覆盖', onOk() {
          same.name = n; same.text = t;
          $('#macro-name').value = ''; $('#macro-text').value = '';
          renderMacros(); toast('已覆盖宏「' + n + '」（大小写不敏感）');
        } });
        return;
      }
      DB.macros.push({ name: n, text: t });
      $('#macro-name').value = ''; $('#macro-text').value = '';
      renderMacros(); renderNavCounts(); toast('通用宏已添加：宏(' + n + ')');
    },
    'tb-switch-clear'() { state.switchKeys = []; state.captures['tb-switch'] = []; renderCapture('tb-switch'); renderSwitchChips(); toast('切换键已清空（默认 DELETE 仍可用）'); },
    'tb-switch-save'() {
      const keys = state.switchKeys || [];
      if (!keys.length) { toast('没有可保存的键：先捕获一个，或保留默认 DELETE'); return; }
      const combo = keys.join('+');
      if (combo === 'ESC' || combo === 'LALT+F4') { toast('这个组合会被系统/窗口吃掉，换个键'); return; }
      if (DB.kbMappings.some(m => m.trig === combo)) { toast('和现有映射触发键撞了：' + combo + '，请换一个'); return; }
      if (DB.turbo.some(m => m.trig === combo)) { toast('和连发条目的触发键撞了：' + combo + '，请换一个'); return; }
      toast('切换键已保存到当前预设：' + combo + '（游戏内按住即暂停 / 恢复）');
    },
    'tb-switch-back'() {
      state.switchKeys = state.switchKeys || [];
      if (!state.switchKeys.length) { toast('已经是空的了'); return; }
      const last = state.switchKeys.pop();
      renderSwitchChips(); toast('已删掉末键：' + last);
    },
    'vib-apply'() { toast('已应用震动预设「' + $('#vib-preset').value + '」'); },
    'vib-save'() { toast('当前引擎镜像已写回预设并落盘'); },
    'vib-delete'() { confirmDialog('删除震动预设', '「' + $('#vib-preset').value + '」将被删除（默认与全零预设受保护）。', { danger: true, okText: '删除', onOk() { toast('预设已删除'); } }); },
    'vib-reset'() { toast('已还原默认预设（内置第一档）'); },
    'vib-export'() { toast('已导出 vibration_export_ACT.toml 到程序目录'); },
    'vib-import'() {
      const sel = $('#vib-import-file');
      const f = sel ? sel.value : '';
      if (!f) { toast('先选一个导出文件（下拉里挑一个）'); return; }
      if (f.indexOf('broken') >= 0) {
        toast('解析失败：' + f + ' 不是有效的震动配置（缺少 [vibration] 段），未做任何修改');
        return;
      }
      toast('已从 ' + f + ' 导入，并往返校验一致');
    },
    'job-apply'() { toast('职业预设已叠加：' + $('#job-base').value + ' · ' + $('#job-class').value); },
    'job-disable'() { toast('职业预设已关闭，回到通用震动'); },
    'job-export'() { toast('已导出 job_export_鬼剑士_狂战士.toml'); },
    'job-import'() { toast('已导入职业设置并叠加'); },
    'audio-self-check'() { pulseTest(.5); toast('马达自检 0.5 秒完成'); },
    'audio-calibrate'() { toast('屏蔽校准 3 秒：请保持环境安静'); setTimeout(() => { $('#audio-calib').hidden = false; toast('校准完成：建议门槛 46'); }, 1500); },
    'audio-calib-apply'() { $('#a-thresh').textContent = '46'; $('#audio-calib').hidden = true; toast('建议已应用并落盘'); },
    'audio-calib-dismiss'() { $('#audio-calib').hidden = true; toast('已忽略本次建议'); },
    'audio-save'() {
      const n = $('#audio-save-name').value.trim();
      if (!n) { toast('请填写预设名'); return; }
      if (DB.audioMine.some(x => x.toLowerCase() === n.toLowerCase())) { toast('已有同名预设「' + n + '」，换个名字（重名不覆盖）'); return; }
      DB.audioMine.push(n); DB.audioUsing = n; $('#audio-save-name').value = '';
      renderAudioMine(); toast('音频预设已保存并启用：' + n);
    },
    'audio-import'() { toast('已导入音频预设（内置重名拒绝）'); },
    'audio-export'() { toast('已导出 audio_export.toml'); },
    'audio-reset'() { confirmDialog('还原默认', '将清空自定义预设并回到「人声清晰」。', { danger: true, okText: '还原', onOk() { toast('已还原默认'); } }); },
    'wl-add'() {
      const v = $('#wl-input').value.trim();
      if (!v) { toast('请输入进程名或完整路径'); return; }
      if (DB.whitelist.find(w => w.name.toLowerCase() === v.toLowerCase())) { toast('该进程已在名单中'); return; }
      DB.whitelist.push({ name: v, path: '' });
      $('#wl-input').value = '';
      renderWl(); drawDial(); toast('已添加：' + v);
    },
    'wl-browse'() { toast('浏览器端只登记文件名；完整路径请在正式版用文件选择器'); },
    'wl-quick'(el) {
      const n = el.dataset.name;
      if (DB.whitelist.find(w => w.name.toLowerCase() === n.toLowerCase())) { toast(n + ' 已在名单中'); return; }
      DB.whitelist.push({ name: n, path: '' });
      renderWl(); drawDial(); toast('已添加常用进程：' + n);
    },
    'wl-clear'() {
      confirmDialog('清空名单', '清空后所有进程都恢复响应连发（二次确认）。', { danger: true, okText: '清空', onOk() {
        DB.whitelist = []; renderWl(); drawDial(); toast('名单已清空');
      } });
    },
    'goto-whitelist'() { go('whitelist'); },
    'goto-combo'() { go('gamepad'); const b = $('[data-gptab="combo"]'); if (b) b.click(); },
    'ph-close'() { const h = $('#preset-health'); if (h) h.hidden = true; },
    'preset-check'() {
      const host = $('#preset-health');
      if (!host) return;
      host.hidden = false;
      clearTimeout(window._phT);
      window._phT = setTimeout(() => { host.hidden = true; }, 8000);
    },
    'apply-preset'() {
      const sel = $('#top-preset');
      DB.currentPreset = sel && sel.value ? sel.value : '';
      renderSelects(); renderNavCounts();
      A['preset-check']();
      toast((DB.currentPreset || '已清空预设名（映射保留）') + ' · 已热重载');
    },
    'set-add-mapping'() {
      const trig = (state.captures['new-trig'] || []).join('+') || '（触发键）';
      const tgt = (state.captures['new-tgt'] || []).join('+') || '（目标键）';
      DB.kbMappings.push({ trig, tgt, flags: [], note: $('#set-new-note').value });
      renderSetMappings(); renderOvMappings(); toast('映射已添加：' + trig + ' → ' + tgt);
    },
    'set-save-preset'() { toast('预设已保存'); },
    'set-delete-preset'() { confirmDialog('删除预设', '该预设将被删除（两次点击武装确认的正式版行为）。', { danger: true, okText: '删除', onOk() { toast('预设已删除'); } }); },
    'settings-save'() { doSave(); $('#set-dirty').textContent = '已保存'; $('#set-dirty').className = 'ef-state'; },
    'settings-cancel'() { $('#set-dirty').textContent = '未修改'; toast('草稿已丢弃'); },
    'log-refresh'() { renderLogs(); toast('读到 ' + DB.logs.length + ' 行'); },
    'dev-test'() { pulseTest(1); toast('按住测试震动中…'); },
    'dev-stop'() { toast('输出已停止'); },
    'dev-stop-all'() { toast('全部输出已停止（Drop 兜底）'); },
    'dev-activate'() { toast('基线抓取中：按任意键后松开即可完成'); },
    'guide-prev'() { if (state.guideNo > 1) { state.guideNo--; renderGuide(); } },
    'guide-next'() {
      if (state.guideNo < GUIDE.length) { state.guideNo++; renderGuide(); }
      else { closeDlg(); try { localStorage.setItem('proto-guide', '1'); } catch (err) {} toast('引导完成，已记住（guide_seen）'); }
    },
    /* ---- v8 新增 ---- */
    'guide-dfo-yes'() { const p = $('#guide-pick1'); if (p) p.textContent = '当前选择：震动 + 连发（已写入配置 dfo_player = true）'; const sw = $('#sw-dfo'); if (sw) sw.checked = true; state.dfo = true; toast('已选：我在玩 DNF —— 震动引擎与 DLL 通道联动开启'); },
    'guide-dfo-no'() { const p = $('#guide-pick1'); if (p) p.textContent = '当前选择：只用连发（已写入配置 dfo_player = false）'; const sw = $('#sw-dfo'); if (sw) sw.checked = false; state.dfo = false; toast('已选：只用连发 —— 震动相关分区将隐藏'); },
    'guide-route-s1'() { const p = $('#guide-pick2'); if (p) p.textContent = '当前选择：S1 ACT 注入版（需 DfoVibration.dll）'; toast('路线：S1 ACT 注入版 —— 已自动套用「ACT1 特供」震动预设'); },
    'guide-route-s4'() { const p = $('#guide-pick2'); if (p) p.textContent = '当前选择：S4+ 通用版（正式服）'; toast('路线：S4+ 通用版 —— 已自动套用「默认」震动预设'); },
    'show-about'() { openDlg('dlg-about'); },
    'open-update-panel'() { const f = $('#update-float'); if (f) f.hidden = true; openDlg('dlg-update'); },
    'float-later'() { const f = $('#update-float'); if (f) f.hidden = true; toast('已稍后提醒（下次启动再说）'); },
    'float-close'() { const f = $('#update-float'); if (f) f.hidden = true; },
    'upd-ignore'() { closeDlg(); toast('已忽略 R2.15：不再提示这个版本'); },
    'restart-bg'() { toast('后台功能已重启：前台探测线程已恢复（4/4）'); },
    'gp-stop-ident'() { const l = $('#gp-ident-line'); if (l) l.innerHTML = '<span class="ef-state is-draft">已停止识别</span> <button class="ef-btn small ghost" data-act="gp-start-ident">开始识别</button> <span>随时可以再开</span>'; toast('已停止识别手柄（映射照旧生效）'); },
    'gp-start-ident'() { const l = $('#gp-ident-line'); if (l) l.innerHTML = '<span class="ef-state">已校对 · 识别中：Xbox 360 Controller</span> <button class="ef-btn small ghost" data-act="gp-stop-ident">停止识别</button> <span>直接按手柄上已校对的键即可选中该槽位</span>'; toast('开始识别：按手柄键即可选中槽位'); },
    'gp-mouse-map'() { openDlg('dlg-mouse'); toast('选一个鼠标动作作为这个手柄键的目标'); },
    'gp-demo-press'() {
      const hots = $$('#pad-hot .hot');
      if (!hots.length) return;
      const h = hots[Math.floor(Math.random() * hots.length)];
      const s2 = SLOTS.find(x => x[0] === h.dataset.slot);
      const { cx, cy, r } = slotXY(s2);
      const ring = document.createElementNS(SVGNS, 'circle');
      ring.setAttribute('class', 'radar');
      ring.setAttribute('cx', cx.toFixed(1)); ring.setAttribute('cy', cy.toFixed(1));
      h.appendChild(ring); h.classList.add('is-live');
      toast('实体按下演示：' + (s2 ? s2[5] : h.dataset.slot));
      setTimeout(() => { h.classList.remove('is-live'); ring.remove(); }, 1400);
    },
    'gp-edit-cur'() {
      const t = state.gpSlot;
      if (!t) { toast('先在手柄图上选一个键'); return; }
      const done = $('#gp-flow-done'); if (done) done.hidden = true;
      const edit = $('#gp-flow-edit'); if (edit) edit.hidden = false;
      selectSlot(t);
      setFlowStep(1);
      toast('回到编辑：' + (SLOTS.find(x => x[0] === t) || ['', 0, 0, 0, '', t])[5]);
    },
    'gp-done-confirm'() { A['gp-done'](); },
    'gp-add-another'() { const d = $('#gp-flow-done'); if (d) d.hidden = true; $('#gp-flow-idle').hidden = false; state.gpSlot = null; $$('#pad-hot .hot').forEach(h => h.classList.remove('is-here')); toast('再选手柄图上的一个键继续加'); },
    'gp-edit-seq'() { $('[data-gptab="slot"]').click(); toast('序列步骤在「连发映射 → 宏（高级）」里编辑（两处共用同一套）'); },
    'gp-attach-seq'() {
      const v = $('#gp-seq').value;
      const tk = $('#gp-seq-takeover');
      if (tk) tk.hidden = !v;
      toast(v ? '已挂载序列宏「' + v + '」：按下该键执行脚本，连发/奔跑停用' : '已卸下序列宏，恢复普通按键行为');
    },
    'wiz-fail'() { const e2 = $('#wiz-err'); if (e2) { e2.hidden = false; setTimeout(() => { e2.hidden = true; }, 3200); } toast('（演示）没认出来：摇杆推直、别同时按多个键'); },
    'set-wl-add'() {
      const el = $('#set-wl-input'); const v = el.value.trim();
      if (!v) { toast('请输入进程名或完整路径'); return; }
      if (DB.whitelist.find(w => w.name.toLowerCase() === v.toLowerCase())) { toast('该进程已在名单中'); return; }
      DB.whitelist.push({ name: v, path: '' }); el.value = '';
      renderWl(); toast('已添加：' + v);
    },
    'set-wl-clear'() {
      confirmDialog('清空名单', '清空后所有程序都会响应连发（会二次确认）。', { danger: true, okText: '清空', onOk() { DB.whitelist = []; renderWl(); toast('名单已清空'); } });
    },
    'set-manifest-save'() {
      const v = $('#set-manifest').value.trim();
      const err = $('#set-manifest-err');
      if (v && !/^https?:\/\//.test(v)) { if (err) { err.hidden = false; err.textContent = '地址格式不对：要以 http:// 或 https:// 开头。'; } toast('清单地址格式不对'); return; }
      if (err) err.hidden = true;
      toast(v ? '已保存自定义清单地址：' + v : '已清空自定义地址，回到内置 GitHub / jsDelivr');
    },
    'awl-custom-save'() { toast('已保存这份单独的音频名单（不覆盖连发白名单）'); },
    'cls-vib-apply'() { toast('经典模式：震动预设「' + $('#cls-vib-preset').value + '」已应用'); },
    'dev-reactivate'() { openDlg('dlg-hid'); },
    'hid-next'() {
      const a1 = $('#hid-step'), a2 = $('#hid-idle'), a3 = $('#hid-write'), a4 = $('#hid-done');
      if (a1) a1.textContent = '✓ 已检测到按下（0x0B / A 键）';
      if (a2) a2.textContent = '✓ 空闲帧已记录（3 帧）';
      if (a3) a3.textContent = '✓ 已写入内存（不落盘）';
      if (a4) a4.textContent = '✓ 完成，可以识别了';
      setTimeout(() => { closeDlg(); toast('HID 手柄基线已就绪：现在按键能识别了'); }, 900);
    },
    'quit-do'() {
      const way = (document.querySelector('input[name="quitway"]:checked') || {}).checked ? 0 : 1;
      const radios = $$('input[name="quitway"]');
      const tray = radios[0] && radios[0].checked;
      closeDlg();
      setTimeout(() => toast(tray ? '已最小化到托盘，连发与震动继续工作' : '已退出程序（原型不真的退出）'), 60);
    },
    'exit-minimal'() { const r = $('input[name="mode"][value="full"]'); if (r) { r.checked = true; } applyMode('full'); },
    'quit'() { openDlg('dlg-quit'); },
  };
  if (A[act]) { A[act](el, e); }
  else toast('（原型）动作：' + act);
}
const WIZ_HINT = {
  trigger: '按到底再松开，别只按一半',
  shoulder: '按一下再松开即可',
  button: '按一下再松开即可',
  system: '按一下再松开即可',
  dpad: '推到底保持 0.3 秒再松开，别斜着推',
  stick: '推到最边再松开；如果是摇杆按下，请直接往里按到底',
};
function renderWiz() {
  const tok = WIZ_ORDER[(state.wizNo - 1) % WIZ_ORDER.length];
  const slot = SLOTS.find(x => x[0] === tok);
  $('#wiz-no').textContent = state.wizNo;
  $('#wiz-num').textContent = state.wizNo + '/24';
  $('#wiz-prog').style.setProperty('--p', (state.wizNo / 24).toFixed(3));
  $('#wiz-slot').textContent = slot ? slot[5] + '（' + tok + '）' : tok;
  const hint = $('#wiz-hint');
  if (hint) hint.textContent = '← ' + (WIZ_HINT[slot ? slot[4] : 'button'] || '按一下再松开');
  const dl = $('#wiz-done-list');
  if (dl) {
    dl.innerHTML = state.wizDone.length
      ? '已完成：' + state.wizDone.map(k => '<span class="kbd">✓ ' + k + ' = ' + (SLOT_HID[k] || '—') + '</span>').join(' ')
      : '已完成：暂无（按顺序从第一项开始）';
  }
  const err = $('#wiz-err'); if (err) err.hidden = true;
}
function stepVibStrength(delta) {
  const sl = document.querySelector('[data-vib="master"]');
  if (!sl) return;
  sl.value = Math.max(0, Math.min(100, +sl.value + delta));
  updateSlider(sl);
  const mirror = document.getElementById('vib-master-mirror');
  if (mirror) mirror.textContent = sl.value + '%';
  toast('总强度 → ' + sl.value + '%（已生效）');
  scheduleSave();
}
function pulseTest(sec) {
  const bars = [['vbar-l', 'vnum-l', .8], ['vbar-r', 'vnum-r', .65], ['abar-l', 'anum-l', .7], ['abar-r', 'anum-r', .6]];
  bars.forEach(([b, n, v]) => {
    const el = $('#' + b); if (!el) return;
    el.style.setProperty('--p', v);
    const num = $('#' + n); if (num) num.textContent = Math.round(v * 100) + '%';
  });
  setTimeout(() => {
    bars.forEach(([b, n, base]) => {
      const el = $('#' + b); if (!el) return;
      el.style.setProperty('--p', base * .5);
      const num = $('#' + n); if (num) num.textContent = Math.round(base * 50) + '%';
    });
  }, (sec || 1) * 1000);
}

/* ---------------- 其它输入事件 ---------------- */
document.addEventListener('change', e => {
  const t = e.target;
  if (t.name === 'mode') applyMode(t.value);
  if (t.name === 'mpv') {
    const job = t.value === 'job';
    ['#mv-vib-preset', '#mv-job-base', '#mv-job-class'].forEach(function (sel) {
      const el = $(sel); if (el) el.hidden = job ? sel === '#mv-vib-preset' : sel !== '#mv-vib-preset';
    });
    toast(job ? '全职业预设：先选基础职业与转职' : '通用预设：直接用震动预设');
  }
  if (t.name === 'engmode') toast('S1 输出引擎：' + ({ classic: '经典', s4: 'S4 纯', s4plus: 'S4+ 不丢震动' })[t.value] + '（已落盘）');
  if (t.name === 'devapi1' || t.name === 'updch') toast('已选择：' + t.parentElement.textContent.trim());
  if (t.name === 'lang') applyLang(t.value);
  if (t.name === 'quitway') { /* 退出方式由 quit-do 读取 */ }
  if (t.id === 'sw-dfo') {
    state.dfo = t.checked;
    ['vibration', 'jobs', 'audio'].forEach(function (r) {
      const a = document.querySelector('.side nav a[data-route="' + r + '"]');
      if (a) a.style.display = t.checked ? '' : 'none';
    });
    renderMinimalState();
    toast(t.checked ? 'DFO 震动已启用：震动 / 全职业 / 音频分区已恢复' : '已切换为「只用连发」：震动相关分区隐藏，配置保留');
  }
  if (t.id === 'set-route') toast('路线已切换：' + t.value + '（整套震动参数互换，映射与连发不受影响）');
  if (t.id === 'sw-adv-enable') {
    state.advEnabled = t.checked;
    const note = $('#sw-adv-note');
    if (note) note.textContent = t.checked ? '开着：62 条按当前值生效' : '已关闭：本区 62 条全部不生效，回到通用默认值';
    toast(t.checked ? '允许高级调校：开' : '允许高级调校：关（62 条全部不生效）');
  }
  if (t.id === 'vib-independent') {
    const st = $('#vib-debug-state');
    if (st) { st.textContent = t.checked ? '调试模式' : '正常模式'; st.className = 'ef-state' + (t.checked ? ' is-alert' : ''); }
    toast('评分 / 移动通道：' + (t.checked ? '独立（不受总强度影响）' : '跟随全局总强度'));
  }
  if (t.id === 'sw-move-indep') { state.moveIndep = t.checked; toast('移动持续震动：' + (t.checked ? '独立于全局强度' : '跟随全局强度')); }
  if (t.name === 'awlsrc') {
    const box = $('#awl-custom'); if (box) box.hidden = t.value !== 'custom';
    toast('音频白名单来源：' + (t.value === 'macro' ? '复用连发白名单那份' : '单独填一份'));
  }
  if (t.id === 'gp-behavior' || t.name === 'gpb') {
    /* gpb 是 radio name；下面用 closest 兜底 */
  }
  if (t.name === 'gpb') {
    const v = t.value;
    const f = $('#gp-rerun-field'); if (f) f.hidden = v !== 'rerun';
    const note = $('#gp-behavior-note');
    const map = { none: '无附加行为', turbo: '连发（按住 = 一直连按）', run: '简易奔跑（按住 = 持续前进）', rerun: '重推奔跑（连点两下 = 冲刺）' };
    if (note) note.textContent = '当前：' + (map[v] || v) + '。四个行为互斥，选中即时生效（无需保存）。';
    toast('行为已切换：' + (map[v] || v));
  }
  if (t.name === 'mode2') applyMode(t.value);
  if (t.id === 'sw-wl') {
    const on = t.checked;
    const n = $('#wl-state');
    if (on) { n.dataset.flag = '就绪'; n.textContent = '白名单已开启 —— ' + DB.whitelist.length + ' 个程序生效，无需修改。连发只在这些程序里生效；其他软件照常打字。'; }
    else { n.dataset.flag = '注意'; n.textContent = '已关闭：所有程序都会触发连发，可能误触其他软件。名单仍然保留在配置里 —— 重新打开开关就立刻恢复，不用重新加一遍。'; }
    toast(on ? '白名单已启用' : '白名单已关闭（名单保留，重开即恢复）');
  }
  if (t.name === 'wlmode') { toast('比对方式：' + (t.value === 'simple' ? '宽松（同名程序都放行）' : '严格（只认登记时的路径）') + '，已落盘'); }
  if (t.id === 'vib-independent') toast('独立测试：' + (t.checked ? '开启' : '关闭') + '（不受总开关限制）');
  if (t.id === 'sw-font') { toast('伤害数字飘字：' + (t.checked ? '开' : '关')); }
  if (t.id === 'sw-audio') toast('音频震动：' + (t.checked ? '启用' : '停用'));
  if (t.id === 'sw-vib-master') { const s2 = $('#tb-vib-state'); if (s2) s2.textContent = t.checked ? '震动 开' : '震动 关'; toast('震动引擎：' + (t.checked ? '开' : '关')); }
  if (t.id === 'sw-ai') {
    const b = $('#ai-mode-b'), a = $('#ai-mode-a'), st = $('#ai-state');
    if (b) { b.classList.toggle('is-set', t.checked); b.setAttribute('aria-pressed', t.checked); }
    if (a) { a.classList.toggle('is-set', !t.checked); a.setAttribute('aria-pressed', !t.checked); }
    if (st) st.textContent = t.checked ? '当前 B 智能模式 · 监听中——智能层参与输出' : '当前 A 经典模式 / 静音中——智能层未参与输出';
    toast('AI 智能增强：' + (t.checked ? '开（B 智能模式）' : '关（A 经典模式）'));
  }
  if (t.id === 'sw-job-apply') { const js = $('#job-apply-state'); if (js) { js.textContent = t.checked ? '已应用' : '未应用'; js.classList.toggle('is-draft', !t.checked); js.classList.toggle('is-alert', t.checked); } toast('全职业预设：' + (t.checked ? '已应用（职业参数覆盖通用震动）' : '未应用（恢复通用震动默认）')); }
  if (t.id === 'job-algo') {
    const rows = ALGO_LABELS[t.value] || [];
    const host = document.getElementById('job-algo-sliders');
    if (host && rows.length) {
      host.innerHTML = rows.map(function (r, i) {
        return '<div class="slider-row"><div class="lbl">' + r[0] + '<small>AP' + (i + 1) + '</small></div>' +
          '<input type="range" class="ef-range" data-vibj="ap' + (i + 1) + '" min="0" max="100" value="' + r[1] + '">' +
          '<span class="val">' + r[1] + '%</span></div>';
      }).join('');
      host.querySelectorAll('.ef-range').forEach(function (r) { updateSlider(r); });
      toast('已切换到「' + t.value + '」，滑杆含义与推荐值已联动');
    }
  }
  if (t.closest && t.closest('#page-settings')) {
    const sb = document.getElementById('set-save-btn');
    if (sb && sb.disabled) { sb.disabled = false; sb.textContent = '保存（Ctrl+S）· 有未保存改动'; }
    const sd = document.getElementById('set-dirty');
    if (sd) { sd.textContent = '有未保存改动'; sd.className = 'ef-state is-alert'; }
  }
  if (t.id === 'sw-job-apply') toast('全职业预设：' + (t.checked ? '开（职业参数覆盖通用震动）' : '关（恢复通用震动默认）'));
  if (t.name === 'awlsrc') toast('音频白名单来源：' + (t.value === 'macro' ? '连发白名单那份' : '单独填写一份'));
  if (t.name === 'vibroute') { toast('路线已切换，自动套用' + (t.value === 's1' ? '「ACT1 特供」' : '「默认」') + '预设'); }
  if (t.id === 'vib-preset') toast('选择预设后点「应用」生效（正式版即时应用）');
  if (t.id === 'quick-preset' || t.id === 'tb-preset') { DB.currentPreset = t.value; renderNavCounts(); toast('预设已切换：' + t.value + '（热重载完成）'); }
  if (t.id === 'seq-macro-pick' && t.value) { state.seqSteps.push({ k: '宏(' + t.value + ')', d: 0, wait: false }); renderSeq(); toast('已引用通用宏：' + t.value); t.selectedIndex = 0; }
  if (t.id === 'job-base' || t.id === 'job-class') { const cls = $('#job-class'); cls.innerHTML = '<option>' + $('#job-base').value + '-一转</option><option>' + $('#job-base').value + '-二转</option>'; }
});
document.addEventListener('input', e => {
  if (e.target.id === 'chord-search') renderChords();
  if (e.target.id === 'log-search') { state.logSearch = e.target.value; renderLogs(); }
  if (e.target.id === 'gp-search') toast('搜索：' + e.target.value + '（正式版过滤槽位）');
  if (e.target.dataset.seqd !== undefined) { state.seqSteps[+e.target.dataset.seqd].d = +e.target.value; }
});
document.addEventListener('mousedown', e => {
  if (!state.waitingCap) return;
  const MB = ['鼠标左键', '鼠标中键', '鼠标右键', '鼠标X1', '鼠标X2'];
  if (e.button < 0 || e.button > 4) return;
  e.preventDefault();
  const name = state.waitingCap;
  const arr = state.captures[name] || (state.captures[name] = []);
  arr.push(MB[e.button]);
  state.waitingCap = null;
  const el = $(`.capture[data-cap="${name}"]`);
  if (el) { el.classList.remove('is-waiting'); const h = el.querySelector('.cap-hint'); if (h) h.textContent = '已捕获'; }
  renderCapture(name);
  toast('已捕获鼠标键：' + MB[e.button]);
}, true);
window.addEventListener('beforeunload', () => { try { localStorage.setItem('proto-guide', '1'); } catch (e) {} });

/* ---------------- 多语言（演示：导航 + 顶栏 + 页标题） ---------------- */
const I18N = {
  zh: { nav: { overview: '概览', gamepad: '手柄映射', turbo: '连发映射', whitelist: '白名单', vibration: '通用震动', jobs: '全职业预设', audio: '音频震动', settings: '设置', logs: '日志' } },
  tw: { nav: { overview: '概覽', gamepad: '手把映射', turbo: '連發映射', whitelist: '白名單', vibration: '通用震動', jobs: '全職業預設', audio: '音訊震動', settings: '設定', logs: '日誌' } },
  en: { nav: { overview: 'Overview', gamepad: 'Gamepad', turbo: 'Turbo', whitelist: 'Whitelist', vibration: 'Vibration', jobs: 'Job Presets', audio: 'Audio', settings: 'Settings', logs: 'Logs' } },
  jp: { nav: { overview: '概要', gamepad: 'パッド割当', turbo: '連射', whitelist: 'ホワイトリスト', vibration: '振動', jobs: '全職プリセット', audio: 'オーディオ', settings: '設定', logs: 'ログ' } },
};
function applyLang(code) {
  const L = I18N[code] || I18N.zh;
  state.lang = code;
  $$('.side nav a[data-route]').forEach(function (a) {
    const label = L.nav[a.dataset.route];
    if (!label) return;
    const first = a.firstChild;
    if (first && first.nodeType === 3) first.nodeValue = label;
    else a.insertAdjacentText('afterbegin', label);
  });
  go(state.route);
  toast(code === 'zh' ? '语言：简体中文' : '语言已切换（演示：仅导航与页名，正文仍为中文）');
}

/* ---------------- 启动 ---------------- */
function init() {
  const qs = new URLSearchParams(location.search);
  // 主题优先级沿用 v8：URL ?theme= > 防闪烁恢复值 > dark（v9 的 ?skin= 参数已随换皮方案撤销）
  const qsTheme = qs.get('theme') || document.documentElement.dataset.theme || 'dark';
  applyLook('ef', qsTheme);
  fitStage();
  // v1.3 Saira 字体就绪重算（修「载入 mod 后字挤在一起」）：
  // ① 主动触发 Saira 可变字体加载（600 字重覆盖 UI 主用字形; 失败静默）;
  // ② fonts.ready 后重跑 fitStage —— 字体晚到/回退导致的舞台度量偏差归零。
  if (document.fonts && document.fonts.load) {
    try { document.fonts.load('600 12px Saira').catch(() => {}); } catch (e) {}
    if (document.fonts.ready && document.fonts.ready.then) {
      document.fonts.ready.then(() => { try { fitStage(); } catch (e) {} }).catch(() => {});
    }
  }
  mountPad();
  mountKeyboard();
  initSliders();
  renderOvMappings(); renderOvLogs(); renderTurboList(); renderChords(); renderMacros();
  renderSeq(); renderWl(); renderSetMappings(); renderAudioMine(); renderLogs();
  renderSelects(); renderSwitchChips(); renderUnsetChips(); renderNavCounts(); renderGuide();
  drawDial();
  renderMinimalState();
  renderChords();
  const wz = $('#gpt-wizard .ef-actions');
  if (wz && !$('#gpt-wizard [data-act="wiz-fail"]')) {
    wz.insertAdjacentHTML('beforeend', '<button class="ef-btn small ghost" data-act="wiz-fail">演示：没认出来</button>');
  }
  if (qs.get('mode')) applyMode(qs.get('mode'));
  go(qs.get('p') || window.location.hash.slice(1) || 'overview');
  if (qs.get('dlg')) openDlg(qs.get('dlg'));
  if (qs.get('gptab')) { const gt = document.querySelector('[data-gptab="' + qs.get('gptab') + '"]'); if (gt) gt.click(); }
  if (qs.get('tedit') !== null && qs.get('tedit') !== undefined && qs.get('tedit') !== '') {
    state.editTurbo = parseInt(qs.get('tedit'), 10) || 0; renderTurboList();
  }
  const to = qs.get('to');
  if (to) {
    const tel = document.querySelector(to);
    if (tel) {
      let anc = tel.closest('details');
      while (anc) { anc.open = true; anc = anc.parentElement && anc.parentElement.closest('details'); }
      setTimeout(() => tel.scrollIntoView({ block: 'start' }), 60);
    }
  }
  setInterval(tick, 1000);
  // 首次访问自动弹引导
  let seen = null;
  try { seen = localStorage.getItem('proto-guide'); } catch (e) {}
  if (seen === null && setSlotCount() === 0 && DB.turbo.length === 0) { openDlg('dlg-guide'); try { localStorage.setItem('proto-guide', '1'); } catch (e2) {} }
}
try { init(); } catch (err) {
  var t = document.getElementById('toast-root');
  if (t) { var d = document.createElement('div'); d.className = 'ef-toast';
    d.style.maxWidth = '60rem';
    var nl = String.fromCharCode(10);
    d.textContent = 'INIT ERR: ' + err.message + ' || ' + String(err.stack || '').split(nl).slice(0, 3).join(' <- '); t.appendChild(d); }
}

/* ============================================================
   v12 桥绑定层（双UI桥契约 v1 §2-§4 / §7）—— 与 replica.js 同一套路径
   快照 → DOM 按域绑定；data-act → SorahkHost.call 命令出口；
   cfg.paths 动态发现白名单表 + 滑杆 300ms 去抖（memory）+ 松手按族档位补正式 write。
   纯 mock（file:// 直开）行为不变；BIND（真桥 / ?bridge=mock）才启用新交互与动态锚点。
============================================================ */
(function () {
  var H = window.SorahkHost;
  if (!H) return;
  var BIND = !!H.binding;
  var REAL = !!H.isReal;
  var lastSnap = null;
  var rawCapActive = false;
  var draggingSlider = null;
  state.vib = true; /* 震动总开关镜像（快照/命令共用） */

  const LANG_VARIANTS = { SimplifiedChinese: 'zh', TraditionalChinese: 'tw', English: 'en', Japanese: 'jp' };
  const XI_TOKENS = { 1: 'DPad_Up', 2: 'DPad_Down', 4: 'DPad_Left', 8: 'DPad_Right', 16: 'Start', 32: 'Back', 64: 'LS_Click', 128: 'RS_Click', 256: 'LB', 512: 'RB', 4096: 'A', 8192: 'B', 16384: 'X', 32768: 'Y' };
  const HID_DPAD = { 1: 'DPad_Up', 3: 'DPad_Right', 5: 'DPad_Down', 7: 'DPad_Left' };
  const HID_AXIS = { 1: ['LS_Left', 'RS_Left'], 2: ['LS_Right', 'RS_Right'], 4: ['LS_Up', 'RS_Up'], 8: ['LS_Down', 'RS_Down'] };
  const MODE_VERDICTS = ['战斗声', '音乐', '人声', '安静'];

  const escHtml = s => String(s == null ? '' : s).replace(/[&<>"]/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' }[c]));

  /* ---------------- 动态锚点（仅 BIND 创建） ---------------- */
  let anchors = null;
  function ensureAnchors() {
    if (!BIND) return null;
    if (anchors) return anchors;
    const tb = document.querySelector('.topbar');
    const badge = document.createElement('span');
    badge.id = 'bridge-badge';
    badge.style.cssText = 'font-size:11px;color:var(--ef-ink-2);letter-spacing:.4px';
    badge.hidden = true;
    if (tb) tb.appendChild(badge);
    const stopped = document.createElement('div');
    stopped.id = 'stopped-bar';
    stopped.style.cssText = 'position:fixed;left:50%;top:44px;transform:translateX(-50%);z-index:900;background:rgba(60,20,10,.92);color:#FFD7A0;border:1px solid rgba(255,120,60,.6);border-radius:6px;padding:5px 12px;font-size:12px;display:none';
    document.body.appendChild(stopped);
    const upd = document.createElement('div');
    upd.id = 'update-bar';
    upd.style.cssText = 'position:fixed;right:14px;bottom:14px;z-index:900;background:rgba(10,14,10,.94);color:var(--ef-ink);border:1px solid rgba(0,255,162,.35);border-radius:6px;padding:8px 12px;font-size:12px;max-width:340px;display:none';
    document.body.appendChild(upd);
    const cap = document.createElement('div');
    cap.id = 'raw-cap-bar';
    cap.style.cssText = 'position:fixed;left:50%;top:10px;transform:translateX(-50%);z-index:900;background:var(--ef-signal);color:#1A1400;border-radius:6px;padding:6px 14px;font-size:12px;font-weight:700;display:none';
    document.body.appendChild(cap);
    anchors = { badge, stopped, upd, cap };
    return anchors;
  }

  /* ---------------- 键名 ---------------- */
  const VK_NAMES = { 8: 'BACKSPACE', 9: 'TAB', 13: 'ENTER', 16: 'LSHIFT', 17: 'LCTRL', 18: 'LALT', 20: 'CAPSLOCK', 27: 'ESC', 32: 'SPACE', 33: 'PAGEUP', 34: 'PAGEDOWN', 35: 'END', 36: 'HOME', 37: 'LEFT', 38: 'UP', 39: 'RIGHT', 40: 'DOWN', 45: 'INSERT', 46: 'DELETE' };
  for (let i = 1; i <= 12; i++) VK_NAMES[111 + i] = 'F' + i;
  function vkName(vk) {
    vk = Number(vk) | 0;
    if (VK_NAMES[vk]) return VK_NAMES[vk];
    if (vk >= 65 && vk <= 90) return String.fromCharCode(vk);
    if (vk >= 48 && vk <= 57) return String.fromCharCode(vk);
    if (vk >= 96 && vk <= 105) return 'NUMPAD' + (vk - 96);
    return 'VK' + vk;
  }

  /* ---------------- 快照 → DOM ---------------- */
  function setBar(barId, numId, pct) {
    const b = document.getElementById(barId);
    if (b) b.style.setProperty('--p', (pct / 100).toFixed(3));
    const n = document.getElementById(numId);
    if (n) n.textContent = String(Math.round(pct)).padStart(3, '0') + '%';
  }
  function pressedTokens(s) {
    const inp = s.input || {};
    const out = [];
    if (inp.live_xinput) {
      const bm = Number(inp.live_xinput[1]) || 0;
      for (const b in XI_TOKENS) if (bm & Number(b)) out.push(XI_TOKENS[b]);
    }
    if (inp.live_hid) {
      const lh = inp.live_hid;
      if (lh.lt) out.push('LT');
      if (lh.rt) out.push('RT');
      if (HID_DPAD[lh.dpad]) out.push(HID_DPAD[lh.dpad]);
      for (const k in HID_AXIS) {
        if (lh.ls & Number(k)) out.push(HID_AXIS[k][0]);
        if (lh.rs & Number(k)) out.push(HID_AXIS[k][1]);
      }
    }
    return out;
  }
  function applyCore(s) {
    const c = s.core || {};
    if (c.paused !== undefined && c.paused !== state.paused) { state.paused = c.paused; setPauseUI(); renderMinimalState(); }
    if (c.language !== undefined && BIND && LANG_VARIANTS[c.language] && LANG_VARIANTS[c.language] !== state.lang) applyLang(LANG_VARIANTS[c.language]);
    if (BIND) {
      const a = ensureAnchors();
      if (a && a.badge) {
        const bits = [];
        if (s.version) bits.push(s.version);
        if (c.config_epoch !== undefined) bits.push('epoch ' + c.config_epoch);
        if (c.vib_legacy_client !== undefined) bits.push(c.vib_legacy_client ? 'S1 ACT' : 'S4+');
        a.badge.textContent = bits.join(' · ');
        a.badge.hidden = !bits.length;
      }
      if (a && a.stopped) {
        const st = (c.stopped_threads || []);
        a.stopped.style.display = st.length ? 'block' : 'none';
        a.stopped.textContent = st.length ? '⚠ 看护线程已放弃重启: ' + st.join('、') : '';
      }
    }
  }
  function applyInput(s) {
    const inp = s.input || {};
    rawCapActive = !!inp.raw_capture_active;
    const pressed = pressedTokens(s);
    document.querySelectorAll('#pad-hot .hot').forEach(h => h.classList.toggle('is-live', pressed.indexOf(h.dataset.slot) >= 0));
    if (BIND) {
      const a = ensureAnchors();
      if (a && a.cap) {
        if (rawCapActive) {
          a.cap.textContent = '● RawInput 捕获中（' + (inp.raw_capture_mode || '') + '）— Esc 取消';
          a.cap.style.display = 'block';
        } else a.cap.style.display = 'none';
      }
    }
  }
  function sliderVisual(r) {
    const min = +r.min || 0, max = +r.max || 100;
    r.style.setProperty('--p', ((r.value - min) / (max - min)).toFixed(3));
    const row = r.closest('.slider-row');
    const val = row ? row.querySelector('.val') : null;
    if (val) val.textContent = r._unit === 'frac' ? (r.value / 100).toFixed(2) : r._unit ? r.value + r._unit : r.value;
  }
  function echoSliders(v) {
    if (!BIND) return;
    document.querySelectorAll('input[type="range"][data-cfg-path]').forEach(sl => {
      if (sl === draggingSlider) return;
      const path = sl.dataset.cfgPath;
      let val = null;
      if (path === 'vibration.rank_level_gain') val = v.rank_level_gain;
      else if (path === 'vibration.rank_duration') val = v.rank_duration;
      else if (path.indexOf('vibration.rank_type_gain.') === 0) val = (v.rank_type_gain || [])[Number(path.slice(26))];
      else if (path.indexOf('vibration.item_lr.') === 0) val = (v.item_lr || [])[Number(path.slice(18)) * 2];
      else if (path.indexOf('vibration.rank_lr.') === 0) val = (v.rank_lr || [])[Number(path.slice(19)) * 2];
      if (val === null || val === undefined) return;
      if (String(sl.value) !== String(val)) { sl.value = String(val); sliderVisual(sl); }
    });
  }
  function applyVib(s) {
    const v = s.vibration || {};
    if (v.enabled !== undefined && v.enabled !== state.vib) {
      state.vib = v.enabled;
      const sv = document.getElementById('side-vib'); if (sv) sv.checked = v.enabled;
      const ts = document.getElementById('tb-vib-state'); if (ts) ts.textContent = v.enabled ? '震动 开' : '震动 关';
      renderMinimalState();
    }
    if (BIND) {
      const pctL = Math.max(0, Math.min(100, (Number(v.out_l) || 0) / 65535 * 100));
      const pctR = Math.max(0, Math.min(100, (Number(v.out_r) || 0) / 65535 * 100));
      setBar('vbar-l', 'vnum-l', pctL); setBar('vbar-r', 'vnum-r', pctR);
      setBar('bar-vl', 'num-vl', pctL); setBar('bar-vr', 'num-vr', pctR);
      echoSliders(v);
    }
  }
  function applyAudio(s) {
    const a = s.audio || {};
    if (a.enabled !== undefined && BIND) {
      const sw = document.getElementById('sw-audio');
      if (sw && sw.checked !== a.enabled) sw.checked = a.enabled;
    }
    if (!BIND) return;
    if (a.status !== undefined) {
      const eng = document.getElementById('a-engine');
      if (eng && a.status) eng.textContent = '引擎：' + a.status + ' · WASAPI 48kHz';
    }
    setBar('abar-l', 'anum-l', Math.max(0, Math.min(100, (Number(a.motor_l) || 0) / 65535 * 100)));
    setBar('abar-r', 'anum-r', Math.max(0, Math.min(100, (Number(a.motor_r) || 0) / 65535 * 100)));
    const bands = a.bands || [];
    document.querySelectorAll('#audio-bands li').forEach((li, i) => { if (bands[i] !== undefined) li.style.setProperty('--v', Number(bands[i]).toFixed(2)); });
    document.querySelectorAll('#ov-bands li').forEach((li, i) => { if (bands[i] !== undefined) li.style.setProperty('--v', Number(bands[i]).toFixed(2)); });
    const noise = document.getElementById('a-noise');
    if (noise) noise.textContent = (-60 + (Number(a.noise_floor) || 0) * 30).toFixed(1) + ' dB';
    const trans = document.getElementById('a-trans');
    if (trans) trans.textContent = (Number(a.transient) || 0).toFixed(3);
    const thresh = document.getElementById('a-thresh');
    if (thresh && a.threshold_eff !== undefined) thresh.textContent = String(Math.round((Number(a.threshold_eff) || 0) * 100));
    const verdict = document.getElementById('a-verdict');
    if (verdict && a.mode !== undefined) verdict.textContent = '判定：' + (MODE_VERDICTS[Number(a.mode) || 0] || '战斗声');
    const calib = document.getElementById('audio-calib');
    if (calib) {
      const cr = Number(a.calib_result) || 0;
      if (cr > 0) {
        calib.hidden = false;
        calib.innerHTML = '屏蔽校准完成：声音门槛建议 <b>' + cr + '</b>（低于门槛的杂音不震）。' +
          '<button class="ef-btn small go" data-act="audio-calib-apply">应用建议</button> ' +
          '<button class="ef-btn small ghost" data-act="audio-calib-dismiss">忽略</button>';
      }
      /* calib_result = 0 时不强制隐藏（本地 audio-calib-dismiss 已处理） */
    }
  }
  function applyUpdate(s) {
    const u = s.update || {};
    if (!BIND) return;
    const a = ensureAnchors(); if (!a) return;
    let html = '';
    if (u.phase === 'checking') html = '⟳ 检查更新中…';
    else if (u.error) html = '<span style="color:var(--ef-danger)">✗ 更新失败: ' + escHtml(u.error) + '</span>';
    else if (Number(u.total) > 0 && Number(u.done) < Number(u.total)) html = '↓ 下载中 ' + Math.round(Number(u.done) / Number(u.total) * 100) + '%';
    else if (u.latest_version) html = '↑ 发现新版本 ' + escHtml(u.latest_version);
    a.upd.innerHTML = html;
    a.upd.style.display = html ? 'block' : 'none';
    const latest = document.getElementById('upd-latest');
    if (latest && u.latest_version) latest.textContent = u.latest_version + '（有新版）';
    const err = document.getElementById('upd-err'); if (err) err.hidden = !u.error;
    if (Number(u.total) > 0) {
      const p = document.getElementById('upd-prog2');
      if (p) p.style.setProperty('--p', (Number(u.done) / Number(u.total)).toFixed(3));
      const n2 = document.getElementById('upd-num2');
      if (n2) n2.textContent = String(Math.round(Number(u.done) / Number(u.total) * 100)).padStart(3, '0') + '%';
    }
    const floatB = document.querySelector('#update-float b');
    if (floatB && u.latest_version) floatB.textContent = u.latest_version;
  }
  function applyCounts(s) {
    const c = s.counts || {};
    const map = { mappings: c.mappings, turbo: c.turbo, whitelist: c.whitelist };
    document.querySelectorAll('[data-count]').forEach(el => {
      const k = el.dataset.count;
      if (k === 'logs') return;
      if (map[k] !== undefined) el.textContent = String(map[k]);
    });
    const pairs = [['fig-mappings', c.mappings], ['fig-turbo', c.turbo], ['fig-combos', c.combos], ['n-combos', c.combos], ['n-set', c.mappings]];
    pairs.forEach(([id, v]) => { if (v !== undefined) { const el = document.getElementById(id); if (el) el.textContent = String(v); } });
    if (c.macros !== undefined) { const om = document.getElementById('ov-macros'); if (om) om.textContent = c.macros + ' 个通用宏'; }
  }
  function applySnapshot(snap) {
    lastSnap = snap;
    try { applyCore(snap); applyInput(snap); applyVib(snap); applyAudio(snap); applyUpdate(snap); applyCounts(snap); } catch (e) {}
  }

  /* ---------------- mock 本地状态机 → 快照推送器 ---------------- */
  function pushLocal() {
    if (REAL) return;
    H.pushSnapshot({
      core: { paused: state.paused },
      vibration: { enabled: state.vib },
      audio: { enabled: !!(document.getElementById('sw-audio') || {}).checked },
      counts: {
        mappings: setSlotCount(), turbo: DB.turbo.length, whitelist: DB.whitelist.length,
        combos: DB.chords.length, macros: DB.macros.length, presets: DB.presets.length,
      },
    });
  }

  /* ---------------- cfg / 命令出口 ---------------- */
  let cfgCache = null;
  function prefetchCfg() {
    H.cfg.get().then(r => { if (r && r.data) cfgCache = r.data; }).catch(() => {});
    H.cfg.paths().catch(() => {});
  }
  function findMappingIndex(trig) {
    if (!cfgCache) return null;
    const ms = cfgCache.mappings || [];
    for (let i = 0; i < ms.length; i++) if (ms[i] && ms[i].trigger_key === trig) return i;
    return null;
  }
  const encU32 = v => (v = Number(v), v < 0 ? v + 4294967296 : v);
  function encVal(path, value) {
    value = Number(value);
    if (path.indexOf('vibration.item_lr.') === 0 || path.indexOf('vibration.rank_lr.') === 0) return encU32(value);
    return value;
  }
  function cfgWrite(path, value, commit) {
    return H.cfg.write([{ path, value }], commit).catch(() => {});
  }
  function wlNames() { return DB.whitelist.map(w => w.name); }

  /* 滑杆：300ms 去抖 memory + 松手按族档位补正式 write（§4） */
  const sliderTimers = {};
  function wireSliders() {
    document.addEventListener('input', e => {
      const s = e.target;
      if (!s.matches || !s.matches('input[type="range"][data-cfg-path]')) return;
      draggingSlider = s;
      const path = s.dataset.cfgPath;
      clearTimeout(sliderTimers[path]);
      sliderTimers[path] = setTimeout(() => { sliderTimers[path] = null; cfgWrite(path, encVal(path, s.value), 'memory'); }, 300);
    });
    document.addEventListener('change', e => {
      const s = e.target;
      if (!s.matches || !s.matches('input[type="range"][data-cfg-path]')) return;
      if (draggingSlider === s) draggingSlider = null;
      const path = s.dataset.cfgPath;
      const val = encVal(path, s.value);
      if (sliderTimers[path]) { clearTimeout(sliderTimers[path]); sliderTimers[path] = null; cfgWrite(path, val, 'memory'); }
      const tier = H.cfg.tier(path) || {};
      cfgWrite(path, val, tier.settle || 'config-only');
    });
    /* 滑杆打标：等级增益/评分时长 + 评分细分 10 + LR 26/30（快照 item_lr[i*2] = L，照 hints.rs） */
    const rankHost = document.getElementById('vib-rank');
    if (rankHost) {
      const rs = rankHost.querySelectorAll('input[type="range"]');
      if (rs[0]) rs[0].dataset.cfgPath = 'vibration.rank_level_gain';
      if (rs[1]) rs[1].dataset.cfgPath = 'vibration.rank_duration';
    }
    const rankEvt = document.getElementById('vib-rank-events');
    if (rankEvt) rankEvt.querySelectorAll('input[type="range"]').forEach((s, i) => { s.dataset.cfgPath = 'vibration.rank_type_gain.' + i; });
    const lrItem = document.getElementById('vib-lr');
    if (lrItem) lrItem.querySelectorAll('input[type="range"]').forEach((s, i) => { s.dataset.cfgPath = 'vibration.item_lr.' + i; });
    const lrRank = document.getElementById('vib-lr-rank');
    if (lrRank) lrRank.querySelectorAll('input[type="range"]').forEach((s, i) => { s.dataset.cfgPath = 'vibration.rank_lr.' + i; });
  }

  /* 设置族开关即时写入 */
  function wireCheckboxes() {
    [['sw-dfo', 'dfo_player'], ['sw-audio', 'audio_haptics.enabled']].forEach(([id, path]) => {
      const cb = document.getElementById(id);
      if (!cb || cb.dataset.wired) return;
      cb.dataset.wired = '1';
      cb.addEventListener('change', () => cfgWrite(path, cb.checked, path === 'audio_haptics.enabled' ? 'config-only' : 'config-only'));
    });
  }

  /* ---------------- UiEvent（外标签枚举） ---------------- */
  function capturedInput(kindName, name) {
    const gcap = document.getElementById('gp-cap');
    if (gcap && state.gpFlow === 'selected') {
      const arr = state.captures.gp || (state.captures.gp = []);
      if (!arr.includes(name)) arr.push(name);
      gcap.classList.remove('is-waiting');
      const hint = gcap.querySelector('.cap-hint'); if (hint) hint.textContent = '已捕获';
      const capVal = document.getElementById('gp-cap-val');
      if (capVal) capVal.innerHTML = arr.map(k => `<span class="kbd">${escHtml(k)}</span>`).join(' ');
      setFlowStep(3);
      state.gpFlow = 'done';
      const slot = state.gpSlot;
      if (slot) {
        if (!DB.padSlots[slot]) DB.padSlots[slot] = {};
        DB.padSlots[slot].tgt = arr.join('+');
        const kb2 = document.getElementById('gp-slot-kbbadge');
        if (kb2) kb2.innerHTML = '<span class="kbd">' + escHtml(arr.join('+')) + '</span>';
        const sum2 = document.getElementById('gp-slot-summary');
        const s2 = SLOTS.find(x => x[0] === slot);
        if (sum2 && s2) { sum2.textContent = s2[5] + ' → 键盘 ' + arr.join('+'); sum2.style.color = 'var(--ef-ink)'; }
        mountPadRefresh();
        const fe = document.getElementById('gp-flow-edit'); if (fe) fe.hidden = true;
        const fd = document.getElementById('gp-flow-done');
        if (fd) { fd.hidden = false; const dt = document.getElementById('gp-done-text'); if (dt) dt.innerHTML = `✓ ${escHtml(slot)} 键 → 键盘 <span class="kbd is-signal">${escHtml(arr.join('+'))}</span>`; }
        renderUnsetChips(); renderNavCounts(); drawDial();
        const qs2 = document.getElementById('gp-qs'); if (qs2) qs2.hidden = true;
      }
    }
    toast('已捕获' + kindName + '：' + name);
  }
  function showHidData(d) {
    if (!d) return;
    openDlg('dlg-hid');
    const host = document.getElementById('dlg-hid');
    if (!host) return;
    const bytes = d.data || [];
    const hex = bytes.map(b => ('0' + Number(b).toString(16)).slice(-2).toUpperCase()).join(' ');
    let pre = host.querySelector('.hid-hex-pre');
    if (!pre) {
      pre = document.createElement('pre');
      pre.className = 'hid-hex-pre';
      pre.style.cssText = 'font:11px Consolas,monospace;background:rgba(0,0,0,.3);padding:8px;border-radius:6px;margin-top:10px;white-space:pre-wrap;word-break:break-all';
      host.appendChild(pre);
    }
    pre.textContent = 'HID 报文 (handle ' + d.handle + ', ' + bytes.length + ' 字节)\n' + (hex || '（空）');
  }
  function onEvent(ev) {
    if (!ev || typeof ev !== 'object') return;
    if (ev.RawCaptured !== undefined) {
      const d = ev.RawCaptured;
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
    if (ev.__hello !== undefined && REAL) toast('桥接就绪 · ' + (ev.__hello.version || ''));
  }

  /* ---------------- data-act → 命令出口（pre 钩子 + 原 handleAct + after 出口） ---------------- */
  function preAct(act) {
    if (act === 'kb-apply' || act === 'kb-conflict-overwrite' || act === 'kb-conflict-skip') {
      return { pick: Array.from(state.kbPick || []), drop: Array.from(state.kbDrop || []) };
    }
    return null;
  }
  function sendMappingUpsert(trig, targets, opts, indexHint) {
    const idx = findMappingIndex(trig);
    H.cmd('UpsertMapping', {
      index: idx !== null ? idx : (indexHint !== undefined ? indexHint : null),
      mapping: {
        trigger_key: trig, target_keys: targets,
        turbo_enabled: !!(opts && opts.turbo),
        lock_enabled: !!(opts && opts.lock),
        double_tap_enabled: !!(opts && opts.run),
        run_enabled: !!(opts && opts.rerun),
        interval: opts && opts.interval !== undefined ? opts.interval : null,
        event_duration: opts && opts.dur !== undefined ? opts.dur : null,
        sequence_text: (opts && opts.seq) || '',
        note: (opts && opts.note) || '',
      },
    });
  }
  function afterAct(act, el, pre) {
    switch (act) {
      case 'toggle-pause': H.cmd('toggle-pause'); pushLocal(); break;
      case 'toggle-vib': {
        const ts = document.getElementById('tb-vib-state');
        const on = ts ? ts.textContent.indexOf('开') >= 0 : state.vib;
        state.vib = on;
        H.cmd('SetVibrationEnabled', { value: on });
        pushLocal();
        break;
      }
      case 'toggle-theme': H.cmd('SetDarkMode', { value: (document.documentElement.dataset.theme || 'dark') === 'dark' }); break;
      case 'save': H.cmd('save'); break;
      case 'test-vibration': H.cmd('test-vibration'); break;
      case 'test-rank': H.cmd('test-rank'); break;
      case 'audio-self-check': H.cmd('audio-self-check'); break;
      case 'audio-calibrate': H.cmd('audio-calibrate'); break;
      case 'audio-calib-dismiss': H.cmd('audio-calib-dismiss'); break;
      case 'audio-calib-apply': if (lastSnap && lastSnap.audio) cfgWrite('audio_haptics.mask_threshold', lastSnap.audio.calib_result, 'config-only'); break;
      case 'refresh-devices': H.cmd('refresh-devices'); break;
      case 'reset-devices': H.cmd('reset-devices'); break;
      case 'check-update': H.cmd('check-update'); break;
      case 'quit-do': H.cmd('quit'); break;
      case 'show-guide': H.cmd('show-guide'); break;
      /* ★v1.5 (契约 §11.6): 混合 mod 窗里「⤺ 完整界面」→ shell.stop{mode:"full"}
         真命令（结束 mod 会话 → 还原 egui 完整窗）；mock/纯壳保持 SetMode 旧演示 */
      case 'exit-minimal':
        if ((REAL || window.__BRIDGE_SMOKE__) && window.__SHELL_INFO__ && window.__SHELL_INFO__.hybrid === true) H.call({ t: 'shell.stop', mode: 'full' });
        else H.cmd('SetMode', { minimal: false, classic: false });
        break;
      case 'apply-preset': { const sel = document.getElementById('top-preset'); if (sel && sel.value) H.cmd('ApplyMappingPreset', { name: sel.value }); break; }
      case 'tb-apply-preset': { const sel2 = document.getElementById('tb-preset'); if (sel2 && sel2.value) H.cmd('ApplyMappingPreset', { name: sel2.value }); break; }
      case 'tb-save-preset': case 'set-save-preset': {
        const nm = el && el.closest ? el.closest('.ef-actions, .ef-form, section') : null;
        const inp = nm ? nm.querySelector('input[type="text"]') : null;
        if (inp && inp.value.trim()) H.cmd('SaveMappingPreset', { name: inp.value.trim() });
        break;
      }
      case 'tb-delete-preset': case 'set-delete-preset': {
        const cur = DB.currentPreset || '';
        if (cur) H.cmd('DeleteMappingPreset', { name: cur });
        break;
      }
      case 'tb-switch-save': {
        const keys = state.switchKeys || [];
        if (keys.length) cfgWrite('switch_key', keys.join('+'), 'config-only');
        break;
      }
      case 'tb-switch-clear': cfgWrite('switch_key', '', 'config-only'); break;
      case 'tb-switch-back': cfgWrite('switch_key', (state.switchKeys || []).join('+'), 'config-only'); break;
      case 'guide-dfo-yes': cfgWrite('dfo_player', true, 'config-only'); break;
      case 'guide-dfo-no': cfgWrite('dfo_player', false, 'config-only'); break;
      case 'wl-add': case 'wl-quick': case 'set-wl-add': cfgWrite('process_whitelist', wlNames(), 'reload'); pushLocal(); break;
      case 'settings-save': {
        const muts = [];
        const lang = document.querySelector('input[name="lang"]:checked');
        if (lang) muts.push({ path: 'language', value: { zh: 'SimplifiedChinese', tw: 'TraditionalChinese', en: 'English', jp: 'Japanese' }[lang.value] || 'SimplifiedChinese' });
        const dfo = document.getElementById('sw-dfo'); if (dfo) muts.push({ path: 'dfo_player', value: dfo.checked });
        const au = document.getElementById('sw-audio'); if (au) muts.push({ path: 'audio_haptics.enabled', value: au.checked });
        muts.push({ path: 'dark_mode', value: (document.documentElement.dataset.theme || 'dark') === 'dark' });
        const mode = document.querySelector('input[name="mode"]:checked');
        if (mode) muts.push({ path: mode.value === 'minimal' ? 'minimal_mode' : mode.value === 'classic' ? 'classic_mode' : 'minimal_mode', value: mode.value !== 'full' });
        H.cfg.write(muts, 'config-only').catch(() => {});
        break;
      }
      case 'kb-apply': case 'kb-conflict-overwrite': case 'kb-conflict-skip': {
        if (pre) {
          pre.pick.forEach(k => sendMappingUpsert(k, [k], { turbo: true }));
          pre.drop.forEach(k => { const i = findMappingIndex(k); if (i !== null) H.cmd('DeleteMapping', { value: i }); });
        }
        pushLocal();
        break;
      }
      case 'set-add-mapping': {
        const trig = (state.captures['new-trig'] || []).join('+');
        const tgt = (state.captures['new-tgt'] || []).join('+');
        if (trig && tgt) sendMappingUpsert(trig, [tgt], {});
        pushLocal();
        break;
      }
      case 'set-del': {
        const i = el && el.dataset ? +el.dataset.setDel : NaN;
        if (!isNaN(i) && DB.kbMappings[i]) {
          const idx = findMappingIndex(DB.kbMappings[i].trig);
          H.cmd('DeleteMapping', { value: idx !== null ? idx : i });
        }
        pushLocal();
        break;
      }
      case 'mod-disable': if (REAL) H.shell.stop(); break;
      case 'mod-apply-dyn': if (REAL && el && el.dataset) H.shell.apply(el.dataset.modId || 'endfield'); break;
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
      /* 捕获流（§7）：第 2 步发起捕获 / 完成确认落位（UpsertMapping） */
      case 'gp-await-kb': if (BIND) H.capture.start('target'); break;
      case 'gp-recal': if (BIND) H.capture.start('trigger'); break;
      case 'gp-done': {
        const slot = state.gpSlot;
        const keys = state.captures && state.captures.gp ? state.captures.gp.slice() : [];
        if (slot && keys.length) sendMappingUpsert(slot, keys, {});
        pushLocal();
        break;
      }
      default: break;
    }
  }
  const _handleAct = handleAct;
  handleAct = function (act, el, e) {
    /* 冒烟模式抑制整页跳转（mod-disable / mod-apply-self 原逻辑是 location.href） */
    if (window.__BRIDGE_SMOKE__ && (act === 'mod-disable' || act === 'mod-apply-self')) {
      if (e && e.preventDefault) e.preventDefault();
      if (act === 'mod-disable') H.shell.stop();
      toast('（冒烟）shell.stop → classic');
      return true;
    }
    const pre = preAct(act);
    const r = _handleAct(act, el, e);
    try { afterAct(act, el, pre); } catch (e2) {}
    return r;
  };
  /* 非 data-act 委托的行内编辑/删除按钮（data-* 属性） */
  document.addEventListener('click', e => {
    const tsave = e.target.closest('[data-tedit-save]');
    if (tsave) {
      const i = +tsave.dataset.teditSave, m = DB.turbo[i];
      if (m) sendMappingUpsert(m.trig, [m.tgt], { turbo: (m.flags || []).includes('连发'), lock: (m.flags || []).includes('锁定'), run: (m.flags || []).includes('简易奔跑'), interval: m.interval, dur: m.dur, seq: m.seq || '', note: m.note || '' }, i);
      pushLocal();
      return;
    }
    const tdel = e.target.closest('[data-tdelete]');
    if (tdel) {
      const i2 = +tdel.dataset.tdelete, m2 = DB.turbo[i2];
      if (m2) { const idx = findMappingIndex(m2.trig); H.cmd('DeleteMapping', { value: idx !== null ? idx : i2 }); }
      pushLocal();
      return;
    }
    const wld = e.target.closest('[data-wl-del]');
    if (wld) { cfgWrite('process_whitelist', wlNames(), 'reload'); pushLocal(); return; }
    const swld = e.target.closest('[data-setwl-del]');
    if (swld) { cfgWrite('process_whitelist', wlNames(), 'reload'); pushLocal(); return; }
  });
  /* 单选/开关 change → 命令（语言 / 模式 / 路线） */
  document.addEventListener('change', e => {
    const t = e.target;
    if (t.name === 'lang' && t.checked) H.cmd('SetLanguage', { language: { zh: 'SimplifiedChinese', tw: 'TraditionalChinese', en: 'English', jp: 'Japanese' }[t.value] || 'SimplifiedChinese' });
    /* ★v1.5 (契约 §11.6): 混合 mod 窗（hello.hybrid=true）里顶栏/设置页的
       「完整/极简/经典模式」radio → shell.stop{mode} 真命令: 结束 mod 会话（关窗）
       → Win32 还原 egui → egui 帧头应用目标模式。mock/纯壳保持 SetMode 旧演示
       （SetMode 只落盘 config, 混合模式下 egui 隐藏期不可见, 故必须走停用切换）。 */
    if ((t.name === 'mode' || t.name === 'mode2') && t.checked) {
      if ((REAL || window.__BRIDGE_SMOKE__) && window.__SHELL_INFO__ && window.__SHELL_INFO__.hybrid === true) H.call({ t: 'shell.stop', mode: t.value });
      else H.cmd('SetMode', { minimal: t.value === 'minimal', classic: t.value === 'classic' });
    }
    if (t.name === 'vibroute' && t.checked) H.cmd('SwitchVibEdition', { legacy: t.value === 's1' });
    if (t.id === 'set-route') H.cmd('SwitchVibEdition', { legacy: String(t.value).indexOf('S1') >= 0 });
    if (t.id === 'quick-preset' || t.id === 'tb-preset') { if (t.value) H.cmd('ApplyMappingPreset', { name: t.value }); }
  });
  /* Esc → capture.cancel（§7）；滑杆拖起标记 */
  document.addEventListener('keydown', e => {
    if (e.key === 'Escape' && rawCapActive && BIND) H.capture.cancel();
  });
  document.addEventListener('mousedown', e => {
    if (e.target && e.target.matches && e.target.matches('input[type="range"][data-cfg-path]')) draggingSlider = e.target;
  }, true);
  document.addEventListener('mouseup', () => { draggingSlider = null; }, true);

  /* ---------------- 真桥：mods.list 渲染主题卡 ---------------- */
  /* ★J 轮: 挂载点固定为「外观 MOD」页的 #mod-cards-dyn（此前动态插到文档第一个
     .ef-panel 前 = 概览页 QUICK ACTIONS 上方，用户实测后要求入口移到左侧栏） */
  function renderModCards(mods) {
    if (!REAL || !mods || !mods.length) return;
    const host = document.getElementById('mod-cards-dyn') || (function () {
      const page = document.getElementById('page-mods');
      if (!page) return null;
      const w = document.createElement('div');
      w.id = 'mod-cards-dyn';
      page.appendChild(w);
      return w;
    })();
    if (!host) return;
    host.innerHTML = '<section class="ef-panel"><h2><span class="ef-eyebrow">Mods</span>外观 MOD（宿主清单）</h2>' +
      mods.map(m => '<div class="ef-record" data-mod-id="' + escHtml(m.id) + '">' +
        '<span class="ef-tag">' + (m.builtin ? '内置' : '外部') + '</span>' +
        '<span class="name">' + escHtml(m.name) + ' <small>' + escHtml(m.version) + '</small></span>' +
        '<span class="ef-field">' + escHtml(m.description || '') + '</span>' +
        (m.previewUrl ? '<img src="' + escHtml(m.previewUrl) + '" alt="" style="height:44px;border-radius:4px">' : '') +
        (m.id === 'classic' ? '<span class="ef-state">使用中</span>' : '<button class="ef-btn small go" data-act="mod-apply-dyn" data-mod-id="' + escHtml(m.id) + '">应用</button>') +
        (m.compatible === false ? '<span class="ef-state is-draft">不兼容</span>' : '') +
        '</div>').join('');
  }

  /* ---------------- 启动 ---------------- */
  H.onSnapshot(applySnapshot);
  H.onEvent(onEvent);
  if (BIND) {
    ensureAnchors(); wireSliders(); wireCheckboxes(); prefetchCfg();
    /* 实机口径：真机手柄图显示实时键位高亮，「演示：实体按下」胶囊仅 mock 演示保留 */
    const demoPress = document.querySelector('[data-act="gp-demo-press"]');
    if (demoPress) demoPress.style.display = 'none';
  }
  if (REAL) H.mods.list().then(r => { if (r && r.data) renderModCards(r.data); }).catch(() => {});
  pushLocal();
  window.__BIND_READY__ = true;
})();