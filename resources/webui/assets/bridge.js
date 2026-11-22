/* ============================================================
   bridge.js — 双壳共用桥适配层（双UI桥契约 v1 §2 / §3）
   ------------------------------------------------------------
   真桥模式：window.__SORAHK_BRIDGE__ + window.wp 存在时启用。
     JS→Rust  window.wp.postMessage(JSON字符串)  {id, t, ...}
     Rust→JS  window.__hostPush(JSON字符串)      {t: hello|snap|evt|reply}
     SorahkHost.call(payload) → Promise（自增 id 匹配 reply）
   mock 模式（file:// 直开）：行为与现状完全一致（回归红线）。
     内部数据流走同一套 onSnapshot/onEvent —— 页面把自身 mock 状态机
     适配成快照推送器（SorahkHost.pushSnapshot），渲染代码一份。
   调试模式：URL ?bridge=mock
     · 内置 mock 快照序列（300/600/900/1200ms 各推一拍）
     · window.__CALLS__ 记录全部 call() 载荷（自动化断言用）
     · window.__injectSnap(obj) 手动注入快照（域级合并）
   冒烟子模式：&smoke=calls | debounce | paths | nav | win | patch（结果写入 #smoke-calls）
   v1.3 变更：
     · 真桥 snap 差量帧（full 替换 / patch 浅合并）→ lastSnap 缓存 → rAF 批处理 emit
       （页面 applySnapshot 接口不变, 收到的永远是合并后的完整快照）;
     · 窗口镶边全量 JS 化（installWindowChrome 双壳通用; hello.dragMode=js 默认）,
       replica.js 的旧实现已改造为委托本文件。
   ============================================================ */
'use strict';
(function () {
  var W = window;
  var qs = null;
  try { qs = new URLSearchParams(location.search); } catch (e) { qs = null; }

  var REAL = !!(W.__SORAHK_BRIDGE__ && W.wp && typeof W.wp.postMessage === 'function');
  var DBG = !REAL && !!qs && qs.get('bridge') === 'mock';
  var SMOKE = DBG ? (qs.get('smoke') || '') : '';
  var BIND = REAL || DBG; /* 绑定模式 = 新行为生效（真桥 / 调试）；纯 mock 保持 v11 行为 */
  if (DBG) { W.__BRIDGE_SMOKE__ = !!SMOKE; W.__CALLS__ = []; try { W.__BRIDGE_DBG__ = !!(qs && qs.get('dbg')); } catch (e) {} }

  /* ================= mock 快照状态（六域照 ui_api serde 名） ================= */
  function fill(n, v) { var a = []; for (var i = 0; i < n; i++) a.push(v === undefined ? 0 : v); return a; }
  function defaultSnapshot() {
    return {
      version: 'R2.14',
      core: {
        paused: false, language: 'SimplifiedChinese', config_epoch: 1,
        workers_configured: 4, workers_actual: 4, stopped_threads: [],
        injection_guard_hits: 0, injection_guard_passed: 0, injection_guard_whitelisted: true,
        gp_calibrating: false, sequence_paused: false, key_record_active: false, vib_legacy_client: false,
      },
      input: {
        live_hid_pad: null, live_hid: null, live_xinput: null,
        raw_capture_active: false, raw_capture_mode: 'MostSustained',
        xinput_capture_mode: 'DiagonalPriority', has_semantic_hid: false,
      },
      vibration: {
        enabled: true, connected: true, font_hits: false, advanced_enabled: true,
        independent_test: false, legacy_output_mode: 0, events_received: 0,
        rank_events: 0, rank_last: 0, params: fill(60, 0),
        item_lr: fill(52, 0), rank_lr: fill(60, 0), rank_type_gain: fill(15, 0),
        rank_type_events: fill(15, 0), rank_level_gain: 70, rank_duration: 480,
        out_l: 0, out_r: 0, rank_out: fill(30, 0), item_out: fill(52, 0),
        test_until_ms: 0, now_ms: 0, algo_id: 1, algo_ap: [60, 55, 48, 30],
      },
      audio: {
        enabled: false, status: '已关闭', motor_l: 0, motor_r: 0,
        bands: [0, 0, 0, 0, 0], band_peak: [0, 0, 0, 0, 0],
        level_l: 0, level_r: 0, mode: 0, noise_floor: 0, transient: 0, threshold_eff: 0,
        wl_entries: 0, wl_source: '', wl_foreground: '', wl_matched: false, wl_degraded: false, wl_error: '',
        self_check_active: false, calib_result: 0,
      },
      update: {
        phase: 'idle', latest_version: null, latest_notes: null, error: null,
        done: 0, total: 0, active_channel: null, saved_path: null, source: null,
        last_check_unix: 0, next_check_unix: 0,
      },
      counts: { mappings: 0, turbo: 0, combos: 0, presets: 0, whitelist: 0, macros: 0 },
    };
  }
  var mock = defaultSnapshot();

  function deepCopy(o) { return JSON.parse(JSON.stringify(o)); }
  function snapCopy() { return deepCopy(mock); }
  function mergeSnap(src) {
    if (!src || typeof src !== 'object') return;
    if (src.version !== undefined) mock.version = String(src.version);
    ['core', 'input', 'vibration', 'audio', 'update', 'counts'].forEach(function (k) {
      if (src[k] && typeof src[k] === 'object') {
        var d = mock[k], s = src[k];
        for (var f in s) if (Object.prototype.hasOwnProperty.call(s, f)) d[f] = s[f];
      }
    });
  }

  var snapCbs = [];
  var evtCbs = [];
  /* v1.3 差量快照: 真桥下 lastSnap = 宿主最近快照的合并缓存 (full 替换 / patch 浅合并) */
  var lastSnap = null;
  var snapRafPending = false;
  function emitSnap() {
    /* 真桥: 深拷贝合并缓存给页面（页面拿到的永远是完整快照, applySnapshot 接口不变）;
       mock/DBG: 深拷贝内置 mock 状态（v11 既有行为, 零回归）。 */
    var s = (REAL && lastSnap) ? deepCopy(lastSnap) : snapCopy();
    for (var i = 0; i < snapCbs.length; i++) { try { snapCbs[i](s); } catch (e) {} }
  }
  function emitEvent(ev) {
    for (var i = 0; i < evtCbs.length; i++) { try { evtCbs[i](ev); } catch (e) {} }
  }
  /* rAF 批处理: 高频 evaluate 到达先暂存进缓存, 一帧最多 emit 一次页面回调
     （消除高频推送造成的布局抖动; 无 rAF 环境兜底同步 emit）。 */
  function emitSnapSoon() {
    if (snapRafPending) return;
    snapRafPending = true;
    if (typeof W.requestAnimationFrame === 'function') {
      W.requestAnimationFrame(function () { snapRafPending = false; emitSnap(); });
    } else {
      snapRafPending = false;
      emitSnap();
    }
  }
  /* 真桥 snap 帧处理 (契约 §3.1 v1.3): full → 整体替换缓存; patch → Object.assign
     按域浅合并（泵侧 patch 域值 = 该域完整序列化对象, 域级替换无损）;
     统一 rAF emit。返回合并后的完整缓存（DBG smoke=patch 断言探针用）。
     v1.2 旧壳全量帧 {"t":"snap","data":…}（无 full/patch 键）同走整体替换, 兼容不回归。 */
  function applyHostSnap(msg) {
    if (msg.patch && typeof msg.patch === 'object' && msg.data === undefined) {
      lastSnap = lastSnap || {};
      Object.assign(lastSnap, msg.patch);
    } else {
      lastSnap = msg.data || {};
    }
    emitSnapSoon();
    return lastSnap;
  }

  /* ================= cfg 白名单表（mock 口径，A 落表后以 cfg.paths 实测为准） ================= */
  /* tier: memory(拖动去抖) + settle(松手正式档)；离散写入直接用 settle 档 */
  var CFG_PATHS = {
    'vibration.enabled': { settle: 'vib' },
    'vibration.advanced_enabled': { settle: 'vib' },
    'vibration.master_gain': { settle: 'vib' }, 'vibration.max_strength': { settle: 'vib' },
    'vibration.attack_gain': { settle: 'vib' }, 'vibration.damage_gain': { settle: 'vib' },
    'vibration.shake_gain': { settle: 'vib' }, 'vibration.move_gain': { settle: 'vib' },
    'vibration.decay_ms': { settle: 'vib' }, 'vibration.hit_boost': { settle: 'vib' },
    'vibration.start_pulse': { settle: 'vib' }, 'vibration.kill_pulse': { settle: 'vib' },
    'vibration.random_gain': { settle: 'vib' }, 'vibration.out_smooth': { settle: 'vib' },
    'vibration.font_strength': { settle: 'vib' }, 'vibration.font_interval': { settle: 'vib' },
    'vibration.rhythm': { settle: 'vib' }, 'vibration.font_hp': { settle: 'vib' },
    'vibration.font_special': { settle: 'vib' }, 'vibration.font_state': { settle: 'vib' },
    'vibration.font_effect': { settle: 'vib' }, 'vibration.font_attack': { settle: 'vib' },
    'vibration.font_hit': { settle: 'vib' },
    'vibration.rank_level_gain': { settle: 'vib' }, 'vibration.rank_duration': { settle: 'vib' },
    'vibration.rank_decay_delay_ms': { settle: 'vib' }, 'vibration.rank_decay_speed': { settle: 'vib' },
    'vibration.rank_decay_min_mul': { settle: 'vib' }, 'vibration.rank_decay_max_mul': { settle: 'vib' },
    'vibration.item_lr.*': { settle: 'vib' },
    'vibration.rank_lr.*': { settle: 'vib' },
    'vibration.rank_type_gain.*': { settle: 'vib' },
    'vibration.throttle_window_ms': { settle: 'vib' }, 'vibration.throttle_max_hits': { settle: 'vib' },
    'vibration.out_threshold': { settle: 'vib' },
    'audio_haptics.*': { settle: 'config-only' },
    'input.rawinput_capture_mode': { settle: 'config-only' },
    'input.xinput_capture_mode': { settle: 'config-only' },
    'language': { settle: 'config-only' },
    'dark_mode': { settle: 'config-only' },
    'always_on_top': { settle: 'config-only' },
    'show_tray_icon': { settle: 'config-only' },
    'show_notifications': { settle: 'config-only' },
    'dfo_player': { settle: 'config-only' },
    'switch_key': { settle: 'config-only' },
    'input_timeout': { settle: 'config-only' },
    'interval': { settle: 'config-only' },
    'event_duration': { settle: 'config-only' },
    'worker_count': { settle: 'config-only' },
    'whitelist_simple_mode': { settle: 'reload' },
    'process_whitelist': { settle: 'reload' },
  };
  function pathTier(path) {
    if (Object.prototype.hasOwnProperty.call(CFG_PATHS, path)) return CFG_PATHS[path];
    var segs = path.split('.');
    for (var k in CFG_PATHS) {
      if (k.indexOf('.*') !== k.length - 2) continue;
      var pre = k.slice(0, -1); /* 去掉尾部 *，留 "a.b." */
      if (path.indexOf(pre) === 0) {
        var rest = path.slice(pre.length);
        if (rest.indexOf('.') < 0) return CFG_PATHS[k];
      }
    }
    return null;
  }

  /* mock AppConfig（cfg.get 回显用；serde 名照 config.rs AppConfig 子集） */
  function mockConfig() {
    return {
      language: mock.core.language,
      dark_mode: true, always_on_top: false, show_tray_icon: true, show_notifications: true,
      minimal_mode: false, classic_mode: false, dfo_player: true,
      vib_legacy_client: mock.core.vib_legacy_client,
      switch_key: 'DELETE', input_timeout: 1000, interval: 5, event_duration: 5, worker_count: 4,
      rawinput_capture_mode: 'AdaptiveIntelligent', xinput_capture_mode: 'MostSustained',
      whitelist_simple_mode: false,
      process_whitelist: ['dnf.exe', 'WeGame.exe', 'DNFLauncher.exe'],
      mappings: [
        { trigger_key: 'J', target_keys: ['J'], turbo_enabled: true, interval: null, event_duration: null, note: '普通攻击' },
        { trigger_key: 'K', target_keys: ['K'], turbo_enabled: true, interval: null, event_duration: null, note: '技能1' },
        { trigger_key: 'CTRL+F1', target_keys: ['1'], turbo_enabled: false, interval: null, event_duration: null, note: '' },
      ],
      current_preset: 'PVE通用',
      /* ★M 轮: mock 也提供 presets 数组（Preset[] 形态），让 cfg.get → 回填走与真桥一致的路径 */
      presets: [
        { name: 'PVE通用', mappings: [], switch_key: '' },
        { name: '搬砖-风法', mappings: [], switch_key: '' },
        { name: '决斗专用', mappings: [], switch_key: '' },
      ],
      audio_haptics: {
        enabled: false, preset: '人声清晰', master_gain_pct: 75, mask_threshold: 43,
        adaptive_threshold: true, auto_profile: true, dual_motor: true, vocal_only: false,
      },
      vibration: { enabled: true, advanced_enabled: true, rank_level_gain: 70, rank_duration: 480 },
    };
  }

  var MOCK_MODS = [
    { id: 'classic', name: '原版经典', version: 'R2.14', engine: 'R2.14+', author: '118coder',
      description: '宿主本体 1:1 复刻界面（内置默认，不可卸载）。', type: 'shell', builtin: true, compatible: true },
    { id: 'endfield', name: '终末地风格', version: '1.0.0', engine: 'R2.14+', author: '118coder',
      description: '工业风 / 明黄 #FFFA00 / 切角面板 / 赛博机甲 —— 面向 DNF 手柄工具的全新视觉。',
      type: 'shell', entry: 'endfield.html', previewUrl: '', builtin: true, compatible: true },
  ];

  /* ================= mock 命令处理（t:"cmd"） ================= */
  function mockHandleCmd(payload) {
    var act = payload.act || '';
    var p = payload.params || {};
    var core = mock.core, vib = mock.vibration, au = mock.audio, upd = mock.update;
    switch (act) {
      case 'toggle-pause': core.paused = !core.paused; break;
      case 'SetPaused': core.paused = !!p.value; break;
      case 'SetVibrationEnabled': vib.enabled = !!p.value; break;
      case 'SetLanguage': if (p.language) core.language = p.language; break;
      case 'SetDarkMode': case 'SetAlwaysOnTop': case 'SetTrayEnabled':
      case 'SetNotificationsEnabled': case 'SetMode': break;
      case 'SwitchVibEdition': if (p.legacy !== undefined) core.vib_legacy_client = !!p.legacy; break;
      case 'ApplyMappingPreset': core.config_epoch++; break;
      case 'SaveMappingPreset': case 'DeleteMappingPreset': break;
      case 'UpsertMapping': core.config_epoch++; if (p.index === null || p.index === undefined) mock.counts.mappings++; break;
      case 'DeleteMapping': core.config_epoch++; mock.counts.mappings = Math.max(0, mock.counts.mappings - 1); break;
      case 'test-pulse': case 'TestPulse': vib.test_until_ms = Date.now() + 1000; break;
      case 'test-vibration': case 'TestVibration': vib.test_until_ms = Date.now() + 1000; break;
      case 'test-rank': case 'TestRank': vib.test_until_ms = Date.now() + 1200; break;
      case 'audio-self-check': case 'AudioSelfCheck':
        au.self_check_active = true;
        setTimeout(function () { au.self_check_active = false; emitSnap(); }, 600);
        break;
      case 'audio-calibrate': case 'AudioCalibrate':
        au.calib_result = 0;
        setTimeout(function () { au.calib_result = 46; emitSnap(); }, 1200);
        break;
      case 'audio-calib-dismiss': case 'AudioCalibDismiss': au.calib_result = 0; break;
      case 'refresh-devices': case 'RefreshDevices': case 'reset-devices': case 'ResetDevices': break;
      case 'check-update': case 'CheckUpdate':
        upd.phase = 'checking'; upd.error = null;
        setTimeout(function () { upd.phase = 'done'; upd.latest_version = 'R2.99'; emitSnap(); }, 800);
        break;
      case 'cancel-update': case 'CancelDownload': upd.phase = 'idle'; break;
      case 'dismiss-update-error': case 'DismissUpdateError': upd.error = null; break;
      case 'save': case 'SaveAll': case 'quit': case 'Quit':
      case 'show-guide': case 'ShowGuide': case 'RestartThreads': break;
      default: break;
    }
    emitSnap();
  }

  /* mock 捕获流：capture.start → 快照驱动 → 500ms 后回 RawCaptured（仅调试模式自动回填） */
  function mockCapture(kind) {
    mock.input.raw_capture_active = true;
    mock.input.raw_capture_mode = kind === 'trigger' ? 'MaxSetBits' : 'MostSustained';
    emitSnap();
    if (DBG) {
      setTimeout(function () {
        mock.input.raw_capture_active = false;
        emitSnap();
        /* UiEvent 外标签枚举：{RawCaptured: <InputDevice>}；
           InputDevice 也是外标签：Keyboard(vk 0x46='F') / 手柄 trigger 用 GenericDevice */
        if (kind === 'trigger') emitEvent({ RawCaptured: { GenericDevice: { device_type: 1, button_id: 629989901 } } });
        else emitEvent({ RawCaptured: { Keyboard: 70 } });
      }, 500);
    }
  }

  /* ================= mock call() 分发 ================= */
  function mockCall(payload) {
    return new Promise(function (resolve) {
      var t = payload.t || '';
      if (t === 'cmd') { mockHandleCmd(payload); resolve({ ok: true }); return; }
      if (t === 'cfg.paths') { resolve({ ok: true, data: CFG_PATHS }); return; }
      if (t === 'cfg.get') { resolve({ ok: true, data: mockConfig() }); return; }
      if (t === 'cfg.write') {
        var muts = payload.mutations || [];
        var errs = [];
        muts.forEach(function (m) { if (!pathTier(m.path)) errs.push(m.path); });
        muts.forEach(function (m) {
          if (m.path === 'audio_haptics.enabled') mock.audio.enabled = !!m.value;
          else if (m.path === 'vibration.enabled') mock.vibration.enabled = !!m.value;
          else if (m.path === 'process_whitelist') mock.counts.whitelist = (m.value || []).length;
          else if (m.path === 'vibration.rank_level_gain') mock.vibration.rank_level_gain = m.value;
          else if (m.path === 'vibration.rank_duration') mock.vibration.rank_duration = m.value;
        });
        emitSnap();
        resolve({ ok: errs.length === 0, data: { applied: muts.length - errs.length, errors: errs } });
        return;
      }
      if (t === 'mods.list') { resolve({ ok: true, data: MOCK_MODS }); return; }
      if (t === 'shell.apply' || t === 'shell.stop') {
        var target = payload.modId === 'classic' ? 'index.html' : 'endfield.html';
        /* 纯 mock 由页面原逻辑跳转；仅调试模式由桥代跳（冒烟模式不跳） */
        if (DBG && !W.__BRIDGE_SMOKE__) setTimeout(function () { location.href = target; }, 300);
        resolve({ ok: true, data: { target: target } });
        return;
      }
      if (t === 'capture.start') { mockCapture(payload.kind === 'trigger' ? 'trigger' : 'target'); resolve({ ok: true }); return; }
      if (t === 'capture.cancel') {
        mock.input.raw_capture_active = false; emitSnap(); resolve({ ok: true }); return;
      }
      resolve({ ok: true });
    });
  }

  /* ================= SorahkHost ================= */
  var msgId = 0;
  var pending = {};

  var host = {
    isReal: REAL,
    binding: BIND,
    debug: DBG,
    shell: (W.__SHELL__ && W.__SHELL__.shell) || (location.href.indexOf('endfield') >= 0 ? 'mod' : 'classic'),
    /* 注册回调：真桥由 __hostPush 驱动；mock 由 pushSnapshot/pushEvent 驱动 */
    onSnapshot: function (cb) { snapCbs.push(cb); if (BIND) { try { cb(snapCopy()); } catch (e) {} } },
    onEvent: function (cb) { evtCbs.push(cb); },
    /* v1.3 窗口镶边安装器（契约 v1.1/v1.3 无边框窗口; 双壳通用、幂等）。
       页面 hello 到达时委托调用即可；真桥模式下 __hostPush(hello) 也会自动安装
       （proto.js 等不消费 hello 的页面无需任何改动即获得拖动/缩放）。 */
    setupWindowChrome: function (hello) { installWindowChrome(hello); },
    /* 页面 → 桥：mock 模式下页面把本地状态机适配成快照推送器（域级合并后广播） */
    pushSnapshot: function (snap) {
      if (REAL) return; /* 真桥下宿主是唯一事实源，页面不推 */
      if (DBG) { mergeSnap(snap); emitSnap(); return; }
      /* 纯 mock：页面快照直接替换合并进桥状态后广播（保证渲染同一条路径） */
      mergeSnap(snap); emitSnap();
    },
    pushEvent: function (ev) {
      if (REAL) return;
      /* ★v1.5: DBG 模式注入 hello 时同步 __SHELL_INFO__（与真桥 __hostPush(hello)
         同一效果）—— 页面「典/简/完整模式」按钮的 hybrid 门控读它（冒烟断言依赖） */
      if (DBG && ev && ev.__hello) { try { W.__SHELL_INFO__ = ev.__hello; } catch (e) {} }
      emitEvent(ev);
    },
    /* 请求-应答 */
    call: function (payload) {
      if (DBG && W.__CALLS__) W.__CALLS__.push(JSON.parse(JSON.stringify(payload)));
      if (REAL) {
        var id = ++msgId;
        return new Promise(function (resolve, reject) {
          pending[id] = { resolve: resolve, reject: reject };
          try { W.wp.postMessage(JSON.stringify(Object.assign({ id: id }, payload))); }
          catch (e) { delete pending[id]; reject(e); }
        });
      }
      return mockCall(payload);
    },
    /* 快捷方式 */
    cmd: function (act, params) {
      var p = { t: 'cmd', act: act };
      if (params !== undefined) p.params = params;
      return host.call(p);
    },
    cfg: {
      _table: null,
      paths: function () {
        var self = this;
        return host.call({ t: 'cfg.paths' }).then(function (r) {
          if (r && r.data) self._table = r.data;
          return r && r.data ? r.data : {};
        }).catch(function () { return {}; });
      },
      tier: function (path) {
        if (this._table) {
          if (Object.prototype.hasOwnProperty.call(this._table, path)) return this._table[path];
          var segs = path.split('.');
          for (var k in this._table) {
            if (k.indexOf('.*') !== k.length - 2) continue;
            var pre = k.slice(0, -1);
            if (path.indexOf(pre) === 0 && path.slice(pre.length).indexOf('.') < 0) return this._table[k];
          }
          return null;
        }
        return pathTier(path); /* 表未取到时兜底（mock 内置表；真桥不会走到） */
      },
      get: function () { return host.call({ t: 'cfg.get', sections: ['*'] }); },
      write: function (mutations, commit) {
        if (!mutations || !mutations.length) return Promise.resolve({ ok: true });
        return host.call({ t: 'cfg.write', commit: commit || 'config-only', mutations: mutations });
      },
    },
    mods: {
      /* ★K 轮 (契约 v1.8): list() 返回前统一规范化 previewUrl —— extmod 包的
         previewUrl 是绝对 sorahk://extmod/<id>/… URL，而 WebView2 页内对原生 scheme
         的请求不被 wry workaround 拦截（实测挂起，H 轮遗留「外部包预览图不显示」根因）。
         必须改写成 http://sorahk.<authority>/… 形态才走同一拦截（wry 注册的过滤器 =
         "http://sorahk.*"，任意 sorahk.<host> 子域都会被还原成 sorahk:// 喂给协议
         handler）。内置包的相对 previewUrl（preview.png）原样保留。 */
      list: function () {
        return host.call({ t: 'mods.list' }).then(function (r) {
          if (r && r.data) {
            for (var i = 0; i < r.data.length; i++) {
              var m = r.data[i];
              if (m && typeof m.previewUrl === 'string' &&
                  m.previewUrl.indexOf('sorahk://') === 0) {
                m.previewUrl = 'http://sorahk.' + m.previewUrl.slice(9);
              }
            }
          }
          return r;
        });
      },
      /* ★K 轮: 导入 .dvmod 单文件格式包 —— 壳侧拉原生文件对话框选文件（文件字节
         不过 IPC，大文件不炸通道），壳读盘做安全校验/解包安装后回执。
         reply.data = {cancelled:true} | {cancelled:false, report:{id,name,version,
         replaced,files,bytes,previewUrl}}；ok=false 时 message = 失败原因。 */
      importDialog: function () { return host.call({ t: 'mods.import-dialog' }); },
    },
    capture: {
      start: function (kind) { return host.call({ t: 'capture.start', kind: kind }); },
      cancel: function () { return host.call({ t: 'capture.cancel' }); },
    },
    shell: {
      apply: function (modId) { return host.call({ t: 'shell.apply', modId: modId }); },
      stop: function () { return host.call({ t: 'shell.stop' }); },
    },
  };
  W.SorahkHost = host;

  /* ================= 窗口镶边（契约 v1.1/v1.3/v1.5 无边框窗口; 双壳通用） =================
     hello.dragMode=js（v1.3 默认）:
       ① 顶栏空白区左键 mousedown → win.drag（高频免 id）; 双击 → win.maximize;
       ② 四边+四角 8 个 position:fixed 12px 热区 → win.resize-begin{direction}
         （tao drag_resize_window 八方向; ★v1.5 热区 8px→12px —— 用户实测 8px
          太挑剔; 细条只在最外 12px, z-index 高但不遮交互
          —— 页面可交互元素都在内侧; cursor 按方向 n/e/s/w/ne/nw/se/sw）。
     hello.dragMode=nc（旧壳兜底）: 注入 app-region:drag CSS（WebView2 原生处理）。
     browser/file:// 下 hello 不到达 → 两者都不启用（原型演示行为不变）。 */
  function installWindowChrome(hello) {
    if (!hello || !hello.frameless || W.__SORAHK_CHROME__) return;
    var mode = hello.dragMode === 'nc' ? 'nc' : 'js';
    W.__SORAHK_CHROME__ = mode;
    function hostCall(msg) { try { W.SorahkHost.call(msg); } catch (e) {} }
    function inCtrl(e) {
      return !!(e.target && e.target.closest &&
        e.target.closest('[data-act],select,input,button,a,textarea,[contenteditable]'));
    }
    if (mode === 'nc') {
      /* 旧壳兜底: 顶栏 = 拖动区; 按钮/下拉/输入框/链接等交互子元素必须 no-drop */
      var st = document.createElement('style');
      st.id = 'win-drag-nc';
      st.textContent = '.topbar{app-region:drag;-webkit-app-region:drag}' +
        '.topbar button,.topbar select,.topbar input,.topbar a,.topbar [data-act]{app-region:no-drop;-webkit-app-region:no-drop}';
      document.head.appendChild(st);
      return;
    }
    var tb = document.querySelector('.topbar');
    if (tb) {
      tb.addEventListener('mousedown', function (e) {
        if (e.button !== 0 || inCtrl(e)) return;
        e.preventDefault(); /* 防顶栏文字选中，等价原生标题栏 */
        hostCall({ t: 'win.drag' });
      });
      tb.addEventListener('dblclick', function (e) {
        if (inCtrl(e)) return;
        hostCall({ t: 'win.maximize' });
      });
    }
    /* 12px 边缘/角热区 (★v1.5, 8px→12px): [方向, cursor, 定位]。
       四边细条让位四角（left/right 12px 内缩），角区 12×12;
       z-index 拉满但不遮交互（页面可点元素距边 > 12px）。 */
    var HZ = 12;
    var DIRS = [
      ['n',  'n-resize',    'top:0;left:' + HZ + 'px;right:' + HZ + 'px;height:' + HZ + 'px'],
      ['s',  's-resize',    'bottom:0;left:' + HZ + 'px;right:' + HZ + 'px;height:' + HZ + 'px'],
      ['w',  'w-resize',    'left:0;top:' + HZ + 'px;bottom:' + HZ + 'px;width:' + HZ + 'px'],
      ['e',  'e-resize',    'right:0;top:' + HZ + 'px;bottom:' + HZ + 'px;width:' + HZ + 'px'],
      ['nw', 'nwse-resize', 'top:0;left:0;width:' + HZ + 'px;height:' + HZ + 'px'],
      ['ne', 'nesw-resize', 'top:0;right:0;width:' + HZ + 'px;height:' + HZ + 'px'],
      ['sw', 'nesw-resize', 'bottom:0;left:0;width:' + HZ + 'px;height:' + HZ + 'px'],
      ['se', 'nwse-resize', 'bottom:0;right:0;width:' + HZ + 'px;height:' + HZ + 'px'],
    ];
    var layer = document.createElement('div');
    layer.id = 'sorahk-resize-hots';
    for (var i = 0; i < DIRS.length; i++) {
      var z = document.createElement('div');
      z.setAttribute('data-resize-dir', DIRS[i][0]);
      z.style.cssText = 'position:fixed;z-index:2147483000;cursor:' + DIRS[i][1] + ';' + DIRS[i][2];
      z.addEventListener('mousedown', function (e) {
        if (e.button !== 0) return;
        e.preventDefault();
        var d = e.currentTarget && e.currentTarget.getAttribute('data-resize-dir');
        if (d) hostCall({ t: 'win.resize-begin', direction: d });
      });
      layer.appendChild(z);
    }
    (document.body || document.documentElement).appendChild(layer);
  }

  /* ================= 真桥：宿主推送入口 ================= */
  if (REAL) {
    W.__hostPush = function (jsonStr) {
      var msg = null;
      /* ★v1.2 传输修复: 兼容宿主以 JS 对象字面量直调 (旧壳) 或 JSON 字符串
         (新壳) —— 两种实参都收; 字符串 parse 失败仍静默丢弃。 */
      if (jsonStr && typeof jsonStr === 'object') { msg = jsonStr; }
      else { try { msg = JSON.parse(jsonStr); } catch (e) { return; } }
      if (!msg || !msg.t) return;
      if (msg.t === 'snap') {
        /* v1.3 差量: full 替换 / patch 浅合并进 lastSnap 缓存, rAF 统一 emit */
        applyHostSnap(msg);
      } else if (msg.t === 'evt') {
        if (msg.ev) emitEvent(msg.ev);
      } else if (msg.t === 'hello') {
        W.__SHELL_INFO__ = msg;
        /* v1.3: 镶边全局安装（双壳通用; proto.js 无需改动即获得拖动/缩放） */
        try { installWindowChrome(msg); } catch (e) {}
        emitEvent({ __hello: msg });
      } else if (msg.t === 'reply') {
        var p = pending[msg.id];
        if (p) { delete pending[msg.id]; if (msg.ok) p.resolve(msg); else p.reject(new Error(msg.message || 'reply error')); }
      }
    };
    /* 本文件就绪较晚时宿主可能已推过 hello —— 通知宿主补发（宿主可忽略） */
    try {
      msgId++;
      W.wp.postMessage(JSON.stringify({ id: msgId, t: 'bridge.ready' }));
    } catch (e) {}
  }

  /* ================= 调试模式：内置 mock 快照序列 ================= */
  if (DBG) {
    W.__injectSnap = function (obj) { mergeSnap(obj); emitSnap(); };
    (function schedule() {
      function at(ms, fn) { setTimeout(fn, ms); }
      at(80, emitSnap); /* 基线一拍，先让 applier 跑起来 */
      at(380, function () {
        mergeSnap({ core: { paused: true, config_epoch: 7, stopped_threads: ['audio_engine'] } });
        emitSnap();
      });
      at(680, function () {
        mergeSnap({
          vibration: {
            enabled: true, out_l: 43690, out_r: 21845,
            test_until_ms: Date.now() + 3000, now_ms: Date.now(),
            rank_level_gain: 88, rank_duration: 777,
            item_lr: (function () { var a = fill(52, 0); a[0] = 33; a[1] = -44; return a; })(),
            rank_lr: (function () { var a = fill(60, 0); a[0] = 21; return a; })(),
            rank_type_gain: [95, 80, 70, 60, 50, 40, 30, 20, 10, 5, 0, 0, 0, 0, 0],
          },
          audio: {
            enabled: true, status: '运行中', motor_l: 33025, motor_r: 15000,
            bands: [0.9, 0.72, 0.5, 0.3, 0.12], band_peak: [0.9, 0.72, 0.5, 0.3, 0.12],
            mode: 1, noise_floor: 0.12, transient: 0.412, threshold_eff: 0.33, calib_result: 0,
          },
          counts: { mappings: 19, turbo: 6, combos: 5, presets: 3, whitelist: 3, macros: 2 },
        });
        emitSnap();
      });
      at(980, function () {
        mergeSnap({
          update: { phase: 'done', latest_version: 'R2.99', error: null },
          input: { live_xinput: [514, 4352] /* 0x045E, A|LB */ },
        });
        emitSnap();
      });
      at(1280, function () { mergeSnap({ version: 'R2.14-SMOKE' }); emitSnap(); });
      at(1500, function () {
        /* 调试：把桥内 mock 状态落到 DOM（--dump-dom 可查），定位序列 vs 渲染 */
        var pre = document.createElement('pre');
        pre.id = 'bridge-mock-state';
        pre.textContent = JSON.stringify({ version: mock.version, core: mock.core, out_l: mock.vibration.out_l, counts: mock.counts });
        document.body.appendChild(pre);
      });
    })();
  }

  /* ================= 冒烟子模式（结果落 #smoke-calls，--dump-dom 断言） ================= */
  function smokeSeed() {
    var wi = document.querySelector('#wl-input'); if (wi) wi.value = 'smoke.exe';
    var swi = document.querySelector('#set-wl-input'); if (swi) swi.value = 'smoke2.exe';
    var pi = document.querySelector('#wl-input, #set-wl-input');
  }
  function smokeClick(el) {
    if (el.tagName === 'SELECT') { el.dispatchEvent(new Event('change', { bubbles: true })); return; }
    el.dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true }));
  }
  function smokeOut() {
    var pre = document.createElement('pre');
    pre.id = 'smoke-calls';
    pre.textContent = JSON.stringify(W.__CALLS__ || []);
    document.body.appendChild(pre);
    document.title = 'SMOKE-DONE';
  }
  function runSmokeCalls() {
    smokeSeed();
    /* 预热：选一个手柄槽位（replica 渲染三步捕获面板 / proto 进编辑流）；
       endfield 的 set-route change 触发 SwitchVibEdition */
    var hot = document.querySelector('#pad-art g.hot') || document.querySelector('#pad-hot .hot');
    if (hot) { try { smokeClick(hot); } catch (e) {} }
    /* endfield 键盘键是 SVG g.kb-key（无 data-act，走 .kb-key 委托）——预点几个键供 kb-apply 批量出口 */
    var kbkeys = document.querySelectorAll('#kb-board .kb-key');
    for (var ki = 0; ki < kbkeys.length && ki < 6; ki++) { try { smokeClick(kbkeys[ki]); } catch (e) {} }
    var sr = document.getElementById('set-route');
    if (sr) { try { sr.dispatchEvent(new Event('change', { bubbles: true })); } catch (e) {} }
    /* 关键：页面点击会重渲染容器（innerHTML），一次性元素快照会脱离 DOM 导致事件
       到不了委托层 —— 因此每轮重新 querySelector，按 act 限额点击（每 act ≤3 次）。 */
    var clickedActs = {};
    var MAX_PER_ACT = 3;
    var guard = 0;
    (function step() {
      var els = document.querySelectorAll('[data-act]');
      var target = null, act = null;
      for (var i = 0; i < els.length; i++) {
        var a = els[i].getAttribute('data-act');
        if ((clickedActs[a] || 0) < MAX_PER_ACT) { target = els[i]; act = a; break; }
      }
      if (!target || guard++ > 600) { setTimeout(smokeOut, 450); return; }
      clickedActs[act] = (clickedActs[act] || 0) + 1;
      try { smokeClick(target); W.__CALLS__.push({ t: '__clicked', act: act }); }
      catch (e) { W.__CALLS__.push({ t: '__err', act: act, msg: String(e && e.message || e) }); }
      setTimeout(step, 6); /* 留出重渲染 / DBG RawCaptured(500ms) 回填等异步 */
    })();
  }
  function runSmokeDebounce() {
    var sel = 'input[type="range"][data-cfg-path="vibration.rank_level_gain"]';
    var el = document.querySelector(sel) ||
             document.querySelector('input[type="range"][data-cfg-path^="vibration.rank_type_gain."]') ||
             document.querySelector('input[type="range"][data-cfg-path^="vibration.item_lr."]');
    if (!el) { W.__CALLS__.push({ t: '__err', act: 'debounce', msg: 'no slider with data-cfg-path' }); smokeOut(); return; }
    W.__CALLS__.push({ t: '__slider', path: el.getAttribute('data-cfg-path') });
    for (var i = 0; i < 6; i++) {
      el.value = String(40 + i * 5);
      el.dispatchEvent(new Event('input', { bubbles: true }));
    }
    setTimeout(function () {
      el.dispatchEvent(new Event('change', { bubbles: true }));
      setTimeout(smokeOut, 700);
    }, 350);
  }
  function runSmokeNav() {
    /* v13 导航点击断言：replica .nav-item[data-pg] 七页逐个点；proto a[data-route] 同构记录。
       每次点击后记录 is-active / page.is-active / 顶栏页名（__CALLS__ → verify.js 断言） */
    var navBtns = Array.prototype.slice.call(document.querySelectorAll('.nav-item[data-pg]'));
    var routeAs = navBtns.length ? [] : Array.prototype.slice.call(document.querySelectorAll('.side nav a[data-route]'));
    var src = navBtns.length ? navBtns : routeAs;
    var key = navBtns.length ? 'pg' : 'route';
    if (!src.length) { W.__CALLS__.push({ t: '__nav-none' }); setTimeout(smokeOut, 250); return; }
    var i = 0;
    (function step() {
      if (i >= src.length) { setTimeout(smokeOut, 300); return; }
      var el = src[i++];
      try { smokeClick(el); } catch (e) { W.__CALLS__.push({ t: '__err', act: 'nav', msg: String(e && e.message || e) }); }
      setTimeout(function () {
        var rec = { t: '__nav', key: key, val: el.getAttribute('data-' + key) };
        var activeNav = document.querySelector('.nav-item.is-active[data-pg]');
        if (activeNav) rec.active = activeNav.getAttribute('data-pg');
        if (el.getAttribute('aria-current')) rec.ariaCurrent = el.getAttribute('aria-current');
        var page = document.querySelector('.page.is-active');
        if (page) rec.pageActive = page.getAttribute('data-pg') || page.getAttribute('data-page');
        var tb = document.getElementById('tb-page');
        if (tb) rec.tb = tb.textContent;
        var top = document.getElementById('topbar-page');
        if (top) rec.top = top.textContent;
        W.__CALLS__.push(rec);
        step();
      }, 40);
    })();
  }
  function runSmokePatch() {
    /* v1.3 差量快照管线断言（DBG 直驱真桥 applyHostSnap, 不依赖宿主）：
       full 帧 → 整体替换缓存（六域齐备）; patch 帧 → Object.assign 按域浅合并
       （泵侧 patch 域值 = 该域完整序列化对象, 域内字段随域整体更新;
        未变化域必须原样保留）。full 记录在 patch 应用前采样。 */
    try {
      var full = { t: 'snap', full: true, data: {
        version: 'R2.14-PATCH',
        core: { paused: false, config_epoch: 3 },
        input: { live_hid_pad: null, raw_capture_active: false },
        vibration: { enabled: true, out_l: 0, params: [1, 2, 3], rank_level_gain: 70 },
        audio: { enabled: false, motor_l: 0 },
        update: { phase: 'idle', done: 0 },
        counts: { mappings: 3, turbo: 0, whitelist: 1 },
      } };
      /* patch 域值 = 完整域对象（照泵实现: parts[i] 为该域全量序列化） */
      var patch = { t: 'snap', patch: {
        vibration: { enabled: true, out_l: 43690, params: [1, 2, 3], rank_level_gain: 70 },
        counts: { mappings: 9, turbo: 0, whitelist: 1 },
      } };
      var fullSnap = applyHostSnap(full);
      var fullRec = { t: '__patch-full', version: fullSnap.version,
        domains: ['core', 'input', 'vibration', 'audio', 'update', 'counts']
          .filter(function (k) { return fullSnap[k] && typeof fullSnap[k] === 'object'; }),
        out_l: fullSnap.vibration.out_l, mappings: fullSnap.counts.mappings,
        params: JSON.stringify(fullSnap.vibration.params) };
      var merged = applyHostSnap(patch);
      var mergedRec = { t: '__patch-merged', version: merged.version,
        out_l: merged.vibration.out_l,
        params_kept: JSON.stringify(merged.vibration.params),
        rank_kept: merged.vibration.rank_level_gain,
        mappings: merged.counts.mappings, turbo_kept: merged.counts.turbo,
        epoch_kept: merged.core.config_epoch, phase_kept: merged.update.phase,
        domains_after: ['core', 'input', 'vibration', 'audio', 'update', 'counts']
          .filter(function (k) { return merged[k] && typeof merged[k] === 'object'; }).length };
      var pre = document.createElement('pre');
      pre.id = 'smoke-calls';
      pre.textContent = JSON.stringify([fullRec, mergedRec]);
      document.body.appendChild(pre);
      document.title = 'SMOKE-DONE';
    } catch (e) {
      W.__CALLS__.push({ t: '__err', act: 'patch', msg: String(e && e.message || e) });
      smokeOut();
    }
  }
  /* v1.5 冒烟辅助: 量 SE 角热区的实际边长 (data-resize-dir 元素 getBoundingClientRect) */
  function zoneSizePx() {
    var z = document.querySelector('[data-resize-dir="se"]');
    if (!z) return 0;
    var r = z.getBoundingClientRect();
    return Math.round(Math.min(r.width, r.height));
  }
  /* v1.5 冒烟辅助: 触发 endfield 顶栏「完整/极简/经典模式」radio 的 change
     （name="mode"; 单选回显 + hybrid 门控的 shell.stop{mode} 出口都走 change） */
  function fireModeRadio(value) {
    var r = document.querySelector('input[name="mode"][value="' + value + '"]');
    if (!r) { W.__CALLS__.push({ t: '__err', act: 'mode-radio', msg: 'input[name=mode][value=' + value + '] missing' }); return; }
    r.checked = true;
    r.dispatchEvent(new Event('change', { bubbles: true }));
  }
  function runSmokeWin() {
    /* v13 窗口控制断言（replica 独有控件；endfield 无 win-* → __win-skip）：
       ① win-min/win-max 载荷 ② hello(dragMode=js) 注入 → mousedown 拖动区 → win.drag
       ③ 双击拖动区 → win.maximize ④ win-close → 弹窗 → 退出 → cmd quit
       ⑤ ShellCloseRequested 注入 → 弹窗 ⑥ 托盘 → win.hide
       ⑦ v1.2 win.close-opened 回执：win-close 与 ShellCloseRequested 路径各发一次；
          再次打开弹窗点「取消」→ 关弹窗且不再发回执（撤销强退后无额外消息）
       ⑧ v1.3 镶边载荷：hello 注入后 12px 热区 ×8 安装 + SE 角 mousedown →
          win.resize-begin{direction:"se"}（index 走 replica 委托路径；
          endfield 走桥安装器直调路径 → 顶栏 mousedown → win.drag 也一并断言）
       ⑨ v1.5 set-mode 真命令路径（混合 mod 窗）：hello 注入带 hybrid:true →
          「典/简」按钮（index）/ 模式 radio（endfield）→ shell.stop{mode} 载荷 */
    var top = document.querySelector('.topbar');
    var btnMin = document.querySelector('[data-act="win-min"]');
    if (!top || !btnMin) {
      /* endfield（proto 壳）: 桥安装器全局可用 —— 直调安装后断言镶边载荷
         （本文件内用 host 引用; 页面脚本的 H 是各自 IIFE 私有, 此处不可见） */
      try {
        host.pushEvent({ __hello: { version: 'R2.14-SMOKE', shell: 'mod', modId: 'endfield', frameless: true, dragMode: 'js', hybrid: true } });
        host.setupWindowChrome({ version: 'R2.14-SMOKE', shell: 'mod', frameless: true, dragMode: 'js', hybrid: true });
        W.__CALLS__.push({ t: '__chrome-zones', count: document.querySelectorAll('[data-resize-dir]').length, size: zoneSizePx() });
        var seZ = document.querySelector('[data-resize-dir="se"]');
        if (seZ) seZ.dispatchEvent(new MouseEvent('mousedown', { bubbles: true, cancelable: true, button: 0, clientX: W.innerWidth - 2, clientY: W.innerHeight - 2 }));
        else W.__CALLS__.push({ t: '__err', act: 'resize-zone', msg: 'se hot zone missing' });
        var tr = top ? top.getBoundingClientRect() : null;
        if (top && tr) top.dispatchEvent(new MouseEvent('mousedown', { bubbles: true, cancelable: true, button: 0, clientX: tr.left + 30, clientY: tr.top + 10 }));
        /* v1.5: 顶栏「极简/经典模式」radio（hybrid mod 窗）→ shell.stop{mode} */
        fireModeRadio('minimal');
        fireModeRadio('classic');
        /* ★J 轮 (v1.7): 1180 设计基准 + 黄条修复 + MOD 左侧入口探针
           —— fitStage 宽度定标 1180（任意窗宽 stage CSS 宽恒 1180）、
           .ef-btn::before z-index = -1（hover 黄 wipe 垫到文字之下）、
           左侧导航最后一项 = 外观 MOD（与原生 egui Page::TABS 同序） */
        try {
          var stg = document.getElementById('stage-root');
          var stf = stg ? getComputedStyle(stg) : null;
          var mtr = stf ? stf.transform.match(/matrix\(([^,]+),/) : null;
          var anyBtn = document.querySelector('.ef-btn');
          var wipeZ = anyBtn ? getComputedStyle(anyBtn, '::before').zIndex : 'no-btn';
          var navAs = document.querySelectorAll('.side nav a[data-route]');
          var lastNav = navAs.length ? navAs[navAs.length - 1] : null;
          W.__CALLS__.push({ t: '__stage-basis',
            cssW: stg ? stg.style.width : null,
            vw: W.innerWidth,
            scale: mtr ? Math.round(parseFloat(mtr[1]) * 10000) / 10000 : (stf && stf.transform === 'none' ? 1 : null),
            wipeZ: wipeZ,
            modsNavLast: !!(lastNav && lastNav.getAttribute('data-route') === 'mods'),
            pageMods: !!document.getElementById('page-mods'),
            modCardsDyn: !!document.getElementById('mod-cards-dyn') });
        } catch (e2) { W.__CALLS__.push({ t: '__err', act: 'stage-basis', msg: String(e2 && e2.message || e2) }); }
      } catch (e) { W.__CALLS__.push({ t: '__err', act: 'chrome', msg: String(e && e.message || e) }); }
      W.__CALLS__.push({ t: '__win-skip' });
      setTimeout(smokeOut, 450);
      return;
    }
    function dlgOpen() {
      var d = document.getElementById('dlg-exit');
      return !!(d && d.classList.contains('open'));
    }
    var steps = [
      function () { smokeClick(document.querySelector('[data-act="win-min"]')); },
      function () { smokeClick(document.querySelector('[data-act="win-max"]')); },
      function () {
        /* 注入 hello 帧（onEvent 同一路径）→ frameless + dragMode=js + hybrid 生效 */
        host.pushEvent({ __hello: { version: 'R2.14-SMOKE', shell: 'classic', frameless: true, dragMode: 'js', hybrid: true } });
        W.__CALLS__.push({ t: '__hello-injected' });
      },
      function () {
        var r = top.getBoundingClientRect();
        top.dispatchEvent(new MouseEvent('mousedown', { bubbles: true, cancelable: true, button: 0, clientX: r.left + r.width / 2, clientY: r.top + 10 }));
      },
      function () { top.dispatchEvent(new MouseEvent('dblclick', { bubbles: true, cancelable: true })); },
      function () {
        /* v1.3/v1.5 镶边断言: 12px 热区 ×8（replica 委托桥安装器安装）+ SE 角
           mousedown → win.resize-begin{direction:"se"} 载荷 + 热区尺寸记录 */
        W.__CALLS__.push({ t: '__chrome-zones', count: document.querySelectorAll('[data-resize-dir]').length, size: zoneSizePx() });
        var seZ = document.querySelector('[data-resize-dir="se"]');
        if (seZ) seZ.dispatchEvent(new MouseEvent('mousedown', { bubbles: true, cancelable: true, button: 0, clientX: W.innerWidth - 2, clientY: W.innerHeight - 2 }));
        else W.__CALLS__.push({ t: '__err', act: 'resize-zone', msg: 'se hot zone missing' });
      },
      function () { smokeClick(document.querySelector('[data-act="win-close"]')); },
      function () { W.__CALLS__.push({ t: '__dlg-exit', phase: 'after-win-close', open: dlgOpen() }); },
      function () { smokeClick(document.querySelector('#dlg-exit [data-act="exit-app"]')); },
      function () { W.__CALLS__.push({ t: '__dlg-exit', phase: 'after-exit-app', open: dlgOpen() }); },
      function () { host.pushEvent('ShellCloseRequested'); },
      function () { W.__CALLS__.push({ t: '__dlg-exit', phase: 'after-ShellCloseRequested', open: dlgOpen() }); },
      function () { smokeClick(document.querySelector('#dlg-exit [data-act="win-hide"]')); },
      function () { W.__CALLS__.push({ t: '__dlg-exit', phase: 'after-win-hide', open: dlgOpen() }); },
      function () { smokeClick(document.querySelector('[data-act="win-close"]')); },
      function () { smokeClick(document.querySelector('#dlg-exit [data-act="close-dlg"]')); },
      function () { W.__CALLS__.push({ t: '__dlg-exit', phase: 'after-cancel', open: dlgOpen() }); },
      /* v1.5: 「典/简」按钮（混合 mod 窗门控）→ shell.stop{mode} 载荷断言 */
      function () { smokeClick(document.querySelector('[data-act="minimal-mode"]')); },
      function () { smokeClick(document.querySelector('[data-act="classic-mode"]')); },
    ];
    var si = 0;
    (function run() {
      if (si >= steps.length) { setTimeout(smokeOut, 350); return; }
      try { steps[si](); } catch (e) { W.__CALLS__.push({ t: '__err', act: 'win-step' + si, msg: String(e && e.message || e) }); }
      si++;
      setTimeout(run, 70);
    })();
  }
  function runSmokePaths() {
    smokeSeed();
    /* 1) 白名单添加 → cfg.write process_whitelist (reload) */
    var add = document.querySelector('[data-act="wl-add"]');
    if (add) smokeClick(add);
    /* 2) 音频开关 → cfg.write audio_haptics.enabled */
    var at = document.querySelector('[data-act="audio-toggle"]');
    if (at) smokeClick(at);
    else {
      var sw = document.getElementById('sw-audio');
      if (sw) { sw.checked = !sw.checked; sw.dispatchEvent(new Event('change', { bubbles: true })); }
      else W.__CALLS__.push({ t: '__err', act: 'audio-toggle', msg: 'element not found' });
    }
    /* 3) DFO 开关 → cfg.write dfo_player */
    var dfo = document.getElementById('set-dfo');
    if (dfo) { dfo.checked = !dfo.checked; dfo.dispatchEvent(new Event('change', { bubbles: true })); }
    else {
      var g = document.querySelector('[data-act="guide-dfo-yes"]');
      if (g) smokeClick(g); else W.__CALLS__.push({ t: '__err', act: 'dfo', msg: 'element not found' });
    }
    /* 4) 语言 → SetLanguage */
    var lang = document.querySelector('input[name="lang"][value="en"]');
    if (lang) { lang.checked = true; lang.dispatchEvent(new Event('change', { bubbles: true })); }
    else {
      var sel2 = document.getElementById('set-language');
      if (sel2) { sel2.value = 'English'; sel2.dispatchEvent(new Event('change', { bubbles: true })); }
      else W.__CALLS__.push({ t: '__err', act: 'language', msg: 'element not found' });
    }
    /* 5) 震动滑杆一拖 → memory 写 + 族档位正式写（rank_duration 优先，按壳回退） */
    var r = document.querySelector('input[type="range"][data-cfg-path="vibration.rank_duration"]') ||
            document.querySelector('input[type="range"][data-cfg-path="vibration.rank_type_gain.0"]') ||
            document.querySelector('input[type="range"][data-cfg-path="vibration.item_lr.0"]');
    if (r) {
      W.__CALLS__.push({ t: '__slider', path: r.getAttribute('data-cfg-path') });
      r.value = '640';
      r.dispatchEvent(new Event('input', { bubbles: true }));
      r.dispatchEvent(new Event('change', { bubbles: true }));
    } else W.__CALLS__.push({ t: '__err', act: 'slider', msg: 'no cfg slider found' });
    /* 6) ★I 轮接线: 音频 C 滑条一拖 → memory 去抖 + settle 档正式写（audio_haptics.c_upgrade_pct） */
    var cr = document.querySelector('input[type="range"][data-cfg-path="audio_haptics.c_upgrade_pct"]');
    if (cr) {
      W.__CALLS__.push({ t: '__cslider', path: cr.getAttribute('data-cfg-path') });
      cr.value = '35';
      cr.dispatchEvent(new Event('input', { bubbles: true }));
      cr.dispatchEvent(new Event('change', { bubbles: true }));
    } else W.__CALLS__.push({ t: '__err', act: 'c-slider', msg: 'c_upgrade_pct slider not found' });
    setTimeout(smokeOut, 800);
  }
  if (DBG && SMOKE) {
    /* 等页面绑定层就绪（__BIND_READY__ 由页面脚本设置）再跑 */
    var tries = 0;
    (function waitReady() {
      tries++;
      if (W.__BIND_READY__ || tries > 40) {
        setTimeout(function () {
          if (SMOKE === 'calls') runSmokeCalls();
          else if (SMOKE === 'debounce') runSmokeDebounce();
          else if (SMOKE === 'paths') runSmokePaths();
          else if (SMOKE === 'nav') runSmokeNav();
          else if (SMOKE === 'win') runSmokeWin();
          else if (SMOKE === 'patch') runSmokePatch();
          else smokeOut();
        }, 120);
      } else setTimeout(waitReady, 25);
    })();
  }
})();
