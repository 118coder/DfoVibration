//! Vibration engine: reads battle events from DfoVibration.dll (shared memory)
//! and drives the Xbox gamepad motors via XInputSetState.
//!
//! Shared memory layout must match common/vib_protocol.h (C).
//! 60 参数槽位, 基础 0-18 / 高级 19-59。

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use windows::core::w;
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::System::Memory::{
    MapViewOfFile, OpenFileMappingW, UnmapViewOfFile, FILE_MAP_ALL_ACCESS,
};
use windows::Win32::UI::Input::XboxController::{XInputSetState, XINPUT_VIBRATION};

use crate::state::AppState;

const VIB_SHM_MAGIC: u32 = 0x564F4656;
const VIB_RING_SIZE: usize = 256 * 1024;

const VEV_FONT: u32 = 10;
const VEV_COMBO: u32 = 9;
const VEV_RANKING: u32 = 11;
const VEV_TARGET_DIE: u32 = 2;  /* 怪物死亡 (OnTargetDie hook 0x7884C0) */
const VEV_KILLPOINT: u32 = 12;
const VEV_DODGE: u32 = 13;
const VEV_CRIT: u32 = 14;
const VEV_BREAK: u32 = 15;
const VEV_BACK: u32 = 16;
const VEV_FINAL_KILL: u32 = 17;
const VEV_AERIAL: u32 = 18;
const VEV_ARMOR_BREAK: u32 = 19;
const VEV_BUFF_STACK: u32 = 20;
const VEV_KILL: u32 = 21;          /* 释放技能 (n2500[5747]) */
const VEV_SHAKE_SCREEN: u32 = 22;  /* 真镜头震动 [shake screen] */
const VEV_MOVE: u32 = 23;          /* 移动持续震动 */
const VEV_SKILL_HIT: u32 = 24;     /* 技能震动 */
const VEV_CRIT_SHAKE: u32 = 25;    /* 暴击/击杀特写震屏 */

/* 评分震动映射: 测试模式全强度 (0~8 全部满震, 便于感知事件是否生效)
 * 等级: 0=SSS 1=SS 2=S 3=A 4=B 5=C 6=D 7=E 8=F (数字越低评价越高) */
fn rank_strength(level: u32) -> f32 {
    match level {
        0..=8 => 1.0,
        _ => 0.0,
    }
}

/* 评分细分事件强度: 测试模式全强度 */
fn rank_type_strength(etype: u32) -> f32 {
    match etype {
        VEV_KILLPOINT => 1.0, /* 击杀点 */
        VEV_DODGE => 1.0,     /* 极限闪避 */
        VEV_CRIT => 1.0,      /* 暴击 */
        VEV_BREAK => 1.0,     /* 破招 */
        VEV_BACK => 1.0,      /* 背击 */
        VEV_FINAL_KILL => 1.0, /* 最终击杀 */
        VEV_AERIAL => 1.0,    /* 凌空追击 */
        VEV_ARMOR_BREAK => 1.0, /* 破甲 */
        VEV_BUFF_STACK => 1.0, /* 增益叠加 */
        VEV_KILL => 1.0,      /* 释放技能 */
        VEV_SHAKE_SCREEN => 1.0, /* 镜头震动 */
        VEV_MOVE => 1.0,      /* 移动 */
        VEV_SKILL_HIT => 1.0, /* 技能震动 */
        VEV_CRIT_SHAKE => 1.0, /* 暴击/击杀特写 */
        _ => 0.0,
    }
}

const FONT_ATTACK: u32 = 0x01;
const FONT_HIT: u32 = 0x02;
const FONT_EFFECT: u32 = 0x04;
const FONT_STATE: u32 = 0x08;
const FONT_SPECIAL: u32 = 0x10;
const FONT_HP: u32 = 0x20;

/* 高级 B 组六类 L/R 比例已移除(与基础区每项权重重复), 改用内置固定基准 */
const BASE_LR: [(f32, f32); 6] = [
    (0.25, 0.50),  /* 0 DOT */
    (0.45, 0.85),  /* 1 特殊攻击 */
    (0.80, 0.25),  /* 2 受击 */
    (0.35, 0.70),  /* 3 普通命中 */
    (0.30, 0.70),  /* 4 装备特效 */
    (0.60, 0.50),  /* 5 状态 */
];

fn base_lr_for(item_idx: usize) -> (f32, f32) {
    let i = match item_idx {
        7 => 0,   /* DOT */
        10 => 1,  /* 特殊 */
        12 => 2,  /* 受击 */
        11 => 3,  /* 命中 */
        8 => 4,   /* 特效 */
        9 => 5,   /* 状态 */
        _ => 3,
    };
    BASE_LR[i]
}
#[allow(dead_code)]
const P_ATTACK: usize = 0;
#[allow(dead_code)]
const P_MAX: usize = 4;
const P_BOOST: usize = 6;
const P_MASTER: usize = 9;
const P_FONT_STR: usize = 10;
const P_FONT_IVL: usize = 11;
const P_FONT_HP: usize = 12;
const P_FONT_SPECIAL: usize = 13;
const P_FONT_STATE: usize = 14;
const P_FONT_EFFECT: usize = 15;
const P_FONT_ATTACK: usize = 16;
const P_FONT_HIT: usize = 17;
const P_RHYTHM: usize = 18;
/* 高级 A */
const P_CURVE_L: usize = 19;
const P_CURVE_R: usize = 20;
/* 移动积累增强 (v22): 长时间移动积累"走位能量", 停止移动后下次短暂
 * 窗口期的攻击增强震动 (走位职业强化)。原 21/22 混合/相位槽位复用。 */
const P_MOVE_CHARGE_RATE: usize = 21; /* 移动积累速率 %/秒 (0=禁用) */
const P_MOVE_CHARGE_CAP: usize = 22;  /* 攻击增强上限 % (最多 ×(1+cap/100)) */
/* 高级 A2: 移动走路质感参数 (v20.2 新增, 原 23-26 为移除的 B 组 L/R) */
const P_MOVE_PACE: usize = 23;       /* 移动步频 ms (默认 380, 自然步频) */
const P_MOVE_PULSE: usize = 24;      /* 移动着地脉冲 % (默认 35, 柔和) */
const P_MOVE_HOLD: usize = 25;       /* 移动抬脚保持 % (默认 8, 极轻) */
const P_MOVE_GAIN: usize = 26;       /* 移动整体增益 % (默认 40, 轻音量) */
const P_MOVE_SMOOTH: usize = 27;     /* 移动平滑系数 % (默认 30, 消除嗡嗡声) */
const P_MOVE_THRESHOLD: usize = 28;  /* 移动最低输出阈值 % (默认 4, 低于归 0 消除沙沙声) */

/* 连击密度自适应 (v22): 短时间连击暴增时自动降低窗口期震动强度
 * 29-34 原为 B 组残留槽位, 现分配为密度算法参数 (高连击职业防震手核心) */
const P_DENSITY_THR: usize = 29;    /* 密度触发阈值 hits/窗口 (100=禁用) */
const P_DENSITY_WIN: usize = 30;    /* 密度检测窗口 ms */
const P_DENSITY_REDUCE: usize = 31; /* 降幅 % (窗口期强度 ×(1-reduce)) */
const P_DENSITY_RECOVER: usize = 32;/* 恢复判定 gap ms (最后命中超过此值→强度恢复) */
const P_DENSITY_FLOOR: usize = 33;  /* 最低保留 % (降幅不超 (1-floor)) */
const P_DENSITY_SMOOTH: usize = 34; /* 恢复平滑 ms (渐变恢复, 0=立即) */
/* 高级 C 衰减 */
const P_DEC_ATTACK: usize = 35;
const P_DEC_SPECIAL: usize = 36;
const P_DEC_HIT: usize = 37;
const P_DEC_STATE: usize = 38;
#[allow(dead_code)]
const P_DEC_EFFECT: usize = 39;
/* 移动积累增强窗口 (v22): 停止移动后增强有效期 ms (原 39 装备特效衰减未用) */
const P_MOVE_BOOST_WIN: usize = 39;

/* 高级 D 时长窗口 */
const P_DOT_HOLD: usize = 40;
const P_EFFECT_PERIOD: usize = 41;
const P_BURST_WIN: usize = 42;
const P_BURST_MIN: usize = 43;
const P_COUNTER_WIN: usize = 44;
const P_COMBO_WIN: usize = 45;
const P_IDLE: usize = 46;
/* 高级 E 连击/自适应 */
const P_COMBO_CAP: usize = 47;
const P_BOOST_SLOPE: usize = 48;
const P_INTERRUPT: usize = 49;
const P_ADAPT_THR: usize = 50;
const P_ADAPT_REDUCE: usize = 51;
const P_ADAPT_MAXGAP: usize = 52;
const P_ADAPT_FLOOR: usize = 53;
/* 高级 F/G/H */
const P_SILENCE: usize = 54;
const P_COUNTER_MUL: usize = 55;
const P_WAKE: usize = 56;
const P_MILESTONE_PULSE: usize = 57;
const P_INTERRUPT_PULSE: usize = 58;
const P_TEST: usize = 59;

/* 里程碑固定档位 */
const MILESTONES: [(u32, u32); 4] = [(50, 0x1), (100, 0x2), (200, 0x4), (400, 0x8)];

#[derive(PartialEq, Clone, Copy)]
enum VibState {
    Idle = 0,
    Combat = 1,
    Burst = 2,
    Counter = 3,
}

struct VibEngine {
    last_output: u32,
    last_event: u32,
    left: f32,
    right: f32,
    decay_l: f32,
    decay_r: f32,
    hold_until: u32,
    hold_l: f32,
    hold_r: f32,
    rhythm_until: u32,
    rhythm_period: u32,
    rhythm_l: f32,
    rhythm_r: f32,
    last_font: u32,
    combo_hits: u32,
    last_combo: u32,
    milestones: u32,
    state: VibState,
    burst_until: u32,
    burst_hits: u32,
    burst_start: u32,
    counter_until: u32,
    silence_until: u32,
    dot_active: bool,
    dot_last: u32,
    wake_done: bool,
    interrupt_fired: bool,
    p: [f32; 60],
    rng_state: u32,
    rank_full_until: u32,
    rank_level: f32,
    /* 当前评分事件的 L/R 马达系数 (rank_lr 权重, 默认 1.0) */
    rank_lr_l: f32,
    rank_lr_r: f32,
    /* 当前评分事件 idx (0-14, -1 = 无) 用于实时输出进度条 */
    rank_out_idx: i32,
    /* 移动持续震动: 独立输出通道 (不随 finalize 总闸关闭) */
    move_level: f32,
    move_last: u32,
    /* 移动走路质感: 步伐节奏 (间隔/相位), 模拟左右脚交替轻震 */
    move_pace_until: u32,
    move_phase: bool,
    /* 移动平滑输出 (消除嗡嗡/沙沙声: 平滑插值状态) */
    move_out_l: f32,
    move_out_r: f32,
    /* 连击密度自适应 (v22): 窗口期连击密度统计与降强度状态 */
    density_hits: u32,
    density_win_start: u32,
    density_active: bool,
    density_scale: f32,
    /* 移动积累增强 (v22): 走位能量 (0..1), 停止移动后窗口内攻击增强 */
    move_charge: f32,
    /* 输出平滑 (v22.3): 一阶低通抑制低频嗡嗡声 (快速连击时输出跳变被柔化) */
    out_smooth_l: f32,
    out_smooth_r: f32,
    /* 震动节流 (v24): 限定时间窗口内最大注入次数 (召唤师等狂震职业防狂震) */
    throttle_window: u32,
    throttle_max: u32,
    throttle_dense_ratio: u32,
    throttle_count: u32,
    throttle_start: u32,
    /* 绝对震动频率 (v26, 召唤专属): 一切算法失效, 只在 时间-次数 内注入 */
    abs_freq_enabled: bool,
    abs_freq_window: u32,
    abs_freq_max: u32,
    abs_count: u32,
    abs_start: u32,
    abs_last: u32,
    /* 评分动态衰减 (类鬼泣, v29): 攻击提升评级, 停手衰减, 反馈倍率联动 */
    ghost_grade: f32,
    ghost_last_hit: u32,
    ghost_mul: f32,
    ghost_enabled: bool,
    ghost_delay: f32,
    ghost_speed: f32,
    ghost_min: f32,
    ghost_max: f32,
    /* 输出低强度死区 (v29.2): 主通道输出低于此值归 0 (消除马达低强度嗡声) */
    out_threshold: f32,
    /* 迟滞状态 (v29.3): 死区边缘迟滞, 防阈值附近反复启停产生嗡声 */
    out_dead_l: bool,
    out_dead_r: bool,
    /* 输出重映射 (v30, ERM/Xbox360): 非零输出映射到 [min,100] */
    remap_enabled: bool,
    remap_min: f32,
    /* 马达分工 (v30, Xbox360 风格): 轻反馈单马达, 重反馈双马达 */
    split_enabled: bool,
    split_thr: f32,
    /* 职业专属高级算法 (v31): id 1-15 + 参数 + 运行状态 */
    algo_id: u32,
    algo_ap: [f32; 4],
    algo_state: [f32; 4],
}

impl VibEngine {
    fn new() -> Self {
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
    fn rng_next(&mut self) -> u32 {
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

    fn push_event(
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

    fn tick(&mut self, params: &[u32; 60]) {
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

    fn finalize(&mut self, params: &[u32; 60]) -> (u16, u16) {
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
}

fn now_ms() -> u32 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u32)
        .unwrap_or(0)
}

pub fn now_ms_u64() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn process_alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    unsafe {
        use windows::Win32::System::Threading::{
            GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
        };
        if let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
            if h.is_invalid() {
                return false;
            }
            let mut code: u32 = 0;
            let ok = GetExitCodeProcess(h, &mut code).is_ok();
            let _ = CloseHandle(h);
            ok && code == 259
        } else {
            false
        }
    }
}

/* ---------------- 共享内存结构 ---------------- */
#[repr(C)]
struct VibEvent {
    etype: u32,
    strength: u32,
    tick: u32,
    reserved: u32,
}

#[repr(C)]
struct VibRing {
    head: AtomicU32,
    tail: AtomicU32,
    capacity: u32,
    data: [u8; VIB_RING_SIZE],
}

#[repr(C)]
struct VibShm {
    magic: u32,
    version: u32,
    game_pid: u32,
    flags: u32,
    seq: u32,
    last_tick: u32,
    api_version: [u32; 7],
    ring: VibRing,
}

const _: () = assert!(std::mem::size_of::<VibEvent>() == 16);
const _: () = assert!(std::mem::size_of::<VibShm>() == 262208);

/* ---------------- 运行线程 ---------------- */
pub fn run(state: Arc<AppState>) {
    std::thread::Builder::new()
        .name("vibration_thread".to_string())
        .spawn(move || {
            eprintln!("[vib] engine thread started");
            let mut shm_handle: Option<HANDLE> = None;
            let mut shm_view: Option<windows::Win32::System::Memory::MEMORY_MAPPED_VIEW_ADDRESS> = None;
            let mut engine = VibEngine::new();

            loop {
                if state.should_exit.load(Ordering::Relaxed) {
                    break;
                }

                let mut params: [u32; 60] = [0; 60];
                for i in 0..60 {
                    let v = state.vibration_params[i].load(Ordering::Relaxed);
                    if v != 0 {
                        params[i] = v;
                    }
                }
                let motor_l = state.vibration_motor_l_gain.load(Ordering::Relaxed) as f32 / 100.0;
                let motor_r = state.vibration_motor_r_gain.load(Ordering::Relaxed) as f32 / 100.0;
                /* 震动随机性: 0=关闭, N=在强度上限附近随机增减 ±N% */
                let random_gain = state.vibration_random_gain.load(Ordering::Relaxed) as f32;
                let rnd = if random_gain > 0.0 {
                    let x = engine.rng_next();
                    let span = (random_gain * 2.0) as u32 + 1; /* 0..=2N */
                    ((x % span) as f32 - random_gain) / 100.0 /* -N..=+N % */
                } else {
                    0.0
                };
                let enabled = state.vibration_enabled.load(Ordering::Relaxed)
                    && !state.is_paused();

                if shm_view.is_none() {
                    if let Ok(h) = unsafe {
                        OpenFileMappingW(FILE_MAP_ALL_ACCESS.0, false, w!("Local\\DfoVibrationShm"))
                    } {
                        if !h.is_invalid() {
                            let view = unsafe {
                                MapViewOfFile(h, FILE_MAP_ALL_ACCESS, 0, 0,
                                              std::mem::size_of::<VibShm>())
                            };
                            if !view.Value.is_null() {
                                let s = unsafe { &mut *(view.Value as *mut VibShm) };
                                if s.magic == VIB_SHM_MAGIC {
                                    shm_handle = Some(h);
                                    shm_view = Some(view);
                                } else {
                                    let _ = unsafe { UnmapViewOfFile(view) };
                                    let _ = unsafe { CloseHandle(h) };
                                }
                            } else {
                                let _ = unsafe { CloseHandle(h) };
                            }
                        }
                    }
                }

                if let Some(view) = shm_view {
                    let s = unsafe { &mut *(view.Value as *mut VibShm) };
                    let connected = s.magic == VIB_SHM_MAGIC && process_alive(s.game_pid);
                    state.vibration_connected.store(connected, Ordering::Relaxed);

                    if connected {
                        let head = s.ring.head.load(Ordering::Acquire);
                        let tail = s.ring.tail.load(Ordering::Relaxed);

                        // 绝对稳定: 共享内存由 DfoVibration.dll 写入(不可信 —— DLL 崩溃、
                        // 写序被中断、版本不匹配时内容可能被污染)。逐项校验后读取,
                        // 任一异常即丢弃本轮(绝不除零 panic / 越界读 / 死循环)。
                        //
                        // 关键语义: head/tail 是"累计字节偏移"(DLL 每写一事件 head+=16,
                        // 软件每读一事件 tail+=16, 二者持续增长、会超过 capacity),
                        // 通过 % capacity 取环内位置。因此**不能用 head<=capacity / tail<=capacity
                        // 判断合法** —— 那会在玩了一会 head 超过 262144 后恒判非法,
                        // 事件永远被丢弃 → 震动中断。正确校验用两者(无符号)差值:
                        // 未满(head-tail<capacity, 正常可读)或恰好满(=capacity)均合法;
                        // 差值过大(落后太多/DLL 重启后 head 回绕)才判异常丢弃。
                        let ev_size = std::mem::size_of::<VibEvent>() as u32;
                        let max_events = (VIB_RING_SIZE as u32) / ev_size;
                        let capacity = s.ring.capacity;
                        let ring_ok = capacity == VIB_RING_SIZE as u32
                            && capacity % ev_size == 0
                            && head.wrapping_sub(tail) <= capacity;
                        let mut t = if ring_ok { tail } else { head }; // 异常时跳过读取
                        let mut got = 0u32;
                        let font_hits = state.vibration_font_hits.load(Ordering::Relaxed);
                        let mut item_lr = [0u32; 26];
                        for (i, v) in item_lr.iter_mut().enumerate() {
                            *v = state.vibration_item_lr[i].load(Ordering::Relaxed);
                        }
                        let mut rank_gain = [0u32; 15];
                        for (i, v) in rank_gain.iter_mut().enumerate() {
                            *v = state.vibration_rank_type_gain[i].load(Ordering::Relaxed);
                        }
                        let mut rank_lr = [0i32; 30];
                        for (i, v) in rank_lr.iter_mut().enumerate() {
                            *v = state.vibration_rank_lr[i].load(Ordering::Relaxed) as i32;
                        }
                        let rank_level_gain = state.vibration_rank_level_gain.load(Ordering::Relaxed);
                        let rank_duration = state.vibration_rank_duration.load(Ordering::Relaxed);
                        // got < max_events 上限保证单轮读取必然终止(异常 head 不再挂死)
                        while ring_ok && t != head && got < max_events {
                            let pos = (t % capacity) as usize;
                            let ev = unsafe {
                                std::ptr::read_unaligned(
                                    s.ring.data.as_ptr().add(pos) as *const VibEvent
                                )
                            };
                            engine.push_event(
                                &ev,
                                font_hits,
                                &item_lr,
                                &state.vibration_item_out,
                                &rank_gain,
                                &rank_lr,
                                rank_level_gain,
                                rank_duration,
                            );
                            if ev.etype == VEV_RANKING {
                                state.vibration_rank_events.fetch_add(1, Ordering::Relaxed);
                                state.vibration_rank_last.store(
                                    ev.strength.min(8),
                                    Ordering::Relaxed,
                                );
                                eprintln!("[vib] RANKING event level={} received", ev.strength);
                            } else if ev.etype == VEV_TARGET_DIE {
                                state.vibration_rank_type_events[14].fetch_add(1, Ordering::Relaxed);
                                eprintln!("[vib] TARGET_DIE event strength={} received", ev.strength);
                            } else if ev.etype >= VEV_KILLPOINT && ev.etype <= VEV_CRIT_SHAKE {
                                let idx = (ev.etype - VEV_KILLPOINT) as usize;
                                if idx < 14 {
                                    state.vibration_rank_type_events[idx].fetch_add(1, Ordering::Relaxed);
                                }
                                eprintln!("[vib] RANK-TYPE event type={} strength={} received", ev.etype, ev.strength);
                            }
                            t = t.wrapping_add(ev_size);
                            got = got.wrapping_add(1);
                        }
                        if ring_ok {
                            s.ring.tail.store(t, Ordering::Release);
                            if got > 0 {
                                state.vibration_events_received.fetch_add(got as u64, Ordering::Relaxed);
                            }
                        } else {
                            // 头尾异常(DLL 重启 head 回绕 / 软件漏读落后超界): 不能一直丢
                            // (会让震动中断), 把 tail 同步到 head 丢弃积压, 下轮立即恢复读取。
                            crate::util::crash_log(
                                "VIB_RING_INVALID",
                                &format!("cap={capacity} head={head} tail={tail} (已同步 tail=head)"),
                            );
                            s.ring.tail.store(head, Ordering::Release);
                        }
                    } else {
                        let _ = unsafe { UnmapViewOfFile(view) };
                        if let Some(h) = shm_handle.take() {
                            let _ = unsafe { CloseHandle(h) };
                        }
                        shm_view = None;
                        engine.left = 0.0;
                        engine.right = 0.0;
                        let zero = XINPUT_VIBRATION { wLeftMotorSpeed: 0, wRightMotorSpeed: 0 };
                        unsafe { let _ = XInputSetState(0, &zero); }
                    }
                } else {
                    state.vibration_connected.store(false, Ordering::Relaxed);
                }

                let zero = XINPUT_VIBRATION { wLeftMotorSpeed: 0, wRightMotorSpeed: 0 };
                if enabled {
                    /* 评分测试: 模拟等级 8 事件 (GUI 测试按钮) */
                    if state.vibration_rank_test.swap(false, Ordering::Relaxed) {
                        state.vibration_rank_events.fetch_add(1, Ordering::Relaxed);
                        state.vibration_rank_last.store(8, Ordering::Relaxed);
                        let fake = VibEvent {
                            etype: VEV_RANKING,
                            strength: 8,
                            tick: 0,
                            reserved: 0,
                        };
                        engine.push_event(&fake, true, &[0u32; 26], &state.vibration_item_out, &[100u32; 15], &[0i32; 30], 100, 300);
                    }
                    let test_until = state.vibration_test_until.load(Ordering::Relaxed);
                    /* 必须用 u64 完整毫秒与 test_until 比较, 否则 u32 截断永远小于, 测试永不结束 */
                    let now_ms_val = now_ms_u64();
                    if test_until != 0 && now_ms_val < test_until {
                        let pct = params[P_TEST].max(1) as f32 / 100.0;
                        let vl = (65535.0 * pct * motor_l).min(65535.0) as u16;
                        let vr = (65535.0 * pct * motor_r).min(65535.0) as u16;
                        let vib = XINPUT_VIBRATION { wLeftMotorSpeed: vl, wRightMotorSpeed: vr };
                        unsafe { let _ = XInputSetState(0, &vib); }
                        state.vibration_out_l.store(vl as u32, Ordering::Relaxed);
                        state.vibration_out_r.store(vr as u32, Ordering::Relaxed);
                    } else {
                        if test_until != 0 {
                            state.vibration_test_until.store(0, Ordering::Relaxed);
                        }
                        engine.tick(&params);
                        /* 节流参数 (v24, 独立字段): 每帧从 AppState 同步 */
                        engine.throttle_window = state.vibration_throttle_window.load(Ordering::Relaxed);
                        engine.throttle_max = state.vibration_throttle_max.load(Ordering::Relaxed);
                        engine.throttle_dense_ratio = state.vibration_throttle_dense_ratio.load(Ordering::Relaxed);
                        engine.abs_freq_enabled = state.vibration_abs_freq_enabled.load(Ordering::Relaxed);
                        engine.abs_freq_window = state.vibration_abs_freq_window.load(Ordering::Relaxed);
                        engine.abs_freq_max = state.vibration_abs_freq_max.load(Ordering::Relaxed);
                        engine.ghost_enabled = state.vibration_rank_decay_enabled.load(Ordering::Relaxed);
                        engine.ghost_delay = state.vibration_rank_decay_delay.load(Ordering::Relaxed) as f32;
                        engine.ghost_speed = state.vibration_rank_decay_speed.load(Ordering::Relaxed) as f32;
                        engine.ghost_min = state.vibration_rank_decay_min.load(Ordering::Relaxed) as f32 / 100.0;
                        engine.ghost_max = state.vibration_rank_decay_max.load(Ordering::Relaxed) as f32 / 100.0;
                        engine.out_threshold = state.vibration_out_threshold.load(Ordering::Relaxed) as f32;
                        engine.remap_enabled = state.vibration_remap_enabled.load(Ordering::Relaxed);
                        engine.remap_min = state.vibration_remap_min.load(Ordering::Relaxed) as f32;
                        engine.split_enabled = state.vibration_split_enabled.load(Ordering::Relaxed);
                        engine.split_thr = state.vibration_split_thr.load(Ordering::Relaxed) as f32;
                        /* 职业专属算法 (v31): id 变化时重置状态 */
                        let new_algo = state.vibration_algo_id.load(Ordering::Relaxed);
                        if new_algo != engine.algo_id {
                            engine.algo_id = new_algo;
                            engine.algo_state = [0.0; 4];
                        }
                        for i in 0..4 {
                            engine.algo_ap[i] = state.vibration_algo_ap[i].load(Ordering::Relaxed) as f32;
                        }
                        /* 评分脉冲: 按各事件强度 × 满幅输出, 持续 rank_duration */
                        if now_ms() < engine.rank_full_until {
                            let lvl = engine.rank_level.clamp(0.0, 1.0);
                            /* 全局总调整[9] + 强度上限[4] 作用于评分通道 (v24.3);
                             * 独立测试开关开时评分通道独立 (单通道测试用) */
                            let (mst, cap4) = if state.vibration_independent_test.load(Ordering::Relaxed) {
                                (1.0, 1.0)
                            } else {
                                (params[P_MASTER] as f32 / 100.0,
                                 (params[P_MAX] as f32 / 100.0).min(1.0))
                            };
                            let vl = (65535.0 * lvl * engine.rank_lr_l * motor_l * mst * cap4).min(65535.0) as u16;
                            let vr = (65535.0 * lvl * engine.rank_lr_r * motor_r * mst * cap4).min(65535.0) as u16;
                            /* 评分动态衰减 (v29): 评级倍率也作用于评分通道 */
                            let vl = (vl as f32 * engine.ghost_mul).min(65535.0) as u16;
                            let vr = (vr as f32 * engine.ghost_mul).min(65535.0) as u16;
                            /* 低强度死区 (v29.3): 评分通道低强度段归 0 (消除嗡声) */
                            let thr3 = (engine.out_threshold.min(50.0) / 100.0) * 65535.0;
                            let vl = if (vl as f32) < thr3 { 0 } else { vl };
                            let vr = if (vr as f32) < thr3 { 0 } else { vr };
                            let vib = XINPUT_VIBRATION { wLeftMotorSpeed: vl, wRightMotorSpeed: vr };
                            unsafe { let _ = XInputSetState(0, &vib); }
                            state.vibration_out_l.store(vl as u32, Ordering::Relaxed);
                            state.vibration_out_r.store(vr as u32, Ordering::Relaxed);
                            /* 评分事件实时输出 (rank_out, 供 L/R 进度条) */
                            for v in state.vibration_rank_out.iter() {
                                v.store(0, Ordering::Relaxed);
                            }
                            if engine.rank_out_idx >= 0 && engine.rank_out_idx < 15 {
                                state.vibration_rank_out[(engine.rank_out_idx * 2) as usize]
                                    .store(vl as u32, Ordering::Relaxed);
                                state.vibration_rank_out[(engine.rank_out_idx * 2 + 1) as usize]
                                    .store(vr as u32, Ordering::Relaxed);
                            }
                            eprintln!("[vib] RANK pulse L={} R={} lvl={:.2}", vl, vr, lvl);
                        } else if engine.move_level > 0.0 {
                            /* 移动走路质感 v3: 正弦步伐 + 平滑 (消除嗡嗡/沙沙声)
                             * - 正弦波: 每步平滑起伏 (无方波突变)
                             * - 平滑插值: 输出向目标平滑过渡 (步伐交替不突跳)
                             * - 低强度抑制: 输出低于阈值归 0 (避免马达沙沙声)
                             * 参数 (高级震动调校 A2): 23步频 24着地 25抬脚 26增益 27平滑 28阈值
                             * 相位交替: 左脚步 L 主 / 右脚步 R 主 (走路韵律) */
                            let ml = engine.move_level.clamp(0.0, 1.0);
                            let now = now_ms();
                            let pace_ms = params[P_MOVE_PACE].max(200).min(800);
                            if now >= engine.move_pace_until {
                                engine.move_pace_until = now + pace_ms;
                                engine.move_phase = !engine.move_phase;
                            }
                            let pace_frac = if engine.move_pace_until > now {
                                1.0 - ((engine.move_pace_until - now) as f32 / pace_ms as f32)
                            } else {
                                1.0
                            };
                            let pulse_pct = (params[P_MOVE_PULSE] as f32).clamp(10.0, 100.0) / 100.0;
                            let hold_pct = (params[P_MOVE_HOLD] as f32).clamp(5.0, 50.0) / 100.0;
                            let gain_pct = (params[P_MOVE_GAIN] as f32).clamp(10.0, 100.0) / 100.0;
                            /* 全局总调整[9] + 强度上限[4] 作用于移动通道 (v24.3);
                             * 移动独立开关 (v24.5 默认开, 移动一直在走, 吃全局强度容易没震动)
                             * 或独立测试开关 → 移动通道独立 */
                            let move_indep = state.vibration_move_independent.load(Ordering::Relaxed)
                                || state.vibration_independent_test.load(Ordering::Relaxed);
                            let (mst, cap4) = if move_indep {
                                (1.0, 1.0)
                            } else {
                                (params[P_MASTER] as f32 / 100.0,
                                 (params[P_MAX] as f32 / 100.0).min(1.0))
                            };
                            /* 正弦步伐: 0→峰值→0 平滑起伏 (无突变) */
                            let wave = (pace_frac * std::f32::consts::PI * 2.0).sin() * 0.5 + 0.5;
                            let pulse = hold_pct + (pulse_pct - hold_pct) * wave;
                            let base = ml * pulse * gain_pct * mst * cap4;
                            let tl;
                            let tr;
                            if engine.move_phase {
                                tl = 65535.0 * base * engine.rank_lr_l * motor_l;
                                tr = 65535.0 * base * 0.4 * engine.rank_lr_r * motor_r;
                            } else {
                                tl = 65535.0 * base * 0.4 * engine.rank_lr_l * motor_l;
                                tr = 65535.0 * base * engine.rank_lr_r * motor_r;
                            }
                            /* 平滑插值: 每帧向目标过渡 (消除交替突跳/嗡嗡声) */
                            let smooth = (params[P_MOVE_SMOOTH] as f32).clamp(5.0, 100.0) / 100.0;
                            engine.move_out_l += (tl - engine.move_out_l) * smooth;
                            engine.move_out_r += (tr - engine.move_out_r) * smooth;
                            /* 低强度抑制: 低于阈值归 0 (消除马达沙沙声) */
                            let thr = (params[P_MOVE_THRESHOLD] as f32).clamp(0.0, 20.0) / 100.0 * 65535.0;
                            let vl = if engine.move_out_l < thr { 0 } else { engine.move_out_l.min(65535.0) as u16 };
                            let vr = if engine.move_out_r < thr { 0 } else { engine.move_out_r.min(65535.0) as u16 };
                            let vib = XINPUT_VIBRATION { wLeftMotorSpeed: vl, wRightMotorSpeed: vr };
                            unsafe { let _ = XInputSetState(0, &vib); }
                            state.vibration_out_l.store(vl as u32, Ordering::Relaxed);
                            state.vibration_out_r.store(vr as u32, Ordering::Relaxed);
                        } else {
                            let (l, r) = engine.finalize(&params);
                            /* 输出平滑 (v22.3): 一阶低通抑制低频嗡嗡声 (快速连击/衰减尾音
                             * 的输出跳变被柔化; 0=不平滑, 100=强平滑, 默认 55) */
                            let sm = state.vibration_out_smooth.load(Ordering::Relaxed).min(100) as f32 / 100.0;
                            let k = sm * 0.9;
                            let (l, r) = if k > 0.0 {
                                let lf = l as f32;
                                let rf = r as f32;
                                engine.out_smooth_l += (lf - engine.out_smooth_l) * k;
                                engine.out_smooth_r += (rf - engine.out_smooth_r) * k;
                                (engine.out_smooth_l.min(65535.0) as u16, engine.out_smooth_r.min(65535.0) as u16)
                            } else {
                                (l, r)
                            };
                            let rk = 1.0 + rnd;
                            let vl = ((l as f32) * motor_l * rk).min(65535.0) as u16;
                            let vr = ((r as f32) * motor_r * rk).min(65535.0) as u16;
                            let vib = XINPUT_VIBRATION { wLeftMotorSpeed: vl, wRightMotorSpeed: vr };
                            unsafe { let _ = XInputSetState(0, &vib); }
                            state.vibration_out_l.store(vl as u32, Ordering::Relaxed);
                            state.vibration_out_r.store(vr as u32, Ordering::Relaxed);
                        }
                    }
                } else {
                    unsafe { let _ = XInputSetState(0, &zero); }
                    engine.left = 0.0;
                    engine.right = 0.0;
                    state.vibration_out_l.store(0, Ordering::Relaxed);
                    state.vibration_out_r.store(0, Ordering::Relaxed);
                }

                std::thread::sleep(std::time::Duration::from_millis(15));
            }

            if let Some(view) = shm_view {
                let _ = unsafe { UnmapViewOfFile(view) };
            }
            if let Some(h) = shm_handle {
                let _ = unsafe { CloseHandle(h) };
            }
        })
        .ok();
}
