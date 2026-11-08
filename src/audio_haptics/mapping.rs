//! ★v24.50 音频震动 — 触发规则 + 纯净反馈门控 (纯函数, 全部可单测)。
//!
//! 规则表 = 策划案 §3.3 默认映射 (首发值, 实机调参为准);
//! 语义覆盖 = YAMNet 标签命中 (Gunshot/Explosion) 时改用专属震感。

use crate::state::{AudioHapState, SemSnapshot};

/// 引擎参数快照 (引擎线程每 tick 从 AudioHapState 原子量搬运一次)。
#[derive(Debug, Clone)]
pub struct EngineParams {
    pub master: u32,
    pub low: u32,
    pub high: u32,
    /// onset 灵敏度 k×10 (10..=80 → k=1..=8)
    pub sensitivity_k10: u32,
    pub release_ms: u32,
    pub sustain: u32,
    pub silence: u32,
    pub max_per_sec: u32,
    pub gate_music: u32,
    pub gate_speech: u32,
}

impl EngineParams {
    pub fn load(ah: &AudioHapState) -> Self {
        use std::sync::atomic::Ordering::Relaxed;
        Self {
            master: ah.master_gain.load(Relaxed),
            low: ah.low_gain.load(Relaxed),
            high: ah.high_gain.load(Relaxed),
            sensitivity_k10: ah.onset_sensitivity.load(Relaxed),
            release_ms: ah.onset_release_ms.load(Relaxed),
            sustain: ah.sustain_gain.load(Relaxed),
            silence: ah.silence_threshold.load(Relaxed),
            max_per_sec: ah.max_onsets_per_sec.load(Relaxed),
            gate_music: ah.gate_music.load(Relaxed),
            gate_speech: ah.gate_speech.load(Relaxed),
        }
    }
}

/// onset → 震感规则输出 (L/R 马达占比 0..1 + 时长)。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RuleOut {
    pub l: f32,
    pub r: f32,
    pub release_ms: u32,
}

/// onset 规则: 语义覆盖 > 频带占比 (策划案 §3.3)。
pub fn onset_rule(bands: &[f32; 5], sem: &SemSnapshot, sem_fresh: bool, p: &EngineParams) -> RuleOut {
    let total = bands.iter().sum::<f32>().max(1e-6);
    let low_share = (bands[0] + bands[1]) / total;
    let high_share = (bands[3] + bands[4]) / total;

    let mut rule = if sem_fresh && sem.gun >= 0.5 {
        // 语义: 枪声 → 重枪 (R 85% 60ms + L 30%)
        RuleOut { l: 0.30, r: 0.85, release_ms: 60 }
    } else if sem_fresh && sem.explosion >= 0.5 {
        // 语义: 爆炸 → 重爆 (L 90% 150ms)
        RuleOut { l: 0.90, r: 0.20, release_ms: 150 }
    } else if low_share >= 0.5 {
        // 爆炸/重砸: 低+超低占比过半 → 左马达主沉闷脉冲
        RuleOut { l: 0.90, r: 0.40, release_ms: p.release_ms.max(120) }
    } else if high_share >= 0.6 {
        // 枪声/打击: 中+高占比过 60% → 右马达主脆响短脉冲
        RuleOut { l: 0.30, r: 0.85, release_ms: 60 }
    } else {
        // 通用响声
        RuleOut { l: 0.60, r: 0.70, release_ms: 80 }
    };
    rule.l = (rule.l * p.low as f32 / 100.0 * p.master as f32 / 100.0).min(1.0);
    rule.r = (rule.r * p.high as f32 / 100.0 * p.master as f32 / 100.0).min(1.0);
    rule
}

/// 纯净反馈门控系数 (0..1): Music/Speech 置信度 >60% 时按用户档位压制快通道。
pub fn gate_factor(sem: &SemSnapshot, sem_fresh: bool, gate_music_pct: u32, gate_speech_pct: u32) -> f32 {
    let mut f = 1.0f32;
    if sem_fresh {
        if sem.music >= 0.6 {
            f *= gate_music_pct as f32 / 100.0;
        }
        if sem.speech >= 0.6 {
            f *= gate_speech_pct as f32 / 100.0;
        }
    }
    f.clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params() -> EngineParams {
        EngineParams {
            master: 100, low: 100, high: 80, sensitivity_k10: 30, release_ms: 120,
            sustain: 40, silence: 20, max_per_sec: 12, gate_music: 20, gate_speech: 30,
        }
    }

    #[test]
    fn low_band_share_gives_left_dominant_pulse() {
        // 爆炸声: 能量集中超低+低带
        let bands = [0.4, 0.3, 0.05, 0.02, 0.01];
        let r = onset_rule(&bands, &SemSnapshot::default(), false, &params());
        assert!(r.l > r.r, "低频占比过半 → 左马达主导 (l={} r={})", r.l, r.r);
        assert_eq!(r.release_ms, 120);
    }

    #[test]
    fn high_band_share_gives_right_dominant_pulse() {
        // 枪声: 能量集中中+高带
        let bands = [0.02, 0.03, 0.1, 0.3, 0.25];
        let r = onset_rule(&bands, &SemSnapshot::default(), false, &params());
        assert!(r.r > r.l, "高频占比过 60% → 右马达主导 (l={} r={})", r.r, r.l);
        assert_eq!(r.release_ms, 60);
    }

    #[test]
    fn semantic_gun_overrides_band_rule() {
        let bands = [0.4, 0.3, 0.1, 0.1, 0.1]; // 本该走低频规则
        let mut sem = SemSnapshot::default();
        sem.gun = 0.8;
        let r = onset_rule(&bands, &sem, true, &params());
        assert!(r.r > r.l && r.release_ms == 60, "枪声标签命中 → 重枪震感");
        // 语义过期 → 回落频带规则
        let r2 = onset_rule(&bands, &sem, false, &params());
        assert!(r2.l > r2.r, "语义不新鲜时不应覆盖");
    }

    #[test]
    fn gains_scale_rule_output() {
        let bands = [0.4, 0.3, 0.05, 0.02, 0.01];
        let mut p = params();
        p.master = 50; // 总强度 50%
        p.low = 100;
        let r = onset_rule(&bands, &SemSnapshot::default(), false, &p);
        assert!((r.l - 0.45).abs() < 1e-6, "0.9 × 50% = 0.45, 实际 {}", r.l);
    }

    #[test]
    fn gate_factor_suppresses_music_and_speech() {
        let mut sem = SemSnapshot::default();
        sem.music = 0.9;
        let f = gate_factor(&sem, true, 20, 30);
        assert!((f - 0.2).abs() < 1e-6, "Music 90% → ×20%");
        sem.speech = 0.8;
        let f2 = gate_factor(&sem, true, 20, 30);
        assert!((f2 - 0.06).abs() < 1e-6, "Music+Speech 双命中 → 相乘 0.2×0.3");
        // 未过期阈值下不触发
        assert_eq!(gate_factor(&sem, false, 20, 30), 1.0);
        // 置信度不足不触发
        let mut low = SemSnapshot::default();
        low.music = 0.5;
        assert_eq!(gate_factor(&low, true, 20, 30), 1.0);
        // 0% = 完全过滤
        let f0 = gate_factor(&sem, true, 0, 30);
        assert_eq!(f0, 0.0);
    }
}
