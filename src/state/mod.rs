//! Application state management.
//!
//! Provides centralized state management for the key remapping application,
//! including configuration, keyboard event handling, and process filtering.

use std::collections::HashMap;
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, OnceLock, RwLock};
use std::time::Instant;

use smallvec::SmallVec;

use crate::util::{likely, unlikely};
use crate::util;

use windows::Win32::UI::WindowsAndMessaging::*;

use crate::config::AppConfig;
use crate::i18n::Language;

// ─── 注入安全保险(全局, 模块级) ─────────────────────────────────────────────
// 防止连发/映射失控时键盘鼠标被“劫持”、系统几乎无法操作。
// 滑动窗口计数注入单元数: 超过上限(说明多映射+连发叠加失控)立即进入 1 秒冷却,
// 冷却期内所有注入被丢弃, 让系统输入恢复; 冷却结束自动继续, 不影响正常使用。
const INJECTION_WINDOW_MS: u64 = 1000;
const INJECTION_MAX_PER_WINDOW: u32 = 900; // ≤900 注入单元/秒(单连发 5ms≈200/s, 多键连发留余量, 仍能防风暴)
static INJECTION_WINDOW_START: AtomicU64 = AtomicU64::new(0);
static INJECTION_WINDOW_COUNT: AtomicU32 = AtomicU32::new(0);
static INJECTION_BLOCKED_UNTIL: AtomicU64 = AtomicU64::new(0);

/// 每次注入前调用; 返回 false 表示本次注入应被丢弃(处于冷却期)。
/// 当前纪元毫秒(注入保险与冷却查询共用)。
#[inline]
fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[inline]
pub(crate) fn injection_guard_ok() -> bool {
    let now = now_ms();
    if now < INJECTION_BLOCKED_UNTIL.load(Ordering::Relaxed) {
        return false;
    }

    let window_start = INJECTION_WINDOW_START.load(Ordering::Relaxed);
    if now - window_start >= INJECTION_WINDOW_MS {
        INJECTION_WINDOW_START.store(now, Ordering::Relaxed);
        INJECTION_WINDOW_COUNT.store(0, Ordering::Relaxed);
    }

    let count = INJECTION_WINDOW_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
    if unlikely(count > INJECTION_MAX_PER_WINDOW) {
        // 超限 → 进入冷却, 强制暂停注入, 避免淹没系统输入
        INJECTION_BLOCKED_UNTIL.store(now + INJECTION_WINDOW_MS, Ordering::Relaxed);
        INJECTION_WINDOW_COUNT.store(0, Ordering::Relaxed);
        return false;
    }
    true
}

/// 注入保险当前是否处于"冷却期"(只读查询, 不消耗计数)。
///
/// 连招场景的关键修复: 冷却期间必须整体"透传" ——
/// 注入被丢弃的同时, 真实按键也不再被拦截(should_block 放行),
/// 否则会出现"注入被停但按键仍被吞"的状态: 玩家狂按技能键+来回切键
/// 时所有按键被吞掉却没有任何输出, 产生"软件卡死"的错觉。
/// 冷却期间按下的键直接到达系统/游戏, 输入永远有效, 不可能卡死。
#[inline]
pub(crate) fn injection_cooldown_active() -> bool {
    let now = now_ms();
    now < INJECTION_BLOCKED_UNTIL.load(Ordering::Relaxed)
}

/// 输入法(如中文拼音)是否正处于"打字组合"状态。
///
/// 判断依据: 前台窗口输入上下文的组合串长度 > 0(正在拼拼音/选字)。
/// 与"输入法是否开启"不同 —— 开启但未打字(如 DNF 开着拼音打副本)不触发暂停,
/// 只有真的在打字时才暂停, 从而:
/// 1) 不拦截按键 → 拼音输入法能正常收到全部按键(输入法吞 KeyUp 卡死的根源被消除);
/// 2) 不注入模拟键 → 手柄映射不会把字母"打"进聊天框。
/// 组合按 Esc 清空后自动恢复映射, 无需用户操作。
pub(crate) fn is_ime_composing() -> bool {
    // 节流缓存: 注入热路径每次注入都调用本函数, IMM API 调用较慢 ——
    // 每 10ms 最多真正查询一次, 其余返回最近结果(打字状态是慢变量, 10ms 足够跟手)。
    const IME_CHECK_INTERVAL_MS: u64 = 10;
    static IME_CHECK_LAST_MS: AtomicU64 = AtomicU64::new(0);
    static IME_CHECK_LAST_RESULT: AtomicBool = AtomicBool::new(false);
    let now = now_ms();
    if now - IME_CHECK_LAST_MS.load(Ordering::Relaxed) < IME_CHECK_INTERVAL_MS {
        return IME_CHECK_LAST_RESULT.load(Ordering::Relaxed);
    }
    IME_CHECK_LAST_MS.store(now, Ordering::Relaxed);
    let r = is_ime_composing_impl();
    IME_CHECK_LAST_RESULT.store(r, Ordering::Relaxed);
    r
}

/// 真正的 IMM 查询(见 is_ime_composing 的节流)。
fn is_ime_composing_impl() -> bool {
    unsafe {
        use windows::Win32::UI::Input::Ime::{
            GCS_COMPSTR, ImmGetCompositionStringW, ImmGetContext, ImmReleaseContext,
        };
        let hwnd = GetForegroundWindow();
        if hwnd.0.is_null() {
            return false;
        }
        let himc = ImmGetContext(hwnd);
        if himc.0.is_null() {
            return false;
        }
        // 第一次查询(缓冲区为 None)返回组合串所需字节数; >0 = 正在组合打字
        let len = ImmGetCompositionStringW(himc, GCS_COMPSTR, None, 0);
        let _ = ImmReleaseContext(hwnd, himc);
        len > 0
    }
}

static GLOBAL_STATE: OnceLock<Arc<AppState>> = OnceLock::new();


// ── 按职责拆分的子模块 (2026-09-27 架构重构 B; 各文件头部注明原 state.rs 行号区间) ──
mod key_names;
mod mappings;
mod events;
mod whitelist;
mod inject;
mod turbo;
mod vib_mirror;
mod types;
pub use types::*;
#[cfg(test)]
mod tests;

/// Central application state manager.
///
/// Manages all runtime state including configuration, key mappings,
/// worker threads, and process filtering.
pub struct AppState {
    /// Current UI language
    language: AtomicU8,
    /// Tray icon visibility flag
    show_tray_icon: AtomicBool,
    /// Notification display flag
    show_notifications: AtomicBool,
    /// Switch key cache for fast combo key detection
    pub switch_key_cache: SwitchKeyCache,
    /// ★v21.7 预设切换键 (手柄类) 绑定表 —— 输入线程检测用
    preset_switch_bindings: RwLock<Vec<PresetSwitchBinding>>,
    /// ★v21.7 已触发的预设切换请求队列 (输入线程写, GUI 每帧取走并真正切换)
    pending_preset_switches: Mutex<Vec<String>>,
    /// ★v21.7 方案A: 配置中是否存在语义 HID 触发键 (RawInput 语义翻译热路径开关)
    semantic_hid_mappings: AtomicBool,
    /// ★v21.7b 正在"实时识别"的第三方手柄 (GUI 选中的 vid:pid; None = 不识别)
    live_hid_pad: RwLock<Option<(u16, u16)>>,
    /// ★v21.7b 该手柄的最新实时状态 (RawInput 写, GUI 读)
    live_hid_state: RwLock<Option<LiveHidState>>,
    /// ★v24.6: 该手柄的最新 XInput 实时输入位图 (XInput 线程写, GUI 读)。
    ///
    /// 供手柄页「按手柄键 → 选中槽位」使用, **不启用捕获模式** ——
    /// 捕获模式会跳过 `handle_normal_mode_xinput`(阈值注册/映射派发), 实机表现为
    /// 进游戏后摇杆方向与奔跑全部失效。
    live_xinput: RwLock<Option<(u16, u32)>>,
    /// Application exit flag
    pub should_exit: Arc<AtomicBool>,
    /// Key repeat pause state
    is_paused: AtomicBool,
    /// ★v22.7: 手柄校准向导进行中 —— 输入线程仍发布实时状态 (供方向步/SVG), 但**不派发**任何映射,
    /// 避免校对时按键被注入游戏、或被其它映射/流程干扰。
    gp_calibrating: AtomicBool,
    /// Window show request flag
    show_window_requested: AtomicBool,
    /// About dialog request flag
    show_about_requested: AtomicBool,
    /// Input timeout in milliseconds
    input_timeout: AtomicU64,
    /// ★v24.27 组合键内相邻两键的间隔 ms (模拟手速); 0 = 整组同按
    pub combo_key_gap_ms: AtomicU64,
    /// Active worker thread count
    worker_count: AtomicU64,
    /// Configured worker count for display
    configured_worker_count: usize,
    /// Input mapping configuration (keyboard + mouse)
    input_mappings: scc::HashMap<InputDevice, InputMappingInfo>,
    /// Worker pool for event processing
    worker_pool: OnceLock<Arc<dyn EventDispatcher>>,
    /// Notification event sender
    notification_sender: std::sync::Mutex<Option<Sender<NotificationEvent>>>,
    /// Process whitelist (empty means all processes enabled)
    process_whitelist: Mutex<Vec<String>>,
    /// 白名单总开关 (false = 全部放行, 列表保留)
    whitelist_enabled: std::sync::atomic::AtomicBool,
    /// ★S1 老方案总开关: true = 震动引擎走 S1 ACT1 老方案 (配老版 DLL 事件语义),
    /// false = S4+ 新方案 (现行)。设置里切换, 震动线程每轮读取, 即时生效。
    pub vib_legacy_client: std::sync::atomic::AtomicBool,
    /// Cached foreground process **full image path** with timestamp
    /// (★白名单路径条目按完整路径匹配, 文件名在匹配时从路径提取)
    cached_process_info: RwLock<(Option<String>, Instant)>,
    /// ★v24.31 映射配置修订号: reload_config 时 +1。worker 里"锁定"的注入键
    /// 在配置变更后强制解锁松开 (映射已改/已删, 再按着就永远悬空 = 卡键)。
    mappings_revision: std::sync::atomic::AtomicU64,
    /// ★v24.31 序列宏: 全局暂停 (KeySequenceToggle/Pause/Continue 切换)
    pub sequence_paused: std::sync::atomic::AtomicBool,
    /// ★v24.31 按设备登记的执行中序列 (同一设备上一条没跑完时忽略新触发)
    sequence_runs: std::sync::Mutex<HashMap<InputDevice, Arc<SequenceRun>>>,
    /// ★v24.31 按键录制 (序列宏"录制"功能; 钩子线程写入, GUI 取走转序列文本)
    key_record: std::sync::Mutex<Option<KeyRecordState>>,
    /// ★v24.31 录制中标志 (钩子热路径的廉价检查)
    pub key_record_active: std::sync::atomic::AtomicBool,
    /// Currently pressed keys for combo detection
    pressed_keys: scc::HashSet<u32>,
    /// Active combo triggers (multiple combos can be active simultaneously)
    /// Maps combo device to the set of modifier keys that were suppressed
    active_combo_triggers: scc::HashMap<InputDevice, SmallVec<[u32; 8]>>,
    /// Cached turbo state for keyboard keys (VK 0-255) for fast access
    cached_turbo_keyboard: [AtomicBool; 256],
    /// Cached turbo state for mouse buttons and key combos
    cached_turbo_other: scc::HashMap<InputDevice, bool>,
    /// Cached combo key index for efficient combo matching
    /// Maps main key to all combos that end with that key
    cached_combo_index: scc::HashMap<u32, Vec<InputDevice>>,
    /// Cached XInput combo index for subset matching
    /// Maps device_type to all combo button_ids for that device
    cached_xinput_combos: scc::HashMap<DeviceType, Vec<Vec<u32>>>,
    /// Raw Input capture event sender for GUI
    raw_input_capture_sender: Sender<InputDevice>,
    /// Raw Input capture event receiver for GUI
    raw_input_capture_receiver: Mutex<Receiver<InputDevice>>,
    /// Flag indicating GUI is in capture mode for Raw Input
    is_capturing_raw_input: AtomicBool,
    /// Raw Input capture mode strategy
    rawinput_capture_mode: RwLock<CaptureMode>,
    /// XInput capture mode strategy
    xinput_capture_mode: RwLock<crate::config::XInputCaptureMode>,
    /// HID device activation request sender
    hid_activation_sender: Sender<HidActivationRequest>,
    /// HID device activation request receiver
    hid_activation_receiver: Mutex<Receiver<HidActivationRequest>>,
    /// HID activation data sender (device_handle, data)
    hid_activation_data_sender: Sender<(isize, Vec<u8>)>,
    /// HID activation data receiver
    hid_activation_data_receiver: Mutex<Receiver<(isize, Vec<u8>)>>,
    /// Currently activating device handle
    activating_device_handle: std::sync::atomic::AtomicIsize,
    /// XInput cache invalidation flag
    xinput_cache_invalid: AtomicBool,
    /// Vibration master switch (runtime toggle)
    pub vibration_enabled: AtomicBool,
    /// Whether damage-font hit events are enabled (default off)
    pub vibration_font_hits: AtomicBool,
    /// Whether vibration shared memory (DfoVibration.dll) is connected
    pub vibration_connected: AtomicBool,
    /// Total events received from DfoVibration.dll (diagnostics)
    pub vibration_events_received: std::sync::atomic::AtomicU64,
    /// Total ranking events (评分等级事件, diagnostics)
    pub vibration_rank_events: std::sync::atomic::AtomicU64,
    /// Latest ranking level received (2~8, 0=none)
    pub vibration_rank_last: std::sync::atomic::AtomicU32,
    /// Set to trigger a simulated ranking-8 event (GUI test button)
    pub vibration_rank_test: std::sync::atomic::AtomicBool,
    /// 评分细分事件独立强度 %: [击杀点, 极限闪避, 暴击, 破招, 背击, 最终击杀, 凌空追击, 破甲, 增益叠加, 释放技能, 镜头震动, 移动, 技能震动, 暴击特写, 怪物死亡]
    pub vibration_rank_type_gain: [std::sync::atomic::AtomicU32; 15],
    /// 评分细分事件累计触发次数 (诊断, GUI 展示哪个事件触发了)
    pub vibration_rank_type_events: [std::sync::atomic::AtomicU64; 15],
    /// 评分等级震动强度 % (VEV_RANKING, 默认 100)
    pub vibration_rank_level_gain: std::sync::atomic::AtomicU32,
    /// 评分满幅脉冲时长 ms (默认 300)
    pub vibration_rank_duration: std::sync::atomic::AtomicU32,
    /// Test pulse until timestamp (GetTickCount based); 0 = none
    pub vibration_test_until: std::sync::atomic::AtomicU64,
    /// Advanced tuning master switch (false = base params only)
    pub vibration_advanced_enabled: AtomicBool,
    /// Left motor global gain % (motor zone)
    pub vibration_motor_l_gain: std::sync::atomic::AtomicU32,
    /// Right motor global gain % (motor zone)
    pub vibration_motor_r_gain: std::sync::atomic::AtomicU32,
    /// Random intensity variation % around cap (0 = off, N = ±N%)
    pub vibration_random_gain: std::sync::atomic::AtomicU32,
    /// Output smoothing % (v22.3: 一阶低通抑制低频嗡嗡声, 0=无, 高=柔)
    pub vibration_out_smooth: std::sync::atomic::AtomicU32,
    /// 震动节流窗口 ms (v24: 限定窗口内最大注入次数, 0=禁用)
    pub vibration_throttle_window: std::sync::atomic::AtomicU32,
    /// 节流窗口内最大注入次数
    pub vibration_throttle_max: std::sync::atomic::AtomicU32,
    /// 密度激活时节流次数比例 % (100=不收紧)
    pub vibration_throttle_dense_ratio: std::sync::atomic::AtomicU32,
    /// 群怪聚合·合并保留 % (v14.1, S1 高级算法; 0=关闭聚合)
    pub vibration_merge_keep: std::sync::atomic::AtomicU32,
    /// 群怪聚合·合并封顶 % (记账+本击强度上限, 100=满幅)
    pub vibration_merge_cap: std::sync::atomic::AtomicU32,
    /// 群怪聚合·补发窗口 ms (0=不补发)
    pub vibration_merge_hold: std::sync::atomic::AtomicU32,
    /// 命中限频·窗口内最多生效次数 (v14.2, 仅普通命中通道, 0=关闭)
    pub vibration_hitcap_max: std::sync::atomic::AtomicU32,
    /// 命中限频·窗口 ms
    pub vibration_hitcap_win: std::sync::atomic::AtomicU32,
    /// 命中聚合窗 ms (v15.2, 群怪一刀多条命中事件合并, 一刀一震)
    pub vibration_hitmerge_ms: std::sync::atomic::AtomicU32,
    /// 持续压制·触发秒数 (v15.3, 限频持续咬合后命中再降档, 0=关闭)
    pub vibration_sustain_secs: std::sync::atomic::AtomicU32,
    /// 持续压制·降幅 %
    pub vibration_sustain_reduce: std::sync::atomic::AtomicU32,
    /// 脉冲落地·阈值 % (v15, 高负载期尾巴归零判据, 0=关闭)
    pub vibration_tail_land_pct: std::sync::atomic::AtomicU32,
    /// 怪物异常反馈·强度 % (v15, 仅 S1; 0x04 出血/中毒跳字独立通道)
    pub vibration_monster_abnormal: std::sync::atomic::AtomicU32,
    /// 命中风暴·阈值 (v16, 仅 S1; 窗内纯命中事件达此数入风暴, 0=关闭)
    pub vibration_storm_thr: std::sync::atomic::AtomicU32,
    /// 命中风暴·保留比例 % (风暴期每 N 条命中保留 1 条)
    pub vibration_storm_keep_pct: std::sync::atomic::AtomicU32,
    /// 命中风暴·统计窗口 ms
    pub vibration_storm_win_ms: std::sync::atomic::AtomicU32,
    /// 命中风暴·恢复暂停 ms (停手此时长即恢复刀刀震)
    pub vibration_storm_pause_ms: std::sync::atomic::AtomicU32,
    /// 命中风暴·风暴期间静音怪物异常反馈 (v16.6, 仅 S1, 默认关)
    pub vibration_storm_mute_abnormal: std::sync::atomic::AtomicBool,
    /// 命中风暴·风暴期间静音评分点系统 (v16.7, 仅 S1, 默认关)
    pub vibration_storm_mute_rank: std::sync::atomic::AtomicBool,
    /* ★高级算法总开关 (v16.8): false = 该算法回到旧行为, 滑块值保留 */
    pub vibration_merge_enabled: std::sync::atomic::AtomicBool,
    pub vibration_hitcap_enabled: std::sync::atomic::AtomicBool,
    pub vibration_storm_enabled: std::sync::atomic::AtomicBool,
    pub vibration_sustain_enabled: std::sync::atomic::AtomicBool,
    pub vibration_tail_land_enabled: std::sync::atomic::AtomicBool,
    pub vibration_density_enabled: std::sync::atomic::AtomicBool,
    pub vibration_adapt_enabled: std::sync::atomic::AtomicBool,
    pub vibration_move_charge_enabled: std::sync::atomic::AtomicBool,
    pub vibration_decay_enabled: std::sync::atomic::AtomicBool,
    pub vibration_algo_windows_enabled: std::sync::atomic::AtomicBool,
    pub vibration_pulse_enabled: std::sync::atomic::AtomicBool,
    /// 统合衰减期 (v17, 仅 S1 风暴期, 默认关) + 固定衰减周期 ms
    pub vibration_storm_unified_enabled: std::sync::atomic::AtomicBool,
    pub vibration_storm_unified_ms: std::sync::atomic::AtomicU32,
    /// S1 输出引擎模式 (v20: 0 经典 / 1 S4 纯 / 2 S4+不丢; 仅 S1 生效)
    pub vibration_legacy_output_mode: std::sync::atomic::AtomicU32,
    /// 独立测试模式 (v24.2: 评分/移动通道独立于全局总调整)
    pub vibration_independent_test: std::sync::atomic::AtomicBool,
    /// 移动持续震动独立于全局强度 (v24.5: 默认开)
    pub vibration_move_independent: std::sync::atomic::AtomicBool,
    /// 绝对震动频率 (v26: 一切算法失效, 只在 时间-次数 内注入)
    pub vibration_abs_freq_enabled: std::sync::atomic::AtomicBool,
    /// 绝对频率窗口 ms
    pub vibration_abs_freq_window: std::sync::atomic::AtomicU32,
    /// 绝对频率窗口内最大注入次数
    pub vibration_abs_freq_max: std::sync::atomic::AtomicU32,
    /// 评分动态衰减 (类鬼泣, v29) 开关
    pub vibration_rank_decay_enabled: std::sync::atomic::AtomicBool,
    /// 停手多久开始衰减 ms
    pub vibration_rank_decay_delay: std::sync::atomic::AtomicU32,
    /// 衰减速度 级/秒
    pub vibration_rank_decay_speed: std::sync::atomic::AtomicU32,
    /// 最低反馈倍率 % (评级 0)
    pub vibration_rank_decay_min: std::sync::atomic::AtomicU32,
    /// 最高反馈倍率 % (评级 8)
    pub vibration_rank_decay_max: std::sync::atomic::AtomicU32,
    /// 输出低强度死区 % (v29.2: 低于归 0 消除马达嗡声)
    pub vibration_out_threshold: std::sync::atomic::AtomicU32,
    /// 输出动态范围重映射 (v30): 非零输出映射到 [min,100]
    pub vibration_remap_enabled: std::sync::atomic::AtomicBool,
    /// 重映射最小输出 %
    pub vibration_remap_min: std::sync::atomic::AtomicU32,
    /// 马达分工 (v30, Xbox360 风格)
    pub vibration_split_enabled: std::sync::atomic::AtomicBool,
    /// 分工分界 %
    pub vibration_split_thr: std::sync::atomic::AtomicU32,
    /// 职业专属高级算法 id (v31: 1-15)
    pub vibration_algo_id: std::sync::atomic::AtomicU32,
    /// 职业专属高级算法参数 [4]
    pub vibration_algo_ap: [std::sync::atomic::AtomicU32; 4],
    /// Per-base-item L/R motor weights % (each base feature has its own L/R)
    /// index = base item id (0=总闸,1=攻击频率,2=强度上限,3=衰减,4=连击,5=节奏,
    /// 6=通用飘字,7=DOT,8=特效,9=状态,10=特殊,11=命中,12=受击), value = [L, R]
    pub vibration_item_lr: [std::sync::atomic::AtomicU32; 26],
    /// Per-rank-event L/R motor weights %: 15 rank events x [L,R] = 30
    pub vibration_rank_lr: [std::sync::atomic::AtomicU32; 30],
    /// Real-time per-rank-event output levels (0-65535) for rank L/R bars
    pub vibration_rank_out: [std::sync::atomic::AtomicU32; 30],
    /// Real-time per-item output levels (0-65535) for item L/R bars
    pub vibration_item_out: [std::sync::atomic::AtomicU32; 26],
    /// Real-time output levels (0-65535) for motor zone UI
    pub vibration_out_l: std::sync::atomic::AtomicU32,
    pub vibration_out_r: std::sync::atomic::AtomicU32,
    /// Vibration params (runtime adjustable):
    /// [0]=attack_gain [1]=damage_gain [2]=shake_gain [3]=move_gain
    /// [4]=max_strength [5]=decay_ms [6]=hit_boost [7]=start_pulse [8]=kill_pulse
    /// [9]=master_gain [10]=font_strength [11]=font_interval_ms
    pub vibration_params: [AtomicU32; 60],
}

impl AppState {
    /// Creates a new application state from configuration.
    ///
    /// # Errors
    ///
    /// Returns an error if the toggle key name is invalid or key mappings cannot be created.
    pub fn new(mut config: AppConfig) -> anyhow::Result<Self> {
        /* ★启动参数兜底 (修"进震动页才有震动"): Config.toml 的 [vibration] 强度
         * 三闸 (attack/master/max) 在 GUI 里从不回写、长期为 0 —— 真实参数在
         * 内置默认预设与 JobVibration.toml, 此前只在震动页首帧的 INIT_DONE 块
         * 加载, 不进页面引擎拿全 0 → finalize 静音。此处镜像该初始化:
         * pristine → 内置默认预设; JobVibration applied → 职业参数覆盖。
         * 震动页首渲染的同名块保留 (此时已非 pristine, 自然跳过)。 */
        let mut job_algo_id: u32 = 0;
        let mut job_algo_ap: [u32; 4] = [0; 4];
        {
            let pristine =
                config.vibration.master_gain == 0 && config.vibration.attack_gain == 0;
            if pristine {
                if let Some(pr) = crate::config::default_vibration_presets().first() {
                    config.vibration.attack_gain = pr.params[0];
                    config.vibration.damage_gain = pr.params[1];
                    config.vibration.shake_gain = pr.params[2];
                    config.vibration.move_gain = pr.params[3];
                    config.vibration.max_strength = pr.params[4];
                    config.vibration.decay_ms = pr.params[5];
                    config.vibration.hit_boost = pr.params[6];
                    config.vibration.start_pulse = pr.params[7];
                    config.vibration.kill_pulse = pr.params[8];
                    config.vibration.master_gain = pr.params[9];
                    config.vibration.font_strength = pr.params[10];
                    config.vibration.font_interval = pr.params[11];
                    config.vibration.font_hp = pr.params[12];
                    config.vibration.font_special = pr.params[13];
                    config.vibration.font_state = pr.params[14];
                    config.vibration.font_effect = pr.params[15];
                    config.vibration.font_attack = pr.params[16];
                    config.vibration.font_hit = pr.params[17];
                    config.vibration.rhythm = pr.params[18];
                    config.vibration.advanced.copy_from_slice(&pr.params[19..60]);
                    config.vibration.item_lr = pr.item_lr;
                    config.vibration.rank_lr = pr.rank_lr;
                    config.vibration.rank_type_gain = pr.rank_type_gain;
                    config.vibration.rank_level_gain = pr.rank_level_gain;
                    config.vibration.rank_duration = pr.rank_duration;
                    config.vibration.out_smooth = pr.out_smooth;
                }
            }
            if let Some(jcfg) = crate::job_presets::load_job_vibration() {
                if jcfg.applied {
                    /* ★v24.15: 按**当前路线**判定 —— ACT 名册的职业在 S4 路线不存在,
                     * 不能因为 JobVibration.toml 记着 applied=true 就把 ACT 特调灌进
                     * S4 的启动参数 (会随保存写进 S4 的震动文件)。 */
                    if let Some((_bi, _ci, cls)) = crate::job_presets::find_class_for_route(
                        &jcfg.base_job,
                        &jcfg.class_name,
                        config.vib_legacy_client,
                    ) {
                        let params: Vec<u32> = if jcfg.params.len() == 60 {
                            jcfg.params.clone()
                        } else {
                            cls.params.to_vec()
                        };
                        /* 职业参数 (60 槽布局与 init_params 一致) → config.vibration */
                        config.vibration.attack_gain = params[0];
                        config.vibration.damage_gain = params[1];
                        config.vibration.shake_gain = params[2];
                        config.vibration.move_gain = params[3];
                        config.vibration.max_strength = params[4];
                        config.vibration.decay_ms = params[5];
                        config.vibration.hit_boost = params[6];
                        config.vibration.start_pulse = params[7];
                        config.vibration.kill_pulse = params[8];
                        config.vibration.master_gain = params[9];
                        config.vibration.font_strength = params[10];
                        config.vibration.font_interval = params[11];
                        config.vibration.font_hp = params[12];
                        config.vibration.font_special = params[13];
                        config.vibration.font_state = params[14];
                        config.vibration.font_effect = params[15];
                        config.vibration.font_attack = params[16];
                        config.vibration.font_hit = params[17];
                        config.vibration.rhythm = params[18];
                        config.vibration.advanced.copy_from_slice(&params[19..60]);
                        config.vibration.rank_lr = cls.rank_lr.map(|v| v as u32);
                        config.vibration.rank_type_gain = cls.rank_type_gain;
                        config.vibration.rank_level_gain = if jcfg.rank_level_gain > 0 {
                            jcfg.rank_level_gain
                        } else {
                            cls.rank_level_gain
                        };
                        config.vibration.rank_duration = if jcfg.rank_duration > 0 {
                            jcfg.rank_duration
                        } else {
                            cls.rank_duration
                        };
                        config.vibration.out_smooth = cls.out_smooth;
                        config.vibration.throttle_window_ms = cls.throttle_window;
                        config.vibration.throttle_max_hits = cls.throttle_max;
                        config.vibration.throttle_dense_ratio = cls.throttle_dense_ratio;
                        /* ★保险A: 启动兜底同样强制关绝对频率 (堵重启复活路径) */
                        config.vibration.abs_freq_enabled = false;
                        job_algo_id = cls.algo_id as u32;
                        job_algo_ap = cls.algo_params;
                    }
                }
            }
        }
        let switch_key_cache = SwitchKeyCache::new();
        Self::update_switch_key_cache(&switch_key_cache, &config.switch_key)?;

        let input_mappings_map = Self::create_input_mappings(&config)?;

        // Create lock-free concurrent HashMap
        let input_mappings = scc::HashMap::new();

        // Populate input_mappings
        for (k, v) in input_mappings_map {
            let _ = input_mappings.insert_sync(k, v);
        }

        // Initialize cached data structures
        let cached_turbo_keyboard: [AtomicBool; 256] =
            std::array::from_fn(|_| AtomicBool::new(true));
        let cached_turbo_other = scc::HashMap::new();
        let cached_combo_index: scc::HashMap<u32, Vec<InputDevice>> = scc::HashMap::new();
        let cached_xinput_combos: scc::HashMap<DeviceType, Vec<Vec<u32>>> = scc::HashMap::new();

        for mapping in config.mappings.iter() {
            if let Some(device) = Self::input_name_to_device(&mapping.trigger_key) {
                // Populate turbo cache and combo index
                match &device {
                    InputDevice::Keyboard(vk) if *vk < 256 => {
                        cached_turbo_keyboard[*vk as usize]
                            .store(mapping.turbo_enabled, Ordering::Relaxed);
                    }
                    InputDevice::KeyCombo(keys) => {
                        if let Some(&last_key) = keys.last() {
                            let mut combos = cached_combo_index
                                .get_sync(&last_key)
                                .map(|v| v.get().clone())
                                .unwrap_or_default();
                            combos.push(device.clone());
                            let _ = cached_combo_index.upsert_sync(last_key, combos);
                        }
                        let _ =
                            cached_turbo_other.insert_sync(device.clone(), mapping.turbo_enabled);
                    }
                    InputDevice::XInputCombo {
                        device_type,
                        button_ids,
                    } => {
                        let mut combos = cached_xinput_combos
                            .get_sync(device_type)
                            .map(|v| v.get().clone())
                            .unwrap_or_default();
                        combos.push(button_ids.clone());
                        let _ = cached_xinput_combos.upsert_sync(*device_type, combos);
                        let _ =
                            cached_turbo_other.insert_sync(device.clone(), mapping.turbo_enabled);
                    }
                    _ => {
                        let _ =
                            cached_turbo_other.insert_sync(device.clone(), mapping.turbo_enabled);
                    }
                }
            }
        }

        let (raw_input_capture_sender, raw_input_capture_receiver) = mpsc::channel();
        let (hid_activation_sender, hid_activation_receiver) = mpsc::channel();
        let (hid_activation_data_sender, hid_activation_data_receiver) = mpsc::channel();

        // params[60] 从 config.vibration 恢复 (索引映射与 GUI 的
        // sync_vib_config_from_params 互逆): 极简模式启动 / 未渲染震动页时,
        // 引擎也要吃到上一次保存的参数, 而不是全 0 (旧行为: 震动全程无声)。
        let init_params = Self::vibration_params_from(&config.vibration);
        Ok(Self {
            language: AtomicU8::new(config.language.to_u8()),
            show_tray_icon: AtomicBool::new(config.show_tray_icon),
            show_notifications: AtomicBool::new(config.show_notifications),
            switch_key_cache,
            preset_switch_bindings: RwLock::new(Vec::new()),
            pending_preset_switches: Mutex::new(Vec::new()),
            semantic_hid_mappings: AtomicBool::new(Self::config_has_semantic_mappings(&config)),
            live_hid_pad: RwLock::new(None),
            live_hid_state: RwLock::new(None),
            live_xinput: RwLock::new(None),
            should_exit: Arc::new(AtomicBool::new(false)),
            is_paused: AtomicBool::new(false),
            gp_calibrating: AtomicBool::new(false),
            show_window_requested: AtomicBool::new(false),
            show_about_requested: AtomicBool::new(false),
            input_timeout: AtomicU64::new(config.input_timeout.clamp(1, 2000)),
            combo_key_gap_ms: std::sync::atomic::AtomicU64::new(config.combo_key_gap_ms),
            worker_count: AtomicU64::new(0),
            process_whitelist: Mutex::new(config.process_whitelist.clone()),
            whitelist_enabled: std::sync::atomic::AtomicBool::new(config.whitelist_enabled),
            vib_legacy_client: std::sync::atomic::AtomicBool::new(config.vib_legacy_client),
            configured_worker_count: config.worker_count,
            input_mappings,
            worker_pool: OnceLock::new(),
            notification_sender: std::sync::Mutex::new(None),
            cached_process_info: RwLock::new((None, Instant::now())),
            mappings_revision: std::sync::atomic::AtomicU64::new(0),
            sequence_paused: std::sync::atomic::AtomicBool::new(false),
            sequence_runs: std::sync::Mutex::new(HashMap::new()),
            key_record: std::sync::Mutex::new(None),
            key_record_active: std::sync::atomic::AtomicBool::new(false),
            pressed_keys: scc::HashSet::new(),
            active_combo_triggers: scc::HashMap::new(),
            cached_turbo_keyboard,
            cached_turbo_other,
            cached_combo_index,
            cached_xinput_combos,
            raw_input_capture_sender,
            raw_input_capture_receiver: Mutex::new(raw_input_capture_receiver),
            is_capturing_raw_input: AtomicBool::new(false),
            rawinput_capture_mode: RwLock::new(
                CaptureMode::from_str(&config.rawinput_capture_mode).unwrap(),
            ),
            xinput_capture_mode: RwLock::new(crate::config::XInputCaptureMode::from_str(
                &config.xinput_capture_mode,
            )?),
            hid_activation_sender,
            hid_activation_receiver: Mutex::new(hid_activation_receiver),
            hid_activation_data_sender,
            hid_activation_data_receiver: Mutex::new(hid_activation_data_receiver),
            activating_device_handle: std::sync::atomic::AtomicIsize::new(-1),
            xinput_cache_invalid: AtomicBool::new(false),
            /* ★v24.10: 「仅用连发」(dfo_player=false) 时震动一律关闭 —— 入口已隐藏就不该在后台驱动 */
            vibration_enabled: AtomicBool::new(config.vibration.enabled && config.dfo_player),
            vibration_font_hits: AtomicBool::new(true),
            vibration_connected: AtomicBool::new(false),
            vibration_events_received: std::sync::atomic::AtomicU64::new(0),
            vibration_rank_events: std::sync::atomic::AtomicU64::new(0),
            vibration_rank_last: std::sync::atomic::AtomicU32::new(0),
            vibration_rank_test: std::sync::atomic::AtomicBool::new(false),
            vibration_rank_type_gain: std::array::from_fn(|i| {
                std::sync::atomic::AtomicU32::new(config.vibration.rank_type_gain[i])
            }),
            vibration_rank_type_events: std::array::from_fn(|_| std::sync::atomic::AtomicU64::new(0)),
            vibration_rank_level_gain: std::sync::atomic::AtomicU32::new(
                config.vibration.rank_level_gain,
            ),
            vibration_rank_duration: std::sync::atomic::AtomicU32::new(
                config.vibration.rank_duration,
            ),
            vibration_test_until: std::sync::atomic::AtomicU64::new(0),
            vibration_advanced_enabled: AtomicBool::new(config.vibration.advanced_enabled),
            vibration_motor_l_gain: std::sync::atomic::AtomicU32::new(
                config.vibration.motor_l_gain,
            ),
            vibration_motor_r_gain: std::sync::atomic::AtomicU32::new(
                config.vibration.motor_r_gain,
            ),
            vibration_random_gain: std::sync::atomic::AtomicU32::new(
                config.vibration.random_gain,
            ),
            vibration_out_smooth: std::sync::atomic::AtomicU32::new(
                config.vibration.out_smooth,
            ),
            vibration_throttle_window: std::sync::atomic::AtomicU32::new(
                config.vibration.throttle_window_ms,
            ),
            vibration_throttle_max: std::sync::atomic::AtomicU32::new(
                config.vibration.throttle_max_hits,
            ),
            vibration_throttle_dense_ratio: std::sync::atomic::AtomicU32::new(
                config.vibration.throttle_dense_ratio,
            ),
            vibration_merge_keep: std::sync::atomic::AtomicU32::new(
                config.vibration.merge_keep,
            ),
            vibration_merge_cap: std::sync::atomic::AtomicU32::new(
                config.vibration.merge_cap,
            ),
            vibration_merge_hold: std::sync::atomic::AtomicU32::new(
                config.vibration.merge_hold,
            ),
            vibration_hitcap_max: std::sync::atomic::AtomicU32::new(
                config.vibration.hitcap_max,
            ),
            vibration_hitcap_win: std::sync::atomic::AtomicU32::new(
                config.vibration.hitcap_win_ms,
            ),
            vibration_hitmerge_ms: std::sync::atomic::AtomicU32::new(
                config.vibration.hitmerge_ms,
            ),
            vibration_sustain_secs: std::sync::atomic::AtomicU32::new(
                config.vibration.sustain_secs,
            ),
            vibration_sustain_reduce: std::sync::atomic::AtomicU32::new(
                config.vibration.sustain_reduce,
            ),
            vibration_tail_land_pct: std::sync::atomic::AtomicU32::new(
                config.vibration.tail_land_pct,
            ),
            vibration_monster_abnormal: std::sync::atomic::AtomicU32::new(
                config.vibration.monster_abnormal_gain,
            ),
            vibration_storm_thr: std::sync::atomic::AtomicU32::new(
                config.vibration.storm_thr,
            ),
            vibration_storm_keep_pct: std::sync::atomic::AtomicU32::new(
                config.vibration.storm_keep_pct,
            ),
            vibration_storm_win_ms: std::sync::atomic::AtomicU32::new(
                config.vibration.storm_win_ms,
            ),
            vibration_storm_pause_ms: std::sync::atomic::AtomicU32::new(
                config.vibration.storm_pause_ms,
            ),
            vibration_storm_mute_abnormal: std::sync::atomic::AtomicBool::new(
                config.vibration.storm_mute_abnormal,
            ),
            vibration_storm_mute_rank: std::sync::atomic::AtomicBool::new(
                config.vibration.storm_mute_rank,
            ),
            vibration_merge_enabled: std::sync::atomic::AtomicBool::new(
                config.vibration.merge_enabled,
            ),
            vibration_hitcap_enabled: std::sync::atomic::AtomicBool::new(
                config.vibration.hitcap_enabled,
            ),
            vibration_storm_enabled: std::sync::atomic::AtomicBool::new(
                config.vibration.storm_enabled,
            ),
            vibration_sustain_enabled: std::sync::atomic::AtomicBool::new(
                config.vibration.sustain_enabled,
            ),
            vibration_tail_land_enabled: std::sync::atomic::AtomicBool::new(
                config.vibration.tail_land_enabled,
            ),
            vibration_density_enabled: std::sync::atomic::AtomicBool::new(
                config.vibration.density_enabled,
            ),
            vibration_adapt_enabled: std::sync::atomic::AtomicBool::new(
                config.vibration.adapt_enabled,
            ),
            vibration_move_charge_enabled: std::sync::atomic::AtomicBool::new(
                config.vibration.move_charge_enabled,
            ),
            vibration_decay_enabled: std::sync::atomic::AtomicBool::new(
                config.vibration.decay_enabled,
            ),
            vibration_algo_windows_enabled: std::sync::atomic::AtomicBool::new(
                config.vibration.algo_windows_enabled,
            ),
            vibration_pulse_enabled: std::sync::atomic::AtomicBool::new(
                config.vibration.pulse_enabled,
            ),
            vibration_storm_unified_enabled: std::sync::atomic::AtomicBool::new(
                config.vibration.storm_unified_enabled,
            ),
            vibration_storm_unified_ms: std::sync::atomic::AtomicU32::new(
                config.vibration.storm_unified_ms,
            ),
            vibration_legacy_output_mode: std::sync::atomic::AtomicU32::new(
                config.vibration.legacy_output_mode,
            ),
            vibration_independent_test: std::sync::atomic::AtomicBool::new(
                config.vibration.independent_test,
            ),
            vibration_move_independent: std::sync::atomic::AtomicBool::new(
                config.vibration.move_independent,
            ),
            vibration_abs_freq_enabled: std::sync::atomic::AtomicBool::new(
                config.vibration.abs_freq_enabled,
            ),
            vibration_abs_freq_window: std::sync::atomic::AtomicU32::new(
                config.vibration.abs_freq_window_ms,
            ),
            vibration_abs_freq_max: std::sync::atomic::AtomicU32::new(
                config.vibration.abs_freq_max,
            ),
            vibration_rank_decay_enabled: std::sync::atomic::AtomicBool::new(
                config.vibration.rank_decay_enabled,
            ),
            vibration_rank_decay_delay: std::sync::atomic::AtomicU32::new(
                config.vibration.rank_decay_delay_ms,
            ),
            vibration_rank_decay_speed: std::sync::atomic::AtomicU32::new(
                config.vibration.rank_decay_speed,
            ),
            vibration_rank_decay_min: std::sync::atomic::AtomicU32::new(
                config.vibration.rank_decay_min_mul,
            ),
            vibration_rank_decay_max: std::sync::atomic::AtomicU32::new(
                config.vibration.rank_decay_max_mul,
            ),
            vibration_out_threshold: std::sync::atomic::AtomicU32::new(
                config.vibration.out_threshold,
            ),
            vibration_remap_enabled: std::sync::atomic::AtomicBool::new(
                config.vibration.remap_enabled,
            ),
            vibration_remap_min: std::sync::atomic::AtomicU32::new(
                config.vibration.remap_min,
            ),
            vibration_split_enabled: std::sync::atomic::AtomicBool::new(
                config.vibration.split_enabled,
            ),
            vibration_split_thr: std::sync::atomic::AtomicU32::new(
                config.vibration.split_thr,
            ),
            vibration_algo_id: std::sync::atomic::AtomicU32::new(job_algo_id),
            vibration_algo_ap: std::array::from_fn(|i| {
                std::sync::atomic::AtomicU32::new(job_algo_ap[i])
            }),
            vibration_item_lr: std::array::from_fn(|i| {
                std::sync::atomic::AtomicU32::new(config.vibration.item_lr[i])
            }),
            vibration_rank_lr: std::array::from_fn(|i| {
                std::sync::atomic::AtomicU32::new(config.vibration.rank_lr[i])
            }),
            vibration_rank_out: std::array::from_fn(|_| std::sync::atomic::AtomicU32::new(0)),
            vibration_item_out: std::array::from_fn(|_| std::sync::atomic::AtomicU32::new(0)),
            vibration_out_l: std::sync::atomic::AtomicU32::new(0),
            vibration_out_r: std::sync::atomic::AtomicU32::new(0),
            vibration_params: std::array::from_fn(|i| AtomicU32::new(init_params[i])),
        })
    }


    /// Reloads configuration at runtime.
    ///
    /// # Errors
    ///
    /// Returns an error if the new toggle key is invalid or mappings cannot be created.
    pub fn reload_config(&self, config: AppConfig) -> anyhow::Result<()> {
        Self::update_switch_key_cache(&self.switch_key_cache, &config.switch_key)?;

        // Update language
        self.language
            .store(config.language.to_u8(), Ordering::Relaxed);

        // Update show_tray_icon and show_notifications
        self.show_tray_icon
            .store(config.show_tray_icon, Ordering::Relaxed);
        self.show_notifications
            .store(config.show_notifications, Ordering::Relaxed);

        // ★S1 老方案总开关 (设置切换 / 首启选择后即时生效)
        self.vib_legacy_client
            .store(config.vib_legacy_client, Ordering::Relaxed);
        // ★S1 输出引擎模式 (v20): 配置导入/重载后同步
        self.vibration_legacy_output_mode
            .store(config.vibration.legacy_output_mode, Ordering::Relaxed);

        // Update input timeout
        self.input_timeout
            .store(config.input_timeout.clamp(1, 2000), Ordering::Relaxed);
        self.combo_key_gap_ms
            .store(config.combo_key_gap_ms, Ordering::Relaxed);

        // Update capture modes (防中毒锁: 锁被 panic 弄脏时继续运行)
        *util::write_guard(&self.rawinput_capture_mode) =
            CaptureMode::from_str(&config.rawinput_capture_mode).unwrap();
        *util::write_guard(&self.xinput_capture_mode) =
            crate::config::XInputCaptureMode::from_str(&config.xinput_capture_mode)?;

        // Update mappings (lock-free concurrent HashMap)
        let new_input_mappings = Self::create_input_mappings(&config)?;
        self.input_mappings.clear_sync();
        for (k, v) in new_input_mappings {
            let _ = self.input_mappings.insert_sync(k, v);
        }

        /* ★v21.7 方案A: 是否存在"语义 HID" 触发键 (..._H<n> / ..._A<n>_<dir>);
         * 有才在 RawInput 热路径调用 HidP_GetUsages 做语义翻译, 否则零开销。 */
        self.semantic_hid_mappings
            .store(Self::config_has_semantic_mappings(&config), Ordering::Relaxed);

        // Update cached data structures
        self.cached_turbo_other.clear_sync();
        self.cached_combo_index.clear_sync();
        self.cached_xinput_combos.clear_sync();

        // Reset keyboard turbo cache to default
        for i in 0..256 {
            self.cached_turbo_keyboard[i].store(true, Ordering::Relaxed);
        }

        for mapping in config.mappings.iter() {
            if let Some(device) = Self::input_name_to_device(&mapping.trigger_key) {
                // Update turbo cache and combo index
                match &device {
                    InputDevice::Keyboard(vk) if *vk < 256 => {
                        self.cached_turbo_keyboard[*vk as usize]
                            .store(mapping.turbo_enabled, Ordering::Relaxed);
                    }
                    InputDevice::KeyCombo(keys) => {
                        if let Some(&last_key) = keys.last() {
                            let mut combos = self
                                .cached_combo_index
                                .get_sync(&last_key)
                                .map(|v| v.clone())
                                .unwrap_or_default();
                            combos.push(device.clone());

                            let _ = self.cached_combo_index.upsert_sync(last_key, combos);
                        }
                        let _ = self
                            .cached_turbo_other
                            .insert_sync(device, mapping.turbo_enabled);
                    }
                    InputDevice::XInputCombo {
                        device_type,
                        button_ids,
                    } => {
                        let mut combos = self
                            .cached_xinput_combos
                            .get_sync(device_type)
                            .map(|v| v.get().clone())
                            .unwrap_or_default();
                        combos.push(button_ids.clone());
                        let _ = self.cached_xinput_combos.upsert_sync(*device_type, combos);
                        let _ = self
                            .cached_turbo_other
                            .insert_sync(device, mapping.turbo_enabled);
                    }
                    _ => {
                        let _ = self
                            .cached_turbo_other
                            .insert_sync(device, mapping.turbo_enabled);
                    }
                }
            }
        }

        // Update process whitelist
        if let Ok(mut whitelist) = self.process_whitelist.lock() {
            *whitelist = config.process_whitelist.clone();
        }
        self.whitelist_enabled
            .store(config.whitelist_enabled, Ordering::Relaxed);

        // Clear process name cache
        if let Ok(mut cache) = self.cached_process_info.write() {
            *cache = (None, Instant::now());
        }
        // ★v24.31: 映射变更 → 修订号 +1, worker 据此强制解锁旧锁定条目 (防卡键)
        self.mappings_revision
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        // Clear pressed keys and active combos (lock-free concurrent structures)
        self.pressed_keys.clear_sync();
        self.active_combo_triggers.clear_sync();

        // Clear worker pool caches (mouse action cache, etc.)
        if let Some(pool) = self.worker_pool.get() {
            pool.clear_cache();
        }

        // Signal XInput to invalidate device-level caches
        self.xinput_cache_invalid.store(true, Ordering::Release);

        Ok(())
    }

    /// Sets the worker pool for event dispatching.
    pub fn set_worker_pool(&self, pool: Arc<dyn EventDispatcher>) {
        let _ = self.worker_pool.set(pool);
    }

    /// Gets the worker pool for event dispatching.
    pub fn get_worker_pool(&self) -> Option<&Arc<dyn EventDispatcher>> {
        self.worker_pool.get()
    }

    /// Gets the Raw Input capture sender for GUI key capture mode.
    pub fn get_raw_input_capture_sender(&self) -> &Sender<InputDevice> {
        &self.raw_input_capture_sender
    }

    /// Checks and resets XInput cache invalidation flag.
    #[inline(always)]
    pub fn check_and_reset_xinput_cache_invalid(&self) -> bool {
        self.xinput_cache_invalid.swap(false, Ordering::Acquire)
    }

    /// Gets the current Raw Input capture mode.
    pub fn get_rawinput_capture_mode(&self) -> CaptureMode {
        *util::read_guard(&self.rawinput_capture_mode)
    }

    /// Gets the current XInput capture mode.
    pub fn get_xinput_capture_mode(&self) -> crate::config::XInputCaptureMode {
        *util::read_guard(&self.xinput_capture_mode)
    }

    /// Tries to receive a captured Raw Input event (non-blocking).
    pub fn try_recv_raw_input_capture(&self) -> Option<InputDevice> {
        if let Ok(receiver) = self.raw_input_capture_receiver.lock() {
            receiver.try_recv().ok()
        } else {
            None
        }
    }

    /// Sets the Raw Input capture mode flag.
    pub fn set_raw_input_capture_mode(&self, enabled: bool) {
        self.is_capturing_raw_input
            .store(enabled, Ordering::Relaxed);

        // When entering capture mode, clear all caches to ensure fresh state
        if enabled {
            // Clear all old events in the channel to avoid showing stale captures
            if let Ok(receiver) = self.raw_input_capture_receiver.lock() {
                while receiver.try_recv().is_ok() {}
            }

            // Clear device display info cache to avoid showing stale device info
            crate::rawinput::clear_device_display_info_cache();

            // Reset HID device states to baseline for clean button detection
            crate::rawinput::reset_hid_device_states();
            /* ★v24.6: 捕获模式不走 XInput 正常派发 → 清掉实时位图, 免得 GUI 拿到过期状态 */
            *util::write_guard(&self.live_xinput) = None;
        }
    }

    /// Clears activation baseline for device.
    #[inline]
    pub fn clear_device_baseline(&self, vid: u16, pid: u16) {
        crate::rawinput::clear_device_baseline(vid, pid);
    }

    /// Checks if Raw Input capture mode is active.
    pub fn is_raw_input_capture_active(&self) -> bool {
        self.is_capturing_raw_input.load(Ordering::Relaxed)
    }

    /* ───────── ★v21.7 预设切换键 (手柄) 绑定与请求 ───────── */

    /// GUI 侧在预设/切换键变化时刷新绑定表 (仅手柄类绑定; 键盘由 GUI 自己轮询)。
    pub fn set_preset_switch_bindings(&self, bindings: Vec<PresetSwitchBinding>) {
        *util::write_guard(&self.preset_switch_bindings) = bindings;
    }

    /// 输入线程检测到预设切换键按下时调用 (边缘已由调用方保证)。
    /// 入队后由 GUI 主循环取走并执行真正的切换 (配置/落盘/热重载都在 UI 线程)。
    pub fn request_preset_switch(&self, preset_name: &str) {
        if let Ok(mut q) = self.pending_preset_switches.lock() {
            /* 防连发堆积: 同名只留一个 */
            if !q.iter().any(|n| n == preset_name) {
                q.push(preset_name.to_string());
            }
        }
    }

    /// GUI 每帧取走全部待切换请求 (FIFO)。
    pub fn take_pending_preset_switches(&self) -> Vec<String> {
        self.pending_preset_switches
            .lock()
            .map(|mut q| std::mem::take(&mut *q))
            .unwrap_or_default()
    }

    /// 取当前绑定表的快照 (输入线程借用期间不做长锁)。
    pub fn preset_switch_bindings_snapshot(&self) -> Vec<PresetSwitchBinding> {
        (*util::read_guard(&self.preset_switch_bindings)).clone()
    }

    /// 绑定表是否为空 (输入热路径零拷贝快速判断)。
    #[inline(always)]
    pub fn preset_switch_bindings_is_empty(&self) -> bool {
        util::read_guard(&self.preset_switch_bindings).is_empty()
    }

    /// 校验一个输入名是否合法 (键盘键名 / 手柄组合 / 原始 HID 设备名 / 鼠标键)。
    /// ★v21.7: 预设切换键保存时用它替代仅支持键盘的 `parse_switch_key`。
    pub fn is_valid_input_name(name: &str) -> bool {
        let n = name.trim();
        !n.is_empty() && Self::input_name_to_device(n).is_some()
    }

    /// ★v21.7 方案A: 配置里是否有任一"语义 HID"触发键 (..._H<n> / ..._A<n>_<dir>)。
    pub fn config_has_semantic_mappings(config: &AppConfig) -> bool {
        config.mappings.iter().any(|m| {
            match Self::input_name_to_device(&m.trigger_key) {
                Some(InputDevice::GenericDevice { button_id, .. }) => {
                    let pos = (button_id & 0xFFFF_FFFF) as u32;
                    crate::hid_layout::semantic_button_usage(pos).is_some()
                        || crate::hid_layout::semantic_axis_decode(pos).is_some()
                }
                _ => false,
            }
        })
    }

    /// ★v21.7 方案A: RawInput 热路径是否需要做语义翻译 (无则零开销跳过)。
    #[inline(always)]
    pub fn has_semantic_hid_mappings(&self) -> bool {
        self.semantic_hid_mappings.load(Ordering::Relaxed)
    }

    /* ───────── ★v21.7b 第三方手柄实时状态 (SVG 按下即亮) ───────── */

    /// GUI 选择"实时识别"的第三方手柄 (None = 停止)。
    pub fn set_live_hid_pad(&self, pad: Option<(u16, u16)>) {
        *util::write_guard(&self.live_hid_pad) = pad;
        if pad.is_none() {
            *util::write_guard(&self.live_hid_state) = None;
            *util::write_guard(&self.live_xinput) = None;
        }
    }

    /// 当前正在实时识别的手柄 vid:pid。
    pub fn live_hid_pad(&self) -> Option<(u16, u16)> {
        *util::read_guard(&self.live_hid_pad)
    }

    /// RawInput 线程发布一帧实时状态 (仅当设备是当前识别对象时记录)。
    pub fn publish_live_hid(&self, state: LiveHidState) {
        let target = *util::read_guard(&self.live_hid_pad);
        if target == Some((state.vid, state.pid)) {
            *util::write_guard(&self.live_hid_state) = Some(state);
        }
    }

    /// ★v22.4: 局部更新"原始位组合哈希" —— 当语义通道没在跑 (无语义映射) 时也能让
    /// 已校准槽位点亮。仅当是该识别目标手柄时生效。
    pub fn publish_live_raw(&self, vid: u16, pid: u16, pos: u32) {
        if *util::read_guard(&self.live_hid_pad) != Some((vid, pid)) {
            return;
        }
        let mut guard = util::write_guard(&self.live_hid_state);
        match guard.as_mut() {
            Some(s) => s.raw_position = pos,
            None => {
                *guard = Some(LiveHidState {
                    vid,
                    pid,
                    raw_position: pos,
                    ..Default::default()
                });
            }
        }
    }

    /// GUI 读取最新实时状态 (None = 未识别/设备已断开)。
    pub fn live_hid_state(&self) -> Option<LiveHidState> {
        *util::read_guard(&self.live_hid_state)
    }

    /// ★v24.6: XInput 线程发布一帧实时输入位图 (input id 0x01..0x1F → bit[id])。
    ///
    /// 仅"识别中"时记录 (手柄页一次只识别一台); 无输入 → 置空。
    /// 走这条路而非捕获模式, 避免抑制正常映射派发 (奔跑/方向失效的根因)。
    pub fn publish_live_xinput(&self, vid: u16, mask: u32) {
        if util::read_guard(&self.live_hid_pad).is_none() {
            return;
        }
        *util::write_guard(&self.live_xinput) = if mask == 0 { None } else { Some((vid, mask)) };
    }

    /// GUI 读取最新 XInput 实时输入 (vid, 位图)。
    pub fn live_xinput_state(&self) -> Option<(u16, u32)> {
        *util::read_guard(&self.live_xinput)
    }

    /// Sends HID device activation request.
    #[inline]
    pub fn request_hid_activation(&self, request: HidActivationRequest) {
        self.activating_device_handle
            .store(request.device_handle, Ordering::Relaxed);
        let _ = self.hid_activation_sender.send(request);
    }

    /// Polls for HID device activation requests
    pub fn poll_hid_activation_requests(&self) -> SmallVec<[HidActivationRequest; 2]> {
        let mut requests = SmallVec::new();
        if let Ok(receiver) = self.hid_activation_receiver.lock() {
            while let Ok(req) = receiver.try_recv() {
                requests.push(req);
            }
        }
        requests
    }

    /// Sends HID activation data during activation process.
    #[inline]
    pub fn send_hid_activation_data(&self, device_handle: isize, data: Vec<u8>) {
        let _ = self.hid_activation_data_sender.send((device_handle, data));
    }

    /// Tries to receive HID activation data for a specific device.
    pub fn try_recv_hid_activation_data(&self, device_handle: isize) -> Option<Vec<u8>> {
        if let Ok(receiver) = self.hid_activation_data_receiver.lock() {
            // Consume all messages until we find one for this device
            // or channel is empty
            while let Ok((handle, data)) = receiver.try_recv() {
                if handle == device_handle {
                    return Some(data);
                }
                // Discard data for other devices (shouldn't happen in normal operation)
            }
        }
        None
    }

    /// Checks if a device is currently being activated.
    #[inline(always)]
    pub fn is_device_activating(&self, device_handle: isize) -> bool {
        self.activating_device_handle.load(Ordering::Relaxed) == device_handle
    }

    /// Clears the activating device handle.
    #[inline]
    pub fn clear_activating_device(&self) {
        self.activating_device_handle.store(-1, Ordering::Relaxed);
    }

    /// Sets the notification event sender.
    pub fn set_notification_sender(&self, sender: Sender<NotificationEvent>) {
        // Mutex 而非 OnceLock: 托盘线程由 supervisor 重启后必须能替换 sender,
        // 旧实现二次 set 失败 → 重启后所有通知静默失联
        if let Ok(mut slot) = self.notification_sender.lock() {
            *slot = Some(sender);
        }
    }

    /// 发送通知 (无接收端 / 通道关闭时静默降级)
    pub fn send_notification(&self, event: NotificationEvent) {
        if let Ok(slot) = self.notification_sender.lock() {
            if let Some(sender) = slot.as_ref() {
                let _ = sender.send(event);
            }
        }
    }

    /// Signals the application to exit.
    pub fn exit(&self) {
        self.should_exit.store(true, Ordering::Relaxed);
    }

    /// Checks if the application should exit (hot path - inlined)
    #[inline(always)]
    pub fn should_exit(&self) -> bool {
        self.should_exit.load(Ordering::Relaxed)
    }

    /// Toggles pause state and returns the previous state.
    pub fn toggle_paused(&self) -> bool {
        self.is_paused.fetch_xor(true, Ordering::Relaxed)
    }

    /// Returns the current pause state (hot path - inlined)
    #[inline(always)]
    pub fn is_paused(&self) -> bool {
        self.is_paused.load(Ordering::Relaxed)
    }

    /// Sets the pause state.
    pub fn set_paused(&self, paused: bool) {
        self.is_paused.store(paused, Ordering::Relaxed);
    }

    /// ★v22.7: 手柄校准向导是否进行中 (热路径, 内联)。
    #[inline(always)]
    pub fn is_gp_calibrating(&self) -> bool {
        self.gp_calibrating.load(Ordering::Relaxed)
    }

    /// ★v22.7: 设置手柄校准向导进行中标志 (进行中时输入线程不派发任何映射)。
    pub fn set_gp_calibrating(&self, on: bool) {
        self.gp_calibrating.store(on, Ordering::Relaxed);
    }

    /// Returns whether the tray icon should be shown.
    pub fn show_tray_icon(&self) -> bool {
        self.show_tray_icon.load(Ordering::Relaxed)
    }

    /// Returns whether notifications should be displayed.
    pub fn show_notifications(&self) -> bool {
        self.show_notifications.load(Ordering::Relaxed)
    }

    /// Returns the current UI language.
    #[inline(always)]
    pub fn language(&self) -> Language {
        Language::from_u8(self.language.load(Ordering::Relaxed))
    }

    /// Requests the main window to be shown.
    pub fn request_show_window(&self) {
        self.show_window_requested.store(true, Ordering::Relaxed);
    }

    /// Checks and clears the show window request flag.
    pub fn check_and_clear_show_window_request(&self) -> bool {
        self.show_window_requested.swap(false, Ordering::Relaxed)
    }

    /// Requests the about dialog to be shown.
    pub fn request_show_about(&self) {
        self.show_about_requested.store(true, Ordering::Relaxed);
    }

    /// Checks and clears the show about request flag.
    pub fn check_and_clear_show_about_request(&self) -> bool {
        self.show_about_requested.swap(false, Ordering::Relaxed)
    }

    /// Returns the input timeout in milliseconds.
    pub fn input_timeout(&self) -> u64 {
        // 绝对稳定: clamp 到 [1, 2000]ms —— 过大则 worker 长时间不取事件(队列积压=卡死),
        // 过小/0 则 recv_timeout 立即超时空转满载 CPU。
        self.input_timeout.load(Ordering::Relaxed).clamp(1, 2000)
    }

    /// Returns the configured worker thread count.
    pub fn get_configured_worker_count(&self) -> usize {
        self.configured_worker_count
    }

    /// Returns the actual number of active worker threads.
    pub fn get_actual_worker_count(&self) -> usize {
        self.worker_count.load(Ordering::Relaxed) as usize
    }

    pub fn set_actual_worker_count(&self, count: usize) {
        self.worker_count.store(count as u64, Ordering::Relaxed);
    }

    /// Fast mapping lookup using lock-free read
    #[inline(always)]
    pub fn get_input_mapping(&self, device: &InputDevice) -> Option<InputMappingInfo> {
        self.input_mappings.read_sync(device, |_, v| v.clone())
    }

    /// ★v24.31 当前映射配置修订号 (锁定条目防卡键用)
    #[inline]
    pub(crate) fn mappings_revision(&self) -> u64 {
        self.mappings_revision
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    /// ★v24.31 登记一条执行中的序列 (同一设备上一条没跑完 → 忽略新触发)。
    /// 返回 Ok(run) 时调用方必须派生 seq-runner 线程。
    pub(crate) fn register_sequence_run(
        &self,
        device: &InputDevice,
    ) -> Result<Arc<SequenceRun>, ()> {
        let mut runs = self.sequence_runs.lock().map_err(|_| ())?;
        if runs.contains_key(device) {
            return Err(());
        }
        let run = Arc::new(SequenceRun {
            stop: std::sync::atomic::AtomicBool::new(false),
        });
        runs.insert(device.clone(), run.clone());
        Ok(run)
    }

    /// ★v24.31 序列执行完毕 (runner 线程收尾): 只清掉自己那条, 防误删新一轮
    pub(crate) fn sequence_run_finished(&self, device: &InputDevice, run: &Arc<SequenceRun>) {
        if let Ok(mut runs) = self.sequence_runs.lock() {
            if runs.get(device).map(|r| Arc::ptr_eq(r, run)).unwrap_or(false)
            {
                runs.remove(device);
            }
        }
    }

    /// ★v24.31 序列控制键: 全局 暂停/继续/切换
    pub(crate) fn sequence_ctl(&self, ctl: SequenceCtl) {
        match ctl {
            SequenceCtl::Toggle => {
                let paused = self.sequence_paused.load(Ordering::Relaxed);
                self.sequence_paused.store(!paused, Ordering::Relaxed);
            }
            SequenceCtl::Pause => self.sequence_paused.store(true, Ordering::Relaxed),
            SequenceCtl::Continue => self.sequence_paused.store(false, Ordering::Relaxed),
        }
    }






}

pub fn set_global_state(state: Arc<AppState>) -> Result<(), Arc<AppState>> {
    let ptr = Arc::as_ptr(&state) as usize;
    crate::vibration::vib_log(&format!(
        "[hook] set_global_state ptr=0x{ptr:X}"
    ));
    GLOBAL_STATE.set(state)
}

pub fn get_global_state() -> Option<&'static Arc<AppState>> {
    GLOBAL_STATE.get()
}
