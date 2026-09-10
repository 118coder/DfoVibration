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
    /// 群怪聚合·合并保留 % (v14.1, S1 老方案高级算法): 注入窗内事件按通道记账
    /// 不丢弃, 下一发兑现 (能量携带) —— 群怪洪流"单击变厚"而非"变密变吵"。
    /// 每记一笔保留强度 % (边际递减), **0 = 关闭聚合 (回到丢弃语义)**
    #[serde(default = "default_vib_merge_keep")]
    pub merge_keep: u32,
    /// 群怪聚合·合并封顶 % (记账+本击强度之和的上限, 100 = 老滑块满幅)
    #[serde(default = "default_vib_merge_cap")]
    pub merge_cap: u32,
    /// 群怪聚合·补发窗口 ms (记账到期无后续注入 → tick 补发收尾脉冲, 0=不补发)
    #[serde(default = "default_vib_merge_hold")]
    pub merge_hold: u32,
    /// 命中限频·窗口内最多生效次数 (v14.2, S1 高级算法, 仅普通命中通道):
    /// 窗口秒内命中脉冲超过此数后, 超额命中不再单独注入而是**全额记账**进
    /// 聚合能量 (下一发兑现变厚/到期补发收尾) —— 频率上限转化为厚度,
    /// 每一击都不丢。**0 = 关闭限频**
    #[serde(default = "default_vib_hitcap_max")]
    pub hitcap_max: u32,
    /// 命中限频·窗口 ms (配合 hitcap_max 的翻滚窗口)
    #[serde(default = "default_vib_hitcap_win")]
    pub hitcap_win_ms: u32,
    /// 命中聚合窗 ms (v15.2, 仅 S1 命中通道): 群怪一刀产生多条命中事件
    /// (每怪一条), 窗内全部记账合并 —— **一刀只震一下 (更厚)**, 能量不丢。
    /// 默认 60; 调大 = 一刀多怪更彻底地合并
    #[serde(default = "default_vib_hitmerge")]
    pub hitmerge_ms: u32,
    /// 持续压制·触发秒数 (v15.3, 仅 S1 命中通道): 命中限频持续咬合该秒数后,
    /// 命中震动自动再降一档 (狂战士血之狂暴等持续性双倍打击的自动降温)。
    /// **0 = 关闭持续压制**
    #[serde(default = "default_vib_sustain_secs")]
    pub sustain_secs: u32,
    /// 持续压制·降幅 % (咬合超时后命中震动额外降低的比例)
    #[serde(default = "default_vib_sustain_reduce")]
    pub sustain_reduce: u32,
    /// 脉冲落地·阈值 % (v15, S1 高级算法): 高负载期 (密度自适应激活/命中限频
    /// 咬合) 衰减尾巴降到本脉冲峰值的此比例即归零, 脉冲间出真静音。
    /// **0 = 关闭落地** (维持自然衰减尾巴)
    #[serde(default = "default_vib_tail_land")]
    pub tail_land_pct: u32,
    /// 怪物异常反馈·强度 % (v15, 仅 S1/ACT 路线): 怪物身上出血/中毒等异常
    /// 状态跳字的独立强度 (0x04 通道, 从"装备特效"彻底拆出, S4 不走此通道)。
    /// 注意 S1 有重映射 20% 抬底: 1 实感约 20% 满幅, **0 = 完全关闭**
    #[serde(default = "default_vib_monster_abnormal")]
    pub monster_abnormal_gain: u32,
    /// 命中风暴·阈值 (v16, 仅 S1 命中通道): 风暴统计窗内纯命中事件 (受伤类
    /// 受击/出血/DOT 全部排除) 超过此数 → 进入风暴, 只保留 storm_keep_pct%
    /// 的命中震动 (抽样丢弃, 不记账不补发), 停手超过 storm_pause_ms 即恢复
    /// 刀刀震动。默认 10 条 (用户明确要求开启, 规范 §9.3 例外)。
    /// **0 = 关闭风暴抽样 (回到旧行为)**
    #[serde(default = "default_vib_storm_thr")]
    pub storm_thr: u32,
    /// 命中风暴·保留比例 % (风暴期每 N 条命中保留 1 条, N = 100÷此值; 50=两条震一条)
    #[serde(default = "default_vib_storm_keep")]
    pub storm_keep_pct: u32,
    /// 命中风暴·统计窗口 ms (窗内纯命中事件计数, 达阈值即入风暴)
    #[serde(default = "default_vib_storm_win")]
    pub storm_win_ms: u32,
    /// 命中风暴·恢复暂停 ms (距上一条纯命中事件超过此时长 → 立即恢复刀刀震动)
    #[serde(default = "default_vib_storm_pause")]
    pub storm_pause_ms: u32,
    /// 命中风暴·风暴期间暂时静音怪物异常反馈 (v16.6, 仅 S1): 勾选后风暴期间
    /// 0x04 怪物出血/中毒跳字不再震动 (零痕迹: 不打锚/不计密度/不进记账),
    /// 风暴结束或停手超过 storm_pause_ms 自动恢复。**默认 false = 旧行为**
    #[serde(default)]
    pub storm_mute_abnormal: bool,
    /// 命中风暴·风暴期间暂时静音评分点系统 (v16.7, 仅 S1): 勾选后风暴期间
    /// 评分族事件 (评分等级脉冲/评分点/闪避/暴击/破招/背击/最终击杀/凌空追击/
    /// 第一击/增益叠加/释放技能/镜头震动/技能震动/暴击特写/怪物死亡) 零痕迹
    /// 静音; 移动持续震动不属于评分点系统, 不受影响。恢复同上。
    /// **默认 false = 旧行为**
    #[serde(default)]
    pub storm_mute_rank: bool,
    /* ── ★高级算法总开关 (v16.8, 用户定稿: 每个高级算法大选项一个开关) ──
     * 语义: false = 该算法回到旧行为 (等效于把其参数按 0/关 语义覆盖),
     * 滑块值保留不动 (重新勾选即恢复)。全部默认 true = 现行行为。
     * 风暴检测 (storm_active) 与抽样开关解耦: 抽样关掉时仍持续检测,
     * 供两个"风暴期静音"开关使用 (修 v16.6/16.7 静音被阈值 0 掐死的 BUG)。 */
    /// 一刀多怪合并 (命中聚合窗 + 聚合记账) 总开关
    #[serde(default = "default_vib_true")]
    pub merge_enabled: bool,
    /// 命中限频 总开关
    #[serde(default = "default_vib_true")]
    pub hitcap_enabled: bool,
    /// 命中风暴抽样 总开关 (关 = 不抽样, 但风暴检测继续为静音开关服务)
    #[serde(default = "default_vib_true")]
    pub storm_enabled: bool,
    /// 持续降温 总开关
    #[serde(default = "default_vib_true")]
    pub sustain_enabled: bool,
    /// 脉冲落地 总开关
    #[serde(default = "default_vib_true")]
    pub tail_land_enabled: bool,
    /// 连击密度自适应 总开关
    #[serde(default = "default_vib_true")]
    pub density_enabled: bool,
    /// 命中自适应 (打太快自动减轻) 总开关
    #[serde(default = "default_vib_true")]
    pub adapt_enabled: bool,
    /// 走位能量积累 (移动积累增强) 总开关
    #[serde(default = "default_vib_true")]
    pub move_charge_enabled: bool,
    /// 各类事件衰减时长 自定义开关 (false = 用内置默认衰减时长, 滑块不生效)
    #[serde(default = "default_vib_true")]
    pub decay_enabled: bool,
    /// 算法窗口时长 (持续/节奏/爆发/连击统计) 自定义开关 (false = 用内置默认窗口)
    #[serde(default = "default_vib_true")]
    pub algo_windows_enabled: bool,
    /// 静默/反击/脉冲 开关 (false = 不再静默/不再反击加成/不再发脉冲; 测试强度保留)
    #[serde(default = "default_vib_true")]
    pub pulse_enabled: bool,
    /// ★统合衰减期 (v17, 仅 S1 风暴期, 默认关): 风暴期普通命中通道改用一条"统合
    /// 包络" —— 每次命中把旧包络瞬间清零后重新起振, 衰减时间统一固定, 到点硬归零。
    /// 连续命中 = 一击接一击的干净持续震动 (旧尾巴不再叠糊)。开启时风暴抽样与
    /// 命中限频/聚合记账对该通道让路 (每一击都重新起振)。
    #[serde(default)]
    pub storm_unified_enabled: bool,
    /// 统合衰减期时长 ms (固定衰减周期, 40-400)
    #[serde(default = "default_vib_storm_unified_ms")]
    pub storm_unified_ms: u32,
    /// ★S1 输出引擎模式 (v20, 仅 S1/ACT 路线; S4 路线不读此字段, 红线):
    /// 0 = 经典 ACT1 管线 (默认, 逐字节不变: 前置抬底 + 15% 只杀真零);
    /// 1 = S4 纯输出整形 (迟滞死区 + 重映射在后; 低于死区的轻反馈会被吞掉);
    /// 2 = S4+不丢震动 (S4 整形 + 低于死区的能量后置携带, 攒厚再出, 不丢击)。
    /// 模式 1/2 下 S4 的死区%/重映射下限% 旋钮对 S1 解锁可调。
    #[serde(default)]
    pub legacy_output_mode: u32,
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
            merge_keep: default_vib_merge_keep(),
            merge_cap: default_vib_merge_cap(),
            merge_hold: default_vib_merge_hold(),
            hitcap_max: default_vib_hitcap_max(),
            hitcap_win_ms: default_vib_hitcap_win(),
            hitmerge_ms: default_vib_hitmerge(),
            sustain_secs: default_vib_sustain_secs(),
            sustain_reduce: default_vib_sustain_reduce(),
            tail_land_pct: default_vib_tail_land(),
            monster_abnormal_gain: default_vib_monster_abnormal(),
            storm_thr: default_vib_storm_thr(),
            storm_keep_pct: default_vib_storm_keep(),
            storm_win_ms: default_vib_storm_win(),
            storm_pause_ms: default_vib_storm_pause(),
            storm_mute_abnormal: false,
            storm_mute_rank: false,
            merge_enabled: true,
            hitcap_enabled: true,
            storm_enabled: true,
            sustain_enabled: true,
            tail_land_enabled: true,
            density_enabled: true,
            adapt_enabled: true,
            move_charge_enabled: true,
            decay_enabled: true,
            algo_windows_enabled: true,
            pulse_enabled: true,
            storm_unified_enabled: false,
            storm_unified_ms: default_vib_storm_unified_ms(),
            legacy_output_mode: 0,
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

/* 群怪聚合 (v14.1) 默认参数: 开启 (keep 80 / cap 100 / 补发 80ms)。
 * keep=0 即关闭 —— 玩家可在震动页「群怪聚合」滑块组自由调控 */
fn default_vib_merge_keep() -> u32 { 80 }
fn default_vib_merge_cap() -> u32 { 100 }
fn default_vib_merge_hold() -> u32 { 80 }

/* 命中限频 (v14.2) 默认参数: 1 秒内命中脉冲最多 6 次 (0=关闭)。
 * 单挑普攻 2-4 次/秒永不触发, 群怪洪流封顶 6 脉冲/秒;
 * 超额命中全额记账进聚合能量, 每一击都不丢。 */
fn default_vib_hitcap_max() -> u32 { 6 }
fn default_vib_hitcap_win() -> u32 { 1000 }
/* 命中聚合窗 (v15.2) 默认 60ms: 群怪一刀的多条命中事件窗内合并, 一刀一震 */
fn default_vib_hitmerge() -> u32 { 60 }

/* 持续压制 (v15.3) 默认: 命中限频持续咬合 3 秒后, 命中震动再降 30%
 * (狂战士血之狂暴等持续性双倍打击的自动降温; 0 秒 = 关闭) */
fn default_vib_sustain_secs() -> u32 { 3 }
fn default_vib_sustain_reduce() -> u32 { 30 }

/* 脉冲落地 (v15) 默认 25%: 高负载期尾巴降到峰值 25% 即归零 (0=关闭) */
fn default_vib_tail_land() -> u32 { 25 }
/* 怪物异常反馈 (v15) 默认 1%: 出血/中毒跳字压到最低档 (0=完全关闭;
 * 注意 S1 重映射 20% 抬底, 1 实感约 20% 满幅) */
fn default_vib_monster_abnormal() -> u32 { 1 }

/* 命中风暴抽样 (v16) 默认: 1 秒窗内纯命中事件 ≥10 条 → 只保留一半震动
 * (每两条震一条), 停手 400ms 即恢复刀刀震 (0 = 关闭)。
 * 狂战士血之狂暴双倍打击+群怪场景命中事件暴增专用; 用户明确要求默认开启
 * (规范 §9.3 例外), 单挑慢速攻击 (1 秒 10 条以下) 永不触发。 */
fn default_vib_storm_thr() -> u32 { 10 }
fn default_vib_storm_keep() -> u32 { 50 }
fn default_vib_storm_win() -> u32 { 1000 }
fn default_vib_storm_pause() -> u32 { 400 }
/* 统合衰减期 (v17) 默认 120ms: 风暴期固定衰减周期 (开关默认关) */
fn default_vib_storm_unified_ms() -> u32 { 120 }

/* ★ACT1 特供预设附加默认 (v16.8, 用户定稿): 应用「ACT1 特供」时同时开启
 * 高级调校, 并默认勾选两个风暴期静音 (怪物异常反馈 / 评分点系统)。
 * 纯函数 (便于测试); GUI 应用预设时调用并同步原子量。 */
pub fn act1_preset_extras(vib: &mut VibrationConfig) {
    vib.advanced_enabled = true;
    vib.storm_mute_abnormal = true;
    vib.storm_mute_rank = true;
}

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
    /// ★v18: 用户是否主动覆盖过该预设 (保存为同名)。false = 内置管理条目:
    /// 应用时用内置值, 且列表条目自愈回内置值 (调乱可一键回归); true = 用户版本优先
    #[serde(default)]
    pub user_modified: bool,
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
            user_modified: false,
        },
        /* ── ACT1 特供: 老方案管线 (IVL 20ms/曲线 100 线性) + 弱机搭配,
         * 按 XBOX360 65535 量纲分配 (docs/震动系统开发规范_v20):
         *   命中 0x01=65 强主体; 技能/暴击 0x10=55 分层 (略低于命中, 且特殊
         *   攻击自带 200ms 静默窗防叠加爆震); 受击 0x02=40; 出血 0x04=12 极低;
         *   评分族压低; 怪物死亡中等。全局限幅: master 90 × 上限 75 (弱机保护)。
         *   密度自适应启用 (thr 45/降 40) + 连击 cap 150/slope 60 =
         *   后期连击暴增自动压制 (借鉴职业算法的密度/倍率封顶机制);
         *   槽 1/2/3/7/8 为引擎无引用死槽, 恒 0; p[39]=移动积累窗口。
         *   ★v15.2 马达 L/R 调校: 命中偏右清脆 / 受击偏左低吼 / 特殊偏右,
         *   出血走独立"怪物异常反馈"通道 (此表中性);
         *   评分族: 移动偏左 (走路低频感)。仅 S1 路线在预设列表中显示。
         *   ★v16.4 参数收敛 (狂战士实机日志剂量归因: 命中/特殊/受击为三个最响
         *   通道, 用户自调配置亦偏收敛): 命中 70→65 / 特殊 60→55 / 受击 45→40,
         *   降幅 7-11%, 单挑手感基本不变, 高攻职业连打更耐听 ── */
        VibrationPreset {
            name: "ACT1 特供".to_string(),
            params: [100, 0, 0, 0, 75, 65, 35, 0, 0, 90, 0, 20, 20, 55, 25, 10, 65, 40, 50, 100, 100, 8, 50, 380, 35, 8, 40, 30, 4, 45, 500, 40, 800, 55, 300, 25, 60, 45, 60, 1200, 150, 90, 600, 30, 800, 1200, 3000, 150, 60, 40, 60, 35, 200, 30, 200, 150, 40, 40, 30, 50],
            item_lr: [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, lr(-5), lr(-5), 0, 0, 0, 0, 0, 15, lr(-10), 10, lr(15), lr(-10)],
            rank_lr: [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, lr(-15), 10, 0, 0, 0, 0, 0, 0],
            rank_type_gain: [20, 20, 30, 30, 30, 20, 20, 20, 20, 20, 20, 40, 20, 20, 45],
            rank_level_gain: 30,
            rank_duration: 250,
            out_smooth: 35,
            user_modified: false,
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
            user_modified: false,
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
            user_modified: false,
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
            user_modified: false,
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
            user_modified: false,
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
            user_modified: false,
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
            user_modified: false,
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
            user_modified: false,
        },
    ]
}

/// 「ACT1 特供」预设管理 (纯函数, GUI 三处预设列表共用):
/// S1 路线 (legacy_client=true) 时保证其位于「默认」之下一位:
/// 缺失 → 从内置表拷贝插入; 已存在但错位 (旧版 push 到末尾的持久化残留)
/// → 搬移到「默认」之下; **内置管理条目 (user_modified=false) 的值自愈回内置表**
/// (版本升级换参数 / 用户调乱后, 应用即回到内置值)。
/// 返回是否发生变动, 供调用方按需落盘自愈。
/// 非 S1 路线不增不删 (由列表过滤器隐藏)。
pub fn ensure_act1_preset_position(
    presets: &mut Vec<VibrationPreset>,
    legacy_client: bool,
) -> bool {
    if !legacy_client {
        return false;
    }
    let target = |ps: &[VibrationPreset]| {
        ps.iter()
            .position(|x| x.name == "默认")
            .map(|i| i + 1)
            .unwrap_or(0)
    };
    match presets.iter().position(|x| x.name == "ACT1 特供") {
        None => {
            if let Some(pr) = default_vibration_presets()
                .into_iter()
                .find(|p| p.name == "ACT1 特供")
            {
                /* 插在「默认」之后 (用户要求: 默认下面), 无默认则置顶 */
                presets.insert(target(presets), pr);
                true
            } else {
                false
            }
        }
        Some(idx) => {
            let mut changed = false;
            /* ★v18: 内置管理条目 (未被用户"保存为同名"覆盖) 自愈回内置值 ——
             * 版本升级换了参数、或用户调乱后, 点应用即回到内置"保存的时候";
             * 用户主动覆盖过的条目 (user_modified=true) 保持不动 */
            if !presets[idx].user_modified {
                if let Some(b) = default_vibration_presets()
                    .into_iter()
                    .find(|p| p.name == "ACT1 特供")
                {
                    if presets[idx].params != b.params
                        || presets[idx].item_lr != b.item_lr
                        || presets[idx].rank_lr != b.rank_lr
                        || presets[idx].rank_type_gain != b.rank_type_gain
                        || presets[idx].rank_level_gain != b.rank_level_gain
                        || presets[idx].rank_duration != b.rank_duration
                        || presets[idx].out_smooth != b.out_smooth
                    {
                        presets[idx].params = b.params;
                        presets[idx].item_lr = b.item_lr;
                        presets[idx].rank_lr = b.rank_lr;
                        presets[idx].rank_type_gain = b.rank_type_gain;
                        presets[idx].rank_level_gain = b.rank_level_gain;
                        presets[idx].rank_duration = b.rank_duration;
                        presets[idx].out_smooth = b.out_smooth;
                        changed = true;
                    }
                }
            }
            let want = target(presets);
            if idx == want {
                return changed;
            }
            /* 先删后重算目标位 (remove 会使「默认」在其后的情形索引偏移) */
            let pr = presets.remove(idx);
            presets.insert(target(presets), pr);
            true
        }
    }
}

/// ★v19: ACT 专属预设命名判定 (仅 S1 路线显示): 「ACT1 特供」与所有「*-ACT」变体。
pub fn is_act_variant_name(name: &str) -> bool {
    name == "ACT1 特供" || name.ends_with("-ACT")
}

/// ★v19: 该名字是否有对应的内置 ACT 变体 (即 S1 下应被 "-ACT" 版替代的内置原版)。
/// 「ACT1 特供」「测试版(全0)」无变体, 不在此列。
pub fn is_act_base_name(name: &str) -> bool {
    name != "ACT1 特供"
        && name != "测试版(全0)"
        && default_vibration_presets().iter().any(|p| p.name == name)
}

/// ★v19: 预设下拉的可见条目 (真实下标, 名称)。
/// S1 路线: 隐藏有 ACT 变体的内置原版 (改用 "-ACT" 版), 保留 ACT1 特供/测试版/
/// 自建预设; S4 路线: 隐藏 ACT 专属 (ACT1 特供 + "*-ACT")。
/// 用真实下标回索引, 修"过滤后下标错位导致选错预设"的 BUG。
pub fn visible_preset_entries(
    presets: &[VibrationPreset],
    legacy: bool,
) -> Vec<(usize, String)> {
    presets
        .iter()
        .enumerate()
        .filter(|(_, p)| {
            if legacy {
                !is_act_base_name(&p.name)
            } else {
                !is_act_variant_name(&p.name)
            }
        })
        .map(|(i, p)| (i, p.name.clone()))
        .collect()
}

/// ★v19 ACT 力度档 (五档): 由职业/预设的重量定位决定上限与事件阶梯。
#[derive(Clone, Copy, PartialEq)]
pub enum ActTier {
    Light,
    Medium,
    Heavy,
    Extreme,
    Max,
}

/// ★v19 ACT 主反馈: 该职业/预设的主体事件 (其余事件落在其下)。
#[derive(Clone, Copy, PartialEq)]
pub enum ActSignature {
    Hit,
    Special,
    Taken,
    Dot,
}

/// 职业原上限 (v21 五档 30/33/37/41/46) → ACT 力度档
pub fn act_tier_from_max(orig_max: u32) -> ActTier {
    match orig_max {
        0..=30 => ActTier::Light,
        31..=33 => ActTier::Medium,
        34..=37 => ActTier::Heavy,
        38..=41 => ActTier::Extreme,
        _ => ActTier::Max,
    }
}

/// (上限, 命中, 特殊, 受击, DOT, 状态, 特效) —— ACT 事件力度阶梯。
/// ★v19.5: 以 ACT1 特供为锚 (Extreme 档 = ACT1 特供: 75/65/55/40/20/25/10)。
pub fn act_ladder(t: ActTier) -> (u32, u32, u32, u32, u32, u32, u32) {
    match t {
        ActTier::Light => (58, 50, 42, 30, 15, 20, 8),
        ActTier::Medium => (63, 56, 47, 34, 17, 22, 9),
        ActTier::Heavy => (68, 61, 51, 37, 19, 24, 10),
        ActTier::Extreme => (75, 65, 55, 40, 20, 25, 10),
        ActTier::Max => (75, 68, 58, 43, 22, 27, 11),
    }
}

/// 四类衰减 (命中/特殊/受击/状态) —— 轻快档更脆, 重击档更沉
/// ★v19.5: Extreme = ACT1 特供 (25/60/45/60)
pub fn act_decay(t: ActTier) -> (u32, u32, u32, u32) {
    match t {
        ActTier::Light => (20, 45, 35, 45),
        ActTier::Medium => (22, 50, 40, 50),
        ActTier::Heavy => (25, 55, 45, 55),
        ActTier::Extreme => (25, 60, 45, 60),
        ActTier::Max => (28, 65, 50, 65),
    }
}

/// ★v19.5: ACT1 特供的 60 槽参数 (职业 ACT 变体的基底 —— 用户定稿: "基于 ACT1
/// 特供预设, 再根据职业理念修改")。
pub fn act1_base_params() -> [u32; 60] {
    default_vibration_presets()
        .into_iter()
        .find(|p| p.name == "ACT1 特供")
        .map(|p| p.params)
        .unwrap_or([0; 60])
}

/// ★v19.4 ACT 高级算法组重建 (职业与通用预设共用): 按源参数的"职业原型标记"
/// 重写密度自适应/连击倍率/命中自适应/移动/窗口/脉冲 —— 原型 = 高连击 / 重击 /
/// 通用 (由源阈值推断) × 走位 / 站桩 / 中性。曲线 (19/20) 保留源值 (饱和个性)。
pub fn act_rebuild_advanced(p: &mut [u32; 60], src: &[u32; 60]) {
    /* 原型标记一律取自 src (职业/预设的原始画像), 值写入 p */
    let combo_heavy = src[29] <= 35 || src[47] <= 145;
    let heavy = src[29] >= 60 || src[47] >= 160;
    let mobile = src[21] >= 10;
    let static_ = src[21] <= 3;
    let src_reduce = src[31];
    let src_dot = src[40];
    let src_period = src[41];
    let src_burst = src[42];
    let src_burst_min = src[43];
    let src_counter = src[44];
    let src_silence = src[54];
    let src_counter_mul = src[55];
    let src_wake = src[56];
    let src_milestone = src[57];
    let src_intr_pulse = src[58];

    /* 移动质感统一 (v22.1 全职业一致) + 走位能量积累按原型 */
    p[23] = 380;
    p[24] = 40;
    p[25] = 8;
    p[26] = 45;
    p[27] = 30;
    p[28] = 4;
    if mobile {
        p[21] = 12;
        p[22] = 40;
        p[39] = 1800;
    } else if static_ {
        p[21] = 2;
        p[22] = 10;
        p[39] = 1000;
    } else {
        p[21] = 6;
        p[22] = 25;
        p[39] = 1500;
    }

    /* 连击密度自适应 (v21 二·五·1 职业配置) */
    if combo_heavy {
        p[29] = 35;
        p[30] = 500;
        p[31] = 40;
        p[32] = 800;
        p[33] = 60;
        p[34] = 300;
    } else if heavy {
        p[29] = 65;
        p[30] = 500;
        p[31] = 20;
        p[32] = 1000;
        p[33] = 75;
        p[34] = 300;
    } else {
        p[29] = 45;
        p[30] = 500;
        p[31] = 32;
        p[32] = 1000;
        p[33] = 55;
        p[34] = 250;
    }

    /* 连击倍率/窗口/中断门槛 */
    if combo_heavy {
        p[45] = 2200;
        p[47] = 140;
        p[48] = 33;
        p[49] = 30;
    } else if heavy {
        p[45] = 1100;
        p[47] = 170;
        p[48] = 43;
        p[49] = 50;
    } else {
        p[45] = 1500;
        p[47] = 155;
        p[48] = 38;
        p[49] = 40;
    }

    /* 命中自适应 (连击密 → 大幅降强度防震手; 重击型弱自适应) */
    if src_reduce >= 40 {
        p[50] = 55;
        p[51] = 35;
        p[52] = 200;
        p[53] = 40;
    } else {
        p[50] = 65;
        p[51] = 20;
        p[52] = 250;
        p[53] = 50;
    }

    /* 窗口 (DOT/特效/爆发/反击/空闲; 连击窗已按原型) */
    p[40] = if src_dot >= 200 { 200 } else { 150 };
    p[41] = if src_period <= 80 { 80 } else { 90 };
    p[42] = if src_burst <= 300 { 250 } else { 600 };
    p[43] = if src_burst_min >= 40 { 40 } else { 30 };
    p[44] = if src_counter >= 800 { 1000 } else { 800 };
    p[46] = 3000;

    /* 静默/反击/脉冲 */
    p[54] = if src_silence >= 200 { 250 } else { 200 };
    p[55] = if src_counter_mul >= 150 { 150 } else { 130 };
    p[56] = if src_wake >= 70 { 80 } else { 40 };
    p[57] = if src_milestone >= 70 { 80 } else { 40 };
    p[58] = if src_intr_pulse >= 40 { 40 } else { 30 };
}

/// ★v19.4 ACT 马达语言 (item_lr[26]): 命中偏右清脆 / 受击偏左低吼 / 特殊偏右 /
/// DOT 微轻; 幅度随档位递增 (重档更明显)。
pub fn act_motor_item_lr(tier: ActTier) -> [u32; 26] {
    let i = match tier {
        ActTier::Light => 0,
        ActTier::Medium => 1,
        ActTier::Heavy => 2,
        ActTier::Extreme => 3,
        ActTier::Max => 4,
    };
    let hit_r = [8u32, 10, 10, 12, 12][i];
    let taken_l = [8u32, 12, 15, 18, 20][i];
    let taken_r = [-6i32, -8, -10, -12, -14][i];
    let special_r = [8u32, 10, 15, 15, 18][i];
    let mut w = [0u32; 26];
    w[7 * 2] = lr(-5); /* DOT 微轻 */
    w[7 * 2 + 1] = lr(-5);
    w[10 * 2 + 1] = special_r; /* 特殊偏右 */
    w[11 * 2] = lr(-10); /* 命中偏右清脆 */
    w[11 * 2 + 1] = hit_r;
    w[12 * 2] = lr(taken_l as i32); /* 受击偏左低吼 */
    w[12 * 2 + 1] = lr(taken_r);
    w
}

/// ★v19.4 ACT 评分马达语言 (rank_lr[30]): 中性, 仅移动偏左低频感。
pub fn act_rank_lr() -> [i32; 30] {
    let mut r = [0i32; 30];
    r[22] = -15;
    r[23] = 10;
    r
}

/// ★v19 ACT 参数构造 (职业与通用预设共用): 按档位重建事件阶梯/衰减/三闸,
/// 主反馈显式指定, 飘字间隔 ≤20ms; 高级算法组按原型重写; 其余槽位保留源参数。
/// `max_override` 用于进一步压低个别预设的响度 (如极简轻巧)。
pub fn act_build_params(
    base: &[u32; 60],
    tier: ActTier,
    sig: ActSignature,
    max_override: Option<u32>,
) -> [u32; 60] {
    act_build_params_from(base, base, tier, sig, max_override)
}

/// ★v19.5 ACT 参数构造 (基底与画像分离): `base` = 参数基底 (职业用 ACT1 特供,
/// 通用预设用其自身), `src` = 原型画像来源 (档位/密度/连击/走位标记)。这样
/// "ACT1 特供概念最优先" 的槽位 (连击增强/曲线/节奏等) 全部取自基底, 而职业
/// 个性只影响原型相关组。
pub fn act_build_params_from(
    base: &[u32; 60],
    src: &[u32; 60],
    tier: ActTier,
    sig: ActSignature,
    max_override: Option<u32>,
) -> [u32; 60] {
    let (mut max, hit, special, taken, dot, state, effect) = act_ladder(tier);
    if let Some(m) = max_override {
        max = m;
    }
    let (dec_a, dec_s, dec_h, dec_st) = act_decay(tier);
    let (hit_v, sp_v, tk_v, dt_v) = match sig {
        ActSignature::Hit => (hit, special, taken, dot),
        ActSignature::Special => (hit.saturating_sub(4), special.max(hit), taken, dot),
        ActSignature::Taken => (hit.saturating_sub(4), special, taken.max(hit), dot),
        ActSignature::Dot => (hit.saturating_sub(4), special, taken, dot.max(hit)),
    };
    let mut p = *base;
    p[0] = 100;
    p[4] = max;
    p[9] = 90;
    p[11] = base[11].min(20);
    p[12] = dt_v;
    p[13] = sp_v;
    p[14] = state;
    p[15] = effect;
    p[16] = hit_v;
    p[17] = tk_v;
    p[35] = dec_a;
    p[36] = dec_s;
    p[37] = dec_h;
    p[38] = dec_st;
    /* ★v19.4/19.5 高级算法组重建: 标记取自 src, 值写入 p */
    act_rebuild_advanced(&mut p, src);
    p
}

/// ★v19 ACT 变体的评分族压低 (ACT1 特供口径: 细分强度 ×0.3, 等级 ≤30, 时长 ≤250)
pub fn act_variant_rank_gains(base: &[u32; 15]) -> [u32; 15] {
    let mut g = [0u32; 15];
    for i in 0..15 {
        g[i] = ((base[i] as f32) * 0.3).round().min(100.0) as u32;
    }
    g
}

/// ★v19 通用预设的 ACT 设计表 (按各自设计理念落到 ACT 框架内, 不求与 ACT1 相同):
/// 默认=中性基准 / 低频攻击=极重+特殊主体 / 高频攻击=轻盈 / 高振幅=最重 /
/// 节奏律动=轻脆+特殊主体 / 极简轻巧=最轻 (上限再压到 50) / 实战竞技=厚重+受击主体。
fn act_general_design(name: &str) -> (ActTier, ActSignature, Option<u32>) {
    use ActSignature::{Hit, Special, Taken};
    use ActTier::{Extreme, Heavy, Light, Max, Medium};
    match name {
        "默认" => (Medium, Hit, None),
        "低频攻击职业" => (Extreme, Special, None),
        "高频攻击职业" => (Light, Hit, None),
        "高振幅" => (Max, Hit, None),
        "节奏律动" => (Light, Special, None),
        "极简轻巧" => (Light, Hit, Some(50)),
        "实战竞技" => (Heavy, Taken, None),
        _ => (Medium, Hit, None),
    }
}

/// ★v19: 通用预设的 ACT 变体表 (仅 S1 路线显示): 每个内置通用预设生成 "X-ACT"
/// (ACT1 特供本身 / 测试版(全0) 除外)。事件强度/衰减/三闸按各预设的设计理念
/// 落到 ACT 档位; 预设个性 (曲线/移动/密度/连击/窗口/马达权重/平滑) 原样保留。
pub fn act_general_presets() -> Vec<VibrationPreset> {
    default_vibration_presets()
        .into_iter()
        .filter(|p| p.name != "ACT1 特供" && p.name != "测试版(全0)")
        .map(|p| {
            let (tier, sig, max_override) = act_general_design(&p.name);
            VibrationPreset {
                name: format!("{}-ACT", p.name),
                params: act_build_params(&p.params, tier, sig, max_override),
                /* ★v19.4 马达 L/R 也换成 ACT 语言 */
                item_lr: act_motor_item_lr(tier),
                rank_lr: act_rank_lr().map(|v| v as u32),
                rank_type_gain: act_variant_rank_gains(&p.rank_type_gain),
                rank_level_gain: p.rank_level_gain.min(30),
                rank_duration: p.rank_duration.min(250),
                out_smooth: p.out_smooth,
                user_modified: false,
            }
        })
        .collect()
}

/// ★v19: 保证通用预设的 ACT 变体在列表中 (仅 S1)。缺失 → 插在其基准预设之后
/// (基准缺失则追加); 内置管理条目 (user_modified=false) 的值自愈回 ACT 变体表。
/// 返回是否发生变动 (调用方按需落盘)。
pub fn ensure_act_general_variants(
    presets: &mut Vec<VibrationPreset>,
    legacy_client: bool,
) -> bool {
    if !legacy_client {
        return false;
    }
    let mut changed = false;
    for want in act_general_presets() {
        match presets.iter().position(|x| x.name == want.name) {
            None => {
                let base = want.name.trim_end_matches("-ACT").to_string();
                let pos = presets
                    .iter()
                    .position(|x| x.name == base)
                    .map(|i| i + 1)
                    .unwrap_or(presets.len());
                presets.insert(pos, want);
                changed = true;
            }
            Some(idx) => {
                if !presets[idx].user_modified {
                    let same = presets[idx].params == want.params
                        && presets[idx].rank_type_gain == want.rank_type_gain
                        && presets[idx].rank_level_gain == want.rank_level_gain
                        && presets[idx].rank_duration == want.rank_duration
                        && presets[idx].item_lr == want.item_lr
                        && presets[idx].rank_lr == want.rank_lr
                        && presets[idx].out_smooth == want.out_smooth;
                    if !same {
                        presets[idx] = want;
                        changed = true;
                    }
                }
            }
        }
    }
    changed
}

/// ★v18 应用预设时的取值规则 (纯函数, 震动页「应用」与快捷卡/极简两条路径共用):
/// - 用户主动覆盖过的同名条目 (`user_modified=true`) 优先 —— 用户版本说了算;
/// - 否则内置同名预设优先 (内置 = "默认值": 版本升级换过参数、或用户调乱后,
///   点应用即回到内置"保存的时候", 不会被列表里的旧快照顶住);
/// - 用户列表有而内置没有 (自建预设) → 用用户条目;
/// - 两边都没有 → None (调用方忽略)。
pub fn pick_preset_for_apply(
    user_entry: Option<&VibrationPreset>,
    name: &str,
) -> Option<VibrationPreset> {
    let builtin = default_vibration_presets()
        .into_iter()
        .find(|p| p.name == name);
    match (user_entry, builtin) {
        (Some(u), _) if u.user_modified => Some(u.clone()),
        (_, Some(b)) => Some(b),
        (Some(u), None) => Some(u.clone()),
        (None, None) => None,
    }
}

/// Main application configuration structure.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct AppConfig {    /// Display tray icon
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
    /// ★v20.4: 经典模式 (老宿主 820×600 三页签窗口) 窗口矩形记忆 [x, y, w, h]
    #[serde(default)]
    pub window_rect_classic: Option<[f32; 4]>,
    /// ★v20.4: 启动时进入经典模式 (用户定稿: 经典界面默认开启;
    /// 与极简模式互斥, 后切换的模式生效)
    #[serde(default = "default_classic_mode")]
    pub classic_mode: bool,
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
    /// ★v20.3: 切换到此预设的组合键 (如 "F6" / "CTRL+F6"; 空 = 不绑定)。
    /// 全局热键 (GetAsyncKeyState, 游戏内也可切); 保存时校验: 不得与其他预设
    /// 或「连发切换键」重复 (防冲突)。
    #[serde(default)]
    pub switch_key: String,
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

/// ★v20.4: 经典模式默认开启 (用户定稿"经典界面为默认开启"; serde default 使旧配置也生效)
fn default_classic_mode() -> bool {
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
            window_rect_classic: None,
            minimal_mode: false,
            classic_mode: default_classic_mode(),
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
                switch_key: String::new(),
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

    /* ── ensure_act1_preset_position 回归 (修"ACT1 特供在列表末尾") ──
     * 旧版 ensure 是 push 到末尾并已持久化进 Vibration.toml; 位置修正
     * 提交只改了插入点, 对"已存在但错位"的残留条目永不搬动。 */

    /// 构造最小预设条目
    fn preset_named(name: &str) -> VibrationPreset {
        VibrationPreset {
            name: name.to_string(),
            params: [0; 60],
            item_lr: [0; 26],
            rank_lr: [0; 30],
            rank_type_gain: [0; 15],
            rank_level_gain: 0,
            rank_duration: 0,
            out_smooth: 0,
            user_modified: false,
        }
    }

    fn act1_builtin() -> VibrationPreset {
        default_vibration_presets()
            .into_iter()
            .find(|p| p.name == "ACT1 特供")
            .expect("内置预设表必须含 ACT1 特供")
    }

    #[test]
    fn act1_preset_missing_inserted_right_below_default() {
        let mut presets = vec![
            preset_named("默认"),
            preset_named("用户A"),
            preset_named("用户B"),
        ];
        assert!(ensure_act1_preset_position(&mut presets, true));
        assert_eq!(presets[1].name, "ACT1 特供", "应插在「默认」之下一位");
        assert_eq!(presets.len(), 4);
    }

    #[test]
    fn act1_preset_stale_end_position_is_moved_below_default() {
        /* 用户实际残留: 旧版 push 到末尾且已落盘 */
        let mut presets = vec![
            preset_named("默认"),
            preset_named("用户A"),
            preset_named("用户B"),
            act1_builtin(),
        ];
        assert!(
            ensure_act1_preset_position(&mut presets, true),
            "错位条目必须被搬移 (本测试即 bug 回归)"
        );
        assert_eq!(presets[1].name, "ACT1 特供", "应搬移到「默认」之下一位");
        assert_eq!(presets[0].name, "默认");
        assert_eq!(presets.len(), 4, "搬移不得增删条目");
    }

    #[test]
    fn act1_preset_already_correct_is_untouched() {
        let mut presets = vec![preset_named("默认"), act1_builtin(), preset_named("用户A")];
        assert!(!ensure_act1_preset_position(&mut presets, true));
        assert_eq!(presets[1].name, "ACT1 特供");
    }

    #[test]
    fn act1_preset_not_managed_when_not_legacy() {
        let mut presets = vec![preset_named("默认"), act1_builtin()];
        assert!(!ensure_act1_preset_position(&mut presets, false));
        assert_eq!(presets.len(), 2, "非 S1 路线由过滤器隐藏, 不增不删");

        let mut missing = vec![preset_named("默认")];
        assert!(!ensure_act1_preset_position(&mut missing, false));
        assert_eq!(missing.len(), 1, "非 S1 路线不插入");
    }

    #[test]
    fn act1_preset_inserted_top_when_no_default() {
        let mut presets = vec![preset_named("用户A")];
        assert!(ensure_act1_preset_position(&mut presets, true));
        assert_eq!(presets[0].name, "ACT1 特供", "无「默认」时置顶");
    }

    #[test]
    fn act1_stale_builtin_entry_values_are_healed() {
        /* 位置正确但值陈旧 (旧版持久化的快照 / 用户调乱): 自愈回内置值 */
        let mut stale = preset_named("ACT1 特供");
        stale.params[16] = 70;
        let mut presets = vec![preset_named("默认"), stale, preset_named("用户A")];
        assert!(
            ensure_act1_preset_position(&mut presets, true),
            "陈旧值应触发自愈"
        );
        assert_eq!(
            presets[1].params,
            act1_builtin().params,
            "值应回到内置 (点应用即回归)"
        );
    }

    #[test]
    fn act1_user_modified_entry_is_not_healed() {
        let mut mine = act1_builtin();
        mine.user_modified = true;
        mine.params[16] = 42;
        let mut presets = vec![preset_named("默认"), mine, preset_named("用户A")];
        assert!(!ensure_act1_preset_position(&mut presets, true));
        assert_eq!(presets[1].params[16], 42, "用户覆盖过的条目不得被自愈");
    }

    #[test]
    fn pick_preset_prefers_builtin_unless_user_modified() {
        let mut stale = act1_builtin();
        stale.user_modified = false;
        stale.params[16] = 1;
        let picked = pick_preset_for_apply(Some(&stale), "ACT1 特供").unwrap();
        assert_eq!(
            picked.params,
            act1_builtin().params,
            "未主动覆盖 → 用内置值 (回归默认)"
        );

        let mut mine = act1_builtin();
        mine.user_modified = true;
        mine.params[16] = 42;
        let picked = pick_preset_for_apply(Some(&mine), "ACT1 特供").unwrap();
        assert_eq!(picked.params[16], 42, "已主动覆盖 → 用户版本优先");

        let custom = preset_named("我的预设");
        let picked = pick_preset_for_apply(Some(&custom), "我的预设").unwrap();
        assert_eq!(picked.name, "我的预设", "自建预设照常使用");
        assert!(pick_preset_for_apply(None, "不存在的预设").is_none());
    }
}
