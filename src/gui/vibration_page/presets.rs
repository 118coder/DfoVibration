//! 预设应用/职业启用/版本切换 —— 原 vibration_page.rs 3235-3576, 2026-09-27 架构重构 C1 归位。
//!
//! ★R2.14: config 写入与引擎原子镜像全部下沉 backend (apply_vibration_preset_by_name /
//! apply_job_vibration_class / disable_job_vibration / switch_vib_edition_explicit);
//! 本文件只保留 GUI 侧的会话字段 (vib_job_*) 编排与 JobVibration.toml 落盘。

use crate::gui::SorahkGui;


impl SorahkGui {
    /// 应用通用震动预设 (★R2.14 后端化)。
    /// 互斥: 若全职业预设开启, 先关闭并恢复默认 (旧 *_in 首段语义)。
    pub(in crate::gui) fn apply_general_vibration_preset(&mut self, name: &str) {
        if self.vib_job_enabled {
            self.disable_job_vibration_preset();
        }
        match self.backend.apply_vibration_preset_by_name(name) {
            Ok(true) => {
                /* 与旧 *_in 一致: 通用预设应用后职业会话不残留 */
                self.vib_job_enabled = false;
                self.vib_job_active = None;
                self.vib_job_loaded = None;
            }
            Ok(false) => {} /* 找不到该预设, 旧语义直接返回 */
            Err(e) => eprintln!("Failed to apply vibration preset: {e}"),
        }
    }


    /// 关闭全职业预设 (★R2.14 后端化): 引擎/config 复位走 backend,
    /// JobVibration.toml applied=false 与 GUI 会话字段在这里收口。
    pub(in crate::gui) fn disable_job_vibration_preset(&mut self) {
        self.backend.disable_job_vibration();
        crate::job_presets::save_job_vibration(&crate::job_presets::JobVibrationConfig {
            base_job: String::new(),
            class_name: String::new(),
            applied: false,
            params: Vec::new(),
            rank_duration: 0,
            rank_level_gain: 0,
        });
        self.vib_job_enabled = false;
        self.vib_job_active = None;
        self.vib_job_loaded = None;
    }


    /// 应用全职业预设 (★R2.14 后端化): config/原子走 backend,
    /// JobVibration.toml 快照 + GUI 会话字段在这里收口。
    pub(in crate::gui) fn apply_job_vibration_preset(&mut self, base_job: &str, cls: &crate::job_presets::JobClass) {
        self.backend.apply_job_vibration_class(base_job, cls);
        self.vib_job_snapshot = cls.params.to_vec();
        self.save_job_vibration_state(base_job, cls.name);
        self.vib_job_enabled = true;
        self.vib_job_active = Some((base_job.to_string(), cls.name.to_string()));
        /* ★v24.46 诊断: 职业预设应用落盘一行关键槽位 —— 排查"S4 全职业预设
         * 手感与 1.61 不一致"时, 先核对这行与 1.61 同职业的数值是否一致。 */
        let legacy = self.backend.with_config(|c| c.vib_legacy_client);
        crate::backend::BackendService::log(&format!(
            "[JOB] apply {} / {} | leg={} atk={} max={} sp={} hit={} smooth={} \
             throttle={}/{} abs={} algo={}",
            base_job,
            cls.name,
            legacy as u8,
            cls.params[0],
            cls.params[4],
            cls.params[13],
            cls.params[16],
            cls.out_smooth,
            cls.throttle_window,
            cls.throttle_max,
            (if legacy { false } else { cls.abs_freq_enabled }) as u8,
            cls.algo_id
        ));
    }


    /// 便捷包装 (极简模式等无借用冲突的调用点)。
    /// ★ACT1 特供预设: S1 路线时确保出现在预设列表「默认」之下 (缺失插入 /
    /// 错位搬移, 见 config::ensure_act1_preset_position); S4/非 DFO 由各列表处
    /// 的过滤器隐藏。搬移/插入发生变更时立即落盘 —— 自愈旧版 push 到末尾
    /// 且已持久化进 Vibration.toml 的错位残留 (bug: ACT1 特供在列表末尾)。
    pub(in crate::gui) fn ensure_act1_preset_listed(&mut self) {
        let mut changed = false;
        let _ = self.backend.update_config(crate::ui_api::Commit::Memory, |c| {
            changed = crate::config::ensure_act1_preset_position(
                &mut c.vibration_presets,
                c.vib_legacy_client,
            );
            /* ★v19: 通用预设的 ACT 变体 (高振幅-ACT 等) 同样按需插入/自愈 */
            changed |= crate::config::ensure_act_general_variants(
                &mut c.vibration_presets,
                c.vib_legacy_client,
            );
        });
        if changed {
            let _ = self
                .backend
                .update_config(crate::ui_api::Commit::SaveVib, |_| {});
        }
    }


    /// ★v24.14: 切换客户端路线 (S1/S4) —— 两条路线各自**独立文件**
    /// (S1 → `Vibration-ACT.toml`; S4+ → `Vibration.toml`), 参数与预设列表互不干扰。
    ///
    /// ★R2.14: 文件互换/镜像/落盘走 backend.switch_vib_edition_explicit;
    /// 这里只做职业预设"已应用"会话状态的重算 (旧 switch 最后一段)。
    ///
    /// `from_legacy` = 旧路线 (调用方须在批量赋值**前**取旧值, 设置弹窗流程在
    /// 调用前已把 config 路线标记写成新值), `to_legacy` = 目标路线。
    /// 目标路线首次使用 (无历史文件) 时 backend 自动套用该路线内置默认预设:
    /// S1 → 「ACT1 特供」; S4 → 内置原版第一项「默认」。
    pub(in crate::gui) fn switch_vibration_edition(&mut self, from_legacy: bool, to_legacy: bool) {
        if from_legacy == to_legacy {
            return;
        }
        if let Err(e) = self
            .backend
            .switch_vib_edition_explicit(from_legacy, to_legacy)
        {
            eprintln!("Failed to switch vibration edition: {e}");
        }
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
    }

}
