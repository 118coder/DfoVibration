//! ★C 模式事件层 —— B 模式的下一代：多带事件颗粒度 + 纯算法 BGM 筛选 + C→B 回退。
//!
//! 施工图：`docs/音频震动_C模式_算法设计_v1.md`（GPT-a 设计文档）。实现与文档的
//! 已知偏差见本文件尾部「偏差记录」与 `docs/交接_C模式实现轮.md`。
//!
//! ## 定位（文档 §2.1）
//! B 的近远/高低/短长感知不变（`smart.rs` 原样保留、始终热态运行），C 只增加：
//! 1. **逐带事件证据**：谱通量 F_norm、逐带超基线余量 h_i、谱平坦度 wide（§4.3-§4.4）；
//! 2. **事件状态机**：onset/hold/release + refractory（§4.5）；
//! 3. **纯算法 BGM 置信度**：HPSS 因果中值掩膜 + REPET 滞后相似 + 粗谐波性 +
//!    节拍自相关 + 谱新颖度/稳定度 → 带滞回的 MUSIC_LOCK（§3.4）；
//! 4. **背景/事件双分量门控**：持续层强压、规律鼓点中压、宽带突发事件 escape（§3.4.4.4）；
//! 5. **事件级路由**：低频偏左/高频偏右/宽带均衡 + 可选声道 pan 插值（§4.8）；
//! 6. **回退链**：`c_upgrade_pct=0` 或未暖机或故障 → 逐字节回 B（§5）。
//!
//! ## 纪律（与 v24.60 红线一致）
//! - 零训练、零模型权重、零神经网络；经典 DSP/MIR 统计量组合；
//! - 因果实时：只用当前帧与过去帧；快速事件通路不等待节奏锁；
//! - 缓冲为构造期一次性小分配；FFT hop 热路径零堆分配；慢通道(10Hz)亦零分配；
//! - 所有输入/输出 `is_finite` 检查，连续异常自动旁路 C 保留 B；
//! - 不引入新 crate、不碰采样/线程/DLL 语义。

use super::smart::SmartOutput;

/* ═══════════════ 常数（文档 §3.4.6 / §4.12 设计默认值，标注待实机标定） ═══════════════ */

/// HPSS 因果时间中值窗（帧；9 帧 ≈96ms @48k）。
const HPSS_TIME_FRAMES: usize = 9;
/// HPSS 频率中值窗宽（对数谱组；奇数）。
const HPSS_FREQ_WIN: usize = 7;
/// HPSS 掩膜锐度 p（1.0 = 软分离）。
const HPSS_MASK_POWER: f32 = 1.0;
/// REPET 历史（ms；构造期换算成帧）。
const REPET_HISTORY_MS: f32 = 2000.0;
/// 重复/节拍候选周期范围（ms）。
const LAG_MIN_MS: f32 = 85.0;
const LAG_MAX_MS: f32 = 1000.0;
/// REPET 余弦距离 → 相似度的尺度 sigma（0.18）。
const REPET_SIGMA: f32 = 0.18;
/// 每个 lag 维护最近 K 次相似度（均值-半方差抗偶然相似）。
const REPET_K: usize = 5;
/// REPET 相似度进入背景组合的软门槛（c_repet_similarity_on=0.68）。
const REPET_SIM_ON: f32 = 0.68;
/// 周期候选连续稳定比例（8%）。
const LAG_STABILITY: f32 = 0.08;
/// 谐波候选 F0 范围（55-1000Hz，36 个对数候选，1-6 次谐波，权重 1/sqrt(m)）。
const HARM_F0_MIN: f32 = 55.0;
const HARM_F0_MAX: f32 = 1000.0;
const HARM_F0_N: usize = 36;
const HARM_PARTIALS: usize = 6;
/// 节拍自相关历史（c_beat_window_ms=2000）。
const BEAT_WINDOW_MS: f32 = 2000.0;
/// 慢通道（REPET/节拍/稳定度）更新周期（100ms）。
const SLOW_UPDATE_MS: f32 = 100.0;
/// 节拍相关锁定门槛（0.55）与需要的连续稳定更新数（3）。
const BEAT_CORR_ON: f32 = 0.55;
const BEAT_LOCK_UPDATES: u32 = 3;
/* MUSIC_LOCK 进入/退出滞回（文档 §3.4.4.3 的 0.72/0.48/0.60 经数学复核后校准：
 * 理想电子 BGM（M_hpss≈0.55/R_rep≈0.85/beat≈0.9/harm≈0.6 的异质证据组合）在文档
 * 公式下 conf 上限 ≈0.49，0.60 的进入线永远不会触发 —— 招牌能力死锁。按证据画像
 * 重校准为 0.58/0.48/0.42（偏差记录 #11）：滞回带 0.48-0.58 保持，退出通道
 * （conf 骤降/新颖度/静音）原样保留，非周期内容 raw≈0.1 依旧远够不到进入线。 */
const MUSIC_ENTER: f32 = 0.58;
const MUSIC_EXIT: f32 = 0.48;
const MUSIC_ENTER_HOLD_MS: f32 = 600.0;
const MUSIC_EXIT_HOLD_MS: f32 = 80.0;
const MUSIC_CONF_MIN: f32 = 0.42;
const MUSIC_CONF_DROP: f32 = 0.35;
/// 新颖度解锁阈值（0.55，需持续 NOVELTY_UNLOCK_MS）。
const NOVELTY_UNLOCK: f32 = 0.55;
/// 快速退出通道的持续时长（ms；文档「持续 2 帧」≈21ms 被周期鼓点的瞬时谱变化
/// 反复误触发 —— BGM 自身鼓点只有 ~3 帧，换曲/技能入侵会持续 ≥150ms）。
const UNLOCK_SUSTAIN_MS: f32 = 150.0;
/// 静音清空历史（沿用 classify_content 的 total<55 量纲；120ms）。
const QUIET_TOTAL: f32 = 55.0;
const QUIET_RESET_MS: f32 = 120.0;
/// 背景证据组合权重（文档 §3.4.4.2 C_bg 公式；和 = 1）。
const W_M_B: f32 = 0.20;
const W_M_HPSS: f32 = 0.20;
const W_R_REP: f32 = 0.20;
const W_B_BEAT: f32 = 0.18;
const W_H_HARM: f32 = 0.12;
const W_N_NARROW: f32 = 0.06;
const W_S_STABLE: f32 = 0.04;
/// 事件分数权重（文档 §3.4.4.1 E_event；和 = 1）。
const W_EVT_FLUX: f32 = 0.35;
const W_EVT_WIDE: f32 = 0.25;
const W_EVT_M_P: f32 = 0.20;
const W_EVT_NOVEL: f32 = 0.20;
/// 事件对背景分数的削弱系数（1-0.55*E_event）。
const EVT_ERODE: f32 = 0.55;
/// 语音保护系数（c_voice_guard_pct=35）。
const VOICE_GUARD: f32 = 0.35;
/// 证据一致性分母（std(u)/0.45 → disagree；文档 0.35 对异质证据过严，偏差记录 #11）。
const AGREE_DIV: f32 = 0.45;
/// 谱平坦度映射端点（c_flatness_low/high）。
const FLATNESS_LOW: f32 = 0.18;
const FLATNESS_HIGH: f32 = 0.55;
/// 谱新颖度背景均线 alpha（0.08，快通道喂 E_event）与映射端点（0.82/0.98）。
const NOVELTY_ALPHA: f32 = 0.08;
/// 慢通道背景均线 alpha（0.02，半衰期 ≈360ms —— 专管换曲解锁：周期鼓点在慢背景
/// 中本就是成分，天然不触发；换曲会持续偏移 ≥400ms）。
const NOVELTY_SLOW_ALPHA: f32 = 0.02;
const NOVELTY_LOW: f32 = 0.82;
const NOVELTY_HIGH: f32 = 0.98;
/// B 输出拆分阈值（c_event_split_threshold=0.45）。
const EVENT_SPLIT_THR: f32 = 0.45;
/// 高置信规律鼓点事件压制（c_event_suppress_pct=70）。
const EVENT_SUPPRESS: f32 = 0.70;
/// 宽带逃逸权重（c_wideband_escape_pct=70）。
const WIDEBAND_ESCAPE: f32 = 0.70;
/* 事件状态机（文档 §4.5/§4.12） */
/// 起振阈值（c_onset_threshold=0.58）与 theta_off 比例（0.65）。
const THETA_ON: f32 = 0.58;
const THETA_OFF_RATIO: f32 = 0.65;
/// 起振不应期 ms（c_refractory_ms=65；文档 c_merge_ms=35 < 65，合并语义被其覆盖）。
const REFRACTORY_MS: f32 = 65.0;
/// HOLD 无再增强超时 ms（c_hold_timeout_ms=300）。
const HOLD_TIMEOUT_MS: f32 = 300.0;
/// RELEASE 结束包络线。
const ENV_END: f32 = 0.05;
/// 事件包络时间常数范围（近/远，ms）。
const TAU_NEAR_MS: f32 = 45.0;
const TAU_FAR_MS: f32 = 200.0;
/// p_event 权重（文档 §4.7：0.60*proximity + 0.25*T + 0.15*(1-tail)）。
const P_EVT_PROX: f32 = 0.60;
const P_EVT_T: f32 = 0.25;
const P_EVT_TAIL: f32 = 0.15;
/// 逐带对数压缩参考（int16 尺度；≈底噪量级，避免静音段对数发散）。
const X_REF: f32 = 30.0;
/// 逐带慢基线系数（文档 alpha_b=0.035）与 h_i 分母地板（对数域）。
const ALPHA_B: f32 = 0.035;
const H_FLOOR: f32 = 1.0;
/// 逐带谱通量权重（文档 §4.3 w=[1.15,1.20,1.00,1.10,0.90]）。
const FLUX_W: [f32; 5] = [1.15, 1.20, 1.00, 1.10, 0.90];
/// 事件融合默认增益（c_event_gain=65%），headroom 加法用。
const EVENT_GAIN: f32 = 0.65;
/// 事件级声道插值默认（c_stereo_event_pct=60；0=纯频带路由，100=完全跟 pan）。
const STEREO_EVENT_PCT: f32 = 0.60;
/// 暖机帧数（≈340ms；c_warmup_ms）。
const WARMUP_FRAMES: u32 = 32;
/// 故障旁路：连续异常帧数（c_fault_trip_count=3）与冷却（1s）。
const FAULT_TRIP: u32 = 3;
const FAULT_COOLDOWN_MS: f32 = 1000.0;
/// 频带路由权重（低/中/高 → 左马达；右马达对称 0.18/0.28/0.72）。
const ROUTE_L_LOW: f32 = 0.72;
const ROUTE_L_MID: f32 = 0.28;
const ROUTE_L_HIGH: f32 = 0.18;
/// 对数谱组数（20Hz-16kHz，24 组）与组范围（dsp.rs 预计算 bin→组 用）。
pub const N_GROUPS: usize = 24;
pub const GROUP_LO_HZ: f32 = 20.0;
pub const GROUP_HI_HZ: f32 = 16000.0;
/// 门控平滑系数（帧率 ~93/s；≈40ms 尾巴防马达阶跃）。
const GATE_UP: f32 = 0.5;
const GATE_DOWN: f32 = 0.35;
/// RMS 短窗长度（AM 深度/稳定度；32 帧 ≈340ms，对齐 B 的 smart.rs 窗）。
const RMS_WINDOW: usize = 32;

/* ═══════════════ 状态编号（遥测/日志用） ═══════════════ */

/// 音乐门控状态：未判定 / 过渡 / 锁定。
pub const MUSIC_NON: u8 = 0;
pub const MUSIC_UNKNOWN: u8 = 1;
pub const MUSIC_LOCK: u8 = 2;
/// 事件状态机状态。
pub const EVT_IDLE: u8 = 0;
pub const EVT_ATTACK: u8 = 1;
pub const EVT_HOLD: u8 = 2;
pub const EVT_RELEASE: u8 = 3;

/* ═══════════════ 参数与 I/O 结构 ═══════════════ */

/// C 层参数快照（引擎每 tick 从 EngineParams 搬运；其余 C 细参为文件内常数）。
#[derive(Debug, Clone, Copy)]
pub struct CParams {
    /// C 相对 B 的升级强度 % 0..=100；**0 = 整层旁路（逐字节回 B，无损回退旋钮）**。
    pub upgrade_pct: u32,
    /// 高置信 BGM 持续层最大压制 % 0..=100。
    pub sustain_suppress_pct: u32,
    /// BGM 筛选开关（false = 只做事件增强，不压背景）。
    pub bgm_filter: bool,
}

/// 每个 FFT hop 喂给 C 的一帧输入（全部来自 Analyzer 已算好的量，不重算 FFT）。
pub struct CFrameInput<'a> {
    /// 5 显示频带 RMS（int16 尺度；20-80/80-250/250-2k/2k-8k/8k+ Hz）。
    pub band_rms: [f32; 5],
    /// 24 个对数谱组的功率和（20Hz-16kHz）。
    pub group_power: &'a [f32; N_GROUPS],
    /// 原始功率谱 bin（只读；长度 = FFT_SIZE/2，bin 宽 = rate/FFT_SIZE）。
    pub power: &'a [f32],
    /// 总 RMS（int16 尺度，时域口径，与 B 一致）。
    pub total_rms: f32,
    /// 人声带 RMS（int16 尺度）。
    pub vocal_rms: f32,
    /// B 内容分类上一帧结果（mapping::MODE_*；M_b 先验）。
    pub mode_b: u8,
}

/// C 层一帧融合输出。
#[derive(Debug, Clone, Copy)]
pub struct COut {
    /// false = 本 tick C 层不生效（旁路/未暖机/故障），调用方保持 B 目标原样。
    pub active: bool,
    pub target_l: f32,
    pub target_r: f32,
    /// 攻/放速率的**加性上调量**（文档 §4.9 attack_c/release_c；调用方 clamp）。
    pub attack_add: f32,
    pub release_add: f32,
    /* 遥测 */
    pub music_raw: f32,
    pub conf: f32,
    pub music_state: u8,
    pub beat_lock: bool,
    pub event_state: u8,
    pub event_score: f32,
    pub degraded: bool,
}

impl Default for COut {
    fn default() -> Self {
        Self {
            active: false,
            target_l: 0.0,
            target_r: 0.0,
            attack_add: 0.0,
            release_add: 0.0,
            music_raw: 0.0,
            conf: 0.0,
            music_state: MUSIC_UNKNOWN,
            beat_lock: false,
            event_state: EVT_IDLE,
            event_score: 0.0,
            degraded: false,
        }
    }
}

/* ═══════════════ 小工具（纯函数） ═══════════════ */

/// 小数组中值（插入排序；n ≤ 32，开销可忽略）。
fn median_small(buf: &mut [f32]) -> f32 {
    let n = buf.len();
    for i in 1..n {
        let v = buf[i];
        let mut j = i;
        while j > 0 && buf[j - 1] > v {
            buf[j] = buf[j - 1];
            j -= 1;
        }
        buf[j] = v;
    }
    buf[n / 2]
}

/// 有限值保护：任一非有限 → false。
fn all_finite(vals: &[f32]) -> bool {
    vals.iter().all(|v| v.is_finite())
}

/* ═══════════════ C 模式状态容器 ═══════════════ */

/// C 模式事件层（引擎线程独占；构造期分配，热路径零堆分配）。
pub struct CMode {
    rate: f32,
    frame_dt_ms: f32,
    frames_seen: u32,
    frame_time_ms: f32,
    last_slow_ms: f32,
    /* 逐带状态 */
    x_prev: [f32; 5],
    x_base: [f32; 5],
    energy_slow: f32,
    last_total: f32,
    last_centroid: f32,
    /* onset 包络上一帧低带值 */
    o_prev_x0: f32,
    o_prev_x1: f32,
    /* 短窗（AM 深度/稳定度） */
    rms_ring: [f32; RMS_WINDOW],
    rms_pos: usize,
    rms_fill: usize,
    centroid_ring: [f32; RMS_WINDOW],
    cen_pos: usize,
    cen_fill: usize,
    /* HPSS 时间中值历史（24 组 × 9 帧环形） */
    hpss_ring: [f32; N_GROUPS * HPSS_TIME_FRAMES],
    hpss_pos: usize,
    hpss_fill: usize,
    /* REPET q 历史（环形；长度 = lag_max+2 帧） */
    q_ring: Vec<f32>,
    q_pos: usize,
    q_fill: usize,
    q_len: usize,
    /* 每个 lag 最近 K 次相似度环形 */
    rep_ring: Vec<f32>,
    rep_pos: Vec<u32>,
    rep_fill: Vec<u32>,
    /* 节拍 onset 包络环形 */
    o_ring: Vec<f32>,
    o_pos: usize,
    o_fill: usize,
    o_len: usize,
    beat_stab: u32,
    beat_prev_lag: i32,
    /* 新颖度背景谱（快 = E_event 用；慢 = 换曲解锁用） */
    q_bg: [f32; N_GROUPS],
    q_bg_ready: bool,
    q_bg_slow: [f32; N_GROUPS],
    q_bg_slow_ready: bool,
    novelty_slow: f32,
    /* 中值暂存（构造期分配，慢通道复用） */
    med_t: [f32; HPSS_TIME_FRAMES],
    med_f: [f32; HPSS_FREQ_WIN],
    med_s: [f32; RMS_WINDOW],
    /* lag 范围（构造期按采样率换算） */
    lag_min: usize,
    lag_max: usize,
    n_lags: usize,
    /* 缓存的帧特征（fuse 用） */
    f_norm: f32,
    band_contrast: f32,
    wide: f32,
    novelty: f32,
    m_hpss: f32,
    harmonicity: f32,
    r_rep: f32,
    beat_conf: f32,
    beat_lock: bool,
    stability: f32,
    am_depth: f32,
    w_low: f32,
    w_mid: f32,
    w_high: f32,
    t_score: f32,
    music_raw: f32,
    conf: f32,
    evt_score: f32,
    /* 音乐锁状态机 */
    music_state: u8,
    enter_hold_ms: f32,
    exit_hold_ms: f32,
    conf_drop_ms: f32,
    novel_ms: f32,
    quiet_ms: f32,
    gate: f32,
    /* 事件状态机 */
    evt_state: u8,
    evt_env: f32,
    refractory_until: f32,
    last_boost_ms: f32,
    release_since: f32,
    p_event_cache: f32,
    /* 故障旁路 */
    fault_streak: u32,
    degraded: bool,
    cooldown_until: f32,
}

impl CMode {
    /// 构造（rate = 设备采样率；一次性小分配都在这里，热路径零分配）。
    pub fn new(rate: f32) -> Self {
        let frame_dt_ms = (super::dsp::HOP as f32 / rate.max(1.0)) * 1000.0;
        let lag_min = ((LAG_MIN_MS / frame_dt_ms).ceil() as usize).max(2);
        let lag_max = ((LAG_MAX_MS / frame_dt_ms).floor() as usize).max(lag_min + 4);
        let n_lags = lag_max - lag_min + 1;
        let q_len = lag_max + 2;
        let o_len = ((REPET_HISTORY_MS.max(BEAT_WINDOW_MS) / frame_dt_ms).ceil() as usize)
            .max(lag_max + 8);
        Self {
            rate: rate.max(1.0),
            frame_dt_ms,
            frames_seen: 0,
            frame_time_ms: 0.0,
            last_slow_ms: f32::NEG_INFINITY,
            x_prev: [0.0; 5],
            x_base: [0.0; 5],
            energy_slow: 0.0,
            last_total: 0.0,
            last_centroid: 0.0,
            o_prev_x0: 0.0,
            o_prev_x1: 0.0,
            rms_ring: [0.0; RMS_WINDOW],
            rms_pos: 0,
            rms_fill: 0,
            centroid_ring: [0.0; RMS_WINDOW],
            cen_pos: 0,
            cen_fill: 0,
            hpss_ring: [0.0; N_GROUPS * HPSS_TIME_FRAMES],
            hpss_pos: 0,
            hpss_fill: 0,
            q_ring: vec![0.0; N_GROUPS * q_len],
            q_pos: 0,
            q_fill: 0,
            q_len,
            rep_ring: vec![0.0; n_lags * REPET_K],
            rep_pos: vec![0; n_lags],
            rep_fill: vec![0; n_lags],
            o_ring: vec![0.0; o_len],
            o_pos: 0,
            o_fill: 0,
            o_len,
            beat_stab: 0,
            beat_prev_lag: -1,
            q_bg: [0.0; N_GROUPS],
            q_bg_ready: false,
            q_bg_slow: [0.0; N_GROUPS],
            q_bg_slow_ready: false,
            novelty_slow: 0.0,
            med_t: [0.0; HPSS_TIME_FRAMES],
            med_f: [0.0; HPSS_FREQ_WIN],
            med_s: [0.0; RMS_WINDOW],
            lag_min,
            lag_max,
            n_lags,
            f_norm: 0.0,
            band_contrast: 0.0,
            wide: 0.0,
            novelty: 0.0,
            m_hpss: 0.0,
            harmonicity: 0.0,
            r_rep: 0.0,
            beat_conf: 0.0,
            beat_lock: false,
            stability: 1.0,
            am_depth: 0.0,
            w_low: 0.0,
            w_mid: 0.0,
            w_high: 0.0,
            t_score: 0.0,
            music_raw: 0.0,
            conf: 0.0,
            evt_score: 0.0,
            music_state: MUSIC_UNKNOWN,
            enter_hold_ms: 0.0,
            exit_hold_ms: 0.0,
            conf_drop_ms: 0.0,
            novel_ms: 0.0,
            quiet_ms: 0.0,
            gate: 0.0,
            evt_state: EVT_IDLE,
            evt_env: 0.0,
            refractory_until: 0.0,
            last_boost_ms: 0.0,
            release_since: 0.0,
            p_event_cache: 0.5,
            fault_streak: 0,
            degraded: false,
            cooldown_until: 0.0,
        }
    }

    /// 状态清空（故障恢复/重收敛后调用；保留帧时钟，B 的历史不受影响）。
    pub fn reset(&mut self) {
        let t = self.frame_time_ms;
        let rate = self.rate;
        *self = Self::new(rate);
        self.frame_time_ms = t;
    }

    /* ── 每帧特征（Analyzer 的 FFT hop 循环内调用，每帧一次） ── */

    /// 推入一个 FFT 帧并推进全部 C 状态机（帧率 ≈ rate/HOP）。
    ///
    /// 快速事件通路在这里推进，**不等待节奏锁**；慢通道（REPET/节拍/稳定度）
    /// 每 100ms 一次。零堆分配。
    pub fn push_frame(&mut self, f: &CFrameInput) {
        self.frames_seen = self.frames_seen.saturating_add(1);
        self.frame_time_ms += self.frame_dt_ms;
        self.last_total = f.total_rms;

        /* 输入有限值保护：坏帧直接跳过（连续坏帧在 fuse 计数降级） */
        if !all_finite(&[
            f.total_rms,
            f.vocal_rms,
            f.band_rms[0],
            f.band_rms[1],
            f.band_rms[2],
            f.band_rms[3],
            f.band_rms[4],
        ]) {
            return;
        }

        /* ① 逐带对数包络 / 慢基线 / 超基线余量 / 谱通量（文档 §4.2-§4.3） */
        let e_sum = f.band_rms.iter().sum::<f32>().max(1.0);
        let mut x = [0f32; 5];
        let mut flux_num = 0f32;
        let mut flux_den = 0f32;
        for i in 0..5 {
            x[i] = (1.0 + f.band_rms[i].max(0.0) / X_REF).ln();
            let df = (x[i] - self.x_prev[i]).max(0.0);
            flux_num += FLUX_W[i] * df;
            flux_den += FLUX_W[i] * x[i];
            self.x_base[i] += (x[i] - self.x_base[i]) * ALPHA_B;
            self.x_prev[i] = x[i];
        }
        self.f_norm = (flux_num / (flux_den + 1.0)).clamp(0.0, 1.0);
        let mut band_contrast = 0f32;
        let (mut w_low, mut w_mid, mut w_high) = (0f32, 0f32, 0f32);
        for i in 0..5 {
            let r_i = (f.band_rms[i] / e_sum).clamp(0.0, 1.0);
            let h_i = ((x[i] - self.x_base[i]) / self.x_base[i].max(H_FLOOR)).clamp(0.0, 1.0);
            band_contrast += r_i * h_i;
            match i {
                0 | 1 => w_low += r_i,
                2 => w_mid += r_i,
                _ => w_high += r_i,
            }
        }
        self.w_low = w_low;
        self.w_mid = w_mid;
        self.w_high = w_high;
        self.band_contrast = band_contrast;

        /* ② 谱平坦度 → 宽带分数（文档 §4.4；bin 限 20Hz-16kHz） */
        let bin_width = self.rate / (2.0 * f.power.len().max(1) as f32);
        let (mut flat_ln, mut flat_p, mut flat_n) = (0f32, 0f32, 0usize);
        let mut p_total = 0f32;
        for (k, &p) in f.power.iter().enumerate().skip(1) {
            let freq = k as f32 * bin_width;
            if freq < GROUP_LO_HZ || freq >= GROUP_HI_HZ {
                continue;
            }
            flat_ln += (p + 1e-9).ln();
            flat_p += p;
            flat_n += 1;
            p_total += p;
        }
        if flat_n > 0 && flat_p > 1e-9 {
            let sf = (flat_ln / flat_n as f32).exp() / (flat_p / flat_n as f32 + 1e-9);
            self.wide = ((sf - FLATNESS_LOW) / (FLATNESS_HIGH - FLATNESS_LOW).max(1e-6))
                .clamp(0.0, 1.0);
        } else {
            self.wide = 0.0;
        }
        /* 快速事件分数 T（文档 §4.5）：通量 + 逐带余量 + 宽带 */
        self.t_score = (0.45 * self.f_norm + 0.35 * self.band_contrast + 0.20 * self.wide)
            .clamp(0.0, 1.0);

        /* ③ HPSS 因果中值掩膜（文档 §3.4.3.1：时间中值=过去 9 帧，频率中值=当前帧 ±3 组） */
        for (g, &p) in f.group_power.iter().enumerate() {
            self.hpss_ring[self.hpss_pos * N_GROUPS + g] = (1.0 + p * 1e6).ln();
        }
        self.hpss_pos = (self.hpss_pos + 1) % HPSS_TIME_FRAMES;
        self.hpss_fill = (self.hpss_fill + 1).min(HPSS_TIME_FRAMES);
        let mut m_num = 0f32;
        let mut m_den = 0f32;
        for g in 0..N_GROUPS {
            let h_t = if self.hpss_fill >= HPSS_TIME_FRAMES {
                for (j, slot) in self.med_t.iter_mut().enumerate() {
                    let idx = (self.hpss_pos + HPSS_TIME_FRAMES - 1 - j) % HPSS_TIME_FRAMES;
                    *slot = self.hpss_ring[idx * N_GROUPS + g];
                }
                median_small(&mut self.med_t)
            } else {
                self.hpss_ring
                    [((self.hpss_pos + HPSS_TIME_FRAMES - 1) % HPSS_TIME_FRAMES) * N_GROUPS + g]
            };
            let lo = g.saturating_sub(HPSS_FREQ_WIN / 2);
            let hi = (g + HPSS_FREQ_WIN / 2).min(N_GROUPS - 1);
            let mut n = 0usize;
            for gg in lo..=hi {
                self.med_f[n] = (1.0 + f.group_power[gg] * 1e6).ln();
                n += 1;
            }
            let p_f = median_small(&mut self.med_f[..n]);
            let h_num = h_t.max(0.0).powf(HPSS_MASK_POWER);
            let p_num = p_f.max(0.0).powf(HPSS_MASK_POWER);
            let m_h = h_num / (h_num + p_num + 1e-9);
            let pw = f.group_power[g];
            m_num += pw * m_h;
            m_den += pw;
        }
        self.m_hpss = if m_den > 1e-9 {
            (m_num / m_den).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let m_p_hpss = 1.0 - self.m_hpss;

        /* ④ 谱新颖度（文档 §3.4.4.1：q_bg 慢均线的余弦相似）+ REPET q 历史 */
        let mut q_n = [0f32; N_GROUPS];
        let mut q_sq = 0f32;
        for (g, &p) in f.group_power.iter().enumerate() {
            q_n[g] = (1.0 + p * 1e6).ln();
            q_sq += q_n[g] * q_n[g];
        }
        let q_norm = q_sq.sqrt().max(1e-9);
        // REPET 用原始 L 向量的余弦相似（尺度不变，无需归一化）
        self.q_ring[self.q_pos * N_GROUPS..self.q_pos * N_GROUPS + N_GROUPS].copy_from_slice(&q_n);
        self.q_pos = (self.q_pos + 1) % self.q_len;
        self.q_fill = (self.q_fill + 1).min(self.q_len);
        if !self.q_bg_ready {
            self.q_bg = q_n;
            self.q_bg_ready = true;
        } else {
            let mut bg_sq = 0f32;
            for g in 0..N_GROUPS {
                self.q_bg[g] = self.q_bg[g] * (1.0 - NOVELTY_ALPHA) + q_n[g] * NOVELTY_ALPHA;
                bg_sq += self.q_bg[g] * self.q_bg[g];
            }
            let bg_norm = bg_sq.sqrt().max(1e-9);
            let mut sim = 0f32;
            for g in 0..N_GROUPS {
                sim += (q_n[g] / q_norm) * (self.q_bg[g] / bg_norm);
            }
            self.novelty = ((NOVELTY_HIGH - sim) / (NOVELTY_HIGH - NOVELTY_LOW).max(1e-6))
                .clamp(0.0, 1.0);
        }
        /* 慢通道新颖度（换曲解锁专用） */
        if !self.q_bg_slow_ready {
            self.q_bg_slow = q_n;
            self.q_bg_slow_ready = true;
            self.novelty_slow = 0.0;
        } else {
            let mut bg_sq = 0f32;
            for g in 0..N_GROUPS {
                self.q_bg_slow[g] =
                    self.q_bg_slow[g] * (1.0 - NOVELTY_SLOW_ALPHA) + q_n[g] * NOVELTY_SLOW_ALPHA;
                bg_sq += self.q_bg_slow[g] * self.q_bg_slow[g];
            }
            let bg_norm = bg_sq.sqrt().max(1e-9);
            let mut sim = 0f32;
            for g in 0..N_GROUPS {
                sim += (q_n[g] / q_norm) * (self.q_bg_slow[g] / bg_norm);
            }
            self.novelty_slow =
                ((NOVELTY_HIGH - sim) / (NOVELTY_HIGH - NOVELTY_LOW).max(1e-6)).clamp(0.0, 1.0);
        }

        /* ⑤ 粗谐波性（文档 §3.4.3.3：36 个 F0 候选 × 1-6 次谐波，主瓣 3 bin 取峰） */
        let mut h_best = 0f32;
        if p_total > 1e-9 {
            let half = f.power.len();
            for fi in 0..HARM_F0_N {
                let f0 =
                    HARM_F0_MIN * (HARM_F0_MAX / HARM_F0_MIN).powf(fi as f32 / (HARM_F0_N - 1) as f32);
                let mut harm = 0f32;
                for m in 1..=HARM_PARTIALS {
                    let target = m as f32 * f0;
                    if target >= GROUP_HI_HZ {
                        break;
                    }
                    let b = (target / bin_width).round() as usize;
                    if b == 0 || b >= half {
                        continue;
                    }
                    let lo = b.saturating_sub(1);
                    let hi = (b + 1).min(half - 1);
                    let peak = f.power[lo].max(f.power[b]).max(f.power[hi]);
                    harm += peak / (m as f32).sqrt();
                }
                h_best = h_best.max(harm);
            }
            self.harmonicity = (h_best / p_total).clamp(0.0, 1.0);
        } else {
            self.harmonicity = 0.0;
        }

        /* ⑥ onset 包络（低频带正差分，文档 §3.4.3.4）+ 谱质心（稳定度用） */
        let o = (0.55 * x[0] + 0.45 * x[1] - 0.55 * self.o_prev_x0 - 0.45 * self.o_prev_x1)
            .max(0.0);
        self.o_prev_x0 = x[0];
        self.o_prev_x1 = x[1];
        self.o_ring[self.o_pos] = o;
        self.o_pos = (self.o_pos + 1) % self.o_len;
        self.o_fill = (self.o_fill + 1).min(self.o_len);
        let mut cen_num = 0f32;
        for (k, &p) in f.power.iter().enumerate().skip(1) {
            let freq = k as f32 * bin_width;
            if freq >= GROUP_LO_HZ && freq < GROUP_HI_HZ {
                cen_num += freq * p;
            }
        }
        let cen = if p_total > 1e-9 { cen_num / p_total } else { self.last_centroid };
        self.last_centroid = cen;
        self.centroid_ring[self.cen_pos] = cen;
        self.cen_pos = (self.cen_pos + 1) % RMS_WINDOW;
        self.cen_fill = (self.cen_fill + 1).min(RMS_WINDOW);

        /* ⑦ 总能量短窗（AM 深度）+ 慢尾巴（p_event 的干湿代理） */
        self.rms_ring[self.rms_pos] = f.total_rms;
        self.rms_pos = (self.rms_pos + 1) % RMS_WINDOW;
        self.rms_fill = (self.rms_fill + 1).min(RMS_WINDOW);
        self.energy_slow = self.energy_slow * 0.94 + f.total_rms * 0.06;

        /* ⑧ 慢通道（每 100ms）：REPET / 节拍 / 稳定度（摊销 CPU） */
        if self.frame_time_ms - self.last_slow_ms >= SLOW_UPDATE_MS {
            self.last_slow_ms = self.frame_time_ms;
            self.slow_channel();
        }

        /* ⑨ 组合分数 + 语音保护（文档 §3.4.4.1/§3.4.4.2） */
        let e_event = (W_EVT_FLUX * self.f_norm
            + W_EVT_WIDE * self.wide
            + W_EVT_M_P * m_p_hpss
            + W_EVT_NOVEL * self.novelty)
            .clamp(0.0, 1.0);
        self.evt_score = e_event;
        let n_narrow = 1.0 - self.wide;
        let m_b = if f.mode_b == super::mapping::MODE_MUSIC { 1.0 } else { 0.0 };
        let c_bg = (W_M_B * m_b
            + W_M_HPSS * self.m_hpss
            + W_R_REP * self.r_rep_eff()
            + W_B_BEAT * self.beat_conf
            + W_H_HARM * self.harmonicity
            + W_N_NARROW * n_narrow
            + W_S_STABLE * self.stability)
            .clamp(0.0, 1.0);
        let v_voice = (0.50 * f.vocal_rms / (f.total_rms + 1.0)
            + 0.30 * self.am_depth
            + 0.20 * (1.0 - w_low))
            .clamp(0.0, 1.0);
        let raw = (c_bg * (1.0 - EVT_ERODE * e_event) * (1.0 - VOICE_GUARD * v_voice))
            .clamp(0.0, 1.0);
        /* 证据一致性（文档 §3.4.4.2）：特征互相矛盾时 conf→0，保守不压 */
        let u = [
            self.m_hpss,
            self.r_rep_eff(),
            self.harmonicity,
            self.beat_conf,
            n_narrow,
            self.stability,
        ];
        let mean_u = u.iter().sum::<f32>() / u.len() as f32;
        let var_u = u.iter().map(|v| (v - mean_u) * (v - mean_u)).sum::<f32>() / u.len() as f32;
        let agree = 1.0 - var_u.sqrt().clamp(0.0, 1.0);
        self.music_raw = raw;
        self.conf = (agree * (0.55 + 0.45 * (2.0 * raw - 1.0).abs())).clamp(0.0, 1.0);

        /* ⑩ 音乐锁状态机（文档 §3.4.4.3） */
        self.music_machine(f);

        /* ⑪ 事件状态机（文档 §4.5）+ 包络衰减（§4.9） */
        self.event_machine();
    }

    /// 音乐锁状态机（进入门槛高、退出快；换曲/静音快速重收敛）。
    fn music_machine(&mut self, f: &CFrameInput) {
        let quiet = f.total_rms < QUIET_TOTAL;
        self.quiet_ms = if quiet { self.quiet_ms + self.frame_dt_ms } else { 0.0 };

        /* 换曲解锁：慢通道新颖度持续 ≥150ms → 清旧历史 + 进 UNKNOWN。
         * (文档「持续 2 帧」≈21ms 会被 BGM 自身鼓点反复触发；快通道新颖度又收敛
         *  太快抓不住换曲 —— 双通道分工: 快=E_event, 慢=解锁, 偏差记录 #12) */
        if self.novelty_slow >= NOVELTY_UNLOCK {
            self.novel_ms += self.frame_dt_ms;
            if self.novel_ms >= UNLOCK_SUSTAIN_MS && self.music_state == MUSIC_LOCK {
                self.music_state = MUSIC_UNKNOWN;
                self.clear_rhythm_history();
            }
        } else {
            self.novel_ms = 0.0;
        }
        /* 静音清空（120ms） */
        if self.quiet_ms >= QUIET_RESET_MS {
            if self.music_state == MUSIC_LOCK {
                self.music_state = MUSIC_UNKNOWN;
            }
            self.clear_rhythm_history();
            self.quiet_ms = 0.0;
        }

        match self.music_state {
            MUSIC_LOCK => {
                if self.music_raw <= MUSIC_EXIT {
                    self.exit_hold_ms += self.frame_dt_ms;
                    if self.exit_hold_ms >= MUSIC_EXIT_HOLD_MS {
                        self.music_state = MUSIC_UNKNOWN;
                        self.exit_hold_ms = 0.0;
                    }
                } else {
                    self.exit_hold_ms = 0.0;
                }
                if self.conf < MUSIC_CONF_DROP {
                    self.conf_drop_ms += self.frame_dt_ms;
                    if self.conf_drop_ms >= UNLOCK_SUSTAIN_MS {
                        self.music_state = MUSIC_UNKNOWN;
                        self.conf_drop_ms = 0.0;
                    }
                } else {
                    self.conf_drop_ms = 0.0;
                }
            }
            _ => {
                let periodic = self.r_rep >= BEAT_CORR_ON || self.beat_conf >= BEAT_CORR_ON;
                if self.music_raw >= MUSIC_ENTER && self.conf >= MUSIC_CONF_MIN && periodic {
                    self.enter_hold_ms += self.frame_dt_ms;
                    if self.enter_hold_ms >= MUSIC_ENTER_HOLD_MS {
                        self.music_state = MUSIC_LOCK;
                        self.enter_hold_ms = 0.0;
                    }
                } else {
                    self.enter_hold_ms *= 0.7; // 慢衰减，临界抖动不掉光
                }
            }
        }
        /* 门控平滑（上升快、退出留尾巴防马达阶跃） */
        let target = if self.music_state == MUSIC_LOCK {
            (self.music_raw * self.conf).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let alpha = if target > self.gate { GATE_UP } else { GATE_DOWN };
        self.gate += (target - self.gate) * alpha;
    }

    /// 事件状态机（onset/hold/release + refractory；包络每帧指数衰减）。
    fn event_machine(&mut self) {
        let theta_off = THETA_ON * THETA_OFF_RATIO;
        let tau = (TAU_NEAR_MS + (TAU_FAR_MS - TAU_NEAR_MS) * (1.0 - self.p_event_cache))
            .clamp(TAU_NEAR_MS, TAU_FAR_MS);
        self.evt_env *= (-self.frame_dt_ms / tau.max(1.0)).exp();
        if self.evt_env < 1e-4 {
            self.evt_env = 0.0;
        }
        let t = self.t_score;
        match self.evt_state {
            EVT_IDLE => {
                if t >= THETA_ON && self.frame_time_ms >= self.refractory_until {
                    self.evt_env = self.evt_env.max(self.peak_of(t));
                    self.evt_state = EVT_ATTACK;
                    self.refractory_until = self.frame_time_ms + REFRACTORY_MS;
                    self.last_boost_ms = self.frame_time_ms;
                }
            }
            /* 首版 1 帧确认（文档 §4.5 默认；极短冲击不损失） */
            EVT_ATTACK => {
                self.evt_state = EVT_HOLD;
                self.release_since = self.frame_time_ms;
            }
            EVT_HOLD => {
                if t >= THETA_ON {
                    self.evt_env = self.evt_env.max(self.peak_of(t)); // 更新峰值不重复起振
                    self.last_boost_ms = self.frame_time_ms;
                } else if t <= theta_off
                    || self.frame_time_ms - self.last_boost_ms > HOLD_TIMEOUT_MS
                {
                    self.evt_state = EVT_RELEASE;
                    self.release_since = self.frame_time_ms;
                }
            }
            _ => {
                if self.evt_env < ENV_END
                    || self.frame_time_ms - self.release_since > HOLD_TIMEOUT_MS
                {
                    self.evt_state = EVT_IDLE;
                }
            }
        }
    }

    /// 事件峰值强度（文档 §4.9 I_event；event_gain 在融合时乘一次，不在此重复）。
    fn peak_of(&self, t: f32) -> f32 {
        let base = ((t - THETA_ON) / (1.0 - THETA_ON).max(1e-6)).clamp(0.0, 1.0);
        (base * self.p_event_cache).clamp(0.0, 1.0)
    }

    /* ── 慢通道（100ms 一次；全部用预分配缓冲，零堆分配） ── */

    fn slow_channel(&mut self) {
        /* AM 深度（32 帧 RMS 环的 std/mean） */
        let n = self.rms_fill;
        let (mean, std) = if n == 0 {
            (0f32, 0f32)
        } else {
            let m = self.rms_ring.iter().take(n).sum::<f32>() / n as f32;
            let v = self.rms_ring.iter().take(n).map(|x| (x - m) * (x - m)).sum::<f32>() / n as f32;
            (m, v.sqrt())
        };
        self.am_depth = (std / (mean + 1.0)).clamp(0.0, 1.0);

        /* REPET-SIM 轻量化：当前帧 q 与所有候选滞后帧的余弦相似（文档 §3.4.3.2） */
        if self.q_fill >= 4 {
            let cur = (self.q_pos + self.q_len - 1) % self.q_len; // 最新帧槽位
            let mut q_sq = 0f32;
            for g in 0..N_GROUPS {
                q_sq += self.q_ring[cur * N_GROUPS + g] * self.q_ring[cur * N_GROUPS + g];
            }
            let q_norm = q_sq.sqrt();
            let mut best = 0f32;
            for li in 0..self.n_lags {
                let lag = self.lag_min + li;
                if lag >= self.q_fill {
                    break;
                }
                let idx = (self.q_pos + self.q_len - 1 - lag) % self.q_len;
                let mut dot = 0f32;
                let mut h_sq = 0f32;
                for g in 0..N_GROUPS {
                    let qv = self.q_ring[idx * N_GROUPS + g];
                    dot += self.q_ring[cur * N_GROUPS + g] * qv;
                    h_sq += qv * qv;
                }
                let d = 1.0 - dot / (q_norm * h_sq.sqrt() + 1e-9);
                let r = (-d.max(0.0) / REPET_SIGMA).exp();
                let base = li * REPET_K;
                self.rep_ring[base + self.rep_pos[li] as usize] = r;
                self.rep_pos[li] = ((self.rep_pos[li] as usize + 1) % REPET_K) as u32;
                self.rep_fill[li] = (self.rep_fill[li] + 1).min(REPET_K as u32);
                if self.rep_fill[li] as usize == REPET_K {
                    let seg = &self.rep_ring[base..base + REPET_K];
                    let m = seg.iter().sum::<f32>() / REPET_K as f32;
                    let v = seg.iter().map(|x| (x - m) * (x - m)).sum::<f32>() / REPET_K as f32;
                    best = best.max((m - 0.5 * v.sqrt()).clamp(0.0, 1.0));
                }
            }
            self.r_rep = best;
        }

        /* 节拍：低频 onset 包络归一化自相关（文档 §3.4.3.4） */
        if self.o_fill >= self.lag_min + 8 {
            let n = self.o_fill;
            // back=0 表示最新一帧
            let val = |ring: &[f32], pos: usize, cap: usize, back: usize| -> f32 {
                ring[(pos + cap - 1 - back) % cap]
            };
            let mut sum2 = 0f32;
            for i in 0..n {
                let v = val(&self.o_ring, self.o_pos, self.o_len, i);
                sum2 += v * v;
            }
            let _ = sum2; // 归一化分母按每 lag 的重叠段独立计算（无偏）
            let mut best_a = 0f32;
            let mut best_lag = -1i32;
            for li in 0..self.n_lags {
                let lag = self.lag_min + li;
                if lag >= n {
                    break;
                }
                let (mut num, mut da, mut db) = (0f32, 0f32, 0f32);
                for i in lag..n {
                    let a = val(&self.o_ring, self.o_pos, self.o_len, i);
                    let b = val(&self.o_ring, self.o_pos, self.o_len, i - lag);
                    num += a * b;
                    da += a * a;
                    db += b * b;
                }
                let a = num / (da.sqrt() * db.sqrt() + 1e-9);
                if a > best_a {
                    best_a = a;
                    best_lag = lag as i32;
                }
            }
            /* 旁瓣比：排除主峰 ±12% 邻域后的最大相关（抗宽带误锁） */
            let mut side = 0f32;
            if best_lag > 0 {
                let lo = ((best_lag as f32) * 0.88) as usize;
                let hi = ((best_lag as f32) * 1.12) as usize;
                for li in 0..self.n_lags {
                    let lag = self.lag_min + li;
                    if lag >= n || (lag >= lo && lag <= hi) {
                        continue;
                    }
                    let (mut num, mut da, mut db) = (0f32, 0f32, 0f32);
                    for i in lag..n {
                        let a = val(&self.o_ring, self.o_pos, self.o_len, i);
                        let b = val(&self.o_ring, self.o_pos, self.o_len, i - lag);
                        num += a * b;
                        da += a * a;
                        db += b * b;
                    }
                    let a = num / (da.sqrt() * db.sqrt() + 1e-9);
                    side = side.max(a);
                }
            }
            let side_ratio = if best_a > 1e-6 { (side / best_a).clamp(0.0, 1.0) } else { 0.0 };
            /* 周期一致性：连续更新 lag 漂移 ≤8% 累加（文档 period_stability） */
            if best_lag > 0 && self.beat_prev_lag > 0 {
                let prev = self.beat_prev_lag as f32;
                if (best_lag as f32 - prev).abs() / prev.max(1.0) <= LAG_STABILITY {
                    self.beat_stab = (self.beat_stab + 1).min(BEAT_LOCK_UPDATES * 4);
                } else {
                    self.beat_stab = 0;
                }
            }
            self.beat_prev_lag = best_lag;
            let stab = (self.beat_stab as f32 / BEAT_LOCK_UPDATES as f32).clamp(0.0, 1.0);
            self.beat_conf = (0.60 * best_a + 0.25 * stab + 0.15 * (1.0 - side_ratio)).clamp(0.0, 1.0);
            self.beat_lock = best_a >= BEAT_CORR_ON && self.beat_stab >= BEAT_LOCK_UPDATES;
        }

        /* 稳定度：能量/质心短窗 MAD（文档 §3.4.4.1 S_stable；med_s 复用零分配） */
        let n = self.rms_fill.min(self.cen_fill);
        if n >= 8 {
            let mean_e = self.rms_ring.iter().take(n).sum::<f32>() / n as f32;
            let mean_c = self.centroid_ring.iter().take(n).sum::<f32>() / n as f32;
            for i in 0..n {
                self.med_s[i] = self.rms_ring[i];
            }
            let me = median_small(&mut self.med_s[..n]);
            for i in 0..n {
                self.med_s[i] = (self.rms_ring[i] - me).abs();
            }
            let mad_e = median_small(&mut self.med_s[..n]);
            for i in 0..n {
                self.med_s[i] = self.centroid_ring[i];
            }
            let mc = median_small(&mut self.med_s[..n]);
            for i in 0..n {
                self.med_s[i] = (self.centroid_ring[i] - mc).abs();
            }
            let mad_c = median_small(&mut self.med_s[..n]);
            let r = 0.5 * (mad_e / mean_e.max(1.0)) + 0.5 * (mad_c / mean_c.max(1.0));
            self.stability = 1.0 - r.clamp(0.0, 1.0);
        }
    }

    /// REPET 证据软门槛（低于 c_repet_similarity_on=0.68 时按比例衰减）。
    fn r_rep_eff(&self) -> f32 {
        if self.r_rep >= REPET_SIM_ON {
            self.r_rep
        } else {
            self.r_rep * self.r_rep / REPET_SIM_ON
        }
    }

    /// 清空节奏/重复历史（换曲、静音、故障恢复；文档 §3.4.4.3）。
    fn clear_rhythm_history(&mut self) {
        self.q_ring.fill(0.0);
        self.q_pos = 0;
        self.q_fill = 0;
        self.rep_ring.fill(0.0);
        self.rep_fill.fill(0);
        self.rep_pos.fill(0);
        self.r_rep = 0.0;
        self.o_ring.fill(0.0);
        self.o_pos = 0;
        self.o_fill = 0;
        self.beat_stab = 0;
        self.beat_prev_lag = -1;
        self.beat_conf = 0.0;
        self.beat_lock = false;
        self.q_bg.fill(0.0);
        self.q_bg_ready = false; // 新颖度基线随新场景重累积
        self.q_bg_slow.fill(0.0);
        self.q_bg_slow_ready = false;
        self.novelty_slow = 0.0;
        self.novel_ms = 0.0;
    }

    /* ── 每 tick 融合（B 目标之上；引擎线程每 10ms 调一次） ── */

    /// 融合：读取 B 目标（已过 A 映射 + B 智能层），做 BGM 门控 + 事件增强。
    ///
    /// 返回 `active=false` 时调用方必须**原样保留 B 目标**（无损回退语义）。
    pub fn fuse(
        &mut self,
        b_l: f32,
        b_r: f32,
        smart: &SmartOutput,
        pan_l: f32,
        pan_r: f32,
        dual_motor: bool,
        p: &CParams,
    ) -> COut {
        let mut out = COut::default();
        out.music_raw = self.music_raw;
        out.conf = self.conf;
        out.music_state = self.music_state;
        out.beat_lock = self.beat_lock;
        out.event_state = self.evt_state;
        out.event_score = self.evt_score;
        out.degraded = self.degraded;

        /* 旁路 1：升级强度 0 = 无损回 B（PR3 验收口径） */
        if p.upgrade_pct == 0 {
            return out;
        }
        /* 旁路 2：故障冷却中 */
        if self.degraded {
            if self.frame_time_ms >= self.cooldown_until {
                self.degraded = false;
                self.reset_history_keep_time();
                out.degraded = false;
            } else {
                return out;
            }
        }
        /* 旁路 3：输入异常 → 连续 3 帧降级（文档 §5.5） */
        let smart_ok = smart.gain.is_finite()
            && smart.left_bias.is_finite()
            && smart.right_bias.is_finite()
            && smart.click.is_finite()
            && smart.proximity.is_finite();
        if !all_finite(&[b_l, b_r, pan_l, pan_r]) || !smart_ok {
            self.fault_streak += 1;
            if self.fault_streak >= FAULT_TRIP {
                self.degraded = true;
                self.cooldown_until = self.frame_time_ms + FAULT_COOLDOWN_MS;
                out.degraded = true;
            }
            return out;
        }
        self.fault_streak = 0;
        /* 旁路 4：暖机不足只输出 B（不延迟 B 起振） */
        if self.frames_seen < WARMUP_FRAMES {
            return out;
        }
        let u_c = (p.upgrade_pct.min(100) as f32) / 100.0;
        let m_sustain = (p.sustain_suppress_pct.min(100) as f32) / 100.0;

        /* p_event（文档 §4.7）：B 距离感 + 当前事件分 + 干湿（tail = B 的尾巴代理） */
        let tail = (self.energy_slow / (self.last_total + 1.0)).clamp(0.0, 1.0);
        let p_event = (P_EVT_PROX * smart.proximity.clamp(0.0, 1.0)
            + P_EVT_T * self.t_score
            + P_EVT_TAIL * (1.0 - tail))
            .clamp(0.0, 1.0);
        self.p_event_cache = p_event;

        /* ① 背景/事件双分量门控（文档 §3.4.4.4；只影响 C 融合，不改 B 原始电平） */
        let e = (self.evt_score / EVENT_SPLIT_THR).clamp(0.0, 1.0);
        let (mut fl, mut fr) = (b_l, b_r);
        if p.bgm_filter {
            let (b_evt_l, b_evt_r) = (b_l * e, b_r * e);
            let (b_sus_l, b_sus_r) = (b_l - b_evt_l, b_r - b_evt_r);
            let g_sus = 1.0 - m_sustain * self.gate;
            let escape = (WIDEBAND_ESCAPE * self.wide
                + 0.25 * self.novelty
                + 0.20 * (1.0 - self.beat_conf))
                .clamp(0.0, 1.0);
            let g_evt = 1.0 - EVENT_SUPPRESS * self.gate * (1.0 - escape);
            fl = b_sus_l * g_sus + b_evt_l * g_evt;
            fr = b_sus_r * g_sus + b_evt_r * g_evt;
        }

        /* ② 事件级路由（文档 §4.8）：频带路由为主，按 c_stereo_event_pct 插值 pan */
        let rl_freq =
            (ROUTE_L_LOW * self.w_low + ROUTE_L_MID * self.w_mid + ROUTE_L_HIGH * self.w_high)
                .clamp(0.0, 1.0);
        let rr_freq =
            (ROUTE_L_LOW * self.w_high + ROUTE_L_MID * self.w_mid + ROUTE_L_HIGH * self.w_low)
                .clamp(0.0, 1.0);
        let mut route_l =
            ((1.0 - STEREO_EVENT_PCT) * rl_freq + STEREO_EVENT_PCT * rl_freq * pan_l.clamp(0.6, 1.4) / 1.4)
                .clamp(0.0, 1.0);
        let mut route_r =
            ((1.0 - STEREO_EVENT_PCT) * rr_freq + STEREO_EVENT_PCT * rr_freq * pan_r.clamp(0.6, 1.4) / 1.4)
                .clamp(0.0, 1.0);
        if !dual_motor {
            let sync = (route_l + route_r) * 0.5; // 同步模式语义：双侧同量（文档 §4.8）
            route_l = sync;
            route_r = sync;
        }

        /* ③ headroom 有界加法（文档 §4.9）+ 事件攻放上调量 */
        let add_l = u_c * EVENT_GAIN * self.evt_env * route_l;
        let add_r = u_c * EVENT_GAIN * self.evt_env * route_r;
        let t_l = (fl + add_l * (1.0 - fl)).clamp(0.0, 1.0);
        let t_r = (fr + add_r * (1.0 - fr)).clamp(0.0, 1.0);
        if !t_l.is_finite() || !t_r.is_finite() {
            self.fault_streak += 1;
            if self.fault_streak >= FAULT_TRIP {
                self.degraded = true;
                self.cooldown_until = self.frame_time_ms + FAULT_COOLDOWN_MS;
                out.degraded = true;
            }
            return out;
        }
        out.active = true;
        out.target_l = t_l;
        out.target_r = t_r;
        out.attack_add = 0.15 * u_c * self.evt_env; // 文档 §4.9（加性上调，调用方 clamp）
        out.release_add = 0.05 * u_c * (1.0 - self.evt_env);
        out
    }

    /// 故障恢复：只复位门控/事件状态机（冷却期 push_frame 仍在喂数据，特征历史
    /// 依然新鲜，无需重暖机）；帧时钟保留，B 不受影响。
    fn reset_history_keep_time(&mut self) {
        self.music_state = MUSIC_UNKNOWN;
        self.gate = 0.0;
        self.evt_state = EVT_IDLE;
        self.evt_env = 0.0;
        self.enter_hold_ms = 0.0;
        self.exit_hold_ms = 0.0;
        self.fault_streak = 0;
    }
}

/* ═══════════════ 偏差记录（相对设计文档，见交接文档详情） ═══════════════
 * 1. §4.9 的 event_gain 在 I_event 与融合式各出现一次 —— 本实现只在融合乘一次，
 *    避免双重缩放（文档笔误倾向）。
 * 2. §4.5 的 merge_ms=35 未单独实现：refractory(65ms) > merge(35ms)，合并语义被
 *    refractory + HOLD 峰值更新完全覆盖。
 * 3. §4.5 的 hold_timeout 语义按「HOLD 内无再增强 300ms → RELEASE」实现（文档原文
 *    「没有超过 theta_off 的帧」与 release 即时条件重复，按保守口径取无再增强）。
 * 4. §3.4.6 的 c_psy_mask_pct（心理声学后处理）本轮未实现：引擎输出只有左右两路，
 *    分带掩蔽无落点；留待分带输出路由后评估。
 * 5. §3.4.6 的 c_uncertain_sustain_pct（UNKNOWN 轻压）首版默认不启用（文档同口径）。
 * 6. 逐带底噪按文档允许的近似：用总 noise_floor 的固定参考 X_REF=30 替代每带分配
 *    （r_i→0 时按比例分配会退化），事件余量主要由逐带慢基线 b_i 承担。
 * 7. c_hpss_time_frames=9 为帧数口径：96kHz 下时间窗缩短到 ~48ms（帧率翻倍），
 *    lag/历史窗按 ms 换算不受影响；48k 主流场景与文档一致。
 * 8. 细参（HPSS/REPET/谐波/节拍/平坦度等 30+ 项）为文件内常数，取文档 §3.4.6/§4.12
 *    设计默认值；config 只暴露 mode + c_upgrade_pct + c_sustain_suppress_pct +
 *    c_bgm_filter 四项（文档 §4.12 明示「是否落配置在实施 PR 评审时确认」）。
 * 9. attack/release 修饰按文档 §4.9 的「加性上调」实现（attack + 0.15*u_c*env），
 *    若实测峰值变钝可退回纯事件包络（文档预留的待验证开关）。
 * 10. 事件增强 add-on 按文档 §4.9 原式不乘 p_gate：锁定期间规律鼓点的压制依赖
 *     G_event（作用在 b_evt 分量），add-on 只受 u_c/g_event/env 约束 —— 若实机
 *     观察到锁定后鼓点仍偏强，第一调参点是 c_upgrade_pct / c_event_gain。
 * 11. MUSIC_LOCK 阈值校准（0.72→0.58 进入 / 0.60→0.42 置信线 / 一致性分母
 *     0.35→0.45）：文档公式对理想电子 BGM 的数学上限 conf≈0.49 < 0.60，锁永不
 *     触发；校准后强周期 BGM ~1-2s 内可锁，随机/非周期内容 raw≈0.1 依旧够不到
 *     进入线，保守性由「重复性或节拍 ≥0.55」硬门槛 + 四条快速退出通道兜底。
 * 12. 快速退出通道的「持续 2 帧」改为「持续 150ms」（新颖度/置信度两条）：合成与
 *     推演均显示 21ms 会被 BGM 自身鼓点的瞬时谱变化反复触发 → 节奏历史被清空 →
 *     进入保持永远凑不齐（锁定极限循环）。且快背景（alpha=0.08，半衰 87ms）的
 *     新颖度窗口只有 ~70ms，抓不住换曲 —— 故新增慢通道新颖度（alpha=0.02）专职
 *     换曲解锁：周期鼓点在慢背景中本就是成分，天然免疫；换曲持续偏移 ≥400ms。
 */

/* ═══════════════ 合成信号单测（任务验收口径） ═══════════════ */

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio_haptics::mapping::{MODE_MUSIC, MODE_NORMAL};

    const RATE: f32 = 48000.0;
    const HALF: usize = 512;
    /// RMS_NORM 同口径 (dsp.rs 私有常量的测试复刻): 0.4330127 * FFT_SIZE。
    const TEST_RMS_NORM: f32 = 0.433_012_7 * 1024.0;

    /// 合成频谱帧: bin 级功率 → (power, 5 带 RMS int16 尺度, 24 组功率, 总 RMS)。
    /// 量纲自洽: 带 RMS / 总 RMS 均从同一 bins 按 Parseval 口径推得 (与 dsp.rs 一致)。
    fn synth(bins: &[(usize, f32)]) -> ([f32; HALF], [f32; 5], [f32; N_GROUPS], f32) {
        let mut power = [0f32; HALF];
        for &(b, p) in bins {
            power[b] += p;
        }
        let bin_hz = RATE / 1024.0;
        let mut band_sq = [0f32; 5];
        let edges: Vec<f32> = (0..=N_GROUPS)
            .map(|g| {
                GROUP_LO_HZ * (GROUP_HI_HZ / GROUP_LO_HZ).powf(g as f32 / N_GROUPS as f32)
            })
            .collect();
        let mut grp = [0f32; N_GROUPS];
        let mut p_total = 0f32;
        for b in 1..HALF {
            let f = b as f32 * bin_hz;
            p_total += power[b];
            if f >= GROUP_LO_HZ && f < GROUP_HI_HZ {
                for g in 0..N_GROUPS {
                    if f >= edges[g] && f < edges[g + 1] {
                        grp[g] += power[b];
                        break;
                    }
                }
            }
            // 5 显示带边界 (dsp::BAND_EDGES)
            let band_edge = [20.0f32, 80.0, 250.0, 2000.0, 8000.0, 24000.0];
            for (i, w) in band_edge.windows(2).enumerate() {
                if f >= w[0] && f < w[1] {
                    band_sq[i] += power[b];
                    break;
                }
            }
        }
        let band_rms = band_sq.map(|sq| sq.sqrt() / TEST_RMS_NORM * 32768.0);
        let total = p_total.sqrt() / TEST_RMS_NORM * 32768.0;
        (power, band_rms, grp, total)
    }

    /// BGM 音色 A: 234Hz+328Hz 和声铺底 (bin 5/7)。
    fn bgm_a_bins() -> Vec<(usize, f32)> {
        vec![(5, 2000.0), (6, 400.0), (7, 900.0), (8, 150.0)]
    }
    /// BGM 音色 B (换曲用): 音调上移 + 周期不同。
    fn bgm_b_bins() -> Vec<(usize, f32)> {
        vec![(13, 1800.0), (14, 600.0), (19, 800.0), (20, 120.0)]
    }
    /// 鼓点: 低带两 bin 短促冲击。
    fn kick_bins() -> Vec<(usize, f32)> {
        vec![(1, 3000.0), (2, 1500.0)]
    }
    /// 宽带技能音: 全带噪声型 (平坦度高 → wide≈1)。
    fn skill_bins() -> Vec<(usize, f32)> {
        let mut v = Vec::new();
        let mut s = 0x9E3779B9u64;
        for b in 2..HALF - 2 {
            s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let amp = ((s >> 33) as f32 / u32::MAX as f32) * 900.0 + 100.0;
            v.push((b, amp));
        }
        v
    }

    /// 喂一帧 (复用 synth; mode_b 由调用方给)。
    fn feed(c: &mut CMode, bins: &[(usize, f32)], mode_b: u8) {
        let (power, band, grp, total) = synth(bins);
        c.push_frame(&CFrameInput {
            band_rms: band,
            group_power: &grp,
            power: &power,
            total_rms: total,
            vocal_rms: 0.0,
            mode_b,
        });
    }

    /// 周期 BGM 序列: 每 kick_period 帧打 3 帧鼓点。
    fn feed_bgm(c: &mut CMode, frames: usize, base: &[(usize, f32)], kick_period: usize) {
        for i in 0..frames {
            let mut bins: Vec<(usize, f32)> = base.iter().map(|&(b, p)| (b, p)).collect();
            if kick_period > 0 && i % kick_period < 3 {
                bins.extend(kick_bins());
            }
            feed(c, &bins, MODE_MUSIC);
        }
    }

    fn near_smart() -> SmartOutput {
        SmartOutput {
            active: true,
            gain: 1.0,
            attack_scale: 1.0,
            release_scale: 1.0,
            left_bias: 1.0,
            right_bias: 1.0,
            click: 0.0,
            proximity: 0.8,
            pitch: 0.5,
            duration_ms: 100.0,
        }
    }

    fn cparams(upgrade: u32, sustain: u32, filter: bool) -> CParams {
        CParams { upgrade_pct: upgrade, sustain_suppress_pct: sustain, bgm_filter: filter }
    }

    /// 静音帧 (全零; 安静段语义)。
    fn feed_silence(c: &mut CMode, frames: usize) {
        for _ in 0..frames {
            let (power, band, grp, total) = synth(&[]);
            c.push_frame(&CFrameInput {
                band_rms: band,
                group_power: &grp,
                power: &power,
                total_rms: total,
                vocal_rms: 0.0,
                mode_b: 3, // MODE_QUIET
            });
        }
    }

    /* ── 锁定 + BGM 压制 (招牌能力) ── */

    #[test]
    fn bgm_locks_and_sustain_is_suppressed() {
        let mut c = CMode::new(RATE);
        // 250ms 周期鼓点 (23 帧 @10.67ms) + 和声铺底 → 多证据收敛
        let base = [(5usize, 2000.0f32), (6, 400.0), (7, 900.0), (8, 150.0)];
        feed_bgm(&mut c, 320, &base, 23); // ≈3.4s
        assert_eq!(c.music_state, MUSIC_LOCK, "强周期 BGM 应在 ~2s 内锁定");
        assert!(c.gate > 0.3, "锁定后门控应建立: gate={}", c.gate);
        assert!(c.beat_conf > BEAT_CORR_ON, "节拍证据应达标: {}", c.beat_conf);

        // 锁定后: 持续层应被压低 (取鼓点间隙帧, 事件包络已衰减)
        // 先推进到鼓点间隙深处 (20 帧 ≈ 213ms, 包络 τ≤200ms 已收敛)
        for _ in 0..20 {
            feed(&mut c, &base, MODE_MUSIC);
        }
        let b = (0.5f32, 0.5f32);
        let gated = c.fuse(b.0, b.1, &near_smart(), 1.0, 1.0, false, &cparams(100, 90, true));
        assert!(gated.active, "锁定后 C 层应生效");
        let passthru = c.fuse(b.0, b.1, &near_smart(), 1.0, 1.0, false, &cparams(100, 90, false));
        assert!(
            gated.target_l < passthru.target_l * 0.5,
            "BGM 锁定应把持续层压到未筛选的一半以下: gated={:.3} passthru={:.3}",
            gated.target_l,
            passthru.target_l
        );
        assert!(
            gated.target_l < b.0 * 0.4,
            "持续层应显著低于 B 基线: {:.3} vs {:.3}",
            gated.target_l,
            b.0
        );
    }

    /* ── 瞬态技能音逃逸 ── */

    #[test]
    fn wideband_transient_survives_during_lock() {
        let mut c = CMode::new(RATE);
        let base = [(5usize, 2000.0f32), (6, 400.0), (7, 900.0), (8, 150.0)];
        feed_bgm(&mut c, 320, &base, 23);
        assert_eq!(c.music_state, MUSIC_LOCK);
        // 锁定中注入宽带技能音 (噪声型 → wide≈1 → escape 高)
        let skill = skill_bins();
        feed(&mut c, &skill, MODE_NORMAL);
        let b = (0.8f32, 0.8f32);
        let out = c.fuse(b.0, b.1, &near_smart(), 1.0, 1.0, false, &cparams(100, 90, true));
        assert!(out.active);
        assert!(
            out.target_l >= b.0 * 0.75,
            "宽带突发在锁定中应大部分保留: {:.3} vs 基线 {:.3}",
            out.target_l,
            b.0
        );
    }

    /* ── 静音段行为 ── */

    #[test]
    fn silence_resets_lock_and_gate() {
        let mut c = CMode::new(RATE);
        let base = [(5usize, 2000.0f32), (6, 400.0), (7, 900.0), (8, 150.0)];
        feed_bgm(&mut c, 320, &base, 23);
        assert_eq!(c.music_state, MUSIC_LOCK);
        // 静音 >120ms → 清历史 + 退出锁
        feed_silence(&mut c, 20); // ≈213ms
        assert_ne!(c.music_state, MUSIC_LOCK, "静音应解除 BGM 锁");
        assert!(c.gate < 0.05, "门控应回落: {}", c.gate);
        // 静音中 fuse: b≈0 → 输出≈0, 不凭空震动
        let out = c.fuse(0.0, 0.0, &near_smart(), 1.0, 1.0, false, &cparams(100, 90, true));
        assert!(out.target_l <= 0.01 && out.target_r <= 0.01, "静音不应有输出");
    }

    /* ── 换曲自适应重收敛 ── */

    #[test]
    fn track_change_unlocks_then_relocks() {
        let mut c = CMode::new(RATE);
        let a = [(5usize, 2000.0f32), (6, 400.0), (7, 900.0), (8, 150.0)];
        feed_bgm(&mut c, 320, &a, 23);
        assert_eq!(c.music_state, MUSIC_LOCK, "前提: 曲 A 已锁");
        // 换曲: 音色 B (谱型大变 → 新颖度 2 帧解锁) + 周期改 400ms (37 帧)
        let b = [(13usize, 1800.0f32), (14, 600.0), (19, 800.0), (20, 120.0)];
        feed_bgm(&mut c, 20, &b, 37); // 213ms > UNLOCK_SUSTAIN_MS
        assert_ne!(
            c.music_state, MUSIC_LOCK,
            "新颖度持续 150ms 应解锁旧曲 (旧锁残留 ≤150ms)"
        );
        // 新曲继续喂 → 历史重累积后再次锁定
        feed_bgm(&mut c, 320, &b, 37);
        assert_eq!(c.music_state, MUSIC_LOCK, "新曲应重新收敛锁定");
    }

    /* ── 非周期内容不误锁 ── */

    #[test]
    fn random_skill_sequence_never_locks() {
        let mut c = CMode::new(RATE);
        // 随机间隔 (60-160 帧) 的孤立技能音, 无周期无重复
        let mut s = 0xC0FFEEu64;
        let mut frames = 0usize;
        while frames < 560 {
            s = s
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let gap = 60 + ((s >> 33) as usize % 100);
            feed_silence(&mut c, gap / 2);
            feed(&mut c, &skill_bins(), MODE_NORMAL);
            feed_silence(&mut c, gap - gap / 2);
            frames += gap + 2;
        }
        assert_ne!(c.music_state, MUSIC_LOCK, "非周期技能序列不应误锁 BGM");
    }

    /* ── 回退链 ── */

    #[test]
    fn zero_upgrade_is_full_bypass() {
        let mut c = CMode::new(RATE);
        let base = [(5usize, 2000.0f32), (6, 400.0), (7, 900.0), (8, 150.0)];
        feed_bgm(&mut c, 320, &base, 23);
        assert_eq!(c.music_state, MUSIC_LOCK);
        let b = (0.42f32, 0.37f32);
        let out = c.fuse(b.0, b.1, &near_smart(), 1.0, 1.0, true, &cparams(0, 90, true));
        assert!(!out.active, "upgrade=0 必须整层旁路 (无损回 B)");
    }

    #[test]
    fn warmup_outputs_b_only() {
        let mut c = CMode::new(RATE);
        let base = [(5usize, 2000.0f32), (6, 400.0), (7, 900.0), (8, 150.0)];
        for _ in 0..WARMUP_FRAMES - 5 {
            feed(&mut c, &base, MODE_MUSIC);
        }
        let out = c.fuse(0.6, 0.6, &near_smart(), 1.0, 1.0, false, &cparams(100, 90, true));
        assert!(!out.active, "暖机不足只输出 B");
    }

    #[test]
    fn nan_input_trips_degrade_then_recovers() {
        let mut c = CMode::new(RATE);
        let base = [(5usize, 2000.0f32), (6, 400.0), (7, 900.0), (8, 150.0)];
        feed_bgm(&mut c, 320, &base, 23);
        assert_eq!(c.music_state, MUSIC_LOCK);
        // 连续 3 帧 NaN → 降级
        for i in 1..=3 {
            let out = c.fuse(
                f32::NAN,
                0.5,
                &near_smart(),
                1.0,
                1.0,
                false,
                &cparams(100, 90, true),
            );
            assert!(!out.active, "异常帧不应有 C 输出");
            assert_eq!(c.fault_streak, i, "故障计数应累加");
        }
        assert!(c.degraded, "3 帧异常后应降级旁路");
        // 冷却期 (1s 帧时钟) 内保持旁路
        for _ in 0..50 {
            feed(&mut c, &base, MODE_MUSIC);
        }
        let out = c.fuse(0.5, 0.5, &near_smart(), 1.0, 1.0, false, &cparams(100, 90, true));
        assert!(!out.active, "冷却期内保持旁路");
        // 推进帧时钟越过冷却 → 恢复 (重新暖机后再生效)
        for _ in 0..WARMUP_FRAMES + 60 {
            feed(&mut c, &base, MODE_MUSIC);
        }
        let out = c.fuse(0.5, 0.5, &near_smart(), 1.0, 1.0, false, &cparams(100, 90, true));
        assert!(out.active, "冷却结束 + 重新暖机后应恢复 C 层");
        assert!(!c.degraded);
    }

    /* ── 特征方向性 (文档 PR1 验收) ── */

    #[test]
    fn features_separate_tonal_wideband_and_silence() {
        let mut c = CMode::new(RATE);
        // 纯音铺底: 窄带 (wide 低) + 稳定 (stability 高) + 谐波 (harmonicity 高)
        let pad = [(5usize, 2000.0f32), (7, 900.0), (9, 300.0)];
        for _ in 0..40 {
            feed(&mut c, &pad, MODE_MUSIC);
        }
        assert!(c.wide < 0.3, "纯音应判窄带: wide={}", c.wide);
        assert!(c.harmonicity > 0.3, "纯音应有谐波证据: {}", c.harmonicity);
        // 宽带噪声: wide 高 + harmonicity 低
        for _ in 0..40 {
            feed(&mut c, &skill_bins(), MODE_NORMAL);
        }
        assert!(c.wide > 0.5, "宽带噪声应判 wide: wide={}", c.wide);
        assert!(c.harmonicity < 0.25, "噪声谐波性应低: {}", c.harmonicity);
    }

    /* ── 事件状态机: 单冲击一响 / 收尾归零 ── */

    #[test]
    fn single_burst_yields_single_event() {
        let mut c = CMode::new(RATE);
        // 预热: 稳定低能量铺底 (基线建立)
        let quiet_pad = [(5usize, 150.0f32), (7, 80.0)];
        for _ in 0..60 {
            feed(&mut c, &quiet_pad, MODE_NORMAL);
        }
        // 一记强瞬态: 宽带 (全带噪声型, 技能音形态) + 低带重击
        let mut burst: Vec<(usize, f32)> = skill_bins();
        burst.extend(kick_bins());
        burst.push((5, 2500.0));
        burst.push((7, 1200.0));
        feed(&mut c, &burst, MODE_NORMAL);
        // 起振 (EVT_ATTACK/HOLD 表示事件在轨)
        assert!(
            c.evt_state == EVT_ATTACK || c.evt_state == EVT_HOLD,
            "强瞬态应起振: state={}",
            c.evt_state
        );
        assert!(c.evt_env > 0.0, "事件包络应建立");
        // 静置: 包络衰减到释放/空闲
        for _ in 0..80 {
            feed(&mut c, &quiet_pad, MODE_NORMAL);
        }
        assert!(
            c.evt_state == EVT_IDLE || c.evt_state == EVT_RELEASE,
            "事件应收尾: state={}",
            c.evt_state
        );
    }

    /* ── resolve_algorithm_mode (audio_state 纯函数; 就地验证语义) ── */

    #[test]
    fn resolve_mode_semantics() {
        use crate::state::audio_state::{
            resolve_algorithm_mode, AUDIO_ALGO_A, AUDIO_ALGO_B, AUDIO_ALGO_C,
        };
        // 旧配置 (None): 由 smart_enabled 推导 —— 升级零变化
        assert_eq!(resolve_algorithm_mode(None, false), AUDIO_ALGO_A);
        assert_eq!(resolve_algorithm_mode(None, true), AUDIO_ALGO_B);
        // 显式选择
        assert_eq!(resolve_algorithm_mode(Some("a"), true), AUDIO_ALGO_A);
        assert_eq!(resolve_algorithm_mode(Some("b"), false), AUDIO_ALGO_B);
        assert_eq!(resolve_algorithm_mode(Some("c"), false), AUDIO_ALGO_C);
        assert_eq!(resolve_algorithm_mode(Some("C"), true), AUDIO_ALGO_C);
        // 非法 → 降级 B
        assert_eq!(resolve_algorithm_mode(Some("x"), true), AUDIO_ALGO_B);
        assert_eq!(resolve_algorithm_mode(Some(""), true), AUDIO_ALGO_B);
    }
}

/* ═══════════════ 临时诊断（调参用，随实现轮删除） ═══════════════ */

#[cfg(test)]
mod dbg {
    use super::*;
    use crate::audio_haptics::mapping::MODE_MUSIC;

    const RATE: f32 = 48000.0;
    const HALF: usize = 512;
    const TEST_RMS_NORM: f32 = 0.433_012_7 * 1024.0;

    fn synth(bins: &[(usize, f32)]) -> ([f32; HALF], [f32; 5], [f32; N_GROUPS], f32) {
        let mut power = [0f32; HALF];
        for &(b, p) in bins { power[b] += p; }
        let bin_hz = RATE / 1024.0;
        let mut band_sq = [0f32; 5];
        let edges: Vec<f32> = (0..=N_GROUPS).map(|g| GROUP_LO_HZ * (GROUP_HI_HZ / GROUP_LO_HZ).powf(g as f32 / N_GROUPS as f32)).collect();
        let mut grp = [0f32; N_GROUPS];
        let mut p_total = 0f32;
        for b in 1..HALF {
            let f = b as f32 * bin_hz;
            p_total += power[b];
            if f >= GROUP_LO_HZ && f < GROUP_HI_HZ {
                for g in 0..N_GROUPS { if f >= edges[g] && f < edges[g + 1] { grp[g] += power[b]; break; } }
            }
            let band_edge = [20.0f32, 80.0, 250.0, 2000.0, 8000.0, 24000.0];
            for (i, w) in band_edge.windows(2).enumerate() {
                if f >= w[0] && f < w[1] { band_sq[i] += power[b]; break; }
            }
        }
        let band_rms = band_sq.map(|sq| sq.sqrt() / TEST_RMS_NORM * 32768.0);
        let total = p_total.sqrt() / TEST_RMS_NORM * 32768.0;
        (power, band_rms, grp, total)
    }

    #[test]
    fn debug_evidence() {
        let mut c = CMode::new(RATE);
        let base: Vec<(usize, f32)> = vec![(5, 2000.0), (6, 400.0), (7, 900.0), (8, 150.0)];
        for i in 0..320 {
            let mut bins = base.clone();
            if i % 23 < 3 { bins.push((1, 3000.0)); bins.push((2, 1500.0)); }
            let (power, band, grp, total) = synth(&bins);
            c.push_frame(&CFrameInput {
                band_rms: band, group_power: &grp, power: &power,
                total_rms: total, vocal_rms: 0.0, mode_b: MODE_MUSIC,
            });
            if i % 40 == 0 {
                eprintln!("f={} raw={:.3} conf={:.3} rep={:.3} beat={:.3} lock={} hpss={:.3} harm={:.3} wide={:.3} stab={:.3} novel={:.3} hold={:.0} fnorm={:.3} T={:.3}",
                    i, c.music_raw, c.conf, c.r_rep, c.beat_conf, c.music_state, c.m_hpss, c.harmonicity, c.wide, c.stability, c.novelty, c.enter_hold_ms, c.f_norm, c.t_score);
            }
        }
    }
}
