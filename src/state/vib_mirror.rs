//! 震动参数镜像: VibrationConfig ↔ 60 槽原子数组 (v24.13) —— 原 state.rs 1301-1411, 2026-09-27 架构重构 B8 归位。
//! ★D 方案 (参数单一事实源) 的改造对象: config↔槽位双向映射将收拢到 vibration/params.rs。

use std::sync::atomic::Ordering;

use crate::config::VibrationConfig;

use super::*;

impl AppState {
    /// ★v24.13: VibrationConfig → 60 槽参数数组 (启动装配 / S1↔S4 切换共用, 防两处漂移)。
    /// ★D 方案: 映射唯一写法在 vibration/params.rs, 此处保留原签名作委托 (callers 不动)。
    pub(super) fn vibration_params_from(c: &VibrationConfig) -> [u32; 60] {
        crate::vibration::params::params_from_config(c)
    }

    /// ★v24.13: 把**整套** VibrationConfig 同步到运行态原子量 (59 个字段 + 60 槽参数数组)。
    ///
    /// 用途: S1(ACT)/S4 两套参数各存一份, 切换客户端路线时全量生效 (此前切换只换引擎路由,
    /// 参数不换 → ACT 特调漏进 S4 手感)。
    /// 注意: **不动** `vibration_algo_id` / `vibration_algo_ap` —— 那是 JobVibration 职业叠加,
    /// 不属于 VibrationConfig, 由职业预设路径单独管理。
    pub fn apply_vibration_config(&self, cfg: &VibrationConfig) {
        self.vibration_enabled.store(cfg.enabled, Ordering::Relaxed);
        for i in 0..15 {
            self.vibration_rank_type_gain[i].store(cfg.rank_type_gain[i], Ordering::Relaxed);
        }
        self.vibration_rank_level_gain.store(cfg.rank_level_gain, Ordering::Relaxed);
        self.vibration_rank_duration.store(cfg.rank_duration, Ordering::Relaxed);
        self.vibration_advanced_enabled.store(cfg.advanced_enabled, Ordering::Relaxed);
        self.vibration_motor_l_gain.store(cfg.motor_l_gain, Ordering::Relaxed);
        self.vibration_motor_r_gain.store(cfg.motor_r_gain, Ordering::Relaxed);
        self.vibration_random_gain.store(cfg.random_gain, Ordering::Relaxed);
        self.vibration_out_smooth.store(cfg.out_smooth, Ordering::Relaxed);
        self.vibration_throttle_window.store(cfg.throttle_window_ms, Ordering::Relaxed);
        self.vibration_throttle_max.store(cfg.throttle_max_hits, Ordering::Relaxed);
        self.vibration_throttle_dense_ratio.store(cfg.throttle_dense_ratio, Ordering::Relaxed);
        self.vibration_merge_keep.store(cfg.merge_keep, Ordering::Relaxed);
        self.vibration_merge_cap.store(cfg.merge_cap, Ordering::Relaxed);
        self.vibration_merge_hold.store(cfg.merge_hold, Ordering::Relaxed);
        self.vibration_hitcap_max.store(cfg.hitcap_max, Ordering::Relaxed);
        self.vibration_hitcap_win.store(cfg.hitcap_win_ms, Ordering::Relaxed);
        self.vibration_hitmerge_ms.store(cfg.hitmerge_ms, Ordering::Relaxed);
        self.vibration_sustain_secs.store(cfg.sustain_secs, Ordering::Relaxed);
        self.vibration_sustain_reduce.store(cfg.sustain_reduce, Ordering::Relaxed);
        self.vibration_tail_land_pct.store(cfg.tail_land_pct, Ordering::Relaxed);
        self.vibration_monster_abnormal.store(cfg.monster_abnormal_gain, Ordering::Relaxed);
        self.vibration_storm_thr.store(cfg.storm_thr, Ordering::Relaxed);
        self.vibration_storm_keep_pct.store(cfg.storm_keep_pct, Ordering::Relaxed);
        self.vibration_storm_win_ms.store(cfg.storm_win_ms, Ordering::Relaxed);
        self.vibration_storm_pause_ms.store(cfg.storm_pause_ms, Ordering::Relaxed);
        self.vibration_storm_mute_abnormal.store(cfg.storm_mute_abnormal, Ordering::Relaxed);
        self.vibration_storm_mute_rank.store(cfg.storm_mute_rank, Ordering::Relaxed);
        self.vibration_merge_enabled.store(cfg.merge_enabled, Ordering::Relaxed);
        self.vibration_hitcap_enabled.store(cfg.hitcap_enabled, Ordering::Relaxed);
        self.vibration_storm_enabled.store(cfg.storm_enabled, Ordering::Relaxed);
        self.vibration_sustain_enabled.store(cfg.sustain_enabled, Ordering::Relaxed);
        self.vibration_tail_land_enabled.store(cfg.tail_land_enabled, Ordering::Relaxed);
        self.vibration_density_enabled.store(cfg.density_enabled, Ordering::Relaxed);
        self.vibration_adapt_enabled.store(cfg.adapt_enabled, Ordering::Relaxed);
        self.vibration_move_charge_enabled.store(cfg.move_charge_enabled, Ordering::Relaxed);
        self.vibration_decay_enabled.store(cfg.decay_enabled, Ordering::Relaxed);
        self.vibration_algo_windows_enabled.store(cfg.algo_windows_enabled, Ordering::Relaxed);
        self.vibration_pulse_enabled.store(cfg.pulse_enabled, Ordering::Relaxed);
        self.vibration_storm_unified_enabled.store(cfg.storm_unified_enabled, Ordering::Relaxed);
        self.vibration_storm_unified_ms.store(cfg.storm_unified_ms, Ordering::Relaxed);
        self.vibration_legacy_output_mode.store(cfg.legacy_output_mode, Ordering::Relaxed);
        self.vibration_independent_test.store(cfg.independent_test, Ordering::Relaxed);
        self.vibration_move_independent.store(cfg.move_independent, Ordering::Relaxed);
        self.vibration_abs_freq_enabled.store(cfg.abs_freq_enabled, Ordering::Relaxed);
        self.vibration_abs_freq_window.store(cfg.abs_freq_window_ms, Ordering::Relaxed);
        self.vibration_abs_freq_max.store(cfg.abs_freq_max, Ordering::Relaxed);
        self.vibration_rank_decay_enabled.store(cfg.rank_decay_enabled, Ordering::Relaxed);
        self.vibration_rank_decay_delay.store(cfg.rank_decay_delay_ms, Ordering::Relaxed);
        self.vibration_rank_decay_speed.store(cfg.rank_decay_speed, Ordering::Relaxed);
        self.vibration_rank_decay_min.store(cfg.rank_decay_min_mul, Ordering::Relaxed);
        self.vibration_rank_decay_max.store(cfg.rank_decay_max_mul, Ordering::Relaxed);
        self.vibration_out_threshold.store(cfg.out_threshold, Ordering::Relaxed);
        self.vibration_remap_enabled.store(cfg.remap_enabled, Ordering::Relaxed);
        self.vibration_remap_min.store(cfg.remap_min, Ordering::Relaxed);
        self.vibration_split_enabled.store(cfg.split_enabled, Ordering::Relaxed);
        self.vibration_split_thr.store(cfg.split_thr, Ordering::Relaxed);
        for i in 0..26 {
            self.vibration_item_lr[i].store(cfg.item_lr[i], Ordering::Relaxed);
        }
        for i in 0..30 {
            self.vibration_rank_lr[i].store(cfg.rank_lr[i], Ordering::Relaxed);
        }
        let p = Self::vibration_params_from(cfg);
        for i in 0..60 {
            self.vibration_params[i].store(p[i], Ordering::Relaxed);
        }
    }
}
