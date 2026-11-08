//! ★v24.50 音频震动模式 —— 运行态容器 (原子量 + 遥测)。
//!
//! 设计 (与宿主线程模型一致, 勿引入新 channel 语义):
//! - 音频引擎线程**写**, 震动线程 (15ms 合流) 与 GUI (每帧) **读**;
//! - 默认全零 = 功能关闭 → 震动管线在关闭态下行为与 v24.49 逐字节一致
//!   (audio_out 恒 0, max 合并后不改变任何 DLL 事件输出);
//! - 参数镜像进原子量, reload_config 逐项 store, 引擎每 tick 读取 (无锁热路径)。

use std::sync::atomic::{AtomicBool, AtomicU16, AtomicU32, AtomicU8, Ordering};
use std::sync::Mutex;

/// 引擎状态机 (`status` 原子量的取值, GUI 显示用)。
pub const AUDIO_STATUS_OFF: u8 = 0; // 总开关关
pub const AUDIO_STATUS_WAIT_GAME: u8 = 1; // 白名单开启但前台不在白名单
pub const AUDIO_STATUS_NO_DEVICE: u8 = 2; // 无音频设备/环回初始化失败 (5s 重试)
pub const AUDIO_STATUS_WAIT_SOUND: u8 = 3; // 环回静默 (系统无播放, WASAPI 环回不推进)
pub const AUDIO_STATUS_RUNNING: u8 = 4; // 运行中
pub const AUDIO_STATUS_PAUSED: u8 = 5; // 暂停 (切换键; 马达归零)

/// 状态码 → 中文文案 (纯函数, GUI 与日志共用)。
pub fn audio_status_name(status: u8) -> &'static str {
    match status {
        AUDIO_STATUS_OFF => "已关闭",
        AUDIO_STATUS_WAIT_GAME => "等待白名单游戏",
        AUDIO_STATUS_NO_DEVICE => "无音频设备",
        AUDIO_STATUS_WAIT_SOUND => "等待声音",
        AUDIO_STATUS_RUNNING => "运行中",
        AUDIO_STATUS_PAUSED => "已暂停",
        _ => "未知",
    }
}

/// YAMNet 语义快照 (sem 线程写, GUI/引擎读)。无模型或未启用时字段全零。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SemSnapshot {
    /// top3 类名 + 置信度 (0..1)
    pub top3: Vec<(String, f32)>,
    /// Music 族最高置信度 (门控信号)
    pub music: f32,
    /// Speech 族最高置信度 (门控信号)
    pub speech: f32,
    /// Gunshot/Gunfire/Machine gun 最高置信度
    pub gun: f32,
    /// Explosion/Fireworks 最高置信度
    pub explosion: f32,
    /// Engine/Vehicle 族最高置信度 (持续低鸣跟随)
    pub engine: f32,
    /// 快照生成时刻 (now_ms_u64)
    pub updated_ms: u64,
}

impl SemSnapshot {
    /// 快照是否足够新鲜 (默认 1s 内)。
    pub fn fresh(&self, now_ms: u64, max_age_ms: u64) -> bool {
        self.updated_ms != 0 && now_ms.saturating_sub(self.updated_ms) <= max_age_ms
    }
}

/// GUI 遥测 (引擎 try_lock 写, GUI try_lock 读; 拿不到锁跳过本帧, 永不阻塞)。
#[derive(Debug, Clone, Default)]
pub struct AudioTelemetry {
    /// 5 频带 RMS 归一 (0..1): [超低 20-80, 低 80-250, 中 250-2k, 高 2k-8k, 超高 8k+] Hz
    pub bands: [f32; 5],
    /// 频带峰值保持 (可视化慢落)
    pub band_peak: [f32; 5],
    /// 引擎目标输出 (0..1, 未乘马达增益; GUI 电平条)
    pub level_l: f32,
    pub level_r: f32,
    /// 最近一次 onset 时刻 (now_ms_u64; 0 = 从未)
    pub last_onset_ms: u64,
    /// 当前秒已触发 onset 数 (节流可视化)
    pub onsets_this_second: u32,
    /// 语义层最新快照
    pub sem: SemSnapshot,
    /// 语义层是否在工作 (模型加载成功且未禁用)
    pub sem_active: bool,
    /// 本帧纯净反馈门控系数 (1=无门控; <1 = Music/Speech 命中被压制)
    pub gate_factor: f32,
}

/// 音频震动运行态 (AppState 持有)。
pub struct AudioHapState {
    /// 总开关 (配置镜像; 关 = 引擎闲置 + 输出归零, 主模式/DLL 模式零影响)
    pub enabled: AtomicBool,
    // ── 参数 (reload_config 镜像; 引擎每 tick 读) ──
    pub master_gain: AtomicU32, // % 0..=200
    pub low_gain: AtomicU32,    // % 低频 (爆炸/重砸的沉闷感)
    pub high_gain: AtomicU32,   // % 高频 (枪声/打击的脆响)
    pub onset_sensitivity: AtomicU32, // 1..=8 (谱通量自适应阈值倍数 k×10 存 10..80)
    pub onset_release_ms: AtomicU32,  // 50..=500 冲击衰减
    pub sustain_gain: AtomicU32,      // % 持续音 (引擎/环境低鸣) 跟震
    pub silence_threshold: AtomicU32, // 0..=100 静音阈值
    pub max_onsets_per_sec: AtomicU32, // 4..=30 每秒触发上限 (DNF 连放保护)
    pub sem_enabled: AtomicBool,      // 语义层开关 (模型缺失时自动失效)
    pub gate_music: AtomicU32,        // % Music 命中时快通道保留比 (0=完全过滤)
    pub gate_speech: AtomicU32,       // % Speech 命中时快通道保留比
    // ── 输出 (引擎写, 震动线程合流读; 关闭/暂停/静默时恒 0) ──
    pub out_l: AtomicU16,
    pub out_r: AtomicU16,
    pub status: AtomicU8,
    /// 遥测 (Mutex 因含 String/Vec; 引擎与 GUI 都 try_lock)
    pub telemetry: Mutex<AudioTelemetry>,
}

impl Default for AudioHapState {
    fn default() -> Self {
        Self {
            enabled: AtomicBool::new(false),
            master_gain: AtomicU32::new(100),
            low_gain: AtomicU32::new(100),
            high_gain: AtomicU32::new(80),
            onset_sensitivity: AtomicU32::new(30), // k=3.0 (×10 存储)
            onset_release_ms: AtomicU32::new(120),
            sustain_gain: AtomicU32::new(40),
            silence_threshold: AtomicU32::new(20),
            max_onsets_per_sec: AtomicU32::new(12),
            sem_enabled: AtomicBool::new(true),
            gate_music: AtomicU32::new(20),
            gate_speech: AtomicU32::new(30),
            out_l: AtomicU16::new(0),
            out_r: AtomicU16::new(0),
            status: AtomicU8::new(AUDIO_STATUS_OFF),
            telemetry: Mutex::new(AudioTelemetry::default()),
        }
    }
}

impl AudioHapState {
    /// 总开关 (廉价读)。
    #[inline]
    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    /// 当前马达输出电平 (震动线程合流用)。关闭语义由引擎保证 (恒 0)。
    #[inline]
    pub fn output(&self) -> (u16, u16) {
        (
            self.out_l.load(Ordering::Relaxed),
            self.out_r.load(Ordering::Relaxed),
        )
    }

    /// 归零输出 + 状态 (关闭/暂停/等待时调用)。
    pub fn set_idle(&self, status: u8) {
        self.out_l.store(0, Ordering::Relaxed);
        self.out_r.store(0, Ordering::Relaxed);
        self.status.store(status, Ordering::Relaxed);
        if let Ok(mut t) = self.telemetry.try_lock() {
            t.level_l = 0.0;
            t.level_r = 0.0;
        }
    }

    /// 配置 → 原子量镜像 (AppState::new 与 reload_config 共用)。
    pub fn apply_config(&self, c: &crate::config::AudioHapticsConfig) {
        self.enabled.store(c.enabled, Ordering::Relaxed);
        self.master_gain.store(c.master_gain_pct.clamp(0, 200), Ordering::Relaxed);
        self.low_gain.store(c.low_gain_pct.clamp(0, 200), Ordering::Relaxed);
        self.high_gain.store(c.high_gain_pct.clamp(0, 200), Ordering::Relaxed);
        self.onset_sensitivity
            .store(c.onset_sensitivity.clamp(1, 8) * 10, Ordering::Relaxed);
        self.onset_release_ms
            .store(c.onset_release_ms.clamp(50, 500), Ordering::Relaxed);
        self.sustain_gain.store(c.sustain_gain_pct.clamp(0, 100), Ordering::Relaxed);
        self.silence_threshold
            .store(c.silence_threshold.clamp(0, 100), Ordering::Relaxed);
        self.max_onsets_per_sec
            .store(c.max_onsets_per_sec.clamp(4, 30), Ordering::Relaxed);
        self.sem_enabled.store(c.sem_enabled, Ordering::Relaxed);
        self.gate_music.store(c.gate_music_pct.clamp(0, 100), Ordering::Relaxed);
        self.gate_speech.store(c.gate_speech_pct.clamp(0, 100), Ordering::Relaxed);
        if !c.enabled {
            self.set_idle(AUDIO_STATUS_OFF);
        }
    }

    /// 语义快照读取 (克隆; GUI 每帧一次, 无热路径顾虑)。
    pub fn sem_snapshot(&self) -> (SemSnapshot, bool) {
        match self.telemetry.try_lock() {
            Ok(t) => (t.sem.clone(), t.sem_active),
            Err(_) => (SemSnapshot::default(), false),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AudioHapticsConfig;

    #[test]
    fn audio_state_defaults_are_zero_and_off() {
        let s = AudioHapState::default();
        assert!(!s.is_enabled());
        assert_eq!(s.output(), (0, 0));
        assert_eq!(s.status.load(Ordering::Relaxed), AUDIO_STATUS_OFF);
    }

    #[test]
    fn audio_state_apply_config_mirrors_and_clamps() {
        let s = AudioHapState::default();
        let mut c = AudioHapticsConfig::default();
        c.enabled = true;
        c.master_gain_pct = 300; // 超界 → 收敛到 200
        c.onset_sensitivity = 99; // 超界 → 收敛到 8
        s.apply_config(&c);
        assert!(s.is_enabled());
        assert_eq!(s.master_gain.load(Ordering::Relaxed), 200);
        assert_eq!(s.onset_sensitivity.load(Ordering::Relaxed), 80);
        // 关闭 → 输出/状态归零
        c.enabled = false;
        s.out_l.store(1234, Ordering::Relaxed);
        s.apply_config(&c);
        assert_eq!(s.output(), (0, 0));
        assert_eq!(s.status.load(Ordering::Relaxed), AUDIO_STATUS_OFF);
    }

    #[test]
    fn audio_state_set_idle_zeroes_output() {
        let s = AudioHapState::default();
        s.out_l.store(999, Ordering::Relaxed);
        s.out_r.store(888, Ordering::Relaxed);
        s.set_idle(AUDIO_STATUS_PAUSED);
        assert_eq!(s.output(), (0, 0));
        assert_eq!(s.status.load(Ordering::Relaxed), AUDIO_STATUS_PAUSED);
    }

    #[test]
    fn audio_status_names_are_distinct() {
        let names: Vec<_> = (0..=5).map(audio_status_name).collect();
        for i in 0..names.len() {
            for j in (i + 1)..names.len() {
                assert_ne!(names[i], names[j], "状态 {} 与 {} 文案重复", i, j);
            }
        }
    }

    #[test]
    fn sem_snapshot_freshness() {
        let mut s = SemSnapshot::default();
        assert!(!s.fresh(1000, 500), "updated_ms=0 (从未更新) 永远不新鲜");
        s.updated_ms = 800;
        assert!(s.fresh(1000, 500));
        assert!(!s.fresh(1301, 500));
    }
}
