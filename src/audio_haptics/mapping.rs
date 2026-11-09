//! ★v24.60 音频震动 — 参数快照 + 响应曲线 + 预设 (纯函数, 全部可单测)。
//!
//! ★v24.60 算法换代: 删除 YAMNet/MRX AI 层后, 引擎改为"频带电平跟随"路线 ——
//! 骨架 (噪声底自适应阈值 + 内容三分类 + 攻击/释放/死区) 取自
//! xbox-audio-rumble-studio (作者已书面授权), 映射手感 (频带硬门槛 + 幂律压缩 +
//! 高低频敏感度) 取自 audio-to-controller-vibration (MIT)。两份算法都没抄代码,
//! 常数按同一量纲 (int16 尺度 RMS, 见 dsp::XSCALE) 平移调优。
//!
//! ## 量纲约定 (重要)
//! xbox6.py 在 int16 原始幅度上标定 (threshold 260~600, map_in_max 4200~6000);
//! 我们的采集链是归一化 f32 (-1..1)。引擎内部统一乘 XSCALE=32768 还原到 int16
//! 尺度, 因此 xbox6 的已调参数可以 1:1 平移, 不做任何换算。

use crate::state::AudioHapState;

/// 引擎参数快照 (引擎线程每 tick 从 AudioHapState 原子量搬运一次)。
#[derive(Debug, Clone)]
pub struct EngineParams {
    /// 全局总强度 % (0..=150)。即 xbox6 的 `intensity`。
    pub master: u32,
    /// 低频量感 % (0..=200) —— 左马达 (低频带) 的出力倍率。
    pub low: u32,
    /// 高频量感 % (0..=200) —— 右马达 (高频带) 的出力倍率。
    pub high: u32,
    /// ★打击对比度 0..=200 (幂律 gamma, [`punch_gamma`])。
    pub punch_pct: u32,
    /// 屏蔽门槛 (int16 尺度 RMS; 比这更小的声音不震)。
    pub mask_threshold: u32,
    /// 自适应底噪: 门槛随背景噪声自动抬高 (threshold = max(固定值, 噪声底×1.55))。
    pub adaptive_threshold: bool,
    /// 智能识别: 自动判断 音乐/人声/安静 段并调整手感 (xbox6 auto_profile)。
    pub auto_profile: bool,
    /// BGM 段压制 % (0..=100): 识别为音乐时, 连续震动额外压制的比例。
    pub music_mask_pct: u32,
    /// 安静段静音: 判定为安静时输出归零 (false = 保留轻量跟随)。
    pub quiet_mask: bool,
    /// 节奏力度 % (0..=100): BGM 段跟着鼓点震的强度 (xbox6 rhythm_punch)。
    pub rhythm_pct: u32,
    /// 细腻麻感 % (0..=100): 人声细节段的微颤纹理 (xbox6 texture_amount)。
    pub texture_pct: u32,
    /// 响应速度 5..=90 (xbox6 attack_rate ×100; 越大越跟手)。
    pub attack_pct: u32,
    /// 余韵 2..=60 (xbox6 release_rate ×100; 越大拖尾越长)。
    pub release_pct: u32,
    /// 死区 0..=20 (xbox6 dead_zone ×100; 低于此值的输出归零)。
    pub dead_zone_pct: u32,
    /// 满量程 (int16 尺度 RMS; 到这个响度算满格, xbox6 map_in_max)。
    pub map_max: u32,
    /// 双马达分频: 低频带→左马达, 高频带→右马达 (AudioVibration 路线)。
    /// false = 双马达同步 (xbox6 非 dual 模式)。
    pub dual_motor: bool,
    /// 只跟人声频段 (双马达关闭时生效; xbox6 vocal_only_mode)。
    pub vocal_only: bool,
    /// 低频带上限 Hz (20..bass_high 算"低频", xbox6 bass_high)。
    pub bass_high_hz: u32,
    /// 人声频段下限 Hz (xbox6 vocal_low)。
    pub vocal_low_hz: u32,
    /// 人声频段上限 Hz (xbox6 vocal_high)。
    pub vocal_high_hz: u32,
    /// 自动屏蔽校准截止时刻 (now_ms 口径; 0 = 不在校准中)。
    pub calibrate_until_ms: u64,
}

impl EngineParams {
    pub fn load(ah: &AudioHapState) -> Self {
        use std::sync::atomic::Ordering::Relaxed;
        Self {
            master: ah.master_gain.load(Relaxed),
            low: ah.low_gain.load(Relaxed),
            high: ah.high_gain.load(Relaxed),
            punch_pct: ah.punch_pct.load(Relaxed),
            mask_threshold: ah.mask_threshold.load(Relaxed),
            adaptive_threshold: ah.adaptive_threshold.load(Relaxed),
            auto_profile: ah.auto_profile.load(Relaxed),
            music_mask_pct: ah.music_mask_pct.load(Relaxed),
            quiet_mask: ah.quiet_mask.load(Relaxed),
            rhythm_pct: ah.rhythm_pct.load(Relaxed),
            texture_pct: ah.texture_pct.load(Relaxed),
            attack_pct: ah.attack_pct.load(Relaxed),
            release_pct: ah.release_pct.load(Relaxed),
            dead_zone_pct: ah.dead_zone_pct.load(Relaxed),
            map_max: ah.map_max.load(Relaxed),
            dual_motor: ah.dual_motor.load(Relaxed),
            vocal_only: ah.vocal_only.load(Relaxed),
            bass_high_hz: ah.bass_high_hz.load(Relaxed),
            vocal_low_hz: ah.vocal_low_hz.load(Relaxed),
            vocal_high_hz: ah.vocal_high_hz.load(Relaxed),
            calibrate_until_ms: ah.calibrate_until.load(Relaxed),
        }
    }
}

/// ★打击对比度 → 幂律指数 gamma (v24.59 引入, v24.60 语义不变)。
///
/// 曲线形式 `((rms-门槛)/(满量程-门槛))^gamma` (xbox6 `map_value_gamma`):
/// gamma<1 压缩 → 轻声也有明显反馈 ("招招有震动"); gamma>1 扩张 → 只有重击才震。
///
/// | punch_pct | gamma | 体感 |
/// |---|---|---|
/// | 0   | 0.45 | 强压缩: 几乎所有过门槛的声音都顶到高输出 |
/// | 82  | 0.90 | 默认 (用户实机调校值): 轻微压缩, 招招有震 |
/// | 100 | 1.00 | 线性 |
/// | 146 | 1.55 | 强扩张: 只有真正的重击才明显 |
pub fn punch_gamma(pct: u32) -> f32 {
    let p = pct.min(200) as f32 / 100.0;
    if p <= 1.0 {
        0.45 + p * 0.55
    } else {
        1.0 + (p - 1.0) * 1.2
    }
}

/// xbox6 的响应映射 (逐字平移): 门槛以下归零, 门槛→满量程 线性归一后取幂。
pub fn map_value_gamma(value: f32, threshold: f32, in_max: f32, gamma: f32) -> f32 {
    if value <= threshold {
        return 0.0;
    }
    let span = (in_max - threshold).max(1.0);
    let normalized = ((value - threshold) / span).clamp(0.0, 1.0);
    normalized.powf(gamma.max(0.1))
}

/// xbox6 的攻/放平滑 (逐字平移): 上升用 attack、下降用 release (0..1 的速率)。
pub fn smooth_signal(current: f32, target: f32, attack: f32, release: f32) -> f32 {
    let rate = if target > current { attack } else { release };
    current + (target - current) * rate.clamp(0.0, 1.0)
}

/// 内容分类的模式编号 (GUI/遥测/日志共用)。
pub const MODE_NORMAL: u8 = 0; // 游戏/普通声音
pub const MODE_MUSIC: u8 = 1; // BGM
pub const MODE_VOCAL: u8 = 2; // 人声/细节 (ASMR)
pub const MODE_QUIET: u8 = 3; // 安静

/// 内容三分类 (xbox6 `classify_content` 逐字平移, 量纲 int16 尺度)。
/// 返回 [`MODE_NORMAL`] / [`MODE_MUSIC`] / [`MODE_VOCAL`] / [`MODE_QUIET`]。
pub fn classify_content(
    total_rms: f32,
    vocal_rms: f32,
    bass_rms: f32,
    transient_avg: f32,
    previous_mode: u8,
) -> u8 {
    if total_rms < 55.0 {
        return MODE_QUIET;
    }
    let total = total_rms.max(1.0);
    let bass_ratio = bass_rms / total;
    let vocal_ratio = vocal_rms / total;
    let transient = transient_avg.clamp(0.0, 1.0);

    let music_score = bass_ratio * 1.25 + transient * 1.65;
    let asmr_score = vocal_ratio * 1.15
        + (1.0 - (transient * 2.4).min(1.0)) * 0.55
        - bass_ratio * 0.45;

    let (music_score, asmr_score) = match previous_mode {
        MODE_MUSIC => (music_score + 0.12, asmr_score),
        MODE_VOCAL => (music_score, asmr_score + 0.12),
        _ => (music_score, asmr_score),
    };

    if music_score > asmr_score + 0.16 {
        MODE_MUSIC
    } else if asmr_score > music_score + 0.10 {
        MODE_VOCAL
    } else if previous_mode == MODE_MUSIC || previous_mode == MODE_VOCAL {
        previous_mode
    } else {
        MODE_VOCAL
    }
}

/// 模式编号 → 中文名 (GUI/日志共用)。
pub fn mode_name(mode: u8) -> &'static str {
    match mode {
        MODE_MUSIC => "BGM 段",
        MODE_VOCAL => "人声/细节段",
        MODE_QUIET => "安静段",
        _ => "游戏段",
    }
}

/* ═══════════════ 预设 (一键手感; 字段与 AudioHapticsConfig 同名) ═══════════════ */

/// 一个预设 = 一组可直接写进 config 的参数值。
#[derive(Debug, Clone, PartialEq)]
pub struct Preset {
    pub name: &'static str,
    pub master_gain_pct: u32,
    pub low_gain_pct: u32,
    pub high_gain_pct: u32,
    pub punch_pct: u32,
    pub mask_threshold: u32,
    pub adaptive_threshold: bool,
    pub auto_profile: bool,
    pub music_mask_pct: u32,
    pub quiet_mask: bool,
    pub rhythm_pct: u32,
    pub texture_pct: u32,
    pub attack_pct: u32,
    pub release_pct: u32,
    pub dead_zone_pct: u32,
    pub map_max: u32,
    pub dual_motor: bool,
    pub vocal_only: bool,
    pub bass_high_hz: u32,
    pub vocal_low_hz: u32,
    pub vocal_high_hz: u32,
}

/// 全部内置预设 (顺序即 UI 展示顺序; 第一项 = 默认推荐档)。
///
/// ★v24.61 排序按 Owner 实测结论调整: 「人声清晰」「轻柔细腻」两档实测最好,
/// **打游戏也优先推荐**, 故提到最前; 原主档「游戏强劲」往后排。
/// 末尾追加「测试版(全0)」(总强度 0% = 引擎在跑但完全不震, 对齐震动区的全0测试档)。
pub const PRESETS: &[Preset] = &[
    Preset {
        name: "人声清晰",
        // ★v24.61 默认推荐档 (Owner 实测最佳): 只跟人声频段, BGM/环境音几乎不进,
        // 打游戏/看剧情都干净利落。数值来自 xbox6「人声清晰」预设。
        master_gain_pct: 72,
        low_gain_pct: 100,
        high_gain_pct: 100,
        punch_pct: 123,
        mask_threshold: 260,
        adaptive_threshold: true,
        auto_profile: false,
        music_mask_pct: 40,
        quiet_mask: true,
        rhythm_pct: 0,
        texture_pct: 30,
        attack_pct: 38,
        release_pct: 12,
        dead_zone_pct: 5,
        map_max: 4200,
        dual_motor: false,
        vocal_only: true,
        bass_high_hz: 520,
        vocal_low_hz: 650,
        vocal_high_hz: 3600,
    },
    Preset {
        name: "轻柔细腻",
        // ★v24.61 Owner 实测次佳: 强扩张曲线 + 微颤纹理, 轻声内容也有细腻反馈。
        master_gain_pct: 52,
        low_gain_pct: 100,
        high_gain_pct: 100,
        punch_pct: 146,
        mask_threshold: 340,
        adaptive_threshold: true,
        auto_profile: true,
        music_mask_pct: 40,
        quiet_mask: true,
        rhythm_pct: 30,
        texture_pct: 50,
        attack_pct: 24,
        release_pct: 8,
        dead_zone_pct: 6,
        map_max: 4800,
        dual_motor: false,
        vocal_only: true,
        bass_high_hz: 480,
        vocal_low_hz: 700,
        vocal_high_hz: 3300,
    },
    Preset {
        name: "游戏强劲",
        // 打击感档: 阈值/满量程/曲线/频带 来自用户实机调校 (XboxAudioRumbleStudio.settings.json);
        // 屏蔽层全开 (智能识别 + 自适应底噪 + BGM 压制 60%)。
        master_gain_pct: 67,
        low_gain_pct: 100,
        high_gain_pct: 100,
        punch_pct: 82,
        mask_threshold: 400,
        adaptive_threshold: true,
        auto_profile: true,
        music_mask_pct: 60,
        quiet_mask: true,
        rhythm_pct: 66,
        texture_pct: 20,
        attack_pct: 75,
        release_pct: 10,
        dead_zone_pct: 4,
        map_max: 6000,
        dual_motor: true,
        vocal_only: false,
        bass_high_hz: 750,
        vocal_low_hz: 600,
        vocal_high_hz: 4200,
    },
    Preset {
        name: "低频沉浸",
        // xbox6「低频沉浸」预设: 双马达主低频, 看电影/大场面用。
        master_gain_pct: 86,
        low_gain_pct: 120,
        high_gain_pct: 80,
        punch_pct: 104,
        mask_threshold: 220,
        adaptive_threshold: true,
        auto_profile: false,
        music_mask_pct: 20,
        quiet_mask: true,
        rhythm_pct: 80,
        texture_pct: 10,
        attack_pct: 46,
        release_pct: 16,
        dead_zone_pct: 4,
        map_max: 5200,
        dual_motor: true,
        vocal_only: false,
        bass_high_hz: 640,
        vocal_low_hz: 600,
        vocal_high_hz: 4000,
    },
    Preset {
        name: "全频强劲",
        // xbox6「全频强劲」预设: 不做任何屏蔽, 所有声音直接跟 (AudioVibration 裸手感)。
        master_gain_pct: 100,
        low_gain_pct: 100,
        high_gain_pct: 100,
        punch_pct: 82,
        mask_threshold: 180,
        adaptive_threshold: false,
        auto_profile: false,
        music_mask_pct: 0,
        quiet_mask: false,
        rhythm_pct: 0,
        texture_pct: 0,
        attack_pct: 58,
        release_pct: 22,
        dead_zone_pct: 4,
        map_max: 6000,
        dual_motor: true,
        vocal_only: false,
        bass_high_hz: 700,
        vocal_low_hz: 600,
        vocal_high_hz: 4200,
    },
    Preset {
        name: "测试版(全0)",
        // ★v24.61 对齐震动区「测试版(全0)」: 总强度 0% → 引擎在跑但完全不震。
        // 用途: 验证"不震是音频引擎关了还是手柄/驱动问题" (自检仍会震 = 马达链正常)。
        master_gain_pct: 0,
        low_gain_pct: 100,
        high_gain_pct: 100,
        punch_pct: 123,
        mask_threshold: 260,
        adaptive_threshold: true,
        auto_profile: false,
        music_mask_pct: 40,
        quiet_mask: true,
        rhythm_pct: 0,
        texture_pct: 30,
        attack_pct: 38,
        release_pct: 12,
        dead_zone_pct: 5,
        map_max: 4200,
        dual_motor: false,
        vocal_only: true,
        bass_high_hz: 520,
        vocal_low_hz: 650,
        vocal_high_hz: 3600,
    },
];

/// 受保护的内置预设名 (不允许被自定义保存覆盖/删除; 全部内置名都受保护 ——
/// 内置预设永远在代码里, 想改手感请另存新名字)。
pub fn is_protected_preset(name: &str) -> bool {
    PRESETS.iter().any(|p| p.name == name)
}

/// 按名字取预设 (找不到 → None)。
pub fn preset_by_name(name: &str) -> Option<&'static Preset> {
    PRESETS.iter().find(|p| p.name == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn punch_gamma_endpoints_match_doc_table() {
        assert!((punch_gamma(0) - 0.45).abs() < 1e-6);
        assert!((punch_gamma(100) - 1.0).abs() < 1e-6);
        assert!((punch_gamma(200) - 2.20).abs() < 1e-6);
        assert!((punch_gamma(82) - 0.901).abs() < 0.01, "默认档 ≈0.90");
        assert!((punch_gamma(146) - 1.552).abs() < 0.01);
    }

    #[test]
    fn map_value_gamma_matches_xbox6_reference() {
        // 与 xbox6.py map_value_gamma 的数值一致性 (手算采样点):
        // value=2100, thr=400, in_max=6000, gamma=0.9:
        // normalized = (2100-400)/5600 = 0.30357; 0.30357^0.9 = e^(0.9·ln 0.30357) ≈ 0.3420
        let v = map_value_gamma(2100.0, 400.0, 6000.0, 0.9);
        assert!((v - 0.3420).abs() < 0.002, "实际 {v}");
        assert_eq!(map_value_gamma(400.0, 400.0, 6000.0, 0.9), 0.0, "门槛值归零");
        assert_eq!(map_value_gamma(100.0, 400.0, 6000.0, 0.9), 0.0, "门槛以下归零");
        let full = map_value_gamma(60000.0, 400.0, 6000.0, 0.9);
        assert!((full - 1.0).abs() < 1e-6, "超过满量程截为 1");
    }

    #[test]
    fn smooth_signal_uses_attack_when_rising_release_when_falling() {
        let up = smooth_signal(0.0, 1.0, 0.5, 0.1);
        assert!((up - 0.5).abs() < 1e-6, "上升走 attack");
        let down = smooth_signal(1.0, 0.0, 0.5, 0.1);
        assert!((down - 0.9).abs() < 1e-6, "下降走 release");
    }

    #[test]
    fn classify_content_matches_xbox6_branches() {
        // 安静: total < 55
        assert_eq!(classify_content(30.0, 10.0, 5.0, 0.0, 0), MODE_QUIET);
        // 鼓点型 (低频占比大 + 爆发度高) → 音乐
        // music = (2400/3000)*1.25 + 0.8*1.65 = 1.0 + 1.32 = 2.32
        // asmr  = (500/3000)*1.15 + (1-1)*0.55 - 0.8*0.45 = 0.192 - 0.36 = -0.168
        let m = classify_content(3000.0, 500.0, 2400.0, 0.8, MODE_NORMAL);
        assert_eq!(m, MODE_MUSIC, "低频+爆发 → 音乐");
        // 平滑人声 (人声占比大, 无爆发) → 人声细节
        // music = (60/800)*1.25 + 0.02*1.65 = 0.094 + 0.033 = 0.127
        // asmr  = (700/800)*1.15 + (1-0.048)*0.55 - 0.075*0.45 = 1.0125 + 0.524 - 0.034 = 1.50
        let v = classify_content(800.0, 700.0, 60.0, 0.02, MODE_NORMAL);
        assert_eq!(v, MODE_VOCAL, "人声主导+无爆发 → 人声细节");
        // 滞回: 上一拍是音乐, 分数接近时保持音乐
        let keep = classify_content(800.0, 600.0, 300.0, 0.3, MODE_MUSIC);
        assert_eq!(keep, MODE_MUSIC, "滞回应保持音乐态");
    }

    #[test]
    fn presets_cover_all_names_and_defaults_are_tuned() {
        assert_eq!(PRESETS.len(), 6, "5 档手感 + 测试版(全0)");
        for p in PRESETS {
            assert!(!p.name.is_empty());
            assert!(p.mask_threshold <= 1200 && p.map_max >= 1000);
            assert!(
                p.bass_high_hz <= p.vocal_high_hz,
                "预设 {} 频带配置颠倒",
                p.name
            );
        }
        // ★v24.61: 第一项 = 默认推荐档 (Owner 实测「人声清晰」最好, 打游戏也优先推荐)
        assert_eq!(PRESETS[0].name, "人声清晰");
        assert!(is_protected_preset("人声清晰") && is_protected_preset("测试版(全0)"));
        assert!(!is_protected_preset("我的手感"));
        let g = preset_by_name("游戏强劲").expect("打击感档存在");
        assert_eq!(g.mask_threshold, 400, "门槛 = 用户实机调校值");
        assert!(g.adaptive_threshold && g.auto_profile, "该档开屏蔽");
        // 全0测试档: 总强度 0% → 引擎在跑但完全不震
        let z = preset_by_name("测试版(全0)").expect("全0档存在");
        assert_eq!(z.master_gain_pct, 0, "全0档总强度必须为 0");
    }
}
