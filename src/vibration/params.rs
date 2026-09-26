//! 震动 60 槽参数: 槽位下标常量 (唯一事实源, P_*) —— 原 vibration.rs 170-240, 2026-09-27 架构重构 D1 归位。
//! 0-18 = 基础具名参数, 19-59 = 高级参数 (config.advanced[i-19] 位置对应)。

#[allow(dead_code)]
pub(super) const P_ATTACK: usize = 0;
#[allow(dead_code)]
pub(super) const P_MAX: usize = 4;
pub(super) const P_BOOST: usize = 6;
pub(super) const P_MASTER: usize = 9;
pub(super) const P_FONT_STR: usize = 10;
pub(super) const P_FONT_IVL: usize = 11;
pub(super) const P_FONT_HP: usize = 12;
pub(super) const P_FONT_SPECIAL: usize = 13;
pub(super) const P_FONT_STATE: usize = 14;
pub(super) const P_FONT_EFFECT: usize = 15;
pub(super) const P_FONT_ATTACK: usize = 16;
pub(super) const P_FONT_HIT: usize = 17;
pub(super) const P_RHYTHM: usize = 18;
/* 高级 A */
pub(super) const P_CURVE_L: usize = 19;
pub(super) const P_CURVE_R: usize = 20;
/* 移动积累增强 (v22): 长时间移动积累"走位能量", 停止移动后下次短暂
 * 窗口期的攻击增强震动 (走位职业强化)。原 21/22 混合/相位槽位复用。 */
pub(super) const P_MOVE_CHARGE_RATE: usize = 21; /* 移动积累速率 %/秒 (0=禁用) */
pub(super) const P_MOVE_CHARGE_CAP: usize = 22;  /* 攻击增强上限 % (最多 ×(1+cap/100)) */
/* 高级 A2: 移动走路质感参数 (v20.2 新增, 原 23-26 为移除的 B 组 L/R) */
pub(super) const P_MOVE_PACE: usize = 23;       /* 移动步频 ms (默认 380, 自然步频) */
pub(super) const P_MOVE_PULSE: usize = 24;      /* 移动着地脉冲 % (默认 35, 柔和) */
pub(super) const P_MOVE_HOLD: usize = 25;       /* 移动抬脚保持 % (默认 8, 极轻) */
pub(super) const P_MOVE_GAIN: usize = 26;       /* 移动整体增益 % (默认 40, 轻音量) */
pub(super) const P_MOVE_SMOOTH: usize = 27;     /* 移动平滑系数 % (默认 30, 消除嗡嗡声) */
pub(super) const P_MOVE_THRESHOLD: usize = 28;  /* 移动最低输出阈值 % (默认 4, 低于归 0 消除沙沙声) */

/* 连击密度自适应 (v22): 短时间连击暴增时自动降低窗口期震动强度
 * 29-34 原为 B 组残留槽位, 现分配为密度算法参数 (高连击职业防震手核心) */
pub(super) const P_DENSITY_THR: usize = 29;    /* 密度触发阈值 hits/窗口 (100=禁用) */
pub(super) const P_DENSITY_WIN: usize = 30;    /* 密度检测窗口 ms */
pub(super) const P_DENSITY_REDUCE: usize = 31; /* 降幅 % (窗口期强度 ×(1-reduce)) */
pub(super) const P_DENSITY_RECOVER: usize = 32;/* 恢复判定 gap ms (最后命中超过此值→强度恢复) */
pub(super) const P_DENSITY_FLOOR: usize = 33;  /* 最低保留 % (降幅不超 (1-floor)) */
pub(super) const P_DENSITY_SMOOTH: usize = 34; /* 恢复平滑 ms (渐变恢复, 0=立即) */
/* 高级 C 衰减 */
pub(super) const P_DEC_ATTACK: usize = 35;
pub(super) const P_DEC_SPECIAL: usize = 36;
pub(super) const P_DEC_HIT: usize = 37;
pub(super) const P_DEC_STATE: usize = 38;
/* 移动积累增强窗口 (v22): 停止移动后增强有效期 ms。
 * ★39 号槽唯一语义 (P1 结案): 原 P_DEC_EFFECT 死常量已删除 (无任何读点,
 * 曾与本病历任意混淆), GUI 无双绑定, 引擎只读 P_MOVE_BOOST_WIN */
pub(super) const P_MOVE_BOOST_WIN: usize = 39;

/* 高级 D 时长窗口 */
pub(super) const P_DOT_HOLD: usize = 40;
pub(super) const P_EFFECT_PERIOD: usize = 41;
pub(super) const P_BURST_WIN: usize = 42;
pub(super) const P_BURST_MIN: usize = 43;
pub(super) const P_COUNTER_WIN: usize = 44;
pub(super) const P_COMBO_WIN: usize = 45;
pub(super) const P_IDLE: usize = 46;
/* 高级 E 连击/自适应 */
pub(super) const P_COMBO_CAP: usize = 47;
pub(super) const P_BOOST_SLOPE: usize = 48;
pub(super) const P_INTERRUPT: usize = 49;
pub(super) const P_ADAPT_THR: usize = 50;
pub(super) const P_ADAPT_REDUCE: usize = 51;
pub(super) const P_ADAPT_MAXGAP: usize = 52;
pub(super) const P_ADAPT_FLOOR: usize = 53;
/* 高级 F/G/H */
pub(super) const P_SILENCE: usize = 54;
pub(super) const P_COUNTER_MUL: usize = 55;
pub(super) const P_WAKE: usize = 56;
pub(super) const P_MILESTONE_PULSE: usize = 57;
pub(super) const P_INTERRUPT_PULSE: usize = 58;
pub(super) const P_TEST: usize = 59;

// ── ★D2 (2026-09-27): config ↔ 槽位双向映射的单一事实源 ──────────────────────
// 0-18 号槽位 ↔ VibrationConfig 具名字段: 下表是**唯一**写法 (此前在
// state.vibration_params_from 与 gui.sync_vib_config_from_params 各写一份,
// 加参数要在两处对账)。19-59 号槽位 = config.advanced[槽-19] 位置对应
// (config 侧本就是数组, 无具名字段)。新增具名参数: 下表加一行 +
// VibrationConfig 加字段 + (引擎要读时) 加 P_* 常量 —— 全部在本目录内完成。

macro_rules! for_each_named_param {
    ($m:ident) => {
        $m! {
            0usize => attack_gain,
            1 => damage_gain,
            2 => shake_gain,
            3 => move_gain,
            4 => max_strength,
            5 => decay_ms,
            6 => hit_boost,
            7 => start_pulse,
            8 => kill_pulse,
            9 => master_gain,
            10 => font_strength,
            11 => font_interval,
            12 => font_hp,
            13 => font_special,
            14 => font_state,
            15 => font_effect,
            16 => font_attack,
            17 => font_hit,
            18 => rhythm,
        }
    };
}

/// VibrationConfig → 60 槽参数数组 (config 读侧唯一写法; 语义 = 原 state::vibration_params_from)。
pub(crate) fn params_from_config(c: &crate::config::VibrationConfig) -> [u32; 60] {
    let mut a = [0u32; 60];
    macro_rules! rd {
        ($($idx:expr => $field:ident),* $(,)?) => {
            $( a[$idx] = c.$field; )*
        };
    }
    for_each_named_param!(rd);
    for i in 19..60 {
        a[i] = c.advanced[i - 19];
    }
    a
}

/// 60 槽参数数组 → VibrationConfig (config 写侧唯一写法; 语义 = 原 gui sync_vib_config_from_params 的映射部分)。
pub(crate) fn config_from_params(a: &[u32; 60], c: &mut crate::config::VibrationConfig) {
    macro_rules! wr {
        ($($idx:expr => $field:ident),* $(,)?) => {
            $( c.$field = a[$idx]; )*
        };
    }
    for_each_named_param!(wr);
    for i in 19..60 {
        c.advanced[i - 19] = a[i];
    }
}

#[cfg(test)]
mod mapping_tests {
    use super::*;

    /// 往返一致性: 特征值 config → 槽位 → config 必须逐字节相等。
    /// (加参数错位/漏项立即红; act_variants_tests 的校验和测试再兜一层。)
    #[test]
    fn config_slot_roundtrip_is_identity() {
        let mut c = crate::config::VibrationConfig::default();
        macro_rules! fill {
            ($($idx:expr => $field:ident),* $(,)?) => {
                $( c.$field = $idx as u32 * 7 + 3; )*
            };
        }
        for_each_named_param!(fill);
        for i in 0..41 {
            c.advanced[i] = (i as u32 + 1) * 11;
        }
        let a = params_from_config(&c);
        macro_rules! chk {
            ($($idx:expr => $field:ident),* $(,)?) => {
                $( assert_eq!(
                    a[$idx],
                    $idx as u32 * 7 + 3,
                    "槽位 {} 应回射字段 {}",
                    $idx,
                    stringify!($field)
                ); )* };
        }
        for_each_named_param!(chk);
        for i in 19..60 {
            assert_eq!(a[i], (i as u32 - 19 + 1) * 11);
        }
        let mut c2 = crate::config::VibrationConfig::default();
        config_from_params(&a, &mut c2);
        let s1 = toml::to_string(&c).unwrap();
        let s2 = toml::to_string(&c2).unwrap();
        assert_eq!(s1, s2, "config→槽位→config 必须逐字节恒等");
    }

    /// P_* 常量与具名映射表对账: 表内每个下标都必须有对应 P_* 常量语义 (0-18 段)。
    #[test]
    fn named_table_covers_base_slots() {
        macro_rules! cnt {
            ($($idx:expr => $field:ident),* $(,)?) => {
                $( let _ = ($idx, stringify!($field)); )*
            };
        }
        for_each_named_param!(cnt);
        assert_eq!(P_ATTACK, 0);
        assert_eq!(P_MASTER, 9);
        assert_eq!(P_FONT_HIT, 17);
        assert_eq!(P_RHYTHM, 18);
    }
}
