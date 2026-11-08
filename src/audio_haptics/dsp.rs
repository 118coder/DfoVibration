//! ★v24.50 音频震动 DSP 核心 —— STFT 频带能量 + 谱通量 onset + 自适应阈值 + 包络。
//!
//! 算法出处 (调研报告 §3.2): 谱通量 onset = librosa/Dixon 2006 (DAFx) 标准配方;
//! 自适应阈值 = 滑动中位数 + k·MAD。全部纯 Rust (rustfft), 无 GPL 组件。
//! 本文件**纯逻辑**, 不碰 WASAPI/线程 —— 全部可单测 (合成信号驱动)。

use std::collections::VecDeque;

use rustfft::{num_complex::Complex, FftPlanner};

use super::mapping::{self, EngineParams};
use crate::state::SemSnapshot;

/// FFT 窗长 (48kHz ≈ 21ms)
pub const FFT_SIZE: usize = 1024;
/// 跳步 (2x 重叠, 48kHz ≈ 10.7ms/帧)
pub const HOP: usize = 512;
/// 5 频带边界 Hz: 超低/低/中/高/超高
pub const BAND_EDGES: [f32; 6] = [20.0, 80.0, 250.0, 2000.0, 8000.0, 24000.0];
pub const NUM_BANDS: usize = 5;
/// onset 不应期 ms (同一 80ms 窗内最多 1 次)
pub const ONSET_REFRACTORY_MS: u64 = 80;
/// 自适应阈值历史窗 (帧) —— 1024/512 hop @48kHz ≈ 1s
const FLUX_HISTORY: usize = 47;
/// 相对谱通量下限 (flux / 总幅度谱和; 稳态纯音的旁瓣干涉 ~1e-3 量级, 远低于此)
const FLUX_FLOOR: f32 = 0.04;

/// bin → 频带下标 (纯函数)。
pub fn band_of_bin(bin: usize, sample_rate: f32, fft_size: usize) -> Option<usize> {
    if bin == 0 || bin >= fft_size / 2 {
        return None; // DC 与 Nyquist 以上不计
    }
    let freq = bin as f32 * sample_rate / fft_size as f32;
    for (i, w) in BAND_EDGES.windows(2).enumerate() {
        if freq >= w[0] && freq < w[1] {
            return Some(i);
        }
    }
    None
}

/// Hann 窗系数 (纯函数)。
pub fn hann(i: usize, n: usize) -> f32 {
    0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / n as f32).cos()
}

/// 滑动窗中位数 + MAD (纯函数; 窗 ≤64 项, 排序足够)。偶数长度取真中位数 (两中平均)。
pub fn median_mad(window: &[f32]) -> (f32, f32) {
    if window.is_empty() {
        return (0.0, 0.0);
    }
    let mut sorted: Vec<f32> = window.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = sorted.len();
    let median = if n % 2 == 1 {
        sorted[n / 2]
    } else {
        (sorted[n / 2 - 1] + sorted[n / 2]) / 2.0
    };
    let mut dev: Vec<f32> = sorted.iter().map(|v| (v - median).abs()).collect();
    dev.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mad = if n % 2 == 1 {
        dev[n / 2]
    } else {
        (dev[n / 2 - 1] + dev[n / 2]) / 2.0
    };
    (median, mad)
}

/// 一帧处理结果 (引擎写输出/遥测用)。
#[derive(Debug, Clone, Default)]
pub struct FrameOut {
    pub bands: [f32; NUM_BANDS],
    pub band_peak: [f32; NUM_BANDS],
    pub level_l: f32,
    pub level_r: f32,
    pub onset: bool,
    pub onsets_sec: u32,
    pub gate_factor: f32,
}

/// 频谱分析器 (单线程使用; 引擎线程独占)。
pub struct Analyzer {
    rate: f32,
    fft: std::sync::Arc<dyn rustfft::Fft<f32>>,
    window: Vec<f32>,
    buf: VecDeque<f32>,
    prev_mag: Vec<f32>,
    flux_hist: VecDeque<f32>,
    bands_smooth: [f32; NUM_BANDS],
    band_peak: [f32; NUM_BANDS],
    pulse_peak: (f32, f32),
    pulse_until: u64,
    pulse_dur: u64,
    last_onset: u64,
    onset_stamps: VecDeque<u64>,
    last_process: u64,
}

impl Analyzer {
    pub fn new(rate: f32) -> Self {
        let mut planner = FftPlanner::<f32>::new();
        let fft = planner.plan_fft_forward(FFT_SIZE);
        let window: Vec<f32> = (0..FFT_SIZE).map(|i| hann(i, FFT_SIZE)).collect();
        Self {
            rate,
            fft,
            window,
            buf: VecDeque::with_capacity(FFT_SIZE * 4),
            prev_mag: vec![0.0; FFT_SIZE / 2],
            flux_hist: VecDeque::with_capacity(FLUX_HISTORY + 1),
            bands_smooth: [0.0; NUM_BANDS],
            band_peak: [0.0; NUM_BANDS],
            pulse_peak: (0.0, 0.0),
            pulse_until: 0,
            pulse_dur: 120,
            last_onset: 0,
            onset_stamps: VecDeque::with_capacity(64),
            last_process: 0,
        }
    }

    /// 推入单声道样本 (48kHz 原始率)。
    pub fn push(&mut self, s: f32) {
        self.buf.push_back(s);
        if self.buf.len() > FFT_SIZE * 4 {
            let drop = self.buf.len() - FFT_SIZE * 4;
            for _ in 0..drop {
                self.buf.pop_front();
            }
        }
    }

    /// 处理全部可用 hop + 包络推进 → 当前输出电平。
    pub fn process(
        &mut self,
        now_ms: u64,
        p: &EngineParams,
        sem: &SemSnapshot,
        sem_fresh: bool,
        spatial: (f32, f32),
    ) -> FrameOut {
        let dt_ms = now_ms.saturating_sub(self.last_process).clamp(1, 100) as f32;
        self.last_process = now_ms;
        let mut onset_hit = false;

        // ── 逐 hop 频谱 ──
        let k = p.sensitivity_k10 as f32 / 10.0;
        while self.buf.len() >= FFT_SIZE {
            let mut frame: Vec<Complex<f32>> = self
                .buf
                .iter()
                .take(FFT_SIZE)
                .enumerate()
                .map(|(i, s)| Complex {
                    re: s * self.window[i],
                    im: 0.0,
                })
                .collect();
            self.fft.process(&mut frame);

            // 5 频带 RMS (归一: 满幅正弦 ≈ 幅值本身, 0..1) + 相对谱通量
            let mut band_sq = [0f32; NUM_BANDS];
            let mut flux = 0f32;
            let mut mag_sum = 0f32;
            for (bin, c) in frame.iter().enumerate() {
                let m = c.norm();
                if bin < self.prev_mag.len() {
                    flux += (m - self.prev_mag[bin]).max(0.0);
                    self.prev_mag[bin] = m;
                    mag_sum += m;
                }
                if let Some(b) = band_of_bin(bin, self.rate, FFT_SIZE) {
                    band_sq[b] += m * m;
                }
            }
            // 相对化 (音量不变性): 稳态信号 (含非整数 bin 驻留的旁瓣干涉) → ≈0;
            // 突发 (枪响/爆炸) → 0.3~1.0。绝对通量会被 BGM 稳态纹波反复击穿。
            flux /= mag_sum + 1e-9;
            let mut bands = [0f32; NUM_BANDS];
            for (i, sq) in band_sq.iter().enumerate() {
                bands[i] = sq.sqrt() / (FFT_SIZE / 4) as f32;
            }

            // 自适应阈值: 滑动中位数 + k·MAD
            self.flux_hist.push_back(flux);
            if self.flux_hist.len() > FLUX_HISTORY {
                self.flux_hist.pop_front();
            }
            let (median, mad) = median_mad(self.flux_hist.make_contiguous());
            let threshold = (median + k * mad).max(FLUX_FLOOR);
            let total: f32 = bands.iter().sum();

            // onset 判定: flux 超阈值 + 总能量过静音线 + 不应期 + 秒限频
            let stamps_sec = self
                .onset_stamps
                .iter()
                .filter(|t| now_ms.saturating_sub(**t) <= 1000)
                .count() as u32;
            if flux > threshold
                && total * 100.0 >= p.silence as f32
                && now_ms.saturating_sub(self.last_onset) >= ONSET_REFRACTORY_MS
                && stamps_sec < p.max_per_sec
            {
                let rule = mapping::onset_rule(&bands, sem, sem_fresh, p);
                self.pulse_peak = (rule.l, rule.r);
                self.pulse_dur = rule.release_ms as u64;
                self.pulse_until = now_ms + rule.release_ms as u64;
                self.last_onset = now_ms;
                self.onset_stamps.push_back(now_ms);
                onset_hit = true;
            }
            if self.onset_stamps.len() > 64 {
                self.onset_stamps.pop_front();
            }

            // 持续能量平滑 (attack 80ms / release 300ms)
            for i in 0..NUM_BANDS {
                let tau = if bands[i] > self.bands_smooth[i] { 80.0 } else { 300.0 };
                let alpha = 1.0 - (-dt_ms / tau).exp();
                self.bands_smooth[i] += (bands[i] - self.bands_smooth[i]) * alpha;
            }

            for _ in 0..HOP {
                self.buf.pop_front();
            }
        }

        // ── 包络合成 ──
        let mut pulse = (0f32, 0f32);
        if now_ms < self.pulse_until && self.pulse_dur > 0 {
            let frac = (self.pulse_until - now_ms) as f32 / self.pulse_dur as f32;
            pulse = (self.pulse_peak.0 * frac, self.pulse_peak.1 * frac);
        }
        let total_smooth: f32 = self.bands_smooth.iter().sum();
        // 持续低鸣: onset 过后 (>300ms) 才参与, 避免与脉冲叠加成糊
        let mut sustain = (0f32, 0f32);
        if now_ms.saturating_sub(self.last_onset) > 300 && total_smooth * 100.0 >= p.silence as f32 {
            let low = (self.bands_smooth[0] + self.bands_smooth[1]) / 2.0;
            let high = (self.bands_smooth[2] + self.bands_smooth[3] + self.bands_smooth[4]) / 3.0;
            let sg = p.sustain as f32 / 100.0;
            sustain = (low * sg, high * sg * 0.25);
        }

        // 纯净反馈门控 (YAMNet Music/Speech 命中时压制)
        let gf = mapping::gate_factor(sem, sem_fresh, p.gate_music, p.gate_speech);
        let spatial_l = spatial.0.clamp(0.3, 1.0);
        let spatial_r = spatial.1.clamp(0.3, 1.0);
        let level_l =
            (pulse.0.max(sustain.0) * gf * spatial_l).clamp(0.0, 1.0);
        let level_r =
            (pulse.1.max(sustain.1) * gf * spatial_r).clamp(0.0, 1.0);

        // 可视化峰值慢落
        for i in 0..NUM_BANDS {
            self.band_peak[i] = (self.bands_smooth[i]).max(self.band_peak[i] * 0.93);
        }

        FrameOut {
            bands: self.bands_smooth,
            band_peak: self.band_peak,
            level_l,
            level_r,
            onset: onset_hit,
            onsets_sec: self
                .onset_stamps
                .iter()
                .filter(|t| now_ms.saturating_sub(**t) <= 1000)
                .count() as u32,
            gate_factor: gf,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_params() -> EngineParams {
        EngineParams {
            master: 100,
            low: 100,
            high: 80,
            sensitivity_k10: 30,
            release_ms: 120,
            sustain: 40,
            silence: 20,
            max_per_sec: 12,
            gate_music: 20,
            gate_speech: 30,
        }
    }

    /// 生成正弦波 (rate Hz 采样; amp = 真幅度, 千万别乘进相位里)
    fn sine(freq: f32, amp: f32, secs: f32, rate: f32) -> Vec<f32> {
        let n = (secs * rate) as usize;
        (0..n)
            .map(|i| amp * (2.0 * std::f32::consts::PI * freq * i as f32 / rate).sin())
            .collect()
    }

    #[test]
    fn band_of_bin_maps_frequency_ranges() {
        // 48kHz / 1024 点: bin 宽 46.875Hz
        let r = 48000.0;
        assert_eq!(band_of_bin(2, r, 1024), Some(1)); // 93.75Hz → 80-250 低带
        assert_eq!(band_of_bin(1, r, 1024), Some(0)); // 46.9Hz → 20-80 超低
        assert_eq!(band_of_bin(20, r, 1024), Some(2)); // 937Hz → 250-2k 中
        assert_eq!(band_of_bin(120, r, 1024), Some(3)); // 5.6kHz → 2k-8k 高
        assert_eq!(band_of_bin(510, r, 1024), Some(4)); // 23.9kHz → 8k+ 超高
        assert_eq!(band_of_bin(0, r, 1024), None, "DC 不计");
    }

    #[test]
    fn median_mad_matches_reference() {
        let (m, mad) = median_mad(&[1.0, 1.0, 1.0, 1.0, 100.0]);
        assert_eq!(m, 1.0);
        assert_eq!(mad, 0.0, "4 平 1 异常 → MAD=0 (中位数鲁棒)");
        let (m, _) = median_mad(&[1.0, 2.0, 3.0, 4.0]);
        assert!((m - 2.5).abs() < 1e-6);
        assert_eq!(median_mad(&[]), (0.0, 0.0));
    }

    #[test]
    fn sine_energy_lands_in_expected_band() {
        let mut an = Analyzer::new(48000.0);
        for s in sine(100.0, 0.5, 0.3, 48000.0) {
            an.push(s);
        }
        let out = an.process(
            1000,
            &default_params(),
            &SemSnapshot::default(),
            false,
            (0.5, 0.5),
        );
        // 100Hz 正弦能量应集中在"低"带 (80-250), 且量级接近幅值
        assert!(
            out.bands[1] > 0.3,
            "低带应有能量, 实际 {:?}",
            out.bands
        );
        assert!(
            out.bands[1] > out.bands[3] * 4.0,
            "低带应远强于高带: {:?}",
            out.bands
        );
    }

    #[test]
    fn impulse_triggers_onset_and_envelope_decays() {
        let mut an = Analyzer::new(48000.0);
        // 1s 静音预热 (让阈值历史建立)
        for s in sine(60.0, 0.0, 1.0, 48000.0) {
            an.push(s);
        }
        let mut out = an.process(2000, &default_params(), &SemSnapshot::default(), false, (0.5, 0.5));
        assert!(!out.onset, "纯静音不应触发 onset");

        // 注入 30ms 突发白噪声 (模拟枪响)
        let mut rng_state = 12345u64;
        for _ in 0..(0.03 * 48000.0) as usize {
            rng_state = rng_state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let noise = ((rng_state >> 33) as f32 / u32::MAX as f32 - 0.5) * 0.8;
            an.push(noise);
        }
        out = an.process(2050, &default_params(), &SemSnapshot::default(), false, (0.5, 0.5));
        assert!(out.onset, "突发噪声应触发 onset");
        assert!(out.level_l > 0.05 || out.level_r > 0.05, "onset 后应有输出电平");

        // 静音 200ms → 包络衰减到 0
        out = an.process(2300, &default_params(), &SemSnapshot::default(), false, (0.5, 0.5));
        assert!(out.level_l < 0.02 && out.level_r < 0.02, "衰减后应归零, 实际 l={} r={}", out.level_l, out.level_r);
    }

    #[test]
    fn steady_sine_does_not_spam_onsets() {
        let mut an = Analyzer::new(48000.0);
        // 3s 持续 1kHz 正弦 (模拟 BGM 稳态) —— 预热 1s 后稳态期不应连发 onset
        let samples = sine(1000.0, 0.4, 3.0, 48000.0);
        let mut onsets = 0;
        let mut now = 1000u64;
        let chunk = 480; // 10ms
        for ch in samples.chunks(chunk) {
            for &s in ch {
                an.push(s);
            }
            let out = an.process(now, &default_params(), &SemSnapshot::default(), false, (0.5, 0.5));
            if out.onset {
                onsets += 1;
            }
            now += 10;
        }
        // 稳态正弦的 flux 恒 ≈0, 最多在起始沿触发 1-2 次
        assert!(onsets <= 2, "稳态正弦不应反复触发 onset, 实际 {}", onsets);
    }

    #[test]
    fn rate_limit_caps_onsets_per_second() {
        let mut an = Analyzer::new(48000.0);
        for s in sine(60.0, 0.0, 1.0, 48000.0) {
            an.push(s);
        }
        an.process(1000, &default_params(), &SemSnapshot::default(), false, (0.5, 0.5));
        // 每隔 100ms 打一记短促冲击 (10Hz, 超过 12/s? 10/s 没超 — 用 30Hz: 每 33ms)
        let mut onsets = 0;
        let mut now = 2000u64;
        let mut rng_state = 999u64;
        for rep in 0..40 {
            for _ in 0..(0.02 * 48000.0) as usize {
                rng_state = rng_state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                let noise = ((rng_state >> 33) as f32 / u32::MAX as f32 - 0.5) * 0.9;
                an.push(noise);
            }
            let out = an.process(now, &default_params(), &SemSnapshot::default(), false, (0.5, 0.5));
            if out.onset {
                onsets += 1;
            }
            now += 25;
        }
        assert!(
            onsets <= default_params().max_per_sec as usize,
            "秒限频应生效: 40 次冲击只允许 {} 次, 实际 {}",
            default_params().max_per_sec,
            onsets
        );
    }
}
