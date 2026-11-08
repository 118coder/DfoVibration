//! ★v24.50 音频震动 — 重采样 (任意率 → 16kHz, YAMNet 输入)。
//!
//! 两级: 窗化 sinc 低通 (防 16k Nyquist 混叠) + 线性插值分数步进。
//! 截止取 5.3kHz 而非 7.2k: 63 tap FIR 的过渡带 ~3kHz, 7.2k 截止会让 12kHz
//! 落在过渡带衰减不足; YAMNet mel 上限 7.5kHz, 5.3k 截止对分类损失可忽略。
//! 纯函数可单测: 1kHz 正弦保真, 12kHz 正弦强衰减 (混叠防护)。

/// LPF 截止 (Hz) — 5.3k, 折衷混叠防护与 YAMNet 高频信息
const LPF_CUTOFF_HZ: f32 = 5300.0;
/// FIR 长度 (48kHz 下过渡带 ≈ 3kHz, 63 tap)
const LPF_TAPS: usize = 63;

/// 单输入率 → 单输出率重采样器 (线性相位 LPF + 线性插值)。
pub struct Resampler {
    taps: Vec<f32>,
    hist: Vec<f32>,
    step: f32, // rate_out / rate_in (每输入样本推进的输出时钟)
    frac: f32,
    prev: f32,
    out: Vec<f32>,
}

impl Resampler {
    pub fn new(rate_in: f32, rate_out: f32) -> Self {
        let cutoff = LPF_CUTOFF_HZ.min(rate_out * 0.45).min(rate_in * 0.45);
        let taps = sinc_lpf(cutoff / rate_in, LPF_TAPS);
        let tap_count = taps.len();
        Self {
            taps,
            hist: vec![0.0; tap_count],
            step: rate_out / rate_in,
            frac: 0.0,
            prev: 0.0,
            out: Vec::new(),
        }
    }

    /// 推入一个输入样本 (内部 LPF + 时钟推进)。
    pub fn push(&mut self, x: f32) {
        // FIR: hist[0]=最新
        self.hist.rotate_right(1);
        self.hist[0] = x;
        let y: f32 = self
            .hist
            .iter()
            .zip(self.taps.iter())
            .map(|(h, t)| h * t)
            .sum();
        self.frac += self.step;
        while self.frac >= 1.0 {
            self.frac -= 1.0;
            let t = (self.frac / self.step).clamp(0.0, 1.0);
            self.out.push(self.prev + (y - self.prev) * t);
        }
        self.prev = y;
    }

    /// 取走累积的输出样本。
    pub fn take(&mut self) -> Vec<f32> {
        std::mem::take(&mut self.out)
    }
}

/// 窗化 sinc 低通系数 (Blackman 窗; 归一化 DC 增益=1)。
fn sinc_lpf(cutoff_norm: f32, taps: usize) -> Vec<f32> {
    let m = (taps - 1) as f32 / 2.0;
    let mut out = Vec::with_capacity(taps);
    let mut sum = 0f32;
    for i in 0..taps {
        let x = i as f32 - m;
        let sinc = if x.abs() < 1e-6 {
            2.0 * cutoff_norm
        } else {
            (2.0 * std::f32::consts::PI * cutoff_norm * x).sin()
                / (std::f32::consts::PI * x)
        };
        // Blackman
        let w = 0.42
            - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / (taps - 1) as f32).cos()
            + 0.08 * (4.0 * std::f32::consts::PI * i as f32 / (taps - 1) as f32).cos();
        let v = sinc * w;
        sum += v;
        out.push(v);
    }
    for v in out.iter_mut() {
        *v /= sum;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(freq: f32, amp: f32, secs: f32, rate: f32) -> Vec<f32> {
        let n = (secs * rate) as usize;
        (0..n)
            .map(|i| amp * (2.0 * std::f32::consts::PI * freq * i as f32 / rate).sin())
            .collect()
    }

    fn rms(v: &[f32]) -> f32 {
        (v.iter().map(|x| x * x).sum::<f32>() / v.len().max(1) as f32).sqrt()
    }

    #[test]
    fn downsample_48k_to_16k_preserves_passband() {
        let mut rs = Resampler::new(48000.0, 16000.0);
        for s in sine(1000.0, 0.5, 0.5, 48000.0) {
            rs.push(s);
        }
        let out = rs.take();
        // 0.5s 输入 → ~0.5s 输出 (丢 31 tap 延迟)
        assert!((out.len() as i32 - 8000).abs() < 120, "输出长度异常: {}", out.len());
        let r = rms(&out[2000..7000]);
        assert!(
            (r - 0.353).abs() < 0.06,
            "1kHz 通带应保真 (期望 rms≈0.353=A/√2), 实际 {}",
            r
        );
    }

    #[test]
    fn downsample_attenuates_alias_band() {
        let mut rs = Resampler::new(48000.0, 16000.0);
        for s in sine(12000.0, 0.5, 0.4, 48000.0) {
            rs.push(s);
        }
        let out = rs.take();
        let r = rms(&out[1000..5000]);
        // 12kHz 超出 16k 采样 Nyquist 一半以上, 截止 7.2kHz → 强衰减 (>20dB)
        assert!(
            r < 0.05,
            "混叠带应强衰减 (<0.05), 实际 {}",
            r
        );
    }

    #[test]
    fn resampler_is_stateless_safe_between_runs() {
        let mut rs = Resampler::new(48000.0, 16000.0);
        rs.push(0.5);
        assert_eq!(rs.take().len(), 0, "不足一个输出步不应出样");
        let first = {
            for s in sine(440.0, 0.5, 0.2, 48000.0) {
                rs.push(s);
            }
            rs.take()
        };
        assert!(!first.is_empty());
        let second = {
            for s in sine(440.0, 0.5, 0.2, 48000.0) {
                rs.push(s);
            }
            rs.take()
        };
        assert!(!second.is_empty(), "连续两轮都应出样");
    }
}
