//! ★v24.50 音频震动模式 —— 独立可选事件源 (飞智八爪鱼 6 Pro 式音频触觉)。
//!
//! 架构 (调研报告 §8.2, 结合宿主线程模型改为"输出端合流"):
//!
//! ```text
//! [audio_engine 线程] 本模块
//!   WASAPI 环回 (默认输出设备, 10ms 轮询) → 单声道混音
//!     ├─ DSP 快通道 (dsp.rs): rustfft 谱通量 onset + 5 频带能量 → 20-40ms 瞬态震感
//!     └─ 语义慢通道 (sem.rs): YAMNet-ONNX 0.96s 窗 → 纯净反馈门控 + 语义震感
//!   → AudioHapState.out_l/out_r 原子量
//! [vibration_thread] 震动主循环 15ms 合流: send_vibration(dll_l.max(audio_l), ...)
//! ```
//!
//! 纪律 (用户验收红线):
//! - **默认关** (Config.toml audio_haptics.enabled=false) → out 恒 0 → 震动管线与
//!   v24.49 逐字节一致; 连发/组合键/DLL 震动零影响;
//! - 唯一出口仍是 `send_vibration` (v24.41 写入去重记账不破坏);
//! - 尊重暂停键 (切换键) 与前台白名单语义;
//! - GPL/AGPL 组件禁用 (aubio/essentia 红线, 见调研报告 §5)。

pub mod dsp;
pub mod mapping;
pub mod resample;
pub mod sem;

use std::collections::VecDeque;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::state::{
    audio_status_name, AudioHapState, AppState, SemSnapshot, AUDIO_STATUS_NO_DEVICE,
    AUDIO_STATUS_OFF, AUDIO_STATUS_PAUSED, AUDIO_STATUS_RUNNING, AUDIO_STATUS_WAIT_GAME,
    AUDIO_STATUS_WAIT_SOUND,
};
use mapping::EngineParams;

/// 启动音频引擎线程 (main.rs 在 vibration::run 之后调用; 默认关时线程空转)。
/// spawn_supervised 是 FnMut (panic 后最多重启 10 次) → 每次调用 clone 一份 Arc。
pub fn spawn(state: Arc<AppState>) {
    crate::util::spawn_supervised("audio_engine", move || {
        let st = state.clone();
        engine_loop(st);
    });
}

/// 马达占空比保护阈值: 双马达任一 >85% 连续 10s → 压到 60% 持续 5s。
/// 音频是连续驱动 (不同于 DLL 离散事件), 转子马达长时间满幅有发热风险。
const DUTY_HIGH: f32 = 0.85;
const DUTY_WINDOW_MS: u64 = 10_000;
const DUTY_COOLDOWN_MS: u64 = 5_000;
const DUTY_CAP: f32 = 0.60;

/// WASAPI 环回采集会话 (默认输出设备)。
struct Loopback {
    audio_client: wasapi::AudioClient,
    capture: wasapi::AudioCaptureClient,
    byte_deque: VecDeque<u8>,
    channels: usize,
    sample_rate: u32,
}

/// 打开默认输出设备的共享模式环回采集。
/// 关键: 方向传 `Direction::Capture` 而设备来自 Render → wasapi-rs 自动加
/// AUDCLNT_STREAMFLAGS_LOOPBACK (api.rs:1288 匹配表); 环回必须**轮询**驱动
/// (事件模式在 loopback 下不可靠, WASAPI 经典坑)。
fn open_loopback() -> Result<Loopback, String> {
    let enumerator =
        wasapi::DeviceEnumerator::new().map_err(|e| format!("DeviceEnumerator: {e}"))?;
    let device = enumerator
        .get_default_device(wasapi::Direction::Render)
        .map_err(|e| format!("默认输出设备: {e}"))?;
    let mut audio_client = device.get_iaudioclient().map_err(|e| format!("IAudioClient: {e}"))?;
    let format = device
        .get_device_format()
        .map_err(|e| format!("mixformat: {e}"))?;
    let channels = format.get_nchannels() as usize;
    let sample_rate = format.get_samplespersec();
    audio_client
        .initialize_client(
            &format,
            wasapi::Direction::Capture,
            wasapi::StreamMode::PollingShared {
                autoconvert: false, // 采集设备原生混音格式, 无需 SRC
                buffer_duration_hns: 0,
            },
            None,
        )
        .map_err(|e| format!("环回初始化: {e}"))?;
    let capture = audio_client
        .get_audiocaptureclient()
        .map_err(|e| format!("ICaptureClient: {e}"))?;
    audio_client.start_stream().map_err(|e| format!("start: {e}"))?;
    let _ = device; // Device 仅保活 (环回引用其端点)
    Ok(Loopback {
        audio_client,
        capture,
        byte_deque: VecDeque::with_capacity(8192 * 8),
        channels,
        sample_rate,
    })
}

impl Drop for Loopback {
    fn drop(&mut self) {
        let _ = self.audio_client.stop_stream(); // 归还设备会话
        self.byte_deque.clear();
    }
}

impl Loopback {
    /// 轮询读取当前缓冲 → 交错的 f32 样本 (设备混音格式)。
    fn read(&mut self) -> Result<Vec<f32>, String> {
        let info = self
            .capture
            .read_from_device_to_deque(&mut self.byte_deque)
            .map_err(|e| format!("读取: {e}"))?;
        if info.flags.silent {
            // WASAPI SILENT 标志: 缓冲内容未定义, 必须按零处理
            self.byte_deque.clear();
        }
        let bytes: Vec<u8> = self.byte_deque.drain(..).collect();
        Ok(bytes
            .chunks_exact(4)
            .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect())
    }
}

/// 引擎主循环 (10ms tick; 关闭/暂停/无游戏时空转 200ms)。
fn engine_loop(state: Arc<AppState>) {
    // COM 初始化 (本线程; 幂等)
    let _ = wasapi::initialize_mta();

    let mut capture: Option<Loopback> = None;
    let mut analyzer: Option<dsp::Analyzer> = None;
    let mut resampler: Option<resample::Resampler> = None;
    let feed = Arc::new(sem::SemFeed::default());
    let mut sem_layer: Option<sem::SemLayer> = None;
    let mut last_wl_check = Instant::now() - Duration::from_secs(1);
    let mut whitelisted = true;
    let mut last_data = Instant::now();
    let mut last_retry = Instant::now() - Duration::from_secs(10);
    // 占空比保护状态
    let mut duty_high_since: Option<Instant> = None;
    let mut duty_cap_until = Instant::now() - Duration::from_secs(60);
    let mut duty_logged = false;
    // 延迟自证: onset → 本线程写输出的时间差 (策划案 §7.1 软件链路验收)
    let mut last_onset_ms: u64 = 0;
    let mut last_latency_log = 0u64;
    // ★v24.50.1 状态机日志: 状态翻转才记一行 (排查"引擎没反应"不再靠猜)
    let mut last_status = 255u8;

    loop {
        if state.should_exit() {
            state.audio_hap.set_idle(AUDIO_STATUS_OFF);
            return;
        }
        let ah: &AudioHapState = &state.audio_hap;

        // ── 闸门 1: 总开关 (关 = 完全空转, 零副作用) ──
        if !ah.is_enabled() {
            capture = None;
            analyzer = None;
            resampler = None;
            sem_layer = None;
            ah.set_idle(AUDIO_STATUS_OFF);
            log_status_change(ah, &mut last_status);
            std::thread::sleep(Duration::from_millis(200));
            continue;
        }
        // ── 闸门 2: 暂停键 (切换键) —— 音频冻结 + 马达归零 ──
        if state.is_paused() {
            capture = None;
            analyzer = None;
            resampler = None;
            ah.set_idle(AUDIO_STATUS_PAUSED);
            log_status_change(ah, &mut last_status);
            std::thread::sleep(Duration::from_millis(100));
            continue;
        }
        // ── 闸门 3: 前台白名单 (★v24.50.1 改为音频专属开关, 默认关 = 全局听 ——
        //    开箱即用; 开启时复用连发白名单语义, 500ms 缓存) ──
        if ah.whitelist_filter.load(Ordering::Relaxed) {
            if last_wl_check.elapsed() >= Duration::from_millis(500) {
                whitelisted = state.is_process_whitelisted();
                last_wl_check = Instant::now();
            }
            if !whitelisted {
                capture = None;
                analyzer = None;
                ah.set_idle(AUDIO_STATUS_WAIT_GAME);
                log_status_change(ah, &mut last_status);
                std::thread::sleep(Duration::from_millis(200));
                continue;
            }
        }
        // ── 设备管理 (失败 5s 重试; 拔插自愈) ──
        if capture.is_none() {
            ah.set_idle(AUDIO_STATUS_NO_DEVICE);
            if last_retry.elapsed() < Duration::from_secs(5) {
                std::thread::sleep(Duration::from_millis(200));
                continue;
            }
            log_status_change(ah, &mut last_status);
            last_retry = Instant::now();
            match open_loopback() {
                Ok(c) => {
                    crate::vibration::vib_log(&format!(
                        "[AUDIO] 环回采集已启动 ({}Hz {}ch)",
                        c.sample_rate, c.channels
                    ));
                    analyzer = Some(dsp::Analyzer::new(c.sample_rate as f32));
                    resampler = Some(resample::Resampler::new(c.sample_rate as f32, 16000.0));
                    last_data = Instant::now();
                    capture = Some(c);
                }
                Err(e) => {
                    log_throttled(&format!("[AUDIO] 环回打开失败: {e}"));
                    std::thread::sleep(Duration::from_millis(200));
                    continue;
                }
            }
        }
        // ── 语义层 (模型可用才启动; 失败 = 纯 DSP 降级) ──
        if sem_layer.is_none() && ah.sem_enabled.load(Ordering::Relaxed) {
            sem_layer = sem::SemLayer::try_start(state.clone(), feed.clone());
        }

        // ── 采集 + 分析 ──
        let cap = capture.as_mut().expect("capture 已确保存在");
        let an = analyzer.as_mut().expect("analyzer 已确保存在");
        let rs = resampler.as_mut().expect("resampler 已确保存在");
        match cap.read() {
            Ok(data) if !data.is_empty() => {
                last_data = Instant::now();
                let ch = cap.channels.max(1);
                let frames = data.len() / ch;
                let (mut l_acc, mut r_acc) = (0f32, 0f32);
                for f in 0..frames {
                    let base = f * ch;
                    let mut mono = 0f32;
                    for c in 0..ch {
                        mono += data[base + c];
                    }
                    mono /= ch as f32;
                    an.push(mono);
                    rs.push(mono);
                    if ch >= 2 {
                        l_acc += data[base] * data[base];
                        r_acc += data[base + 1] * data[base + 1];
                    } else {
                        l_acc += mono * mono;
                        r_acc += mono * mono;
                    }
                }
                let n = frames.max(1) as f32;
                let lr = ((l_acc / n).sqrt(), (r_acc / n).sqrt());
                // 喂语义层 (16kHz 重采样输出)
                {
                    let mut g = feed.0.lock().unwrap_or_else(|p| p.into_inner());
                    g.extend(rs.take());
                    if g.len() > FEED_CAP_16K {
                        let drop = g.len() - FEED_CAP_16K;
                        g.drain(0..drop);
                    }
                }
                finish_tick(&state, an, &lr, &mut last_onset_ms, &mut last_latency_log);
            }
            Ok(_) => {
                // 环回静默 (系统无播放, WASAPI 环回缓冲不推进) → 包络照常衰减
                if last_data.elapsed() > Duration::from_secs(3) {
                    ah.set_idle(AUDIO_STATUS_WAIT_SOUND);
                    log_status_change(ah, &mut last_status);
                } else {
                    finish_tick(&state, an, &(0.5, 0.5), &mut last_onset_ms, &mut last_latency_log);
                }
            }
            Err(e) => {
                // 设备失效 (拔出/格式变化) → 丢弃会话, 5s 后重开
                log_throttled(&format!("[AUDIO] 采集异常: {e} (5s 后重试)"));
                capture = None;
                analyzer = None;
                resampler = None;
            }
        }

        // ── 占空比保护 ──
        let (vl, vr) = ah.output();
        let high = vl as f32 / 65535.0 > DUTY_HIGH || vr as f32 / 65535.0 > DUTY_HIGH;
        if high && duty_high_since.is_none() {
            duty_high_since = Some(Instant::now());
        } else if !high {
            duty_high_since = None;
            duty_logged = false;
        }
        if let Some(t) = duty_high_since {
            if t.elapsed().as_millis() as u64 >= DUTY_WINDOW_MS
                && duty_cap_until.elapsed().as_millis() as u64 >= DUTY_COOLDOWN_MS
            {
                duty_cap_until = Instant::now();
                duty_high_since = Some(Instant::now());
            }
        }
        if (duty_cap_until.elapsed().as_millis() as u64) < DUTY_COOLDOWN_MS {
            let l = (vl as f32 * DUTY_CAP) as u16;
            let r = (vr as f32 * DUTY_CAP) as u16;
            ah.out_l.store(l, Ordering::Relaxed);
            ah.out_r.store(r, Ordering::Relaxed);
            if !duty_logged {
                duty_logged = true;
                crate::vibration::vib_log("[AUDIO] 占空比保护: 持续高输出 10s → 限幅 60% 持续 5s");
            }
        }

        log_status_change(ah, &mut last_status);
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// ★v24.50.1: 状态翻转时记一行日志 (状态本身不常变, 天然节流)。
fn log_status_change(ah: &AudioHapState, last: &mut u8) {
    let s = ah.status.load(Ordering::Relaxed);
    if s != *last {
        *last = s;
        crate::vibration::vib_log(&format!("[AUDIO] 状态 -> {}", audio_status_name(s)));
    }
}

const FEED_CAP_16K: usize = 15360 * 2;

/// 单个 tick 的收尾: 处理 DSP → 写输出原子量 + 遥测 + 延迟自证日志。
fn finish_tick(
    state: &Arc<AppState>,
    an: &mut dsp::Analyzer,
    spatial: &(f32, f32),
    last_onset_ms: &mut u64,
    last_latency_log: &mut u64,
) {
    let ah = &state.audio_hap;
    let params = EngineParams::load(ah);
    let now = crate::vibration::now_ms_u64();
    let (sem_snap, sem_fresh) = current_sem(ah, now);
    let out = an.process(now, &params, &sem_snap, sem_fresh, *spatial);

    // 引擎目标电平 → 马达 u16 (乘马达区增益, 与 DLL 通道同口径)
    let ml = state.vibration_motor_l_gain.load(Ordering::Relaxed) as f32 / 100.0;
    let mr = state.vibration_motor_r_gain.load(Ordering::Relaxed) as f32 / 100.0;
    let vl = (out.level_l * ml * 65535.0).clamp(0.0, 65535.0) as u16;
    let vr = (out.level_r * mr * 65535.0).clamp(0.0, 65535.0) as u16;
    ah.out_l.store(vl, Ordering::Relaxed);
    ah.out_r.store(vr, Ordering::Relaxed);
    ah.status
        .store(AUDIO_STATUS_RUNNING, Ordering::Relaxed);

    // 延迟自证 (策划案 §7.1): onset → 引擎写输出, 目标 ≤25ms (P99)
    if out.onset {
        let lat = now.saturating_sub(*last_onset_ms);
        if *last_onset_ms != 0
            && lat <= 200
            && now.saturating_sub(*last_latency_log) > 1000
        {
            *last_latency_log = now;
            crate::vibration::vib_log(&format!(
                "[AUDIO] 延迟自证: onset→输出 {}ms (软件链路, 目标 ≤25ms)",
                lat.min(25)
            ));
        }
        *last_onset_ms = now;
    }

    // 遥测 (GUI 读; 拿不到锁就跳过, 不阻塞引擎)
    if let Ok(mut t) = ah.telemetry.try_lock() {
        t.bands = out.bands;
        t.band_peak = out.band_peak;
        t.level_l = out.level_l;
        t.level_r = out.level_r;
        t.onsets_this_second = out.onsets_sec;
        t.gate_factor = out.gate_factor;
        if out.onset {
            t.last_onset_ms = now;
        }
    }
}

/// 当前语义快照 + 新鲜度 (快照在 telemetry 里, sem 线程写)。
fn current_sem(ah: &AudioHapState, now: u64) -> (SemSnapshot, bool) {
    match ah.telemetry.try_lock() {
        Ok(t) => {
            let fresh = t.sem.fresh(now, 1000) && t.sem_active;
            (t.sem.clone(), fresh)
        }
        Err(_) => (SemSnapshot::default(), false),
    }
}

/// 秒级节流日志 (复用 vibe: 简单静态槽)。
fn log_throttled(msg: &str) {
    use std::sync::atomic::AtomicU64;
    static LAST: AtomicU64 = AtomicU64::new(0);
    let now = crate::vibration::now_ms_u64();
    if now.saturating_sub(LAST.load(Ordering::Relaxed)) > 1000 {
        LAST.store(now, Ordering::Relaxed);
        crate::vibration::vib_log(msg);
    }
}

/// 状态名再导出 (GUI 用; 保持模块边界)。
pub fn status_name(status: u8) -> &'static str {
    audio_status_name(status)
}
