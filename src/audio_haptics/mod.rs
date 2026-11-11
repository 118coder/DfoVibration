//! ★v24.60 音频震动模式 —— 独立可选事件源 (飞智八爪鱼 6 Pro 式音频触觉)。
//!
//! 架构 (v24.60 换代: 删除 YAMNet/MRX AI 层, 纯算法电平跟随引擎):
//!
//! ```text
//! [audio_engine 线程] 本模块
//!   WASAPI 环回 (默认输出设备或按进程环回, 10ms 轮询) → 单声道混音
//!     └─ 融合引擎 (dsp.rs): 频带电平跟随 + 噪声底自适应门槛 + 内容三分类
//!        (骨架=xbox-audio-rumble-studio, 映射=audio-to-controller-vibration)
//!   → AudioHapState.out_l/out_r 原子量
//! [vibration_thread] 震动主循环 15ms 合流: send_vibration(dll_l.max(audio_l), ...)
//! ```
//!
//! 纪律 (用户验收红线):
//! - **默认关** (Config.toml audio_haptics.enabled=false) → out 恒 0 → 震动管线与
//!   旧版逐字节一致; 连发/组合键/DLL 震动零影响;
//! - 唯一出口仍是 `send_vibration` (v24.41 写入去重记账不破坏);
//! - 尊重暂停键 (切换键) 与前台白名单语义;
//! - GPL/AGPL 组件禁用 (aubio/essentia 红线)。
//!
//! ★v24.51 修复 (实机"引擎运行中但马达无感"):
//! - 采集层按实际 `WaveFormat` 解码 (旧代码无条件把 4 字节当 f32, 非 float 设备直接全哑);
//! - 音频专属白名单: 独立列表 + 可关闭 + fail-closed + 按 PID 精确环回;
//! - 自检脉冲: 绕过 DSP 直写马达, 一击区分"音频链"与"马达链"。

pub mod dsp;
pub mod mapping;
pub mod resample;

use std::collections::VecDeque;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::state::{
    audio_status_name, AudioHapState, AppState, AUDIO_STATUS_NO_DEVICE,
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

/// ★v24.51: 设备混音格式的采样类型。旧实现无条件按 `chunks_exact(4)` 把字节
/// 当 IEEE-float 解析; 若默认输出设备的混音格式是 16/24 位整型, 解出来的"样本"
/// 是乱码 (幅值 ~1e-38 或饱和噪声) → DSP 能量恒 0 → 静音阈值永远拦住 → 马达全哑。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SampleKind {
    F32,
    I16,
    I24Packed,
    I32,
}

/// WASAPI 环回采集会话。
/// `pid = Some` 表示按进程环回 (只读该 PID 的音频); `None` = 默认输出设备全量环回。
struct Loopback {
    audio_client: wasapi::AudioClient,
    capture: wasapi::AudioCaptureClient,
    byte_deque: VecDeque<u8>,
    channels: usize,
    sample_rate: u32,
    bytes_per_sample: usize,
    sample_kind: SampleKind,
    pid: Option<u32>,
}

/// 按 PID 精确环回 (Win10 2004+/19041+)。wasapi 0.27 的 process loopback
/// 必须 `Direction::Capture` + `ShareMode::Shared`; 用自带 float32 格式 +
/// autoconvert 让音频引擎负责格式/采样率转换, 采集侧永远拿到 f32。
fn open_process_loopback(
    pid: u32,
    include_tree: bool,
) -> Result<Loopback, String> {
    let mut audio_client =
        wasapi::AudioClient::new_application_loopback_client(pid, include_tree)
            .map_err(|e| format!("进程环回客户端: {e}"))?;
    let format = wasapi::WaveFormat::new(
        32,
        32,
        wasapi::SampleType::Float,
        48000,
        2,
        None,
    );
    audio_client
        .initialize_client(
            &format,
            wasapi::Direction::Capture,
            wasapi::StreamMode::PollingShared {
                autoconvert: true,
                buffer_duration_hns: 200_000, // 20ms
            },
            None,
        )
        .map_err(|e| format!("进程环回初始化: {e}"))?;
    let capture = audio_client
        .get_audiocaptureclient()
        .map_err(|e| format!("进程环回 ICaptureClient: {e}"))?;
    audio_client
        .start_stream()
        .map_err(|e| format!("进程环回 start: {e}"))?;
    Ok(Loopback {
        audio_client,
        capture,
        byte_deque: VecDeque::with_capacity(8192 * 8),
        channels: 2,
        sample_rate: 48000,
        bytes_per_sample: 4,
        sample_kind: SampleKind::F32,
        pid: Some(pid),
    })
}

/// 打开默认输出设备的共享模式环回采集。
/// 关键: 方向传 `Direction::Capture` 而设备来自 Render → wasapi-rs 自动加
/// AUDCLNT_STREAMFLAGS_LOOPBACK (api.rs:1289 匹配表); 环回必须**轮询**驱动
/// (事件模式在 loopback 下不可靠, WASAPI 经典坑)。
fn open_device_loopback() -> Result<(Loopback, SampleKind), String> {
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
    let bits = format.get_bitspersample() as usize;
    let is_float = matches!(format.get_subformat(), Ok(wasapi::SampleType::Float));
    let bytes_per_sample = bits.div_ceil(8).max(1);
    let sample_kind = if is_float && bits == 32 {
        SampleKind::F32
    } else if !is_float && bits == 16 {
        SampleKind::I16
    } else if !is_float && bits == 24 && bytes_per_sample == 3 {
        SampleKind::I24Packed
    } else if !is_float && bits >= 24 && bytes_per_sample == 4 {
        // 24-in-32 容器: 有效位左对齐, 按 i32 / 2^31 归一即可
        SampleKind::I32
    } else {
        return Err(format!(
            "不支持的设备混音格式 (float={is_float}, {bits}bit, {bytes_per_sample}B/样本)"
        ));
    };
    audio_client
        .initialize_client(
            &format,
            wasapi::Direction::Capture,
            wasapi::StreamMode::PollingShared {
                autoconvert: false, // 采集设备原生混音格式, 由 read() 自行解码
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
    Ok((
        Loopback {
            audio_client,
            capture,
            byte_deque: VecDeque::with_capacity(8192 * 8),
            channels,
            sample_rate,
            bytes_per_sample,
            sample_kind,
            pid: None,
        },
        sample_kind,
    ))
}

/// 统一入口: 白名单 process 模式优先按 PID 精确环回; 失败 (旧系统/目标进程
/// 尚未建立音频流) 才回退默认设备全量环回, 并写一行日志明示降级。
fn open_loopback(
    target_pid: Option<u32>,
    include_tree: bool,
) -> Result<(Loopback, &'static str), String> {
    if let Some(pid) = target_pid {
        match open_process_loopback(pid, include_tree) {
            Ok(c) => return Ok((c, "进程环回")),
            Err(e) => {
                crate::vibration::vib_log(&format!(
                    "[AUDIO] 进程环回不可用 (pid={pid}): {e} → 回退默认设备环回(仅前台门控)"
                ));
            }
        }
    }
    let (c, kind) = open_device_loopback()?;
    Ok((c, match kind {
        SampleKind::F32 => "默认设备环回/FLOAT32",
        SampleKind::I16 => "默认设备环回/INT16",
        SampleKind::I24Packed => "默认设备环回/INT24",
        SampleKind::I32 => "默认设备环回/INT32",
    }))
}

impl Drop for Loopback {
    fn drop(&mut self) {
        let _ = self.audio_client.stop_stream(); // 归还设备会话
        self.byte_deque.clear();
    }
}

impl Loopback {
    /// 轮询读取当前缓冲 → 交错的 f32 样本 (按设备真实格式解码, 归一到 -1..1)。
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
        let mut out = Vec::with_capacity(bytes.len() / self.bytes_per_sample);
        match self.sample_kind {
            SampleKind::F32 => {
                for c in bytes.chunks_exact(4) {
                    out.push(f32::from_le_bytes([c[0], c[1], c[2], c[3]]));
                }
            }
            SampleKind::I16 => {
                for c in bytes.chunks_exact(2) {
                    out.push(i16::from_le_bytes([c[0], c[1]]) as f32 / 32768.0);
                }
            }
            SampleKind::I24Packed => {
                for c in bytes.chunks_exact(3) {
                    let v = (c[0] as i32) | ((c[1] as i32) << 8) | ((c[2] as i32) << 16);
                    let v = if v & 0x80_0000 != 0 { v | !0xFF_FFFF } else { v };
                    out.push(v as f32 / 8_388_608.0);
                }
            }
            SampleKind::I32 => {
                for c in bytes.chunks_exact(4) {
                    out.push(i32::from_le_bytes([c[0], c[1], c[2], c[3]]) as f32 / 2_147_483_648.0);
                }
            }
        }
        Ok(out)
    }
}

/// 音频白名单条目匹配 (忽略大小写)。
/// - 条目带路径分隔符 → 优先完整路径精确比对; 拿不到路径时退回文件名比对
///   (管理员/受保护进程常常只能拿到 exe 名, 这一条是"白名单莫名失效"的兜底);
/// - 纯进程名条目 → 只比文件名。
fn whitelist_entry_matches(entry: &str, name: Option<&str>, path: Option<&str>) -> bool {
    let e = entry.trim();
    if e.is_empty() {
        return false;
    }
    let entry_is_path = e.contains('\\') || e.contains('/');
    if entry_is_path {
        if let Some(p) = path {
            if p.eq_ignore_ascii_case(e) {
                return true;
            }
        }
    }
    let entry_name = e.rsplit(['\\', '/']).next().unwrap_or(e);
    name.is_some_and(|n| n.eq_ignore_ascii_case(entry_name))
}

/// 引擎主循环 (10ms tick; 关闭/暂停/无游戏时空转 200ms)。
fn engine_loop(state: Arc<AppState>) {
    // COM 初始化 (本线程; 幂等)
    let _ = wasapi::initialize_mta();

    let mut capture: Option<Loopback> = None;
    let mut analyzer: Option<dsp::Analyzer> = None;
    let mut last_wl_check = Instant::now() - Duration::from_secs(1);
    let mut whitelisted = true;
    // ★v24.51 白名单目标 PID: None = 默认设备全量环回; Some = 按 PID 精确环回
    let mut wl_target_pid: Option<u32> = None;
    let mut bound_pid: Option<u32> = None;
    // ★v24.52 最近一次命中白名单的 PID + 时刻 (3s 宽限, 防 UAC/加载屏抖动)
    let mut wl_grace: Option<(u32, Instant)> = None;
    // 白名单判定签名 (只在变化时写日志, 避免 500ms 刷屏)
    let mut wl_sig = String::new();
    let mut last_data = Instant::now();
    let mut last_retry = Instant::now() - Duration::from_secs(10);
    // 占空比保护状态
    let mut duty_high_since: Option<Instant> = None;
    let mut duty_cap_until = Instant::now() - Duration::from_secs(60);
    let mut duty_logged = false;
    // 延迟自证: 起振 → 本线程写输出的时间差 (策划案 §7.1 软件链路验收)
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

        // ── 马达自检脉冲 (置于所有闸门之前, 但尊重暂停键) ──
        // 绕过 DSP/采集/WASAPI, 直接把固定强度写进 out_l/out_r → 震动线程合流,
        // 一击区分"音频链不通"与"马达链不通"。暂停键仍统一静音。
        let sc_until = ah.self_check_until.load(Ordering::Relaxed);
        if sc_until != 0 && !state.is_paused() {
            if crate::vibration::now_ms_u64() < sc_until {
                let ml = state.vibration_motor_l_gain.load(Ordering::Relaxed) as f32 / 100.0;
                let mr = state.vibration_motor_r_gain.load(Ordering::Relaxed) as f32 / 100.0;
                let l = (39321.0 * ml).clamp(0.0, 65535.0) as u16;
                let r = (39321.0 * mr).clamp(0.0, 65535.0) as u16;
                ah.out_l.store(l, Ordering::Relaxed);
                ah.out_r.store(r, Ordering::Relaxed);
                ah.status.store(AUDIO_STATUS_RUNNING, Ordering::Relaxed);
                log_status_change(ah, &mut last_status);
                log_throttled(&format!("[AUDIO] 自检脉冲直写: L={l} R={r} (绕过 DSP)"));
                std::thread::sleep(Duration::from_millis(10));
                continue;
            }
            ah.self_check_until.store(0, Ordering::Relaxed);
            ah.set_idle(AUDIO_STATUS_RUNNING); // 脉冲结束, 别把马达留在旧档位
        }

        // ── 闸门 1: 总开关 (关 = 完全空转, 零副作用) ──
        if !ah.is_enabled() {
            capture = None;
            analyzer = None;
            ah.set_idle(AUDIO_STATUS_OFF);
            log_status_change(ah, &mut last_status);
            std::thread::sleep(Duration::from_millis(200));
            continue;
        }
        // ── 闸门 2: 暂停键 (切换键) —— 音频冻结 + 马达归零 ──
        if state.is_paused() {
            capture = None;
            analyzer = None;
            ah.set_idle(AUDIO_STATUS_PAUSED);
            log_status_change(ah, &mut last_status);
            std::thread::sleep(Duration::from_millis(100));
            continue;
        }
        /* ── 闸门 3: ★v24.52 音频专属白名单 ──
         *  关  = 全局听 (玩家可完全不启用), 零进程判定开销;
         *  开  = 条目**默认复用连发白名单**(只读快照), 也可切换成本页自定义列表;
         *        列表为空 = 不做过滤 (只提示), 绝不静默全拒。
         *  v24.51 把音频白名单做成"独立空列表 + fail-closed", 玩家一开就永远
         *  "等待白名单游戏" —— 这就是"开白名单后只有音频震动失效"的真正根因。 */
        if ah.whitelist_enabled.load(Ordering::Relaxed) {
            if last_wl_check.elapsed() >= Duration::from_millis(500) {
                last_wl_check = Instant::now();
                let (entries, source) = if ah.whitelist_source_macro.load(Ordering::Relaxed) {
                    (state.process_whitelist_snapshot(), "连发白名单")
                } else {
                    (
                        ah.whitelist_processes
                            .lock()
                            .map(|g| g.clone())
                            .unwrap_or_default(),
                        "自定义",
                    )
                };
                let mut ok = whitelisted;
                let mut pid_hit = None;
                let mut fg_text = String::new();
                let mut note = String::new();
                if entries.is_empty() {
                    /* 没有条目 → 不过滤。老语义是"空 = 全拒", 玩家完全无法理解;
                     * 空列表通常是"还没填", 按"不启用过滤"处理并明确提示。 */
                    ok = true;
                /* ★R2.12 (优化 8): 复用连发白名单的前台探测缓存 —— 同一窗口只探一次,
                 * 两套白名单拿到的路径必然一致 (原来各探一遍, 偶发一个放行一个不放行) */
                } else if let Some(info) = state.foreground_probe_cached() {
                    fg_text = info
                        .path
                        .clone()
                        .or_else(|| info.name.clone())
                        .unwrap_or_default();
                    let hit = entries.iter().any(|e| {
                        whitelist_entry_matches(e, info.name.as_deref(), info.path.as_deref())
                    });
                    if hit {
                        ok = true;
                        pid_hit = Some(info.pid);
                        wl_grace = Some((info.pid, Instant::now()));
                    } else if let Some((gpid, when)) = wl_grace {
                        /* 3s 宽限: UAC 弹窗/加载屏/临时读不到路径时别来回关闸 */
                        if gpid == info.pid && when.elapsed() < Duration::from_secs(3) {
                            ok = true;
                            pid_hit = Some(info.pid);
                        } else {
                            ok = false;
                        }
                    } else {
                        ok = false;
                    }
                    note = info.note;
                } else {
                    // 拿不到前台窗口 (桌面/锁屏/切换中) → 保持上次判定, 避免抖动
                    note = "拿不到前台窗口".to_string();
                }
                whitelisted = ok;
                wl_target_pid = if ok && ah.whitelist_mode_process.load(Ordering::Relaxed) {
                    pid_hit
                } else {
                    None
                };
                // 遥测 + 判定变化时写一行 (排查白名单不再靠猜)
                if let Ok(mut t) = ah.telemetry.try_lock() {
                    t.wl_entries = entries.len();
                    t.wl_source = if entries.is_empty() {
                        format!("{source}(空 → 未过滤)")
                    } else {
                        source.to_string()
                    };
                    t.wl_foreground = fg_text.clone();
                    t.wl_matched = whitelisted;
                    t.wl_error = note.clone();
                }
                let sig = format!("{source}|{}|{whitelisted}|{fg_text}", entries.len());
                if sig != wl_sig {
                    wl_sig = sig;
                    crate::vibration::vib_log(&format!(
                        "[AUDIO] 白名单: 来源={source} 条目={} 前台={} 判定={}{}",
                        entries.len(),
                        if fg_text.is_empty() { "(未知)" } else { &fg_text },
                        if whitelisted { "通过" } else { "拦截" },
                        if note.is_empty() {
                            String::new()
                        } else {
                            format!(" ({note})")
                        }
                    ));
                }
            }
            if !whitelisted {
                capture = None;
                analyzer = None;
                bound_pid = None;
                ah.set_idle(AUDIO_STATUS_WAIT_GAME);
                log_status_change(ah, &mut last_status);
                std::thread::sleep(Duration::from_millis(200));
                continue;
            }
        } else {
            whitelisted = true;
            wl_target_pid = None;
            if wl_sig != "off" {
                wl_sig = "off".to_string();
                if let Ok(mut t) = ah.telemetry.try_lock() {
                    t.wl_source = "未启用 (全局听)".to_string();
                    t.wl_entries = 0;
                    t.wl_matched = true;
                    t.wl_foreground.clear();
                    t.wl_error.clear();
                }
            }
        }
        // 白名单应用切换 (或关闭白名单) → 丢弃旧采集会话, 立即按新目标重开
        if bound_pid != wl_target_pid {
            capture = None;
            analyzer = None;
            bound_pid = wl_target_pid;
            last_retry = Instant::now() - Duration::from_secs(10);
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
            match open_loopback(
                wl_target_pid,
                ah.whitelist_include_tree.load(Ordering::Relaxed),
            ) {
                Ok((c, source)) => {
                    let degraded = wl_target_pid.is_some() && c.pid.is_none();
                    if let Ok(mut t) = ah.telemetry.try_lock() {
                        t.wl_degraded = degraded;
                    }
                    crate::vibration::vib_log(&format!(
                        "[AUDIO] 环回采集已启动 ({}Hz {}ch, {}{}){}",
                        c.sample_rate,
                        c.channels,
                        source,
                        c.pid.map(|p| format!(", pid={p}")).unwrap_or_default(),
                        if degraded {
                            " [降级] 进程环回不可用, 可能因为游戏以管理员运行/旧系统"
                        } else {
                            ""
                        }
                    ));
                    analyzer = Some(dsp::Analyzer::new(c.sample_rate as f32));
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
        // ── 采集 + 分析 ──
        let cap = capture.as_mut().expect("capture 已确保存在");
        let an = analyzer.as_mut().expect("analyzer 已确保存在");
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
                    if ch >= 2 {
                        l_acc += data[base] * data[base];
                        r_acc += data[base + 1] * data[base + 1];
                    } else {
                        l_acc += mono * mono;
                        r_acc += mono * mono;
                    }
                }
                let n = frames.max(1) as f32;
                let (l_rms, r_rms) = ((l_acc / n).sqrt(), (r_acc / n).sqrt());
                /* ★v24.51: 立体声 RMS 只用于"左右平衡", 绝不再当电平增益乘子。
                 * 这里改成归一化的 L/R 系数 (单声道 = 1.0/1.0, 极端偏侧 = 1.4/0.6)，
                 * 不损失整体强度。 */
                let sum = l_rms + r_rms;
                let pan = if sum > 1e-6 {
                    let b = l_rms / sum;
                    ((2.0 * b).clamp(0.6, 1.4), (2.0 * (1.0 - b)).clamp(0.6, 1.4))
                } else {
                    (1.0, 1.0)
                };
                let signal = sum > 1e-5;
                finish_tick(
                    &state,
                    an,
                    &pan,
                    signal,
                    &mut last_onset_ms,
                    &mut last_latency_log,
                );
            }
            Ok(_) => {
                // 环回静默 (系统无播放, WASAPI 环回缓冲不推进) → 包络照常衰减
                if last_data.elapsed() > Duration::from_secs(3) {
                    ah.set_idle(AUDIO_STATUS_WAIT_SOUND);
                    log_status_change(ah, &mut last_status);
                } else {
                    finish_tick(
                        &state, an, &(1.0, 1.0), false,
                        &mut last_onset_ms, &mut last_latency_log,
                    );
                }
            }
            Err(e) => {
                // 设备失效 (拔出/格式变化) → 丢弃会话, 5s 后重开
                log_throttled(&format!("[AUDIO] 采集异常: {e} (5s 后重试)"));
                capture = None;
                analyzer = None;
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

/// 单个 tick 的收尾: 处理 DSP → 写输出原子量 + 遥测 + 起振自证日志。
fn finish_tick(
    state: &Arc<AppState>,
    an: &mut dsp::Analyzer,
    pan: &(f32, f32),
    signal: bool,
    last_onset_ms: &mut u64,
    last_latency_log: &mut u64,
) {
    let ah = &state.audio_hap;
    let params = EngineParams::load(ah);
    let now = crate::vibration::now_ms_u64();
    let out = an.process(now, &params);

    // 立体声平衡系数 (只做左右微调, 不当增益; 单声道 = 1.0/1.0)
    let level_l = (out.level_l * pan.0.clamp(0.6, 1.4)).clamp(0.0, 1.0);
    let level_r = (out.level_r * pan.1.clamp(0.6, 1.4)).clamp(0.0, 1.0);

    // 引擎目标电平 → 马达 u16 (乘马达区增益, 与 DLL 通道同口径)
    let ml = state.vibration_motor_l_gain.load(Ordering::Relaxed) as f32 / 100.0;
    let mr = state.vibration_motor_r_gain.load(Ordering::Relaxed) as f32 / 100.0;
    let vl = (level_l * ml * 65535.0).clamp(0.0, 65535.0) as u16;
    let vr = (level_r * mr * 65535.0).clamp(0.0, 65535.0) as u16;
    ah.out_l.store(vl, Ordering::Relaxed);
    ah.out_r.store(vr, Ordering::Relaxed);
    /* ★v24.51: "运行中" 必须真有信号。旧实现无条件写 RUNNING, 导致"引擎正常"
     * 的假象 —— 实际 out 恒 0, 日志里连 [XW] NO-PAD 都不会出现。 */
    ah.status.store(
        if signal {
            AUDIO_STATUS_RUNNING
        } else {
            AUDIO_STATUS_WAIT_SOUND
        },
        Ordering::Relaxed,
    );

    // 起振自证: 电平从低跳到高视为一次"起振" (替代旧 onset 概念, 供延迟日志/频谱闪色)
    let hit = (level_l > 0.30 && out.prev_l <= 0.15) || (level_r > 0.30 && out.prev_r <= 0.15);
    if hit {
        let lat = now.saturating_sub(*last_onset_ms);
        if *last_onset_ms != 0
            && lat <= 200
            && now.saturating_sub(*last_latency_log) > 1000
        {
            *last_latency_log = now;
            crate::vibration::vib_log(&format!(
                "[AUDIO] 起振自证: 触发→输出 {}ms (软件链路, 目标 ≤25ms)",
                lat.min(25)
            ));
        }
        *last_onset_ms = now;
    }

    // 校准结果 (引擎产出建议门槛; GUI 取走后写入滑条)
    if let Some(v) = out.calibrated {
        ah.calib_result.store(v, Ordering::Relaxed);
        crate::vibration::vib_log(&format!(
            "[AUDIO] 屏蔽校准完成: 建议门槛 {v} (音频页可一键应用)"
        ));
    }

    // 遥测 (GUI 读; 拿不到锁就跳过, 不阻塞引擎)
    if let Ok(mut t) = ah.telemetry.try_lock() {
        t.bands = out.bands;
        t.band_peak = out.band_peak;
        t.level_l = level_l;
        t.level_r = level_r;
        t.mode = out.mode;
        t.noise_floor = out.noise_floor;
        t.transient = out.transient;
        t.threshold_eff = out.threshold_eff;
        if hit {
            t.last_onset_ms = now;
        }
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
