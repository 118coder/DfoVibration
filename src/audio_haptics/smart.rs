//! ★ AI 智能增强 (B 模式) —— 纯算法空间感知层, 无模型、无新依赖。
//!
//! ## 依据 (2026-11-12 调研, 见工作区《方案_AI智能增强_空键位预设_20点优化》)
//! - **Fast-SDE** (arXiv 2606.12339) / **DCASE 2024 SELD**: 单声道也能靠
//!   DRR (直达/混响能量比)、早期反射时距估声源距离, 不需要麦克风阵列;
//! - **听觉距离心理物理** (Bronkhorst & Houtgast; PMC9256371; PMC4381006):
//!   四个可用线索 = 响度(逆平方律) / 高频空气吸收(谱质心下降, 声音变闷) /
//!   DRR / 单耳 AM 调制深度, 按可靠性加权融合;
//! - **IEEE ToH 2024** (10.1109/TOH.2023.3338224, Sound-to-Touch
//!   Crossmodal Pitch Matching): 声音响度谱可映射到最和谐的触觉音高;
//! - **CHI'23 / CHI'25** (POSTECH): 实时声音→触觉要按事件选择性给不同包络。
//!
//! ## 落点
//! 挂在 `dsp::Analyzer::process` 的映射层之后、攻/放平滑之前。**直接复用引擎
//! 已算好的频带能量**, 不重算 FFT; 定长环形窗, 热路径零堆分配。
//!
//! ## 纪律 (与 v24.60 红线一致)
//! - **A 模式 = 默认关** (`enabled=false` 或 `strength=0`) → 返回全 1 系数,
//!   输出与改动前**逐字节一致**;
//! - 任何 NaN/Inf 立即复位状态并回落中性系数 (等同 A 模式);
//! - 不引入新 crate、不碰采样/线程、不碰 DLL 与连发语义。

/// 近期 RMS 环形窗长度 (每 hop ≈10.7ms, 32 帧 ≈340ms 记忆)。
const WINDOW: usize = 32;

/// 距离 / 音高档位 (GUI 文案用, 纯函数)。
pub fn distance_label(p: f32) -> &'static str {
    if p >= 0.66 { "近" } else if p >= 0.33 { "中" } else { "远" }
}

pub fn pitch_label(p: f32) -> &'static str {
    if p >= 0.66 { "高" } else if p >= 0.33 { "中" } else { "低" }
}

/// 引擎每 tick 从原子量搬运来的智能层参数。
#[derive(Debug, Clone, Copy)]
pub struct SmartParams {
    /// A=经典(false) / B=AI 智能(true)。唯一权威开关。
    pub enabled: bool,
    /// 增强强度 0..=100; 0 = 自动等同 A 模式 (一键无损回退)。
    pub strength_pct: u32,
    /// 持续时间/收尾的影响程度 0..=100。
    pub duration_pct: u32,
}

/// 一帧输入 (全部来自 `Analyzer` 已有结果, int16 尺度 RMS)。
#[derive(Debug, Clone, Copy, Default)]
pub struct SmartInput {
    pub total: f32,
    pub bass: f32,
    pub high: f32,
    pub noise_floor: f32,
    pub map_max: f32,
    pub transient: f32,
    pub threshold: f32,
}

/// 一帧输出: 乘到经典映射上的一组系数 + 遥测。
#[derive(Debug, Clone, Copy)]
pub struct SmartOutput {
    /// false = 增强层不生效 (A 模式 / 静音 / 异常回落), 调用方应完全旁路。
    pub active: bool,
    pub gain: f32,
    pub attack_scale: f32,
    /// `smooth_signal` 的 rate 越大收尾越快: 近 → 放大(短促), 远 → 缩小(长尾)。
    pub release_scale: f32,
    pub left_bias: f32,
    pub right_bias: f32,
    /// 近场瞬态「点击感」(**乘性增益**, 不是加性脉冲: 基础输出为 0 时不该凭空冒震动)。
    pub click: f32,
    pub proximity: f32,
    pub pitch: f32,
    pub duration_ms: f32,
}

impl Default for SmartOutput {
    fn default() -> Self {
        Self {
            active: false,
            gain: 1.0,
            attack_scale: 1.0,
            release_scale: 1.0,
            left_bias: 1.0,
            right_bias: 1.0,
            click: 0.0,
            proximity: 0.5,
            pitch: 0.5,
            duration_ms: 0.0,
        }
    }
}

/// 有状态的空间感知器 (引擎线程独占, 无锁)。
#[derive(Debug, Clone)]
pub struct SmartEnhancer {
    pub proximity: f32,
    pub pitch: f32,
    pub duration_ms: f32,
    rms_window: [f32; WINDOW],
    rms_pos: usize,
    rms_count: usize,
    /// 慢均线: 声音持续/混响尾巴越长, 它越接近当前能量 → DRR 近似里的"晚期能量"。
    energy_slow: f32,
}

impl Default for SmartEnhancer {
    fn default() -> Self {
        Self {
            proximity: 0.5,
            pitch: 0.5,
            duration_ms: 0.0,
            rms_window: [0.0; WINDOW],
            rms_pos: 0,
            rms_count: 0,
            energy_slow: 0.0,
        }
    }
}

impl SmartEnhancer {
    /// 每个 FFT hop 推一帧总能量 (只记历史, 不做判定; 零堆分配)。
    pub fn push_frame(&mut self, total_rms: f32) {
        if !total_rms.is_finite() {
            return;
        }
        let v = total_rms.max(0.0);
        self.rms_window[self.rms_pos] = v;
        self.rms_pos = (self.rms_pos + 1) % WINDOW;
        if self.rms_count < WINDOW {
            self.rms_count += 1;
        }
        self.energy_slow = self.energy_slow * 0.94 + v * 0.06;
    }

    /// 环形窗的均值与标准差 (单耳 AM 深度线索)。
    fn window_stats(&self) -> (f32, f32) {
        if self.rms_count == 0 {
            return (0.0, 0.0);
        }
        let mut mean = 0f32;
        for i in 0..self.rms_count {
            mean += self.rms_window[i];
        }
        mean /= self.rms_count as f32;
        let mut var = 0f32;
        for i in 0..self.rms_count {
            let d = self.rms_window[i] - mean;
            var += d * d;
        }
        var /= self.rms_count as f32;
        (mean, var.sqrt())
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// 计算本帧增强系数。**A 模式/静音/异常一律返回全 1 系数**, 调用方据此旁路。
    pub fn process(&mut self, input: &SmartInput, p: &SmartParams) -> SmartOutput {
        /* 静音提前退出: 低于门槛时整层不算, CPU 接近零 */
        if !p.enabled
            || p.strength_pct == 0
            || input.total <= input.threshold
            || !input.total.is_finite()
            || !input.bass.is_finite()
            || !input.high.is_finite()
            || !input.noise_floor.is_finite()
            || !input.map_max.is_finite()
            || !input.transient.is_finite()
        {
            return SmartOutput::default();
        }

        let eps = 1.0; // int16 尺度下加 1 足够防除零
        let nf = input.noise_floor.max(0.0);
        let span = (input.map_max - nf).max(1.0);

        /* ① 响度 (逆平方律): 近 = 响 */
        let loudness = ((input.total - nf) / span).clamp(0.0, 1.0);
        /* ② 高频空气吸收: 远 = 高频被吃掉 → 闷 (谱质心的廉价替代) */
        let brightness = (input.high / (input.bass + input.high + eps)).clamp(0.0, 1.0);
        /* ③ DRR 近似: 直达强/尾巴短 = 近。energy_slow 是晚期能量代理 */
        let tail = (self.energy_slow / (input.total + eps)).clamp(0.0, 1.0);
        let dryness = (0.6 * input.transient.clamp(0.0, 1.0) + 0.4 * (1.0 - tail))
            .clamp(0.0, 1.0);
        /* ④ 单耳 AM 调制深度: 近 = 包络起伏深 */
        let (mean, std) = self.window_stats();
        let am_depth = (std / (mean + eps)).clamp(0.0, 1.0);

        let raw = (0.40 * loudness + 0.25 * brightness + 0.20 * dryness + 0.15 * am_depth)
            .clamp(0.0, 1.0);

        /* 非对称平滑: 快起慢落 + 滞回, 避免临界抖动让马达"哆嗦" */
        let alpha = if raw > self.proximity { 0.45 } else { 0.12 };
        self.proximity += (raw - self.proximity) * alpha;

        /* 音高: 高频占比为主, AM 深度做轻微修正 (IEEE ToH 2024 的廉价实现) */
        let pitch_raw = (0.75 * brightness + 0.25 * (1.0 - am_depth)).clamp(0.0, 1.0);
        self.pitch += (pitch_raw - self.pitch) * 0.25;

        if !self.proximity.is_finite() || !self.pitch.is_finite() {
            self.reset();
            return SmartOutput::default();
        }
        let prox = self.proximity.clamp(0.0, 1.0);
        let pitch = self.pitch.clamp(0.0, 1.0);
        let mix = (p.strength_pct.min(100) as f32) / 100.0;
        let dur_mix = (p.duration_pct.min(100) as f32) / 100.0;

        /* 差异化输出: 近 = 强/快起/快收/点击感; 远 = 弱/长尾/无点击 */
        let gain = 1.0 + (0.55 + 0.75 * prox - 1.0) * mix;
        let attack_scale = 1.0 + (0.80 + 0.60 * prox - 1.0) * mix;
        let release_scale = 1.0 + (0.55 + 0.90 * prox - 1.0) * mix * dur_mix;
        /* 低音偏左(大转子, 闷) / 高音偏右(小转子, 脆) */
        let left_bias = 1.0 + ((1.15 - 0.50 * pitch) - 1.0) * mix;
        let right_bias = 1.0 + ((0.75 + 0.60 * pitch) - 1.0) * mix;
        /* 乘性脉冲: 调用方用 tl*(1+click) 而不是 tl+click */
        let click = prox * input.transient.clamp(0.0, 1.0) * (1.0 - pitch) * 0.18 * mix;
        /* 持续时间只由距离感决定 (近短远长)。
         * ★云端 CI 实测修正: 早期版本还乘了 (0.7+0.6*loudness), 但"近=响"会把
         * 近场反而拉长, 与"近音短促"的设计意图冲突 (实测近 221ms > 远 217ms)。 */
        let duration_ms = (60.0 + 260.0 * (1.0 - prox)).clamp(40.0, 400.0);
        self.duration_ms = duration_ms;

        SmartOutput {
            active: true,
            gain,
            attack_scale,
            release_scale,
            left_bias,
            right_bias,
            click,
            proximity: prox,
            pitch,
            duration_ms,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params() -> SmartParams {
        SmartParams { enabled: true, strength_pct: 70, duration_pct: 70 }
    }

    /// 造一个"近场"信号: 响、瞬态强、包络起伏大 (AM 深)、慢均线远落后于当前。
    fn near_input(bass: f32, high: f32) -> SmartInput {
        SmartInput {
            total: 3500.0,
            bass,
            high,
            noise_floor: 100.0,
            map_max: 4200.0,
            transient: 0.85,
            threshold: 260.0,
        }
    }

    /// 造一个"远场"信号: 轻、闷、无明显瞬态、能量平稳 (AM 浅、尾巴长)。
    fn far_input(bass: f32, high: f32) -> SmartInput {
        SmartInput {
            total: 520.0,
            bass,
            high,
            noise_floor: 100.0,
            map_max: 4200.0,
            transient: 0.03,
            threshold: 260.0,
        }
    }

    /// 预填环形窗: 近场大幅起伏; 远场近乎恒定。
    fn feed(e: &mut SmartEnhancer, near: bool) {
        for i in 0..WINDOW * 2 {
            let v = if near {
                if i % 2 == 0 { 3600.0 } else { 400.0 }
            } else {
                500.0 + (i % 3) as f32 * 5.0
            };
            e.push_frame(v);
        }
    }

    fn run(inp: SmartInput, near: bool) -> (SmartEnhancer, SmartOutput) {
        let mut e = SmartEnhancer::default();
        feed(&mut e, near);
        // 逐帧收敛到稳态
        let mut out = SmartOutput::default();
        for _ in 0..40 {
            out = e.process(&inp, &params());
            e.push_frame(inp.total);
        }
        (e, out)
    }

    #[test]
    fn a_mode_is_byte_identical_to_classic() {
        let mut e = SmartEnhancer::default();
        feed(&mut e, true);
        let off = SmartParams { enabled: false, strength_pct: 70, duration_pct: 70 };
        let o = e.process(&near_input(3000.0, 300.0), &off);
        assert!(!o.active);
        assert_eq!(o.gain, 1.0);
        assert_eq!(o.attack_scale, 1.0);
        assert_eq!(o.release_scale, 1.0);
        assert_eq!(o.left_bias, 1.0);
        assert_eq!(o.right_bias, 1.0);
        assert_eq!(o.click, 0.0);
    }

    #[test]
    fn zero_strength_is_also_classic() {
        let mut e = SmartEnhancer::default();
        feed(&mut e, true);
        let zero = SmartParams { enabled: true, strength_pct: 0, duration_pct: 70 };
        let o = e.process(&near_input(3000.0, 300.0), &zero);
        assert!(!o.active, "强度 0 必须能一键无损回退 A 手感");
    }

    #[test]
    fn near_is_stronger_faster_and_shorter_than_far() {
        let (_, n) = run(near_input(3000.0, 300.0), true);
        let (_, f) = run(far_input(450.0, 25.0), false);
        assert!(n.active && f.active);
        assert!(n.proximity > f.proximity + 0.15,
            "近场 proximity 应显著高于远场: {} vs {}", n.proximity, f.proximity);
        assert!(n.gain > f.gain, "近场应更猛: {} vs {}", n.gain, f.gain);
        assert!(n.attack_scale > f.attack_scale, "近场起振应更快");
        assert!(n.release_scale > f.release_scale, "近场收尾应更快(rate 越大越快)");
        assert!(n.duration_ms < f.duration_ms,
            "近场持续时间应更短: {} vs {} ms", n.duration_ms, f.duration_ms);
        assert!(n.click > f.click, "近场才该有点击感");
    }

    #[test]
    fn low_pitch_biases_left_high_pitch_biases_right() {
        let (_, low) = run(near_input(3400.0, 40.0), true);
        let (_, high) = run(near_input(200.0, 3200.0), true);
        assert!(low.pitch < high.pitch, "高频占比高应判为高音");
        assert!(low.left_bias > low.right_bias, "低音应偏左马达");
        assert!(high.right_bias > high.left_bias, "高音应偏右马达");
    }

    #[test]
    fn silence_skips_the_whole_layer() {
        let mut e = SmartEnhancer::default();
        feed(&mut e, true);
        let mut quiet = near_input(3000.0, 300.0);
        quiet.total = 10.0;
        let o = e.process(&quiet, &params());
        assert!(!o.active, "低于门槛应整层跳过");
    }

    #[test]
    fn non_finite_input_falls_back_to_classic() {
        let mut e = SmartEnhancer::default();
        feed(&mut e, true);
        let mut bad = near_input(3000.0, 300.0);
        bad.total = f32::NAN;
        let o = e.process(&bad, &params());
        assert!(!o.active, "NaN 必须回落经典模式");
        assert_eq!(o.gain, 1.0);
    }

    #[test]
    fn labels_cover_the_full_range() {
        assert_eq!(distance_label(0.9), "近");
        assert_eq!(distance_label(0.5), "中");
        assert_eq!(distance_label(0.1), "远");
        assert_eq!(pitch_label(0.9), "高");
        assert_eq!(pitch_label(0.1), "低");
    }
}
