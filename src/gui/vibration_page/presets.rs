//! 预设应用/职业启用/版本切换 (显式参数版 *_in 供闭包使用) —— 原 vibration_page.rs 3235-3576, 2026-09-27 架构重构 C1 归位。

use crate::gui::SorahkGui;


impl SorahkGui {
    /// 应用通用震动预设 (显式参数版: 供 vib_section 闭包内使用, 避免整体 &mut self 捕获)。
    /// 互斥: 若全职业预设开启, 先关闭并恢复默认。
    #[allow(clippy::too_many_arguments)]
    pub(in crate::gui) fn apply_general_vibration_preset_in(
        name: &str,
        app_state: &crate::state::AppState,
        config: &mut crate::config::AppConfig,
        job_enabled: &mut bool,
        job_active: &mut Option<(String, String)>,
        job_loaded: &mut Option<(usize, usize)>,
    ) {
        if *job_enabled {
            Self::disable_job_vibration_preset_in(app_state, config, job_enabled, job_active, job_loaded);
        }
        let p = &app_state.vibration_params;
        /* ★v18: 取值规则收敛到纯函数 —— 用户主动覆盖过同名预设才优先, 否则内置
         * 同名预设优先 (版本升级/调乱后可一键回归内置值; ACT1 特供不必落在列表) */
        let user_entry = config
            .vibration_presets
            .iter()
            .find(|x| x.name == name)
            .cloned();
        let pr = match crate::config::pick_preset_for_apply(user_entry.as_ref(), name) {
            Some(x) => x,
            None => return,
        };
        for (i, v) in pr.params.iter().enumerate() {
            p[i].store(*v, std::sync::atomic::Ordering::Relaxed);
        }
        for (i, v) in pr.item_lr.iter().enumerate() {
            app_state.vibration_item_lr[i].store(*v, std::sync::atomic::Ordering::Relaxed);
            config.vibration.item_lr[i] = *v;
        }
        for (i, v) in pr.rank_lr.iter().enumerate() {
            app_state.vibration_rank_lr[i].store(*v, std::sync::atomic::Ordering::Relaxed);
            config.vibration.rank_lr[i] = *v;
        }
        for (i, v) in pr.rank_type_gain.iter().enumerate() {
            app_state.vibration_rank_type_gain[i].store(*v, std::sync::atomic::Ordering::Relaxed);
        }
        app_state.vibration_rank_level_gain.store(pr.rank_level_gain, std::sync::atomic::Ordering::Relaxed);
        app_state.vibration_rank_duration.store(pr.rank_duration, std::sync::atomic::Ordering::Relaxed);
        config.vibration.rank_type_gain = pr.rank_type_gain;
        config.vibration.rank_level_gain = pr.rank_level_gain;
        config.vibration.rank_duration = pr.rank_duration;
        app_state.vibration_out_smooth.store(pr.out_smooth, std::sync::atomic::Ordering::Relaxed);
        config.vibration.out_smooth = pr.out_smooth;
        // 职业专属通道复位 (与 disable_job 同语义): 切回通用预设后节流/算法
        // 不能残留上一职业的值 (apply_job 会写入它们, 三个函数复位集合必须一致)
        app_state.vibration_throttle_window.store(0, std::sync::atomic::Ordering::Relaxed);
        app_state.vibration_throttle_max.store(0, std::sync::atomic::Ordering::Relaxed);
        app_state.vibration_throttle_dense_ratio.store(100, std::sync::atomic::Ordering::Relaxed);
        app_state.vibration_algo_id.store(0, std::sync::atomic::Ordering::Relaxed);
        for ap in &app_state.vibration_algo_ap {
            ap.store(0, std::sync::atomic::Ordering::Relaxed);
        }
        config.vibration.throttle_window_ms = 0;
        config.vibration.throttle_max_hits = 0;
        config.vibration.throttle_dense_ratio = 100;
        Self::sync_vib_config_from_params(&mut config.vibration, p);
        /* ★v16.8 ACT1 特供附加默认 + ★v24.16 群怪治理标量 (用户手调定稿):
         * 高级调校默认开启 + 两个风暴期静音 + 持续降温/震尾切断/统合衰减期/怪物异常反馈。
         * 这些不是 60 槽字段, 必须在这里同步到运行态原子量 (以及 config)。 */
        if name == "ACT1 特供" {
            crate::config::act1_preset_extras(&mut config.vibration);
            let v = &config.vibration;
            app_state
                .vibration_advanced_enabled
                .store(v.advanced_enabled, std::sync::atomic::Ordering::Relaxed);
            app_state
                .vibration_storm_mute_abnormal
                .store(v.storm_mute_abnormal, std::sync::atomic::Ordering::Relaxed);
            app_state
                .vibration_storm_mute_rank
                .store(v.storm_mute_rank, std::sync::atomic::Ordering::Relaxed);
            app_state
                .vibration_sustain_enabled
                .store(v.sustain_enabled, std::sync::atomic::Ordering::Relaxed);
            app_state
                .vibration_sustain_secs
                .store(v.sustain_secs, std::sync::atomic::Ordering::Relaxed);
            app_state
                .vibration_tail_land_enabled
                .store(v.tail_land_enabled, std::sync::atomic::Ordering::Relaxed);
            app_state
                .vibration_tail_land_pct
                .store(v.tail_land_pct, std::sync::atomic::Ordering::Relaxed);
            app_state
                .vibration_storm_unified_enabled
                .store(v.storm_unified_enabled, std::sync::atomic::Ordering::Relaxed);
            app_state
                .vibration_storm_unified_ms
                .store(v.storm_unified_ms, std::sync::atomic::Ordering::Relaxed);
            app_state
                .vibration_monster_abnormal
                .store(v.monster_abnormal_gain, std::sync::atomic::Ordering::Relaxed);
        }
        let _ = config.save_vibration_to_file(crate::config::AppConfig::vibration_path_for("Config.toml"));
    }


    /// 关闭全职业预设 (显式参数版): 恢复默认 + 落盘 applied=false。
    pub(in crate::gui) fn disable_job_vibration_preset_in(
        app_state: &crate::state::AppState,
        config: &mut crate::config::AppConfig,
        job_enabled: &mut bool,
        job_active: &mut Option<(String, String)>,
        job_loaded: &mut Option<(usize, usize)>,
    ) {
        *job_enabled = false;
        *job_active = None;
        *job_loaded = None;
        let p = &app_state.vibration_params;
        if let Some(pr) = crate::config::default_vibration_presets().first() {
            for (i, v) in pr.params.iter().enumerate() {
                p[i].store(*v, std::sync::atomic::Ordering::Relaxed);
            }
            for (i, v) in pr.item_lr.iter().enumerate() {
                app_state.vibration_item_lr[i].store(*v, std::sync::atomic::Ordering::Relaxed);
            }
            for (i, v) in pr.rank_lr.iter().enumerate() {
                app_state.vibration_rank_lr[i].store(*v, std::sync::atomic::Ordering::Relaxed);
            }
            for (i, v) in pr.rank_type_gain.iter().enumerate() {
                app_state.vibration_rank_type_gain[i].store(*v, std::sync::atomic::Ordering::Relaxed);
            }
            app_state.vibration_rank_level_gain.store(pr.rank_level_gain, std::sync::atomic::Ordering::Relaxed);
            app_state.vibration_rank_duration.store(pr.rank_duration, std::sync::atomic::Ordering::Relaxed);
            app_state.vibration_out_smooth.store(pr.out_smooth, std::sync::atomic::Ordering::Relaxed);
        }
        app_state.vibration_abs_freq_enabled.store(false, std::sync::atomic::Ordering::Relaxed);
        // 职业专属通道复位: 节流/专属算法由 apply_job 写入, 关闭职业预设必须清零,
        // 否则 UI 已显示"默认"而引擎仍按旧职业的节流+算法运行, 且残留值随保存持久化
        app_state.vibration_throttle_window.store(0, std::sync::atomic::Ordering::Relaxed);
        app_state.vibration_throttle_max.store(0, std::sync::atomic::Ordering::Relaxed);
        app_state.vibration_throttle_dense_ratio.store(100, std::sync::atomic::Ordering::Relaxed);
        app_state.vibration_algo_id.store(0, std::sync::atomic::Ordering::Relaxed);
        for ap in &app_state.vibration_algo_ap {
            ap.store(0, std::sync::atomic::Ordering::Relaxed);
        }
        config.vibration.throttle_window_ms = 0;
        config.vibration.throttle_max_hits = 0;
        config.vibration.throttle_dense_ratio = 100;
        config.vibration.abs_freq_enabled = false;
        crate::job_presets::save_job_vibration(&crate::job_presets::JobVibrationConfig {
            base_job: String::new(),
            class_name: String::new(),
            applied: false,
            params: Vec::new(),
            rank_duration: 0,
            rank_level_gain: 0,
        });
    }


    /// 应用全职业预设 (显式参数版): 置启用态并落盘。
    #[allow(clippy::too_many_arguments)]
    pub(in crate::gui) fn apply_job_vibration_preset_in(
        base_job: &str,
        cls: &crate::job_presets::JobClass,
        app_state: &crate::state::AppState,
        config: &mut crate::config::AppConfig,
        job_enabled: &mut bool,
        job_active: &mut Option<(String, String)>,
        job_snapshot: &mut Vec<u32>,
    ) {
        let p = &app_state.vibration_params;
        for (i, v) in cls.params.iter().enumerate() {
            p[i].store(*v, std::sync::atomic::Ordering::Relaxed);
        }
        /* ★v19.4: ACT 职业预设 (-ACT) 一并写入 ACT 马达语言 (item_lr) ——
         * JobClass 不携带马达权重, 由档位 (params[4]) 推导 */
        if base_job.ends_with("-ACT") {
            let tier = crate::config::act_tier_from_max(cls.params[4]);
            let mlr = crate::config::act_motor_item_lr(tier);
            for (i, v) in mlr.iter().enumerate() {
                app_state.vibration_item_lr[i].store(*v, std::sync::atomic::Ordering::Relaxed);
                config.vibration.item_lr[i] = *v;
            }
        }
        for (i, v) in cls.rank_lr.iter().enumerate() {
            app_state.vibration_rank_lr[i].store(*v as u32, std::sync::atomic::Ordering::Relaxed);
            config.vibration.rank_lr[i] = *v as u32;
        }
        for (i, v) in cls.rank_type_gain.iter().enumerate() {
            app_state.vibration_rank_type_gain[i].store(*v, std::sync::atomic::Ordering::Relaxed);
            config.vibration.rank_type_gain[i] = *v;
        }
        app_state.vibration_rank_level_gain.store(cls.rank_level_gain, std::sync::atomic::Ordering::Relaxed);
        app_state.vibration_rank_duration.store(cls.rank_duration, std::sync::atomic::Ordering::Relaxed);
        config.vibration.rank_level_gain = cls.rank_level_gain;
        config.vibration.rank_duration = cls.rank_duration;
        app_state.vibration_out_smooth.store(cls.out_smooth, std::sync::atomic::Ordering::Relaxed);
        config.vibration.out_smooth = cls.out_smooth;
        app_state.vibration_throttle_window.store(cls.throttle_window, std::sync::atomic::Ordering::Relaxed);
        app_state.vibration_throttle_max.store(cls.throttle_max, std::sync::atomic::Ordering::Relaxed);
        app_state.vibration_throttle_dense_ratio.store(cls.throttle_dense_ratio, std::sync::atomic::Ordering::Relaxed);
        /* ★保险A (2026-09-09, 用户定稿): 切换职业预设一律强制关闭绝对震动频率 ——
         * 该模式 3s/6 次的节流曾因遗留开启导致"完全不震" (用户实测确认根因)。
         * 转职视为意图变更, 召唤师也不自动恢复; 需要时改 Vibration.toml 手动开。 */
        app_state.vibration_abs_freq_enabled.store(false, std::sync::atomic::Ordering::Relaxed);
        app_state.vibration_algo_id.store(cls.algo_id as u32, std::sync::atomic::Ordering::Relaxed);
        for (i, v) in cls.algo_params.iter().enumerate() {
            app_state.vibration_algo_ap[i].store(*v, std::sync::atomic::Ordering::Relaxed);
        }
        config.vibration.throttle_window_ms = cls.throttle_window;
        config.vibration.throttle_max_hits = cls.throttle_max;
        config.vibration.throttle_dense_ratio = cls.throttle_dense_ratio;
        config.vibration.abs_freq_enabled = false;
        /* 保险A立即落盘 (震动页三个调用点没有后续 save_to_file) */
        let _ = config.save_vibration_to_file(crate::config::AppConfig::vibration_path_for("Config.toml"));
        *job_snapshot = cls.params.to_vec();
        Self::save_job_vibration_state(app_state, base_job, cls.name);
        *job_enabled = true;
        *job_active = Some((base_job.to_string(), cls.name.to_string()));
    }


    /// 便捷包装 (极简模式等无借用冲突的调用点)。
    /// ★ACT1 特供预设: S1 路线时确保出现在预设列表「默认」之下 (缺失插入 /
    /// 错位搬移, 见 config::ensure_act1_preset_position); S4/非 DFO 由各列表处
    /// 的过滤器隐藏。搬移/插入发生变更时立即落盘 —— 自愈旧版 push 到末尾
    /// 且已持久化进 Vibration.toml 的错位残留 (bug: ACT1 特供在列表末尾)。
    pub(in crate::gui) fn ensure_act1_preset_listed(&mut self) {
        let legacy = self.config.vib_legacy_client;
        let mut changed =
            crate::config::ensure_act1_preset_position(&mut self.config.vibration_presets, legacy);
        /* ★v19: 通用预设的 ACT 变体 (高振幅-ACT 等) 同样按需插入/自愈 */
        changed |= crate::config::ensure_act_general_variants(
            &mut self.config.vibration_presets,
            legacy,
        );
        if changed {
            let _ = self
                .config
                .save_vibration_to_file(crate::config::AppConfig::vibration_path_for(
                    "Config.toml",
                ));
        }
    }

    pub(in crate::gui) fn apply_general_vibration_preset(&mut self, name: &str) {
        Self::apply_general_vibration_preset_in(
            name,
            &self.app_state,
            &mut self.config,
            &mut self.vib_job_enabled,
            &mut self.vib_job_active,
            &mut self.vib_job_loaded,
        );
    }


    /// ★v24.14: 切换客户端路线 (S1/S4) —— 两条路线各自**独立文件**
    /// (S1 → `Vibration-ACT.toml`; S4+ → `Vibration.toml`), 参数与预设列表互不干扰。
    ///
    /// `from_legacy` = 当前 `self.config.vibration` 所属路线 (调用方须在批量赋值**前**取旧值),
    /// `to_legacy` = 目标路线。目标路线首次使用 (无历史文件) 时套用该路线内置默认预设:
    /// S1 → 「ACT1 特供」; S4 → 内置原版第一项「默认」。
    pub(in crate::gui) fn switch_vibration_edition(&mut self, from_legacy: bool, to_legacy: bool) {
        if from_legacy == to_legacy {
            return;
        }
        let base = crate::config::AppConfig::vibration_path_for("Config.toml");
        let had_saved = self
            .config
            .switch_vibration_edition_to(&base, from_legacy, to_legacy);
        if had_saved {
            /* 目标路线有历史参数 → 全量同步到运行态 */
            self.app_state.apply_vibration_config(&self.config.vibration);
        } else {
            /* 目标路线首次使用 → 套该路线内置默认预设 (会写 config.vibration + 运行态) */
            let want = if to_legacy { "ACT1 特供" } else { "默认" };
            let name = crate::config::default_vibration_presets()
                .iter()
                .find(|p| p.name == want)
                .map(|p| p.name.clone())
                .unwrap_or_else(|| "默认".to_string());
            Self::apply_general_vibration_preset_in(
                &name,
                &self.app_state,
                &mut self.config,
                &mut self.vib_job_enabled,
                &mut self.vib_job_active,
                &mut self.vib_job_loaded,
            );
            self.app_state.apply_vibration_config(&self.config.vibration);
        }
        self.app_state
            .vib_legacy_client
            .store(to_legacy, std::sync::atomic::Ordering::Relaxed);
        /* ★v24.15: 职业预设的"已应用"会话状态按新路线重算 ——
         * 两条路线的职业名册互不相交, 所以切完路线后记着的那个职业必然不属于新路线:
         * 不清掉的话界面会继续显示"当前职业: X (已应用职业预设)", 而参数其实是新路线那套,
         * 且再动一次滑块就会把旧职业的快照当成新路线的参数写进 JobVibration.toml。
         * JobVibration.toml 本身不动 (切回原路线仍恢复)。 */
        let job_applies = crate::job_presets::load_job_vibration()
            .map(|j| {
                j.applied
                    && crate::job_presets::find_class_for_route(
                        &j.base_job,
                        &j.class_name,
                        to_legacy,
                    )
                    .is_some()
            })
            .unwrap_or(false);
        if !job_applies {
            self.vib_job_enabled = false;
            self.vib_job_active = None;
            self.vib_job_loaded = None;
        }
        let _ = self.config.save_vibration_to_file(&base);
        let _ = self.config.save_to_file("Config.toml");
        if let Err(e) = self.app_state.reload_config(self.config.clone()) {
            eprintln!("Failed to reload config after edition switch: {e}");
        }
    }

    pub(in crate::gui) fn disable_job_vibration_preset(&mut self) {
        Self::disable_job_vibration_preset_in(
            &self.app_state,
            &mut self.config,
            &mut self.vib_job_enabled,
            &mut self.vib_job_active,
            &mut self.vib_job_loaded,
        );
    }


    pub(in crate::gui) fn apply_job_vibration_preset(&mut self, base_job: &str, cls: &crate::job_presets::JobClass) {
        Self::apply_job_vibration_preset_in(
            base_job,
            cls,
            &self.app_state,
            &mut self.config,
            &mut self.vib_job_enabled,
            &mut self.vib_job_active,
            &mut self.vib_job_snapshot,
        );
    }

}
