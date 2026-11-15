//! ★v24.60 音频震动 DSP 核心 —— 频带电平跟随融合引擎 (无 AI 模型, 全纯 DSP)。
//!
//! ## 算法来源 (v24.60 换代说明)
//! v24.52~v24.59 的 "SuperFlux onset 脉冲" 路线 (逐频带白化 + 谱通量 + 节拍周期性)
//! 整体退役; 新路线直接跟随频带电平, 两个参考实现:
//! - **xbox-audio-rumble-studio** (xbox6.py, 作者已书面授权): 噪声底自适应阈值、
//!   内容三分类 (音乐/人声/安静, 带滞回)、攻击/释放/死区、自动校准、节奏鼓点、微颤纹理;
//! - **audio-to-controller-vibration** (MIT): 低频→左马达 / 高频→右马达的分频映射、
//!   门槛→满量程归一 + 幂律压缩 (gamma<1 → "招招有震动")、高低频敏感度。
//! 两边都没有复制代码; 常数在**同一量纲** (int16 尺度 RMS, [`XSCALE`]) 下平移调优。
//!
//! ## 量纲 (重要)
//! xbox6.py 在 int16 原始幅度上标定 (threshold 400 = 归一化幅度的 400/32768);
//! 我们采集链输出归一化 f32。所有 RMS 在进引擎前乘 [`XSCALE`] 还原到 int16 尺度,
//! 因此 xbox6 的已调参数可以 1:1 平移, 不做任何换算。
//!
//! ## 频带 RMS 的 Parseval 修正
//! Hann 窗 FFT 的分频能量要除以 sqrt(Σw²/N)=sqrt(0.375) 才等于该频带的真 RMS
//! (xbox6 是对遮蔽谱做 irfft 求真 RMS)。常数见 [`RMS_NORM`]。
//!
//! 纪律: 本文件纯逻辑 (不碰 WASAPI/线程), 全部可用合成信号单测。

use std::collections::VecDeque;

use rustfft::{num_complex::Complex, FftPlanner};

use super::mapping::{
    self, classify_content, map_value_gamma, smooth_signal, EngineParams, MODE_MUSIC, MODE_QUIET,
    MODE_VOCAL,
};
use super::smart::{SmartEnhancer, SmartInput, SmartParams};

/// int16 尺度换算: 归一化 RMS × 32768 = xbox6 标定域的 RMS。
pub const XSCALE: f32 = 32768.0;

/// FFT 窗长 (48kHz ≈ 21ms)
pub const FFT_SIZE: usize = 1024;
/// 跳步 (2x 重叠, 48kHz ≈ 10.7ms/帧)
pub const HOP: usize = 512;
/// 5 频带边界 Hz: 超低/低/中/高/超高 (仅频谱显示用)
pub const BAND_EDGES: [f32; 6] = [20.0, 80.0, 250.0, 2000.0, 8000.0, 24000.0];
pub const NUM_BANDS: usize = 5;

/// Hann 窗 + 单边谱的 Parseval 修正: 只累加正频率 bin (1..N/2) 只拿到一半能量,
/// rms = sqrt(带内功率) × √2 / (sqrt(Σw²/N)·N) = sqrt(带内功率)/(0.4330·N)。
/// (已用"正弦真 RMS"与"白噪真 RMS"双基准验证; 用 matlab 口径对过)
const RMS_NORM: f32 = 0.433_012_7 * FFT_SIZE as f32;

/// 能量慢均线 (xbox6: energy_avg = 0.965·prev + 0.035·rms)
const ENERGY_AVG_UP: f32 = 0.035;
/// 瞬态均线的快慢系数 (xbox6: 0.82/0.18)
const TRANSIENT_SLOW: f32 = 0.82;
/// 瞬态分母的地板 (xbox6: max(energy_avg, 80), int16 尺度)
const TRANSIENT_DENOM_MIN: f32 = 80.0;
/// 噪声底跟踪系数 (xbox6: nf = 0.995·prev + 0.005·min(total, vocal))
const NF_DECAY: f32 = 0.995;
const NF_UP: f32 = 0.005;
/// 自适应阈值倍率: 游戏段 / 音乐段 / 人声段 / 安静段 (xbox6: 1.55 / 1.7 / 1.18 / 2.0)
const NF_K_GAME: f32 = 1.55;
const NF_K_MUSIC: f32 = 1.7;
const NF_K_VOCAL: f32 = 1.18;
const NF_K_QUIET: f32 = 2.0;
/// 各模式门槛下限 (xbox6 原值; 音乐段 170 / 人声段 75 / 安静段 260)
const THR_FLOOR_MUSIC: f32 = 170.0;
const THR_FLOOR_VOCAL: f32 = 75.0;
const THR_FLOOR_QUIET: f32 = 260.0;
/// 安静判定 (int16 尺度总 RMS; xbox6: total < 55)
const QUIET_TOTAL: f32 = 55.0;
/// 音乐段连续输出的最大压制深度 (music_mask 100% 时保留 15%)
const MUSIC_SUP_MAX: f32 = 0.85;
/// 鼓点脉冲的马达分配: 低频侧 (左) 主脉冲 / 右马达辅助 (xbox6 是右主左辅 ——
/// 他的 dual 模式把贝斯放右马达; 我们按物理惯例贝斯在左, 分配随之翻转)
const BEAT_MAIN: f32 = 0.55;
const BEAT_AUX: f32 = 0.20;
/// 微颤纹理: 幅度系数与调制频率 (xbox6: 0.075 / 34Hz), 右马达出力比例
const TEXTURE_AMP: f32 = 0.075;
const TEXTURE_HZ: f32 = 34.0;
const TEXTURE_R: f32 = 0.82;
/// 校准: 最短 3 秒 (xbox6), 结果门槛 = max(120, 峰值噪声底×1.8)
const CALIB_SECS: u64 = 3000;
const CALIB_FLOOR: f32 = 120.0;
const CALIB_K: f32 = 1.8;
/// quiet_mask 关闭时安静段的保留输出比例
const QUIET_KEEP: f32 = 0.5;

/// bin → 显示频带下标 (纯函数)。
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

/// 一帧处理结果 (引擎写输出/遥测用)。
#[derive(Debug, Clone, Default)]
pub struct FrameOut {
    /// 5 显示频带平滑 RMS (旧归一化口径, 0..~1)
    pub bands: [f32; NUM_BANDS],
    /// 频带峰值保持 (可视化慢落)
    pub band_peak: [f32; NUM_BANDS],
    /// 马达目标电平 (0..1, 已过攻/放平滑与死区)
    pub level_l: f32,
    pub level_r: f32,
    /// 内容判定 (mapping::MODE_*)
    pub mode: u8,
    /// 当前噪声底 (int16 尺度; GUI 显示)
    pub noise_floor: f32,
    /// 瞬态均线 0..1 (GUI 显示 "爆发度")
    pub transient: f32,
    /// 本 tick 生效阈值 (int16 尺度; GUI 显示)
    pub threshold_eff: f32,
    /// 上一 tick 的马达电平 (供引擎做"起振"检测: 低→高跳变)
    pub prev_l: f32,
    pub prev_r: f32,
    /// 自动校准完成 → 建议门槛值 (仅在完成的那一 tick 为 Some)
    pub calibrated: Option<u32>,
    // ── ★AI 智能增强 (B 模式) 遥测 ──
    /// 增强层是否生效 (A 模式恒 false)
    pub smart_active: bool,
    /// 距离感 0..1 (1=近, 0=远)
    pub smart_proximity: f32,
    /// 音高 0..1 (0=低沉, 1=尖锐)
    pub smart_pitch: f32,
    /// 估算震动持续时间 ms (近短远长)
    pub smart_duration_ms: f32,
}

/// 频谱分析器 (单线程使用; 引擎线程独占)。
pub struct Analyzer {
    rate: f32,
    fft: std::sync::Arc<dyn rustfft::Fft<f32>>,
    window: Vec<f32>,
    buf: VecDeque<f32>,
    /* ── 最新一帧的频带 RMS (int16 尺度) ── */
    bass_rms: f32,
    high_rms: f32,
    voc_rms: f32,
    total_rms: f32,
    /* ── 屏蔽层状态 (xbox6 逐字平移) ── */
    energy_avg: f32,
    transient_avg: f32,
    noise_floor: f32,
    mode: u8,
    /* ── 显示 ── */
    bands_smooth: [f32; NUM_BANDS],
    band_peak: [f32; NUM_BANDS],
    /* ── 马达包络 ── */
    vib_l: f32,
    vib_r: f32,
    /* ── 自动校准 ── */
    calib_active: bool,
    last_process: u64,
    /* ── ★AI 智能增强 (B 模式; A 模式完全旁路) ── */
    smart: SmartEnhancer,
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
            bass_rms: 0.0,
            high_rms: 0.0,
            voc_rms: 0.0,
            total_rms: 0.0,
            energy_avg: 0.0,
            transient_avg: 0.0,
            noise_floor: 0.0,
            mode: 0,
            bands_smooth: [0.0; NUM_BANDS],
            band_peak: [0.0; NUM_BANDS],
            vib_l: 0.0,
            vib_r: 0.0,
            calib_active: false,
            last_process: 0,
            smart: SmartEnhancer::default(),
        }
    }

    /// 推入单声道样本 (设备原始采样率)。
    pub fn push(&mut self, s: f32) {
        self.buf.push_back(s);
        if self.buf.len() > FFT_SIZE * 4 {
            let drop = self.buf.len() - FFT_SIZE * 4;
            for _ in 0..drop {
                self.buf.pop_front();
            }
        }
    }

    /// 处理全部可用 hop + 屏蔽层 + 映射 → 当前马达电平。
    pub fn process(&mut self, now_ms: u64, p: &EngineParams) -> FrameOut {
        /* ── 逐 hop 频谱: 更新四个工作频带 RMS + 显示频带 ── */
        let mut got_frame = false;
        let bass_hi = p.bass_high_hz.clamp(20, 4000) as f32;
        let voc_lo = p.vocal_low_hz.clamp(20, 8000) as f32;
        let voc_hi = (p.vocal_high_hz as f32).clamp(voc_lo + 100.0, 16000.0);
        while self.buf.len() >= FFT_SIZE {
            let frame: Vec<f32> = self.buf.iter().take(FFT_SIZE).cloned().collect();
            // 总 RMS: 时域直接算 (不加重叠窗畸变, 与 xbox6 口径一致)
            let sq: f32 = frame.iter().map(|s| s * s).sum();
            self.total_rms = (sq / FFT_SIZE as f32).sqrt() * XSCALE;
            /* ★AI 智能增强: 同频更新近期能量环形窗 (A 模式也记, 切到 B 无需预热) */
            self.smart.push_frame(self.total_rms);

            let mut cplx: Vec<Complex<f32>> = frame
                .iter()
                .enumerate()
                .map(|(i, s)| Complex { re: s * self.window[i], im: 0.0 })
                .collect();
            self.fft.process(&mut cplx);

            let half = FFT_SIZE / 2;
            let (mut bass_sq, mut high_sq, mut voc_sq) = (0f32, 0f32, 0f32);
            let mut disp_sq = [0f32; NUM_BANDS];
            for bin in 1..half {
                let freq = bin as f32 * self.rate / FFT_SIZE as f32;
                let m2 = cplx[bin].norm_sqr();
                if freq < bass_hi {
                    bass_sq += m2;
                } else if freq < 16000.0 {
                    high_sq += m2;
                }
                if freq >= voc_lo && freq < voc_hi {
                    voc_sq += m2;
                }
                if let Some(b) = band_of_bin(bin, self.rate, FFT_SIZE) {
                    disp_sq[b] += m2;
                }
            }
            self.bass_rms = bass_sq.sqrt() / RMS_NORM * XSCALE;
            self.high_rms = high_sq.sqrt() / RMS_NORM * XSCALE;
            self.voc_rms = voc_sq.sqrt() / RMS_NORM * XSCALE;
            got_frame = true;

            // 显示频带平滑 (旧口径 /256, 与 v24.59 频谱观感一致)
            for i in 0..NUM_BANDS {
                let e = disp_sq[i].sqrt() / (FFT_SIZE / 4) as f32;
                let tau: f32 = if e > self.bands_smooth[i] { 80.0 } else { 300.0 };
                let dt_ms: f32 = 10.0;
                let alpha = 1.0 - (-dt_ms / tau).exp();
                self.bands_smooth[i] += (e - self.bands_smooth[i]) * alpha;
                self.band_peak[i] = self.bands_smooth[i].max(self.band_peak[i] * 0.93);
            }

            for _ in 0..HOP {
                self.buf.pop_front();
            }
        }

        /* ── 屏蔽层 (xbox6 audio_vibration_thread 逐字平移, int16 尺度) ── */
        let total = self.total_rms;
        let voc = self.voc_rms;
        let bass = self.bass_rms;

        self.energy_avg = self.energy_avg * (1.0 - ENERGY_AVG_UP) + total * ENERGY_AVG_UP;
        let transient_raw = ((total - self.energy_avg)
            / self.energy_avg.max(TRANSIENT_DENOM_MIN))
        .clamp(0.0, 1.0);
        self.transient_avg = self.transient_avg * TRANSIENT_SLOW + transient_raw * (1.0 - TRANSIENT_SLOW);

        // 自动校准: 3 秒内只记峰值噪声底; 结束的瞬间给出建议门槛
        let calibrating = p.calibrate_until_ms != 0 && now_ms < p.calibrate_until_ms;
        let mut calibrated = None;
        if calibrating {
            self.calib_active = true;
            self.noise_floor = (self.noise_floor * 0.97).max(total).max(voc);
        } else {
            if self.calib_active {
                // 校准刚结束: xbox6 calibrated = max(120, noise_floor*1.8)
                let v = (self.noise_floor * CALIB_K).max(CALIB_FLOOR).round() as u32;
                calibrated = Some(v.clamp(0, 1200));
                self.calib_active = false;
            }
            self.noise_floor = self.noise_floor * NF_DECAY + total.min(voc) * NF_UP;
        }
        let nf = self.noise_floor;

        // 内容三分类 (带滞回)
        self.mode = classify_content(total, voc, bass, self.transient_avg, self.mode);
        let mode = self.mode;

        /* ── 生效参数 = 用户滑条 ∪ 模式覆盖 (xbox6 auto_profile 表) ── */
        let mut thr = p.mask_threshold.clamp(0, 1200) as f32;
        let mut intensity = p.master.min(150) as f32 / 100.0;
        let mut gamma = mapping::punch_gamma(p.punch_pct);
        let mut map_max = p.map_max.clamp(500, 12000) as f32;
        let mut attack = (p.attack_pct.clamp(5, 90) as f32) / 100.0;
        let mut release = (p.release_pct.clamp(2, 60) as f32) / 100.0;
        let mut dead = (p.dead_zone_pct.clamp(0, 20) as f32) / 100.0;
        let mut eff_dual = p.dual_motor;
        let mut eff_vocal_only = p.vocal_only;
        let mut music_sup = 1.0f32;
        let mut beat_on = false;
        let mut texture_on = false;

        if calibrating {
            // 校准期间保持安静, 避免校准值被自身输出干扰
            intensity = 0.0;
        }

        if p.adaptive_threshold {
            thr = thr.max(nf * NF_K_GAME);
        }

        if p.auto_profile && !calibrating {
            match mode {
                MODE_MUSIC => {
                    // 音乐段: 门槛随噪声底抬高, 连续输出按滑条压制, 鼓点脉冲放行
                    if p.adaptive_threshold {
                        thr = thr.max(nf * NF_K_MUSIC).max(THR_FLOOR_MUSIC);
                    }
                    intensity *= 0.90;
                    gamma = gamma.min(0.92);
                    attack = attack.max(0.58);
                    release = release.min(0.17);
                    music_sup = 1.0 - MUSIC_SUP_MAX * (p.music_mask_pct.min(100) as f32 / 100.0);
                    beat_on = true;
                }
                MODE_VOCAL => {
                    // 人声/细节段: 只跟人声, 强度大降 + 强扩张曲线 + 微颤纹理
                    if p.adaptive_threshold {
                        thr = thr.max(nf * NF_K_VOCAL).max(THR_FLOOR_VOCAL);
                    } else {
                        thr = (thr * 0.6).max(THR_FLOOR_VOCAL);
                    }
                    intensity *= 0.48;
                    gamma = 1.75;
                    map_max = 2300.0;
                    attack = attack.min(0.20);
                    release = release.min(0.045);
                    dead = dead.max(0.012);
                    eff_vocal_only = true;
                    texture_on = true;
                }
                MODE_QUIET => {
                    // 安静段: 门槛抬到噪声底的 2 倍; quiet_mask 开 = 完全静音
                    if p.adaptive_threshold {
                        thr = thr.max(nf * NF_K_QUIET).max(THR_FLOOR_QUIET);
                    }
                    attack = attack.min(0.16);
                    release = release.min(0.09);
                    dead = dead.max(0.05);
                    if p.quiet_mask {
                        intensity = 0.0;
                    } else {
                        intensity *= QUIET_KEEP;
                    }
                }
                _ => {}
            }
        }

        /* ── 映射层 (AudioVibration 手感 + xbox6 曲线) ──
         * 双马达分频: 低频带→左马达, 高频带→右马达;
         * 同步模式: 双马达跟同一声源 (total, 或 vocal_only 时跟人声带)。 */
        let (src_l, src_r) = if eff_dual {
            (bass, self.high_rms)
        } else {
            let s = if eff_vocal_only { voc } else { total };
            (s, s)
        };
        // 模式判定为"安静"时给映射一个绝对静音的硬保障 (total<55 门槛本来就拦,
        // 但 intensity=0 的语义要落到输出上, 不能只靠曲线)
        let mut tl = map_value_gamma(src_l, thr, map_max, gamma) * intensity;
        let mut tr = map_value_gamma(src_r, thr, map_max, gamma) * intensity;
        // 高低频量感 (AudioVibration 敏感度; 同步模式取平均, 保持左右对称)
        let (gl, gr) = if eff_dual {
            (
                p.low.min(200) as f32 / 100.0,
                p.high.min(200) as f32 / 100.0,
            )
        } else {
            let g = (p.low.min(200) + p.high.min(200)) as f32 / 200.0;
            (g, g)
        };
        tl = (tl * gl).clamp(0.0, 1.0);
        tr = (tr * gr).clamp(0.0, 1.0);

        // BGM 段: 连续输出压制 + 鼓点脉冲 (节奏力度滑条)
        if beat_on {
            tl *= music_sup;
            tr *= music_sup;
            let beat = self.transient_avg * (p.rhythm_pct.min(100) as f32 / 100.0) * intensity;
            tl = (tl + beat * BEAT_MAIN).clamp(0.0, 1.0);
            tr = (tr + beat * BEAT_AUX).clamp(0.0, 1.0);
        }

        // 人声/细节段: 呼吸包络 × 34Hz 微颤 (xbox6 texture)
        if texture_on {
            let breath = ((voc - nf * 0.85) / (900.0f32).max(thr * 4.0)).clamp(0.0, 1.0);
            let wave = 0.55
                + 0.45 * (now_ms as f32 / 1000.0 * 2.0 * std::f32::consts::PI * TEXTURE_HZ).sin();
            let tex = breath * (p.texture_pct.min(100) as f32 / 100.0) * TEXTURE_AMP * wave;
            tl = (tl + tex).clamp(0.0, 1.0);
            tr = (tr + tex * TEXTURE_R).clamp(0.0, 1.0);
        }

        /* ── ★AI 智能增强 (B 模式): 复用本帧频带能量, 不重算 FFT ──
         * A 模式 / 强度 0 / 静音 / 异常 → active=false → 下面整块跳过,
         * tl/tr/attack/release 保持经典值, 输出与改动前逐字节一致。 */
        let s_high = self.high_rms;
        let s_transient = self.transient_avg;
        let smart_out = self.smart.process(
            &SmartInput {
                total,
                bass,
                high: s_high,
                noise_floor: nf,
                map_max,
                transient: s_transient,
                threshold: thr,
            },
            &SmartParams {
                enabled: p.smart_enabled,
                strength_pct: p.smart_strength,
                duration_pct: p.smart_duration,
            },
        );
        if smart_out.active {
            /* click 是乘性脉冲: 基础输出为 0 (不该震) 时不能凭空冒出震动。
             * ★云端 CI 实测修正: 加性写法让远场从 0.000 变成 0.052。 */
            tl = (tl * smart_out.gain * smart_out.left_bias * (1.0 + smart_out.click))
                .clamp(0.0, 1.0);
            tr = (tr * smart_out.gain * smart_out.right_bias * (1.0 + smart_out.click * 0.35))
                .clamp(0.0, 1.0);
            attack = (attack * smart_out.attack_scale).clamp(0.001, 1.0);
            release = (release * smart_out.release_scale).clamp(0.001, 1.0);
        }

        // 攻/放平滑 + 死区 (xbox6); prev_* 供引擎做"起振"跳变检测
        let (prev_l, prev_r) = (self.vib_l, self.vib_r);
        self.vib_l = smooth_signal(self.vib_l, tl, attack, release);
        self.vib_r = smooth_signal(self.vib_r, tr, attack, release);
        if self.vib_l < dead {
            self.vib_l = 0.0;
        }
        if self.vib_r < dead {
            self.vib_r = 0.0;
        }

        let _ = got_frame; // 帧空缺时沿用上一帧数据 (10~20ms 陈旧度, 攻/放平滑连续)

        FrameOut {
            bands: self.bands_smooth,
            band_peak: self.band_peak,
            level_l: self.vib_l,
            level_r: self.vib_r,
            mode,
            noise_floor: nf,
            transient: self.transient_avg,
            threshold_eff: thr,
            prev_l,
            prev_r,
            calibrated,
            smart_active: smart_out.active,
            smart_proximity: smart_out.proximity,
            smart_pitch: smart_out.pitch,
            smart_duration_ms: smart_out.duration_ms,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio_haptics::mapping::Preset;

    /// 「游戏强劲」档的引擎参数 (双马达分频 + 屏蔽层全开)。
    /// ★v24.61: 测试**固定按名取档**, 不随 PRESETS 排序变化 (第一档已是人声清晰)。
    fn default_params() -> EngineParams {
        let g = mapping::preset_by_name("游戏强劲").expect("游戏强劲档存在");
        preset_params(g)
    }

    fn preset_params(p: &Preset) -> EngineParams {
        EngineParams {
            master: p.master_gain_pct,
            low: p.low_gain_pct,
            high: p.high_gain_pct,
            punch_pct: p.punch_pct,
            mask_threshold: p.mask_threshold,
            adaptive_threshold: p.adaptive_threshold,
            auto_profile: p.auto_profile,
            music_mask_pct: p.music_mask_pct,
            quiet_mask: p.quiet_mask,
            rhythm_pct: p.rhythm_pct,
            texture_pct: p.texture_pct,
            attack_pct: p.attack_pct,
            release_pct: p.release_pct,
            dead_zone_pct: p.dead_zone_pct,
            map_max: p.map_max,
            dual_motor: p.dual_motor,
            vocal_only: p.vocal_only,
            bass_high_hz: p.bass_high_hz,
            vocal_low_hz: p.vocal_low_hz,
            vocal_high_hz: p.vocal_high_hz,
            calibrate_until_ms: 0,
            smart_enabled: false,
            smart_strength: 70,
            smart_duration: 70,
        }
    }

    /// 生成正弦波 (rate Hz 采样; amp = 真幅度)
    fn sine(freq: f32, amp: f32, secs: f32, rate: f32) -> Vec<f32> {
        let n = (secs * rate) as usize;
        (0..n)
            .map(|i| amp * (2.0 * std::f32::consts::PI * freq * i as f32 / rate).sin())
            .collect()
    }

    fn noise(secs: f32, amp: f32, seed: u64) -> Vec<f32> {
        let n = (secs * 48000.0) as usize;
        let mut s = seed;
        (0..n)
            .map(|_| {
                s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                ((s >> 33) as f32 / u32::MAX as f32 - 0.5) * amp
            })
            .collect()
    }

    /// 逐 10ms 喂入并返回每 tick 输出
    fn run(an: &mut Analyzer, p: &EngineParams, samples: &[f32], now: &mut u64) -> Vec<FrameOut> {
        let mut outs = Vec::with_capacity(samples.len() / 480 + 1);
        for ch in samples.chunks(480) {
            for &s in ch {
                an.push(s);
            }
            *now += 10;
            outs.push(an.process(*now, p));
        }
        outs
    }

    fn last_max(outs: &[FrameOut], from: usize) -> (f32, f32) {
        outs[from.min(outs.len())..]
            .iter()
            .fold((0f32, 0f32), |(l, r), o| (l.max(o.level_l), r.max(o.level_r)))
    }

    #[test]
    fn band_of_bin_maps_frequency_ranges() {
        let r = 48000.0;
        assert_eq!(band_of_bin(2, r, 1024), Some(1)); // 93.75Hz → 80-250 低带
        assert_eq!(band_of_bin(1, r, 1024), Some(0)); // 46.9Hz → 20-80 超低
        assert_eq!(band_of_bin(20, r, 1024), Some(2)); // 937Hz → 250-2k 中
        assert_eq!(band_of_bin(120, r, 1024), Some(3)); // 5.6kHz → 2k-8k 高
        assert_eq!(band_of_bin(510, r, 1024), Some(4)); // 23.9kHz → 8k+ 超高
        assert_eq!(band_of_bin(0, r, 1024), None, "DC 不计");
    }

    /// 映射隔离档 (关掉智能识别, 纯看 AudioVibration 式映射):
    /// 纯低频音 → 左马达明显强于右马达 (双马达分频)。
    #[test]
    fn bass_sine_drives_left_motor() {
        let mut an = Analyzer::new(48000.0);
        let mut now = 1000u64;
        let mut p = default_params();
        p.auto_profile = false;
        // 1.5s 100Hz 正弦 (BGM 贝斯量级: 归一 0.3 ≈ int16 尺度 6941, 远超门槛 400)
        let outs = run(&mut an, &p, &sine(100.0, 0.3, 1.5, 48000.0), &mut now);
        let (l, r) = last_max(&outs, 40);
        assert!(l > 0.30, "低频音应驱动左马达, 实际 l={l}");
        assert!(l > r * 3.0, "左马达应远强于右马达: l={l} r={r}");
    }

    /// ★AI 智能增强验收红线: A 模式与「强度 0」逐帧输出完全相同,
    /// 且 A 模式全程 smart_active=false (增强层完全旁路, 旧行为逐字节不变)。
    #[test]
    fn a_mode_is_identical_to_zero_strength() {
        let sig = sine(100.0, 0.3, 1.5, 48000.0);
        let mut pa = default_params();
        pa.auto_profile = false;
        pa.smart_enabled = false;
        let mut pz = default_params();
        pz.auto_profile = false;
        pz.smart_enabled = true;
        pz.smart_strength = 0;
        let mut a = Analyzer::new(48000.0);
        let mut z = Analyzer::new(48000.0);
        let (mut na, mut nz) = (1000u64, 1000u64);
        let oa = run(&mut a, &pa, &sig, &mut na);
        let oz = run(&mut z, &pz, &sig, &mut nz);
        assert_eq!(oa.len(), oz.len());
        for (x, y) in oa.iter().zip(oz.iter()) {
            assert!(!x.smart_active, "A 模式增强层必须全程旁路");
            assert_eq!(x.level_l, y.level_l, "A 模式左马达与强度0必须逐帧相同");
            assert_eq!(x.level_r, y.level_r, "A 模式右马达与强度0必须逐帧相同");
        }
    }

    /// ★B 模式确实拉开「近(响) / 远(轻)」的力度差, 而不只是换个数字。
    #[test]
    fn smart_mode_amplifies_distance_contrast() {
        let near = sine(100.0, 0.35, 1.5, 48000.0);
        let far = sine(100.0, 0.03, 1.5, 48000.0);
        let mk = |smart: bool| {
            let mut p = default_params();
            p.auto_profile = false;
            /* 本测试只验「智能层对远近的相对改写」, 关掉门槛/自适应/人声带门控,
             * 保证 100Hz 近/远两档都稳定有基础输出, 不被内容分类或噪声底吞成 0。 */
            p.adaptive_threshold = false;
            p.mask_threshold = 0;
            p.vocal_only = false;
            p.smart_enabled = smart;
            p.smart_strength = 100;
            p.smart_duration = 100;
            p
        };
        let feed = |sig: &[f32], p: &EngineParams| {
            let mut an = Analyzer::new(48000.0);
            let mut now = 1000u64;
            let outs = run(&mut an, p, sig, &mut now);
            last_max(&outs, 40).0
        };
        let a_near = feed(&near, &mk(false));
        let a_far = feed(&far, &mk(false));
        let b_near = feed(&near, &mk(true));
        let b_far = feed(&far, &mk(true));
        /* 近场在 A 模式已接近满幅 (clamp 到 1), 用「差值变大」会被天花板吃掉,
         * 因此分别断言两条单调性: 远场必须被削弱, 近场不能变弱。 */
        assert!(a_far > 0.0, "测试前提: 远场在 A 模式应有基础输出, 实得 {a_far}");
        assert!(b_far < a_far,
            "远场在 B 模式应更轻: A {a_far:.3} vs B {b_far:.3}");
        assert!(b_near >= a_near - 1e-6,
            "近场在 B 模式不应变弱: A {a_near:.3} vs B {b_near:.3}");
    }

    /// 映射隔离档: 高频音 → 右马达主导。
    #[test]
    fn high_sine_drives_right_motor() {
        let mut an = Analyzer::new(48000.0);
        let mut now = 1000u64;
        let mut p = default_params();
        p.auto_profile = false;
        let outs = run(&mut an, &p, &sine(3000.0, 0.25, 1.5, 48000.0), &mut now);
        let (l, r) = last_max(&outs, 40);
        assert!(r > 0.30, "高频音应驱动右马达, 实际 r={r}");
        assert!(r > l * 3.0, "右马达应远强于左马达: l={l} r={r}");
    }

    /// 屏蔽门槛: 低于门槛的声音不震 (AudioVibration 硬门槛语义)。
    #[test]
    fn below_threshold_signal_is_silent() {
        let mut an = Analyzer::new(48000.0);
        let mut now = 1000u64;
        let mut p = default_params();
        p.auto_profile = false;
        // 0.008 归一幅度 ≈ int16 尺度 185 < 门槛 400
        let outs = run(&mut an, &p, &sine(100.0, 0.008, 1.5, 48000.0), &mut now);
        let (l, r) = last_max(&outs, 40);
        assert!(l < 0.02 && r < 0.02, "低于门槛不应震动: l={l} r={r}");
    }

    /// 安静段静音: 只有微弱背景噪声时 (quiet_mask 开) 输出归零。
    #[test]
    fn quiet_segment_is_silenced() {
        let mut an = Analyzer::new(48000.0);
        let mut now = 1000u64;
        // 均匀噪声 rms = amp/√12 → 0.004 幅度 ≈ int16 尺度 38 < 55 = 安静判定
        let outs = run(&mut an, &default_params(), &noise(2.0, 0.004, 42), &mut now);
        let (l, r) = last_max(&outs, 80);
        assert!(l < 0.02 && r < 0.02, "安静段应静音: l={l} r={r}");
        // 判定确实落在安静段
        assert!(
            outs.iter().filter(|o| o.mode == MODE_QUIET).count() > 100,
            "微弱噪声应被判为安静段"
        );
    }

    /// BGM 段压制: 稳态音乐 (和声+鼓点) 被判为音乐段后, 连续输出被压制。
    /// 对照: 关掉压制 (mask=0) 后输出明显更高。
    #[test]
    fn bgm_segment_suppressed_but_beat_survives() {
        // 合成 BGM: 220Hz+330Hz 和声铺底 (归一 0.15/0.10) + 每 0.5s 一记 60Hz 鼓点
        let bgm = bgm_with_kicks(6.0, 0.35);
        let mut p_mask = default_params();
        p_mask.music_mask_pct = 100;
        p_mask.rhythm_pct = 0; // 只看连续输出被压制的部分
        let mut p_nomask = default_params();
        p_nomask.music_mask_pct = 0;
        p_nomask.rhythm_pct = 0;

        let mut a = Analyzer::new(48000.0);
        let mut now = 1000u64;
        let outs_mask = run(&mut a, &p_mask, &bgm, &mut now);
        let mut a = Analyzer::new(48000.0);
        let mut now = 1000u64;
        let outs_nomask = run(&mut a, &p_nomask, &bgm, &mut now);

        // 取稳态段 (1.5s 后, 避开起音)
        let (l_m, r_m) = last_max(&outs_mask, 160);
        let (l_n, r_n) = last_max(&outs_nomask, 160);
        assert!(
            l_m > 0.0 || r_m > 0.0,
            "BGM 段不应完全无声 (和声仍可能过门槛): l={l_m} r={r_m}"
        );
        let m = l_m.max(r_m);
        let n = l_n.max(r_n);
        assert!(
            m < n * 0.55,
            "音乐段压制应明显压低连续输出: mask={m:.3} nomask={n:.3}"
        );
        // 至少一半 tick 判定为音乐段
        assert!(
            outs_mask.iter().filter(|o| o.mode == MODE_MUSIC).count() > 300,
            "稳态和声+鼓点应被识别为音乐段"
        );
    }

    /// 合成 BGM: 和声铺底 + 周期鼓点。
    fn bgm_with_kicks(secs: f32, kick_amp: f32) -> Vec<f32> {
        const PI: f32 = std::f32::consts::PI;
        let rate = 48000.0f32;
        let n = (secs * rate) as usize;
        let mut v: Vec<f32> = (0..n)
            .map(|i| {
                let t = i as f32 / rate;
                0.12 * (2.0 * PI * 220.0 * t).sin() + 0.08 * (2.0 * PI * 329.6 * t).sin()
            })
            .collect();
        let mut k = 0.0f32;
        while k < secs {
            let start = (k * rate) as usize;
            for j in 0..(0.12 * rate) as usize {
                if start + j < n {
                    let t = j as f32 / rate;
                    v[start + j] += kick_amp * (-t / 0.04).exp() * (2.0 * PI * 60.0 * t).sin();
                }
            }
            k += 0.5;
        }
        v
    }

    /// 自动校准: 3 秒背景噪声 → 返回建议门槛 ≈ max(120, 噪声底×1.8)。
    #[test]
    fn calibration_produces_threshold_suggestion() {
        let mut an = Analyzer::new(48000.0);
        let mut now = 1000u64;
        // 背景噪声 (int16 尺度 rms ≈ 0.01*32768/sqrt(3) ≈ 189)
        let mut p = default_params();
        p.calibrate_until_ms = now + CALIB_SECS;
        let noise_bg = noise(CALIB_SECS as f32 / 1000.0 + 0.5, 0.01, 777);
        let outs = run(&mut an, &p, &noise_bg, &mut now);
        let cal: Vec<u32> = outs.iter().filter_map(|o| o.calibrated).collect();
        assert_eq!(cal.len(), 1, "校准只应返回一次建议, 实际 {:?}", cal);
        let v = cal[0];
        let expected_min = (120.0 * 0.9) as u32;
        let expected_max = (0.01 * XSCALE * 1.8 * 1.5) as u32; // 峰值×1.8, 容差 1.5x
        assert!(
            v >= expected_min && v <= expected_max,
            "建议门槛 {v} 应在 [{expected_min}, {expected_max}]"
        );
    }

    /// 响应速度: 突然出现的响声应在几十 ms 内顶起输出 (attack 快; 映射隔离档)。
    /// 断言用 0.15 相对标准: 测试用的 LCG 伪随机噪声低频能量偏重 (非理想白噪),
    /// 绝对幅度预估不稳; 正弦类精确测试 (bass/high_sine) 已覆盖映射标定。
    #[test]
    fn loud_sudden_sound_ramps_up_fast() {
        let mut an = Analyzer::new(48000.0);
        let mut now = 1000u64;
        let mut p = default_params();
        p.auto_profile = false;
        for s in sine(60.0, 0.0, 1.0, 48000.0) {
            an.push(s);
        }
        let _ = an.process(now, &p);
        now += 1000;
        // 突然注入响亮宽带声 (均匀噪声 0.5 → int16 尺度 rms ≈ 4730, 高频带 ≈ 3746)
        let mut outs = Vec::new();
        for ch in noise(0.30, 0.50, 555).chunks(480) {
            for &s in ch {
                an.push(s);
            }
            now += 10;
            outs.push(an.process(now, &p));
        }
        let (l, r) = last_max(&outs, 3);
        assert!(
            l > 0.15 || r > 0.15,
            "响声应快速顶起输出 (招招有震动): l={l} r={r}"
        );
    }

    /// 余韵: 声音停了以后输出按 release 平滑收尾, 不粘滞。
    #[test]
    fn output_decays_after_sound_stops() {
        let mut an = Analyzer::new(48000.0);
        let mut now = 1000u64;
        let mut hot = run(&mut an, &default_params(), &sine(100.0, 0.3, 1.0, 48000.0), &mut now);
        let tail = run(&mut an, &default_params(), &sine(60.0, 0.0, 0.8, 48000.0), &mut now);
        hot.extend(tail);
        let (l, r) = last_max(&hot, hot.len() - 10);
        assert!(l < 0.05 && r < 0.05, "声音停止后应收尾归零: l={l} r={r}");
    }

    /// 弱信号跟随 (AudioVibration 灵敏度): 超过门槛但远小于满量程的信号,
    /// 经 gamma 压缩后仍有可观输出 —— 这是"招招有震动"与旧 onset 引擎的本质区别。
    #[test]
    fn moderate_signal_still_vibrates() {
        let mut an = Analyzer::new(48000.0);
        let mut now = 1000u64;
        let mut p = default_params();
        p.auto_profile = false;
        // 归一 0.05 ≈ int16 尺度 1159: 过门槛 (400) 但只有满量程 (6000) 的 ~13%
        let outs = run(&mut an, &p, &sine(100.0, 0.05, 1.5, 48000.0), &mut now);
        let (l, _) = last_max(&outs, 60);
        assert!(l > 0.05, "中等响度也应有明显震动: l={l}");
    }
}
