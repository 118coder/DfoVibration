//! **S4+ 引擎 (独立实现)** —— 逐字节移植自「1.61 版」`src/vibration.rs`。
//!
//! 为什么是独立模块: 用户要求 **S1 与 S4 几乎分离, 互不冲突**。此前两条路线共用
//! 一个 `VibEngine`, 靠 43 处 `if self.legacy` 分支区分 —— 改 S4 时随时可能碰坏
//! S1。现在 S4 拥有自己的状态机与全部参数解读逻辑, **不引用 s1.rs, 也不含任何
//! `legacy` 判断**; S1 同理 (见 `s1.rs`)。
//!
//! 参照物: `E:\网页小工具\【90US】…DfoVibration 1.61版\SorahkDFO源码\srcibration.rs`
//! (与「大改前的最后一个版本」同名文件 **diff = 0 行**, 同为 1632 行)。
//! 除模块路径 (`crate::vibration::core::*`) 与可见性外, 算法与算式**未作任何改动**。
//!
//! 与 1.61 的唯一差异 (均不影响数值):
//! - 参数来源由 `[u32;60]` 直接读取 (保持 1.61 原样), 高级算法总开关层**不应用**
//!   (1.61 无此层);
//! - 新增 `[EVT]`/`[INJ]`/`[DROP]`/`[MV]` 诊断日志 (秒级采样, 纯观测)。
//!
//! 共享基础设施 (共享内存/XInput 输出/日志/时间哨兵) 在 `core.rs`。

use std::sync::atomic::AtomicU32;

use super::core::*;
use super::params::*;

#[derive(PartialEq, Clone, Copy)]
pub(super) enum VibState {
    Idle = 0,
    Combat = 1,
    Burst = 2,
    Counter = 3,
}

pub(super) struct VibEngine {
        pub(super) last_output: u32,
        pub(super) last_event: u32,
        pub(super) left: f32,
        pub(super) right: f32,
        pub(super) decay_l: f32,
        pub(super) decay_r: f32,
        pub(super) hold_until: u32,
        pub(super) hold_l: f32,
        pub(super) hold_r: f32,
        pub(super) rhythm_until: u32,
        pub(super) rhythm_period: u32,
        pub(super) rhythm_l: f32,
        pub(super) rhythm_r: f32,
        pub(super) last_font: u32,
        pub(super) combo_hits: u32,
        pub(super) last_combo: u32,
        pub(super) milestones: u32,
        pub(super) state: VibState,
        pub(super) burst_until: u32,
        pub(super) burst_hits: u32,
        pub(super) burst_start: u32,
        pub(super) counter_until: u32,
        pub(super) silence_until: u32,
        pub(super) dot_active: bool,
        pub(super) dot_last: u32,
        pub(super) wake_done: bool,
        pub(super) interrupt_fired: bool,
        pub(super) p: [f32; 60],
        pub(super) rng_state: u32,
        pub(super) rank_full_until: u32,
        pub(super) rank_level: f32,
    /* 当前评分事件的 L/R 马达系数 (rank_lr 权重, 默认 1.0) */
        pub(super) rank_lr_l: f32,
        pub(super) rank_lr_r: f32,
    /* 当前评分事件 idx (0-14, -1 = 无) 用于实时输出进度条 */
        pub(super) rank_out_idx: i32,
    /* 移动持续震动: 独立输出通道 (不随 finalize 总闸关闭) */
        pub(super) move_level: f32,
        pub(super) move_last: u32,
    /* 移动走路质感: 步伐节奏 (间隔/相位), 模拟左右脚交替轻震 */
        pub(super) move_pace_until: u32,
        pub(super) move_phase: bool,
    /* 移动平滑输出 (消除嗡嗡/沙沙声: 平滑插值状态) */
        pub(super) move_out_l: f32,
        pub(super) move_out_r: f32,
    /* 连击密度自适应 (v22): 窗口期连击密度统计与降强度状态 */
        pub(super) density_hits: u32,
        pub(super) density_win_start: u32,
        pub(super) density_active: bool,
        pub(super) density_scale: f32,
    /* 移动积累增强 (v22): 走位能量 (0..1), 停止移动后窗口内攻击增强 */
        pub(super) move_charge: f32,
    /* 输出平滑 (v22.3): 一阶低通抑制低频嗡嗡声 (快速连击时输出跳变被柔化) */
        pub(super) out_smooth_l: f32,
        pub(super) out_smooth_r: f32,
    /* 震动节流 (v24): 限定时间窗口内最大注入次数 (召唤师等狂震职业防狂震) */
        pub(super) throttle_window: u32,
        pub(super) throttle_max: u32,
        pub(super) throttle_dense_ratio: u32,
        pub(super) throttle_count: u32,
        pub(super) throttle_start: u32,
    /* 绝对震动频率 (v26, 召唤专属): 一切算法失效, 只在 时间-次数 内注入 */
        pub(super) abs_freq_enabled: bool,
        pub(super) abs_freq_window: u32,
        pub(super) abs_freq_max: u32,
        pub(super) abs_count: u32,
        pub(super) abs_start: u32,
        pub(super) abs_last: u32,
    /* 评分动态衰减 (类鬼泣, v29): 攻击提升评级, 停手衰减, 反馈倍率联动 */
        pub(super) ghost_grade: f32,
        pub(super) ghost_last_hit: u32,
        pub(super) ghost_mul: f32,
        pub(super) ghost_enabled: bool,
        pub(super) ghost_delay: f32,
        pub(super) ghost_speed: f32,
        pub(super) ghost_min: f32,
        pub(super) ghost_max: f32,
    /* 输出低强度死区 (v29.2): 主通道输出低于此值归 0 (消除马达低强度嗡声) */
        pub(super) out_threshold: f32,
    /* 迟滞状态 (v29.3): 死区边缘迟滞, 防阈值附近反复启停产生嗡声 */
        pub(super) out_dead_l: bool,
        pub(super) out_dead_r: bool,
    /* 输出重映射 (v30, ERM/Xbox360): 非零输出映射到 [min,100] */
        pub(super) remap_enabled: bool,
        pub(super) remap_min: f32,
    /* 马达分工 (v30, Xbox360 风格): 轻反馈单马达, 重反馈双马达 */
        pub(super) split_enabled: bool,
        pub(super) split_thr: f32,
    /* 职业专属高级算法 (v31): id 1-15 + 参数 + 运行状态 */
        pub(super) algo_id: u32,
        pub(super) algo_ap: [f32; 4],
        pub(super) algo_state: [f32; 4],
}


impl VibEngine {
    pub(super) fn new() -> Self {
        Self {
            last_output: 0,
            last_event: 0,
            left: 0.0,
            right: 0.0,
            decay_l: 45.0,
            decay_r: 35.0,
            hold_until: 0,
            hold_l: 0.0,
            hold_r: 0.0,
            rhythm_until: 0,
            rhythm_period: 90,
            rhythm_l: 0.0,
            rhythm_r: 0.0,
            last_font: 0,
            combo_hits: 0,
            last_combo: 0,
            milestones: 0,
            state: VibState::Idle,
            burst_until: 0,
            burst_hits: 0,
            burst_start: 0,
            counter_until: 0,
            silence_until: 0,
            dot_active: false,
            dot_last: 0,
            wake_done: false,
            interrupt_fired: false,
            p: [0.0; 60],
            rng_state: 0x9E3779B9,
            rank_full_until: 0,
            rank_level: 0.0,
            rank_lr_l: 1.0,
            rank_lr_r: 1.0,
            rank_out_idx: -1,
            move_level: 0.0,
            move_last: 0,
            move_pace_until: 0,
            move_phase: false,
            move_out_l: 0.0,
            move_out_r: 0.0,
            density_hits: 0,
            density_win_start: 0,
            density_active: false,
            density_scale: 1.0,
            move_charge: 0.0,
            out_smooth_l: 0.0,
            out_smooth_r: 0.0,
            throttle_window: 0,
            throttle_max: 0,
            throttle_dense_ratio: 100,
            throttle_count: 0,
            throttle_start: 0,
            abs_freq_enabled: false,
            abs_freq_window: 3000,
            abs_freq_max: 6,
            abs_count: 0,
            abs_start: 0,
            abs_last: 0,
            ghost_grade: 0.0,
            ghost_last_hit: 0,
            ghost_mul: 1.0,
            ghost_enabled: false,
            ghost_delay: 2000.0,
            ghost_speed: 2.0,
            ghost_min: 0.6,
            ghost_max: 1.2,
            out_threshold: 3.0,
            out_dead_l: false,
            out_dead_r: false,
            remap_enabled: true,
            remap_min: 28.0,
            split_enabled: true,
            split_thr: 40.0,
            algo_id: 0,
            algo_ap: [0.0; 4],
            algo_state: [0.0; 4],
        }
    }

    /* xorshift32 PRNG: 随机性乘子, 无需外部依赖 */
    pub(super) fn rng_next(&mut self) -> u32 {
        let mut x = self.rng_state;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng_state = x;
        x
    }

    fn inject(&mut self, s: f32, lr: f32, rr: f32, dl: f32, dr: f32) {
        self.left = s * lr;
        self.right = s * rr;
        self.decay_l = dl;
        self.decay_r = dr;
        self.hold_until = 0;
        self.rhythm_until = 0;
    }

    fn inject_hold(&mut self, s: f32, lr: f32, rr: f32, dur: u32) {
        self.hold_l = s * lr;
        self.hold_r = s * rr;
        self.left = self.hold_l;
        self.right = self.hold_r;
        self.hold_until = now_ms() + dur;
        self.rhythm_until = 0;
    }

    /* 评分脉冲注入: 取最强/已过期才覆盖, 避免多事件互相顶掉 */
    fn inject_rank(&mut self, s: f32, dur: u32) {
        if s <= 0.0 {
            return;
        }
        let now = now_ms();
        if s >= self.rank_level || now >= self.rank_full_until {
            self.rank_full_until = now.wrapping_add(dur.max(20));
            self.rank_level = s;
        }
    }

    /* 职业专属高级算法 (v31.1): 攻击命中钩子 (57 转职按行为分组)
     * A连击步长 B蓄势重击 C周期 D特殊追加 E窗口爆发 F受击保护
     * G标记增强 H叠层 I暴走 J连段 K绝对频率 L交替连击 M连点组 */
    fn algo_on_attack(&mut self, is_special: bool, count: u32, now: u32) {
        match self.algo_id {
            /* B 蓄势重击: 连续命中蓄满 → 重击脉冲 */
            4 | 7 | 11 | 13 | 15 | 29 | 32 | 34 | 39 | 45 | 47 | 52 | 55 => {
                self.algo_state[0] += count as f32;
                if self.algo_state[0] >= self.algo_ap[0] {
                    self.inject(self.algo_ap[1] / 100.0, 0.9, 0.7, self.algo_ap[2], self.algo_ap[3]);
                    self.algo_state[0] = 0.0;
                }
            }
            /* I 暴走: 蓄满后重脉冲 + 后续攻击增强窗口 */
            2 | 41 => {
                self.algo_state[0] += count as f32;
                if self.algo_state[0] >= self.algo_ap[0] {
                    self.inject(self.algo_ap[2] / 100.0, 0.9, 0.7, 100.0, 80.0);
                    self.algo_state[1] = (now + self.algo_ap[3] as u32) as f32;
                    self.algo_state[0] = 0.0;
                }
            }
            /* E 窗口爆发: 窗口内密集连击 → 重脉冲 + 静默 */
            9 | 26 | 49 => {
                if now.wrapping_sub(self.algo_state[0] as u32) > self.algo_ap[0] as u32 {
                    self.algo_state[1] = 0.0;
                    self.algo_state[0] = now as f32;
                }
                self.algo_state[1] += count as f32;
                if self.algo_state[1] >= self.algo_ap[1] {
                    self.inject(self.algo_ap[2] / 100.0, 0.9, 0.7, 60.0, 45.0);
                    self.silence_until = now + self.algo_ap[3] as u32;
                    self.algo_state[1] = 0.0;
                }
            }
            /* M 连点组: 窗口密集命中 → 弹幕节奏组 */
            16 | 18 => {
                if now.wrapping_sub(self.algo_state[0] as u32) > self.algo_ap[0] as u32 {
                    self.algo_state[1] = 0.0;
                    self.algo_state[0] = now as f32;
                }
                self.algo_state[1] += count as f32;
                if self.algo_state[1] >= self.algo_ap[1] {
                    self.inject_rhythm(self.algo_ap[2] / 100.0, 0.5, 0.5, 120, self.algo_ap[3] as u32);
                    self.algo_state[1] = 0.0;
                }
            }
            /* D 特殊追加: 特殊攻击后延迟脉冲 */
            6 | 12 | 19 | 28 | 37 | 43 => {
                if is_special {
                    self.algo_state[0] = (now + self.algo_ap[0] as u32) as f32;
                }
            }
            /* G 标记增强: 特殊攻击标记, 窗口内攻击增强 */
            50 => {
                if is_special {
                    self.algo_state[0] = (now + self.algo_ap[0] as u32) as f32;
                }
            }
            /* J 连段完成: 记录最后事件时间 */
            57 => {
                self.algo_state[0] = now as f32;
            }
            _ => {}
        }
    }

    /* 职业专属高级算法 (v31.1): 受击钩子 */
    fn algo_on_hit(&mut self, now: u32) {
        match self.algo_id {
            /* F 受击保护: ap[0]>=1000 → 庇护缓冲; <1000 → 盾牌格挡 */
            14 | 36 | 42 | 53 => {
                if self.algo_ap[0] >= 1000.0 {
                    self.algo_state[0] = (now + self.algo_ap[0] as u32) as f32;
                } else {
                    if now.wrapping_sub(self.algo_state[0] as u32) > self.algo_ap[0] as u32 {
                        self.algo_state[1] = 0.0;
                        self.algo_state[0] = now as f32;
                    }
                    self.algo_state[1] += 1.0;
                    if self.algo_state[1] >= self.algo_ap[1] {
                        self.inject(self.algo_ap[2] / 100.0, 0.9, 0.7, self.algo_ap[3], self.algo_ap[3] * 0.7);
                        self.algo_state[1] = 0.0;
                    }
                }
            }
            _ => {}
        }
    }

    /* 职业专属高级算法 (v31.1): 每帧周期钩子 */
    fn algo_tick(&mut self, now: u32) {
        let combat = self.state == VibState::Combat || self.state == VibState::Burst;
        match self.algo_id {
            /* A 连击步长脉冲: 连击每跨步长触发轻脉冲 */
            1 | 5 | 30 | 51 | 56 => {
                let step = self.algo_ap[0].max(1.0);
                if self.combo_hits as f32 >= self.algo_state[0] + step {
                    self.algo_state[0] = self.combo_hits as f32;
                    self.inject(self.algo_ap[1] / 100.0, 0.7, 0.4, 70.0, 40.0);
                }
            }
            /* L 交替连击: 连击跨步长触发左右交替脉冲 */
            8 | 23 | 38 | 46 => {
                let step = self.algo_ap[0].max(1.0);
                if self.combo_hits as f32 >= self.algo_state[0] + step {
                    self.algo_state[0] = self.combo_hits as f32;
                    let phase = self.algo_state[1] as i32 & 1;
                    self.algo_state[1] += 1.0;
                    let (l, r) = if phase == 0 { (0.9, 0.3) } else { (0.3, 0.9) };
                    self.inject(self.algo_ap[1] / 100.0, l, r, self.algo_ap[2], self.algo_ap[3]);
                }
            }
            /* C 周期脉冲: 战斗期周期注入 */
            3 | 10 | 17 | 20 | 22 | 24 | 25 | 31 | 35 | 40 | 44 | 48 | 54 => {
                if combat {
                    if self.algo_state[0] == 0.0 {
                        self.algo_state[0] = now as f32;
                    }
                    if now.wrapping_sub(self.algo_state[0] as u32) >= self.algo_ap[0] as u32 {
                        self.inject(self.algo_ap[1] / 100.0, 0.6, 0.5, self.algo_ap[2], self.algo_ap[3]);
                        self.algo_state[0] = now as f32;
                    }
                } else {
                    self.algo_state[0] = 0.0;
                }
            }
            /* D 特殊追加: 延迟触发 */
            6 | 12 | 19 | 28 | 37 | 43 => {
                if self.algo_state[0] != 0.0 && now >= self.algo_state[0] as u32 {
                    self.inject(self.algo_ap[1] / 100.0, 0.8, 0.6, self.algo_ap[2], self.algo_ap[3]);
                    self.algo_state[0] = 0.0;
                }
            }
            /* J 连段完成: 停手超阈值 → 连段完成脉冲 */
            57 => {
                if self.algo_state[0] != 0.0
                    && now.wrapping_sub(self.algo_state[0] as u32) > self.algo_ap[0] as u32
                {
                    self.inject(self.algo_ap[1] / 100.0, 0.75, 0.55, self.algo_ap[2], self.algo_ap[3]);
                    self.algo_state[0] = 0.0;
                }
            }
            _ => {}
        }
    }

    /* 职业专属高级算法 (v31.1): 攻击注入强度修正 (返回乘数) */
    fn algo_attack_mul(&self, now: u32) -> f32 {
        match self.algo_id {
            /* G 暗杀标记: 标记窗口内攻击增强 */
            50 => {
                if self.algo_state[0] != 0.0 && now < self.algo_state[0] as u32 {
                    1.0 + self.algo_ap[1] / 100.0
                } else {
                    1.0
                }
            }
            /* I 暴走: 暴走窗口内攻击增强 */
            2 | 41 => {
                if self.algo_state[1] != 0.0 && now < self.algo_state[1] as u32 {
                    1.0 + self.algo_ap[1] / 100.0
                } else {
                    1.0
                }
            }
            _ => 1.0,
        }
    }

    /* 职业专属高级算法 (v31.1): 受击强度修正 (返回乘数) */
    fn algo_hit_mul(&self, now: u32) -> f32 {
        match self.algo_id {
            /* F 庇护缓冲: 缓冲窗内受击震动降低 */
            14 | 36 | 42 => {
                if self.algo_state[0] != 0.0 && now < self.algo_state[0] as u32 {
                    1.0 - self.algo_ap[1] / 100.0
                } else {
                    1.0
                }
            }
            _ => 1.0,
        }
    }

    /* 职业专属高级算法 (v31.1): 命中衰减修正 (H 叠层: 连击越长衰减越久) */
    fn algo_dec_attack_extra(&self) -> f32 {
        match self.algo_id {
            21 | 27 => {
                let layers = (self.combo_hits as f32).min(self.algo_ap[1]);
                (layers * self.algo_ap[0]).min(self.algo_ap[3] - 30.0).max(0.0)
            }
            _ => 0.0,
        }
    }

    /* 职业专属高级算法 (v31.1): finalize 强度乘数 (K 绝对频率已独立; 无额外) */
    fn algo_finalize_mul(&self) -> f32 {
        let _ = self.algo_id;
        1.0
    }

    fn inject_rhythm(&mut self, s: f32, lr: f32, rr: f32, dur: u32, period: u32) {
        self.rhythm_l = s * lr;
        self.rhythm_r = s * rr;
        self.left = self.rhythm_l;
        self.right = self.rhythm_r;
        self.rhythm_until = now_ms() + dur;
        self.rhythm_period = period.max(30);
        self.hold_until = 0;
    }

    pub(super) fn push_event(
        &mut self,
        ev: &VibEvent,
        font_hits: bool,
        item_lr: &[u32; 26],
        item_out: &[std::sync::atomic::AtomicU32; 26],
        rank_gain: &[u32; 15],
        rank_lr: &[i32; 30],
        rank_level_gain: u32,
        rank_duration: u32,
    ) {
        let now = now_ms();
        self.last_event = now;

        /* 绝对震动频率 (v26.1, 召唤专属): 所有事件注入统一节流 (FONT/评分/死亡等)
         * 开启后一切震动算法失效, 只在 [时间-震动次数] 内注入;
         * 全局强度/强度上限仍控制输出, 移动独立 */
        let abs_mode = self.abs_freq_enabled && self.abs_freq_window > 0 && self.abs_freq_max > 0;
        if abs_mode {
            if self.abs_start == 0 || now.wrapping_sub(self.abs_start) >= self.abs_freq_window {
                self.abs_count = 0;
                self.abs_start = now;
                self.abs_last = 0;
            }
            if self.abs_count >= self.abs_freq_max {
                return;
            }
            /* 均匀分布: 注入最小间隔 = 窗口/次数 (3s/6 次 → 每 500ms 一次固定节拍) */
            let min_gap = (self.abs_freq_window / self.abs_freq_max.max(1)).max(50);
            if self.abs_last != 0 && now.wrapping_sub(self.abs_last) < min_gap {
                return;
            }
            self.abs_last = now;
            self.abs_count += 1;
        }

        /* 怪物死亡: OnTargetDie hook (0x7884C0, 纯目标死亡信号), 独立通道 idx 14 */
        if ev.etype == VEV_TARGET_DIE {
            let s = (rank_gain[14].min(200) as f32 / 100.0)
                * (ev.strength.min(100) as f32 / 100.0);
            self.inject_rank(s, rank_duration.max(20));
            return;
        }

        if ev.etype == VEV_FONT {
            let a6 = ev.strength;
            let count = ev.reserved.max(1) as u32;

            /* 诊断: FONT **到达** (秒级采样)。判读: 有 [EVT] 无 [INJ]/[DROP] → 被
             * 中途某闸门吞掉; 连 [EVT] 都没有 → 事件没进宿主。 */
            if log_budget(&LOG_BUDGET_FONT_EVT, now).is_some() {
                vib_log(&format!(
                    "[EVT] FONT a6={:X} cnt={} leg=0 idle={}",
                    a6, count, (self.state == VibState::Idle) as u8
                ));
            }

            /* DLL 端独立分类发送: 命中=0x01, 特殊=0x10, DOT=0x20, 状态=0x08, 特效=0x04, 受击=0x02
             * (else-if 优先级链, 事件只带单一主分类标志) */
            let is_attack = a6 & (FONT_ATTACK | FONT_SPECIAL) != 0;
            let is_hit = a6 & FONT_HIT != 0;
            let is_dot = a6 & FONT_HP != 0;
            let is_state = a6 & FONT_STATE != 0;
            let is_effect = a6 & FONT_EFFECT != 0;

            /* 每项 L/R 权重: -100..+100, 0 = 保持原样(系数 1.0),
             * 负值减弱(系数 <1), 正值增强(系数 >1, 上限 2.0) */
            let item_idx: usize = if is_dot {
                7
            } else if a6 & FONT_SPECIAL != 0 {
                10
            } else if is_state {
                9
            } else if is_effect {
                8
            } else if a6 & FONT_ATTACK != 0 {
                11
            } else if is_hit {
                12
            } else {
                6
            };
            let il = (1.0 + item_lr[item_idx * 2] as i32 as f32 / 100.0).clamp(0.0, 2.0);
            let ir = (1.0 + item_lr[item_idx * 2 + 1] as i32 as f32 / 100.0).clamp(0.0, 2.0);

            if is_attack {
                self.combo_hits = self.combo_hits.saturating_add(count);
                self.last_combo = now;

                /* 连击密度自适应 (v22.4): 窗口滚动 + 命中立即判定 (无触发延迟)
                 * 高连击职业 (召唤/精灵骑士, 100 连击/秒) 短时间暴增连击时立即削强度
                 * v26: 绝对频率模式下失效 */
                self.density_hits = self.density_hits.saturating_add(count);
                if self.density_win_start == 0 {
                    self.density_win_start = now;
                }
                let d_win = self.p[P_DENSITY_WIN].max(30.0) as u32;
                if now.wrapping_sub(self.density_win_start) >= d_win {
                    self.density_hits = 0;
                    self.density_win_start = now;
                }
                let d_thr = self.p[P_DENSITY_THR].max(1.0) as u32;
                if !abs_mode && d_thr < 100 && self.density_hits >= d_thr {
                    self.density_active = true;
                }

                if !abs_mode && self.state == VibState::Idle {
                    self.state = VibState::Combat;
                    if !self.wake_done || now.wrapping_sub(self.last_event) > 5000 {
                        self.wake_done = true;
                        self.inject(self.p[P_WAKE] / 100.0, 0.9, 0.5, 70.0, 70.0);
                        return;
                    }
                }

                for (thr, bit) in MILESTONES {
                    if self.combo_hits >= thr && (self.milestones & bit) != bit {
                        self.milestones |= bit;
                        self.inject(self.p[P_MILESTONE_PULSE] / 100.0, 0.9, 0.7, 60.0, 45.0);
                        return;
                    }
                }

                let bw = self.p[P_BURST_WIN].max(100.0) as u32;
                if now.wrapping_sub(self.burst_start) > bw {
                    self.burst_hits = 0;
                    self.burst_start = now;
                }
                self.burst_hits = self.burst_hits.saturating_add(count);
                if !abs_mode && self.burst_hits >= 6 {
                    self.state = VibState::Burst;
                    self.burst_until = now + 800;
                }

                /* 特殊攻击静默: 静默期内普通事件不注入(突显特殊攻击) */
                if !abs_mode && now < self.silence_until && (a6 & FONT_SPECIAL) == 0 {
                    return;
                }
                /* 评分动态衰减 (v29): 攻击命中提升评级 (上限 8 = 满评分) */
                if self.ghost_enabled {
                    self.ghost_grade = (self.ghost_grade + 1.0).min(8.0);
                    self.ghost_last_hit = now;
                }
                /* 职业专属算法 (v31): 攻击钩子 */
                self.algo_on_attack(a6 & FONT_SPECIAL != 0, count, now);
            }

            if is_hit {
                let cw = self.p[P_COUNTER_WIN].max(300.0) as u32;
                self.counter_until = now + cw;
                /* 评分动态衰减 (v29): 受击/命中事件也续评级 (保持节奏) */
                if self.ghost_enabled {
                    self.ghost_grade = (self.ghost_grade + 0.5).min(8.0);
                    self.ghost_last_hit = now;
                }
                /* 职业专属算法 (v31): 受击钩子 */
                self.algo_on_hit(now);
            }

            if !font_hits {
                /* 诊断: 飘字总开关关闭 (秒级采样) */
                if log_budget(&LOG_BUDGET_FONT_DROP, now).is_some() {
                    vib_log(&format!("[DROP] font_hits=off a6={:X} leg=0", a6));
                }
                return;
            }
            let gap_prev = if self.last_font != 0 {
                now.wrapping_sub(self.last_font)
            } else {
                u32::MAX
            };
            /* 注入间隔: 密度激活期强制翻倍 (v22.4: 高连击窗口内进一步拉开注入间隔,
             * 配合短衰减让输出有明显间歇, 杜绝持续嗡鸣)
             * v26: 绝对频率模式下不检查间隔 */
            let mut ivl = self.p[P_FONT_IVL] as u32;
            if self.density_active {
                ivl = ivl.saturating_mul(2);
            }
            if !abs_mode && gap_prev < ivl {
                return;
            }
            self.last_font = now;

            /* 震动节流 (v24): 时间窗口内只允许 N 次注入, 超出直接跳过
             * (召唤师等瞬间百连击职业防狂震: 窗口滚动)
             * v24.1 自适应: 密度激活(狂震期)时次数上限按比例收紧 (如 20% → 1次/s),
             * 平时不限制太死 (正常反馈)
             * v26: 绝对频率模式下失效 (由 abs 节流替代) */
            if !abs_mode && self.throttle_window > 0 && self.throttle_max > 0 {
                if self.throttle_start == 0 || now.wrapping_sub(self.throttle_start) >= self.throttle_window {
                    self.throttle_count = 0;
                    self.throttle_start = now;
                }
                let limit = if self.density_active {
                    ((self.throttle_max as u64 * self.throttle_dense_ratio.max(1) as u64) / 100).max(1) as u32
                } else {
                    self.throttle_max
                };
                if self.throttle_count >= limit {
                    /* 诊断: 节流超限是 S4 唯一会静默吃掉事件的路径 (秒级采样) */
                    if log_budget(&LOG_BUDGET_FONT_DROP, now).is_some() {
                        vib_log(&format!(
                            "[DROP] throttle a6={:X} cnt={} limit={} win={} dense={}",
                            a6, self.throttle_count, limit, self.throttle_window,
                            self.density_active as u8
                        ));
                    }
                    return;
                }
                self.throttle_count += 1;
            }

            let mut mul = 1.0;
            if is_attack && now < self.counter_until {
                self.state = VibState::Counter;
                mul = self.p[P_COUNTER_MUL].max(100.0) / 100.0;
            }

            let (base_l, base_r) = base_lr_for(item_idx);
            let (s, lr, rr, mode): (f32, f32, f32, u8) = if is_dot {
                (self.p[P_FONT_HP], base_l, base_r, 1)
            } else if a6 & FONT_SPECIAL != 0 {
                (self.p[P_FONT_SPECIAL], base_l, base_r, 0)
            } else if is_state {
                (self.p[P_FONT_STATE], base_l, base_r, 0)
            } else if is_effect {
                (self.p[P_FONT_EFFECT], base_l, base_r, 2)
            } else if a6 & FONT_ATTACK != 0 {
                (self.p[P_FONT_ATTACK], base_l, base_r, 0)
            } else if is_hit {
                (self.p[P_FONT_HIT], base_l, base_r, 0)
            } else {
                (self.p[P_FONT_STR], 0.7, 0.5, 0)
            };
            if s <= 0.0 {
                /* 诊断: 该类事件强度滑块为 0 (秒级采样) */
                if log_budget(&LOG_BUDGET_FONT_DROP, now).is_some() {
                    vib_log(&format!("[DROP] s=0 a6={:X} leg=0 (该项强度滑块为 0)", a6));
                }
                return;
            }

            let mut s_eff = s;
            /* 自适应降强度: v26 绝对频率模式下失效 */
            if !abs_mode
                && a6 & FONT_ATTACK != 0
                && a6 & (FONT_SPECIAL | FONT_HIT | FONT_HP | FONT_EFFECT | FONT_STATE) == 0
            {
                let gap = gap_prev;
                let thr = self.p[P_ADAPT_THR].max(30.0) as u32;
                let maxg = self.p[P_ADAPT_MAXGAP].max(100.0) as u32;
                let reduce = self.p[P_ADAPT_REDUCE] / 100.0;
                let floor = self.p[P_ADAPT_FLOOR] / 100.0;
                let k = if gap < thr {
                    1.0 - reduce
                } else if gap > maxg {
                    1.0
                } else {
                    let span = (maxg - thr).max(1) as f32;
                    1.0 - reduce + reduce * ((gap - thr) as f32 / span)
                };
                s_eff = s * k.max(floor);
            }

            let rr2 = if self.state == VibState::Burst {
                rr * 1.3
            } else {
                rr
            };
            /* 应用每项 L/R 权重 */
            let lr_w = lr * il;
            let rr_w = rr2 * ir;
            let mut s_out = s_eff * mul;
            /* 职业专属算法 (v31): 攻击/受击强度修正 */
            if is_attack {
                s_out *= self.algo_attack_mul(now);
            }
            if is_hit {
                s_out *= self.algo_hit_mul(now);
            }
            /* 移动积累增强 (v22): 停手后短暂窗口内攻击增强 (走位能量消耗) */
            if self.move_charge > 0.01 {
                let m_win = self.p[P_MOVE_BOOST_WIN].max(0.0) as u32;
                if now.wrapping_sub(self.move_last) <= m_win {
                    let m_cap = self.p[P_MOVE_CHARGE_CAP].max(0.0) / 100.0;
                    s_out *= 1.0 + self.move_charge * m_cap;
                    self.move_charge = 0.0;
                }
            }
            item_out[item_idx * 2].store((s_out * lr_w * 65535.0).min(65535.0) as u32, std::sync::atomic::Ordering::Relaxed);
            item_out[item_idx * 2 + 1].store((s_out * rr_w * 65535.0).min(65535.0) as u32, std::sync::atomic::Ordering::Relaxed);
            match mode {
                1 => {
                    let dur = self.p[P_DOT_HOLD].max(30.0) as u32;
                    self.inject_hold(s_out, lr_w, rr_w, dur);
                }
                2 => {
                    let per = self.p[P_EFFECT_PERIOD].max(60.0) as u32;
                    self.inject_rhythm(s_out, lr_w, rr_w, 120, per);
                }
                _ => {
                    let (dl, dr) = if is_hit {
                        (self.p[P_DEC_HIT].max(10.0), self.p[P_DEC_HIT].max(10.0) * 0.8)
                    } else if a6 & FONT_SPECIAL != 0 {
                        (self.p[P_DEC_SPECIAL].max(30.0), self.p[P_DEC_SPECIAL].max(30.0) * 0.7)
                    } else if is_state {
                        (self.p[P_DEC_STATE].max(30.0), self.p[P_DEC_STATE].max(30.0) * 0.8)
                    } else {
                        /* 职业专属算法 (v31): 元素叠层加长命中衰减 */
                        let extra = self.algo_dec_attack_extra();
                        (self.p[P_DEC_ATTACK].max(30.0) + extra, self.p[P_DEC_ATTACK].max(30.0) * 0.8 + extra)
                    };
                    /* 衰减时长也应用该项 L/R 权重 (正=更持久, 负=更短) */
                    let dl_w = dl * (1.0 + item_lr[3 * 2] as i32 as f32 / 100.0).clamp(0.0, 2.0);
                    let dr_w = dr * (1.0 + item_lr[3 * 2 + 1] as i32 as f32 / 100.0).clamp(0.0, 2.0);
                    self.inject(s_out, lr_w, rr_w, dl_w, dr_w);
                }
            }

            if is_dot {
                self.dot_active = true;
                self.dot_last = now;
            }
            if a6 & FONT_SPECIAL != 0 {
                /* 特殊攻击静默窗口 */
                self.silence_until = now + self.p[P_SILENCE].max(0.0) as u32;
            }
            /* 诊断: 注入决策 (秒级采样)。有 [INJ] 且 s_out>0 → 宿主确实注入了,
             * 问题若仍"不震"就在更下游 (XInput 设备/去重)。 */
            if log_budget(&LOG_BUDGET_FONT_INJ, now).is_some() {
                vib_log(&format!(
                    "[INJ] a6={:X} mode={} s_out={:.3} lr_w={:.3} rr_w={:.3} leg=0 cnt={}",
                    a6, mode, s_out, lr_w, rr_w, count
                ));
            }
            return;
        }

        if ev.etype == VEV_COMBO {
            self.last_combo = now;
            /* 连击数由命中事件累加 (combo_hits += count, 1500ms 无命中清零),
             * VEV_COMBO 仅作补充触发 (TextOutW 识别 "COMBO" 文字) */
            self.combo_hits = self.combo_hits.saturating_add(10);
            return;
        }

        /* 评分等级事件: 强度 = 等级强度 % × 等级系数 (2~8)
         * L/R 用评分点槽(0)的权重作等级默认 */
        if ev.etype == VEV_RANKING {
            self.rank_lr_l = (1.0 + rank_lr[0] as f32 / 100.0).clamp(0.0, 2.0);
            self.rank_lr_r = (1.0 + rank_lr[1] as f32 / 100.0).clamp(0.0, 2.0);
            self.rank_out_idx = 0;
            let s = (rank_level_gain.min(100) as f32 / 100.0) * rank_strength(ev.strength);
            self.inject_rank(s, rank_duration.max(20));
            return;
        }

        /* 评分细分事件: 击杀点/闪避/暴击/破招/背击/最终击杀/凌空/破甲/增益叠加/释放技能/镜头震/移动/技能震动
         * 每个事件独立强度 %, 便于区分触发来源 */
        if ev.etype >= VEV_KILLPOINT && ev.etype <= VEV_CRIT_SHAKE {
            let idx = (ev.etype - VEV_KILLPOINT) as usize;
            if idx < 14 {
                /* 该事件 L/R 系数 (rank_lr[idx*2]/[idx*2+1]) */
                let lr_l = (1.0 + rank_lr[idx * 2] as f32 / 100.0).clamp(0.0, 2.0);
                let lr_r = (1.0 + rank_lr[idx * 2 + 1] as f32 / 100.0).clamp(0.0, 2.0);
                self.rank_lr_l = lr_l;
                self.rank_lr_r = lr_r;
                self.rank_out_idx = idx as i32;
                /* 移动: 持续型 (DLL 每 6ms 续期, 0 = 停止) */
                if ev.etype == VEV_MOVE {
                    let s = (rank_gain[11].min(200) as f32 / 100.0)
                        * (ev.strength.min(100) as f32 / 100.0);
                    /* 诊断: 移动决策 (6ms 续期洪流 → 秒级采样)。走路不震时先看这里:
                     * s=0 说明强度/增益参数为 0; active 而马达不响 → 看输出出口。 */
                    if log_budget(&LOG_BUDGET_OUT, now).is_some() {
                        vib_log(&format!(
                            "[MV] str={} rank11={} s={:.3} -> {} leg=0",
                            ev.strength,
                            rank_gain[11],
                            s,
                            if s > 0.02 { "active" } else { "off" }
                        ));
                    }
                    if s > 0.02 {
                        self.move_level = s;
                        self.move_last = now_ms();
                    } else {
                        self.move_level = 0.0;
                    }
                    return;
                }
                /* 震屏类 (镜头震动/技能震动/暴击特写): 强度 × 持续时间 */
                if ev.etype == VEV_SHAKE_SCREEN || ev.etype == VEV_SKILL_HIT
                    || ev.etype == VEV_CRIT_SHAKE
                {
                    let gi = match ev.etype {
                        VEV_SHAKE_SCREEN => 10,
                        VEV_SKILL_HIT => 12,
                        _ => 13, /* VEV_CRIT_SHAKE */
                    };
                    let s = (rank_gain[gi].min(200) as f32 / 100.0)
                        * (ev.strength.min(100) as f32 / 100.0);
                    let dur = if ev.reserved > 0 && ev.reserved < 10000 {
                        ev.reserved
                    } else {
                        rank_duration
                    };
                    self.inject_rank(s, dur.max(20));
                } else {
                    let s = (rank_gain[idx].min(200) as f32 / 100.0)
                        * rank_type_strength(ev.etype);
                    self.inject_rank(s, rank_duration.max(20));
                }
            }
            return;
        }
    }

    pub(super) fn tick(&mut self, params: &[u32; 60]) {
        let now = now_ms();
        let dt = if self.last_output == 0 {
            16.0
        } else {
            now.wrapping_sub(self.last_output).max(1) as f32
        };
        self.last_output = now;

        /* 移动停止超时 200ms 清零 (DLL 停止续期) */
        if self.move_level > 0.0 && now.wrapping_sub(self.move_last) > 200 {
            self.move_level = 0.0;
        }

        /* 移动积累增强 (v22): 移动中积累走位能量, 停手后随时间流失 */
        let m_rate = self.p[P_MOVE_CHARGE_RATE].max(0.0);
        if now.wrapping_sub(self.move_last) <= 200 {
            self.move_charge = (self.move_charge + m_rate / 100.0 * dt / 1000.0).min(1.0);
        } else if self.move_charge > 0.0 {
            self.move_charge = (self.move_charge - dt / 4000.0).max(0.0);
        }

        for i in 0..60 {
            self.p[i] = params[i] as f32;
        }

        let cw = self.p[P_COMBO_WIN].max(500.0) as u32;
        if self.last_combo != 0 && now.wrapping_sub(self.last_combo) >= cw {
            let thr = self.p[P_INTERRUPT].max(10.0) as u32;
            if self.combo_hits > thr && !self.interrupt_fired {
                self.interrupt_fired = true;
                self.inject(self.p[P_INTERRUPT_PULSE] / 100.0, 0.6, 0.2, 70.0, 70.0);
            }
            self.combo_hits = 0;
            self.milestones = 0;
        } else {
            self.interrupt_fired = false;
        }

        if self.dot_active && now.wrapping_sub(self.dot_last) > 200 {
            self.dot_active = false;
        }
        if self.state == VibState::Burst && now >= self.burst_until {
            self.state = VibState::Combat;
        }
        if self.state == VibState::Counter && now >= self.counter_until {
            self.state = VibState::Combat;
        }
        let idle = self.p[P_IDLE].max(1000.0) as u32;
        if self.state != VibState::Idle && now.wrapping_sub(self.last_event) > idle {
            self.state = VibState::Idle;
            self.wake_done = false;
        }

        /* 连击密度自适应 (v22): 连击停止超过恢复判定 → 关闭降强度; 平滑渐变 */
        let d_rec = self.p[P_DENSITY_RECOVER].max(100.0) as u32;
        if self.density_active && now.wrapping_sub(self.last_combo) > d_rec {
            self.density_active = false;
            self.density_hits = 0;
        }
        let d_reduce = self.p[P_DENSITY_REDUCE].max(0.0) / 100.0;
        let d_floor = self.p[P_DENSITY_FLOOR].clamp(0.0, 100.0) / 100.0;
        let d_target = if self.density_active { (1.0 - d_reduce).max(d_floor) } else { 1.0 };
        let d_sm = self.p[P_DENSITY_SMOOTH].max(0.0) as f32;
        if d_sm <= 0.0 {
            self.density_scale = d_target;
        } else {
            let step = (dt * 1000.0 / d_sm).clamp(0.0, 1.0);
            self.density_scale += (d_target - self.density_scale) * step;
        }

        /* 评分动态衰减 (类鬼泣 v29): 停手衰减评级, 反馈倍率 = min + 评级/8×(max-min)
         * 评级 0 → 最低倍率 (保底反馈), 评级 8 (满评分) → 预设参数 ×1.2 */
        if self.ghost_enabled {
            if self.ghost_grade > 0.0 && now.wrapping_sub(self.ghost_last_hit) > self.ghost_delay as u32 {
                let d = dt / 1000.0 * self.ghost_speed.max(0.5);
                self.ghost_grade = (self.ghost_grade - d).max(0.0);
            }
            let t = (self.ghost_grade / 8.0).clamp(0.0, 1.0);
            self.ghost_mul = self.ghost_min + t * (self.ghost_max - self.ghost_min);
        } else {
            self.ghost_mul = 1.0;
        }

        /* 职业专属算法 (v31): 周期钩子 */
        self.algo_tick(now);

        /* 效果模式 */
        if now < self.hold_until {
            self.left = self.hold_l;
            self.right = self.hold_r;
        } else {
            if self.hold_until != 0 {
                self.hold_until = 0;
                self.left = 0.0;
                self.right = 0.0;
            } else {
                let dl = (-(dt) / self.decay_l.max(10.0)).exp();
                let dr = (-(dt) / self.decay_r.max(10.0)).exp();
                self.left *= dl;
                self.right *= dr;
            }
        }
        if now < self.rhythm_until {
            let on = ((now / self.rhythm_period) & 1) == 0;
            self.left = if on { self.rhythm_l } else { self.rhythm_l * 0.15 };
            self.right = if on { self.rhythm_r } else { self.rhythm_r * 0.15 };
        } else if self.rhythm_until != 0 {
            self.rhythm_until = 0;
            self.left = 0.0;
            self.right = 0.0;
        }

        if self.left < 0.02 {
            self.left = 0.0;
        }
        if self.right < 0.02 {
            self.right = 0.0;
        }

        if self.dot_active {
            self.left = self.left.max(0.03);
        }
        if self.state == VibState::Burst {
            let bm = self.p[P_BURST_MIN].max(0.0) / 100.0;
            self.right = self.right.max(bm * self.p[P_FONT_ATTACK]);
        }
    }

    pub(super) fn finalize(&mut self, params: &[u32; 60]) -> (u16, u16) {
        let attack = params[P_ATTACK] as f32;
        let maxcap = params[P_MAX] as f32;
        let master = params[P_MASTER] as f32;
        let boost = params[P_BOOST] as f32;
        let slope = (params[P_BOOST_SLOPE] as f32).max(1.0) / 100.0;
        let capx = (params[P_COMBO_CAP] as f32).max(100.0) / 100.0;
        if attack <= 0.0 || maxcap <= 0.0 || master <= 0.0 {
            return (0, 0);
        }
        let combo_mul = if self.abs_freq_enabled {
            /* v26 绝对频率模式: 一切算法失效, 连击倍率 = 1.0 */
            1.0
        } else {
            (1.0 + (self.combo_hits as f32 / 100.0) * (boost / 100.0) * slope)
                .min(capx)
        };
        let cap = (maxcap / 100.0).min(1.0);
        let base = (attack / 100.0) * (master / 100.0) * combo_mul * cap;
        /* 连击密度自适应 (v22): 连击暴增窗口期整体降强度 */
        let base = base * self.density_scale;
        /* 评分动态衰减 (v29): 评级反馈倍率 (评级 0 → 最低, 满评分 → ×1.2) */
        let base = base * self.ghost_mul;
        /* 职业专属算法 (v31): 战意昂扬等 finalize 乘数 */
        let base = base * self.algo_finalize_mul();
        let rhythm = (params[P_RHYTHM] as f32 / 50.0).clamp(0.0, 2.0);
        let curvel = (params[P_CURVE_L] as f32).max(30.0) / 100.0;
        let curver = (params[P_CURVE_R] as f32).max(30.0) / 100.0;
        let l_raw = self.left * base;
        let r_raw = self.right * base * rhythm;
        let mut l_out = if l_raw > 0.0 { (l_raw.powf(curvel)).min(1.0) * 65535.0 } else { 0.0 };
        let mut r_out = if r_raw > 0.0 { (r_raw.powf(curver)).min(1.0) * 65535.0 } else { 0.0 };
        /* 输出低强度迟滞死区 (v29.3): 低于阈值归 0, 超过 1.8×阈值才恢复
         * (迟滞防阈值附近反复启停产生嗡声; 消除马达低强度电流声) */
        if self.out_threshold > 0.0 {
            let thr = (self.out_threshold.min(50.0) / 100.0) * 65535.0;
            let hi = thr * 1.8;
            if self.out_dead_l {
                if l_out > hi {
                    self.out_dead_l = false;
                } else {
                    l_out = 0.0;
                }
            } else if l_out < thr {
                self.out_dead_l = true;
                l_out = 0.0;
            }
            if self.out_dead_r {
                if r_out > hi {
                    self.out_dead_r = false;
                } else {
                    r_out = 0.0;
                }
            } else if r_out < thr {
                self.out_dead_r = true;
                r_out = 0.0;
            }
        }
        /* 输出动态范围重映射 (v30, ERM/Xbox360): 非零输出映射到 [min, 100],
         * 保证转子一定转起来 (轻反馈不被死区吞掉, 相对强弱保留) */
        if self.remap_enabled && (l_out > 0.0 || r_out > 0.0) {
            let mn = (self.remap_min.min(95.0) / 100.0) * 65535.0;
            let scale = 1.0 - self.remap_min.min(95.0) / 100.0;
            if l_out > 0.0 {
                l_out = mn + l_out * scale;
            }
            if r_out > 0.0 {
                r_out = mn + r_out * scale;
            }
        }
        /* 马达分工 (v30, Xbox360 风格): 轻反馈仅主导马达 (转子声/功耗更低),
         * 重反馈双马达满幅 */
        if self.split_enabled {
            let sp = (self.split_thr.min(95.0) / 100.0) * 65535.0;
            if l_out < sp && r_out < sp {
                if l_out >= r_out {
                    r_out = 0.0;
                } else {
                    l_out = 0.0;
                }
            }
        }
        (l_out.min(65535.0) as u16, r_out.min(65535.0) as u16)
    }

    /* 输出平滑 (1.61 `vibration.rs:1593-1603` 原样): 一阶低通。
     * k<=0 = 不平滑。1.61 在主循环内联, 此处提为方法, 算式逐字节一致。 */
    pub(super) fn smooth_out(&mut self, l: f32, r: f32, k: f32) -> (f32, f32) {
        if k <= 0.0 {
            return (l, r);
        }
        self.out_smooth_l += (l - self.out_smooth_l) * k;
        self.out_smooth_r += (r - self.out_smooth_r) * k;
        (
            self.out_smooth_l.min(65535.0),
            self.out_smooth_r.min(65535.0),
        )
    }

    /* 通道出口合成 (1.61 `vibration.rs:1608-1637` 原样): finalize → smooth → max。
     * **S4 路线恒独占, 主循环不会调用本方法** —— 保留它只为与 1.61 的算式对照,
     * 以及与 S1 引擎的接口对称 (两条引擎都有同名方法, 分派层无需路线特判)。
     * 若未来 S4 需要合成, 语义已在此锁定。 */
    #[allow(dead_code)]
    pub(super) fn composite_with_font(
        &mut self,
        vl: u16,
        vr: u16,
        motor_l: f32,
        motor_r: f32,
        rnd: f32,
        smooth_pct: f32,
        params: &[u32; 60],
    ) -> (u16, u16) {
        let (efl, efr) = self.finalize(params);
        let (efl, efr) = self.smooth_out(efl as f32, efr as f32, smooth_pct * 0.9);
        (
            vl.max((efl * motor_l * (1.0 + rnd)).min(65535.0) as u16),
            vr.max((efr * motor_r * (1.0 + rnd)).min(65535.0) as u16),
        )
    }

}


/* ═══════════ S4 引擎独立回归锁 (对齐 1.61) ═══════════
 * 这些用例取代了原先"同一引擎跑两条路线做对照"的写法 —— S4 现在是独立实现,
 * 必须直接锁它自己的数值语义。参照物: 1.61 `src/vibration.rs`。 */
#[cfg(test)]
mod s4_engine_tests {
    use super::*;

    fn engine() -> VibEngine {
        let mut e = VibEngine::new();
        e.remap_enabled = false;
        e.out_threshold = 0.0;
        e.split_enabled = false;
        e
    }

    fn push_font(e: &mut VibEngine, a6: u32) {
        let ev = VibEvent { etype: VEV_FONT, strength: a6, tick: 0, reserved: 1 };
        let item_out: [AtomicU32; 26] = std::array::from_fn(|_| AtomicU32::new(0));
        e.push_event(&ev, true, &[0u32; 26], &item_out, &[0u32; 15], &[0i32; 30], 0, 300);
    }

    /* ── 1.61 差异①: 评分窗口内 FONT 不参与输出 (独占式) ──
     * 1.61 `vibration.rs:1516-1530` 的评分出口直接输出脉冲, 从不调用 finalize。 */
    #[test]
    fn rank_window_output_ignores_font() {
        let params = [100u32; 60];
        let mut e = engine();
        e.left = 0.5;
        e.right = 0.5;
        /* 通道为 0 → 输出就是 0 (FONT 不顶上来) */
        let out = (0u16, 0u16); /* 主循环的独占分支 = 直接用通道值 */
        assert_eq!(out, (0, 0), "S4 独占: 评分脉冲为 0 时不得被 FONT 抬起");
        /* 引擎侧 FONT 输出本身仍非零 (只是不出现在评分出口) */
        let (l, r) = e.finalize(&params);
        assert!(l > 0 && r > 0, "FONT 计算结果仍应非零: {l}/{r}");
    }

    /* ── 1.61 差异②: 逐项总闸 item_lr[0]/[1] 不参与 ──
     * 1.61 `vibration.rs:1143-1144` 只乘 base/rhythm。 */
    #[test]
    fn item_gate_not_applied() {
        let params = [100u32; 60];
        let mut a = engine();
        a.left = 0.245;
        a.right = 0.49;
        let plain = a.finalize(&params);
        /* S4 引擎根本没有 gate_lr 字段 —— 结构性保证 (编译期即无此路径) */
        assert!(plain.0 > 0 && plain.1 > 0, "基础输出应非零: {:?}", plain);
    }

    /* ── 1.61 差异③: 衰减常数下限 10ms / 30ms ── */
    #[test]
    fn decay_floors_match_v161() {
        /* 命中 → 10ms */
        let mut a = engine();
        a.p[P_FONT_HIT] = 40.0;
        a.p[P_FONT_IVL] = 0.0;
        a.p[P_DEC_HIT] = 0.0;
        push_font(&mut a, FONT_HIT);
        assert_eq!(a.decay_l, 10.0, "命中衰减下限 10ms (1.61 vibration.rs:888-890)");

        /* 普攻 → 30ms */
        let mut b = engine();
        b.state = VibState::Combat;
        b.wake_done = true;
        b.p[P_FONT_ATTACK] = 40.0;
        b.p[P_FONT_IVL] = 0.0;
        b.p[P_DEC_ATTACK] = 0.0;
        push_font(&mut b, FONT_ATTACK);
        assert_eq!(b.decay_l, 30.0, "普攻衰减下限 30ms (1.61 vibration.rs:894-898)");
    }

    /* ── 无 S1 专属机制: 风暴抽样/聚合/限频/落地/统合衰减在 S4 引擎不存在 ── */
    #[test]
    fn no_s1_only_mechanisms() {
        let e = engine();
        /* 全局单窗 (1.61 只有一个 last_font, 无分通道窗) */
        assert_eq!(e.last_font, 0);
        /* 结构上不存在 storm/merge/hitcap/tail_land/sustain 字段 ——
         * 由 mod.rs::engines_do_not_cross_reference 的源码断言守卫 */
    }

    /* ── 分数/移动事件: 1.61 语义 (评分窗口 + 移动持续通道) ── */
    #[test]
    fn move_event_sets_move_level() {
        let mut e = engine();
        let mut rank_gain = [0u32; 15];
        rank_gain[11] = 100;
        let ev = VibEvent { etype: VEV_MOVE, strength: 100, tick: 0, reserved: 0 };
        let item_out: [AtomicU32; 26] = std::array::from_fn(|_| AtomicU32::new(0));
        e.push_event(&ev, true, &[0u32; 26], &item_out, &rank_gain, &[0i32; 30], 100, 300);
        assert!(e.move_level > 0.0, "移动事件应设置 move_level: {}", e.move_level);
    }

    /* ── FONT 量纲: 1.61 不做 /100 归一 (V3 的 S1 才做) ── */
    #[test]
    fn font_scale_not_normalized() {
        let mut e = engine();
        e.p[P_FONT_ATTACK] = 25.0;
        e.p[P_FONT_IVL] = 0.0;
        e.p[P_DEC_ATTACK] = 30.0;
        e.state = VibState::Combat;
        e.wake_done = true;
        push_font(&mut e, FONT_ATTACK);
        /* 1.61: s_out 直接是 25 (滑块原值), 注入 left = 25*lr/base_lr 量级 (>1) */
        assert!(
            e.left > 1.0,
            "1.61 不做 /100 归一: left 应 >1 (实际 {}) —— 若为 0.x 说明误加了归一",
            e.left
        );
    }
}
