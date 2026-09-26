//! 震动/职业参数导入导出 (JobVibration.toml 快照 + 剪贴板/文件导出) —— 原 vibration_page.rs 290-398, 2026-09-27 架构重构 C1 归位。

use crate::gui::SorahkGui;


impl SorahkGui {
    /// 保存当前职业参数快照到 JobVibration.toml (应用/滑块修改时调用)
    pub(in crate::gui) fn save_job_vibration_state(app_state: &crate::state::AppState, base_job: &str, class_name: &str) {
        use std::sync::atomic::Ordering;
        let params: Vec<u32> = (0..60)
            .map(|i| app_state.vibration_params[i].load(Ordering::Relaxed))
            .collect();
        crate::job_presets::save_job_vibration(&crate::job_presets::JobVibrationConfig {
            base_job: base_job.to_string(),
            class_name: class_name.to_string(),
            applied: true,
            params,
            rank_duration: app_state.vibration_rank_duration.load(Ordering::Relaxed),
            rank_level_gain: app_state.vibration_rank_level_gain.load(Ordering::Relaxed),
        });
    }


    /// 导出通用震动设定 (当前生效参数 + 评分 + 高级算法) 为 TOML 字符串
    pub(in crate::gui) fn export_vibration_settings(app_state: &crate::state::AppState, config: &crate::config::AppConfig) -> String {
        use std::fmt::Write;
        use std::sync::atomic::Ordering;
        let mut s = String::new();
        s.push_str("# ═══════════════════════════════════════════════════════\n");
        s.push_str("#  Sorahk 通用震动设定导出\n");
        s.push_str("# ═══════════════════════════════════════════════════════\n");
        /* ★v24.14: 注明路线与对应设定文件 (文件名也带同样的标记) */
        let route = if config.vib_legacy_client {
            crate::config::VIBRATION_FILE_ACT
        } else {
            crate::config::VIBRATION_FILE
        };
        s.push_str(&format!(
            "#  客户端路线: {}\n#  对应设定文件: {}\n\n",
            if config.vib_legacy_client { "S1 ACT (老方案)" } else { "S4+ 新版 (现行方案)" },
            route,
        ));
        s.push_str(&format!("independent_test = {}\n", config.vibration.independent_test));
        s.push_str(&format!("move_independent = {}\n", config.vibration.move_independent));
        s.push_str(&format!("out_smooth = {}\n", config.vibration.out_smooth));
        s.push_str(&format!("throttle_window_ms = {}\n", config.vibration.throttle_window_ms));
        s.push_str(&format!("throttle_max_hits = {}\n", config.vibration.throttle_max_hits));
        s.push_str(&format!("throttle_dense_ratio = {}\n", config.vibration.throttle_dense_ratio));
        s.push_str(&format!("random_gain = {}\n", config.vibration.random_gain));
        s.push_str(&format!("motor_l_gain = {}\n", config.vibration.motor_l_gain));
        s.push_str(&format!("motor_r_gain = {}\n", config.vibration.motor_r_gain));
        s.push_str(&format!("rank_level_gain = {}\n", app_state.vibration_rank_level_gain.load(Ordering::Relaxed)));
        s.push_str(&format!("rank_duration = {}\n", app_state.vibration_rank_duration.load(Ordering::Relaxed)));
        let _ = writeln!(s, "\nparams = [{}]", (0..60)
            .map(|i| app_state.vibration_params[i].load(Ordering::Relaxed).to_string())
            .collect::<Vec<_>>().join(", "));
        let _ = writeln!(s, "item_lr = [{}]", (0..26)
            .map(|i| app_state.vibration_item_lr[i].load(Ordering::Relaxed).to_string())
            .collect::<Vec<_>>().join(", "));
        let _ = writeln!(s, "rank_lr = [{}]", (0..30)
            .map(|i| app_state.vibration_rank_lr[i].load(Ordering::Relaxed).to_string())
            .collect::<Vec<_>>().join(", "));
        let _ = writeln!(s, "rank_type_gain = [{}]", (0..15)
            .map(|i| app_state.vibration_rank_type_gain[i].load(Ordering::Relaxed).to_string())
            .collect::<Vec<_>>().join(", "));
        s
    }


    /// 导出当前职业完整设置 (全职业预设页) 为 TOML 字符串
    pub(in crate::gui) fn export_job_settings(base: &str, cls: &crate::job_presets::JobClass) -> String {
        use std::fmt::Write;
        let mut s = String::new();
        s.push_str("# ═══════════════════════════════════════════════════════\n");
        s.push_str("#  Sorahk 全职业预设导出\n");
        s.push_str("# ═══════════════════════════════════════════════════════\n");
        s.push_str(&format!(
            "#  客户端路线: {}\n\n",
            if cls.name.ends_with("-ACT") { "S1 ACT (老方案)" } else { "S4+ 新版 (现行方案)" },
        ));
        let _ = writeln!(s, "base_job = \"{}\"", base);
        let _ = writeln!(s, "class_name = \"{}\"", cls.name);
        let _ = writeln!(s, "out_smooth = {}", cls.out_smooth);
        let _ = writeln!(s, "throttle_window = {}", cls.throttle_window);
        let _ = writeln!(s, "throttle_max = {}", cls.throttle_max);
        let _ = writeln!(s, "throttle_dense_ratio = {}", cls.throttle_dense_ratio);
        let _ = writeln!(s, "rank_level_gain = {}", cls.rank_level_gain);
        let _ = writeln!(s, "rank_duration = {}", cls.rank_duration);
        let _ = writeln!(s, "desc = \"{}\"", cls.desc);
        let _ = writeln!(s, "\nparams = [{}]", cls.params.iter()
            .map(|v| v.to_string()).collect::<Vec<_>>().join(", "));
        let _ = writeln!(s, "rank_lr = [{}]", cls.rank_lr.iter()
            .map(|v| v.to_string()).collect::<Vec<_>>().join(", "));
        let _ = writeln!(s, "rank_type_gain = [{}]", cls.rank_type_gain.iter()
            .map(|v| v.to_string()).collect::<Vec<_>>().join(", "));
        s
    }


    /// 把导出内容写入文件 (程序目录), 返回文件名
    pub(in crate::gui) fn save_export_file(content: &str, prefix: &str) -> String {
        use std::io::Write;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let fname = format!("{}_{}.toml", prefix, now);
        if let Ok(mut f) = std::fs::File::create(&fname) {
            let _ = f.write_all(content.as_bytes());
            let _ = f.flush();
        }
        fname
    }


}
