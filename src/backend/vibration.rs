//! ★R2.14 震动领域 (BackendService impl)。
//!
//! 单一写路径纪律：**任何震动参数改动都先改 `config.vibration`，再整体镜像进
//! 引擎原子** (`apply_vibration_config`)，落盘走路线文件。职业叠加层
//! (`vibration_algo_id/ap`，JobVibration 路径) 不受镜像影响，只由显式方法复位。
//!
//! 槽位常量唯一出处 = `vibration/params.rs` (P_* / 双向映射函数，GUI 渲染共用)。
//! 预设应用语义 = 旧 `gui/vibration_page/presets.rs::apply_general_vibration_preset_in`
//! 的 config/原子部分逐字移植 (job 会话状态仍归 GUI 管理)。

use std::sync::atomic::Ordering::Relaxed;

use super::BackendService;
use crate::config::AppConfig;
use crate::ui_api::Commit;
use crate::vibration::params::{config_from_params, params_from_config};

impl BackendService {
    // ── 60 槽参数 (滑杆热路径：改 config → 镜像 → 去抖落盘) ─────────────

    /// 设置单个震动参数槽 (params.rs 常量定槽位；0-18 具名字段，19-59 advanced)。
    pub fn set_vib_param(&self, slot: usize, value: u32) {
        self.update_vibration_config(|v| {
            if slot < 60 {
                let mut a = params_from_config(v);
                a[slot] = value;
                config_from_params(&a, v);
            }
        });
    }

    /// 设置基础项 L/R 权重 (flat 26 = 13 项 × [L,R])。
    pub fn set_vib_item_lr(&self, index: usize, value: u32) {
        self.update_vibration_config(|v| {
            if index < 26 {
                v.item_lr[index] = value;
            }
        });
    }

    /// 设置评分事件 L/R 权重 (flat 30 = 15 项 × [L,R])。
    pub fn set_vib_rank_lr(&self, index: usize, value: u32) {
        self.update_vibration_config(|v| {
            if index < 30 {
                v.rank_lr[index] = value;
            }
        });
    }

    /// 设置评分细分强度 (15 项)。
    pub fn set_vib_rank_type_gain(&self, index: usize, value: u32) {
        self.update_vibration_config(|v| {
            if index < 15 {
                v.rank_type_gain[index] = value;
            }
        });
    }

    // ── 预设应用 (旧 apply_general_vibration_preset_in 的后端版) ────────

    /// 按名应用震动预设 (内置/自定义；用户主动覆盖过的同名自定义预设优先 ——
    /// `pick_preset_for_apply` 既有语义)。
    ///
    /// 行为：60 槽参数 + item_lr + rank_lr + rank_type_gain + 等级强度/时长 +
    /// 输出平滑 写入 config 并镜像；**职业专属通道复位** (节流清零、算法 id/ap
    /// 归零，与旧「切回通用预设后不残留上一职业」语义一致)；ACT1 特供套附加默认。
    /// 返回 false = 找不到该预设。
    pub fn apply_vibration_preset_by_name(&self, name: &str) -> anyhow::Result<bool> {
        let applied = self.cfg.update_config(&self.app, Commit::SaveVib, |c| {
            let user_entry = c
                .vibration_presets
                .iter()
                .find(|x| x.name == name)
                .cloned();
            let Some(pr) = crate::config::pick_preset_for_apply(user_entry.as_ref(), name) else {
                return false;
            };
            let v = &mut c.vibration;
            config_from_params(&pr.params, v);
            v.item_lr = pr.item_lr;
            v.rank_lr = pr.rank_lr;
            v.rank_type_gain = pr.rank_type_gain;
            v.rank_level_gain = pr.rank_level_gain;
            v.rank_duration = pr.rank_duration;
            v.out_smooth = pr.out_smooth;
            /* 职业专属通道复位 (config 侧；与旧 reset 集合一致) */
            v.throttle_window_ms = 0;
            v.throttle_max_hits = 0;
            v.throttle_dense_ratio = 100;
            /* ★v16.8 ACT1 特供附加默认 + ★v24.16 群怪治理标量 */
            if name == "ACT1 特供" {
                crate::config::act1_preset_extras(v);
            }
            true
        })?;
        if !applied {
            return Ok(false);
        }
        /* 原子镜像 (覆盖 rank_type_gain/等级/时长/平滑/节流/算法开关等全部字段) */
        self.cfg
            .with_config(|c| self.app.apply_vibration_config(&c.vibration));
        /* 职业叠加层复位 (apply_vibration_config 有意不动它们) */
        self.app.vibration_algo_id.store(0, Relaxed);
        for ap in &self.app.vibration_algo_ap {
            ap.store(0, Relaxed);
        }
        Ok(true)
    }

    /// 还原默认 = 应用内置默认预设第一档 (旧「还原默认」语义)。
    pub fn reset_vibration_defaults(&self) -> anyhow::Result<()> {
        let name = crate::config::default_vibration_presets()
            .first()
            .map(|p| p.name.clone())
            .unwrap_or_else(|| "默认".to_string());
        self.apply_vibration_preset_by_name(&name)?;
        Ok(())
    }

    // ── S1/S4 路线切换 (旧 switch_vibration_edition 的后端版；job 会话归 GUI) ──

    /// 切换震动客户端路线。返回目标路线是否有历史文件
    /// (`false` = 首次使用已自动套该路线内置默认预设：S1→「ACT1 特供」，S4→「默认」)。
    ///
    /// GUI 调用后须自行重算职业预设会话状态 (旧 switch 的最后一段)。
    pub fn switch_vib_edition(&self, to_legacy: bool) -> anyhow::Result<bool> {
        let from = self.cfg.with_config(|c| c.vib_legacy_client);
        self.switch_vib_edition_explicit(from, to_legacy)
    }

    // ── 职业叠加层 (JobVibration 路径专用；平时勿碰) ────────────────────

    /// 写职业算法 id/ap 原子 (仅 JobVibration 应用流程使用)。
    pub fn set_vib_algo(&self, id: u32, ap: [u32; 4]) {
        self.app.vibration_algo_id.store(id, Relaxed);
        for (i, v) in ap.iter().enumerate() {
            self.app.vibration_algo_ap[i].store(*v, Relaxed);
        }
    }

    // ── ★R2.14 B 阶段: 震动页 GUI 所需的补充 API ────────────────────────

    /// 60 槽参数定点写 (全职业预设页滑杆热路径): **只写引擎原子**, 不进 config、
    /// 不镜像、不落盘 —— JobVibration 会话的 60 槽参数以原子为会话状态, 由 GUI
    /// 检测快照变化后落 JobVibration.toml (与旧 GUI 的 job_mode 滑杆行为一致)。
    pub fn set_vib_param_job(&self, slot: usize, value: u32) {
        if slot < 60 {
            self.app.vibration_params[slot].store(value, Relaxed);
        }
    }

    /// 飘字总开关 (vibration_font_hits 无 config 字段, 纯运行态原子)。
    pub fn set_vib_font_hits(&self, on: bool) {
        self.app.vibration_font_hits.store(on, Relaxed);
    }

    /// 基础项 L/R 权重定点写: 引擎原子 + config 内存同步 (Commit::Memory ——
    /// 不镜像、不落盘, 与旧 render_item_lr 的「原子+config 双写」语义逐字一致;
    /// 不走全量镜像, 职业叠加层会话期间同样安全)。
    pub fn set_vib_item_lr_quiet(&self, index: usize, value: u32) {
        if index < 26 {
            self.app.vibration_item_lr[index].store(value, Relaxed);
            let _ = self.cfg.update_config(&self.app, Commit::Memory, |c| {
                c.vibration.item_lr[index] = value;
            });
        }
    }

    /// 评分事件 L/R 权重定点写 (语义同 [`Self::set_vib_item_lr_quiet`])。
    pub fn set_vib_rank_lr_quiet(&self, index: usize, value: u32) {
        if index < 30 {
            self.app.vibration_rank_lr[index].store(value, Relaxed);
            let _ = self.cfg.update_config(&self.app, Commit::Memory, |c| {
                c.vibration.rank_lr[index] = value;
            });
        }
    }

    /// 除 60 槽参数外的全量镜像 (`vib_mirror::apply_vibration_config` 的去参数版):
    /// 把 config.vibration 的权重/评分/治理标量/开关同步进引擎原子。
    ///
    /// 全职业预设会话期间 60 槽参数原子归 JobVibration 会话所有 (config 里仍是
    /// 通用参数), 不得被 config 覆盖 —— 字段表与 apply_vibration_config 逐行对应,
    /// 仅去掉 `vibration_params[60]` 一段。
    fn apply_vibration_config_keep_params(&self) {
        let v = self.cfg.with_config(|c| c.vibration.clone());
        let a = &self.app;
        let o = Relaxed;
        a.vibration_enabled.store(v.enabled, o);
        for i in 0..15 {
            a.vibration_rank_type_gain[i].store(v.rank_type_gain[i], o);
        }
        a.vibration_rank_level_gain.store(v.rank_level_gain, o);
        a.vibration_rank_duration.store(v.rank_duration, o);
        a.vibration_advanced_enabled.store(v.advanced_enabled, o);
        a.vibration_motor_l_gain.store(v.motor_l_gain, o);
        a.vibration_motor_r_gain.store(v.motor_r_gain, o);
        a.vibration_random_gain.store(v.random_gain, o);
        a.vibration_out_smooth.store(v.out_smooth, o);
        a.vibration_throttle_window.store(v.throttle_window_ms, o);
        a.vibration_throttle_max.store(v.throttle_max_hits, o);
        a.vibration_throttle_dense_ratio.store(v.throttle_dense_ratio, o);
        a.vibration_merge_keep.store(v.merge_keep, o);
        a.vibration_merge_cap.store(v.merge_cap, o);
        a.vibration_merge_hold.store(v.merge_hold, o);
        a.vibration_hitcap_max.store(v.hitcap_max, o);
        a.vibration_hitcap_win.store(v.hitcap_win_ms, o);
        a.vibration_hitmerge_ms.store(v.hitmerge_ms, o);
        a.vibration_sustain_secs.store(v.sustain_secs, o);
        a.vibration_sustain_reduce.store(v.sustain_reduce, o);
        a.vibration_tail_land_pct.store(v.tail_land_pct, o);
        a.vibration_monster_abnormal.store(v.monster_abnormal_gain, o);
        a.vibration_storm_thr.store(v.storm_thr, o);
        a.vibration_storm_keep_pct.store(v.storm_keep_pct, o);
        a.vibration_storm_win_ms.store(v.storm_win_ms, o);
        a.vibration_storm_pause_ms.store(v.storm_pause_ms, o);
        a.vibration_storm_mute_abnormal.store(v.storm_mute_abnormal, o);
        a.vibration_storm_mute_rank.store(v.storm_mute_rank, o);
        a.vibration_merge_enabled.store(v.merge_enabled, o);
        a.vibration_hitcap_enabled.store(v.hitcap_enabled, o);
        a.vibration_storm_enabled.store(v.storm_enabled, o);
        a.vibration_sustain_enabled.store(v.sustain_enabled, o);
        a.vibration_tail_land_enabled.store(v.tail_land_enabled, o);
        a.vibration_density_enabled.store(v.density_enabled, o);
        a.vibration_adapt_enabled.store(v.adapt_enabled, o);
        a.vibration_move_charge_enabled.store(v.move_charge_enabled, o);
        a.vibration_decay_enabled.store(v.decay_enabled, o);
        a.vibration_algo_windows_enabled.store(v.algo_windows_enabled, o);
        a.vibration_pulse_enabled.store(v.pulse_enabled, o);
        a.vibration_storm_unified_enabled.store(v.storm_unified_enabled, o);
        a.vibration_storm_unified_ms.store(v.storm_unified_ms, o);
        a.vibration_legacy_output_mode.store(v.legacy_output_mode, o);
        a.vibration_independent_test.store(v.independent_test, o);
        a.vibration_move_independent.store(v.move_independent, o);
        a.vibration_abs_freq_enabled.store(v.abs_freq_enabled, o);
        a.vibration_abs_freq_window.store(v.abs_freq_window_ms, o);
        a.vibration_abs_freq_max.store(v.abs_freq_max, o);
        a.vibration_rank_decay_enabled.store(v.rank_decay_enabled, o);
        a.vibration_rank_decay_delay.store(v.rank_decay_delay_ms, o);
        a.vibration_rank_decay_speed.store(v.rank_decay_speed, o);
        a.vibration_rank_decay_min.store(v.rank_decay_min_mul, o);
        a.vibration_rank_decay_max.store(v.rank_decay_max_mul, o);
        a.vibration_out_threshold.store(v.out_threshold, o);
        a.vibration_remap_enabled.store(v.remap_enabled, o);
        a.vibration_remap_min.store(v.remap_min, o);
        a.vibration_split_enabled.store(v.split_enabled, o);
        a.vibration_split_thr.store(v.split_thr, o);
        for i in 0..26 {
            a.vibration_item_lr[i].store(v.item_lr[i], o);
        }
        for i in 0..30 {
            a.vibration_rank_lr[i].store(v.rank_lr[i], o);
        }
    }

    /// 震动配置更新 (去抖落盘) + **保留 60 槽参数**的部分镜像。
    ///
    /// 全职业预设页滑杆的统一写路径: 权重/评分/治理标量/开关即时生效,
    /// 而 60 槽参数原子不被 config 内的通用参数覆盖 (JobVibration 叠加层保护)。
    pub fn update_vib_config_keep_params<R>(
        &self,
        f: impl FnOnce(&mut crate::config::VibrationConfig) -> R,
    ) -> R {
        let ret = self.cfg.update_vib_debounced(|c| f(&mut c.vibration));
        self.apply_vibration_config_keep_params();
        ret
    }

    /// 应用全职业预设 (旧 gui/vibration_page/presets.rs::apply_job_vibration_preset_in
    /// 的 config/原子部分逐字移植):
    /// * 引擎原子: 60 槽参数 (职业叠加层, 不进 config)、rank_lr、rank_type_gain、
    ///   等级强度/时长、输出平滑、节流三件套、绝对频率 (S1 强制关/ S4 honor 预设)、
    ///   ACT 职业另写 item_lr 马达语言 (力度档推导)、职业算法 id/ap;
    /// * config.vibration (Commit::SaveVib 立即落盘, 与旧 save_vibration_to_file 一致):
    ///   上述除 60 槽与算法外的全部字段。
    ///
    /// JobVibration.toml 快照与 GUI 会话字段 (vib_job_*) 由调用方负责。
    pub fn apply_job_vibration_class(&self, base_job: &str, cls: &crate::job_presets::JobClass) {
        let legacy = self.cfg.with_config(|c| c.vib_legacy_client);
        let abs_on = if legacy { false } else { cls.abs_freq_enabled };
        let act_item_lr: Option<[u32; 26]> = if base_job.ends_with("-ACT") {
            let tier = crate::config::act_tier_from_max(cls.params[4]);
            Some(crate::config::act_motor_item_lr(tier))
        } else {
            None
        };
        let _ = self.cfg.update_config(&self.app, Commit::SaveVib, |c| {
            let v = &mut c.vibration;
            if let Some(mlr) = act_item_lr {
                v.item_lr = mlr;
            }
            v.rank_lr = cls.rank_lr.map(|x| x as u32);
            v.rank_type_gain = cls.rank_type_gain;
            v.rank_level_gain = cls.rank_level_gain;
            v.rank_duration = cls.rank_duration;
            v.out_smooth = cls.out_smooth;
            v.throttle_window_ms = cls.throttle_window;
            v.throttle_max_hits = cls.throttle_max;
            v.throttle_dense_ratio = cls.throttle_dense_ratio;
            v.abs_freq_enabled = abs_on;
        });
        /* 引擎原子直写 (60 槽与职业算法是叠加层, 绝不能走全量镜像) */
        let a = &self.app;
        let o = Relaxed;
        for (i, val) in cls.params.iter().enumerate() {
            a.vibration_params[i].store(*val, o);
        }
        for (i, val) in cls.rank_lr.iter().enumerate() {
            a.vibration_rank_lr[i].store(*val as u32, o);
        }
        for (i, val) in cls.rank_type_gain.iter().enumerate() {
            a.vibration_rank_type_gain[i].store(*val, o);
        }
        if let Some(mlr) = act_item_lr {
            for (i, val) in mlr.iter().enumerate() {
                a.vibration_item_lr[i].store(*val, o);
            }
        }
        a.vibration_rank_level_gain.store(cls.rank_level_gain, o);
        a.vibration_rank_duration.store(cls.rank_duration, o);
        a.vibration_out_smooth.store(cls.out_smooth, o);
        a.vibration_throttle_window.store(cls.throttle_window, o);
        a.vibration_throttle_max.store(cls.throttle_max, o);
        a.vibration_throttle_dense_ratio.store(cls.throttle_dense_ratio, o);
        a.vibration_abs_freq_enabled.store(abs_on, o);
        a.vibration_algo_id.store(cls.algo_id as u32, o);
        for (i, val) in cls.algo_params.iter().enumerate() {
            a.vibration_algo_ap[i].store(*val, o);
        }
    }

    /// 启动恢复全职业会话 (旧 vib_first_render_init 职业段的 config/原子部分):
    /// 与 [`Self::apply_job_vibration_class`] 相同的原子写入; config 侧只做内存
    /// 同步 (Commit::Memory —— 旧恢复路径不落盘), 且不做 ACT item_lr 马达语言
    /// 同步 (旧语义)。`jcfg` 的 60 槽快照 (len==60) 与等级/时长覆盖 (>0) 生效。
    pub fn restore_job_vibration_session(
        &self,
        jcfg: &crate::job_presets::JobVibrationConfig,
        cls: &crate::job_presets::JobClass,
    ) {
        let legacy = self.cfg.with_config(|c| c.vib_legacy_client);
        let abs_on = if legacy { false } else { cls.abs_freq_enabled };
        let params: [u32; 60] = if jcfg.params.len() == 60 {
            let mut p = cls.params;
            p.copy_from_slice(&jcfg.params);
            p
        } else {
            cls.params
        };
        let dur = if jcfg.rank_duration > 0 { jcfg.rank_duration } else { cls.rank_duration };
        let lg = if jcfg.rank_level_gain > 0 { jcfg.rank_level_gain } else { cls.rank_level_gain };
        let a = &self.app;
        let o = Relaxed;
        for (i, val) in params.iter().enumerate() {
            a.vibration_params[i].store(*val, o);
        }
        for (i, val) in cls.rank_lr.iter().enumerate() {
            a.vibration_rank_lr[i].store(*val as u32, o);
        }
        for (i, val) in cls.rank_type_gain.iter().enumerate() {
            a.vibration_rank_type_gain[i].store(*val, o);
        }
        a.vibration_rank_duration.store(dur, o);
        a.vibration_rank_level_gain.store(lg, o);
        a.vibration_out_smooth.store(cls.out_smooth, o);
        a.vibration_throttle_window.store(cls.throttle_window, o);
        a.vibration_throttle_max.store(cls.throttle_max, o);
        a.vibration_throttle_dense_ratio.store(cls.throttle_dense_ratio, o);
        a.vibration_abs_freq_enabled.store(abs_on, o);
        a.vibration_algo_id.store(cls.algo_id as u32, o);
        for (i, val) in cls.algo_params.iter().enumerate() {
            a.vibration_algo_ap[i].store(*val, o);
        }
        /* config 内存同步 (除 60 槽外; 保证 keep_params 镜像在会话期间不回冲) */
        let _ = self.cfg.update_config(&self.app, Commit::Memory, |c| {
            let v = &mut c.vibration;
            v.rank_lr = cls.rank_lr.map(|x| x as u32);
            v.rank_type_gain = cls.rank_type_gain;
            v.rank_level_gain = lg;
            v.rank_duration = dur;
            v.out_smooth = cls.out_smooth;
            v.throttle_window_ms = cls.throttle_window;
            v.throttle_max_hits = cls.throttle_max;
            v.throttle_dense_ratio = cls.throttle_dense_ratio;
            v.abs_freq_enabled = abs_on;
        });
    }

    /// 导入职业设置 (旧「导入职业设置」按钮的后端版): **只写引擎原子** (不进
    /// config、不落盘) —— GUI 快照检测会把参数落 JobVibration.toml (旧注释语义)。
    /// 等级/时长/平滑沿用旧的条件覆盖 (>0 才写)。
    pub fn import_job_vibration(&self, ij: &crate::job_presets::ImportedJob) {
        let a = &self.app;
        let o = Relaxed;
        for (i, val) in ij.params.iter().enumerate() {
            if i < 60 {
                a.vibration_params[i].store(*val, o);
            }
        }
        for (i, val) in ij.rank_lr.iter().enumerate() {
            if i < 30 {
                a.vibration_rank_lr[i].store(*val as u32, o);
            }
        }
        for (i, val) in ij.rank_type_gain.iter().enumerate() {
            if i < 15 {
                a.vibration_rank_type_gain[i].store(*val, o);
            }
        }
        if ij.rank_level_gain > 0 {
            a.vibration_rank_level_gain.store(ij.rank_level_gain, o);
        }
        if ij.rank_duration > 0 {
            a.vibration_rank_duration.store(ij.rank_duration, o);
        }
        if ij.out_smooth > 0 {
            a.vibration_out_smooth.store(ij.out_smooth, o);
        }
        a.vibration_throttle_window.store(ij.throttle_window, o);
        a.vibration_throttle_max.store(ij.throttle_max, o);
        a.vibration_throttle_dense_ratio.store(ij.throttle_dense_ratio, o);
    }

    /// 关闭全职业预设 (旧 disable_job_vibration_preset_in 的 config/原子部分):
    /// 内置默认预设第一档回引擎原子 (60 槽不进 config), 绝对频率/节流/职业算法
    /// 复位; config.vibration 同步复位值 (Commit::Memory —— 旧关闭路径不落盘,
    /// JobVibration.toml applied=false 由 GUI 写)。
    pub fn disable_job_vibration(&self) {
        let pr = crate::config::default_vibration_presets().first().cloned();
        let a = &self.app;
        let o = Relaxed;
        if let Some(pr) = pr {
            for (i, val) in pr.params.iter().enumerate() {
                a.vibration_params[i].store(*val, o);
            }
            for (i, val) in pr.item_lr.iter().enumerate() {
                a.vibration_item_lr[i].store(*val, o);
            }
            for (i, val) in pr.rank_lr.iter().enumerate() {
                a.vibration_rank_lr[i].store(*val, o);
            }
            for (i, val) in pr.rank_type_gain.iter().enumerate() {
                a.vibration_rank_type_gain[i].store(*val, o);
            }
            a.vibration_rank_level_gain.store(pr.rank_level_gain, o);
            a.vibration_rank_duration.store(pr.rank_duration, o);
            a.vibration_out_smooth.store(pr.out_smooth, o);
        }
        a.vibration_abs_freq_enabled.store(false, o);
        a.vibration_throttle_window.store(0, o);
        a.vibration_throttle_max.store(0, o);
        a.vibration_throttle_dense_ratio.store(100, o);
        a.vibration_algo_id.store(0, o);
        for ap in &a.vibration_algo_ap {
            ap.store(0, o);
        }
        let _ = self.cfg.update_config(&self.app, Commit::Memory, |c| {
            let v = &mut c.vibration;
            if let Some(pr) = crate::config::default_vibration_presets().first() {
                config_from_params(&pr.params, v);
                v.item_lr = pr.item_lr;
                v.rank_lr = pr.rank_lr;
                v.rank_type_gain = pr.rank_type_gain;
                v.rank_level_gain = pr.rank_level_gain;
                v.rank_duration = pr.rank_duration;
                v.out_smooth = pr.out_smooth;
            }
            v.throttle_window_ms = 0;
            v.throttle_max_hits = 0;
            v.throttle_dense_ratio = 100;
            v.abs_freq_enabled = false;
        });
    }

    /// 旧「保存当前」语义: 把当前引擎镜像 (60 槽/权重/评分参数) 写回
    /// config.vibration 并立即落盘路线文件。
    pub fn save_vib_runtime_to_config(&self) -> anyhow::Result<()> {
        let snap = self.snapshot();
        let v = &snap.vibration;
        let mut params = [0u32; 60];
        params.copy_from_slice(&v.params);
        let mut item_lr = [0u32; 26];
        item_lr.copy_from_slice(&v.item_lr);
        let mut rank_lr = [0u32; 30];
        rank_lr.copy_from_slice(&v.rank_lr);
        let mut rank_type_gain = [0u32; 15];
        rank_type_gain.copy_from_slice(&v.rank_type_gain);
        self.cfg
            .update_config(&self.app, Commit::SaveVib, |c| {
                let vc = &mut c.vibration;
                config_from_params(&params, vc);
                vc.item_lr = item_lr;
                vc.rank_lr = rank_lr;
                vc.rank_type_gain = rank_type_gain;
                vc.rank_level_gain = v.rank_level_gain;
                vc.rank_duration = v.rank_duration;
            })?;
        Ok(())
    }

    /// 导入通用震动设定 (旧「导入设置」按钮的后端版; 往返一致规范):
    /// 60 槽参数 + 权重/评分/治理标量/开关写进 config 并全量镜像 + 立即落盘。
    /// (旧实现 60 槽只写原子不进 config, 重启即丢; 新世界 config 为唯一权威,
    /// 导入即持久化 —— 与导出文件头的「往返一致规范」一致。)
    pub fn import_vib_export(&self, iv: &crate::job_presets::ImportedVibration) {
        let mut params = [0u32; 60];
        params.copy_from_slice(&iv.params);
        let mut item_lr = [0u32; 26];
        item_lr.copy_from_slice(&iv.item_lr);
        let mut rank_lr = [0u32; 30];
        rank_lr.copy_from_slice(&iv.rank_lr);
        let mut rank_type_gain = [0u32; 15];
        rank_type_gain.copy_from_slice(&iv.rank_type_gain);
        let _ = self
            .cfg
            .update_config(&self.app, Commit::SaveVib, |c| {
                let v = &mut c.vibration;
                config_from_params(&params, v);
                v.item_lr = item_lr;
                v.rank_lr = rank_lr;
                v.rank_type_gain = rank_type_gain;
                v.rank_level_gain = iv.rank_level_gain;
                v.rank_duration = iv.rank_duration;
                v.out_smooth = iv.out_smooth;
                v.throttle_window_ms = iv.throttle_window_ms;
                v.throttle_max_hits = iv.throttle_max_hits;
                v.throttle_dense_ratio = iv.throttle_dense_ratio;
                v.independent_test = iv.independent_test;
                v.move_independent = iv.move_independent;
                v.random_gain = iv.random_gain;
                v.motor_l_gain = iv.motor_l_gain;
                v.motor_r_gain = iv.motor_r_gain;
            });
        self.cfg
            .with_config(|c| self.app.apply_vibration_config(&c.vibration));
    }

    /// 切换震动客户端路线 (显式 from 版) —— 设置弹窗流程在调本方法**前**已把
    /// config 的路线标记写成新值, 必须用旧值判断是否真的换了路线
    /// (旧 gui switch_vibration_edition 的 from_legacy 语义)。
    /// 返回目标路线是否有历史文件 (false = 已自动套该路线内置默认预设)。
    pub fn switch_vib_edition_explicit(
        &self,
        from_legacy: bool,
        to_legacy: bool,
    ) -> anyhow::Result<bool> {
        if from_legacy == to_legacy {
            return Ok(true);
        }
        let base = AppConfig::vibration_path_for(self.cfg.path());
        let had_saved = self.cfg.update_config(&self.app, Commit::Memory, |c| {
            c.switch_vibration_edition_to(&base, from_legacy, to_legacy)
        })?;
        if had_saved {
            /* 目标路线有历史参数 → 全量同步到运行态 */
            self.cfg
                .with_config(|c| self.app.apply_vibration_config(&c.vibration));
        } else {
            /* 目标路线首次使用 → 套该路线内置默认预设 (会写 config + 镜像 + 落盘) */
            let want = if to_legacy { "ACT1 特供" } else { "默认" };
            self.apply_vibration_preset_by_name(&want)?;
        }
        /* 路线标记落盘 (Config.toml 根字段; 旧流程等下次保存才持久化, 这里即时化) */
        self.cfg
            .update_config(&self.app, Commit::Save, |c| c.vib_legacy_client = to_legacy)?;
        self.app.vib_legacy_client.store(to_legacy, Relaxed);
        Ok(had_saved)
    }
}
