//! Application configuration management.
//!
//! Handles loading, saving, and validation of application settings
//! including key mappings and runtime parameters.

use serde::{Deserialize, Serialize};
use smallvec::SmallVec;
use std::convert::Infallible;
use std::{collections::HashMap, fs, path::Path, str::FromStr};

use crate::i18n::Language;

/// Device API preference for input handling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default)]
pub enum DeviceApiPreference {
    /// Auto-detect best API.
    #[default]
    Auto,
    /// Force XInput.
    XInput,
    /// Force Raw Input.
    RawInput,
}

/// XInput capture mode strategy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default)]
pub enum XInputCaptureMode {
    /// Captures the most sustained input pattern.
    MostSustained,
    /// Captures the last stable input before release.
    LastStable,
    /// Prioritizes diagonal directions over straight directions with combo key support.
    #[default]
    DiagonalPriority,
}

impl FromStr for XInputCaptureMode {
    type Err = Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "LastStable" => Ok(Self::LastStable),
            "DiagonalPriority" => Ok(Self::DiagonalPriority),
            _ => Ok(Self::default()),
        }
    }
}

impl XInputCaptureMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::MostSustained => "MostSustained",
            Self::LastStable => "LastStable",
            Self::DiagonalPriority => "DiagonalPriority",
        }
    }

    pub fn all_modes() -> &'static [XInputCaptureMode] {
        &[
            XInputCaptureMode::MostSustained,
            XInputCaptureMode::LastStable,
            XInputCaptureMode::DiagonalPriority,
        ]
    }
}

/// Vibration configuration (DFO battle events -> gamepad motors).
#[derive(Debug, Clone, Copy, PartialEq, Deserialize, Serialize)]
pub struct VibrationConfig {
    /// Master switch for vibration feedback
    #[serde(default = "default_vib_true")]
    pub enabled: bool,
    /// Advanced tuning switch (default off: only base params take effect)
    #[serde(default)]
    pub advanced_enabled: bool,
    /// Left motor global gain % (motor zone)
    #[serde(default = "default_vib_100")]
    pub motor_l_gain: u32,
    /// Right motor global gain % (motor zone)
    #[serde(default = "default_vib_100")]
    pub motor_r_gain: u32,
    /// Per-base-item L/R weights %: 13 items x [L,R] = 26
    /// 0=总闸,1=攻击频率,2=强度上限,3=衰减,4=连击,5=节奏,
    /// 6=通用飘字,7=DOT,8=特效,9=状态,10=特殊,11=命中,12=受击
    #[serde(default = "default_vib_item_lr", with = "vib_arr26")]
    pub item_lr: [u32; 26],
    /// Per-rank-event L/R weights %: 15 rank events x [L,R] = 30
    /// idx 0=评分点 1=闪避 2=暴击 3=破招 4=背击 5=最终击杀 6=凌空 7=命中第一击
    ///     8=增益叠加 9=释放技能 10=镜头震动 11=移动 12=技能震动 13=暴击特写 14=怪物死亡
    #[serde(default = "default_vib_rank_lr", with = "vib_arr30")]
    pub rank_lr: [u32; 30],
    /// 15 项评分细分事件独立强度 % (rank_type_gain, 0=关闭)
    #[serde(default = "default_vib_rank_gain", with = "vib_arr15")]
    pub rank_type_gain: [u32; 15],
    /// 评分等级强度 % (rank_level_gain)
    #[serde(default = "default_vib_100")]
    pub rank_level_gain: u32,
    /// 评分事件满幅脉冲时长 ms (rank_duration)
    #[serde(default = "default_vib_rank_dur")]
    pub rank_duration: u32,
    /// Random intensity variation % around cap (0 = off, N = ±N%)
    #[serde(default)]
    pub random_gain: u32,
    /// Output smoothing % (v22.3: 一阶低通抑制低频嗡嗡声, 0=无, 高=柔)
    #[serde(default = "default_vib_55")]
    pub out_smooth: u32,
    /// 独立测试模式 (v24.2: 开 = 评分/移动通道独立于全局总调整, 便于单通道测试;
    /// 关 = 全局总调整应用到所有通道)
    #[serde(default)]
    pub independent_test: bool,
    /// 移动持续震动独立于全局强度 (v24.5: 默认开, 移动一直在走,
    /// 吃全局强度容易直接没有震动; 关 = 移动也受全局总调整/强度上限约束)
    #[serde(default = "default_vib_move_indep")]
    pub move_independent: bool,
    /// 评分动态衰减 (类鬼泣, v29): 攻击提升评级/停手衰减,
    /// 最高评级反馈 = 预设参数 ×1.2, 最低档保底 0.6 (不失去基础反馈)
    #[serde(default = "default_vib_rank_decay")]
    pub rank_decay_enabled: bool,
    /// 输出低强度死区 % (v29.2: 主通道输出低于此值归 0, 消除马达低强度嗡声;
    /// 默认 3, 建议 2-8, 过高会吃掉轻反馈)
    #[serde(default = "default_vib_3")]
    pub out_threshold: u32,
    /// 输出动态范围重映射 (v30, Xbox360/ERM 适配): 非零输出映射到
    /// [remap_min, 100] 区间, 保证转子一定转起来 (轻反馈不再被死区吞掉)
    #[serde(default = "default_vib_true")]
    pub remap_enabled: bool,
    /// 重映射最小输出 % (与死区一致, 默认 28)
    #[serde(default = "default_vib_28")]
    pub remap_min: u32,
    /// 马达分工 (v30, Xbox360 风格): 轻反馈仅主导马达, 重反馈双马达
    #[serde(default = "default_vib_true")]
    pub split_enabled: bool,
    /// 分工分界 % (输出峰值低于此值为轻反馈 → 单马达)
    #[serde(default = "default_vib_split_thr")]
    pub split_thr: u32,
    /// 停手多久开始衰减 ms
    #[serde(default = "default_vib_rank_decay_delay")]
    pub rank_decay_delay_ms: u32,
    /// 衰减速度 级/秒
    #[serde(default = "default_vib_rank_decay_speed")]
    pub rank_decay_speed: u32,
    /// 最低反馈倍率 % (评级 0)
    #[serde(default = "default_vib_rank_decay_min")]
    pub rank_decay_min_mul: u32,
    /// 最高反馈倍率 % (评级 8, 满评分 SSS)
    #[serde(default = "default_vib_rank_decay_max")]
    pub rank_decay_max_mul: u32,
    /// 绝对震动频率 (v26: 开启后一切震动算法失效, 只在 时间-次数 内注入;
    /// 全局强度/强度上限仍可控制, 移动独立; 召唤专属 3s/6 次)
    #[serde(default)]
    pub abs_freq_enabled: bool,
    /// 绝对频率窗口 ms
    #[serde(default = "default_vib_abs_win")]
    pub abs_freq_window_ms: u32,
    /// 绝对频率窗口内最大注入次数
    #[serde(default = "default_vib_abs_max")]
    pub abs_freq_max: u32,
    /// 震动节流窗口 ms (v24: 限定窗口内最大注入次数, 0=禁用; 召唤师等狂震职业)
    #[serde(default)]
    pub throttle_window_ms: u32,
    /// 节流窗口内最大注入次数 (0=禁用)
    #[serde(default)]
    pub throttle_max_hits: u32,
    /// 密度激活时次数比例 % (v24.1: 狂震期收紧, 100=不收紧; 召唤师 20 → 1次/s)
    #[serde(default = "default_vib_100")]
    pub throttle_dense_ratio: u32,
    /// Attack frequency gain -196
    #[serde(default = "default_vib_40")]
    pub attack_gain: u32,
    /// Hit damage gain -196
    #[serde(default = "default_vib_60")]
    pub damage_gain: u32,
    /// Camera shake gain -196
    #[serde(default = "default_vib_80")]
    pub shake_gain: u32,
    /// Movement gain -196
    #[serde(default = "default_vib_20")]
    pub move_gain: u32,
    /// Total strength cap -196
    #[serde(default = "default_vib_100")]
    pub max_strength: u32,
    /// Decay time constant ms
    #[serde(default = "default_vib_180")]
    pub decay_ms: u32,
    /// Combo reset boost -196
    #[serde(default = "default_vib_15")]
    pub hit_boost: u32,
    /// Battle start pulse -196
    #[serde(default = "default_vib_35")]
    pub start_pulse: u32,
    /// Kill pulse -196
    #[serde(default = "default_vib_50")]
    pub kill_pulse: u32,
    /// Global master gain -196 (scales all output)
    #[serde(default = "default_vib_100")]
    pub master_gain: u32,
    /// Damage font hit strength -196
    #[serde(default = "default_vib_30")]
    pub font_strength: u32,
    /// Damage font minimum interval ms
    #[serde(default = "default_vib_40")]
    pub font_interval: u32,
    /// Right motor rhythm ratio -196 (50 = balanced)
    #[serde(default = "default_vib_50")]
    pub rhythm: u32,
    /// Font type strengths (default 0 = off, user enables one by one)
    #[serde(default)]
    pub font_hp: u32,
    #[serde(default)]
    pub font_special: u32,
    #[serde(default)]
    pub font_state: u32,
    #[serde(default)]
    pub font_effect: u32,
    #[serde(default)]
    pub font_attack: u32,
    #[serde(default)]
    pub font_hit: u32,
    /// Advanced params 19-29 (L/R ratios, decays, windows, combo/adaptive, pulses)
    #[serde(default = "default_vib_advanced", with = "vib_arr41")]
    pub advanced: [u32; 41],
}

pub fn default_vib_advanced() -> [u32; 41] {
    [100, 100, 0, 0, 380, 50, 10, 55, 30, 60, 50, 85, 25, 70, 70, 50, 45, 60, 30, 70, 70, 150, 90, 380, 35, 8, 40, 30, 4, 100, 30, 60, 20, 200, 50, 0, 150, 90, 90, 30, 100, ]
}

/// serde helpers for 26-element array
pub mod vib_arr26 {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(arr: &[u32; 26], s: S) -> Result<S::Ok, S::Error> {
        s.collect_seq(arr.iter())
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<[u32; 26], D::Error> {
        let v: Vec<u32> = Vec::deserialize(d)?;
        let mut a = [0u32; 26];
        for (i, x) in v.iter().enumerate().take(26) {
            a[i] = *x;
        }
        Ok(a)
    }
}

/// serde helpers for 30-element array (rank L/R)
pub mod vib_arr30 {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(arr: &[u32; 30], s: S) -> Result<S::Ok, S::Error> {
        s.collect_seq(arr.iter())
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<[u32; 30], D::Error> {
        let v: Vec<u32> = Vec::deserialize(d)?;
        let mut a = [0u32; 30];
        for (i, x) in v.iter().enumerate().take(30) {
            a[i] = *x;
        }
        Ok(a)
    }
}

pub fn default_vib_rank_lr() -> [u32; 30] {
    [0; 30]
}

/// serde helpers for 15-element array (rank gains)
pub mod vib_arr15 {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(arr: &[u32; 15], s: S) -> Result<S::Ok, S::Error> {
        s.collect_seq(arr.iter())
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<[u32; 15], D::Error> {
        let v: Vec<u32> = Vec::deserialize(d)?;
        let mut a = [0u32; 15];
        for (i, x) in v.iter().enumerate().take(15) {
            a[i] = *x;
        }
        Ok(a)
    }
}

pub fn default_vib_rank_gain() -> [u32; 15] {
    [100; 15]
}

pub fn default_vib_rank_dur() -> u32 {
    300
}

pub fn default_vib_item_lr() -> [u32; 26] {
    [
        0, 0,  /* 0 总闸 L/R (0=保持原样) */
        0, 0,  /* 1 攻击频率 */
        0, 0,  /* 2 强度上限 */
        0, 0,  /* 3 衰减 */
        0, 0,  /* 4 连击 */
        0, 0,  /* 5 节奏 */
        0, 0,  /* 6 通用飘字 */
        0, 0,  /* 7 DOT */
        0, 0,  /* 8 特效 */
        0, 0,  /* 9 状态 */
        0, 0,  /* 10 特殊 */
        0, 0,  /* 11 命中 */
        0, 0,  /* 12 受击 */
    ]
}

impl Default for VibrationConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            advanced_enabled: false,
            motor_l_gain: 100,
            motor_r_gain: 100,
            random_gain: 0,
            out_smooth: 55,
            independent_test: false,
            move_independent: true,
            abs_freq_enabled: false,
            abs_freq_window_ms: 3000,
            abs_freq_max: 6,
            rank_decay_enabled: false,
            out_threshold: 28,
            remap_enabled: true,
            remap_min: 28,
            split_enabled: true,
            split_thr: 40,
            rank_decay_delay_ms: 2000,
            rank_decay_speed: 2,
            rank_decay_min_mul: 60,
            rank_decay_max_mul: 120,
            throttle_window_ms: 0,
            throttle_max_hits: 0,
            throttle_dense_ratio: 100,
            item_lr: default_vib_item_lr(),
            rank_lr: default_vib_rank_lr(),
            rank_type_gain: default_vib_rank_gain(),
            rank_level_gain: 100,
            rank_duration: default_vib_rank_dur(),
            attack_gain: 0,
            damage_gain: 0,
            shake_gain: 0,
            move_gain: 0,
            max_strength: 0,
            decay_ms: 0,
            hit_boost: 0,
            start_pulse: 0,
            kill_pulse: 0,
            master_gain: 0,
            font_strength: 0,
            font_interval: 0,
            rhythm: 50,
            font_hp: 0,
            font_special: 0,
            font_state: 0,
            font_effect: 0,
            font_attack: 0,
            font_hit: 0,
            advanced: default_vib_advanced(),
        }
    }
}

fn default_vib_true() -> bool { true }
fn default_vib_40() -> u32 { 40 }
fn default_vib_60() -> u32 { 60 }
fn default_vib_80() -> u32 { 80 }
fn default_vib_20() -> u32 { 20 }
fn default_vib_100() -> u32 { 100 }

fn default_vib_55() -> u32 {
    55
}

fn default_vib_move_indep() -> bool {
    true
}

fn default_vib_abs_win() -> u32 {
    3000
}
fn default_vib_rank_decay() -> bool {
    false
}

fn default_vib_3() -> u32 {
    28
}

fn default_vib_28() -> u32 {
    28
}

fn default_vib_split_thr() -> u32 {
    40
}

fn default_vib_rank_decay_delay() -> u32 {
    2000
}

fn default_vib_rank_decay_speed() -> u32 {
    2
}

fn default_vib_rank_decay_min() -> u32 {
    60
}

fn default_vib_rank_decay_max() -> u32 {
    120
}

fn default_vib_abs_max() -> u32 {
    6
}
fn default_vib_180() -> u32 { 180 }
fn default_vib_15() -> u32 { 15 }
fn default_vib_35() -> u32 { 35 }
fn default_vib_50() -> u32 { 50 }
fn default_vib_30() -> u32 { 30 }

/// serde helpers for long fixed-size arrays (>32 not supported by serde derive)
pub mod vib_arr60 {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(arr: &[u32; 60], s: S) -> Result<S::Ok, S::Error> {
        s.collect_seq(arr.iter())
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<[u32; 60], D::Error> {
        let v: Vec<u32> = Vec::deserialize(d)?;
        let mut a = [0u32; 60];
        for (i, x) in v.iter().enumerate().take(60) {
            a[i] = *x;
        }
        Ok(a)
    }
}

pub mod vib_arr41 {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(arr: &[u32; 41], s: S) -> Result<S::Ok, S::Error> {
        s.collect_seq(arr.iter())
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<[u32; 41], D::Error> {
        let v: Vec<u32> = Vec::deserialize(d)?;
        let mut a = [0u32; 41];
        for (i, x) in v.iter().enumerate().take(41) {
            a[i] = *x;
        }
        Ok(a)
    }
}

/// Named vibration parameter preset.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct VibrationPreset {
    pub name: String,
    /// 60 params, same order as AppState.vibration_params:
    /// [attack,damage,shake,move,max,decay,boost,start,kill,
    ///  master,font_str,font_ivl, font_hp,font_special,font_state,
    ///  font_effect,font_attack,font_hit, rhythm]
    #[serde(with = "vib_arr60")]
    pub params: [u32; 60],
    /// 13 项 L/R 权重 (-100..+100, 0=保持原样): 0=总闸 1=攻击 2=上限 3=衰减
    /// 4=连击 5=节奏 6=通用 7=DOT 8=特效 9=状态 10=特殊 11=命中 12=受击
    #[serde(default = "default_vib_item_lr", with = "vib_arr26")]
    pub item_lr: [u32; 26],
    /// 15 项评分事件 L/R 权重 (-100..+100, 0=保持原样)
    #[serde(default = "default_vib_rank_lr", with = "vib_arr30")]
    pub rank_lr: [u32; 30],
    /// 15 项评分细分事件独立强度 % (rank_type_gain, 0=关闭)
    #[serde(default = "default_vib_rank_gain", with = "vib_arr15")]
    pub rank_type_gain: [u32; 15],
    /// 评分等级强度 % (rank_level_gain)
    #[serde(default = "default_vib_100")]
    pub rank_level_gain: u32,
    /// 评分事件满幅脉冲时长 ms (rank_duration)
    #[serde(default = "default_vib_rank_dur")]
    pub rank_duration: u32,
    /// 输出平滑 % (v22.3: 一阶低通抑制低频嗡嗡声, 0=无, 高=柔)
    #[serde(default = "default_vib_55")]
    pub out_smooth: u32,
}

/// 将带符号权重 (-100..+100) 转为 u32 存储 (负数用补码)
const fn lr(w: i32) -> u32 {
    w as u32
}

pub fn default_vibration_presets() -> Vec<VibrationPreset> {
    vec![
        /* ── 默认: 均衡全能 (清爽脆快, 整体偏轻) ──
         * 设计: 上限 45 整体偏轻, 衰减 50ms 柔和不震手,
         * 命中 40/特殊 55 分层清晰但轻, 曲线 95 平缓不饱和,
         * 左右均衡偏右 (命中 L-10/R+10 微偏右) */
        VibrationPreset {
            name: "默认".to_string(),
            params: [100, 0, 0, 0, 55, 40, 30, 0, 0, 100, 0, 40, 20, 80, 35, 40, 25, 30, 50, 95, 95, 6, 25, 380, 35, 8, 40, 30, 4, 40, 500, 35, 1200, 55, 200, 25, 55, 25, 60, 1500, 150, 90, 250, 20, 600, 1500, 3000, 160, 60, 30, 60, 25, 200, 50, 0, 150, 90, 90, 30, 100],
            item_lr: [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, lr(-10), 10, 0, 0],
            rank_lr: [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, lr(-15), 10, 0, 0, 10, 0, 0, 0],
            rank_type_gain: [60, 100, 100, 100, 100, 100, 90, 70, 75, 70, 100, 30, 100, 100, 100],
            rank_level_gain: 100,
            rank_duration: 300,
            out_smooth: 55,
        },
        /* ── ACT1 特供: 老方案管线 (IVL 20ms/曲线 100 线性) + 弱机搭配,
         * 按 XBOX360 65535 量纲分配 (docs/震动系统开发规范_v20):
         *   命中 0x01=70 强主体; 技能/暴击 0x10=60 分层 (略低于命中, 且特殊
         *   攻击自带 200ms 静默窗防叠加爆震); 出血 0x04=12 极低; 评分族压低;
         *   怪物死亡中等。全局限幅: master 90 × 上限 75 (弱机保护)。
         *   密度自适应启用 (thr 45/降 40) + 连击 cap 150/slope 60 =
         *   后期连击暴增自动压制 (借鉴职业算法的密度/倍率封顶机制);
         *   槽 1/2/3/7/8 为引擎无引用死槽, 恒 0; p[39]=移动积累窗口。
         * 仅 S1 路线在预设列表中显示 (各选择处按 vib_legacy_client 门控) ── */
        VibrationPreset {
            name: "ACT1 特供".to_string(),
            params: [100, 0, 0, 0, 75, 65, 35, 0, 0, 90, 0, 20, 20, 60, 25, 10, 70, 45, 50, 100, 100, 8, 50, 380, 35, 8, 40, 30, 4, 45, 500, 40, 800, 55, 300, 60, 60, 45, 60, 1200, 150, 90, 600, 30, 800, 1200, 3000, 150, 60, 40, 60, 35, 200, 30, 200, 150, 40, 40, 30, 50],
            item_lr: [0; 26],
            rank_lr: [0; 30],
            rank_type_gain: [20, 20, 30, 30, 30, 20, 20, 20, 20, 20, 20, 40, 20, 20, 45],
            rank_level_gain: 30,
            rank_duration: 250,
            out_smooth: 35,
        },
        /* ── 低频攻击职业: 泛用低频震动 (重击向, 默认下方第 2 位) ──
         * 设计: 低频攻击间隔大, 每次震动可重可持久 (无叠加问题),
         * 上限 70/衰减 60ms 长余韵, 命中 45/受击 50/特殊 95 重反馈,
         * 密度自适应弱 (thr60 仅防异常暴增), 自适应降幅低 (低频无叠加),
         * 曲线 105 干净只反馈强击, 输出平滑 40 保持干脆 */
        VibrationPreset {
            name: "低频攻击职业".to_string(),
            params: [100, 0, 0, 0, 70, 60, 40, 0, 0, 100, 0, 40, 40, 95, 50, 45, 45, 50, 50, 105, 105, 5, 20, 380, 40, 8, 45, 30, 4, 60, 500, 20, 1200, 75, 200, 40, 95, 40, 60, 60, 160, 90, 300, 20, 600, 1000, 3000, 180, 60, 30, 90, 15, 200, 50, 300, 180, 90, 90, 30, 100],
            item_lr: [0, 0, 0, 15, 0, 0, 20, 10, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 20, lr(-20), 15, 30, lr(-15)],
            rank_lr: [0, 0, 0, 0, 20, lr(-10), 0, 0, 0, 0, 25, lr(-10), 0, 0, 0, 0, 0, 0, 10, 0, 0, 0, lr(-25), 5, 0, 0, 30, lr(-15), 20, 0],
            rank_type_gain: [65, 90, 100, 100, 100, 100, 90, 75, 70, 80, 100, 25, 100, 100, 100],
            rank_level_gain: 100,
            rank_duration: 400,
            out_smooth: 40,
        },
        /* ── 高频攻击职业: 泛用高频震动 (轻快防震手, 默认下方第 3 位) ──
         * 设计: 高频连击密集防叠加震手, 上限 45/衰减 35ms 快收,
         * 命中 20/受击 25/特殊 80 轻反馈, 注入间隔 45ms 拉开,
         * 密度自适应强 (thr25 窗口降 50%), 连击倍率限 1.4,
         * 强自适应 (连击密降 35%), 走位积累增强 (10%/35%/1800ms),
         * 柔和曲线 85 细腻, 输出平滑 70 防嗡 */
        VibrationPreset {
            name: "高频攻击职业".to_string(),
            params: [100, 0, 0, 0, 45, 35, 30, 0, 0, 100, 0, 45, 25, 80, 35, 40, 20, 25, 50, 85, 85, 10, 35, 380, 40, 8, 45, 30, 4, 25, 500, 50, 1200, 45, 200, 15, 50, 15, 60, 60, 150, 90, 140, 25, 600, 2000, 3000, 140, 40, 30, 80, 35, 200, 50, 120, 150, 90, 90, 30, 100],
            item_lr: [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, lr(-10), 10, 0, 0],
            rank_lr: [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, lr(-15), 10, 0, 0, 10, 0, 0, 0],
            rank_type_gain: [55, 90, 90, 90, 90, 90, 90, 70, 75, 75, 90, 30, 90, 100, 90],
            rank_level_gain: 100,
            rank_duration: 250,
            out_smooth: 70,
        },
        /* ── 高振幅: 重型打击感 (重击向) ──
         * 设计: 衰减 75ms 长余韵(沉重), 命中 85/特殊 100 拉满,
         * 曲线 180 强饱和, 上限 70; 左马达也吃重(受击 L30),
         * 整体风格"重砸", 与默认/节奏律动拉开明显差距 */
        VibrationPreset {
            name: "高振幅".to_string(),
            params: [100, 0, 0, 0, 70, 65, 55, 0, 0, 100, 0, 40, 45, 100, 55, 65, 60, 70, 50, 180, 180, 3, 15, 380, 40, 8, 40, 30, 4, 60, 500, 20, 1200, 75, 200, 40, 60, 30, 80, 1000, 200, 90, 300, 30, 600, 1500, 3000, 200, 90, 30, 60, 15, 200, 50, 0, 200, 90, 90, 30, 100],
            item_lr: [0, 0, 0, 15, 0, 0, 20, 10, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 20, lr(-20), 15, 30, lr(-15)],
            rank_lr: [0, 0, 0, 0, 20, lr(-10), 0, 0, 0, 0, 25, lr(-10), 0, 0, 0, 0, 0, 0, 10, 0, 0, 0, lr(-25), 5, 0, 0, 30, lr(-15), 20, 0],
            rank_type_gain: [65, 90, 100, 100, 100, 100, 90, 75, 70, 80, 100, 25, 100, 100, 100],
            rank_level_gain: 100,
            rank_duration: 450,
            out_smooth: 35,
        },
        /* ── 节奏律动: 技能节奏感 (极脆律动向) ──
         * 设计: 衰减 25ms 极致短脆(每段技能清脆一跳), 命中 70 中高但极短,
         * 特效节奏周期 80ms 密集律动; 左马达仅微衬底(L-25),
         * 风格"跳节奏", 与高振幅的长余韵形成两个极端 */
        VibrationPreset {
            name: "节奏律动".to_string(),
            params: [100, 0, 0, 0, 70, 30, 35, 0, 0, 100, 0, 45, 35, 95, 35, 70, 55, 45, 55, 100, 100, 8, 30, 360, 30, 8, 35, 30, 4, 30, 500, 40, 1200, 50, 200, 20, 55, 20, 60, 1600, 160, 90, 250, 20, 700, 1500, 3000, 150, 50, 30, 70, 30, 200, 50, 0, 150, 90, 90, 30, 100],
            item_lr: [0, 0, 0, 15, 0, 0, lr(-10), 0, 0, 0, 0, 10, 0, 0, 0, 0, lr(-10), 15, 0, 0, 0, 15, lr(-15), 20, 0, 0],
            rank_lr: [0, 15, 0, 20, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 20, 0, 0, lr(-20), 5, 0, 0, 0, 30, 0, 0],
            rank_type_gain: [60, 90, 90, 90, 90, 90, 90, 75, 85, 80, 90, 30, 90, 100, 90],
            rank_level_gain: 100,
            rank_duration: 200,
            out_smooth: 55,
        },
        /* ── 极简轻巧: 长时间刷图 (超轻省电向) ──
         * 设计: 命中 35/特殊 50 压到最轻, 衰减 30ms 瞬收,
         * 总闸 80 略降, 上限 45; 左马达大幅减弱(L-25~-30), 只留右马达轻反馈 */
        VibrationPreset {
            name: "极简轻巧".to_string(),
            params: [100, 0, 0, 0, 45, 30, 0, 0, 0, 80, 0, 60, 15, 60, 20, 35, 20, 20, 50, 100, 100, 2, 10, 400, 25, 6, 30, 30, 4, 25, 500, 45, 1200, 45, 200, 15, 55, 15, 45, 800, 90, 90, 250, 20, 600, 1500, 3000, 120, 30, 30, 60, 35, 200, 40, 0, 150, 90, 90, 30, 100],
            item_lr: [0, 0, lr(-15), lr(-5), 0, 0, lr(-15), lr(-5), 0, 0, 0, 0, 0, 0, lr(-15), lr(-5), 0, 0, lr(-15), 0, lr(-15), lr(-5), lr(-15), 0, lr(-15), 0],
            rank_lr: [0, 5, 0, 5, 0, 5, 0, 5, 0, 5, 0, 5, 0, 5, 0, 5, 0, 5, 0, 5, 0, 5, lr(-25), 0, 0, 5, 0, 5, 0, 5],
            rank_type_gain: [40, 50, 50, 50, 50, 50, 50, 45, 40, 45, 40, 20, 40, 50, 50],
            rank_level_gain: 60,
            rank_duration: 180,
            out_smooth: 70,
        },
        /* ── 实战竞技: 高难本警示 (预警极端化) ──
         * 设计: 受击 90/状态 70/DOT 60 拉满生存预警(左重),
         * 命中 60 保持中等不吵; 静默 250ms 突显暴击, 反击 200%;
         * 与高振幅(纯重击)区分: 本预设重"生存感知"而非"打击感" */
        VibrationPreset {
            name: "实战竞技".to_string(),
            params: [100, 0, 0, 0, 65, 35, 60, 0, 0, 100, 0, 45, 55, 90, 70, 45, 40, 80, 55, 120, 120, 8, 30, 380, 35, 8, 40, 30, 4, 45, 500, 30, 1200, 60, 200, 30, 60, 30, 70, 1500, 180, 90, 250, 25, 800, 1500, 3000, 180, 70, 30, 60, 25, 200, 50, 250, 200, 90, 90, 30, 100],
            item_lr: [0, 0, 0, 5, 0, 0, 15, 0, 0, 0, 0, 0, 0, 0, 25, 10, 0, 0, 30, 10, 0, 20, lr(-10), 10, 30, 15],
            rank_lr: [0, 0, 30, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, lr(-20), 5, 0, 0, 20, 0, 30, 0],
            rank_type_gain: [60, 100, 90, 90, 90, 90, 85, 70, 80, 75, 90, 30, 90, 95, 100],
            rank_level_gain: 100,
            rank_duration: 380,
            out_smooth: 50,
        },
        /* 测试版: 全部为 0, 玩家自行设计 */
        VibrationPreset {
            name: "测试版(全0)".to_string(),
            params: [0; 60],
            item_lr: [0; 26],
            rank_lr: [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            rank_type_gain: [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            rank_level_gain: 0,
            rank_duration: 0,
            out_smooth: 0,
        },
    ]
}

/// Main application configuration structure.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct AppConfig {
    /// Display tray icon
    pub show_tray_icon: bool,
    /// Show notification messages
    pub show_notifications: bool,
    /// Keep window always on top
    #[serde(default)]
    pub always_on_top: bool,
    /// Use dark theme mode
    #[serde(default)]
    pub dark_mode: bool,
    /// Application language
    #[serde(default)]
    pub language: Language,
    /// 首次运行用户指引已阅读 (serde default false → 新装/旧配置首次都会看到一次)
    #[serde(default)]
    pub guide_seen: bool,
    /// 完整模式窗口矩形记忆 [x, y, w, h] (逻辑坐标)
    #[serde(default)]
    pub window_rect_normal: Option<[f32; 4]>,
    /// 极简模式窗口矩形记忆 [x, y, w, h] (逻辑坐标)
    #[serde(default)]
    pub window_rect_minimal: Option<[f32; 4]>,
    /// 启动时进入极简模式
    #[serde(default)]
    pub minimal_mode: bool,
    /// 极简模式震动预设来源 (false=通用 / true=全职业)
    #[serde(default)]
    pub minimal_vib_preset_job: bool,
    /// 是否为 DFO 玩家 (首次运行询问; false 时隐藏「通用震动」「全职业预设」入口,
    /// 可随时在设置中改回; serde default true = 老用户行为不变)
    #[serde(default = "default_dfo_player")]
    pub dfo_player: bool,
    /// 震动引擎路线: true = S1 ACT1 老方案 (配老版 DLL 的事件语义, 通道拆分/
    /// 合成式输出/量纲归一/重映射前置), false = S4+ 新方案 (默认, 现行引擎)。
    /// 仅 dfo_player=true 时有意义; 首启第三段弹窗询问, 设置里可随时切换。
    #[serde(default)]
    pub vib_legacy_client: bool,
    /// 首启「客户端版本」询问弹窗是否已展示过 (dfo_player=true 时只在首次弹)
    #[serde(default)]
    pub vib_edition_asked: bool,
    /// Toggle hotkey name
    pub switch_key: String,
    /// Key mapping configurations
    pub mappings: Vec<KeyMapping>,
    /// Input timeout in milliseconds
    #[serde(default = "default_input_timeout")]
    pub input_timeout: u64,
    /// Default key repeat interval in milliseconds
    #[serde(default = "default_interval")]
    pub interval: u64,
    /// Default key press duration in milliseconds
    #[serde(default = "default_event_duration")]
    pub event_duration: u64,
    /// Worker thread count (0 for auto-detection)
    #[serde(default = "default_worker_count")]
    pub worker_count: usize,
    /// Process whitelist (empty means all processes)
    #[serde(default)]
    pub process_whitelist: Vec<String>,
    /// 是否启用进程白名单过滤 (false = 全部放行, 列表保留不删)
    #[serde(default = "default_whitelist_enabled")]
    pub whitelist_enabled: bool,
    /// HID device baselines for button detection
    #[serde(default)]
    pub hid_baselines: Vec<HidDeviceBaseline>,
    /// Raw Input capture mode strategy
    #[serde(default = "default_capture_mode")]
    pub rawinput_capture_mode: String,
    /// XInput capture mode strategy
    #[serde(default = "default_xinput_capture_mode")]
    pub xinput_capture_mode: String,
    /// Device API preferences (VID:PID -> API preference)
    /// 序列化时按键名排序 (HashMap 迭代顺序不定, 避免每次保存文件抖动)
    #[serde(
        default,
        serialize_with = "serialize_sorted_device_prefs"
    )]
    pub device_api_preferences: HashMap<String, DeviceApiPreference>,
    /// Saved presets (named mapping snapshots)
    #[serde(default)]
    pub presets: Vec<Preset>,
    /// Currently active preset name (empty = none)
    #[serde(default)]
    pub current_preset: String,
    /// Vibration settings (DFO battle events -> gamepad motors)
    /// 实际持久化在 Vibration.toml (save_vibration_to_file), Config.toml 不写此节
    #[serde(default, skip_serializing)]
    pub vibration: VibrationConfig,
    /// Named vibration presets (built-in 3 + user defined)
    /// 实际持久化在 Vibration.toml, Config.toml 不写此节
    #[serde(default = "default_vibration_presets", skip_serializing)]
    pub vibration_presets: Vec<VibrationPreset>,
}

/// HID device baseline configuration for button state detection.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct HidDeviceBaseline {
    /// Device identifier (vendor_id, product_id, serial or handle)
    pub device_id: String,
    /// Baseline HID data (idle state with no buttons pressed)
    pub baseline_data: Vec<u8>,
}

/// Preset configuration storing a named snapshot of mappings.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct Preset {
    /// Preset display name
    pub name: String,
    /// Key mappings included in this preset
    #[serde(default)]
    pub mappings: Vec<KeyMapping>,
}

/// Key mapping configuration for trigger-target pairs.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct KeyMapping {
    /// Trigger key name
    pub trigger_key: String,
    /// Target keys to send (supports multiple keys for simultaneous press)
    /// Uses SmallVec with inline capacity of 4 to reduce heap allocations for common cases
    #[serde(default = "default_target_keys")]
    pub target_keys: SmallVec<[String; 4]>,
    /// Optional override for repeat interval
    #[serde(default)]
    pub interval: Option<u64>,
    /// Optional override for press duration
    #[serde(default)]
    pub event_duration: Option<u64>,
    /// Enable turbo mode (auto-repeat)
    #[serde(default = "default_turbo_enabled")]
    pub turbo_enabled: bool,
    /// Mouse move speed in pixels per move (only for mouse movement)
    #[serde(default = "default_move_speed")]
    pub move_speed: i32,
    /// Enable double-tap simulation on first press (e.g. DNF run)
    #[serde(default)]
    pub double_tap_enabled: bool,
    /// Gap between the two simulated taps in milliseconds
    #[serde(default = "default_double_tap_gap_ms")]
    pub double_tap_gap_ms: u64,
    /// User note/remark for this mapping
    #[serde(default)]
    pub note: String,
}

fn default_move_speed() -> i32 {
    5
}

/// 双击间隔默认值 (pub(crate): state.rs 测试模块复用)
pub(crate) fn default_double_tap_gap_ms() -> u64 {
    50
}

fn default_turbo_enabled() -> bool {
    true
}

fn default_target_keys() -> SmallVec<[String; 4]> {
    SmallVec::new()
}

impl KeyMapping {
    /// Gets the target keys slice
    pub fn get_target_keys(&self) -> &[String] {
        &self.target_keys
    }

    /// Adds a target key
    pub fn add_target_key(&mut self, key: String) {
        if !self.target_keys.contains(&key) {
            self.target_keys.push(key);
        }
    }

    /// Removes a target key
    pub fn remove_target_key(&mut self, key: &str) {
        self.target_keys.retain(|k| k != key);
    }

    /// Clears all target keys
    pub fn clear_target_keys(&mut self) {
        self.target_keys.clear();
    }

    /// Gets target keys as display string (comma separated)
    pub fn target_keys_display(&self) -> String {
        if self.target_keys.is_empty() {
            String::new()
        } else {
            self.target_keys.join(", ")
        }
    }
}

fn default_input_timeout() -> u64 {
    5
}
fn default_interval() -> u64 {
    5
}
fn default_event_duration() -> u64 {
    5
}
fn default_worker_count() -> usize {
    0 // 0 means auto-detect based on CPU cores
}
fn default_capture_mode() -> String {
    "MostSustained".to_string()
}
fn default_xinput_capture_mode() -> String {
    "MostSustained".to_string()
}

fn default_dfo_player() -> bool {
    true
}

fn default_whitelist_enabled() -> bool {
    true
}

/// device_api_preferences 按键名排序序列化 (HashMap 迭代顺序不定)
fn serialize_sorted_device_prefs<S>(
    m: &HashMap<String, DeviceApiPreference>,
    s: S,
) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    use serde::Serialize;
    let sorted: std::collections::BTreeMap<&String, &DeviceApiPreference> =
        m.iter().collect();
    sorted.serialize(s)
}

impl Default for AppConfig {
    /// Creates a default configuration with sensible defaults.
    fn default() -> Self {
        Self {
            show_tray_icon: true,
            show_notifications: true,
            always_on_top: false, // Default: not always on top for backward compatibility
            dark_mode: false,     // Default: light theme for backward compatibility
            language: Language::default(),
            guide_seen: false,
            window_rect_normal: None,
            window_rect_minimal: None,
            minimal_mode: false,
            minimal_vib_preset_job: false,
            dfo_player: default_dfo_player(),
            vib_legacy_client: false,
            vib_edition_asked: false,
            switch_key: "DELETE".to_string(),
            mappings: vec![KeyMapping {
                trigger_key: "Q".to_string(),
                target_keys: SmallVec::from_vec(vec!["Q".to_string()]),
                interval: None,
                event_duration: None,
                turbo_enabled: true,
                move_speed: 10,
                double_tap_enabled: false,
                double_tap_gap_ms: default_double_tap_gap_ms(),
                note: String::new(),
            }],
            input_timeout: default_input_timeout(),
            interval: default_interval(),
            event_duration: default_event_duration(),
            worker_count: default_worker_count(),
            process_whitelist: vec![], // Empty means all processes enabled
            whitelist_enabled: default_whitelist_enabled(),
            hid_baselines: Vec::new(),
            rawinput_capture_mode: default_capture_mode(),
            xinput_capture_mode: default_xinput_capture_mode(),
            device_api_preferences: HashMap::new(),
            presets: vec![Preset {
                name: "默认".to_string(),
                mappings: Vec::new(),
            }],
            current_preset: String::new(),
            vibration: VibrationConfig::default(),
            vibration_presets: default_vibration_presets(),
        }
    }
}

impl AppConfig {
    /// Loads configuration from file, creating default if not found.
    ///
    /// # Errors
    ///
    /// Returns an error if file operations fail.
    pub fn load_or_create<P: AsRef<Path>>(path: P) -> anyhow::Result<Self> {
        let mut config = if !path.as_ref().exists() {
            let default_config = Self::default();
            default_config.save_to_file(&path)?;
            default_config
        } else {
            Self::load_from_file(&path)?
        };
        let vib_path = Self::vibration_path_for(&path);
        config.load_vibration_from_file(&vib_path)?;
        Ok(config)
    }

    /// Returns the sibling Vibration.toml path for a Config.toml path.
    pub fn vibration_path_for<P: AsRef<Path>>(path: P) -> std::path::PathBuf {
        let p = path.as_ref();
        let dir = p.parent().unwrap_or_else(|| std::path::Path::new("."));
        dir.join("Vibration.toml")
    }

    /// Loads configuration from a TOML file.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be read or parsed.
    pub fn load_from_file<P: AsRef<Path>>(path: P) -> anyhow::Result<Self> {
        let content = fs::read_to_string(&path)?;
        let mut config: AppConfig = toml::from_str(&content)?;

        // Validate configuration
        if config.input_timeout < 2 {
            config.input_timeout = 2;
        }
        if config.interval < 5 {
            config.interval = 5;
        }
        if config.event_duration < 2 {
            config.event_duration = 2;
        }

        // Deduplicate process whitelist
        config.process_whitelist.sort();
        config.process_whitelist.dedup();

        // 注意: load 路径不得有任何写副作用。旧版曾在此对 LS_Left/LS_Right
        // 强制开启双击并 save_to_file——彼时 vibration 还是 serde 默认值,
        // 会把磁盘上的 Vibration.toml 整体覆盖成默认 (用户自建预设/调参清空),
        // 且用户手动关闭的双击每次启动都被改回。双击现在完全由 GUI/配置文件决定。

        Ok(config)
    }

    /// Saves configuration to a TOML file.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be written or serialized.
    pub fn save_to_file<P: AsRef<Path>>(&self, path: P) -> anyhow::Result<()> {
        // 纯 serde 序列化: 字段清单以 AppConfig 结构体为唯一事实来源,
        // 杜绝手工模板漏行 (此前 rawinput_capture_mode 与预设 double_tap_*
        // 曾静默丢失; 用户输入引号未转义、current_preset 归错节同源)。
        let header = "\
             # ═══════════════════════════════════════════════════════\n\
             #  🌸 Sorahk Configuration File 🌸\n\
             # ═══════════════════════════════════════════════════════\n\
             # 本文件由程序自动管理: 请在 GUI 内修改设置, 手改内容会在下次保存时被覆盖。\n\
             #\n\
             # 字段速查:\n\
             #   language = \"English\" / \"SimplifiedChinese\" / \"TraditionalChinese\" / \"Japanese\"\n\
             #   xinput_capture_mode / rawinput_capture_mode = \"DiagonalPriority\" / \"MostSustained\" / \"LastStable\"\n\
             #   组合键: 用 + 连接, 如 \"LALT+1\"; 左右修饰键区分: \"LALT1\" 只响应左 Alt\n\
             #   鼠标按键: LBUTTON / RBUTTON / MBUTTON / XBUTTON1 / XBUTTON2\n\
             #   鼠标移动/滚轮: MOUSE_UP / MOUSE_DOWN / MOUSE_LEFT / MOUSE_RIGHT / SCROLL_UP / SCROLL_DOWN\n\
             #   手柄: GAMEPAD_VID_按钮名, 组合用 + 连接, 如 \"GAMEPAD_045E_LS_RightUp+A\"\n\
             #   Raw Input 设备: DEVICE_VID_PID_SERIAL_Bx.x (首次使用会在 GUI 引导激活)\n\
             #\n\
             # 震动设置在旁边的 Vibration.toml; 全职业微调在 JobVibration.toml。\n\
             \n\
             ";
        let body = toml::to_string_pretty(self)?;
        fs::write(&path, format!("{header}{body}"))?;

        // Vibration settings are stored in the separate Vibration.toml
        // (see save_vibration_to_file / load_vibration_from_file).
        let vib_path = Self::vibration_path_for(&path);
        self.save_vibration_to_file(vib_path)?;
        Ok(())
    }
    /// Saves vibration settings + presets to the independent Vibration.toml file.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be written.
    pub fn save_vibration_to_file<P: AsRef<Path>>(&self, path: P) -> anyhow::Result<()> {
        #[derive(Serialize)]
        struct VibrationFile<'a> {
            vibration: &'a VibrationConfig,
            vibration_presets: &'a [VibrationPreset],
        }
        // 纯 serde 序列化 (同 save_to_file: 字段清单以结构体为唯一事实来源)。
        let header = "\
             # ═══════════════════════════════════════════════════════\n\
             #  🌸 Sorahk Vibration Settings (独立文件) 🌸\n\
             # ═══════════════════════════════════════════════════════\n\
             # 独立于 Config.toml, 由程序自动管理 (GUI 内修改, 手改会在下次保存时被覆盖)。\n\
             # 需要 us_extend_dll\\DfoVibration.dll 注入游戏才有震动事件来源。\n\
             # advanced_enabled 仅控制 UI 是否展开精细调校; 高级参数始终使用本文件保存的值。\n\
             \n\
             ";
        let doc = VibrationFile {
            vibration: &self.vibration,
            vibration_presets: &self.vibration_presets,
        };
        fs::write(path, format!("{header}{}", toml::to_string_pretty(&doc)?))?;
        Ok(())
    }
    /// Loads vibration settings from the independent Vibration.toml file.
    /// Returns Ok(false) if the file does not exist (caller keeps defaults).
    ///
    /// # Errors
    ///
    /// Returns an error if the file exists but cannot be parsed.
    pub fn load_vibration_from_file<P: AsRef<Path>>(&mut self, path: P) -> anyhow::Result<bool> {
        if !path.as_ref().exists() {
            return Ok(false);
        }
        let content = fs::read_to_string(path)?;
        #[derive(Deserialize)]
        struct VibFile {
            #[serde(default)]
            vibration: VibrationConfig,
            #[serde(default)]
            vibration_presets: Vec<VibrationPreset>,
        }
        let file: VibFile = toml::from_str(&content)?;
        if file.vibration.attack_gain != 0
            || file.vibration.master_gain != 0
            || file.vibration.advanced_enabled
            || !file.vibration_presets.is_empty()
        {
            self.vibration = file.vibration;
            if !file.vibration_presets.is_empty() {
                self.vibration_presets = file.vibration_presets;
            }
        }
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    fn get_test_config_path(name: &str) -> PathBuf {
        use std::time::{SystemTime, UNIX_EPOCH};
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let mut path = std::env::temp_dir();
        path.push(format!("sorahk_test_{}_{}.toml", name, timestamp));
        path
    }

    fn cleanup_test_file(path: &PathBuf) {
        let _ = fs::remove_file(path);
    }

    #[test]
    fn test_default_config_creation() {
        let config = AppConfig::default();

        assert!(config.show_tray_icon);
        assert!(config.show_notifications);
        assert!(!config.always_on_top);
        assert!(!config.dark_mode);
        assert_eq!(config.switch_key, "DELETE");
        assert_eq!(config.input_timeout, 5);
        assert_eq!(config.interval, 5);
        assert_eq!(config.event_duration, 5);
        assert_eq!(config.worker_count, 0);
        assert!(config.process_whitelist.is_empty());
        assert_eq!(config.mappings.len(), 1);
    }

    #[test]
    fn test_config_save_and_load() {
        let path = get_test_config_path("save_and_load");
        cleanup_test_file(&path); // Clean up before test

        let mut config = AppConfig::default();
        config.show_tray_icon = false;
        config.show_notifications = false;
        config.always_on_top = true;
        config.dark_mode = true;
        config.switch_key = "F12".to_string();
        config.input_timeout = 20;
        config.interval = 10;
        config.event_duration = 15;
        config.worker_count = 4;

        config.save_to_file(&path).expect("Failed to save config");

        let loaded_config = AppConfig::load_from_file(&path).expect("Failed to load config");

        assert_eq!(loaded_config.show_tray_icon, config.show_tray_icon);
        assert_eq!(loaded_config.show_notifications, config.show_notifications);
        assert_eq!(loaded_config.always_on_top, config.always_on_top);
        assert_eq!(loaded_config.dark_mode, config.dark_mode);
        assert_eq!(loaded_config.switch_key, config.switch_key);
        assert_eq!(loaded_config.input_timeout, config.input_timeout);
        assert_eq!(loaded_config.interval, config.interval);
        assert_eq!(loaded_config.event_duration, config.event_duration);
        assert_eq!(loaded_config.worker_count, config.worker_count);

        cleanup_test_file(&path);
    }

    #[test]
    fn test_config_validation_input_timeout() {
        let path = get_test_config_path("validation_timeout");
        cleanup_test_file(&path);

        let content = r#"
            show_tray_icon = true
            show_notifications = true
            switch_key = "DELETE"
            input_timeout = 1
            interval = 5
            event_duration = 5
            worker_count = 0
            process_whitelist = []
            mappings = []
        "#;

        fs::write(&path, content).expect("Failed to write test config");

        let config = AppConfig::load_from_file(&path).expect("Failed to load config");
        assert!(
            config.input_timeout >= 2,
            "Input timeout should be clamped to minimum 2"
        );

        cleanup_test_file(&path);
    }

    #[test]
    fn test_config_validation_interval() {
        let path = get_test_config_path("validation_interval");
        cleanup_test_file(&path);

        let content = r#"
            show_tray_icon = true
            show_notifications = true
            switch_key = "DELETE"
            input_timeout = 10
            interval = 2
            event_duration = 5
            worker_count = 0
            process_whitelist = []
            mappings = []
        "#;

        fs::write(&path, content).expect("Failed to write test config");

        let config = AppConfig::load_from_file(&path).expect("Failed to load config");
        assert!(
            config.interval >= 5,
            "Interval should be clamped to minimum 5"
        );

        cleanup_test_file(&path);
    }

    #[test]
    fn test_config_validation_event_duration() {
        let path = get_test_config_path("validation_duration");
        cleanup_test_file(&path);

        let content = r#"
            show_tray_icon = true
            show_notifications = true
            switch_key = "DELETE"
            input_timeout = 10
            interval = 5
            event_duration = 2
            worker_count = 0
            process_whitelist = []
            mappings = []
        "#;

        fs::write(&path, content).expect("Failed to write test config");

        let config = AppConfig::load_from_file(&path).expect("Failed to load config");
        assert!(
            config.event_duration >= 2,
            "Event duration should be clamped to minimum 2"
        );

        cleanup_test_file(&path);
    }

    #[test]
    fn test_load_or_create_missing_file() {
        let path = get_test_config_path("missing_file");
        cleanup_test_file(&path);

        let config = AppConfig::load_or_create(&path).expect("Failed to load or create config");

        assert!(path.exists(), "Config file should be created");
        assert_eq!(config.switch_key, "DELETE");
        assert_eq!(config.interval, 5);

        cleanup_test_file(&path);
    }

    #[test]
    fn test_load_or_create_existing_file() {
        let path = get_test_config_path("existing_file");
        cleanup_test_file(&path);

        let mut config = AppConfig::default();
        config.switch_key = "F11".to_string();
        config.save_to_file(&path).expect("Failed to save config");

        let loaded_config = AppConfig::load_or_create(&path).expect("Failed to load config");

        assert_eq!(loaded_config.switch_key, "F11");

        cleanup_test_file(&path);
    }

    #[test]
    fn test_key_mapping_with_overrides() {
        let mapping = 
            KeyMapping {
            trigger_key: "A".to_string(),
            target_keys: SmallVec::from_vec(vec!["B".to_string()]),
            interval: Some(10),
            event_duration: Some(8),
            turbo_enabled: true,
            move_speed: 5,
            double_tap_enabled: false,
            double_tap_gap_ms: default_double_tap_gap_ms(),

            note: String::new(),
        };

        assert_eq!(mapping.trigger_key, "A");
        assert_eq!(mapping.target_keys.as_slice(), &["B".to_string()]);
        assert_eq!(mapping.interval, Some(10));
        assert_eq!(mapping.event_duration, Some(8));
    }

    #[test]
    fn test_key_mapping_without_overrides() {
        let mapping = 
            KeyMapping {
            trigger_key: "C".to_string(),
            target_keys: SmallVec::from_vec(vec!["D".to_string()]),
            interval: None,
            event_duration: None,
            turbo_enabled: true,
            move_speed: 10,            double_tap_enabled: false,
            double_tap_gap_ms: default_double_tap_gap_ms(),

            note: String::new(),
        };

        assert_eq!(mapping.trigger_key, "C");
        assert_eq!(mapping.target_keys.as_slice(), &["D".to_string()]);
        assert_eq!(mapping.interval, None);
        assert_eq!(mapping.event_duration, None);
    }

    #[test]
    fn test_process_whitelist_serialization() {
        let path = get_test_config_path("whitelist");
        cleanup_test_file(&path);

        let mut config = AppConfig::default();
        config.process_whitelist = vec!["notepad.exe".to_string(), "chrome.exe".to_string()];

        config.save_to_file(&path).expect("Failed to save config");

        let loaded_config = AppConfig::load_from_file(&path).expect("Failed to load config");

        assert_eq!(loaded_config.process_whitelist.len(), 2);
        assert!(
            loaded_config
                .process_whitelist
                .contains(&"notepad.exe".to_string())
        );
        assert!(
            loaded_config
                .process_whitelist
                .contains(&"chrome.exe".to_string())
        );

        cleanup_test_file(&path);
    }

    #[test]
    fn test_language_serialization() {
        let languages = vec![
            Language::English,
            Language::SimplifiedChinese,
            Language::TraditionalChinese,
            Language::Japanese,
        ];

        for (idx, lang) in languages.iter().enumerate() {
            let path = get_test_config_path(&format!("language_{}", idx));
            cleanup_test_file(&path);

            let mut config = AppConfig::default();
            config.language = *lang;

            config.save_to_file(&path).expect("Failed to save config");
            let loaded_config = AppConfig::load_from_file(&path).expect("Failed to load config");

            assert_eq!(loaded_config.language, *lang);

            cleanup_test_file(&path);
        }
    }

    #[test]
    fn test_multiple_mappings_serialization() {
        let path = get_test_config_path("multiple_mappings");
        cleanup_test_file(&path);

        let mut config = AppConfig::default();
        config.mappings = vec![
            
                KeyMapping {
                trigger_key: "A".to_string(),
                target_keys: SmallVec::from_vec(vec!["1".to_string()]),
                interval: Some(10),
                event_duration: Some(5),
                turbo_enabled: true,
                move_speed: 10,                double_tap_enabled: false,
                double_tap_gap_ms: default_double_tap_gap_ms(),

                note: String::new(),
            },
            
                KeyMapping {
                trigger_key: "B".to_string(),
                target_keys: SmallVec::from_vec(vec!["2".to_string()]),
                interval: None,
                event_duration: None,
                turbo_enabled: true,
                move_speed: 10,                double_tap_enabled: false,
                double_tap_gap_ms: default_double_tap_gap_ms(),

                note: String::new(),
            },
            
                KeyMapping {
                trigger_key: "F1".to_string(),
                target_keys: SmallVec::from_vec(vec!["SPACE".to_string()]),
                interval: Some(20),
                event_duration: Some(10),
                turbo_enabled: true,
                move_speed: 10,                double_tap_enabled: false,
                double_tap_gap_ms: default_double_tap_gap_ms(),

                note: String::new(),
            },
        ];

        config.save_to_file(&path).expect("Failed to save config");
        let loaded_config = AppConfig::load_from_file(&path).expect("Failed to load config");

        assert_eq!(loaded_config.mappings.len(), 3);
        assert_eq!(loaded_config.mappings[0].trigger_key, "A");
        assert_eq!(
            loaded_config.mappings[1].target_keys.as_slice(),
            &["2".to_string()]
        );
        assert_eq!(loaded_config.mappings[2].interval, Some(20));

        cleanup_test_file(&path);
    }

    #[test]
    fn test_config_load_invalid_toml() {
        let path = get_test_config_path("invalid_toml");
        cleanup_test_file(&path);

        // Write invalid TOML
        std::fs::write(&path, "invalid [[ toml \n syntax").expect("Failed to write");

        let result = AppConfig::load_from_file(&path);
        assert!(result.is_err());

        cleanup_test_file(&path);
    }

    #[test]
    fn test_config_load_nonexistent_file() {
        let path = get_test_config_path("nonexistent");

        // Ensure file doesn't exist
        let _ = std::fs::remove_file(&path);

        let result = AppConfig::load_from_file(&path);
        assert!(result.is_err());
    }

    #[test]
    fn test_config_with_extreme_values() {
        let path = get_test_config_path("extreme_values");
        cleanup_test_file(&path);

        let mut config = AppConfig::default();
        config.interval = 1000; // Very large interval
        config.event_duration = 500;
        config.input_timeout = 10000;
        config.worker_count = 64; // Large worker count

        config.save_to_file(&path).expect("Failed to save");
        let loaded = AppConfig::load_from_file(&path).expect("Failed to load");

        assert_eq!(loaded.interval, 1000);
        assert_eq!(loaded.event_duration, 500);
        assert_eq!(loaded.input_timeout, 10000);
        assert_eq!(loaded.worker_count, 64);

        cleanup_test_file(&path);
    }

    #[test]
    fn test_config_with_special_characters_in_process_name() {
        let path = get_test_config_path("special_chars");
        cleanup_test_file(&path);

        let mut config = AppConfig::default();
        config.process_whitelist = vec![
            "app-name.exe".to_string(),
            "app_name.exe".to_string(),
            "app123.exe".to_string(),
        ];

        config.save_to_file(&path).expect("Failed to save");
        let loaded = AppConfig::load_from_file(&path).expect("Failed to load");

        assert_eq!(loaded.process_whitelist.len(), 3);
        assert!(
            loaded
                .process_whitelist
                .contains(&"app-name.exe".to_string())
        );

        cleanup_test_file(&path);
    }

    #[test]
    fn test_config_save_to_readonly_path() {
        // This test verifies error handling for readonly paths
        // On Windows, we can't easily create readonly directories in tests
        // so we test with an invalid path
        let path = PathBuf::from("/nonexistent/invalid/path/config.toml");

        let config = AppConfig::default();
        let result = config.save_to_file(&path);

        assert!(result.is_err());
    }

    #[test]
    fn test_config_language_default() {
        let config = AppConfig::default();
        assert_eq!(config.language, Language::default());
    }

    #[test]
    fn test_config_with_duplicate_process_names() {
        let path = get_test_config_path("duplicate_processes");
        cleanup_test_file(&path);

        let mut config = AppConfig::default();
        config.process_whitelist = vec![
            "app.exe".to_string(),
            "app.exe".to_string(), // Duplicate
            "other.exe".to_string(),
        ];

        config.save_to_file(&path).expect("Failed to save");
        let loaded = AppConfig::load_from_file(&path).expect("Failed to load");

        assert_eq!(loaded.process_whitelist.len(), 2);

        cleanup_test_file(&path);
    }

    #[test]
    fn test_config_validation_clamps_negative_values() {
        let path = get_test_config_path("negative_values");
        cleanup_test_file(&path);

        // Manually write config with "negative" (actually minimum) values
        let content = r#"
            show_tray_icon = true
            show_notifications = true
            switch_key = "DELETE"
            input_timeout = 1
            interval = 1
            event_duration = 1
            worker_count = 0
            process_whitelist = []
            mappings = []
        "#;

        std::fs::write(&path, content).expect("Failed to write");
        let loaded = AppConfig::load_from_file(&path).expect("Failed to load");

        // Values should be clamped to minimums
        assert!(loaded.input_timeout >= 2);
        assert!(loaded.interval >= 5);
        assert!(loaded.event_duration >= 2);

        cleanup_test_file(&path);
    }

    #[test]
    fn test_config_deduplicates_process_whitelist() {
        // Test that duplicate processes are automatically removed when loading config
        let path = get_test_config_path("process_dedup");
        cleanup_test_file(&path);

        let content = r#"
            show_tray_icon = true
            show_notifications = true
            switch_key = "DELETE"
            input_timeout = 10
            interval = 5
            event_duration = 5
            worker_count = 0
            process_whitelist = ["chrome.exe", "notepad.exe", "chrome.exe", "firefox.exe", "notepad.exe"]
            mappings = []
        "#;

        std::fs::write(&path, content).expect("Failed to write test config");
        let loaded = AppConfig::load_from_file(&path).expect("Failed to load config");

        // Should have exactly 3 unique processes after deduplication
        assert_eq!(loaded.process_whitelist.len(), 3);
        assert!(loaded.process_whitelist.contains(&"chrome.exe".to_string()));
        assert!(
            loaded
                .process_whitelist
                .contains(&"notepad.exe".to_string())
        );
        assert!(
            loaded
                .process_whitelist
                .contains(&"firefox.exe".to_string())
        );

        cleanup_test_file(&path);
    }

    #[test]
    fn test_multiple_target_keys_single() {
        let mapping = 
            KeyMapping {
            trigger_key: "A".to_string(),
            target_keys: SmallVec::from_vec(vec!["B".to_string()]),
            interval: None,
            event_duration: None,
            turbo_enabled: true,
            move_speed: 5,
            double_tap_enabled: false,
            double_tap_gap_ms: default_double_tap_gap_ms(),

            note: String::new(),
        };

        assert_eq!(mapping.target_keys.len(), 1);
        assert_eq!(mapping.target_keys_display(), "B");
    }

    #[test]
    fn test_multiple_target_keys_multiple() {
        let mapping = 
            KeyMapping {
            trigger_key: "A".to_string(),
            target_keys: SmallVec::from_vec(vec!["MOUSE_UP".to_string(), "MOUSE_LEFT".to_string()]),
            interval: None,
            event_duration: None,
            turbo_enabled: true,
            move_speed: 10,            double_tap_enabled: false,
            double_tap_gap_ms: default_double_tap_gap_ms(),

            note: String::new(),
        };

        assert_eq!(mapping.target_keys.len(), 2);
        assert_eq!(mapping.target_keys_display(), "MOUSE_UP, MOUSE_LEFT");
    }

    #[test]
    fn test_multiple_target_keys_empty() {
        let mapping = 
            KeyMapping {
            trigger_key: "A".to_string(),
            target_keys: SmallVec::new(),
            interval: None,
            event_duration: None,
            turbo_enabled: true,
            move_speed: 10,            double_tap_enabled: false,
            double_tap_gap_ms: default_double_tap_gap_ms(),

            note: String::new(),
        };

        assert_eq!(mapping.target_keys.len(), 0);
        assert_eq!(mapping.target_keys_display(), "");
    }

    #[test]
    fn test_add_target_key() {
        let mut mapping = 
            KeyMapping {
            trigger_key: "A".to_string(),
            target_keys: SmallVec::from_vec(vec!["B".to_string()]),
            interval: None,
            event_duration: None,
            turbo_enabled: true,
            move_speed: 10,            double_tap_enabled: false,
            double_tap_gap_ms: default_double_tap_gap_ms(),

            note: String::new(),
        };

        mapping.add_target_key("C".to_string());
        assert_eq!(mapping.target_keys.len(), 2);
        assert_eq!(mapping.target_keys[1], "C");

        // Adding duplicate should not increase count
        mapping.add_target_key("B".to_string());
        assert_eq!(mapping.target_keys.len(), 2);
    }

    #[test]
    fn test_remove_target_key() {
        let mut mapping = 
            KeyMapping {
            trigger_key: "A".to_string(),
            target_keys: SmallVec::from_vec(vec![
                "B".to_string(),
                "C".to_string(),
                "D".to_string(),
            ]),
            interval: None,
            event_duration: None,
            turbo_enabled: true,
            move_speed: 10,            double_tap_enabled: false,
            double_tap_gap_ms: default_double_tap_gap_ms(),

            note: String::new(),
        };

        mapping.remove_target_key("C");
        assert_eq!(mapping.target_keys.len(), 2);
        assert_eq!(mapping.target_keys[0], "B");
        assert_eq!(mapping.target_keys[1], "D");
    }

    #[test]
    fn test_clear_target_keys() {
        let mut mapping = 
            KeyMapping {
            trigger_key: "A".to_string(),
            target_keys: SmallVec::from_vec(vec!["B".to_string(), "C".to_string()]),
            interval: None,
            event_duration: None,
            turbo_enabled: true,
            move_speed: 10,            double_tap_enabled: false,
            double_tap_gap_ms: default_double_tap_gap_ms(),

            note: String::new(),
        };

        mapping.clear_target_keys();
        assert_eq!(mapping.target_keys.len(), 0);
    }

    #[test]
    fn test_multiple_target_keys_serialization() {
        let path = get_test_config_path("multi_target_keys");
        cleanup_test_file(&path);

        let mut config = AppConfig::default();
        config.mappings = vec![
            
                KeyMapping {
                trigger_key: "Q".to_string(),
                target_keys: SmallVec::from_vec(vec![
                    "MOUSE_UP".to_string(),
                    "MOUSE_LEFT".to_string(),
                ]),
                interval: Some(5),
                event_duration: None,
                turbo_enabled: true,
                move_speed: 10,                double_tap_enabled: false,
                double_tap_gap_ms: default_double_tap_gap_ms(),

                note: String::new(),
            },
            
                KeyMapping {
                trigger_key: "E".to_string(),
                target_keys: SmallVec::from_vec(vec![
                    "MOUSE_UP".to_string(),
                    "MOUSE_RIGHT".to_string(),
                ]),
                interval: Some(5),
                event_duration: None,
                turbo_enabled: true,
                move_speed: 10,                double_tap_enabled: false,
                double_tap_gap_ms: default_double_tap_gap_ms(),

                note: String::new(),
            },
        ];

        config.save_to_file(&path).expect("Failed to save config");
        let loaded_config = AppConfig::load_from_file(&path).expect("Failed to load config");

        assert_eq!(loaded_config.mappings.len(), 2);
        assert_eq!(loaded_config.mappings[0].target_keys.len(), 2);
        assert_eq!(loaded_config.mappings[0].target_keys[0], "MOUSE_UP");
        assert_eq!(loaded_config.mappings[0].target_keys[1], "MOUSE_LEFT");
        assert_eq!(loaded_config.mappings[1].target_keys.len(), 2);
        assert_eq!(loaded_config.mappings[1].target_keys[0], "MOUSE_UP");
        assert_eq!(loaded_config.mappings[1].target_keys[1], "MOUSE_RIGHT");

        cleanup_test_file(&path);
    }

    #[test]
    fn test_smallvec_inline_capacity() {
        // Test that SmallVec uses inline storage for small collections
        let small_vec: SmallVec<[String; 4]> =
            SmallVec::from_vec(vec!["A".to_string(), "B".to_string(), "C".to_string()]);

        // Should be stored inline (capacity <= 4)
        assert_eq!(small_vec.len(), 3);
        assert!(small_vec.spilled() == false); // Not heap allocated

        let mut large_vec: SmallVec<[String; 4]> = SmallVec::new();
        for i in 0..6 {
            large_vec.push(format!("KEY_{}", i));
        }

        // Should spill to heap (capacity > 4)
        assert_eq!(large_vec.len(), 6);
        assert!(large_vec.spilled()); // Heap allocated
    }

    #[test]
    fn test_get_target_keys() {
        let mapping = 
            KeyMapping {
            trigger_key: "A".to_string(),
            target_keys: SmallVec::from_vec(vec!["B".to_string(), "C".to_string()]),
            interval: None,
            event_duration: None,
            turbo_enabled: true,
            move_speed: 10,            double_tap_enabled: false,
            double_tap_gap_ms: default_double_tap_gap_ms(),

            note: String::new(),
        };

        let keys = mapping.get_target_keys();
        assert_eq!(keys.len(), 2);
        assert_eq!(keys[0], "B");
        assert_eq!(keys[1], "C");
    }

    #[test]
    fn test_empty_target_keys_serialization() {
        let path = get_test_config_path("empty_target_keys");
        cleanup_test_file(&path);

        let mut config = AppConfig::default();
        config.mappings = vec![
            KeyMapping {
            trigger_key: "A".to_string(),
            target_keys: SmallVec::new(),
            interval: None,
            event_duration: None,
            turbo_enabled: true,
            move_speed: 10,            double_tap_enabled: false,
            double_tap_gap_ms: default_double_tap_gap_ms(),

            note: String::new(),
        }];

        config.save_to_file(&path).expect("Failed to save config");
        let loaded_config = AppConfig::load_from_file(&path).expect("Failed to load config");

        assert_eq!(loaded_config.mappings.len(), 1);
        assert_eq!(loaded_config.mappings[0].target_keys.len(), 0);

        cleanup_test_file(&path);
    }

    #[test]
    fn test_many_target_keys_serialization() {
        let path = get_test_config_path("many_target_keys");
        cleanup_test_file(&path);

        let mut config = AppConfig::default();
        config.mappings = vec![
            KeyMapping {
            trigger_key: "A".to_string(),
            target_keys: SmallVec::from_vec(vec![
                "1".to_string(),
                "2".to_string(),
                "3".to_string(),
                "4".to_string(),
                "5".to_string(),
                "6".to_string(),
            ]),
            interval: None,
            event_duration: None,
            turbo_enabled: true,
            move_speed: 10,            double_tap_enabled: false,
            double_tap_gap_ms: default_double_tap_gap_ms(),

            note: String::new(),
        }];

        config.save_to_file(&path).expect("Failed to save config");
        let loaded_config = AppConfig::load_from_file(&path).expect("Failed to load config");

        assert_eq!(loaded_config.mappings.len(), 1);
        assert_eq!(loaded_config.mappings[0].target_keys.len(), 6);
        assert_eq!(loaded_config.mappings[0].target_keys[5], "6");

        cleanup_test_file(&path);
    }
}
