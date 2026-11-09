//! ★v24.60 音频震动模式 —— 运行态容器 (原子量 + 遥测)。
//!
//! 设计 (与宿主线程模型一致, 勿引入新 channel 语义):
//! - 音频引擎线程**写**, 震动线程 (15ms 合流) 与 GUI (每帧) **读**;
//! - 默认全零 = 功能关闭 → 震动管线在关闭态下行为与旧版逐字节一致
//!   (audio_out 恒 0, max 合并后不改变任何 DLL 事件输出);
//! - 参数镜像进原子量, reload_config 逐项 store, 引擎每 tick 读取 (无锁热路径)。
//!
//! ★v24.60: 删除 YAMNet (sem) / MRX (sep) AI 层的全部字段; 参数集改为融合引擎
//! (xbox6 骨架 + AudioVibration 映射) 的直译, 默认值取内置预设第一档「游戏强劲」。

use std::sync::atomic::{AtomicBool, AtomicU16, AtomicU32, AtomicU64, AtomicU8, Ordering};
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

/// GUI 遥测 (引擎 try_lock 写, GUI try_lock 读; 拿不到锁跳过本帧, 永不阻塞)。
#[derive(Debug, Clone, Default)]
pub struct AudioTelemetry {
    /// 5 显示频带平滑 RMS (旧归一化口径 0..~1): [超低 20-80, 低 80-250, 中 250-2k, 高 2k-8k, 超高 8k+] Hz
    pub bands: [f32; 5],
    /// 频带峰值保持 (可视化慢落)
    pub band_peak: [f32; 5],
    /// 引擎目标输出 (0..1, 未乘马达增益; GUI 电平条)
    pub level_l: f32,
    pub level_r: f32,
    /// 最近一次输出起振时刻 (now_ms_u64; 0 = 从未; 频谱闪色用)
    pub last_onset_ms: u64,
    /// ★v24.60 内容判定 (mapping::MODE_*): 0=游戏 1=BGM 2=人声/细节 3=安静
    pub mode: u8,
    /// 噪声底 (int16 尺度; 自适应门槛的基准)
    pub noise_floor: f32,
    /// 瞬态均线 0..1 (爆发度; BGM 段鼓点脉冲的驱动源)
    pub transient: f32,
    /// 本 tick 生效门槛 (int16 尺度; GUI 显示"为什么不震")
    pub threshold_eff: f32,
    // ── 白名单可观测性 (排查"开了白名单就没震动"不再靠猜) ──
    /// 当前生效的白名单条目数 (0 = 未过滤)
    pub wl_entries: usize,
    /// 条目来源: "连发白名单" / "自定义" / "空(未过滤)"
    pub wl_source: String,
    /// 当前前台进程 (完整路径优先, 拿不到路径时只有进程名)
    pub wl_foreground: String,
    /// 前台进程是否命中白名单
    pub wl_matched: bool,
    /// 进程环回是否降级成了默认设备环回 (管理员权限/旧系统常见)
    pub wl_degraded: bool,
    /// 最近一次进程信息获取失败原因 (空 = 正常)
    pub wl_error: String,
}

/// 音频震动运行态 (AppState 持有)。
pub struct AudioHapState {
    /// 总开关 (配置镜像; 关 = 引擎闲置 + 输出归零, 主模式/DLL 模式零影响)
    pub enabled: AtomicBool,
    // ── 融合引擎参数 (reload_config 镜像; 引擎每 tick 读; 默认值 = 预设「游戏强劲」) ──
    pub master_gain: AtomicU32, // % 0..=150 全局总强度
    pub low_gain: AtomicU32,    // % 低频量感 (左马达)
    pub high_gain: AtomicU32,   // % 高频量感 (右马达)
    pub punch_pct: AtomicU32,   // 0..=200 打击对比度 (幂律 gamma)
    pub mask_threshold: AtomicU32, // 屏蔽门槛 (int16 尺度 0..=1200)
    pub adaptive_threshold: AtomicBool, // 自适应底噪
    pub auto_profile: AtomicBool,       // 智能识别 (音乐/人声/安静)
    pub music_mask_pct: AtomicU32,      // 0..=100 BGM 段压制
    pub quiet_mask: AtomicBool,         // 安静段静音
    pub rhythm_pct: AtomicU32,          // 0..=100 节奏力度
    pub texture_pct: AtomicU32,         // 0..=100 细腻麻感
    pub attack_pct: AtomicU32,          // 5..=90 响应速度
    pub release_pct: AtomicU32,         // 2..=60 余韵
    pub dead_zone_pct: AtomicU32,       // 0..=20 死区
    pub map_max: AtomicU32,             // 满量程 (int16 尺度 500..=12000)
    pub dual_motor: AtomicBool,         // 双马达分频
    pub vocal_only: AtomicBool,         // 只跟人声 (双马达关闭时)
    pub bass_high_hz: AtomicU32,        // 低频带上限 Hz 200..=1000
    pub vocal_low_hz: AtomicU32,        // 人声带下限 Hz 300..=1500
    pub vocal_high_hz: AtomicU32,       // 人声带上限 Hz 1500..=8000
    /// 自动屏蔽校准截止时刻 (now_ms 口径; 0 = 不在校准中)
    pub calibrate_until: AtomicU64,
    /// 校准结果 (引擎写建议门槛; 0 = 无待取结果; GUI 取走后清零)
    pub calib_result: AtomicU32,
    /// ★v24.50.1 遗留别名 (默认 false; 与 whitelist_enabled 取或)
    pub whitelist_filter: AtomicBool,
    // ── ★v24.51 音频专属白名单 (与连发 process_whitelist 完全独立) ──
    /// 白名单总开关: 关 = 全局听 (玩家可不启用该机制)
    pub whitelist_enabled: AtomicBool,
    /// 白名单条目 (配置镜像, 500ms 刷新一次; 空 = 不过滤)
    pub whitelist_processes: Mutex<Vec<String>>,
    /// true = 按 PID 精确环回; false = 前台门控 + 默认设备全量环回
    pub whitelist_mode_process: AtomicBool,
    /// ★v24.52 条目来源: true = 复用连发白名单 (默认), false = 本页自定义列表
    pub whitelist_source_macro: AtomicBool,
    /// 进程环回是否含子进程
    pub whitelist_include_tree: AtomicBool,
    /// 马达自检脉冲截止时刻 (now_ms_u64 口径, 0 = 无)
    pub self_check_until: AtomicU64,
    // ── 输出 (引擎写, 震动线程合流读; 关闭/暂停/静默时恒 0) ──
    pub out_l: AtomicU16,
    pub out_r: AtomicU16,
    pub status: AtomicU8,
    /// 遥测 (Mutex 因含 String/Vec; 引擎与 GUI 都 try_lock)
    pub telemetry: Mutex<AudioTelemetry>,
}

impl Default for AudioHapState {
    fn default() -> Self {
        // 默认值 = 内置预设第一档「游戏强劲」(用户实机调校 + 屏蔽层全开)。
        let g = &crate::audio_haptics::mapping::PRESETS[0];
        Self {
            enabled: AtomicBool::new(false),
            master_gain: AtomicU32::new(g.master_gain_pct),
            low_gain: AtomicU32::new(g.low_gain_pct),
            high_gain: AtomicU32::new(g.high_gain_pct),
            punch_pct: AtomicU32::new(g.punch_pct),
            mask_threshold: AtomicU32::new(g.mask_threshold),
            adaptive_threshold: AtomicBool::new(g.adaptive_threshold),
            auto_profile: AtomicBool::new(g.auto_profile),
            music_mask_pct: AtomicU32::new(g.music_mask_pct),
            quiet_mask: AtomicBool::new(g.quiet_mask),
            rhythm_pct: AtomicU32::new(g.rhythm_pct),
            texture_pct: AtomicU32::new(g.texture_pct),
            attack_pct: AtomicU32::new(g.attack_pct),
            release_pct: AtomicU32::new(g.release_pct),
            dead_zone_pct: AtomicU32::new(g.dead_zone_pct),
            map_max: AtomicU32::new(g.map_max),
            dual_motor: AtomicBool::new(g.dual_motor),
            vocal_only: AtomicBool::new(g.vocal_only),
            bass_high_hz: AtomicU32::new(g.bass_high_hz),
            vocal_low_hz: AtomicU32::new(g.vocal_low_hz),
            vocal_high_hz: AtomicU32::new(g.vocal_high_hz),
            calibrate_until: AtomicU64::new(0),
            calib_result: AtomicU32::new(0),
            whitelist_filter: AtomicBool::new(false),
            whitelist_enabled: AtomicBool::new(false),
            whitelist_processes: Mutex::new(Vec::new()),
            whitelist_mode_process: AtomicBool::new(true),
            whitelist_source_macro: AtomicBool::new(true),
            whitelist_include_tree: AtomicBool::new(true),
            self_check_until: AtomicU64::new(0),
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
        let rel = Ordering::Relaxed;
        self.enabled.store(c.enabled, rel);
        self.master_gain.store(c.master_gain_pct.clamp(0, 150), rel);
        self.low_gain.store(c.low_gain_pct.clamp(0, 200), rel);
        self.high_gain.store(c.high_gain_pct.clamp(0, 200), rel);
        self.punch_pct.store(c.punch_pct.clamp(0, 200), rel);
        self.mask_threshold.store(c.mask_threshold.clamp(0, 1200), rel);
        self.adaptive_threshold.store(c.adaptive_threshold, rel);
        self.auto_profile.store(c.auto_profile, rel);
        self.music_mask_pct.store(c.music_mask_pct.clamp(0, 100), rel);
        self.quiet_mask.store(c.quiet_mask, rel);
        self.rhythm_pct.store(c.rhythm_pct.clamp(0, 100), rel);
        self.texture_pct.store(c.texture_pct.clamp(0, 100), rel);
        self.attack_pct.store(c.attack_pct.clamp(5, 90), rel);
        self.release_pct.store(c.release_pct.clamp(2, 60), rel);
        self.dead_zone_pct.store(c.dead_zone_pct.clamp(0, 20), rel);
        self.map_max.store(c.map_max.clamp(500, 12000), rel);
        self.dual_motor.store(c.dual_motor, rel);
        self.vocal_only.store(c.vocal_only, rel);
        self.bass_high_hz.store(c.bass_high_hz.clamp(200, 1000), rel);
        self.vocal_low_hz.store(c.vocal_low_hz.clamp(300, 1500), rel);
        self.vocal_high_hz.store(c.vocal_high_hz.clamp(1500, 8000), rel);
        self.whitelist_filter.store(c.whitelist_filter, rel);
        /* ★v24.51: 遗留别名与新开关取或 —— 老 Config.toml 的 whitelist_filter=true
         * 仍能生效; 新配置一律用 whitelist_enabled。 */
        self.whitelist_enabled
            .store(c.whitelist_enabled || c.whitelist_filter, rel);
        if let Ok(mut v) = self.whitelist_processes.lock() {
            *v = c.whitelist_processes.clone();
        }
        self.whitelist_mode_process
            .store(c.whitelist_mode.eq_ignore_ascii_case("process"), rel);
        self.whitelist_source_macro
            .store(!c.whitelist_source.eq_ignore_ascii_case("custom"), rel);
        self.whitelist_include_tree
            .store(c.whitelist_include_tree, rel);
        if !c.enabled {
            self.set_idle(AUDIO_STATUS_OFF);
        }
    }

    /// ★v24.51 马达自检: 让引擎绕过 DSP 直接写 500ms 固定满幅脉冲,
    /// 一击区分"音频→输出链通不通"。仅产品内自检控件使用, 不参与任何算法。
    pub fn request_self_check(&self) {
        let now = crate::vibration::now_ms_u64();
        self.self_check_until
            .store(now.saturating_add(500), Ordering::Relaxed);
    }

    /// 自检脉冲是否仍在有效期 (震动线程用它临时放行音频输出通道)。
    #[inline]
    pub fn self_check_active(&self) -> bool {
        self.self_check_until.load(Ordering::Relaxed) > crate::vibration::now_ms_u64()
    }

    /// ★v24.60 自动屏蔽校准: 引擎在 3 秒内只记录背景噪声峰值,
    /// 结束后给出建议门槛 (写入 calib_result, 由 GUI 取走应用)。
    pub fn request_calibration(&self) {
        let now = crate::vibration::now_ms_u64();
        self.calib_result.store(0, Ordering::Relaxed);
        self.calibrate_until
            .store(now.saturating_add(3000), Ordering::Relaxed);
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
        // 引擎默认参数 = 内置预设第一档 (★v24.61 起为「人声清晰」; 断言跟随排序不写死)
        let g = &crate::audio_haptics::mapping::PRESETS[0];
        assert_eq!(s.mask_threshold.load(Ordering::Relaxed), g.mask_threshold);
        assert_eq!(
            s.adaptive_threshold.load(Ordering::Relaxed),
            g.adaptive_threshold
        );
        assert_eq!(s.auto_profile.load(Ordering::Relaxed), g.auto_profile);
        assert_eq!(s.master_gain.load(Ordering::Relaxed), g.master_gain_pct);
    }

    #[test]
    fn audio_state_apply_config_mirrors_and_clamps() {
        let s = AudioHapState::default();
        let mut c = AudioHapticsConfig::default();
        c.enabled = true;
        c.master_gain_pct = 300; // 超界 → 收敛到 150
        c.mask_threshold = 99999; // 超界 → 收敛到 1200
        c.whitelist_filter = true;
        s.apply_config(&c);
        assert!(s.is_enabled());
        assert_eq!(s.master_gain.load(Ordering::Relaxed), 150);
        assert_eq!(s.mask_threshold.load(Ordering::Relaxed), 1200);
        assert!(s.whitelist_filter.load(Ordering::Relaxed));
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
    fn calibration_request_sets_window() {
        let s = AudioHapState::default();
        assert_eq!(s.calibrate_until.load(Ordering::Relaxed), 0);
        s.request_calibration();
        let until = s.calibrate_until.load(Ordering::Relaxed);
        let now = crate::vibration::now_ms_u64();
        assert!(
            until > now && until - now <= 3100,
            "校准窗口应为 3 秒: until={until} now={now}"
        );
    }

    /// ★v24.60 一致性兜底: config.rs 是叶子模块, 默认值硬编码 (不许引用
    /// audio_haptics)。这里保证 config 默认值与内置预设「游戏强劲」逐字段一致,
    /// 改预设忘了同步 config 时在这里爆。
    #[test]
    fn config_defaults_match_preset() {
        let g = &crate::audio_haptics::mapping::PRESETS[0];
        let c = AudioHapticsConfig::default();
        assert_eq!(c.preset, g.name);
        assert_eq!(c.master_gain_pct, g.master_gain_pct);
        assert_eq!(c.low_gain_pct, g.low_gain_pct);
        assert_eq!(c.high_gain_pct, g.high_gain_pct);
        assert_eq!(c.punch_pct, g.punch_pct);
        assert_eq!(c.mask_threshold, g.mask_threshold);
        assert_eq!(c.adaptive_threshold, g.adaptive_threshold);
        assert_eq!(c.auto_profile, g.auto_profile);
        assert_eq!(c.music_mask_pct, g.music_mask_pct);
        assert_eq!(c.quiet_mask, g.quiet_mask);
        assert_eq!(c.rhythm_pct, g.rhythm_pct);
        assert_eq!(c.texture_pct, g.texture_pct);
        assert_eq!(c.attack_pct, g.attack_pct);
        assert_eq!(c.release_pct, g.release_pct);
        assert_eq!(c.dead_zone_pct, g.dead_zone_pct);
        assert_eq!(c.map_max, g.map_max);
        assert_eq!(c.dual_motor, g.dual_motor);
        assert_eq!(c.vocal_only, g.vocal_only);
        assert_eq!(c.bass_high_hz, g.bass_high_hz);
        assert_eq!(c.vocal_low_hz, g.vocal_low_hz);
        assert_eq!(c.vocal_high_hz, g.vocal_high_hz);
    }
}
