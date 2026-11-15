//! 震动/职业参数导入导出 (JobVibration.toml 快照 + 剪贴板/文件导出) —— 原 vibration_page.rs 290-398, 2026-09-27 架构重构 C1 归位。
//!
//! ★R2.14: 数据来源改为 backend (引擎镜像走快照, config 字段走 with_config 短借)。

use crate::gui::SorahkGui;


impl SorahkGui {
    /// 保存当前职业参数快照到 JobVibration.toml (应用/滑块修改时调用)
    ///
    /// ★R2.14: 引擎读数 (60 槽/等级/时长) 取自 backend 运行态快照;
    /// JobVibration.toml 的读写编排仍归 GUI (职业会话状态自管)。
    pub(in crate::gui) fn save_job_vibration_state(&self, base_job: &str, class_name: &str) {
        let snap = self.backend.snapshot();
        crate::job_presets::save_job_vibration(&crate::job_presets::JobVibrationConfig {
            base_job: base_job.to_string(),
            class_name: class_name.to_string(),
            applied: true,
            params: snap.vibration.params.clone(),
            rank_duration: snap.vibration.rank_duration,
            rank_level_gain: snap.vibration.rank_level_gain,
        });
    }


    /// 导出通用震动设定 (当前生效参数 + 评分 + 高级算法) 为 TOML 字符串
    ///
    /// ★R2.14: 数组取自运行态快照 (引擎镜像), 标量取自 config (权威)。
    /// 显式传 `&Backend` (闭包内避免整份 self 捕获)。
    pub(in crate::gui) fn export_vibration_settings(backend: &crate::backend::Backend) -> String {
        use std::fmt::Write;
        let snap = backend.snapshot();
        let (vib, legacy) = backend.with_config(|c| (c.vibration.clone(), c.vib_legacy_client));
        let mut s = String::new();
        s.push_str("# ═══════════════════════════════════════════════════════\n");
        s.push_str("#  Sorahk 通用震动设定导出\n");
        s.push_str("# ═══════════════════════════════════════════════════════\n");
        /* ★v24.14: 注明路线与对应设定文件 (文件名也带同样的标记) */
        let route = if legacy {
            crate::config::VIBRATION_FILE_ACT
        } else {
            crate::config::VIBRATION_FILE
        };
        s.push_str(&format!(
            "#  客户端路线: {}\n#  对应设定文件: {}\n\n",
            if legacy { "S1 ACT (老方案)" } else { "S4+ 新版 (现行方案)" },
            route,
        ));
        s.push_str(&format!("independent_test = {}\n", vib.independent_test));
        s.push_str(&format!("move_independent = {}\n", vib.move_independent));
        s.push_str(&format!("out_smooth = {}\n", vib.out_smooth));
        s.push_str(&format!("throttle_window_ms = {}\n", vib.throttle_window_ms));
        s.push_str(&format!("throttle_max_hits = {}\n", vib.throttle_max_hits));
        s.push_str(&format!("throttle_dense_ratio = {}\n", vib.throttle_dense_ratio));
        s.push_str(&format!("random_gain = {}\n", vib.random_gain));
        s.push_str(&format!("motor_l_gain = {}\n", vib.motor_l_gain));
        s.push_str(&format!("motor_r_gain = {}\n", vib.motor_r_gain));
        s.push_str(&format!("rank_level_gain = {}\n", snap.vibration.rank_level_gain));
        s.push_str(&format!("rank_duration = {}\n", snap.vibration.rank_duration));
        let _ = writeln!(s, "\nparams = [{}]", snap.vibration.params.iter()
            .map(|v| v.to_string()).collect::<Vec<_>>().join(", "));
        let _ = writeln!(s, "item_lr = [{}]", vib.item_lr.iter()
            .map(|v| v.to_string()).collect::<Vec<_>>().join(", "));
        let _ = writeln!(s, "rank_lr = [{}]", vib.rank_lr.iter()
            .map(|v| v.to_string()).collect::<Vec<_>>().join(", "));
        let _ = writeln!(s, "rank_type_gain = [{}]", vib.rank_type_gain.iter()
            .map(|v| v.to_string()).collect::<Vec<_>>().join(", "));
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
