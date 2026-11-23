//! ★R2.14 音频震动领域 (BackendService impl) + AudioHapState 只读遥测访问器。
//!
//! 遥测访问器放这里而不是 `state/audio_state.rs`：避免碰既有后端文件，
//! 且这些访问器只服务于快照/UI 读取 (Mutex<AudioTelemetry> 的容错读)。

use std::sync::atomic::Ordering::Relaxed;

use super::BackendService;
use crate::state::audio_state::{
    AudioHapState, AudioTelemetry, AUDIO_ALGO_A, AUDIO_ALGO_B, AUDIO_ALGO_C,
};
use crate::ui_api::Commit;

// ═══════════════════════════════════════════════════════════════════════
// AudioHapState 只读遥测访问器 (锁中毒容错；绝不在热路径使用 —— 仅 UI/快照)
// ═══════════════════════════════════════════════════════════════════════

impl AudioHapState {
    fn telemetry_snapshot<R>(&self, f: impl FnOnce(&AudioTelemetry) -> R) -> R {
        let t = self
            .telemetry
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        f(&t)
    }

    pub fn telemetry_bands(&self) -> [f32; 5] {
        self.telemetry_snapshot(|t| t.bands)
    }
    pub fn telemetry_band_peak(&self) -> [f32; 5] {
        self.telemetry_snapshot(|t| t.band_peak)
    }
    pub fn telemetry_level(&self) -> (f32, f32) {
        self.telemetry_snapshot(|t| (t.level_l, t.level_r))
    }
    pub fn telemetry_mode(&self) -> u8 {
        self.telemetry_snapshot(|t| t.mode)
    }
    pub fn telemetry_noise_floor(&self) -> f32 {
        self.telemetry_snapshot(|t| t.noise_floor)
    }
    pub fn telemetry_transient(&self) -> f32 {
        self.telemetry_snapshot(|t| t.transient)
    }
    pub fn telemetry_threshold_eff(&self) -> f32 {
        self.telemetry_snapshot(|t| t.threshold_eff)
    }
    pub fn telemetry_wl_entries(&self) -> usize {
        self.telemetry_snapshot(|t| t.wl_entries)
    }
    pub fn telemetry_wl_source(&self) -> String {
        self.telemetry_snapshot(|t| t.wl_source.clone())
    }
    pub fn telemetry_wl_foreground(&self) -> String {
        self.telemetry_snapshot(|t| t.wl_foreground.clone())
    }
    pub fn telemetry_wl_matched(&self) -> bool {
        self.telemetry_snapshot(|t| t.wl_matched)
    }
    pub fn telemetry_wl_degraded(&self) -> bool {
        self.telemetry_snapshot(|t| t.wl_degraded)
    }
    pub fn telemetry_wl_error(&self) -> String {
        self.telemetry_snapshot(|t| t.wl_error.clone())
    }
    /// 状态码。
    pub fn status_code(&self) -> u8 {
        self.status.load(Relaxed)
    }
    /// 状态中文名 (audio_status_name 纯函数)。
    pub fn status_name(&self) -> &'static str {
        crate::state::audio_state::audio_status_name(self.status_code())
    }
}

// ═══════════════════════════════════════════════════════════════════════
// BackendService 音频领域方法
// ═══════════════════════════════════════════════════════════════════════

impl BackendService {
    /// 音频参数改动统一入口：改 `config.audio_haptics` → `apply_config` 镜像 →
    /// 配置文件去抖落盘 (单一写路径；高频滑杆安全，锁内只有原子写)。
    pub fn update_audio_config<R>(&self, f: impl FnOnce(&mut crate::config::AudioHapticsConfig) -> R) -> R {
        let ret = self.cfg.update_config_debounced(|c| f(&mut c.audio_haptics));
        self.cfg
            .with_config(|c| self.app.audio_hap.apply_config(&c.audio_haptics));
        ret
    }

    /// A/B/C 算法选择的唯一写入口：同步 `smart_enabled` + `audio_algorithm_mode`
    /// 双字段（A = false/"a"，B = true/"b"，C = true/"c"），避免两个字段各写一半
    /// 导致界面显示与引擎实际模式不一致。
    pub fn set_audio_algorithm_mode(&self, mode: u8) {
        let (smart, tag) = match mode {
            AUDIO_ALGO_A => (false, "a"),
            AUDIO_ALGO_B => (true, "b"),
            AUDIO_ALGO_C => (true, "c"),
            _ => return,
        };
        self.update_audio_config(|cfg| {
            cfg.smart_enabled = smart;
            cfg.audio_algorithm_mode = Some(tag.to_string());
        });
    }

    /// 应用校准建议 (旧 audio_page.rs:412-417 语义：写 mask_threshold + 落盘)。
    pub fn audio_apply_calib(&self) -> anyhow::Result<u32> {
        let v = self.app.audio_hap.calib_result.load(Relaxed);
        if v == 0 {
            return Ok(0);
        }
        self.update_audio_config(|a| a.mask_threshold = v);
        self.cfg.save_all_now()?;
        self.app.audio_hap.calib_result.store(0, Relaxed);
        Ok(v)
    }

    // ── 音频专属白名单 (与连发 process_whitelist 独立) ───────────────────

    /// 自定义白名单条目整表替换 (Commit::Save；旧白名单页保存语义)。
    pub fn audio_whitelist_set(&self, entries: Vec<String>) -> anyhow::Result<()> {
        self.update_config(Commit::Save, |c| {
            c.audio_haptics.whitelist_processes = entries;
        })?;
        Ok(())
    }

    /// 追加条目。
    pub fn audio_whitelist_add(&self, process: &str) -> anyhow::Result<()> {
        self.update_config(Commit::Save, |c| {
            let list = &mut c.audio_haptics.whitelist_processes;
            if !list.iter().any(|p| p == process) {
                list.push(process.to_string());
            }
        })?;
        Ok(())
    }

    /// 移除条目。
    pub fn audio_whitelist_remove(&self, process: &str) -> anyhow::Result<()> {
        self.update_config(Commit::Save, |c| {
            c.audio_haptics.whitelist_processes.retain(|p| p != process);
        })?;
        Ok(())
    }

    /// 清空 + 关自定义来源 (切回 "macro" = 复用连发白名单)。
    pub fn audio_whitelist_clear(&self) -> anyhow::Result<()> {
        self.update_config(Commit::Save, |c| {
            c.audio_haptics.whitelist_processes.clear();
            c.audio_haptics.whitelist_source = "macro".to_string();
        })?;
        Ok(())
    }

    // ── 音频预设 ────────────────────────────────────────────────────────

    /// `AudioHapticsConfig` → 预设快照 (引擎参数逐项拷贝，与归档 webui 构造语义一致)。
    fn preset_from_audio(a: &crate::config::AudioHapticsConfig, name: &str) -> crate::config::AudioPresetConfig {
        crate::config::AudioPresetConfig {
            name: name.to_string(),
            user_modified: true,
            master_gain_pct: a.master_gain_pct,
            low_gain_pct: a.low_gain_pct,
            high_gain_pct: a.high_gain_pct,
            punch_pct: a.punch_pct,
            mask_threshold: a.mask_threshold,
            adaptive_threshold: a.adaptive_threshold,
            auto_profile: a.auto_profile,
            music_mask_pct: a.music_mask_pct,
            quiet_mask: a.quiet_mask,
            rhythm_pct: a.rhythm_pct,
            texture_pct: a.texture_pct,
            attack_pct: a.attack_pct,
            release_pct: a.release_pct,
            dead_zone_pct: a.dead_zone_pct,
            map_max: a.map_max,
            dual_motor: a.dual_motor,
            vocal_only: a.vocal_only,
            bass_high_hz: a.bass_high_hz,
            vocal_low_hz: a.vocal_low_hz,
            vocal_high_hz: a.vocal_high_hz,
        }
    }

    /// 应用自定义音频预设 (index 指向 config.audio_haptics.presets；逐项写回)。
    pub fn apply_audio_preset(&self, index: usize) -> anyhow::Result<bool> {
        let preset = self.cfg.with_config(|c| {
            c.audio_haptics
                .presets
                .get(index)
                .map(|p| p.clone())
        });
        let Some(p) = preset else { return Ok(false) };
        self.update_audio_config(|a| {
            a.master_gain_pct = p.master_gain_pct;
            a.low_gain_pct = p.low_gain_pct;
            a.high_gain_pct = p.high_gain_pct;
            a.punch_pct = p.punch_pct;
            a.mask_threshold = p.mask_threshold;
            a.adaptive_threshold = p.adaptive_threshold;
            a.auto_profile = p.auto_profile;
            a.music_mask_pct = p.music_mask_pct;
            a.quiet_mask = p.quiet_mask;
            a.rhythm_pct = p.rhythm_pct;
            a.texture_pct = p.texture_pct;
            a.attack_pct = p.attack_pct;
            a.release_pct = p.release_pct;
            a.dead_zone_pct = p.dead_zone_pct;
            a.map_max = p.map_max;
            a.dual_motor = p.dual_motor;
            a.vocal_only = p.vocal_only;
            a.bass_high_hz = p.bass_high_hz;
            a.vocal_low_hz = p.vocal_low_hz;
            a.vocal_high_hz = p.vocal_high_hz;
        });
        self.cfg.save_all_now()?;
        Ok(true)
    }

    /// 当前参数存为自定义预设 (同名覆盖；只拷引擎参数子集，不嵌套)。
    pub fn save_audio_preset(&self, name: &str) -> anyhow::Result<()> {
        self.cfg.update_config(&self.app, Commit::Save, |c| {
            let snapshot = Self::preset_from_audio(&c.audio_haptics, name);
            match c
                .audio_haptics
                .presets
                .iter_mut()
                .find(|p| p.name == name)
            {
                Some(p) => *p = snapshot,
                None => c.audio_haptics.presets.push(snapshot),
            }
        })?;
        Ok(())
    }

    /// 删除自定义音频预设。
    pub fn delete_audio_preset(&self, index: usize) -> anyhow::Result<()> {
        self.cfg.update_config(&self.app, Commit::Save, |c| {
            if index < c.audio_haptics.presets.len() {
                c.audio_haptics.presets.remove(index);
            }
        })?;
        Ok(())
    }

    // ── ★R2.14 B 阶段: 音频页 GUI 所需的补充 API ────────────────────────

    /// 内置预设表 (audio_haptics::mapping::PRESETS 静态切片; 顺序即 UI 展示顺序)。
    pub fn audio_builtin_presets(&self) -> &'static [crate::audio_haptics::mapping::Preset] {
        crate::audio_haptics::mapping::PRESETS
    }

    /// 按名查内置预设。
    pub fn audio_preset_by_name(
        &self,
        name: &str,
    ) -> Option<&'static crate::audio_haptics::mapping::Preset> {
        crate::audio_haptics::mapping::preset_by_name(name)
    }

    /// 内置预设保护名 (与内置重名的自定义/导入一律拒绝, 旧 is_protected_preset 语义)。
    pub fn audio_preset_protected(&self, name: &str) -> bool {
        crate::audio_haptics::mapping::is_protected_preset(name)
    }

    /// 引擎遥测快照 (旧 audio_page 帧首 try_lock + clone 的等价物;
    /// Mutex 被引擎占用时返回 None, GUI 显示"本帧跳过")。
    pub fn audio_telemetry(&self) -> Option<crate::state::audio_state::AudioTelemetry> {
        self.app.audio_hap.telemetry.try_lock().ok().map(|t| t.clone())
    }

    /// 引擎状态码 (AUDIO_STATUS_* 常量口径; 状态名走 snapshot.audio.status)。
    pub fn audio_status_code(&self) -> u8 {
        self.app.audio_hap.status_code()
    }

    /// ★v24.61 应用预设 (按名; **同名自定义优先**, 否则内置 —— 对齐震动区
    /// pick_preset_for_apply 语义)。返回 false = 找不到该预设。
    pub fn apply_audio_preset_by_name(&self, name: &str) -> anyhow::Result<bool> {
        let custom = self.cfg.with_config(|c| {
            c.audio_haptics
                .presets
                .iter()
                .find(|p| p.name == name)
                .cloned()
        });
        let entry = match custom {
            Some(e) => e,
            None => match self.audio_preset_by_name(name) {
                Some(p) => crate::config::AudioPresetConfig {
                    name: p.name.to_string(),
                    user_modified: false,
                    master_gain_pct: p.master_gain_pct,
                    low_gain_pct: p.low_gain_pct,
                    high_gain_pct: p.high_gain_pct,
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
                },
                None => return Ok(false),
            },
        };
        self.apply_audio_preset_entry(&entry)?;
        Ok(true)
    }

    /// 把预设条目写进 config 引擎字段 (不改 enabled / 白名单; 写 preset 名),
    /// `apply_config` 镜像即时生效 + 立即落盘 (旧 apply_preset_entry 语义)。
    pub fn apply_audio_preset_entry(
        &self,
        e: &crate::config::AudioPresetConfig,
    ) -> anyhow::Result<()> {
        self.update_audio_config(|a| {
            a.preset = e.name.clone();
            a.master_gain_pct = e.master_gain_pct;
            a.low_gain_pct = e.low_gain_pct;
            a.high_gain_pct = e.high_gain_pct;
            a.punch_pct = e.punch_pct;
            a.mask_threshold = e.mask_threshold;
            a.adaptive_threshold = e.adaptive_threshold;
            a.auto_profile = e.auto_profile;
            a.music_mask_pct = e.music_mask_pct;
            a.quiet_mask = e.quiet_mask;
            a.rhythm_pct = e.rhythm_pct;
            a.texture_pct = e.texture_pct;
            a.attack_pct = e.attack_pct;
            a.release_pct = e.release_pct;
            a.dead_zone_pct = e.dead_zone_pct;
            a.map_max = e.map_max;
            a.dual_motor = e.dual_motor;
            a.vocal_only = e.vocal_only;
            a.bass_high_hz = e.bass_high_hz;
            a.vocal_low_hz = e.vocal_low_hz;
            a.vocal_high_hz = e.vocal_high_hz;
        });
        self.cfg.save_all_now()?;
        Ok(())
    }
}
