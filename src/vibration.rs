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
use windows::Win32::UI::Input::XboxController::{XINPUT_STATE, XINPUT_VIBRATION};

use crate::state::AppState;

const VIB_SHM_MAGIC: u32 = 0x564F4656;
/// 必须与 DLL 源码 common/vib_protocol.h 的 VIB_SHM_VERSION 一致;
/// DLL 协议升级 (事件语义/参数槽位变化) 后旧版本直接拒接, 避免按旧语义误读
const VIB_SHM_VERSION: u32 = 2;
const VIB_RING_SIZE: usize = 256 * 1024;

/* ★S1 老方案 (v13.34/35 语义移植, 仅 vib_legacy_client=true 时参与):
 * 保持式标记位 —— 老 DLL stub10 的 CC 保持震带 0x20|0x40, 据此走 mode1 hold;
 * 纯 0x20 (玩家 DOT 红字跳字) 走 mode3 衰减脉冲; 0x04 怪物 DOT 走 mode4 脉冲。 */
const FONT_OTHER: u32 = 0x40;

/* ★S1 老方案诊断: FONT 注入决策日志 (exe 同目录 SorahkDFO_vib.log)。
 * "命中到了但没有震动"时, 此日志直接给出每条 FONT 事件的注入/丢弃决策与原因。 */
static VIB_LOG_PATH: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();

fn vib_log(msg: &str) {
    let path = VIB_LOG_PATH.get_or_init(|| {
        std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.join("SorahkDFO_vib.log")))
            .unwrap_or_else(|| std::path::PathBuf::from("SorahkDFO_vib.log"))
    });
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        use std::io::Write;
        let _ = writeln!(f, "[{}] {}", now_ms(), msg);
    }
}

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

/// ★v24.12: 该事件是否计入宿主"累计事件"诊断口径 (`vibration_events_received`)。
///
/// `VEV_MOVE` 是 **6ms 续期的持续流** (走路 10 分钟 ≈ 6 万条), 计入会让总数暴增且毫无诊断价值
/// (UI 上就是"累计 N 事件"疯狂涨)。S1(legacy) 侧原本按"真实注入"过滤且排除 MOVE,
/// **S4 侧漏了 MOVE 排除** —— 本函数把口径统一为两种模式都不计移动。
#[inline]
fn counts_toward_event_total(etype: u32, legacy: bool, last_injected: bool) -> bool {
    etype != VEV_MOVE && (!legacy || last_injected)
}

#[cfg(test)]
mod event_count_tests {
    use super::*;

    #[test]
    fn move_stream_never_counts_toward_event_total() {
        // 移动流: 两种模式都不计入 (S4 曾漏排除 → 总事件暴增)
        assert!(!counts_toward_event_total(VEV_MOVE, false, true));
        assert!(!counts_toward_event_total(VEV_MOVE, true, true));
        // 普通战斗事件: 两种模式都计入
        assert!(counts_toward_event_total(VEV_FONT, false, true));
        assert!(counts_toward_event_total(VEV_RANKING, false, true));
        // S1 仍保留"只计真实注入"的老口径
        assert!(counts_toward_event_total(VEV_FONT, true, true));
        assert!(!counts_toward_event_total(VEV_FONT, true, false));
    }
}

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
/* 移动积累增强窗口 (v22): 停止移动后增强有效期 ms。
 * ★39 号槽唯一语义 (P1 结案): 原 P_DEC_EFFECT 死常量已删除 (无任何读点,
 * 曾与本病历任意混淆), GUI 无双绑定, 引擎只读 P_MOVE_BOOST_WIN */
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

/* ★S1 群怪聚合记帐 (v14.1, 修"群怪持续爆震"): 注入窗 (<30ms) 内的多次命中,
 * 手感上是一次更重的冲击 —— 窗内事件不再丢弃, 强度按通道记账, 下一发注入
 * 兑现 (能量携带): 群怪洪流因此"单击变厚"而非"变密变吵", 每一击都不丢能量。
 * ★玩家可调 (规范纪律: 高级算法必须进 GUI 可调区): merge_keep/merge_cap/
 * merge_hold 三个参数由震动页「群怪聚合」滑块组控制 (config.vibration →
 * AppState 原子 → run 循环同步), keep=0 即完全关闭回到丢弃语义。
 * 下面仅剩引擎内部常量 (不暴露): */
const FONT_CARRY_FLUSH_MIN: f32 = 0.06; /* 补发门槛 (再低会被重映射抬底放大成噪音尾巴) */

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
    /* ★S1 老方案 (补丁A): 每类飘字独立注入窗 (0=受击 1=特殊 2=DOT/状态/特效族 3=命中) */
    last_font_ch: [u32; 4],
    /* ★S1 聚合记帐 (v14.1): 注入窗内被合并事件按通道记账的能量状态
     * (s=记账强度 0-1 量纲 / lr=兑现用的 L/R 权重 / mode=末笔传送模式
     *  用于补发时选衰减常数 / until=记账到期戳) */
    font_carry_s: [f32; 4],
    font_carry_lr: [[f32; 2]; 4],
    font_carry_mode: [u8; 4],
    font_carry_until: [u32; 4],
    /* ★S1 老方案计数口径: 最近一次 push_event 是否真实注入了震动
     * (老宿主 push_event 返回 bool 的等价实现; run 循环据此只计真实震动) */
    last_injected: bool,
    /* ★S1 老方案总开关: true = S1 ACT1 老方案 (配老 DLL 事件语义), false = S4+ 新方案 */
    legacy: bool,
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
    /* ★S1 群怪聚合记帐 (v14.1): 玩家可调参数 (震动页滑块组, run 循环每轮同步)
     * keep = 每记一笔保留强度 % (0=关闭聚合, 回到丢弃语义); cap = 记账+本击
     * 强度和的封顶 % (100=老滑块满幅); hold = 记账到期补发窗口 ms (0=不补发) */
    merge_keep: u32,
    merge_cap: u32,
    merge_hold: u32,
    /* ★S1 命中限频 (v14.2): 仅普通命中通道 (ch=3) 的翻滚窗限频 —— 超额命中
     * 全额记账进聚合能量 (不丢弃), 频率上限转化为厚度。max=0 关闭。 */
    hitcap_max: u32,
    hitcap_win_ms: u32,
    hitcap_count: u32,
    hitcap_start: u32,
    /* ★S1 命中聚合窗 (v15.2): ch=3 专属更宽聚合窗 —— 群怪一刀的多条命中
     * 事件 (每怪一条) 窗内全部记账合并, 一刀只震一下 (更厚)。 */
    hitmerge_ms: u32,
    /* ★S1 持续压制 (v15.3): 命中注入持续打满限速上限 (狂战士血之狂暴等
     * 持续性双倍打击) 超过 sustain_secs 秒后, 命中震动额外 ×(1-降幅%)。
     * 实现 = sustain_secs×1000 长窗统计真实注入次数, 达到"限速上限×秒数"
     * 即判定持续满载。secs=0 关闭。sus_count/sus_start = 长窗计数器 */
    sustain_secs: u32,
    sustain_reduce: u32,
    sus_count: u32,
    sus_start: u32,
    /* ★S1 脉冲落地 (v15): 高负载期衰减尾巴提前归零 —— pct=0 关闭;
     * peak = 最近一次写入衰减通道的幅值 (inject 家族 + FLUSH 同步记录) */
    tail_land_pct: u32,
    tail_peak_l: f32,
    tail_peak_r: f32,
    /* ★S1 怪物异常反馈 (v15): 0x04 (怪物出血/中毒跳字) 的独立强度 (0-100 量纲),
     * 从"装备特效"滑块 (p[15]) 彻底拆出 —— 该通道在 ACT 里 100% 是怪物异常跳字 */
    monster_abnormal: u32,
    /* ★S1 命中风暴抽样 (v16): 极短窗内纯命中事件 (受伤类排除) 暴增 → 只保留
     * storm_keep_pct% 的震动 (抽样丢弃, 不记账不补发), 停手超过 storm_pause_ms
     * 即恢复刀刀震动。thr=0 关闭。storm_count/storm_start = 翻滚窗计数器;
     * storm_seen = 风暴期内事件序号 (确定性抽样用); storm_last_hit = 最近一条
     * 纯命中事件时间戳 (停顿检测) */
    storm_thr: u32,
    storm_keep_pct: u32,
    storm_win_ms: u32,
    storm_pause_ms: u32,
    storm_count: u32,
    storm_start: u32,
    storm_seen: u32,
    storm_active: bool,
    storm_last_hit: u32,
    /* ★v16.8: 抽样总开关 —— false 时只检测风暴 (供静音开关用), 不抽样丢弃 */
    storm_enabled: bool,
    /* ★S1 风暴静音怪物异常 (v16.6, 可选): 风暴期间 0x04 出血/中毒跳字零痕迹 */
    storm_mute_abnormal: bool,
    storm_mute_logged: bool,
    /* ★S1 风暴静音评分点系统 (v16.7, 可选): 风暴期间评分族事件零痕迹 (移动除外) */
    storm_mute_rank: bool,
    storm_mute_rank_logged: bool,
    /* ★S1 统合衰减期 (v17, 可选): 风暴期命中通道统一固定衰减 + 到点硬归零;
     * unified_until = 当前包络的硬截止时刻 (0 = 未布防) */
    storm_unified_enabled: bool,
    storm_unified_ms: u32,
    unified_until: u32,
    /* ★S1 输出引擎模式 (v20): 0 经典 / 1 S4 纯 / 2 S4+不丢 (见 config 字段注释) */
    legacy_output_mode: u32,
    /* ★模式 2 后置携带 (v20): 低于死区的输出能量暂存, 攒到阈值释放成一次厚脉冲
     * —— S4 死区会吞掉的轻反馈在此"攒厚再出", 保证不丢击 */
    shape_carry_l: f32,
    shape_carry_r: f32,
    /* ★S1 单帧脉冲 (v15.1): 衰减 0-5ms 时注入帧全幅直出、下一帧硬归零 */
    instant_armed: bool,
    /* ★优先级阶梯 (v20): 本帧被高位阶段 (统合衰减/脉冲落地/单帧脉冲) 硬归零的
     * 马达标记 —— 低位阶段 (爆发保底/输出平滑) 不得把已归零的震动抬回。
     * 每帧 tick 开头清零, 仅 legacy 分支置位; S4 恒 false = 行为逐字节不变 (红线) */
    hard_cut_l: bool,
    hard_cut_r: bool,
    /* ★总闸 L/R (v15.1): item_lr[0]/[1] 接通为左右马达独立总闸微调 (0-1 乘数) */
    gate_lr_l: f32,
    gate_lr_r: f32,
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
            last_font_ch: [0; 4],
            /* ★S1 聚合记帐 (v14.1): 注入窗内被合并事件按通道记账的能量状态 */
            font_carry_s: [0.0; 4],
            font_carry_lr: [[0.0; 2]; 4],
            font_carry_mode: [0u8; 4],
            font_carry_until: [0; 4],
            last_injected: false,
            legacy: false,
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
            /* 群怪聚合默认参数 (与 config.vibration serde 默认一致; run 循环每轮
             * 从 AppState 原子同步覆盖) */
            merge_keep: 80,
            merge_cap: 100,
            merge_hold: 80,
            /* 命中限频默认: 1 秒最多 6 次 (run 循环每轮同步覆盖) */
            hitcap_max: 6,
            hitcap_win_ms: 1000,
            hitcap_count: 0,
            hitcap_start: 0,
            hitmerge_ms: 60,
            /* 持续压制默认: 打满限速 3 秒后再降 30% (run 循环同步覆盖) */
            sustain_secs: 3,
            sustain_reduce: 30,
            sus_count: 0,
            sus_start: 0,
            /* 脉冲落地: 引擎默认关闭 (config/state 默认 25), run 循环同步覆盖 */
            tail_land_pct: 0,
            tail_peak_l: 0.0,
            tail_peak_r: 0.0,
            /* 怪物异常反馈默认 1% (run 循环同步覆盖) */
            monster_abnormal: 1,
            /* 命中风暴默认值与 config 一致 (run 循环同步覆盖) */
            storm_thr: 10,
            storm_keep_pct: 50,
            storm_win_ms: 1000,
            storm_pause_ms: 400,
            storm_count: 0,
            storm_start: 0,
            storm_seen: 0,
            storm_active: false,
            storm_last_hit: 0,
            storm_enabled: true,
            storm_mute_abnormal: false,
            storm_mute_logged: false,
            storm_mute_rank: false,
            storm_mute_rank_logged: false,
            storm_unified_enabled: false,
            storm_unified_ms: 120,
            unified_until: 0,
            legacy_output_mode: 0,
            shape_carry_l: 0.0,
            shape_carry_r: 0.0,
            instant_armed: false,
            hard_cut_l: false,
            hard_cut_r: false,
            gate_lr_l: 1.0,
            gate_lr_r: 1.0,
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
        /* ★脉冲落地 (v15): 记录本次注入峰值, 供高负载期尾巴归零判据 */
        self.tail_peak_l = self.left;
        self.tail_peak_r = self.right;
        self.decay_l = dl;
        self.decay_r = dr;
        /* ★单帧脉冲 (v15.1): 衰减 ≤5ms 时布防, tick 保持本帧全幅、下一帧归零 */
        self.instant_armed = dl <= 5.0;
        /* ★S1 老方案 (v13.34 合成式): 不清除 hold/rhythm —— 三通道共存互不覆盖;
         * 新方案维持现行抢占语义 */
        if !self.legacy {
            self.hold_until = 0;
            self.rhythm_until = 0;
        }
    }

    fn inject_hold(&mut self, s: f32, lr: f32, rr: f32, dur: u32) {
        self.hold_l = s * lr;
        self.hold_r = s * rr;
        self.left = self.hold_l;
        self.right = self.hold_r;
        /* ★脉冲落地: hold 注入也写衰减通道, 残尾按自身峰值落地 */
        self.tail_peak_l = self.left;
        self.tail_peak_r = self.right;
        self.hold_until = now_ms().wrapping_add(dur);
        /* ★S1 老方案: 不清除 rhythm_until (合成式共存) */
        if !self.legacy {
            self.rhythm_until = 0;
        }
    }

    /* 评分脉冲注入: 取最强/已过期才覆盖, 避免多事件互相顶掉 */
    fn inject_rank(&mut self, s: f32, dur: u32) {
        if s <= 0.0 {
            return;
        }
        let now = now_ms();
        if s >= self.rank_level || !before(now, self.rank_full_until) {
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
                /* ★0 哨兵 (规范 §7.9): state[0]=0 = 未初始化, 直接开窗 */
                if self.algo_state[0] == 0.0
                    || now.wrapping_sub(self.algo_state[0] as u32) > self.algo_ap[0] as u32
                {
                    self.algo_state[1] = 0.0;
                    self.algo_state[0] = now as f32;
                }
                self.algo_state[1] += count as f32;
                if self.algo_state[1] >= self.algo_ap[1] {
                    self.inject(self.algo_ap[2] / 100.0, 0.9, 0.7, 60.0, 45.0);
                    self.silence_until = now.wrapping_add(self.algo_ap[3] as u32);
                    self.algo_state[1] = 0.0;
                }
            }
            /* M 连点组: 窗口密集命中 → 弹幕节奏组 */
            16 | 18 => {
                /* ★0 哨兵 (规范 §7.9): state[0]=0 = 未初始化, 直接开窗 */
                if self.algo_state[0] == 0.0
                    || now.wrapping_sub(self.algo_state[0] as u32) > self.algo_ap[0] as u32
                {
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
                    /* ★0 哨兵 (规范 §7.9): state[0]=0 = 未初始化, 直接开窗 */
                    if self.algo_state[0] == 0.0
                        || now.wrapping_sub(self.algo_state[0] as u32) > self.algo_ap[0] as u32
                    {
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
        /* ★脉冲落地: rhythm 注入也写衰减通道, 残尾按自身峰值落地 */
        self.tail_peak_l = self.left;
        self.tail_peak_r = self.right;
        self.rhythm_until = now_ms().wrapping_add(dur);
        self.rhythm_period = period.max(30);
        /* ★S1 老方案: 不清除 hold_until (合成式共存) */
        if !self.legacy {
            self.hold_until = 0;
        }
    }

    /* ★S1 聚合记帐 (v14.1): FONT 强度/权重/模式选择 —— 注入主路优先级链的
     * 纯查询版 (原链内联在 push_event, 现两处共用此单一事实来源):
     * 主路注入用返回值注入; 窗内记账用 s 估计能量、mode 供补发选衰减。 */
    fn font_pick(&self, a6: u32, item_idx: usize) -> (f32, f32, f32, u8) {
        let is_hit = a6 & FONT_HIT != 0;
        let is_dot = a6 & FONT_HP != 0;
        let is_state = a6 & FONT_STATE != 0;
        let is_effect = a6 & FONT_EFFECT != 0;
        let (base_l, base_r) = base_lr_for(item_idx);
        /* ★S1 老方案 (v13.34/35 通道语义拆分, 修"通道冲突互相覆盖"):
         * 0x60 (0x20|0x40 保持式标记, 老 DLL stub10 的 CC tick) → mode1 hold;
         * 纯 0x20 (玩家 DOT 红字跳字, 每红字一条) → mode3 衰减脉冲;
         * 0x04 (怪物出血/中毒跳字) → mode4 衰减脉冲 (节拍式被 <120ms 群怪
         * 出血连续刷新 = 永久节拍震)。新方案维持现行优先级链不动。 */
        if self.legacy && is_dot && a6 & FONT_OTHER != 0 {
            (self.p[P_FONT_HP], base_l, base_r, 1)
        } else if self.legacy && is_dot {
            (self.p[P_FONT_HP], base_l, base_r, 3)
        } else if self.legacy && is_effect {
            /* ★怪物异常反馈 (v15): 0x04 在 ACT 里 100% 是怪物出血/中毒跳字
             * (DLL 取证: hooks_old.c 怪物侧 DOT 唯一出口; 装备特效从不产生伤害
             * 数字), 用独立强度滑块, 彻底与"装备特效" (p[15], S4 专用) 拆分 */
            (self.monster_abnormal as f32, base_l, base_r, 4)
        } else if is_dot {
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
        }
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
        /* ★S1 老方案计数口径: 入口清零, 真实注入的路径置位 (run 循环只计 true) */
        self.last_injected = false;
        let now = now_ms();
        self.last_event = now;

        /* ★S1 风暴静音评分点系统 (v16.7, 可选开关): 命中风暴期间评分族事件零痕迹
         * 静音 —— 评分等级脉冲 (VEV_RANKING) + 细分事件 (评分点/闪避/暴击/破招/
         * 背击/最终击杀/凌空追击/第一击/增益叠加/释放技能/镜头震动/技能震动/
         * 暴击特写) + 怪物死亡 (VEV_TARGET_DIE)。移动持续震动 (VEV_MOVE) 不属于
         * 评分点系统, 不受影响。恢复与怪物异常静音同源: 停手超过 storm_pause_ms
         * 的时间窗判定, 无需等下一发命中。默认关 = 旧行为。 */
        if self.legacy
            && self.storm_mute_rank
            && self.storm_active
            && now.wrapping_sub(self.storm_last_hit) <= self.storm_pause_ms.max(50)
            && (ev.etype == VEV_TARGET_DIE
                || ev.etype == VEV_RANKING
                || (ev.etype >= VEV_KILLPOINT
                    && ev.etype <= VEV_CRIT_SHAKE
                    && ev.etype != VEV_MOVE))
        {
            if !self.storm_mute_rank_logged {
                self.storm_mute_rank_logged = true;
                vib_log("[STORM] 风暴期静音评分点系统");
            }
            return;
        }

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
            /* ★S1 老方案: 极小强度不碰评分窗口 (零输出占位会压掉战斗震动) */
            if self.legacy && s <= 0.001 {
                return;
            }
            self.inject_rank(s, rank_duration.max(20));
            self.last_injected = true;
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
                    self.burst_until = now.wrapping_add(800);
                }

                /* 特殊攻击静默: 静默期内普通事件不注入(突显特殊攻击)
                 * ★0 哨兵守卫 (v14.1): silence_until=0 = 从未静默; 裸 before(now,0)
                 * 在 GetTickCount 跨 2^31 (开机 24.8 天) 后回绕成"未来 21 亿 ms",
                 * 普通攻击会被静默门全部吞掉 (只有技能震) —— 本机 25 天uptime 实测复现 */
                if !abs_mode
                    && self.silence_until != 0
                    && before(now, self.silence_until)
                    && (a6 & FONT_SPECIAL) == 0
                {
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

            /* ★S1 风暴静音怪物异常 (v16.6, 可选开关): 命中风暴期间把 0x04
             * 怪物出血/中毒跳字反馈暂时关掉 —— 零痕迹丢弃 (不打锚/不计密度/
             * 不进记账); 风暴结束 (停手超过 storm_pause_ms, 用时间窗判定, 无需
             * 等下一发命中) 自动恢复。仅 S1, 仅纯 0x04 (玩家 DOT 0x20/状态 0x08
             * 不受影响)。默认关 = 旧行为。 */
            if self.legacy
                && self.storm_mute_abnormal
                && self.storm_active
                && is_effect
                && !is_dot
                && now.wrapping_sub(self.storm_last_hit) <= self.storm_pause_ms.max(50)
            {
                if !self.storm_mute_logged {
                    self.storm_mute_logged = true;
                    vib_log("[STORM] 风暴期静音怪物异常反馈");
                }
                return;
            }
            /* ★S1 聚合记帐 (v14.1): DOT/状态/特效事件也计入密度统计 ——
             * v22.4 原版只统计攻击命中, 群怪的 DOT 洪流 (每怪每跳一条)
             * 对密度自适应完全不可见 = 压制失效盲区 */
            if self.legacy && !is_attack && (is_dot || is_state || is_effect) {
                /* ★v16.1: 0 强度事件不参与密度统计 —— 怪物异常反馈=0 时出血洪流
                 * 不得再触发密度压制 (调 0 = 该事件流对引擎零影响); DOT/状态
                 * 滑块为 0 时同理。font_pick 返回的强度即注入强度 (单一事实来源) */
                if self.font_pick(a6, item_idx).0 > 0.0 {
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
                }
            }

            if is_hit {
                let cw = self.p[P_COUNTER_WIN].max(300.0) as u32;
                self.counter_until = now.wrapping_add(cw);
                /* 评分动态衰减 (v29): 受击/命中事件也续评级 (保持节奏) */
                if self.ghost_enabled {
                    self.ghost_grade = (self.ghost_grade + 0.5).min(8.0);
                    self.ghost_last_hit = now;
                }
                /* 职业专属算法 (v31): 受击钩子 */
                self.algo_on_hit(now);
            }

            if !font_hits {
                if self.legacy {
                    vib_log(&format!("[DROP] font_hits=off a6={:X}", a6));
                }
                return;
            }
            /* ★S1 命中风暴抽样 (v16, 玩家可调): 极短窗内纯命中事件暴增 → 只保留
             * 一部分震动 (用户定稿算法)。要点:
             * - 只统计/只丢弃"纯命中"事件 (0x01): 受击 0x02 (受伤类)、DOT/状态/
             *   特效/特殊攻击全部排除在外, 完全不受风暴影响;
             * - 风暴期确定性抽样 (每 N 条保留 1 条): 丢弃 = 整条跳过, 不打 IVL
             *   锚点/不进聚合记账/不占限频名额 —— 与 merge/hitcap 的"记账不丢击"
             *   互补 (风暴期宁可少震, 不要节拍堆叠);
             * - 恢复: 距上一条纯命中事件超过 storm_pause_ms (短暂停顿) → 立即
             *   退出风暴, 恢复刀刀震动;
             * - thr=0 关闭; 新方案 (!legacy) 不进此分支 (红线)。 */
            if self.legacy
                && self.storm_thr > 0
                && a6 & FONT_ATTACK != 0
                && a6 & (FONT_HIT | FONT_SPECIAL | FONT_HP | FONT_STATE | FONT_EFFECT) == 0
            {
                let pause_gap = if self.storm_last_hit != 0 {
                    now.wrapping_sub(self.storm_last_hit)
                } else {
                    0
                };
                self.storm_last_hit = now;
                let win = self.storm_win_ms.max(100);
                if self.storm_start == 0 || now.wrapping_sub(self.storm_start) >= win {
                    self.storm_count = 0;
                    self.storm_start = now;
                }
                self.storm_count = self.storm_count.saturating_add(1);
                /* 停顿恢复: 两条命中之间隔太久 → 风暴退出, 本发必震 */
                if self.storm_active && pause_gap > self.storm_pause_ms.max(50) {
                    self.storm_active = false;
                    self.storm_seen = 0;
                    self.storm_count = 1;
                    self.storm_start = now;
                    self.storm_mute_logged = false;
                    self.storm_mute_rank_logged = false;
                    vib_log("[STORM] 停顿恢复 -> 刀刀震");
                }
                if self.storm_count >= self.storm_thr {
                    if !self.storm_active {
                        self.storm_active = true;
                        self.storm_seen = 0;
                        self.storm_mute_logged = false;
                        self.storm_mute_rank_logged = false;
                        vib_log(&format!(
                            "[STORM] 命中风暴开启 (窗{}ms 内 {} 条)",
                            win, self.storm_count
                        ));
                    }
                    /* ★v16.8: 抽样受总开关控制; 关掉时只保留检测 (静音开关继续可用),
                     * 事件照常走后续注入链 (不丢弃)。★v17: 统合衰减期开启时同样不抽样
                     * (每一击都重新起振, 由统一包络保证干净) */
                    if self.storm_enabled && !self.storm_unified_enabled {
                        self.storm_seen = self.storm_seen.saturating_add(1);
                        let keep = self.storm_keep_pct.clamp(1, 100) as u32;
                        let stride = (100 / keep).max(1);
                        /* 确定性抽样: 进入风暴后的第 1 条保留, 之后每 stride 条保留 1 条 */
                        if stride > 1 && (self.storm_seen - 1) % stride != 0 {
                            return;
                        }
                    }
                }
            }
            /* ★S1 老方案 (补丁A): 每类飘字独立注入窗 (0=受击 1=特殊 2=DOT/状态/
             * 特效族 3=命中), 受击不再被命中/飘字流的全局窗吞掉; 新方案维持全局单窗 */
            let (gap_prev, font_ch) = if self.legacy {
                let ch: usize = if is_hit {
                    0
                } else if a6 & FONT_SPECIAL != 0 {
                    1
                } else if is_dot || is_state || is_effect {
                    2
                } else {
                    3
                };
                let last_ch = self.last_font_ch[ch];
                let gap = if last_ch != 0 {
                    now.wrapping_sub(last_ch)
                } else {
                    u32::MAX
                };
                (gap, Some(ch))
            } else if self.last_font != 0 {
                (now.wrapping_sub(self.last_font), None)
            } else {
                (u32::MAX, None)
            };
            /* 注入间隔: 密度激活期强制翻倍 (v22.4: 高连击窗口内进一步拉开注入间隔,
             * 配合短衰减让输出有明显间歇, 杜绝持续嗡鸣)
             * v26: 绝对频率模式下不检查间隔 */
            let mut ivl = self.p[P_FONT_IVL] as u32;
            /* ★S1 老方案 (v13.35): 注入窗 20ms —— 40ms 窗会吞掉 10-39ms 的极速
             * 连段 (攻击震动丢失的根因, 老项目格斗家实测定案) */
            if self.legacy {
                ivl = ivl.min(20);
            }
            if self.density_active {
                ivl = ivl.saturating_mul(2);
            }
            /* ★S1 命中聚合窗 (v15.2, 玩家可调): 普通命中通道 (ch=3) 专属更宽
             * 聚合窗 —— 群怪一刀的多条命中事件 (每怪一条, 错峰到达) 窗内全部
             * 记账合并, 一刀只震一下 (更厚), 能量不丢; 单挑慢速攻击不受影响。 */
            if self.legacy && font_ch == Some(3) && self.hitmerge_ms > ivl {
                ivl = self.hitmerge_ms.min(200);
            }
            /* ★v17 统合衰减期: 风暴期普通命中通道走"统一包络" —— 每一击都直接
             * 重新起振 (旧包络瞬间消亡), 绕过注入间隔门与聚合/限频记账 */
            let unified = self.legacy
                && self.storm_unified_enabled
                && self.storm_active
                && font_ch == Some(3);
            if !abs_mode && gap_prev < ivl && !unified {
                if self.legacy {
                    /* ★S1 群怪聚合记帐 (v14.1, 玩家可调): 窗内事件不再丢弃 ——
                     * 强度按通道记账 (能量携带), 下一发注入兑现; 记账期内无后续
                     * 注入由 tick 到期补发收尾脉冲。感知依据: <30ms 的两次冲击
                     * 手感为一次更重的冲击, 群怪洪流因此"单击变厚"而非"变密变吵"。
                     * merge_keep=0 = 关闭聚合 → 回到丢弃语义 (旧行为)。 */
                    if self.merge_keep > 0 {
                        if let Some(ch) = font_ch {
                            let (s_e, lr_e, rr_e, mode_e) = self.font_pick(a6, item_idx);
                            if s_e > 0.0 {
                                let c = self.font_carry_s[ch];
                                let cap = (self.merge_cap.max(10) as f32 / 100.0).min(1.5);
                                self.font_carry_s[ch] =
                                    (c + s_e / 100.0 * self.merge_keep.min(100) as f32 / 100.0)
                                        .min(cap);
                                self.font_carry_lr[ch] = [lr_e * il, rr_e * ir];
                                self.font_carry_mode[ch] = mode_e;
                                self.font_carry_until[ch] =
                                    now.wrapping_add(self.merge_hold.min(500).max(1));
                            }
                        }
                    } else {
                        vib_log(&format!(
                            "[DROP] ivl ch={} gap={} ivl={} a6={:X}",
                            font_ch.map_or(0, |c| c),
                            gap_prev,
                            ivl,
                            a6
                        ));
                    }
                }
                return;
            }
            /* ★S1 命中限频 (v14.2, 玩家可调): 仅普通命中通道 (ch=3) —— 窗口秒内
             * 命中脉冲超过 hitcap_max 后, 超额命中不再单独注入而是**全额记账**进
             * 聚合能量 (下一发兑现变厚 / 到期补发收尾), 每一击都不丢; 其他通道
             * (受击/特殊/DOT/状态/特效) 完全不受影响。max=0 关闭。
             * 位置在打戳之前: 超额命中不推进 IVL 锚点, 防止"通道饥饿"。 */
            if !abs_mode
                && self.hitcap_max > 0
                && self.legacy
                && font_ch == Some(3)
                && !unified
            {
                if self.hitcap_start == 0
                    || now.wrapping_sub(self.hitcap_start) >= self.hitcap_win_ms.max(50)
                {
                    self.hitcap_count = 0;
                    self.hitcap_start = now;
                }
                if self.hitcap_count >= self.hitcap_max {
                    let (s_e, lr_e, rr_e, mode_e) = self.font_pick(a6, item_idx);
                    if s_e > 0.0 {
                        let cap = (self.merge_cap.max(10) as f32 / 100.0).min(1.5);
                        let c = self.font_carry_s[3];
                        self.font_carry_s[3] = (c + s_e / 100.0).min(cap);
                        self.font_carry_lr[3] = [lr_e * il, rr_e * ir];
                        self.font_carry_mode[3] = mode_e;
                        self.font_carry_until[3] =
                            now.wrapping_add(self.merge_hold.min(500).max(1));
                        vib_log(&format!(
                            "[HITCAP] 超额记账 carry={:.2} a6={:X}",
                            self.font_carry_s[3], a6
                        ));
                    }
                    return;
                }
            }
            /* ★S1 老方案 (v13.35 语义): 过了间隔门才打通道戳 —— 先打戳会让被丢
             * 事件把窗口永远前推 = 通道饥饿 (只有第一条震)。
             * ★v16.1: legacy 分通道戳后移到 s>0 门之后 (见 font_pick 处), 0 强度
             * 事件不再刷新注入窗; 新方案维持在此打全局戳 (红线, 逐字节不动)。 */
            if font_ch.is_none() {
                self.last_font = now;
            }

            /* 震动节流 (v24): 时间窗口内只允许 N 次注入, 超出直接跳过
             * (召唤师等瞬间百连击职业防狂震: 窗口滚动)
             * v24.1 自适应: 密度激活(狂震期)时次数上限按比例收紧 (如 20% → 1次/s),
             * 平时不限制太死 (正常反馈)
             * v26: 绝对频率模式下失效 (由 abs 节流替代)
             * ★S1 聚合记帐 (v14.1): 老方案绕过硬限流 —— 限流=丢事件产生空震
             * (用户明确不要), 聚合记帐已在不丢能量的前提下压住注入密度 */
            if !abs_mode && !self.legacy && self.throttle_window > 0 && self.throttle_max > 0 {
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
            /* ★0 哨兵守卫 (v14.1): counter_until=0 = 无受击缓冲; 跨 2^31 后
             * 裸 before(now,0) 恒真 → 每次攻击都误入 Counter 态吃计数乘数 */
            if is_attack && self.counter_until != 0 && before(now, self.counter_until) {
                self.state = VibState::Counter;
                mul = self.p[P_COUNTER_MUL].max(100.0) / 100.0;
            }

            /* 强度/权重/模式选择 (优先级链已提取为 font_pick, 与窗内记账共用) */
            let (s, lr, rr, mode): (f32, f32, f32, u8) = self.font_pick(a6, item_idx);
            if s <= 0.0 {
                if self.legacy {
                    vib_log(&format!("[DROP] s=0 a6={:X}", a6));
                }
                return;
            }
            /* ★v16.1: legacy 分通道戳 (v13.35 语义, 过了间隔门+强度门才打戳) ——
             * 0 强度事件 (如怪物异常=0 时的出血洪流) 不再刷新注入窗, 否则紧随
             * 其后的真实 DOT/状态/特效会被挤进记账补发 (FLUSH 节拍感)。
             * 新方案打全局戳原位不动 (见间隔门处, 红线)。 */
            if let Some(ch) = font_ch {
                self.last_font_ch[ch] = now;
            }

            let mut s_eff = s;
            /* ★合并降幅 (v20, 仅 S1): 命中自适应与持续压制原本各自相乘叠加
             * (= 过压/不可预测), S1 改为取两者中更强的降幅只应用一次 (见下方
             * k_cool)。S4 维持逐项相乘, 行为逐字节不变 (红线)。 */
            let mut k_adapt = 1.0f32;
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
                if self.legacy && self.legacy_output_mode != 0 {
                    k_adapt = k.max(floor);
                } else {
                    s_eff = s * k.max(floor);
                }
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
            /* ★S1 老方案 (v13.30 FONT 量纲归一): FONT 链原以滑块 0-100 原值注入
             * (非 FONT 通道均为 0-1 量纲) → finalize l_raw 恒 ≥1 被 min(1.0) 钳成
             * 满幅 = 强度上限/总调失效、滑块无梯度。统一 0-1 量纲后滑块全程线性;
             * 必须配 finalize 的重映射前置保底, 轻反馈才不静音。新方案维持现行量纲。 */
            if self.legacy {
                s_out /= 100.0;
            }
            /* ★S1 群怪聚合记帐 (v14.1, 玩家可调): 兑现通道记账能量 —— 群怪窗内
             * 合并的命中让这一发更厚; 封顶 (merge_cap 滑块) 随密度疲劳下调
             * (高密度期脉冲变厚受限于 density_scale, 与 ivl 翻倍共同把洪流压成
             * "变厚变疏"而非爆震)。新方案 font_ch 恒为 None, 此块天然不参与 (红线)。 */
            if let Some(ch) = font_ch {
                let c = self.font_carry_s[ch];
                if c > 0.0 {
                    let cap = (self.merge_cap.max(10) as f32 / 100.0).min(1.5);
                    s_out = (s_out + c).min(cap * self.density_scale);
                    self.font_carry_s[ch] = 0.0;
                    self.font_carry_until[ch] = 0;
                    vib_log(&format!("[CARRY] ch={} carry={:.2} s_out={:.3}", ch, c, s_out));
                }
            }
            /* ★S1 持续压制 (v15.3): 长窗统计真实命中注入次数 —— 持续打满限速
             * 上限 (限速上限 × 窗口秒数) 超时后, 命中震动自动再降一档 (狂战士
             * 血之狂暴等持续性双倍打击的降温; 降速/停手即自动恢复, 单挑不触发) */
            let mut k_sustain = 1.0f32;
            if self.legacy && font_ch == Some(3) && self.sustain_secs > 0 {
                let sus_win = (self.sustain_secs * 1000).max(500);
                if self.sus_start == 0 || now.wrapping_sub(self.sus_start) >= sus_win {
                    self.sus_count = 0;
                    self.sus_start = now;
                }
                self.sus_count = self.sus_count.saturating_add(1);
                let sus_thr = ((self.hitcap_max as f32) * sus_win as f32
                    / (self.hitcap_win_ms.max(1) as f32))
                    .ceil()
                    .max(1.0);
                if self.sus_count as f32 >= sus_thr {
                    let red = 1.0 - (self.sustain_reduce.min(80) as f32 / 100.0);
                    if self.legacy && self.legacy_output_mode != 0 {
                        k_sustain = red;
                    } else {
                        s_out *= red;
                    }
                }
            }
            /* ★合并降幅 (v20, 仅 S1 的 S4 模式 1/2): 自适应/持续压制取强者应用一次;
             * 经典模式 0 维持 v19.7 逐项相乘 (ACT1 特调手感基线, 勿动) */
            if self.legacy && self.legacy_output_mode != 0 {
                s_out *= k_adapt.min(k_sustain);
            }
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
                /* ★S1 老方案 mode3: 玩家 DOT 衰减脉冲(每红字一震) —— 衰减常数取
                 * P_DOT_HOLD(150ms), LR 权重沿用命中槽(item_lr[3]) */
                3 => {
                    let dl = self.p[P_DOT_HOLD].max(30.0);
                    self.inject(s_out, lr_w, rr_w, dl, dl * 0.8);
                }
                /* ★S1 老方案 mode4: 怪物 DOT 衰减脉冲(每红字一震), 衰减取
                 * P_EFFECT_PERIOD(90ms), LR 沿用特效槽(item_lr[8]) */
                4 => {
                    let dl = self.p[P_EFFECT_PERIOD].max(60.0);
                    self.inject(s_out, lr_w, rr_w, dl, dl * 0.8);
                }
                _ => {
                    /* ★v17 统合衰减期 (风暴期): 旧的包络瞬间消亡 → 以本次命中强度
                     * 重新起振 → 统一固定衰减周期 → 到点由 tick 硬归零。
                     * 连续命中因此变成"一击接一击"的干净持续震动, 旧尾巴不叠糊。 */
                    if unified {
                        let ms = self.storm_unified_ms.clamp(30, 1000) as f32;
                        self.left = 0.0;
                        self.right = 0.0;
                        self.decay_l = 0.0;
                        self.decay_r = 0.0;
                        self.unified_until = now.wrapping_add(ms as u32);
                        self.inject(s_out, lr_w, rr_w, ms, ms * 0.8);
                    } else {
                    /* ★v15.1: 衰减常数放开到 0 —— 0-5ms 由 tick 的"单帧脉冲"模式
                     * 承载 (注入帧全幅直出, 下一帧硬归零), >5ms 走指数衰减 */
                    let (dl, dr) = if is_hit {
                        (self.p[P_DEC_HIT].max(0.0), self.p[P_DEC_HIT].max(0.0) * 0.8)
                    } else if a6 & FONT_SPECIAL != 0 {
                        (self.p[P_DEC_SPECIAL].max(0.0), self.p[P_DEC_SPECIAL].max(0.0) * 0.7)
                    } else if is_state {
                        (self.p[P_DEC_STATE].max(0.0), self.p[P_DEC_STATE].max(0.0) * 0.8)
                    } else {
                        /* 职业专属算法 (v31): 元素叠层加长命中衰减 */
                        let extra = self.algo_dec_attack_extra();
                        (self.p[P_DEC_ATTACK].max(0.0) + extra, self.p[P_DEC_ATTACK].max(0.0) * 0.8 + extra)
                    };
                    /* 衰减时长也应用该项 L/R 权重 (正=更持久, 负=更短) */
                    let dl_w = dl * (1.0 + item_lr[3 * 2] as i32 as f32 / 100.0).clamp(0.0, 2.0);
                    let dr_w = dr * (1.0 + item_lr[3 * 2 + 1] as i32 as f32 / 100.0).clamp(0.0, 2.0);
                    self.inject(s_out, lr_w, rr_w, dl_w, dr_w);
                    }
                }
            }

            self.last_injected = true; /* FONT 注入完成 → 确实震了 */
            /* ★命中限频计数: 仅 legacy 命中通道真实注入 +1 (s=0 空枪不占名额;
             * 超额记账的事件在上方已 return, 不会到达这里); max=0 完全休眠 */
            if self.legacy && self.hitcap_max > 0 && font_ch == Some(3) {
                self.hitcap_count = self.hitcap_count.saturating_add(1);
            }
            if self.legacy {
                vib_log(&format!(
                    "[INJ] a6={:X} mode={} s_out={:.3} lr_w={:.3} rr_w={:.3}",
                    a6, mode, s_out, lr_w, rr_w
                ));
            }
            if is_dot {
                self.dot_active = true;
                self.dot_last = now;
            }
            if a6 & FONT_SPECIAL != 0 {
                /* 特殊攻击静默窗口 */
                self.silence_until = now.wrapping_add(self.p[P_SILENCE].max(0.0) as u32);
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
            /* ★S1 老方案: 极小强度不碰评分窗口 */
            if self.legacy && s <= 0.001 {
                return;
            }
            self.inject_rank(s, rank_duration.max(20));
            self.last_injected = true;
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
                    /* ★S1 老方案 (v13.38 诊断): 移动事件决策落盘 (城镇移动
                     * "事件到但不震"排查用); 新方案不写 (移动事件是洪流) */
                    if self.legacy {
                        vib_log(&format!(
                            "[MV] str={} rank11={} s={:.3} -> {}",
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

    fn tick(&mut self, params: &[u32; 60]) {
        let now = now_ms();
        /* ★优先级阶梯: 本帧硬归零标记清零 (仅下方 legacy 分支会重新置位) */
        self.hard_cut_l = false;
        self.hard_cut_r = false;
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
        if self.state == VibState::Burst && !before(now, self.burst_until) {
            self.state = VibState::Combat;
        }
        if self.state == VibState::Counter && !before(now, self.counter_until) {
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
        if self.legacy {
            /* ★S1 群怪聚合记帐 (v14.1, 玩家可调): 记账到期仍无后续注入 → 补发
             * 收尾脉冲 (防能量蒸发; 用 max 合成, 绝不压低已有输出)。
             * merge_hold=0 = 不补发 (记账等下一发兑现, 到期作废)。 */
            for ch in 0..4 {
                if self.merge_hold > 0
                    && self.font_carry_s[ch] >= FONT_CARRY_FLUSH_MIN
                    && self.font_carry_until[ch] != 0
                    && !before(now, self.font_carry_until[ch])
                {
                    let cap = (self.merge_cap.max(10) as f32 / 100.0).min(1.5);
                    let s_c = self.font_carry_s[ch].min(cap * self.density_scale);
                    let (lr_c, rr_c) = (self.font_carry_lr[ch][0], self.font_carry_lr[ch][1]);
                    let (dl, dr) = match self.font_carry_mode[ch] {
                        /* mode1/3: CC 保持/玩家 DOT → P_DOT_HOLD; mode4: 怪物 DOT → P_EFFECT_PERIOD;
                         * 其余 (命中/特殊/状态) → P_DEC_ATTACK, 与注入主路的衰减常数一致 */
                        1 | 3 => (self.p[P_DOT_HOLD].max(30.0), self.p[P_DOT_HOLD].max(30.0) * 0.8),
                        4 => (
                            self.p[P_EFFECT_PERIOD].max(60.0),
                            self.p[P_EFFECT_PERIOD].max(60.0) * 0.8,
                        ),
                        _ => (
                            self.p[P_DEC_ATTACK].max(0.0),
                            self.p[P_DEC_ATTACK].max(0.0) * 0.8,
                        ),
                    };
                    self.font_carry_s[ch] = 0.0;
                    self.font_carry_until[ch] = 0;
                    self.left = self.left.max(s_c * lr_c);
                    self.right = self.right.max(s_c * rr_c);
                    /* ★脉冲落地: 补发脉冲必须刷新峰值 —— 否则陈旧大 peak 会把
                     * 小收尾脉冲在落地门当帧误杀 (防能量蒸发失效) */
                    self.tail_peak_l = self.left;
                    self.tail_peak_r = self.right;
                    self.decay_l = dl;
                    self.decay_r = dr;
                    self.instant_armed = dl <= 5.0;
                    vib_log(&format!("[FLUSH] ch={} s={:.3}", ch, s_c));
                }
            }
            /* ★S1 老方案 (v13.34 合成式, 修"通道冲突互相覆盖"核心):
             * 旧逻辑三套状态机(hold/rhythm/decay)抢占同一个 left/right:
             *   hold 激活期每帧强制覆盖 → 命中注入一帧内被抹掉;
             *   hold/rhythm 结束帧直接清零 → 砍掉其他通道的衰减尾巴;
             *   inject() 又清 hold/rhythm → 命中打断持续震。
             * 老方案: 三分量独立推进, 输出取 max 合成 —— 保持震/节拍/衰减脉冲
             * 共存, 互不覆盖; 各状态自然过期, 结束帧不再清零。 */
            let mut l_acc: f32 = 0.0;
            let mut r_acc: f32 = 0.0;
            /* 1) 保持分量 (CC 保持震: 每帧 tick 经注入窗续期)
             * ★0 哨兵守卫 (v15): hold_until=0=未激活; 裸 before(now,0) 在
             * GetTickCount 跨 2^31 后恒真 → S4 分支会用 hold_l=0 覆盖输出
             * (长开机机器上 S4 FONT 全静默), legacy 也会空走 hold 分支 */
            if self.hold_until != 0 && before(now, self.hold_until) {
                l_acc = l_acc.max(self.hold_l);
                r_acc = r_acc.max(self.hold_r);
            } else {
                self.hold_until = 0;
            }
            /* 2) 节拍分量 (怪物 DOT 跳字: 0x04 → 120ms on/off 节拍; 同上 0 哨兵守卫) */
            if self.rhythm_until != 0 && before(now, self.rhythm_until) {
                let on = ((now / self.rhythm_period) & 1) == 0;
                let rl = if on { self.rhythm_l } else { self.rhythm_l * 0.15 };
                let rr2 = if on { self.rhythm_r } else { self.rhythm_r * 0.15 };
                l_acc = l_acc.max(rl);
                r_acc = r_acc.max(rr2);
            } else {
                self.rhythm_until = 0;
            }
            /* 3) 衰减分量 (命中/暴击/受击/DOT 脉冲: 指数衰减始终推进)
             * ★单帧脉冲模式 (v15.1, 衰减 0-5ms): 注入/补发帧全幅直出 (armed),
             * 下一帧硬归零 —— "命中之后立刻衰减"的最极端形态, 一顿一顿。 */
            if self.decay_l <= 5.0 {
                if self.instant_armed {
                    self.instant_armed = false; /* 注入/补发帧: 保持全幅直出 */
                } else {
                    self.left = 0.0;
                    self.right = 0.0;
                    /* 高位阶段硬归零: 标记, 禁止低位阶段抬回 */
                    self.hard_cut_l = true;
                    self.hard_cut_r = true;
                }
            } else {
                let dl = (-(dt) / self.decay_l.max(10.0)).exp();
                let dr = (-(dt) / self.decay_r.max(10.0)).exp();
                self.left *= dl;
                self.right *= dr;
            }
            /* ★v17 统合衰减期: 到点硬归零 —— 旧震动瞬间消失, 不拖尾不进下一击;
             * 若期间有新命中, unified_until 已被重触发刷新 (哨兵 0 = 未布防) */
            if self.unified_until != 0 && !before(now, self.unified_until) {
                self.left = 0.0;
                self.right = 0.0;
                self.decay_l = 0.0;
                self.decay_r = 0.0;
                self.unified_until = 0;
                /* 高位阶段硬归零: 标记, 禁止低位阶段抬回 */
                self.hard_cut_l = true;
                self.hard_cut_r = true;
            }
            /* ★S1 脉冲落地 (v15): 高负载期 (密度自适应激活 / 命中限频咬合) 衰减
             * 尾巴降到本脉冲峰值的 tail_land_pct% 即归零 —— 脉冲之间出真静音
             * (借用 S4 的落地质感), 单挑/低负载期完全不变。只清衰减分量 (在
             * max 合成之前), hold/rhythm 分量不受影响。pct=0 关闭。 */
            if self.tail_land_pct > 0
                && (self.density_active
                    || (self.hitcap_max > 0 && self.hitcap_count >= self.hitcap_max))
            {
                let k = self.tail_land_pct.min(100) as f32 / 100.0;
                if self.tail_peak_l > 0.0 && self.left < self.tail_peak_l * k {
                    self.left = 0.0;
                    self.hard_cut_l = true; /* 高位阶段落地: 禁止低位阶段抬回 */
                }
                if self.tail_peak_r > 0.0 && self.right < self.tail_peak_r * k {
                    self.right = 0.0;
                    self.hard_cut_r = true;
                }
            }
            l_acc = l_acc.max(self.left);
            r_acc = r_acc.max(self.right);
            self.left = l_acc;
            self.right = r_acc;
        } else {
            /* ★0 哨兵守卫 (v15): 同 legacy —— hold_until/rhythm_until=0 在跨 2^31
             * 的机器上被裸 before(now,0) 误判为激活, hold_l=0 每帧覆盖输出 =
             * S4 FONT 全静默 (本机 25 天 uptime 实测复现) */
            if self.hold_until != 0 && before(now, self.hold_until) {
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
            if self.rhythm_until != 0 && before(now, self.rhythm_until) {
                let on = ((now / self.rhythm_period) & 1) == 0;
                self.left = if on { self.rhythm_l } else { self.rhythm_l * 0.15 };
                self.right = if on { self.rhythm_r } else { self.rhythm_r * 0.15 };
            } else if self.rhythm_until != 0 {
                self.rhythm_until = 0;
                self.left = 0.0;
                self.right = 0.0;
            }
        }

        if self.left < 0.02 {
            self.left = 0.0;
        }
        if self.right < 0.02 {
            self.right = 0.0;
        }

        /* ★S1 老方案 (v13.34): 删除 dot_active 的 0.03 保底 —— /100 量纲归一+
         * 重映射前置后, 0.03 非零输出会被抬进保底区间 = 每次吃 DOT 后约 30%
         * 的持续低鸣; DOT 语义已由 mode3 衰减脉冲承载。新方案维持保底。 */
        if !self.legacy && self.dot_active {
            self.left = self.left.max(0.03);
        }
        /* ★优先级阶梯 (v20): 高位阶段本帧硬归零的马达, 爆发保底不得抬回。
         * ★只在 S1 的 S4 模式 (1/2) 生效; 经典模式 0 维持 v19.7 原条件
         * (仅排除统合衰减期), 保证 ACT1 特调手感逐字节不变; S4 路线亦不变。 */
        let burst_floor_ok = if self.legacy && self.legacy_output_mode != 0 {
            !self.hard_cut_r
        } else {
            !(self.legacy && self.storm_unified_enabled && self.storm_active)
        };
        if self.state == VibState::Burst && burst_floor_ok
        {
            /* ★v17: 统合衰减期让路 —— 否则 Burst 保底会在硬归零后立刻把右马达
             * 抬回地板值, "旧的瞬间消失"失效 (实测 left=0 而 right=1.0) */
            let bm = self.p[P_BURST_MIN].max(0.0) / 100.0;
            /* ★S1 老方案 (v13.30): P_FONT_ATTACK 是 0-100 滑块量纲, right 是 0-1
             * —— 旧代码 bm×25=8.75 直接把 Burst 右马达顶满幅, 一并归一 */
            if self.legacy {
                /* ★S1 聚合记帐 (v14.1): Burst 保底乘密度疲劳 —— 群怪期连击
                 * 永续 Burst 造成"无法通过强度参数关闭的持续右马达地板",
                 * 随密度降温, 独立刷怪/单挑期保底满额不变 */
                self.right = self
                    .right
                    .max(bm * self.p[P_FONT_ATTACK] / 100.0 * self.density_scale);
            } else {
                self.right = self.right.max(bm * self.p[P_FONT_ATTACK]);
            }
        }
    }

    /* 输出动态范围重映射 (v30): 非零输出映射到 [min, 100] (65535 尺度),
     * 保证转子一定转起来, 相对强弱保留 */
    fn remap_out(&self, l_out: &mut f32, r_out: &mut f32) {
        if self.remap_enabled && (*l_out > 0.0 || *r_out > 0.0) {
            let mn = (self.remap_min.min(95.0) / 100.0) * 65535.0;
            let scale = 1.0 - self.remap_min.min(95.0) / 100.0;
            if *l_out > 0.0 {
                *l_out = mn + *l_out * scale;
            }
            if *r_out > 0.0 {
                *r_out = mn + *r_out * scale;
            }
        }
    }

    /* ★S1 × S4 不丢震动 (v20, 模式 2): S4 死区会把低于阈值的输出归零 ——
     * 那正是"轻反馈丢失"的来源。这里改为: 低于死区的能量不丢弃, 转入后置携带,
     * 攒到阈值即释放成一次厚脉冲 (攒厚再出); 高于死区的正常脉冲照常通过。
     * 携带无输入时按帧缓慢泄放 (防幽灵脉冲), 上限 = 2×阈值。
     * 最后统一走 S4 的重映射 (非零输出抬进 [remap_min,100])。 */
    fn shape_carry_and_filter(&mut self, l_out: &mut f32, r_out: &mut f32) {
        let thr = (self.out_threshold.min(50.0) / 100.0) * 65535.0;
        if thr <= 0.0 {
            self.remap_out(l_out, r_out);
            return;
        }
        let cap = thr * 2.0;
        let mut added_l = false;
        if *l_out > 0.0 && *l_out < thr {
            self.shape_carry_l = (self.shape_carry_l + *l_out).min(cap);
            *l_out = 0.0;
            added_l = true;
        }
        if self.shape_carry_l >= thr {
            *l_out = (*l_out).max(self.shape_carry_l);
            self.shape_carry_l = 0.0;
        } else if !added_l && *l_out <= 0.0 {
            self.shape_carry_l *= 0.9;
        }
        let mut added_r = false;
        if *r_out > 0.0 && *r_out < thr {
            self.shape_carry_r = (self.shape_carry_r + *r_out).min(cap);
            *r_out = 0.0;
            added_r = true;
        }
        if self.shape_carry_r >= thr {
            *r_out = (*r_out).max(self.shape_carry_r);
            self.shape_carry_r = 0.0;
        } else if !added_r && *r_out <= 0.0 {
            self.shape_carry_r *= 0.9;
        }
        self.remap_out(l_out, r_out);
    }

    /* ★优先级阶梯 (v20): 输出平滑统一入口 —— 高位阶段硬归零的马达直接复位
     * 平滑状态, 不让一阶低通把静音尾巴拖回来 (低通是"低位阶段", 不得压过
     * 统合衰减/脉冲落地的静音)。k<=0 与旧实现一致: 不平滑, 直接输出原值。
     * ★阶梯只在 S1 的 S4 模式 (legacy_output_mode 1/2) 生效; 经典模式 0 与
     * S4 路线维持旧的一阶低通逐字节不变 (ACT1 特调手感基线, 勿动)。 */
    fn smooth_out(&mut self, l: f32, r: f32, k: f32) -> (f32, f32) {
        if k <= 0.0 {
            return (l, r);
        }
        let ladder = self.legacy && self.legacy_output_mode != 0;
        if ladder && self.hard_cut_l {
            self.out_smooth_l = 0.0;
        } else {
            self.out_smooth_l += (l - self.out_smooth_l) * k;
        }
        if ladder && self.hard_cut_r {
            self.out_smooth_r = 0.0;
        } else {
            self.out_smooth_r += (r - self.out_smooth_r) * k;
        }
        (self.out_smooth_l.min(65535.0), self.out_smooth_r.min(65535.0))
    }

    /* 输出低强度死区: hysteresis=true 时恢复线为 1.8×阈值 (v29.3 防反复启停);
     * false 时恢复线=阈值 (S1 老方案 v13.27 修订: 真零静默, 非零一律放行)。
     * thr_pct = 死区阈值 (百分比; S1 老方案固定 15, 新方案取 out_threshold) */
    fn deadzone_filter(&mut self, l_out: &mut f32, r_out: &mut f32, hysteresis: bool, thr_pct: f32) {
        if thr_pct > 0.0 {
            let thr = (thr_pct.min(50.0) / 100.0) * 65535.0;
            let hi = if hysteresis { thr * 1.8 } else { thr };
            if self.out_dead_l {
                if *l_out > hi {
                    self.out_dead_l = false;
                } else {
                    *l_out = 0.0;
                }
            } else if *l_out < thr {
                self.out_dead_l = true;
                *l_out = 0.0;
            }
            if self.out_dead_r {
                if *r_out > hi {
                    self.out_dead_r = false;
                } else {
                    *r_out = 0.0;
                }
            } else if *r_out < thr {
                self.out_dead_r = true;
                *r_out = 0.0;
            }
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
        /* ★总闸 L/R (v15.1): item_lr[0]/[1] 接通为左右马达独立总闸微调
         * (±100% → ×0..×2), 与全局总调整 (master) 叠乘 */
        let l_raw = self.left * base * self.gate_lr_l;
        let r_raw = self.right * base * rhythm * self.gate_lr_r;
        let mut l_out = if l_raw > 0.0 { (l_raw.powf(curvel)).min(1.0) * 65535.0 } else { 0.0 };
        let mut r_out = if r_raw > 0.0 { (r_raw.powf(curver)).min(1.0) * 65535.0 } else { 0.0 };
        /* 输出动态范围重映射 + 低强度死区: 两条路线顺序与阈值不同 ——
         * ★S1 老方案 (v13.36 调校): remap_min=20 强制启用 + 重映射前置,
         * 死区 15 只杀真零、迟滞退出线=阈值 (无困死轻反馈的迟滞门槛);
         * 马达低强度电流声由重映射的下限保证占空比, 不再依赖死区。
         * ★审计修复 (P0, v15): 抬底下限跟随"全局总调整"(master) 缩放 ——
         * 固定 20% 曾把 master/attack/max 三闸可动范围压到 10-13 个百分点
         * (master 90→100 马达仅 +1.5 点 = "调了没调一样")。master=90 → 18%
         * 与原手感几乎一致; master 调低时地板同步下沉, 全局闸恢复线性。
         * 新方案 (v29.3/v30): 迟滞死区在前 (1.8× 恢复线防阈值附近反复启停),
         * 重映射在后。 */
        if self.legacy {
            match self.legacy_output_mode {
                1 => {
                    /* ★模式 1 S4 纯 (v20): S4 输出整形 —— 迟滞死区 (真静音) +
                     * 重映射在后。低于死区的轻反馈会被吞掉 (= 丢震动), A/B 对照用 */
                    self.deadzone_filter(&mut l_out, &mut r_out, true, self.out_threshold);
                    self.remap_out(&mut l_out, &mut r_out);
                }
                2 => {
                    /* ★模式 2 S4+不丢震动 (v20): 同 S4 整形, 但低于死区的能量
                     * 后置携带攒厚再出 —— 玩家不丢震动 */
                    self.shape_carry_and_filter(&mut l_out, &mut r_out);
                }
                _ => {
                    /* 模式 0 经典 ACT1 管线 (默认, 逐字节不变) */
                    let f = 0.20 * (master / 100.0).clamp(0.0, 1.0);
                    let mn = f * 65535.0;
                    let scale = 1.0 - f;
                    if l_out > 0.0 {
                        l_out = mn + l_out * scale;
                    }
                    if r_out > 0.0 {
                        r_out = mn + r_out * scale;
                    }
                    self.deadzone_filter(&mut l_out, &mut r_out, false, 15.0);
                }
            }
        } else {
            /* 输出低强度迟滞死区 (v29.3): 低于阈值归 0, 超过 1.8×阈值才恢复
             * (迟滞防阈值附近反复启停产生嗡声; 消除马达低强度电流声) */
            self.deadzone_filter(&mut l_out, &mut r_out, true, self.out_threshold);
            /* 输出动态范围重映射 (v30, ERM/Xbox360): 非零输出映射到 [min, 100],
             * 保证转子一定转起来 (轻反馈不被死区吞掉, 相对强弱保留) */
            self.remap_out(&mut l_out, &mut r_out);
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

/// u32 毫秒时间戳的回绕安全比较: now_ms 截断 u32, 49.7 天回绕一次。
/// "未过期"判定 = (deadline - now) 的有符号差 > 0, 在 ±2^31 ms 内恒正确;
/// 裸比较 `now < deadline` 在回绕点后会恒真 → 震动卡死最长 49.7 天。
#[inline]
fn before(now: u32, deadline: u32) -> bool {
    deadline.wrapping_sub(now) as i32 > 0
}

/// ★v24.11: 评分脉冲窗口是否激活 —— **必须带 0 哨兵守卫**。
///
/// `rank_full_until` 初始/复位为 0 (= 没有评分族事件)。裸 `before(now, 0)` 在 `now` 的低 31 位
/// 为负时恒为真 (u32 回绕语义), 会让输出每帧都走"评分分支" → **移动分支成为死代码**
/// (实机: 城镇/副本走路都不震, 但战斗/评分照震 —— 因为 legacy 评分分支把战斗输出 max 了进来)。
/// 该陷阱只在 `now_ms()` 落进"负半区"的约 24.8 天窗口内暴露, 故长期未被发现。
#[inline]
fn rank_window_active(now: u32, rank_full_until: u32) -> bool {
    rank_full_until != 0 && before(now, rank_full_until)
}

/// ★v24.11: 计时器"已到期 **或从未设置 (0)**" —— 用于需要"首次使用即武装"的步频/节奏计时器。
/// 裸 `!before(now, 0)` 恒为 false → 计时器永不武装; 而 `0 - now` 还会下溢。
#[inline]
fn expired_or_unset(now: u32, deadline: u32) -> bool {
    deadline == 0 || !before(now, deadline)
}

#[cfg(test)]
mod time_sentinel_tests {
    use super::*;

    /// 用户日志里的真实时间戳 (u32 截断后 bit31=1)。这正是当年没暴露的原因:
    /// 只有 now 落在"负半区"时, 裸 before(now, 0) 才会恒真。
    const NOW: u32 = 2_517_474_289;

    #[test]
    fn bare_before_on_zero_is_the_trap() {
        assert!(
            before(NOW, 0),
            "裸 before(now,0) 在此时间戳下为真 —— 这就是把移动分支挡死的根因"
        );
    }

    #[test]
    fn rank_window_requires_nonzero_deadline() {
        assert!(!rank_window_active(NOW, 0), "0 = 无评分事件, 不得判为激活");
        assert!(!rank_window_active(0, 0));
        assert!(
            rank_window_active(NOW, NOW.wrapping_add(200)),
            "窗口内应激活"
        );
        assert!(
            !rank_window_active(NOW.wrapping_add(300), NOW.wrapping_add(200)),
            "已过期应关闭"
        );
        // 回绕边界仍按有符号差判定
        assert!(rank_window_active(u32::MAX - 10, 5), "回绕后仍在窗口内");
    }

    #[test]
    fn expired_or_unset_arms_on_first_use() {
        assert!(expired_or_unset(NOW, 0), "未设置 → 需要武装");
        assert!(!expired_or_unset(NOW, NOW.wrapping_add(200)), "未到期不武装");
        assert!(
            expired_or_unset(NOW.wrapping_add(300), NOW.wrapping_add(200)),
            "已到期 → 需要武装"
        );
    }
}

/// 向"当前已连接"的 XInput 槽位输出震动 (每次输出时扫描 0..4)。
/// 旧实现硬编码 0 号槽 —— 非 0 号槽位的手柄收不到任何游戏震动。
fn send_vibration(left: u16, right: u16) {
    let vib = XINPUT_VIBRATION {
        wLeftMotorSpeed: left,
        wRightMotorSpeed: right,
    };
    for slot in 0..4u32 {
        let mut st = XINPUT_STATE::default();
        if unsafe { crate::xinput::xinput_get_state(slot, &mut st) } == 0 {
            unsafe {
                let _ = crate::xinput::xinput_set_state(slot, &vib);
            }
            return;
        }
    }
}

/// ★v16.8 高级算法总开关对 60 槽参数的覆盖 (纯函数, 便于测试):
/// 关闭 = 把该算法的"关"语义写入参数 (滑块值仍保留在 AppState/config)。
/// v16.9 扩展三组: 衰减时长/算法窗口 (回内置「默认」预设值) + 静默脉冲 (中性值)。
pub fn apply_algo_toggle_params(
    params: &mut [u32; 60],
    density_enabled: bool,
    adapt_enabled: bool,
    move_charge_enabled: bool,
    decay_enabled: bool,
    algo_windows_enabled: bool,
    pulse_enabled: bool,
) {
    if !density_enabled {
        params[29] = 100; /* P_DENSITY_THR: 100 = 关闭密度自适应 */
    }
    if !adapt_enabled {
        params[51] = 0; /* P_ADAPT_REDUCE: 0 = 打太快不减轻 */
    }
    if !move_charge_enabled {
        params[21] = 0; /* P_MOVE_CHARGE_RATE: 0 = 不积累走位能量 */
    }
    /* 各类事件衰减时长 → 内置「默认」预设值 (25/55/25/60) */
    if !decay_enabled {
        for (i, v) in [(35usize, 25u32), (36, 55), (37, 25), (38, 60)] {
            params[i] = v;
        }
    }
    /* 算法窗口时长 → 内置「默认」预设值 (150/90/250/20/600/1500/3000) */
    if !algo_windows_enabled {
        for (i, v) in [
            (40usize, 150u32),
            (41, 90),
            (42, 250),
            (43, 20),
            (44, 600),
            (45, 1500),
            (46, 3000),
        ] {
            params[i] = v;
        }
    }
    /* 静默/反击/脉冲 → 中性值 (静默 0 / 反击倍数 100 / 唤醒 0 / 里程碑 0 / 收尾 0;
     * 测试强度 59 保留不动) */
    if !pulse_enabled {
        for (i, v) in [(54usize, 0u32), (55, 100), (56, 0), (57, 0), (58, 0)] {
            params[i] = v;
        }
    }
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
            /* 环状态沿记录: VIB_RING_INVALID 只在 ok→invalid 跳变时记一条
             * (DLL 重启窗口每轮都判 invalid 时不再刷屏; crash.log 是 panic 日志,
             * 保持信噪比) */
            let mut ring_prev_ok = true;
            /* ★S1 老方案: 路线开关 (评分/移动合成、计数口径、注入门控按路线分叉) */
            let legacy = state.vib_legacy_client.load(Ordering::Relaxed);
            /* 输出闸门诊断: 翻转才记 [GATE] 行 (防刷屏), 见循环内注释 */
            let mut last_gate_state: Option<(bool, bool, bool)> = None;

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
                /* ★v16.8/16.9: 高级算法总开关对参数槽的覆盖
                 * (密度/命中自适应/走位能量 + 衰减时长/算法窗口/静默脉冲) */
                apply_algo_toggle_params(
                    &mut params,
                    state.vibration_density_enabled.load(Ordering::Relaxed),
                    state.vibration_adapt_enabled.load(Ordering::Relaxed),
                    state.vibration_move_charge_enabled.load(Ordering::Relaxed),
                    state.vibration_decay_enabled.load(Ordering::Relaxed),
                    state.vibration_algo_windows_enabled.load(Ordering::Relaxed),
                    state.vibration_pulse_enabled.load(Ordering::Relaxed),
                );
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
                /* ★输出闸门诊断 (v14.1.1): 事件消费在总开关判断之前, 总开关关闭/
                 * 暂停时 [INJ] 照打但每轮强制 send(0,0) —— 曾因此"日志有注入命中、
                 * 马达纹丝不动"无从排查。有效输出状态一旦翻转, 记一行 [GATE]。 */
                let gate_paused = state.is_paused();
                let gate_abs = state.vibration_abs_freq_enabled.load(Ordering::Relaxed);
                let gate_state = (enabled, gate_paused, gate_abs);
                if last_gate_state != Some(gate_state) {
                    vib_log(&format!(
                        "[GATE] 输出{} (总开关={}, 暂停={}, 绝对频率={})",
                        if enabled { "开" } else { "关" },
                        state.vibration_enabled.load(Ordering::Relaxed),
                        gate_paused,
                        gate_abs
                    ));
                    last_gate_state = Some(gate_state);
                }

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
                                if s.magic == VIB_SHM_MAGIC && s.version == VIB_SHM_VERSION {
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
                    let connected = s.magic == VIB_SHM_MAGIC
                        && s.version == VIB_SHM_VERSION
                        && process_alive(s.game_pid);
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
                            /* ★S1 老方案计数口径 (老宿主同源): 只计真实注入的震动;
                             * ★v24.12 移动洪流 (6ms 续期, 10 分钟 ≈ 6 万条) 两种模式都不计入总数,
                             * 否则 S4 模式下"累计事件"会暴增 (只留 rank_type_events[11] 的移动细分计数)。 */
                            let counted = counts_toward_event_total(
                                ev.etype,
                                legacy,
                                engine.last_injected,
                            );
                            t = t.wrapping_add(ev_size);
                            if counted {
                                got = got.wrapping_add(1);
                            }
                        }
                        if ring_ok {
                            s.ring.tail.store(t, Ordering::Release);
                            if got > 0 {
                                state.vibration_events_received.fetch_add(got as u64, Ordering::Relaxed);
                            }
                        } else {
                            // 头尾异常(DLL 重启 head 回绕 / 软件漏读落后超界): 不能一直丢
                            // (会让震动中断), 把 tail 同步到 head 丢弃积压, 下轮立即恢复读取。
                            if ring_prev_ok {
                                crate::util::crash_log(
                                    "VIB_RING_INVALID",
                                    &format!("cap={capacity} head={head} tail={tail} (已同步 tail=head)"),
                                );
                                ring_prev_ok = false;
                            }
                            s.ring.tail.store(head, Ordering::Release);
                        }
                        ring_prev_ok = ring_ok;
                    } else {
                        let _ = unsafe { UnmapViewOfFile(view) };
                        if let Some(h) = shm_handle.take() {
                            let _ = unsafe { CloseHandle(h) };
                        }
                        shm_view = None;
                        // 整引擎重置: 旧实现只清 left/right, hold/rhythm/评分脉冲等
                        // deadline 字段残留 (配合 u32 回绕会把震动卡死到下一轮 tick)
                        engine = VibEngine::new();
                        send_vibration(0, 0);
                    }
                } else {
                    state.vibration_connected.store(false, Ordering::Relaxed);
                }

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
                        send_vibration(vl, vr);
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
                        /* ★v16.8 高级算法总开关: 关 = 该算法回到旧行为 (参数按 0/关
                         * 语义覆盖, 滑块值保留); 默认全开 = 现行行为 */
                        let merge_en = state.vibration_merge_enabled.load(Ordering::Relaxed);
                        let hitcap_en = state.vibration_hitcap_enabled.load(Ordering::Relaxed);
                        let sustain_en = state.vibration_sustain_enabled.load(Ordering::Relaxed);
                        let tail_en = state.vibration_tail_land_enabled.load(Ordering::Relaxed);
                        /* 群怪聚合参数 (v14.1, 玩家可调): 每帧从 AppState 同步 */
                        engine.merge_keep = if merge_en {
                            state.vibration_merge_keep.load(Ordering::Relaxed)
                        } else {
                            0
                        };
                        engine.merge_cap = state.vibration_merge_cap.load(Ordering::Relaxed);
                        engine.merge_hold = if merge_en {
                            state.vibration_merge_hold.load(Ordering::Relaxed)
                        } else {
                            0
                        };
                        /* 命中限频参数 (v14.2, 玩家可调): 每帧从 AppState 同步 */
                        engine.hitcap_max = if hitcap_en {
                            state.vibration_hitcap_max.load(Ordering::Relaxed)
                        } else {
                            0
                        };
                        engine.hitcap_win_ms = state.vibration_hitcap_win.load(Ordering::Relaxed);
                        engine.hitmerge_ms = if merge_en {
                            state.vibration_hitmerge_ms.load(Ordering::Relaxed)
                        } else {
                            0
                        };
                        /* 持续压制参数 (v15.3, 玩家可调): 每帧同步 */
                        engine.sustain_secs = if sustain_en {
                            state.vibration_sustain_secs.load(Ordering::Relaxed)
                        } else {
                            0
                        };
                        engine.sustain_reduce = state.vibration_sustain_reduce.load(Ordering::Relaxed);
                        /* 脉冲落地 / 怪物异常反馈 (v15, 玩家可调): 每帧同步 */
                        engine.tail_land_pct = if tail_en {
                            state.vibration_tail_land_pct.load(Ordering::Relaxed)
                        } else {
                            0
                        };
                        engine.monster_abnormal = state.vibration_monster_abnormal.load(Ordering::Relaxed);
                        /* 命中风暴抽样 (v16, 玩家可调): 每帧同步。
                         * ★v16.8: 检测阈值独立于抽样开关 —— 旧配置 storm_thr=0
                         * (旧语义=关闭) 迁移为 抽样关 + 阈值 10, 风暴检测继续运行,
                         * 两个"风暴期静音"开关不再被阈值 0 掐死 */
                        let storm_thr_raw = state.vibration_storm_thr.load(Ordering::Relaxed);
                        engine.storm_thr = if storm_thr_raw == 0 { 10 } else { storm_thr_raw };
                        engine.storm_enabled = state.vibration_storm_enabled.load(Ordering::Relaxed)
                            && storm_thr_raw != 0;
                        engine.storm_keep_pct = state.vibration_storm_keep_pct.load(Ordering::Relaxed);
                        engine.storm_win_ms = state.vibration_storm_win_ms.load(Ordering::Relaxed);
                        engine.storm_pause_ms = state.vibration_storm_pause_ms.load(Ordering::Relaxed);
                        engine.storm_mute_abnormal = state.vibration_storm_mute_abnormal.load(Ordering::Relaxed);
                        engine.storm_mute_rank = state.vibration_storm_mute_rank.load(Ordering::Relaxed);
                        /* 统合衰减期 (v17, 玩家可调): 每帧同步 */
                        engine.storm_unified_enabled =
                            state.vibration_storm_unified_enabled.load(Ordering::Relaxed);
                        engine.storm_unified_ms = state.vibration_storm_unified_ms.load(Ordering::Relaxed);
                        /* ★S1 输出引擎模式 (v20): 每帧同步 (0-2, 越界回经典) */
                        engine.legacy_output_mode =
                            state.vibration_legacy_output_mode.load(Ordering::Relaxed).min(2);
                        /* 总闸 L/R (v15.1): item_lr[0]/[1] → 左右马达独立总闸微调 */
                        engine.gate_lr_l = (1.0
                            + state.vibration_item_lr[0].load(Ordering::Relaxed) as i32 as f32 / 100.0)
                            .clamp(0.0, 2.0);
                        engine.gate_lr_r = (1.0
                            + state.vibration_item_lr[1].load(Ordering::Relaxed) as i32 as f32 / 100.0)
                            .clamp(0.0, 2.0);
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
                        /* ★S1 老方案总开关: 设置里切换, 每轮同步 (改设置即时生效) */
                        engine.legacy = state.vib_legacy_client.load(Ordering::Relaxed);
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
                        /* 评分脉冲: 按各事件强度 × 满幅输出, 持续 rank_duration
                         * ★v24.11: 必须用带 0 哨兵守卫的 rank_window_active —— 裸 before(now,0)
                         * 会在"负半区"时间戳下恒真, 把移动分支挡死 (根因见该函数注释)。 */
                        if rank_window_active(now_ms(), engine.rank_full_until) {
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
                            /* 低强度死区 (v29.3): 评分通道低强度段归 0 (消除嗡声)
                             * ★审计修复 (P0): out_threshold 是 S4 的死区参数, 经 thr3
                             * 泄漏进 S1 评分通道会把低增益评分全部杀光 (实锤: 用户
                             * out_threshold=60 + rank_gain=30 → 评分全灭)。S1 评分与
                             * FONT 同用 15% 只杀真零语义。 */
                            let thr3 = if legacy {
                                (15.0f32 / 100.0) * 65535.0
                            } else {
                                (engine.out_threshold.min(50.0) / 100.0) * 65535.0
                            };
                            let vl = if (vl as f32) < thr3 { 0 } else { vl };
                            let vr = if (vr as f32) < thr3 { 0 } else { vr };
                            /* ★S1 老方案 (v13.37 合成式): 评分脉冲不再独占马达 ——
                             * 与战斗 FONT 引擎输出取 max (走位/战斗中击杀+评分不再
                             * 压掉命中震动; "通道冲突"的主循环版)。新方案维持纯评分脉冲 */
                            if legacy {
                                let (efl, efr) = engine.finalize(&params);
                                let smr = state.vibration_out_smooth.load(Ordering::Relaxed).min(100) as f32 / 100.0;
                                let (efl, efr) = engine.smooth_out(efl as f32, efr as f32, smr * 0.9);
                                let vl = vl.max((efl * motor_l * (1.0 + rnd)).min(65535.0) as u16);
                                let vr = vr.max((efr * motor_r * (1.0 + rnd)).min(65535.0) as u16);
                                send_vibration(vl, vr);
                                state.vibration_out_l.store(vl as u32, Ordering::Relaxed);
                                state.vibration_out_r.store(vr as u32, Ordering::Relaxed);
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
                            } else {
                            send_vibration(vl, vr);
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
                            }
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
                            /* ★v24.11: 0 哨兵 —— 首次进入移动分支时 move_pace_until=0,
                             * 裸 !before(now,0) 恒为 false 会导致步频计时器永不武装
                             * (且下面的 0 - now 会下溢)。 */
                            if expired_or_unset(now, engine.move_pace_until) {
                                engine.move_pace_until = now.wrapping_add(pace_ms);
                                engine.move_phase = !engine.move_phase;
                            }
                            let pace_frac = if engine.move_pace_until != 0
                                && before(now, engine.move_pace_until)
                            {
                                1.0 - (engine.move_pace_until.wrapping_sub(now) as f32
                                    / pace_ms as f32)
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
                            /* ★S1 老方案 (v13.37 合成式): 移动通道不再独占马达 ——
                             * 与战斗 FONT 引擎输出取 max (边走边打的命中不再被
                             * 移动通道覆盖)。新方案维持纯移动输出 */
                            if legacy {
                                let (efl, efr) = engine.finalize(&params);
                                let smr = state.vibration_out_smooth.load(Ordering::Relaxed).min(100) as f32 / 100.0;
                                let (efl, efr) = engine.smooth_out(efl as f32, efr as f32, smr * 0.9);
                                let vl = vl.max((efl * motor_l * (1.0 + rnd)).min(65535.0) as u16);
                                let vr = vr.max((efr * motor_r * (1.0 + rnd)).min(65535.0) as u16);
                                send_vibration(vl, vr);
                                state.vibration_out_l.store(vl as u32, Ordering::Relaxed);
                                state.vibration_out_r.store(vr as u32, Ordering::Relaxed);
                            } else {
                            send_vibration(vl, vr);
                            state.vibration_out_l.store(vl as u32, Ordering::Relaxed);
                            state.vibration_out_r.store(vr as u32, Ordering::Relaxed);
                            }
                        } else {
                            let (l, r) = engine.finalize(&params);
                            /* 输出平滑 (v22.3): 一阶低通抑制低频嗡嗡声 (快速连击/衰减尾音
                             * 的输出跳变被柔化; 0=不平滑, 100=强平滑, 默认 55) */
                            let sm = state.vibration_out_smooth.load(Ordering::Relaxed).min(100) as f32 / 100.0;
                            let (l, r) = engine.smooth_out(l as f32, r as f32, sm * 0.9);
                            let l = l as u16;
                            let r = r as u16;
                            let rk = 1.0 + rnd;
                            let vl = ((l as f32) * motor_l * rk).min(65535.0) as u16;
                            let vr = ((r as f32) * motor_r * rk).min(65535.0) as u16;
                            send_vibration(vl, vr);
                            state.vibration_out_l.store(vl as u32, Ordering::Relaxed);
                            state.vibration_out_r.store(vr as u32, Ordering::Relaxed);
                        }
                    }
                } else {
                    send_vibration(0, 0);
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


#[cfg(test)]
mod legacy_route_tests {
    use super::*;

    fn engine_with(legacy: bool) -> VibEngine {
        let mut e = VibEngine::new();
        e.legacy = legacy;
        e
    }

    /* ★S1 老方案回归: 重映射前置 + 死区只杀真零。
     * 场景 = 老项目实测"命中只有第一下震"的根因:
     * 输出 25% < 死区阈值 30% —— 新方案被迟滞死区归零且重映射救不回;
     * 老方案先重映射抬进 [28,100] (25→46%), 死区 (hi=阈值) 不再吞掉。 */
    #[test]
    fn legacy_finalize_remap_rescues_light_feedback() {
        let mut params = [100u32; 60];
        let mut e = engine_with(true);
        e.left = 0.25;
        e.remap_enabled = true;
        e.remap_min = 28.0;
        e.out_threshold = 30.0;
        let (l, r) = e.finalize(&params);
        let pct = l as f32 / 65535.0 * 100.0;
        assert!(
            pct >= 28.0,
            "老方案: 轻反馈应被重映射抬到 ≥28%, 实际 {:.1}%",
            pct
        );
    }

    #[test]
    fn new_route_deadzone_still_zeroes_light_feedback() {
        let mut params = [100u32; 60];
        let mut e = engine_with(false);
        e.left = 0.25;
        e.remap_enabled = true;
        e.remap_min = 28.0;
        e.out_threshold = 30.0;
        let (l, _) = e.finalize(&params);
        assert_eq!(l, 0, "新方案: 低于死区阈值的输出应保持归零 (现行行为不变)");
    }

    /* 老方案 inject 不再清除共存通道: hold 激活时注入衰减脉冲,
     * hold_until 不得被清 (合成式共存的前提); 新方案维持抢占清零。 */
    #[test]
    fn legacy_inject_keeps_hold_alive() {
        let mut e = engine_with(true);
        e.inject_hold(0.5, 1.0, 1.0, 200);
        assert!(e.hold_until > 0);
        e.inject(0.3, 1.0, 1.0, 100.0, 80.0);
        assert!(e.hold_until > 0, "老方案: 命中注入不得打断 CC 保持震");

        let mut n = engine_with(false);
        n.inject_hold(0.5, 1.0, 1.0, 200);
        n.inject(0.3, 1.0, 1.0, 100.0, 80.0);
        assert_eq!(n.hold_until, 0, "新方案: 现行抢占语义不变");
    }

    /* FONT 量纲归一仅老方案生效: 同一滑块 25, 老方案输出 0-1 量纲 (线性),
     * 新方案维持 0-100 原值注入 (现行行为不变)。 */
    #[test]
    fn legacy_normalizes_font_scale() {
        let mut o = engine_with(true);
        let mut n = engine_with(false);
        // 直填 finalize 输入: left=0.25 (已含 /100 的等价效果由 push 链产生,
        // 此处只锁 finalize 输出一致性 —— 两条路线 finalize 本身相同)
        o.left = 0.25;
        n.left = 0.25;
        o.remap_enabled = false;
        n.remap_enabled = false;
        o.out_threshold = 0.0;
        n.out_threshold = 0.0;
        let params = [100u32; 60];
        let (ol, _) = o.finalize(&params);
        let (nl, _) = n.finalize(&params);
        // S1 老方案 finalize 现带内置输出管线 (remap_min=20/死区=15 强制),
        // 两路线不再同构: 老方案 0.25 → 20+0.25*80 = 40%; 新方案线性 25%
        let expect = (0.20 * 65535.0 + 0.25 * 0.80 * 65535.0) as u16;
        assert_eq!(ol, expect, "老方案: 应走内置 remap(20) 管线");
        assert_eq!(nl, 16383, "新方案: 线性 25% 不变");
    }

    /* ★S1 老方案 (v13.36): remap_min=20/死区 15 为路线内置基线,
     * 不随新方案的 remap_enabled/remap_min/out_threshold 设置漂移 */
    #[test]
    fn legacy_finalize_uses_builtin_output_pipeline() {
        let mut params = [100u32; 60];
        let mut e = engine_with(true);
        e.left = 0.5;
        e.remap_enabled = false; // 老方案强制启用, 不看这个开关
        e.remap_min = 5.0; // 老方案用内置 20, 不看这个值
        e.out_threshold = 40.0; // 老方案用内置 15, 不看这个值
        let (l, _) = e.finalize(&params);
        let pct = l as f32 / 65535.0 * 100.0;
        assert!(
            (pct - 60.0).abs() < 1.0,
            "老方案: remap_min=20/死区=15 内置管线, 实际 {:.1}%",
            pct
        );
    }

    /* ── ★S1 群怪聚合记帐 (v14.1, 玩家可调) 回归: 窗内事件记账不丢弃,
     * 能量携带兑现; keep=0 完全关闭 = 旧行为 ── */

    /* 公共态: 密度/自适应关闭, IVL 20, 命中 70, 玩家 DOT 20, 已过唤醒;
     * 聚合默认参数 80/100/80 (与 config.vibration serde 默认一致) */
    fn carry_engine(legacy: bool) -> VibEngine {
        let mut e = engine_with(legacy);
        e.p[P_FONT_IVL] = 20.0;
        e.p[P_FONT_ATTACK] = 70.0;
        e.p[P_FONT_HP] = 20.0;
        e.p[P_DENSITY_THR] = 100.0; /* 密度自适应关闭 */
        e.p[P_ADAPT_REDUCE] = 0.0; /* 自适应早触发关闭 */
        e.p[P_COUNTER_MUL] = 100.0;
        e.p[P_DEC_ATTACK] = 60.0;
        e.p[P_DOT_HOLD] = 150.0;
        e.p[P_EFFECT_PERIOD] = 90.0;
        e.state = VibState::Combat;
        e.wake_done = true;
        e
    }

    fn push_font(e: &mut VibEngine, a6: u32) {
        let ev = VibEvent {
            etype: VEV_FONT,
            strength: a6,
            tick: 0,
            reserved: 1,
        };
        let item_out: [AtomicU32; 26] = std::array::from_fn(|_| AtomicU32::new(0));
        e.push_event(
            &ev, true, &[0u32; 26], &item_out, &[0u32; 15], &[0i32; 30], 0, 300,
        );
    }

    /* 非 FONT 事件 (评分族/死亡/移动) 的测试注入 */
    fn push_etype(
        e: &mut VibEngine,
        etype: u32,
        strength: u32,
        rank_gain: &[u32; 15],
        rank_level_gain: u32,
    ) {
        let ev = VibEvent {
            etype,
            strength,
            tick: 0,
            reserved: 1,
        };
        let item_out: [AtomicU32; 26] = std::array::from_fn(|_| AtomicU32::new(0));
        e.push_event(
            &ev, true, &[0u32; 26], &item_out, rank_gain, &[0i32; 30], rank_level_gain, 300,
        );
    }

    #[test]
    fn legacy_merge_carries_energy_into_next_hit() {
        let mut e = carry_engine(true);
        /* 群怪第 2 击落在 20ms 注入窗内 → 记账, 不注入不丢失 */
        e.last_font_ch[3] = now_ms();
        push_font(&mut e, 0x01);
        assert!(
            e.font_carry_s[3] > 0.0,
            "窗内命中应按通道记账 (能量携带)"
        );
        assert!(!e.last_injected, "记账本身不产生注入");
        /* 下一发命中 (通道戳拨旧 1000ms 过间隔门) → 兑现记账, 单击变厚 */
        e.last_font_ch[3] = now_ms().wrapping_sub(1000);
        push_font(&mut e, 0x01);
        assert!(e.last_injected, "兑现发是真实注入");
        assert_eq!(e.font_carry_s[3], 0.0, "兑现后记账清零");
        /* 对照组: 同参数单发命中 (无记账) */
        let mut c = carry_engine(true);
        c.last_font_ch[3] = now_ms().wrapping_sub(1000);
        push_font(&mut c, 0x01);
        assert!(
            e.left > c.left * 1.2,
            "群怪后的命中应明显更厚: 兑现 {:.3} vs 单发 {:.3}",
            e.left,
            c.left
        );
    }

    #[test]
    fn legacy_merge_caps_carry_energy() {
        let mut e = carry_engine(true);
        e.merge_cap = 100; /* 封顶滑块 100% = 1.0 (0-1 量纲) */
        for _ in 0..3 {
            e.last_font_ch[3] = now_ms();
            push_font(&mut e, 0x01);
        }
        let cap = e.merge_cap as f32 / 100.0;
        assert!(
            (e.font_carry_s[3] - cap).abs() < 1e-6,
            "记账封顶 {} 防叠加爆震, 实际 {}",
            cap,
            e.font_carry_s[3]
        );
    }

    #[test]
    fn legacy_merge_keep_zero_disables_merge() {
        /* 关闭开关回归: merge_keep=0 → 完全回到丢弃语义 (旧行为), 无记账 */
        let mut e = carry_engine(true);
        e.merge_keep = 0;
        e.last_font_ch[3] = now_ms();
        push_font(&mut e, 0x01);
        assert_eq!(e.font_carry_s[3], 0.0, "keep=0 不得记账");
        assert!(!e.last_injected, "keep=0 窗内事件仍丢弃 (旧行为)");
        /* 补发开关回归: merge_hold=0 → 到期不补发 (记账等到期作废) */
        let mut h = carry_engine(true);
        h.last_font_ch[3] = now_ms();
        push_font(&mut h, 0x01);
        assert!(h.font_carry_s[3] > 0.0);
        h.merge_hold = 0;
        h.font_carry_until[3] = now_ms().wrapping_sub(1);
        h.tick(&[100u32; 60]);
        assert!(h.left == 0.0 && h.right == 0.0, "hold=0 不得补发");
    }

    #[test]
    fn legacy_merge_flushes_expired_carry_in_tick() {
        let mut e = carry_engine(true);
        e.last_font_ch[3] = now_ms();
        push_font(&mut e, 0x01);
        assert!(e.font_carry_s[3] >= FONT_CARRY_FLUSH_MIN);
        e.font_carry_until[3] = now_ms().wrapping_sub(1); /* 拨到已到期 */
        let params = [100u32; 60];
        e.tick(&params);
        assert_eq!(e.font_carry_s[3], 0.0, "到期记账应清零");
        assert!(
            e.left > 0.0 || e.right > 0.0,
            "到期记账应补发收尾脉冲 (防能量蒸发)"
        );
    }

    #[test]
    fn legacy_dot_events_feed_density_counter() {
        let mut e = carry_engine(true);
        e.p[P_DENSITY_THR] = 2.0;
        e.p[P_DENSITY_WIN] = 500.0;
        push_font(&mut e, 0x20); /* 玩家 DOT 红字 */
        assert!(!e.density_active);
        push_font(&mut e, 0x20);
        assert!(
            e.density_active,
            "群怪 DOT 洪流应喂入密度计数 (v22.4 盲区修复)"
        );
        /* 新方案: DOT 不计密度 (现行行为不变, 红线) */
        let mut n = carry_engine(false);
        n.p[P_DENSITY_THR] = 2.0;
        n.p[P_DENSITY_WIN] = 500.0;
        push_font(&mut n, 0x20);
        push_font(&mut n, 0x20);
        assert!(!n.density_active, "新方案: DOT 不计密度 (现行行为不变)");
    }

    #[test]
    fn legacy_burst_floor_scales_with_density_fatigue() {
        let now0 = now_ms();
        let mut params = [100u32; 60];
        params[P_BURST_MIN] = 30;
        params[P_FONT_ATTACK] = 70;
        params[P_DENSITY_REDUCE] = 45;
        params[P_DENSITY_FLOOR] = 55;
        params[P_DENSITY_SMOOTH] = 0;
        params[P_DENSITY_RECOVER] = 100;
        /* 密度疲劳激活期 (density_scale → 0.55): Burst 右马达保底同步降温 */
        let mut e = carry_engine(true);
        e.state = VibState::Burst;
        e.burst_until = now0.wrapping_add(800);
        e.last_combo = now0.wrapping_sub(50);
        e.last_event = now0; /* 防 Idle 检查把 state 复位 (last_event=0 哨兵) */
        e.density_active = true;
        e.tick(&params);
        let expect = 0.30 * 0.70 * 0.55;
        assert!(
            (e.right - expect).abs() < 1e-3,
            "Burst 保底应乘密度疲劳: 期望 {:.4}, 实际 {:.4}",
            expect,
            e.right
        );
        /* 无密度疲劳: 保底满额 0.30×0.70 (老方案手感不变) */
        let mut e2 = carry_engine(true);
        e2.state = VibState::Burst;
        e2.burst_until = now0.wrapping_add(800);
        e2.last_event = now0;
        e2.tick(&params);
        assert!(
            (e2.right - 0.21).abs() < 1e-3,
            "无密度疲劳时 Burst 保底满额 0.21, 实际 {:.4}",
            e2.right
        );
    }

    #[test]
    fn new_route_gate_unchanged_no_carry() {
        /* 红线: 新方案 (S4+) 注入窗内仍丢弃, 无任何记账状态 */
        let mut n = carry_engine(false);
        n.last_font = now_ms(); /* 新方案全局单窗 */
        push_font(&mut n, 0x01);
        assert!(!n.last_injected, "新方案: 窗内仍丢弃 (现行行为不变)");
        assert!(
            n.font_carry_s.iter().all(|&c| c == 0.0),
            "新方案不得产生记账状态"
        );
    }

    /* ── ★S1 命中限频 (v14.2) 回归: 超额命中全额记账不丢弃, 关闭态零影响 ── */

    #[test]
    fn legacy_hitcap_limits_and_carries_energy() {
        let mut e = carry_engine(true);
        e.hitcap_max = 1;
        e.hitcap_win_ms = 1000;
        /* 第 1 发: 窗口滚动放行 → 正常注入, count=1 */
        e.last_font_ch[3] = now_ms().wrapping_sub(1000);
        push_font(&mut e, 0x01);
        assert!(e.last_injected);
        assert_eq!(e.hitcap_count, 1);
        assert_eq!(e.font_carry_s[3], 0.0);
        /* 第 2 发: <20ms → 走 merge 记账, 不占限频名额 */
        e.last_font_ch[3] = now_ms();
        push_font(&mut e, 0x01);
        assert!(!e.last_injected);
        assert!(e.font_carry_s[3] > 0.0);
        assert_eq!(e.hitcap_count, 1);
        /* 第 3 发: 过 IVL 门但限频已满 → 全额记账, 不打戳不注入 */
        e.last_font_ch[3] = now_ms().wrapping_sub(500);
        let anchor_before = e.last_font_ch[3];
        push_font(&mut e, 0x01);
        assert!(!e.last_injected, "超限命中不得注入");
        assert!(e.font_carry_s[3] > 0.0, "超限命中应全额记账 (不丢能量)");
        assert_eq!(
            e.last_font_ch[3], anchor_before,
            "超限命中不得推进 IVL 锚点 (防通道饥饿)"
        );
        /* 第 4 发: 滚窗放行 → 兑现记账能量, 比对照组单发明显更厚 */
        e.hitcap_start = now_ms().wrapping_sub(1001);
        e.last_font_ch[3] = now_ms().wrapping_sub(1000);
        push_font(&mut e, 0x01);
        assert!(e.last_injected, "滚窗后恢复注入");
        assert_eq!(e.font_carry_s[3], 0.0, "兑现后记账清零");
        assert_eq!(e.hitcap_count, 1, "新窗口重新计数");
        let mut c = carry_engine(true);
        c.hitcap_max = 0;
        c.last_font_ch[3] = now_ms().wrapping_sub(1000);
        push_font(&mut c, 0x01);
        assert!(
            e.left > c.left * 1.2,
            "被限命中的能量应让下一发更厚: {:.3} vs {:.3}",
            e.left,
            c.left
        );
    }

    #[test]
    fn legacy_hitcap_off_passes_through() {
        /* 关闭态 (max=0): 命中全部照常注入, 完全休眠零影响 */
        let mut e = carry_engine(true);
        e.hitcap_max = 0;
        for _ in 0..8 {
            e.last_font_ch[3] = now_ms().wrapping_sub(1000);
            push_font(&mut e, 0x01);
            assert!(e.last_injected, "hitcap=0 不得拦截任何命中");
        }
        assert_eq!(e.hitcap_count, 0, "关闭时完全不计数");
        assert_eq!(e.font_carry_s[3], 0.0);
    }

    #[test]
    fn new_route_ignores_hitcap() {
        /* 红线: 新方案不受命中限频影响 */
        let mut n = carry_engine(false);
        n.hitcap_max = 6;
        n.last_font = now_ms();
        push_font(&mut n, 0x01);
        assert!(!n.last_injected, "新方案: 窗内仍丢弃 (现行行为不变)");
        assert_eq!(n.hitcap_count, 0, "新方案不计数");
        assert!(
            n.font_carry_s.iter().all(|&c| c == 0.0),
            "新方案无记账状态"
        );
    }

    /* ── ★S1 脉冲落地 (v15) 回归: 高负载期尾巴归零, 关闭态/S4/补发零影响 ── */

    /* 公共 tick 参数: 衰减 60ms + 密度恢复窗拉满 (防 tick 复位 density_active) */
    fn landing_params() -> [u32; 60] {
        let mut p = [100u32; 60];
        p[P_DEC_ATTACK] = 60;
        p[P_DENSITY_RECOVER] = 60000;
        p
    }

    #[test]
    fn legacy_pulse_landing_density_active() {
        let mut e = carry_engine(true);
        e.tail_land_pct = 25;
        e.density_active = true;
        e.last_combo = now_ms(); /* 防 tick 复位 density_active */
        e.last_font_ch[3] = now_ms().wrapping_sub(1000);
        push_font(&mut e, 0x01); /* peak_left = 0.70×0.35 = 0.245 */
        assert!(e.tail_peak_l > 0.2, "注入应记录峰值: {}", e.tail_peak_l);
        let params = landing_params();
        /* 2×60ms: 0.245 → 0.090 (37% > 25% 存活) → 0.033 (13.5% < 25% 落地) */
        for _ in 0..2 {
            e.last_output = now_ms().wrapping_sub(60); /* 强制 dt=60ms */
            e.tick(&params);
        }
        assert_eq!(
            e.left, 0.0,
            "高负载期衰减尾巴应在峰值 25% 处落地归零"
        );
    }

    #[test]
    fn legacy_pulse_landing_off_keeps_natural_tail() {
        /* 关闭对照 (pct=0): 同场景尾巴自然衰减, 不被强制归零 */
        let mut e = carry_engine(true);
        e.tail_land_pct = 0;
        e.density_active = true;
        e.last_combo = now_ms();
        e.last_font_ch[3] = now_ms().wrapping_sub(1000);
        push_font(&mut e, 0x01);
        let params = landing_params();
        for _ in 0..2 {
            e.last_output = now_ms().wrapping_sub(60);
            e.tick(&params);
        }
        assert!(
            e.left > 0.02,
            "落地关闭时尾巴应自然衰减存在 (高于 0.02 硬地板): {}",
            e.left
        );
    }

    #[test]
    fn legacy_flush_pulse_not_killed_by_stale_peak() {
        /* 防回归: 补发 (FLUSH) 必须刷新峰值 —— 否则陈旧大 peak 把小收尾脉冲
         * 在落地门当帧误杀 (防能量蒸发失效) */
        let mut e = carry_engine(true);
        e.tail_land_pct = 25;
        e.density_active = true;
        e.last_combo = now_ms();
        /* 制造陈旧大 peak (0.9) */
        e.inject(0.9, 1.0, 1.0, 60.0, 48.0);
        assert!(e.tail_peak_l > 0.8);
        /* 清空当前输出, 挂一笔到期的小记账 */
        e.left = 0.0;
        e.right = 0.0;
        e.font_carry_s[3] = 0.10;
        e.font_carry_mode[3] = 0;
        e.font_carry_lr[3] = [1.0, 1.0];
        e.font_carry_until[3] = now_ms().wrapping_sub(1);
        e.last_output = now_ms().wrapping_sub(60);
        e.tick(&landing_params());
        assert!(
            e.left > 0.03,
            "补发脉冲应存活 (陈旧 peak 不得误杀): {}",
            e.left
        );
        assert_eq!(e.font_carry_s[3], 0.0);
    }

    #[test]
    fn s4_route_ignores_tail_landing() {
        /* 红线: 新方案无落地机制, 输出走自然衰减 */
        let mut n = carry_engine(false);
        n.tail_land_pct = 25;
        n.density_active = true;
        n.last_combo = now_ms();
        n.inject(0.9, 1.0, 1.0, 60.0, 48.0);
        let params = landing_params();
        for _ in 0..2 {
            n.last_output = now_ms().wrapping_sub(60);
            n.tick(&params);
        }
        assert!(
            n.left > 0.02,
            "新方案: 输出应按自然衰减推进, 不被强制清零: {}",
            n.left
        );
    }

    /* ── ★优先级阶梯 (v20): 高位归零后低位不得抬回 ── */

    /* 脉冲落地归零右马达后, 爆发保底不得把右马达抬回 (否则"脉冲间真静音"失效)。
     * 仅 S1 的 S4 模式 (1/2) 生效。红灯判据: 去掉 Burst 门的 hard_cut_r 检查,
     * right 会被地板抬到 1.0。 */
    #[test]
    fn legacy_hard_cut_suppresses_burst_floor() {
        let mut e = carry_engine(true);
        e.legacy_output_mode = 2;
        e.tail_land_pct = 50; /* 峰值 0.9 的 50% = 0.45, 首帧 0.258 即触发落地 */
        e.density_active = true;
        e.last_combo = now_ms();
        e.state = VibState::Burst;
        e.burst_until = now_ms().wrapping_add(800);
        e.last_event = now_ms(); /* 防 tick 的空闲判定把 Burst 打回 Idle */
        e.inject(0.9, 1.0, 1.0, 60.0, 48.0);
        let mut params = landing_params();
        params[P_BURST_MIN] = 30;
        params[P_FONT_ATTACK] = 65; /* 地板 = 0.30×0.65 = 0.195 (可观测, 低于落地线) */
        for _ in 0..2 {
            e.last_output = now_ms().wrapping_sub(60);
            e.tick(&params);
        }
        assert!(e.hard_cut_r, "落地应置硬归零标记");
        assert_eq!(
            e.right, 0.0,
            "落地归零后爆发保底不得抬回右马达 (优先级阶梯)"
        );
    }

    /* ★金标回归: 经典模式 0 的爆发保底维持 v19.7 行为 —— 落地归零后仍抬回
     * (ACT1 特调手感基线; 优先级阶梯只在 S4 模式生效)。 */
    #[test]
    fn legacy_mode0_burst_floor_still_applies_after_tail_land() {
        let mut e = carry_engine(true);
        assert_eq!(e.legacy_output_mode, 0, "默认必须是经典模式");
        e.tail_land_pct = 50;
        e.density_active = true;
        e.last_combo = now_ms();
        e.state = VibState::Burst;
        e.burst_until = now_ms().wrapping_add(800);
        e.last_event = now_ms(); /* 防 tick 的空闲判定把 Burst 打回 Idle */
        e.inject(0.9, 1.0, 1.0, 60.0, 48.0);
        let mut params = landing_params();
        params[P_BURST_MIN] = 30;
        params[P_FONT_ATTACK] = 65;
        for _ in 0..2 {
            e.last_output = now_ms().wrapping_sub(60);
            e.tick(&params);
        }
        assert!(e.hard_cut_r, "落地标记仍会置位 (只是模式 0 不消费)");
        assert!(
            e.right > 0.0,
            "经典模式: 爆发保底必须照旧抬回右马达 (v19.7 行为): {}",
            e.right
        );
    }

    /* 红线: S4 路径不参与优先级阶梯, 硬归零标记恒 false */
    #[test]
    fn s4_route_hard_cut_flags_never_set() {
        let mut n = carry_engine(false);
        n.tail_land_pct = 25;
        n.density_active = true;
        n.last_combo = now_ms();
        n.inject(0.9, 1.0, 1.0, 60.0, 48.0);
        n.last_output = now_ms().wrapping_sub(60);
        n.tick(&landing_params());
        assert!(
            !n.hard_cut_l && !n.hard_cut_r,
            "红线: S4 不得进入优先级阶梯"
        );
    }

    /* 输出平滑不得把硬归零的帧拖回非零 (仅 S1 的 S4 模式; 否则低通把静音尾巴变回嗡鸣) */
    #[test]
    fn legacy_smooth_out_resets_on_hard_cut() {
        let mut e = engine_with(true);
        e.legacy_output_mode = 2;
        e.out_smooth_l = 1000.0;
        e.out_smooth_r = 1000.0;
        e.hard_cut_l = true;
        e.hard_cut_r = true;
        let (l, r) = e.smooth_out(0.0, 0.0, 0.5);
        assert_eq!(
            (l, r),
            (0.0, 0.0),
            "硬归零帧: 平滑状态应直接复位, 不得拖尾"
        );
        /* 对照: 无硬归零时维持旧的一阶低通 (1000 → 500) */
        let mut n = engine_with(true);
        n.out_smooth_l = 1000.0;
        let (l2, _) = n.smooth_out(0.0, 0.0, 0.5);
        assert!((l2 - 500.0).abs() < 0.01, "无硬归零: 低通行为不变: {l2}");
    }

    /* ★金标回归: 经典模式 0 即使置了硬归零标记, 输出平滑也维持 v19.7 低通
     * (阶梯不消费), 保证 ACT1 特调手感不变。 */
    #[test]
    fn legacy_mode0_smooth_out_ignores_hard_cut() {
        let mut e = engine_with(true);
        assert_eq!(e.legacy_output_mode, 0);
        e.out_smooth_l = 1000.0;
        e.out_smooth_r = 1000.0;
        e.hard_cut_l = true;
        e.hard_cut_r = true;
        let (l, r) = e.smooth_out(0.0, 0.0, 0.5);
        assert!(
            (l - 500.0).abs() < 0.01 && (r - 500.0).abs() < 0.01,
            "经典模式: 平滑必须照旧插值 (v19.7): {l}/{r}"
        );
    }

    /* ── ★S1 输出引擎模式 (v20): 0 经典 / 1 S4 纯 / 2 S4+不丢 ── */

    /* 模式 1 (S4 纯): 低于死区的轻反馈被直接吞掉 = 丢震动 (A/B 对照) */
    #[test]
    fn legacy_mode1_s4_pure_drops_subthreshold() {
        let params = [100u32; 60];
        let mut e = engine_with(true);
        e.legacy_output_mode = 1;
        e.out_threshold = 30.0;
        e.remap_min = 0.0;
        e.left = 0.15;
        let (l, _) = e.finalize(&params);
        assert_eq!(l, 0, "模式 1: 低于死区直接丢弃 (S4 纯语义)");
        assert_eq!(e.shape_carry_l, 0.0, "模式 1 不启用后置携带");
    }

    /* 模式 2 (S4+不丢): 低于死区的能量不丢, 攒够阈值释放成厚脉冲 */
    #[test]
    fn legacy_mode2_carries_subthreshold_into_thicker_pulse() {
        let params = [100u32; 60];
        let mut e = engine_with(true);
        e.legacy_output_mode = 2;
        e.out_threshold = 30.0;
        e.remap_min = 0.0;
        e.right = 0.0;
        e.left = 0.15; /* 9830 < 阈值 19660 */
        let (l1, _) = e.finalize(&params);
        assert_eq!(l1, 0, "单次轻反馈: 本帧先攒着不输出");
        assert!(e.shape_carry_l > 0.0, "能量应进入后置携带而非被丢弃");
        let (l2, _) = e.finalize(&params);
        assert!(
            l2 as f32 >= 0.30 * 65535.0 * 0.99,
            "攒够阈值应释放成 ≥阈值 的厚脉冲: {l2}"
        );
        assert_eq!(e.shape_carry_l, 0.0, "释放后携带清零");
    }

    /* 模式 2 的能量守恒: 连续轻反馈不得成批蒸发 (攒厚再出的核心承诺) */
    #[test]
    fn legacy_mode2_carry_conserves_energy() {
        let params = [100u32; 60];
        let mut e = engine_with(true);
        e.legacy_output_mode = 2;
        e.out_threshold = 30.0;
        e.remap_min = 0.0; /* 关掉重映射抬底, 直接核对能量账 */
        e.right = 0.0;
        let mut fed = 0.0f64;
        let mut emitted = 0.0f64;
        for _ in 0..21 {
            e.left = 0.10; /* 6553.5 < 阈值 19660 → 全额携带 */
            fed += 0.10 * 65535.0;
            let (l, _) = e.finalize(&params);
            emitted += l as f64;
        }
        emitted += e.shape_carry_l as f64; /* 尚未释放的携带也算保留 */
        assert!(
            emitted >= fed * 0.95,
            "模式 2 能量不得蒸发: fed={fed:.0} emitted={emitted:.0}"
        );
        assert!(emitted > 0.0, "应至少释放出厚脉冲");
    }

    /* 红线: S4 路线不读 legacy_output_mode, 不启用 S1 后置携带 */
    #[test]
    fn s4_route_ignores_legacy_output_mode() {
        let params = [100u32; 60];
        let mut n = engine_with(false);
        n.legacy_output_mode = 2;
        n.out_threshold = 30.0;
        n.remap_min = 0.0;
        n.left = 0.15;
        let (l, _) = n.finalize(&params);
        assert_eq!(l, 0, "S4 走自己的迟滞死区");
        assert_eq!(n.shape_carry_l, 0.0, "红线: S4 不得启用 S1 后置携带");
    }

    /* ★合并降幅 (v20): 只在 S1 的 S4 模式 (1/2) 生效 (取强者一次);
     * 经典模式 0 维持 v19.7 逐项相乘 —— 这是 ACT1 特调手感基线, 必须锁定。 */
    #[test]
    fn legacy_adapt_sustain_merge_only_in_s4_mode() {
        fn mk(mode: u32, adapt: bool, sustain: bool) -> VibEngine {
            let mut e = carry_engine(true);
            e.legacy_output_mode = mode;
            e.merge_keep = 0;
            e.hitmerge_ms = 0;
            e.hitcap_max = if sustain { 1 } else { 0 };
            e.hitcap_win_ms = 1000;
            e.sustain_secs = if sustain { 1 } else { 0 };
            e.sustain_reduce = 20;
            e.p[P_ADAPT_REDUCE] = if adapt { 50.0 } else { 0.0 };
            e.p[P_ADAPT_THR] = if adapt { 100000.0 } else { 0.0 };
            e.p[P_ADAPT_MAXGAP] = 100000.0;
            e.p[P_ADAPT_FLOOR] = 0.0;
            e.last_font_ch[3] = now_ms().wrapping_sub(1000);
            push_font(&mut e, 0x01);
            e
        }
        /* S4 模式: 取强者 (min) 应用一次 */
        let a2 = mk(2, true, false).left;
        let s2 = mk(2, false, true).left;
        let b2 = mk(2, true, true).left;
        assert!(
            (b2 - a2.min(s2)).abs() < 1e-4,
            "S4 模式应取强者一次: b={b2} a={a2} s={s2}"
        );
        /* 经典模式 0: 逐项相乘 (b/a=0.8, b/s=0.5) */
        let a0 = mk(0, true, false).left;
        let s0 = mk(0, false, true).left;
        let b0 = mk(0, true, true).left;
        assert!(
            (b0 / a0 - 0.8).abs() < 1e-3 && (b0 / s0 - 0.5).abs() < 1e-3,
            "经典模式必须逐项相乘 (v19.7): b={b0} a={a0} s={s0}"
        );
    }

    #[test]
    fn legacy_monster_abnormal_split_from_effect() {
        /* 0x04 拆分: legacy 用独立 monster_abnormal 强度, S4 维持 p[15] */
        let mut e = carry_engine(true);
        e.monster_abnormal = 3;
        e.p[P_FONT_EFFECT] = 40.0;
        let (s, _, _, mode) = e.font_pick(FONT_EFFECT, 8);
        assert_eq!(s, 3.0, "legacy 0x04 应取怪物异常反馈强度");
        assert_eq!(mode, 4);
        let mut n = carry_engine(false);
        n.p[P_FONT_EFFECT] = 40.0;
        let (s2, _, _, mode2) = n.font_pick(FONT_EFFECT, 8);
        assert_eq!(s2, 40.0, "新方案 0x04 维持 p[15] (现行行为不变)");
        assert_eq!(mode2, 2);
    }

    /* ── ★S1 单帧脉冲 (v15.1, 衰减 0-5ms) + 总闸 L/R 回归 ── */

    #[test]
    fn legacy_instant_decay_single_frame_pulse() {
        let mut e = carry_engine(true);
        e.p[P_DEC_ATTACK] = 0.0; /* 0ms = 单帧脉冲 */
        e.tail_land_pct = 0; /* 与落地机制解耦, 单测单帧语义 */
        e.last_font_ch[3] = now_ms().wrapping_sub(1000);
        push_font(&mut e, 0x01);
        assert!(e.instant_armed, "0ms 衰减应布防单帧脉冲");
        assert!(e.left > 0.2, "注入帧: left={}", e.left);
        /* 第 1 帧 tick: 布防保持全幅直出 (一记全力的"顿") */
        e.tick(&landing_params());
        assert!(e.left > 0.2, "注入帧 tick 应全幅直出: {}", e.left);
        /* 第 2 帧 tick: 硬归零 (立即静音) */
        e.last_output = now_ms().wrapping_sub(60);
        e.tick(&landing_params());
        assert_eq!(e.left, 0.0, "单帧脉冲下一帧应硬归零");
    }

    #[test]
    fn gate_lr_scales_motors_independently() {
        /* 总闸 L/R (item_lr[0]/[1]): 左右马达独立微调 */
        let mut e = carry_engine(true);
        e.gate_lr_l = 0.5;
        e.gate_lr_r = 1.5;
        e.left = 0.245;
        e.right = 0.49;
        let params = [100u32; 60];
        let (l, r) = e.finalize(&params);
        let mut c = carry_engine(true);
        c.left = 0.245;
        c.right = 0.49;
        let (cl, cr) = c.finalize(&params);
        assert!(l < cl, "左闸 0.5 应压低左马达: {} vs {}", l, cl);
        assert!(r > cr, "右闸 1.5 应抬高右马达: {} vs {}", r, cr);
    }

    /* ── ★S1 命中聚合窗 (v15.2) 回归: 群怪一刀的多条命中事件只震一下 ── */

    #[test]
    fn legacy_hitmerge_aggregates_swing_burst() {
        let mut e = carry_engine(true);
        e.hitmerge_ms = 40;
        e.hitcap_max = 0; /* 与限频解耦, 单测聚合窗语义 */
        e.tail_land_pct = 0;
        /* 一刀砍中 3 怪: 事件错峰到达 (0 / +30 / +60ms, >20ms 旧窗拦不住) */
        e.last_font_ch[3] = now_ms().wrapping_sub(1000);
        push_font(&mut e, 0x01); /* 距锚 1000 ≥ 40 → 注入 */
        assert!(e.last_injected, "首怪正常注入");
        e.last_font_ch[3] = now_ms().wrapping_sub(30);
        push_font(&mut e, 0x01); /* 距锚 30 < 40 → 记账合并 */
        assert!(!e.last_injected, "聚合窗内第 2 怪不单独注入");
        assert!(e.font_carry_s[3] > 0.0, "能量记账不丢失");
        e.last_font_ch[3] = now_ms().wrapping_sub(60);
        push_font(&mut e, 0x01); /* 距锚 60 ≥ 40 → 兑现注入 (更厚) */
        assert!(e.last_injected, "窗满后兑现注入");
        assert_eq!(e.font_carry_s[3], 0.0, "兑现后记账清零");
        /* 对照: 聚合窗关闭 (10ms = 回到 20ms 行为) 同样节奏 → 每条都注入 */
        let mut c = carry_engine(true);
        c.hitmerge_ms = 10;
        c.hitcap_max = 0;
        c.last_font_ch[3] = now_ms().wrapping_sub(1000);
        push_font(&mut c, 0x01);
        c.last_font_ch[3] = now_ms().wrapping_sub(60);
        push_font(&mut c, 0x01); /* 距锚 60 ≥ 20 (旧窗) → 直接注入, 无合并 */
        assert!(c.last_injected, "聚合窗关闭: 旧行为 (每条注入)");
        assert!(
            e.left > c.left * 1.2,
            "聚合后的单发应比散开发更厚: {:.3} vs {:.3}",
            e.left,
            c.left
        );
    }

    /* ── ★S1 持续压制 (v15.3) 回归: 持续打满限速 → 命中自动再降一档 ── */

    #[test]
    fn legacy_sustained_suppression_after_full_rate() {
        let mut e = carry_engine(true);
        e.hitmerge_ms = 0;
        e.hitcap_max = 1;
        e.hitcap_win_ms = 1000;
        e.sustain_secs = 2;
        e.sustain_reduce = 40;
        /* inj1: sus 长窗启动, count=1 < 2 (限速1/s×2s) → 满强度 */
        e.last_font_ch[3] = now_ms().wrapping_sub(1000);
        push_font(&mut e, 0x01);
        assert!(e.last_injected);
        let peak = e.left;
        assert_eq!(e.sus_count, 1);
        /* inj2 (1 秒后): sus 窗内 count=2 = 限速上限×2 秒 → 降 40% */
        e.sus_start = now_ms().wrapping_sub(1000);
        e.hitcap_start = now_ms().wrapping_sub(1001); /* 滚动限频窗 → 放行 */
        e.last_font_ch[3] = now_ms().wrapping_sub(1000);
        push_font(&mut e, 0x01);
        assert!(
            e.left < peak * 0.65,
            "持续满载后命中应降档: {} vs {}",
            e.left,
            peak
        );
        /* inj3 (2.1 秒后): sus 长窗滚动重置 → 恢复满强度 */
        e.sus_start = now_ms().wrapping_sub(2100);
        e.hitcap_start = now_ms().wrapping_sub(1001);
        e.last_font_ch[3] = now_ms().wrapping_sub(1000);
        push_font(&mut e, 0x01);
        assert!(
            (e.left - peak).abs() < 1e-6,
            "降速后应恢复满强度: {}",
            e.left
        );
    }

    /* ── ★S1 命中风暴抽样 (v16) 回归 ── */

    #[test]
    fn legacy_storm_sampling_drops_excess_hits() {
        let mut e = carry_engine(true);
        e.hitmerge_ms = 0;
        e.hitcap_max = 0; /* 关闭限频, 隔离风暴 */
        e.storm_thr = 5;
        e.storm_keep_pct = 50;
        e.storm_win_ms = 1000;
        e.storm_pause_ms = 400;
        /* 8 条纯命中事件窗内到达: 1-4 条风暴未激活全注入;
         * 第 5 条入风暴 (seen=1 保留), 第 6 条 (seen=2) 丢弃,
         * 第 7 条 (seen=3) 保留, 第 8 条 (seen=4) 丢弃 → 共 6 次注入 */
        let mut injected = 0;
        for i in 0..8 {
            e.last_font_ch[3] = now_ms().wrapping_sub(100);
            push_font(&mut e, 0x01);
            if e.last_injected {
                injected += 1;
            }
            if i == 4 {
                /* 第 6 条被风暴丢弃: 不推进 IVL 锚点 (防通道饥饿)。
                 * 锚点断言取"拨锚后"的值: 拨锚本身会改写字段, 断言的是
                 * push 不得再前推它 */
                e.last_font_ch[3] = now_ms().wrapping_sub(100);
                let anchor = e.last_font_ch[3];
                push_font(&mut e, 0x01);
                assert!(!e.last_injected, "风暴期第 2 条应被抽样丢弃");
                assert_eq!(e.last_font_ch[3], anchor, "丢弃事件不得打 IVL 锚点");
                assert_eq!(e.font_carry_s[3], 0.0, "丢弃事件不得进聚合记账");
                break;
            }
        }
        assert_eq!(e.storm_active, true, "5 条应触发风暴");
        assert_eq!(injected, 5, "前 5 条 (含风暴首条) 应全部注入");
        /* 续 2 条: 保留 1 丢 1 */
        e.last_font_ch[3] = now_ms().wrapping_sub(100);
        push_font(&mut e, 0x01);
        assert!(e.last_injected, "seen=3 应保留");
        e.last_font_ch[3] = now_ms().wrapping_sub(100);
        push_font(&mut e, 0x01);
        assert!(!e.last_injected, "seen=4 应丢弃");
    }

    #[test]
    fn legacy_storm_pause_recovers_per_hit() {
        let mut e = carry_engine(true);
        e.hitmerge_ms = 0;
        e.hitcap_max = 0;
        e.storm_thr = 3;
        e.storm_keep_pct = 50;
        e.storm_pause_ms = 400;
        /* 3 条快速命中 → 风暴激活; 第 4 条被抽样丢弃 */
        for _ in 0..3 {
            e.last_font_ch[3] = now_ms().wrapping_sub(100);
            push_font(&mut e, 0x01);
        }
        assert!(e.storm_active);
        e.last_font_ch[3] = now_ms().wrapping_sub(100);
        push_font(&mut e, 0x01);
        assert!(!e.last_injected, "风暴期应丢弃");
        /* 停顿 500ms (> 400) → 恢复刀刀震: 本发必震且风暴退出 */
        e.storm_last_hit = now_ms().wrapping_sub(500);
        e.last_font_ch[3] = now_ms().wrapping_sub(100);
        push_font(&mut e, 0x01);
        assert!(e.last_injected, "停顿后第一击必震");
        assert!(!e.storm_active, "停顿后风暴应退出");
        /* 后续命中在阈值以下继续刀刀震 */
        e.last_font_ch[3] = now_ms().wrapping_sub(100);
        push_font(&mut e, 0x01);
        assert!(e.last_injected, "恢复后应刀刀震");
        assert_eq!(e.storm_active, false);
    }

    #[test]
    fn legacy_storm_off_thr0_keeps_old_behavior() {
        let mut e = carry_engine(true);
        e.hitmerge_ms = 0;
        e.hitcap_max = 0;
        e.storm_thr = 0; /* 关闭 */
        for _ in 0..12 {
            e.last_font_ch[3] = now_ms().wrapping_sub(100);
            push_font(&mut e, 0x01);
            assert!(e.last_injected, "thr=0 关闭态应逐条注入 (旧行为)");
        }
        assert!(!e.storm_active);
    }

    #[test]
    fn legacy_storm_excludes_wound_classes() {
        let mut e = carry_engine(true);
        e.hitmerge_ms = 0;
        e.hitcap_max = 0;
        e.storm_thr = 2;
        e.storm_keep_pct = 50;
        e.p[P_FONT_HIT] = 45.0; /* 受击通道默认 0, 测试需显式给强度 */
        /* 2 条命中触发风暴 */
        e.last_font_ch[3] = now_ms().wrapping_sub(100);
        push_font(&mut e, 0x01);
        e.last_font_ch[3] = now_ms().wrapping_sub(100);
        push_font(&mut e, 0x01);
        assert!(e.storm_active);
        let hits_before = e.storm_count;
        /* 受击 (0x02) 与玩家 DOT (0x20): 不计入风暴、也不被风暴丢弃 */
        e.last_font_ch[0] = now_ms().wrapping_sub(100);
        push_font(&mut e, 0x02);
        assert!(e.last_injected, "受伤类不受风暴影响 (受击)");
        e.last_font_ch[2] = now_ms().wrapping_sub(100);
        push_font(&mut e, 0x20);
        assert!(e.last_injected, "受伤类不受风暴影响 (DOT)");
        assert_eq!(e.storm_count, hits_before, "受伤类不得计入风暴计数");
    }

    #[test]
    fn new_route_storm_never_applies() {
        /* 红线: 新方案 (!legacy) 不进风暴分支, 行为与 v15.3 逐字节一致 */
        let mut e = carry_engine(false);
        e.storm_thr = 2;
        e.storm_keep_pct = 50;
        for _ in 0..6 {
            e.last_font = now_ms().wrapping_sub(100);
            push_font(&mut e, 0x01);
            assert!(e.last_injected, "新方案不受风暴抽样影响");
        }
        assert!(!e.storm_active);
    }

    /* ── ★v16.6 风暴期静音怪物异常 (可选开关) ── */

    #[test]
    fn legacy_storm_mutes_abnormal_feedback_when_enabled() {
        let mut e = carry_engine(true);
        e.hitmerge_ms = 0;
        e.hitcap_max = 0;
        e.monster_abnormal = 5;
        e.storm_thr = 2;
        e.storm_keep_pct = 100; /* 不抽样, 只验证静音门 */
        e.storm_mute_abnormal = true;
        /* 2 条命中触发风暴 */
        e.last_font_ch[3] = now_ms().wrapping_sub(100);
        push_font(&mut e, 0x01);
        e.last_font_ch[3] = now_ms().wrapping_sub(100);
        push_font(&mut e, 0x01);
        assert!(e.storm_active);
        /* 风暴期 0x04: 零痕迹丢弃 */
        e.last_font_ch[2] = now_ms().wrapping_sub(100);
        let anchor = e.last_font_ch[2];
        let dens = e.density_hits;
        push_font(&mut e, 0x04);
        assert!(!e.last_injected, "风暴期 0x04 应被静音");
        assert_eq!(e.last_font_ch[2], anchor, "静音事件不打注入窗戳");
        assert_eq!(e.density_hits, dens, "静音事件不计密度");
        /* 玩家 DOT (0x20) 不受影响 */
        e.last_font_ch[2] = now_ms().wrapping_sub(100);
        push_font(&mut e, 0x20);
        assert!(e.last_injected, "玩家 DOT 不受风暴静音影响");
        /* 停顿超过 pause → 自动恢复 (时间窗判定, 无需等下一发命中) */
        e.storm_last_hit = now_ms().wrapping_sub(600);
        e.last_font_ch[2] = now_ms().wrapping_sub(100);
        push_font(&mut e, 0x04);
        assert!(e.last_injected, "停手后 0x04 应恢复震动");
    }

    #[test]
    fn legacy_storm_mute_off_keeps_abnormal_feedback() {
        let mut e = carry_engine(true);
        e.hitmerge_ms = 0;
        e.hitcap_max = 0;
        e.monster_abnormal = 5;
        e.storm_thr = 2;
        e.storm_keep_pct = 100;
        e.storm_mute_abnormal = false; /* 默认关 = 旧行为 */
        e.last_font_ch[3] = now_ms().wrapping_sub(100);
        push_font(&mut e, 0x01);
        e.last_font_ch[3] = now_ms().wrapping_sub(100);
        push_font(&mut e, 0x01);
        assert!(e.storm_active);
        e.last_font_ch[2] = now_ms().wrapping_sub(100);
        push_font(&mut e, 0x04);
        assert!(e.last_injected, "开关关闭时风暴期 0x04 照常震动");
    }

    /* ── ★v16.7 风暴期静音评分点系统 (可选开关) ── */

    #[test]
    fn legacy_storm_mutes_rank_when_enabled() {
        let mut e = carry_engine(true);
        e.hitmerge_ms = 0;
        e.hitcap_max = 0;
        e.storm_thr = 2;
        e.storm_keep_pct = 100; /* 不抽样, 只验证静音门 */
        e.storm_mute_rank = true;
        /* 2 条命中触发风暴 */
        e.last_font_ch[3] = now_ms().wrapping_sub(100);
        push_font(&mut e, 0x01);
        e.last_font_ch[3] = now_ms().wrapping_sub(100);
        push_font(&mut e, 0x01);
        assert!(e.storm_active);
        let mut rank_gain = [0u32; 15];
        rank_gain[0] = 50; /* 评分点 */
        rank_gain[11] = 40; /* 移动 */
        rank_gain[14] = 50; /* 怪物死亡 */
        e.rank_level = 0.0;
        e.rank_full_until = 0;
        /* 评分等级脉冲 (VEV_RANKING) 静音 */
        push_etype(&mut e, VEV_RANKING, 5, &rank_gain, 100);
        assert!(!e.last_injected, "风暴期评分等级脉冲应静音");
        assert_eq!(e.rank_level, 0.0, "静音不得写入评分脉冲");
        /* 评分点 (细分事件) 静音 */
        push_etype(&mut e, VEV_KILLPOINT, 100, &rank_gain, 100);
        assert_eq!(e.rank_level, 0.0, "风暴期评分点应静音");
        /* 怪物死亡静音 */
        push_etype(&mut e, VEV_TARGET_DIE, 100, &rank_gain, 100);
        assert!(!e.last_injected, "风暴期怪物死亡应静音");
        assert_eq!(e.rank_level, 0.0);
        /* 移动 (VEV_MOVE) 不属于评分点系统, 不受影响 */
        e.move_level = 0.0;
        push_etype(&mut e, VEV_MOVE, 100, &rank_gain, 100);
        assert!(e.move_level > 0.0, "移动持续震动不受评分静音影响");
        /* 停顿超过 pause → 恢复 */
        e.storm_last_hit = now_ms().wrapping_sub(600);
        push_etype(&mut e, VEV_KILLPOINT, 100, &rank_gain, 100);
        assert!(e.rank_level > 0.0, "停手后评分事件应恢复");
    }

    #[test]
    fn legacy_storm_mute_rank_off_keeps_rank() {
        let mut e = carry_engine(true);
        e.hitmerge_ms = 0;
        e.hitcap_max = 0;
        e.storm_thr = 2;
        e.storm_keep_pct = 100;
        e.storm_mute_rank = false; /* 默认关 = 旧行为 */
        e.last_font_ch[3] = now_ms().wrapping_sub(100);
        push_font(&mut e, 0x01);
        e.last_font_ch[3] = now_ms().wrapping_sub(100);
        push_font(&mut e, 0x01);
        assert!(e.storm_active);
        let mut rank_gain = [0u32; 15];
        rank_gain[0] = 50;
        e.rank_level = 0.0;
        e.rank_full_until = 0;
        push_etype(&mut e, VEV_KILLPOINT, 100, &rank_gain, 100);
        assert!(e.rank_level > 0.0, "开关关闭时风暴期评分事件照常震动");
    }

    /* ── ★v16.8 风暴检测与抽样解耦 + 算法总开关参数覆盖 ── */

    #[test]
    fn legacy_storm_detection_survives_sampling_off() {
        let mut e = carry_engine(true);
        e.hitmerge_ms = 0;
        e.hitcap_max = 0;
        e.storm_thr = 2;
        e.storm_keep_pct = 50;
        e.storm_enabled = false; /* 抽样总开关关 */
        e.storm_mute_rank = true; /* 静音开 */
        let mut injected = 0;
        for _ in 0..6 {
            e.last_font_ch[3] = now_ms().wrapping_sub(100);
            push_font(&mut e, 0x01);
            if e.last_injected {
                injected += 1;
            }
        }
        assert!(e.storm_active, "抽样关闭时风暴检测仍应工作 (静音开关依赖它)");
        assert_eq!(injected, 6, "抽样关闭时命中不得被丢弃");
        /* 静音开关仍然有效 (修复前会被阈值/抽样开关掐死) */
        let mut rank_gain = [0u32; 15];
        rank_gain[0] = 50;
        e.rank_level = 0.0;
        e.rank_full_until = 0;
        push_etype(&mut e, VEV_KILLPOINT, 100, &rank_gain, 100);
        assert_eq!(e.rank_level, 0.0, "抽样关闭时风暴期静音仍应生效");
    }

    #[test]
    fn algo_toggle_params_override() {
        let mut p = [7u32; 60];
        apply_algo_toggle_params(&mut p, false, false, false, false, false, false);
        assert_eq!(p[29], 100, "密度自适应关 → 阈值 100");
        assert_eq!(p[51], 0, "命中自适应关 → 降幅 0");
        assert_eq!(p[21], 0, "走位能量关 → 速率 0");
        assert_eq!(
            [p[35], p[36], p[37], p[38]],
            [25, 55, 25, 60],
            "衰减组关 → 回内置默认衰减"
        );
        assert_eq!(
            [p[40], p[41], p[42], p[43], p[44], p[45], p[46]],
            [150, 90, 250, 20, 600, 1500, 3000],
            "窗口组关 → 回内置默认窗口"
        );
        assert_eq!(
            [p[54], p[55], p[56], p[57], p[58]],
            [0, 100, 0, 0, 0],
            "静默脉冲关 → 中性值"
        );
        assert_eq!(p[59], 7, "测试强度不受任何开关影响");
        let mut q = [7u32; 60];
        apply_algo_toggle_params(&mut q, true, true, true, true, true, true);
        assert_eq!([q[29], q[35], q[40], q[54]], [7, 7, 7, 7], "全开时参数不动");
    }

    /* ── ★v17 统合衰减期 (风暴期统一包络) ── */

    #[test]
    fn legacy_unified_decay_retriggers_and_hard_cuts() {
        let mut e = carry_engine(true);
        e.hitmerge_ms = 0;
        e.hitcap_max = 0;
        e.storm_thr = 2;
        e.storm_keep_pct = 50; /* 抽样本会丢一半 */
        e.storm_enabled = true;
        e.storm_unified_enabled = true;
        e.storm_unified_ms = 100;
        /* 2 条命中触发风暴 */
        e.last_font_ch[3] = now_ms().wrapping_sub(100);
        push_font(&mut e, 0x01);
        e.last_font_ch[3] = now_ms().wrapping_sub(100);
        push_font(&mut e, 0x01);
        assert!(e.storm_active);
        /* 统合模式: 每一击都重新起振 (抽样/限频让路) */
        let mut injected = 0;
        for _ in 0..6 {
            e.last_font_ch[3] = now_ms().wrapping_sub(100);
            push_font(&mut e, 0x01);
            if e.last_injected {
                injected += 1;
            }
        }
        assert_eq!(injected, 6, "统合衰减期下每一击都应重新起振 (不抽样)");
        assert!(e.unified_until != 0, "统合包络应已布防");
        assert!(e.left > 0.0, "重触发后应全幅起振");
        /* 到点硬归零 (旧的瞬间消失) */
        e.unified_until = now_ms().wrapping_sub(1);
        let params = [100u32; 60];
        e.tick(&params);
        assert_eq!(e.left, 0.0, "统合衰减期到点应硬归零");
        assert_eq!(e.right, 0.0);
        assert_eq!(e.unified_until, 0, "归零后清哨兵");
    }

    #[test]
    fn legacy_unified_decay_off_keeps_sampling() {
        let mut e = carry_engine(true);
        e.hitmerge_ms = 0;
        e.hitcap_max = 0;
        e.storm_thr = 2;
        e.storm_keep_pct = 50;
        e.storm_unified_enabled = false; /* 默认关 = 抽样照旧 */
        for _ in 0..2 {
            e.last_font_ch[3] = now_ms().wrapping_sub(100);
            push_font(&mut e, 0x01);
        }
        assert!(e.storm_active);
        let mut injected = 0;
        for _ in 0..4 {
            e.last_font_ch[3] = now_ms().wrapping_sub(100);
            push_font(&mut e, 0x01);
            if e.last_injected {
                injected += 1;
            }
        }
        assert_eq!(injected, 2, "统合关闭时应按 50% 抽样 (4 条留 2 条)");
        assert_eq!(e.unified_until, 0, "统合关闭时不应布防");
    }

    #[test]
    fn new_route_unified_never_applies() {
        let mut e = carry_engine(false);
        e.storm_unified_enabled = true;
        e.storm_active = true; /* 人为激活, 验证 legacy 门 */
        e.last_font = now_ms().wrapping_sub(100);
        push_font(&mut e, 0x01);
        assert_eq!(e.unified_until, 0, "新方案不参与统合衰减期");
    }

    /* ── ★v16.1: 0 强度事件零影响 (注入窗戳 / 密度统计) ── */

    #[test]
    fn legacy_zero_strength_events_leave_no_trace() {
        let mut e = carry_engine(true);
        e.hitmerge_ms = 0;
        e.hitcap_max = 0;
        e.monster_abnormal = 0; /* 怪物异常反馈关闭 */
        /* 0x04 (怪物出血, 强度 0): 不注入、不打通道戳、不计密度 */
        e.last_font_ch[2] = now_ms().wrapping_sub(100);
        let anchor = e.last_font_ch[2];
        push_font(&mut e, 0x04);
        assert!(!e.last_injected, "怪物异常=0 时出血事件不注入");
        assert_eq!(e.last_font_ch[2], anchor, "0 强度事件不得刷新注入窗");
        assert_eq!(e.density_hits, 0, "0 强度事件不得计密度");
        /* 出血洪流 (强度 0) 不得触发密度压制 */
        e.p[P_DENSITY_THR] = 45.0;
        for _ in 0..10 {
            e.last_font_ch[2] = now_ms().wrapping_sub(100);
            push_font(&mut e, 0x04);
        }
        assert!(!e.density_active, "出血洪流 (强度 0) 不得触发密度压制");
        /* 真实 DOT 不受影响: 正常注入 */
        e.last_font_ch[2] = now_ms().wrapping_sub(100);
        push_font(&mut e, 0x20);
        assert!(e.last_injected, "真实 DOT 不受 0 强度事件影响");
        /* 对照组: monster_abnormal>0 时 0x04 恢复注入+计数 */
        e.monster_abnormal = 5;
        e.density_hits = 0;
        e.density_active = false;
        e.density_win_start = 0;
        e.last_font_ch[2] = now_ms().wrapping_sub(100);
        push_font(&mut e, 0x04);
        assert!(e.last_injected, "怪物异常>0 时出血事件应注入");
        assert!(e.density_hits >= 1, "非 0 强度出血应计密度");
    }
}
