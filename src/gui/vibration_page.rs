//! Main window implementation and rendering logic.

use crate::gui::SorahkGui;
use crate::gui::about_dialog::render_about_dialog;
use crate::gui::theme::{self, Theme, truncate_chars};
use crate::gui::widgets;
use crate::gui::types::{KeyCaptureMode, Page};
use crate::state::NotificationEvent;

use eframe::egui;
use super::main_window::FrameState;

impl SorahkGui {
    /// 将当前参数写入 config.vibration (索引映射, 与预设 params 顺序一致)
    pub(super) fn sync_vib_config_from_params(
        cfg: &mut crate::config::VibrationConfig,
        p: &[std::sync::atomic::AtomicU32; 60],
    ) {
        use std::sync::atomic::Ordering;
        let g = cfg;
        g.attack_gain = p[0].load(Ordering::Relaxed);
        g.damage_gain = p[1].load(Ordering::Relaxed);
        g.shake_gain = p[2].load(Ordering::Relaxed);
        g.move_gain = p[3].load(Ordering::Relaxed);
        g.max_strength = p[4].load(Ordering::Relaxed);
        g.decay_ms = p[5].load(Ordering::Relaxed);
        g.hit_boost = p[6].load(Ordering::Relaxed);
        g.start_pulse = p[7].load(Ordering::Relaxed);
        g.kill_pulse = p[8].load(Ordering::Relaxed);
        g.master_gain = p[9].load(Ordering::Relaxed);
        g.font_strength = p[10].load(Ordering::Relaxed);
        g.font_interval = p[11].load(Ordering::Relaxed);
        g.font_hp = p[12].load(Ordering::Relaxed);
        g.font_special = p[13].load(Ordering::Relaxed);
        g.font_state = p[14].load(Ordering::Relaxed);
        g.font_effect = p[15].load(Ordering::Relaxed);
        g.font_attack = p[16].load(Ordering::Relaxed);
        g.font_hit = p[17].load(Ordering::Relaxed);
        g.rhythm = p[18].load(Ordering::Relaxed);
        for i in 19..60 {
            g.advanced[i - 19] = p[i].load(Ordering::Relaxed);
        }
    }


    /// 渲染某基础项的 L/R 马达权重滑块 + 实时输出条
    /// 权重范围 -100..+100: 0 = 保持原样(不动原参数), 负 = 减弱, 正 = 增强
    /// item: 0=总闸 1=攻击 2=上限 3=衰减 4=连击 5=节奏 6=通用 7=DOT 8=特效 9=状态 10=特殊 11=命中 12=受击
    pub(super) fn render_item_lr(
        ui: &mut egui::Ui,
        dark: bool,
        state: &crate::state::AppState,
        config_item_lr: &mut [u32; 26],
        item: usize,
    ) {
        use std::sync::atomic::Ordering;
        let il = state.vibration_item_lr[item * 2].load(Ordering::Relaxed) as i32 as f32;
        let ir = state.vibration_item_lr[item * 2 + 1].load(Ordering::Relaxed) as i32 as f32;
        let ol = state.vibration_item_out[item * 2].load(Ordering::Relaxed);
        let or_ = state.vibration_item_out[item * 2 + 1].load(Ordering::Relaxed);
        let mut l = il;
        let mut r = ir;

        ui.vertical(|ui| {
            /* L = 橙色, R = 红色 (清晰区分), 每行一个马达, 宽度自适应 */
            let th = Theme::new(dark);
            let l_orange = th.motor_l;
            let r_red = th.motor_r;
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("L").size(13.0).strong().color(l_orange));
                ui.add(
                    egui::Slider::new(&mut l, -100.0..=100.0)
                        .show_value(false)
                        .trailing_fill(true)
                        .text_color(l_orange),
                );
                ui.add(
                    egui::ProgressBar::new(ol as f32 / 65535.0)
                        .desired_width(40.0)
                        .fill(l_orange),
                );
                ui.label(
                    egui::RichText::new(format!("L{:+}", l as i32))
                        .size(11.0)
                        .weak(),
                );
            });
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("R").size(13.0).strong().color(r_red));
                ui.add(
                    egui::Slider::new(&mut r, -100.0..=100.0)
                        .show_value(false)
                        .trailing_fill(true)
                        .text_color(r_red),
                );
                ui.add(
                    egui::ProgressBar::new(or_ as f32 / 65535.0)
                        .desired_width(40.0)
                        .fill(r_red),
                );
                ui.label(
                    egui::RichText::new(format!("R{:+}", r as i32))
                        .size(11.0)
                        .weak(),
                );
            });
        });
        if (l as i32) != (il as i32) {
            state.vibration_item_lr[item * 2].store((l as i32) as u32, Ordering::Relaxed);
            config_item_lr[item * 2] = l as u32;
        }
        if (r as i32) != (ir as i32) {
            state.vibration_item_lr[item * 2 + 1].store((r as i32) as u32, Ordering::Relaxed);
            config_item_lr[item * 2 + 1] = r as u32;
        }
    }


    /// 渲染评分事件的 L/R 马达权重滑块 (rank_lr, 30 项 = 15 事件 x L/R)
    pub(super) fn render_rank_lr(
        ui: &mut egui::Ui,
        dark: bool,
        state: &crate::state::AppState,
        config_rank_lr: &mut [u32; 30],
        item: usize,
    ) {
        use std::sync::atomic::Ordering;
        let il = state.vibration_rank_lr[item * 2].load(Ordering::Relaxed) as i32 as f32;
        let ir = state.vibration_rank_lr[item * 2 + 1].load(Ordering::Relaxed) as i32 as f32;
        let ol = state.vibration_rank_out[item * 2].load(Ordering::Relaxed);
        let or_ = state.vibration_rank_out[item * 2 + 1].load(Ordering::Relaxed);
        let mut l = il;
        let mut r = ir;

        ui.vertical(|ui| {
            let th = Theme::new(dark);
            let l_orange = th.motor_l;
            let r_red = th.motor_r;
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("L").size(13.0).strong().color(l_orange));
                ui.add(
                    egui::Slider::new(&mut l, -100.0..=100.0)
                        .show_value(false)
                        .trailing_fill(true)
                        .text_color(l_orange),
                );
                ui.add(
                    egui::ProgressBar::new(ol as f32 / 65535.0)
                        .desired_width(40.0)
                        .fill(l_orange),
                );
                ui.label(
                    egui::RichText::new(format!("L{:+}", l as i32))
                        .size(11.0)
                        .weak(),
                );
            });
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("R").size(13.0).strong().color(r_red));
                ui.add(
                    egui::Slider::new(&mut r, -100.0..=100.0)
                        .show_value(false)
                        .trailing_fill(true)
                        .text_color(r_red),
                );
                ui.add(
                    egui::ProgressBar::new(or_ as f32 / 65535.0)
                        .desired_width(40.0)
                        .fill(r_red),
                );
                ui.label(
                    egui::RichText::new(format!("R{:+}", r as i32))
                        .size(11.0)
                        .weak(),
                );
            });
        });
        if (l as i32) != (il as i32) {
            state.vibration_rank_lr[item * 2].store((l as i32) as u32, Ordering::Relaxed);
            config_rank_lr[item * 2] = l as u32;
        }
        if (r as i32) != (ir as i32) {
            state.vibration_rank_lr[item * 2 + 1].store((r as i32) as u32, Ordering::Relaxed);
            config_rank_lr[item * 2 + 1] = r as u32;
        }
    }


    /// 保存当前职业参数快照到 JobVibration.toml (应用/滑块修改时调用)
    pub(super) fn save_job_vibration_state(app_state: &crate::state::AppState, base_job: &str, class_name: &str) {
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
    pub(super) fn export_vibration_settings(app_state: &crate::state::AppState, config: &crate::config::AppConfig) -> String {
        use std::fmt::Write;
        use std::sync::atomic::Ordering;
        let mut s = String::new();
        s.push_str("# ═══════════════════════════════════════════════════════\n");
        s.push_str("#  Sorahk 通用震动设定导出\n");
        s.push_str("# ═══════════════════════════════════════════════════════\n\n");
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
    pub(super) fn export_job_settings(base: &str, cls: &crate::job_presets::JobClass) -> String {
        use std::fmt::Write;
        let mut s = String::new();
        s.push_str("# ═══════════════════════════════════════════════════════\n");
        s.push_str("#  Sorahk 全职业预设导出\n");
        s.push_str("# ═══════════════════════════════════════════════════════\n\n");
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
    pub(super) fn save_export_file(content: &str, prefix: &str) -> String {
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


    /// 【调试模式】独立测试开关 (v24.2)。
    ///
    /// 开 = 评分/移动通道独立于全局总调整 (单通道调试用, 方便逐项验证);
    /// 关 = 全局总调整统一应用到所有通道 (正常使用)。
    /// 不影响各卡片自己的测试按钮与滑块。状态持久化到 Config.toml。
    pub(super) fn render_independent_test_toggle(
        ui: &mut egui::Ui,
        app_state: &crate::state::AppState,
        config: &mut crate::config::AppConfig,
    ) {
        use std::sync::atomic::Ordering;
        let dark = config.dark_mode;
        let th = Theme::new(dark);
        let mut ind = app_state.vibration_independent_test.load(Ordering::Relaxed);

        th.panel(ui, Some("🧪 调试模式 (独立测试)"), |ui| {
            ui.horizontal(|ui| {
                if ui
                    .checkbox(
                        &mut ind,
                        "评分/移动通道独立于全局总调整",
                    )
                    .on_hover_text(
                        "开启后: 全局总调整不会影响评分震动与移动持续震动 (单通道调试方便)。\n\
                         关闭后: 全局总调整统一应用到所有通道 (正常使用)。\n\
                         不影响各卡片自己的测试按钮与滑块。",
                    )
                    .changed()
                {
                    app_state
                        .vibration_independent_test
                        .store(ind, Ordering::Relaxed);
                    config.vibration.independent_test = ind;
                    let _ = config.save_vibration_to_file(crate::config::AppConfig::vibration_path_for("Config.toml"));
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let (text, fg, bg) = if ind {
                        ("调试中", th.warn, th.warn_soft)
                    } else {
                        ("正常模式", th.good, th.good_soft)
                    };
                    th.badge(ui, text, fg, bg);
                });
            });
            ui.add_space(4.0);
            ui.label(th.hint_text(if ind {
                "调试模式已开启: 评分/移动通道不吃全局总调整, 便于逐项单独验证。"
            } else {
                "正常模式: 全局总调整统一作用于所有通道。需要单通道调试时再打开。"
            }));
        });
    }


    /// 震动分区卡片: 与主窗口其他卡片视觉统一 (容器样式见 Theme::panel)。
    pub(super) fn vib_section(
        ui: &mut egui::Ui,
        dark: bool,
        title: &str,
        body: impl FnOnce(&mut egui::Ui),
    ) {
        Theme::new(dark).panel(ui, Some(title), body);
        ui.add_space(10.0);
    }


    /// Full vibration settings page (all params + status + test).
    pub(super) fn render_vibration_full_page(&mut self, ui: &mut egui::Ui, _frame_state: &FrameState, job_mode: bool) {
        use std::sync::atomic::Ordering;
        let th = self.theme();
        let title_color = th.heading;

        /* 放大字号与控件: 震动页全局风格; 滑块宽度随窗口自适应, 宽屏不再挤在左侧 */
        ui.style_mut().text_styles.insert(
            egui::TextStyle::Body,
            egui::FontId::new(14.0, egui::FontFamily::Proportional),
        );
        ui.style_mut().text_styles.insert(
            egui::TextStyle::Button,
            egui::FontId::new(14.0, egui::FontFamily::Proportional),
        );
        ui.style_mut().spacing.slider_width = ((ui.available_width() - 240.0) * 0.5).clamp(200.0, 340.0);
        ui.style_mut().spacing.interact_size.y = 26.0;

        /* 初始化参数(仅一次): 仅"全新安装"(master/attack 全 0 = 从未应用过
         * 任何预设)才应用内置"默认"预设。已有用户配置时 params 已由
         * AppState::new 从 config.vibration 恢复 —— 旧实现无条件覆盖, 会把
         * 用户"保存当前"落盘的参数在首渲染时替换回内置默认, 与持久化语义矛盾。 */
        {
            use std::sync::atomic::AtomicBool;
            static INIT_DONE: AtomicBool = AtomicBool::new(false);
            if !INIT_DONE.load(Ordering::Relaxed) {
                INIT_DONE.store(true, Ordering::Relaxed);
                let p = &self.app_state.vibration_params;
                let pristine = self.config.vibration.master_gain == 0
                    && self.config.vibration.attack_gain == 0;
                if pristine {
                if let Some(pr) = crate::config::default_vibration_presets().first() {
                    for (i, v) in pr.params.iter().enumerate() {
                        p[i].store(*v, Ordering::Relaxed);
                    }
                    /* 同步 L/R 权重 + 评分参数 (UI 显示与内置默认一致) */
                    for (i, v) in pr.item_lr.iter().enumerate() {
                        self.app_state.vibration_item_lr[i].store(*v, Ordering::Relaxed);
                        self.config.vibration.item_lr[i] = *v;
                    }
                    for (i, v) in pr.rank_lr.iter().enumerate() {
                        self.app_state.vibration_rank_lr[i].store(*v, Ordering::Relaxed);
                        self.config.vibration.rank_lr[i] = *v;
                    }
                    for (i, v) in pr.rank_type_gain.iter().enumerate() {
                        self.app_state.vibration_rank_type_gain[i].store(*v, Ordering::Relaxed);
                        self.config.vibration.rank_type_gain[i] = *v;
                    }
                    self.app_state.vibration_rank_level_gain.store(pr.rank_level_gain, Ordering::Relaxed);
                    self.app_state.vibration_rank_duration.store(pr.rank_duration, Ordering::Relaxed);
                    self.config.vibration.rank_level_gain = pr.rank_level_gain;
                    self.config.vibration.rank_duration = pr.rank_duration;
                        self.app_state.vibration_out_smooth.store(pr.out_smooth, Ordering::Relaxed);
                        self.config.vibration.out_smooth = pr.out_smooth;
                }
                }
                /* 全职业预设恢复 (v21): applied=true 时覆盖默认预设; 否则绝对频率强制关闭 */
                if let Some(jcfg) = crate::job_presets::load_job_vibration() {
                    self.vib_job_enabled = jcfg.applied;
                    if !jcfg.applied {
                        self.app_state.vibration_abs_freq_enabled.store(false, Ordering::Relaxed);
                        self.config.vibration.abs_freq_enabled = false;
                    }
                    if jcfg.applied {                        if let Some((bi, ci, cls)) = crate::job_presets::find_class(&jcfg.base_job, &jcfg.class_name) {
                            self.vib_job_base = bi;
                            self.vib_job_class = ci;
                            let params: Vec<u32> = if jcfg.params.len() == 60 {
                                jcfg.params.clone()
                            } else {
                                cls.params.to_vec()
                            };
                            self.vib_job_snapshot = params.clone();
                            for (i, v) in params.iter().enumerate() {
                                p[i].store(*v, Ordering::Relaxed);
                            }
                            for (i, v) in cls.rank_lr.iter().enumerate() {
                                self.app_state.vibration_rank_lr[i].store(*v as u32, Ordering::Relaxed);
                            }
                            for (i, v) in cls.rank_type_gain.iter().enumerate() {
                                self.app_state.vibration_rank_type_gain[i].store(*v, Ordering::Relaxed);
                            }
                            let dur = if jcfg.rank_duration > 0 { jcfg.rank_duration } else { cls.rank_duration };
                            let lg = if jcfg.rank_level_gain > 0 { jcfg.rank_level_gain } else { cls.rank_level_gain };
                            self.app_state.vibration_rank_duration.store(dur, Ordering::Relaxed);
                            self.app_state.vibration_rank_level_gain.store(lg, Ordering::Relaxed);
                            self.app_state.vibration_out_smooth.store(cls.out_smooth, Ordering::Relaxed);
                            self.config.vibration.out_smooth = cls.out_smooth;
                            self.app_state.vibration_throttle_window.store(cls.throttle_window, Ordering::Relaxed);
                            self.app_state.vibration_throttle_max.store(cls.throttle_max, Ordering::Relaxed);
                            self.app_state.vibration_throttle_dense_ratio.store(cls.throttle_dense_ratio, Ordering::Relaxed);
                            self.app_state.vibration_abs_freq_enabled.store(cls.abs_freq_enabled, Ordering::Relaxed);
                            self.app_state.vibration_algo_id.store(cls.algo_id as u32, Ordering::Relaxed);
                            for (i, v) in cls.algo_params.iter().enumerate() {
                                self.app_state.vibration_algo_ap[i].store(*v, Ordering::Relaxed);
                            }
                            self.config.vibration.throttle_window_ms = cls.throttle_window;
                            self.config.vibration.throttle_max_hits = cls.throttle_max;
                            self.config.vibration.throttle_dense_ratio = cls.throttle_dense_ratio;
                            self.config.vibration.abs_freq_enabled = cls.abs_freq_enabled;
                            self.vib_job_active = Some((jcfg.base_job.clone(), jcfg.class_name.clone()));
                            self.vib_job_loaded = Some((bi, ci));
                        }
                    }
                }
            }
        }

        egui::Frame::NONE
            .inner_margin(egui::Margin::same(6))
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                let p = &self.app_state.vibration_params;

                /* 标题行 */
                Self::vib_section(ui, self.dark_mode, "震动控制中心", |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("通用型震动设定 (DFO 战斗事件 → 手柄马达)")
                            .size(17.0)
                            .strong()
                            .color(title_color),
                    );
                    ui.add_space(20.0);
                    let vib_on = self
                        .app_state
                        .vibration_enabled
                        .load(Ordering::Relaxed);
                    let (vtext, vcolor) = if vib_on {
                        ("震动: 开", th.good)
                    } else {
                        ("震动: 关", th.bad)
                    };
                    if ui
                        .button(egui::RichText::new(vtext).size(14.0).color(vcolor))
                        .clicked()
                    {
                        let v = self
                            .app_state
                            .vibration_enabled
                            .load(Ordering::Relaxed);
                        self.app_state
                            .vibration_enabled
                            .store(!v, Ordering::Relaxed);
                    }
                    ui.add_space(12.0);
                    if ui
                        .button(egui::RichText::new("测试震动 1 秒").size(13.0))
                        .clicked()
                    {
                        let until = crate::vibration::now_ms_u64() + 1000;
                        self.app_state
                            .vibration_test_until
                            .store(until, Ordering::Relaxed);
                    }
                });
                ui.add_space(6.0);

                /* 独立测试开关 (v24.2): 控制中心主开关 */
                Self::render_independent_test_toggle(ui, &self.app_state, &mut self.config);
                ui.add_space(6.0);

                /* 全职业预设页: 应用开关 + 职业选择 (替换震动设置预设行) */
                if job_mode {
                    let jobs = crate::job_presets::builtin_jobs();
                    let base_sel = self.vib_job_base.min(jobs.len().saturating_sub(1));
                    let classes = &jobs[base_sel].classes;
                    let class_sel = self.vib_job_class.min(classes.len().saturating_sub(1));
                    let cur_cls = &classes[class_sel];
                    /* 应用开关 */
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("全职业预设:").size(13.0).strong());
                        let mut en = self.vib_job_enabled;
                        if ui
                            .checkbox(
                                &mut en,
                                "应用全职业预设 (开: 职业参数覆盖震动设置; 关: 不应用, 恢复震动设置默认)",
                            )
                            .changed()
                        {
                            self.vib_job_enabled = en;
                            if en {
                                Self::apply_job_vibration_preset_in(
                                    jobs[base_sel].base_job,
                                    cur_cls,
                                    &self.app_state,
                                    &mut self.config,
                                    &mut self.vib_job_enabled,
                                    &mut self.vib_job_active,
                                    &mut self.vib_job_snapshot,
                                );
                                self.vib_job_loaded = Some((base_sel, class_sel));
                            } else {
                                Self::disable_job_vibration_preset_in(
                                    &self.app_state,
                                    &mut self.config,
                                    &mut self.vib_job_enabled,
                                    &mut self.vib_job_active,
                                    &mut self.vib_job_loaded,
                                );
                            }
                        }
                    });
                    ui.add_space(4.0);
                    /* 职业选择行 */
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("基础职业:").size(13.0).strong());
                        let base_names: Vec<String> = jobs.iter().map(|j| j.base_job.to_string()).collect();
                        egui::ComboBox::from_id_salt("vib_job_base")
                            .selected_text(base_names[base_sel].clone())
                            .width(140.0)
                            .show_ui(ui, |ui| {
                                for (i, n) in base_names.iter().enumerate() {
                                    if ui.selectable_label(i == base_sel, n).clicked() {
                                        self.vib_job_base = i;
                                        self.vib_job_class = 0;
                                    }
                                }
                            });
                        ui.add_space(8.0);
                        ui.label(egui::RichText::new("转职:").size(13.0).strong());
                        let class_names: Vec<String> = classes.iter().map(|c| c.name.to_string()).collect();
                        egui::ComboBox::from_id_salt("vib_job_class")
                            .selected_text(class_names[class_sel].clone())
                            .width(140.0)
                            .show_ui(ui, |ui| {
                                for (i, n) in class_names.iter().enumerate() {
                                    if ui.selectable_label(i == class_sel, n).clicked() {
                                        self.vib_job_class = i;
                                    }
                                }
                            });
                        ui.add_space(8.0);
                        if ui
                            .button(egui::RichText::new("应用该职业").size(13.0))
                            .clicked()
                        {
                            Self::apply_job_vibration_preset_in(
                                jobs[base_sel].base_job,
                                cur_cls,
                                &self.app_state,
                                &mut self.config,
                                &mut self.vib_job_enabled,
                                &mut self.vib_job_active,
                                &mut self.vib_job_snapshot,
                            );
                            self.vib_job_loaded = Some((base_sel, class_sel));
                        }
                        ui.add_space(8.0);
                        if ui
                            .button(egui::RichText::new("导出当前职业").size(13.0))
                            .on_hover_text("导出当前选择职业的完整设置 (仅当前职业)")
                            .clicked()
                        {
                            let content = Self::export_job_settings(jobs[base_sel].base_job, cur_cls);
                            let fname = Self::save_export_file(&content, &format!("job_export_{}", cur_cls.name));
                            self.vib_export_msg = Some(fname);
                        }
                        ui.add_space(8.0);
                        /* 导入职业设置 (v33.1: 按规范 base_job/class_name 匹配内置职业) */
                        if ui
                            .button(egui::RichText::new("导入职业设置").size(13.0))
                            .on_hover_text("从 job_export_*.toml 导入 (按 base_job/class_name 匹配内置职业后应用)")
                            .clicked()
                        {
                            let files = crate::job_presets::list_export_files("job_export");
                            let sel = self.job_import_sel.min(files.len().saturating_sub(1));
                            if let Some(f) = files.get(sel) {
                                match crate::job_presets::parse_job_export(f) {
                                    Some(ij) => {
                                        /* 规范: 按 base_job/class_name 匹配内置职业 */
                                        if let Some((bi, ci, _cls)) = crate::job_presets::find_class(&ij.base_job, &ij.class_name) {
                                            self.vib_job_base = bi;
                                            self.vib_job_class = ci;
                                            /* 导入参数覆盖 (只写 AppState; 快照检测自动落盘 JobVibration.toml) */
                                            for (i, v) in ij.params.iter().enumerate() {
                                                self.app_state.vibration_params[i].store(*v, Ordering::Relaxed);
                                            }
                                            for (i, v) in ij.rank_lr.iter().enumerate() {
                                                self.app_state.vibration_rank_lr[i].store(*v as u32, Ordering::Relaxed);
                                            }
                                            for (i, v) in ij.rank_type_gain.iter().enumerate() {
                                                self.app_state.vibration_rank_type_gain[i].store(*v, Ordering::Relaxed);
                                            }
                                            if ij.rank_level_gain > 0 {
                                                self.app_state.vibration_rank_level_gain.store(ij.rank_level_gain, Ordering::Relaxed);
                                            }
                                            if ij.rank_duration > 0 {
                                                self.app_state.vibration_rank_duration.store(ij.rank_duration, Ordering::Relaxed);
                                            }
                                            if ij.out_smooth > 0 {
                                                self.app_state.vibration_out_smooth.store(ij.out_smooth, Ordering::Relaxed);
                                            }
                                            self.app_state.vibration_throttle_window.store(ij.throttle_window, Ordering::Relaxed);
                                            self.app_state.vibration_throttle_max.store(ij.throttle_max, Ordering::Relaxed);
                                            self.app_state.vibration_throttle_dense_ratio.store(ij.throttle_dense_ratio, Ordering::Relaxed);
                                            self.vib_job_active = Some((ij.base_job.clone(), ij.class_name.clone()));
                                            self.vib_job_loaded = Some((bi, ci));
                                            self.vib_job_enabled = true;
                                            self.vib_import_msg = Some(format!("已导入并应用: {} - {}", ij.base_job, ij.class_name));
                                        } else {
                                            self.vib_import_msg = Some(format!("规范不符: 未匹配内置职业 {} - {}", ij.base_job, ij.class_name));
                                        }
                                    }
                                    None => {
                                        self.vib_import_msg = Some(format!("{} 解析失败 (规范不符)", f));
                                    }
                                }
                            }
                        }
                    });
                    /* 开关打开时, 切换职业自动应用 */
                    if self.vib_job_enabled && self.vib_job_loaded != Some((base_sel, class_sel)) {
                        Self::apply_job_vibration_preset_in(
                            jobs[base_sel].base_job,
                            cur_cls,
                            &self.app_state,
                            &mut self.config,
                            &mut self.vib_job_enabled,
                            &mut self.vib_job_active,
                            &mut self.vib_job_snapshot,
                        );
                        self.vib_job_loaded = Some((base_sel, class_sel));
                    }
                    if let Some(m) = &self.vib_import_msg {
                    ui.label(
                        egui::RichText::new(format!("{}", m))
                            .size(11.0)
                            .color(th.info),
                    );
                    ui.add_space(2.0);
                }
                if let Some(m) = &self.vib_export_msg {
                        ui.label(
                            egui::RichText::new(format!("已导出: {} (程序目录)", m))
                                .size(11.0)
                                .color(th.good),
                        );
                        ui.add_space(2.0);
                    }
                    ui.add_space(4.0);
                    if let Some((b, c)) = &self.vib_job_active {
                        ui.label(
                            egui::RichText::new(format!("当前职业: {} - {} (已应用职业预设, 通用型震动设定被覆盖)", b, c))
                                .size(12.0)
                                .color(th.good)
                                .strong(),
                        );
                    } else {
                        ui.label(
                            egui::RichText::new("开关已关闭: 全职业预设不应用, 使用通用型震动设定参数")
                                .size(11.0)
                                .weak(),
                        );
                    }
                    ui.add_space(4.0);
                    ui.label(
                        egui::RichText::new(format!("{} (攻速→力度设计: 极快轻盈防叠加震手, 慢速沉稳给强度)", cur_cls.desc))
                            .size(11.0)
                            .weak(),
                    );
                    ui.add_space(4.0);
                    ui.separator();
                    ui.add_space(4.0);
                    ui.label(
                        egui::RichText::new("下方参数与高级震动调校与【通用型震动设定】一致, 修改实时生效并自动保存到 JobVibration.toml")
                            .size(11.0)
                            .weak(),
                    );
                } else {
                /* 预设行 */
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("预设:").size(13.0).strong(),
                    );
                    let names: Vec<String> = self
                        .config
                        .vibration_presets
                        .iter()
                        .map(|p| p.name.clone())
                        .collect();
                    let sel = self.vib_preset_idx.min(names.len().saturating_sub(1));
                    if !names.is_empty() {
                        egui::ComboBox::from_id_salt("vib_preset_sel")
                            .selected_text(names[sel].clone())
                            .width(140.0)
                            .show_ui(ui, |ui| {
                                for (i, n) in names.iter().enumerate() {
                                    if ui.selectable_label(i == sel, n).clicked() {
                                        self.vib_preset_idx = i;
                                    }
                                }
                            });
                    }
                    if ui
                        .button(egui::RichText::new("应用").size(13.0))
                        .clicked()
                    {
                        if let Some(pr0) = self.config.vibration_presets.get(sel) {
                            /* 内置同名预设优先: 兼容旧 Config.toml (旧预设无评分参数字段,
                             * serde 默认 [100;15]/100/300, 应用时用内置新设计值覆盖) */
                            let pr = crate::config::default_vibration_presets()
                                .into_iter()
                                .find(|p| p.name == pr0.name)
                                .unwrap_or_else(|| pr0.clone());
                            for (i, v) in pr.params.iter().enumerate() {
                                p[i].store(*v, Ordering::Relaxed);
                            }
                            /* 应用预设的 L/R 权重 */
                            for (i, v) in pr.item_lr.iter().enumerate() {
                                self.app_state.vibration_item_lr[i].store(*v, Ordering::Relaxed);
                                self.config.vibration.item_lr[i] = *v;
                            }
                            /* 应用预设的评分事件 L/R 权重 */
                            for (i, v) in pr.rank_lr.iter().enumerate() {
                                self.app_state.vibration_rank_lr[i].store(*v, Ordering::Relaxed);
                                self.config.vibration.rank_lr[i] = *v;
                            }
                            /* 应用预设的评分特效参数 (细分强度/等级强度/时长) */
                            for (i, v) in pr.rank_type_gain.iter().enumerate() {
                                self.app_state.vibration_rank_type_gain[i].store(*v, Ordering::Relaxed);
                            }
                            self.app_state.vibration_rank_level_gain.store(pr.rank_level_gain, Ordering::Relaxed);
                            self.app_state.vibration_rank_duration.store(pr.rank_duration, Ordering::Relaxed);
                            self.config.vibration.rank_type_gain = pr.rank_type_gain;
                            self.config.vibration.rank_level_gain = pr.rank_level_gain;
                            self.config.vibration.rank_duration = pr.rank_duration;
                        self.app_state.vibration_out_smooth.store(pr.out_smooth, Ordering::Relaxed);
                        self.config.vibration.out_smooth = pr.out_smooth;
                            /* 同步到 config.vibration 并落盘(重启保持) */
                            Self::sync_vib_config_from_params(&mut self.config.vibration, p);
                            let _ = self.config.save_vibration_to_file(crate::config::AppConfig::vibration_path_for("Config.toml"));
                        }
                    }
                    ui.add_space(8.0);
                    if ui
                        .button(egui::RichText::new("保存当前").size(13.0))
                        .on_hover_text("把当前滑块参数写入 [vibration] 并保存 Config.toml")
                        .clicked()
                    {
                        Self::sync_vib_config_from_params(&mut self.config.vibration, p);
                        for i in 0..26 {
                            self.config.vibration.item_lr[i] = self
                                .app_state
                                .vibration_item_lr[i]
                                .load(Ordering::Relaxed);
                        }
                        for i in 0..30 {
                            self.config.vibration.rank_lr[i] = self
                                .app_state
                                .vibration_rank_lr[i]
                                .load(Ordering::Relaxed);
                        }
                        for i in 0..15 {
                            self.config.vibration.rank_type_gain[i] = self
                                .app_state
                                .vibration_rank_type_gain[i]
                                .load(Ordering::Relaxed);
                        }
                        self.config.vibration.rank_level_gain = self
                            .app_state
                            .vibration_rank_level_gain
                            .load(Ordering::Relaxed);
                        self.config.vibration.rank_duration = self
                            .app_state
                            .vibration_rank_duration
                            .load(Ordering::Relaxed);
                        let _ = self.config.save_vibration_to_file(crate::config::AppConfig::vibration_path_for("Config.toml"));
                    }
                    ui.add_space(8.0);
                    ui.label(egui::RichText::new("保存为:").size(13.0).weak());
                    ui.add(
                        egui::TextEdit::singleline(&mut self.vib_preset_name)
                            .desired_width(90.0)
                            .hint_text("名称"),
                    );
                    if ui
                        .button(egui::RichText::new("保存").size(13.0))
                        .clicked()
                    {
                        let name = self.vib_preset_name.trim().to_string();
                        if !name.is_empty() {
                            let mut params = [0u32; 60];
                            for i in 0..60 {
                                params[i] = p[i].load(Ordering::Relaxed);
                            }
                            let mut item_lr = [0u32; 26];
                            for i in 0..26 {
                                item_lr[i] = self.app_state.vibration_item_lr[i].load(Ordering::Relaxed);
                            }
                            let mut rank_lr = [0u32; 30];
                            for i in 0..30 {
                                rank_lr[i] = self.app_state.vibration_rank_lr[i].load(Ordering::Relaxed);
                            }
                            let mut rank_type_gain = [0u32; 15];
                            for i in 0..15 {
                                rank_type_gain[i] = self.app_state.vibration_rank_type_gain[i].load(Ordering::Relaxed);
                            }
                            let rank_level_gain = self.app_state.vibration_rank_level_gain.load(Ordering::Relaxed);
                            let rank_duration = self.app_state.vibration_rank_duration.load(Ordering::Relaxed);
                            let preset = crate::config::VibrationPreset { name: name.clone(), params, item_lr, rank_lr, rank_type_gain, rank_level_gain, rank_duration, out_smooth: self.app_state.vibration_out_smooth.load(Ordering::Relaxed) };
                            // 同名替换而非追加: 追加重名预设后, 应用按名匹配会
                            // 顶替用户自存版本, 且重名项永远删不掉
                            let protected = name == "默认" || name == "测试版(全0)";
                            if let Some(pos) = self.config.vibration_presets.iter().position(|x| x.name == name) {
                                if !protected {
                                    self.config.vibration_presets[pos] = preset;
                                    self.vib_preset_name.clear();
                                    self.vib_preset_idx = pos;
                                }
                            } else {
                                self.config.vibration_presets.push(preset);
                                self.vib_preset_name.clear();
                                self.vib_preset_idx =
                                    self.config.vibration_presets.len() - 1;
                            }
                            let _ = self.config.save_vibration_to_file(crate::config::AppConfig::vibration_path_for("Config.toml"));
                        }
                    }
                    if ui
                        .button(egui::RichText::new("删除").size(13.0))
                        .on_hover_text("仅\"默认\"与\"测试版(全0)\"受保护, 其余可删除")
                        .clicked()
                    {
                        if let Some(pr) = self.config.vibration_presets.get(sel) {
                            let protected =
                                pr.name == "默认" || pr.name == "测试版(全0)";
                            if !protected {
                                self.config.vibration_presets.remove(sel);
                                self.vib_preset_idx = 0;
                                let _ = self.config.save_vibration_to_file(crate::config::AppConfig::vibration_path_for("Config.toml"));
                            }
                        }
                    }
                    ui.add_space(8.0);
                    if ui
                        .button(egui::RichText::new("导出当前设置").size(13.0))
                        .on_hover_text("导出当前页面生效的通用震动设定 (params/评分/高级算法) 为 TOML")
                        .clicked()
                    {
                        let content = Self::export_vibration_settings(&self.app_state, &self.config);
                        let fname = Self::save_export_file(&content, "vibration_export");
                        self.vib_export_msg = Some(fname);
                    }
                    ui.add_space(8.0);
                    /* 导入设置 (v33.1: 扫描 vibration_export_*.toml) */
                    {
                        let files = crate::job_presets::list_export_files("vibration_export");
                        let sel = self.vib_import_sel.min(files.len().saturating_sub(1));
                        if !files.is_empty() {
                            egui::ComboBox::from_id_salt("vib_import_sel")
                                .selected_text(files[sel].clone())
                                .width(220.0)
                                .show_ui(ui, |ui| {
                                    for (i, f) in files.iter().enumerate() {
                                        if ui.selectable_label(i == sel, f).clicked() {
                                            self.vib_import_sel = i;
                                        }
                                    }
                                });
                        }
                        if ui
                            .button(egui::RichText::new("导入设置").size(13.0))
                            .on_hover_text("从 vibration_export_*.toml 导入通用震动设定 (往返一致规范)")
                            .clicked()
                        {
                            if let Some(f) = crate::job_presets::list_export_files("vibration_export").get(self.vib_import_sel) {
                                if let Some(iv) = crate::job_presets::parse_vibration_export(f) {
                                    for (i, v) in iv.params.iter().enumerate() {
                                        self.app_state.vibration_params[i].store(*v, Ordering::Relaxed);
                                    }
                                    for (i, v) in iv.item_lr.iter().enumerate() {
                                        self.app_state.vibration_item_lr[i].store(*v, Ordering::Relaxed);
                                        self.config.vibration.item_lr[i] = *v;
                                    }
                                    for (i, v) in iv.rank_lr.iter().enumerate() {
                                        self.app_state.vibration_rank_lr[i].store(*v, Ordering::Relaxed);
                                        self.config.vibration.rank_lr[i] = *v;
                                    }
                                    for (i, v) in iv.rank_type_gain.iter().enumerate() {
                                        self.app_state.vibration_rank_type_gain[i].store(*v, Ordering::Relaxed);
                                        self.config.vibration.rank_type_gain[i] = *v;
                                    }
                                    self.app_state.vibration_rank_level_gain.store(iv.rank_level_gain, Ordering::Relaxed);
                                    self.app_state.vibration_rank_duration.store(iv.rank_duration, Ordering::Relaxed);
                                    self.app_state.vibration_out_smooth.store(iv.out_smooth, Ordering::Relaxed);
                                    self.app_state.vibration_throttle_window.store(iv.throttle_window_ms, Ordering::Relaxed);
                                    self.app_state.vibration_throttle_max.store(iv.throttle_max_hits, Ordering::Relaxed);
                                    self.app_state.vibration_throttle_dense_ratio.store(iv.throttle_dense_ratio, Ordering::Relaxed);
                                    self.app_state.vibration_independent_test.store(iv.independent_test, Ordering::Relaxed);
                                    self.app_state.vibration_move_independent.store(iv.move_independent, Ordering::Relaxed);
                                    self.app_state.vibration_random_gain.store(iv.random_gain, Ordering::Relaxed);
                                    self.app_state.vibration_motor_l_gain.store(iv.motor_l_gain, Ordering::Relaxed);
                                    self.app_state.vibration_motor_r_gain.store(iv.motor_r_gain, Ordering::Relaxed);
                                    self.config.vibration.rank_level_gain = iv.rank_level_gain;
                                    self.config.vibration.rank_duration = iv.rank_duration;
                                    self.config.vibration.out_smooth = iv.out_smooth;
                                    self.config.vibration.throttle_window_ms = iv.throttle_window_ms;
                                    self.config.vibration.throttle_max_hits = iv.throttle_max_hits;
                                    self.config.vibration.throttle_dense_ratio = iv.throttle_dense_ratio;
                                    self.config.vibration.independent_test = iv.independent_test;
                                    self.config.vibration.move_independent = iv.move_independent;
                                    self.config.vibration.random_gain = iv.random_gain;
                                    self.config.vibration.motor_l_gain = iv.motor_l_gain;
                                    self.config.vibration.motor_r_gain = iv.motor_r_gain;
                                    let _ = self.config.save_vibration_to_file(crate::config::AppConfig::vibration_path_for("Config.toml"));
                                    self.vib_import_msg = Some(f.clone());
                                } else {
                                    self.vib_import_msg = Some(format!("{} 解析失败 (规范不符)", f));
                                }
                            }
                        }
                    }
                    ui.add_space(8.0);
                    if ui
                        .button(egui::RichText::new("还原默认").size(13.0))
                        .on_hover_text("把预设恢复为出厂内置 (误删后一键还原)")
                        .clicked()
                    {
                        self.config.vibration_presets =
                            crate::config::default_vibration_presets();
                        self.vib_preset_idx = 0;
                        /* 若当前为空或损坏, 同时把参数复位为默认预设 */
                        if let Some(pr) = self.config.vibration_presets.first() {
                            if p[0].load(Ordering::Relaxed) == 0 {
                                for (i, v) in pr.params.iter().enumerate() {
                                    p[i].store(*v, Ordering::Relaxed);
                                }
                                for (i, v) in pr.item_lr.iter().enumerate() {
                                    self.app_state.vibration_item_lr[i].store(*v, Ordering::Relaxed);
                                    self.config.vibration.item_lr[i] = *v;
                                }
                                for (i, v) in pr.rank_lr.iter().enumerate() {
                                    self.app_state.vibration_rank_lr[i].store(*v, Ordering::Relaxed);
                                    self.config.vibration.rank_lr[i] = *v;
                                }
                                for (i, v) in pr.rank_type_gain.iter().enumerate() {
                                    self.app_state.vibration_rank_type_gain[i].store(*v, Ordering::Relaxed);
                                    self.config.vibration.rank_type_gain[i] = *v;
                                }
                                self.app_state.vibration_rank_level_gain.store(pr.rank_level_gain, Ordering::Relaxed);
                                self.app_state.vibration_rank_duration.store(pr.rank_duration, Ordering::Relaxed);
                                self.config.vibration.rank_level_gain = pr.rank_level_gain;
                                self.config.vibration.rank_duration = pr.rank_duration;
                        self.app_state.vibration_out_smooth.store(pr.out_smooth, Ordering::Relaxed);
                        self.config.vibration.out_smooth = pr.out_smooth;
                            }
                        }
                        let _ = self.config.save_vibration_to_file(crate::config::AppConfig::vibration_path_for("Config.toml"));
                    }
                });
                ui.add_space(6.0);
                if let Some(m) = &self.vib_export_msg {
                    ui.label(
                        egui::RichText::new(format!("已导出: {} (程序目录)", m))
                            .size(11.0)
                            .color(th.good),
                    );
                    ui.add_space(2.0);
                }
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(
                            "内置: 默认/高振幅/节奏律动/极简轻巧/实战竞技/测试版(全0)",
                        )
                        .size(11.0)
                        .weak(),
                    )
                    .wrap(),
                );
                ui.add_space(4.0);
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(
                            "提示: 仅\"默认\"与\"测试版(全0)\"不可删除; 误删可用\"还原默认\"恢复。",
                        )
                        .size(11.0)
                        .weak(),
                    )
                    .wrap(),
                );
                ui.add_space(8.0);
                ui.separator();
                ui.add_space(8.0);

                } /* job_mode else 结束 */

                /* 状态 */
                {
                    let connected = self.app_state.vibration_connected.load(Ordering::Relaxed);
                    let events = self
                        .app_state
                        .vibration_events_received
                        .load(Ordering::Relaxed);
                    let (ctext, ccolor) = if connected {
                        (
                            format!("● 已连接游戏 (累计 {} 事件)", events),
                            th.good,
                        )
                    } else {
                        (
                            "○ 未连接 (请先启动游戏)".to_string(),
                            th.info,
                        )
                    };
                    ui.label(egui::RichText::new(ctext).size(13.0).color(ccolor).strong());
                }
                });
                ui.add_space(10.0);
                ui.separator();
                ui.add_space(8.0);

                /* 召唤专属: 绝对震动频率 (v26.3, 置顶显示在马达区上方, 仅召唤职业) */
                if job_mode && self.app_state.vibration_abs_freq_enabled.load(Ordering::Relaxed) {
                    Self::vib_section(ui, self.dark_mode, "【召唤专属】绝对震动频率 (置顶)", |ui| {
                        ui.label(
                            egui::RichText::new(
                                "开启后一切震动算法失效 (密度/间隔/节流/连击倍率/评分通道等),\n\
                                 只在 [时间-震动次数] 内注入, 均匀分布固定节拍。\n\
                                 全局强度/强度上限仍可控制输出, 移动独立。",
                            )
                            .size(11.0)
                            .weak(),
                        );
                        ui.add_space(4.0);
                        let mut ae = self
                            .app_state
                            .vibration_abs_freq_enabled
                            .load(Ordering::Relaxed);
                        if ui
                            .checkbox(&mut ae, "绝对震动频率 (3 秒最多 6 次)")
                            .changed()
                        {
                            self.app_state
                                .vibration_abs_freq_enabled
                                .store(ae, Ordering::Relaxed);
                            self.config.vibration.abs_freq_enabled = ae;
                            let _ = self.config.save_vibration_to_file(crate::config::AppConfig::vibration_path_for("Config.toml"));
                        }
                        let mut aw = self
                            .app_state
                            .vibration_abs_freq_window
                            .load(Ordering::Relaxed) as f32;
                        if ui
                            .add(egui::Slider::new(&mut aw, 1000.0..=10000.0).text("绝对频率窗口 ms"))
                            .changed()
                        {
                            self.app_state
                                .vibration_abs_freq_window
                                .store(aw as u32, Ordering::Relaxed);
                            self.config.vibration.abs_freq_window_ms = aw as u32;
                            let _ = self.config.save_vibration_to_file(crate::config::AppConfig::vibration_path_for("Config.toml"));
                        }
                        let mut am = self
                            .app_state
                            .vibration_abs_freq_max
                            .load(Ordering::Relaxed) as f32;
                        if ui
                            .add(egui::Slider::new(&mut am, 1.0..=20.0).text("窗口内最大震动次数"))
                            .changed()
                        {
                            self.app_state
                                .vibration_abs_freq_max
                                .store(am as u32, Ordering::Relaxed);
                            self.config.vibration.abs_freq_max = am as u32;
                            let _ = self.config.save_vibration_to_file(crate::config::AppConfig::vibration_path_for("Config.toml"));
                        }
                    });
                    ui.add_space(10.0);
                    ui.separator();
                    ui.add_space(8.0);
                }

                /* 马达区: 实时输出检测 (增益已并入各基础功能的 L/R 调控) */
                Self::vib_section(
                    ui,
                    self.dark_mode,
                    "马达区 (实时输出检测)",
                    |ui| {
                    ui.label(
                        egui::RichText::new(
                            "左右马达实时输出检测 (测试按钮或进图战斗时观察)。",
                        )
                        .size(11.0)
                        .weak(),
                    );
                    let out_l = self.app_state.vibration_out_l.load(Ordering::Relaxed);
                    let out_r = self.app_state.vibration_out_r.load(Ordering::Relaxed);
                    let pct_l = out_l as f32 / 65535.0;
                    let pct_r = out_r as f32 / 65535.0;
                    ui.add_space(2.0);
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new("左马达").strong().size(12.0).color(th.motor_l),
                        );
                        ui.add(
                            egui::ProgressBar::new(pct_l)
                                .desired_width(220.0)
                                .fill(th.motor_l)
                                .show_percentage(),
                        );
                        ui.label(
                            egui::RichText::new(format!("{:>5}", out_l))
                                .monospace()
                                .size(11.0)
                                .weak(),
                        );
                    });
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new("右马达").strong().size(12.0).color(th.motor_r),
                        );
                        ui.add(
                            egui::ProgressBar::new(pct_r)
                                .desired_width(220.0)
                                .fill(th.motor_r)
                                .show_percentage(),
                        );
                        ui.label(
                            egui::RichText::new(format!("{:>5}", out_r))
                                .monospace()
                                .size(11.0)
                                .weak(),
                        );
                    });
                });
                ui.add_space(10.0);
                ui.separator();
                ui.add_space(8.0);

                /* 全局总调整 */
                Self::vib_section(
                    ui,
                    self.dark_mode,
                    "全局总闸 (任一为 0 则全部关闭)",
                    |ui| {
                    let rows = [
                        ("全局总调整 %", 9, 100.0, "", "总开关: 0 = 全部关闭; 调低 = 整体减弱所有震动", 0usize),
                        ("攻击频率 (总闸1)", 0, 100.0, "", "攻击/技能命中反馈总闸: 0 = 命中不震", 1),
                        ("强度上限 (总闸2)", 4, 100.0, "%", "输出强度上限: 调低 = 所有震动更弱 (保护手柄)", 2),
                        ("衰减时间", 5, 500.0, "ms", "震动衰减速度: 越小越脆快, 越大越绵长 (基线)", 3),
                        ("连击增强 (渐进至上限)", 6, 100.0, "", "连击数越高震动越强, 渐进到强度上限", 4),
                        ("节奏感 (右马达比例)", 18, 100.0, "", "右马达强度占比: 50 = 均衡, 高 = 右重左轻", 5),
                    ];
                    /* 每项独立块: 名称+滑块+说明+L/R, 宽度自适应卡片, 长文本自然换行 */
                    /* 每项独立块: 名称+滑块+说明+L/R, 宽屏双列排布 */
                    let col_w = (ui.available_width() - theme::SP_L) / 2.0;
                    ui.horizontal(|ui| {
                        let half = rows.len().div_ceil(2);
                        for chunk in [&rows[..half], &rows[half..]] {
                            ui.vertical(|ui| {
                                ui.set_min_width(col_w);
                                ui.set_max_width(col_w);
                                for (name, pidx, max, suffix, hint, item) in chunk.iter().copied() {
                                    ui.label(
                                        egui::RichText::new(name)
                                            .size(13.0)
                                            .color(th.text),
                                    );
                                    let mut v = p[pidx].load(Ordering::Relaxed) as f32;
                                    if ui
                                        .add(egui::Slider::new(&mut v, 0.0..=max).suffix(suffix))
                                        .changed()
                                    {
                                        p[pidx].store(v as u32, Ordering::Relaxed);
                                    }
                                    ui.label(egui::RichText::new(hint).size(11.0).weak());
                                    Self::render_item_lr(ui, self.dark_mode, &self.app_state, &mut self.config.vibration.item_lr, item);
                                    ui.add_space(6.0);
                                }
                            });
                        }
                    });
                    /* 震动随机性 (独立参数, 不占 60 槽) */
                    let mut rg = self
                        .app_state
                        .vibration_random_gain
                        .load(Ordering::Relaxed) as f32;
                    if ui
                        .add(
                            egui::Slider::new(&mut rg, 0.0..=100.0)
                                .text("震动随机性 %"),
                        )
                        .changed()
                    {
                        self.app_state
                            .vibration_random_gain
                            .store(rg as u32, Ordering::Relaxed);
                        self.config.vibration.random_gain = rg as u32;
                        let _ = self.config.save_vibration_to_file(crate::config::AppConfig::vibration_path_for("Config.toml"));
                    }
                    ui.label(
                        egui::RichText::new(
                            "0 = 关闭; N = 强度在原本上限附近随机增减 ±N%\n\
                             (例: 5 = 每次震动有 0~5% 随机浮动, 手感更自然)",
                        )
                        .size(11.0)
                        .weak(),
                    );
                    /* 移动 (持续震动): 位于节奏感下方 (v22 布局调整, 原在评分特效卡片) */
                    ui.add_space(8.0);
                    ui.separator();
                    ui.add_space(8.0);
                    ui.vertical(|ui| {
                        /* 移动独立开关 (v24.5): 默认开, 移动一直在走, 不吃全局强度 */
                        let mut mi = self.app_state.vibration_move_independent.load(Ordering::Relaxed);
                        if ui
                            .checkbox(&mut mi, "移动持续震动独立于全局强度 (默认开)")
                            .on_hover_text(
                                "开: 移动震动不受全局总调整/强度上限影响 (移动一直在走, 吃全局强度容易直接没有震动)。\n\
                                 关: 移动震动也受全局总调整与强度上限约束。",
                            )
                            .changed()
                        {
                            self.app_state.vibration_move_independent.store(mi, Ordering::Relaxed);
                            self.config.vibration.move_independent = mi;
                            let _ = self.config.save_vibration_to_file(crate::config::AppConfig::vibration_path_for("Config.toml"));
                        }
                        ui.set_width(ui.available_width());
                        let cnt = self.app_state.vibration_rank_type_events[11].load(Ordering::Relaxed);
                        ui.label(
                            egui::RichText::new(format!("移动 (持续震动) ({})", cnt))
                                .size(13.0)
                                .color(th.text),
                        );
                        let mut g = self.app_state.vibration_rank_type_gain[11].load(Ordering::Relaxed) as f32;
                        if ui
                            .add(egui::Slider::new(&mut g, 0.0..=100.0).suffix("").text("移动持续震动强度"))
                            .changed()
                        {
                            self.app_state.vibration_rank_type_gain[11].store(g as u32, Ordering::Relaxed);
                            self.config.vibration.rank_type_gain[11] = g as u32;
                            let _ = self.config.save_vibration_to_file(crate::config::AppConfig::vibration_path_for("Config.toml"));
                        }
                        ui.label(
                            egui::RichText::new("角色移动持续震动 (0 停)。\n与高级调校 A2 移动走路质感/A3 移动积累增强配合: 长时间移动积累走位能量, 停手后窗口内攻击增强")
                                .size(11.0)
                                .weak(),
                        );
                        Self::render_rank_lr(ui, self.dark_mode, &self.app_state, &mut self.config.vibration.rank_lr, 11);
                    });
                });
                ui.add_space(10.0);
                ui.separator();
                ui.add_space(8.0);

                /* 伤害飘字 */
                Self::vib_section(
                    ui,
                    self.dark_mode,
                    "伤害飘字震动 (事件类型细分)",
                    |ui| {
                    ui.label(
                        egui::RichText::new(
                            "DLL 采集游戏伤害飘字(每次命中必经), 按需开启。",
                        )
                        .size(11.0)
                        .weak(),
                    );
                    let mut font_on = self
                        .app_state
                        .vibration_font_hits
                        .load(Ordering::Relaxed);
                    if ui
                        .checkbox(&mut font_on, "启用伤害飘字震动 (默认关闭)")
                        .changed()
                    {
                        self.app_state
                            .vibration_font_hits
                            .store(font_on, Ordering::Relaxed);
                    }
                    ui.label(
                        egui::RichText::new("关闭后所有战斗事件都不震, 仅保留测试按钮可用")
                            .size(11.0)
                            .weak(),
                    );
                    let mut fs = p[10].load(Ordering::Relaxed) as f32;
                    if ui
                        .add(egui::Slider::new(&mut fs, 0.0..=100.0).text("通用飘字强度"))
                        .changed()
                    {
                        p[10].store(fs as u32, Ordering::Relaxed);
                    }
                    ui.label(
                        egui::RichText::new(
                            "飘字类型强度的兜底强度，设置为0的时候，全部震动类型为独立可调控。",
                        )
                        .size(11.0)
                        .weak(),
                    );
                    Self::render_item_lr(ui, self.dark_mode, &self.app_state, &mut self.config.vibration.item_lr, 6);
                    let mut fi = p[11].load(Ordering::Relaxed) as f32;
                    if ui
                        .add(egui::Slider::new(&mut fi, 10.0..=500.0).text("飘字最小间隔 ms"))
                        .changed()
                    {
                        p[11].store(fi as u32, Ordering::Relaxed);
                    }
                    ui.label(
                        egui::RichText::new("同类型事件最短触发间隔, 防止高频连打过度震动")
                            .size(11.0)
                            .weak(),
                    );
                    ui.add_space(4.0);
                    ui.label(
                        egui::RichText::new("飘字类型强度 (a6 标志分类, 默认 0 = 不震, 逐个开启测试)")
                            .size(12.0)
                            .color(th.accent_text),
                    );
                    let rows = [
                        ("持续伤害 DOT 反馈 (0x20)", 12, 7usize, "中毒/灼烧/出血等持续掉血反馈, 每次掉血持续震动"),
                        ("装备特效反馈 (0x04)", 15, 8, "装备触发效果(特效等), 例如天域之类的"),
                        ("玩家状态变化反馈 (0x08)", 14, 9, "自身状态变化(增益/减益/回复)"),
                        ("特殊攻击反馈 (0x10) 暴击/破招/背击", 13, 10, "暴击/破招/背击瞬间的重击强调"),
                        ("玩家命中反馈 (0x01)", 16, 11, "普通攻击与技能每次命中的基础反馈 (核心)"),
                        ("玩家受击反馈 (0x02)", 17, 12, "被敌人击中的反馈, 数值越大被打越有感觉"),
                    ];
                    /* 每项独立块: 名称+滑块+说明+L/R, 宽屏双列排布 */
                    let col_w = (ui.available_width() - theme::SP_L) / 2.0;
                    ui.horizontal(|ui| {
                        let half = rows.len().div_ceil(2);
                        for chunk in [&rows[..half], &rows[half..]] {
                            ui.vertical(|ui| {
                                ui.set_min_width(col_w);
                                ui.set_max_width(col_w);
                                for (name, pidx, item, hint) in chunk.iter().copied() {
                                    ui.label(
                                        egui::RichText::new(name)
                                            .size(13.0)
                                            .color(th.text),
                                    );
                                    let mut v = p[pidx].load(Ordering::Relaxed) as f32;
                                    if ui
                                        .add(egui::Slider::new(&mut v, 0.0..=100.0))
                                        .changed()
                                    {
                                        p[pidx].store(v as u32, Ordering::Relaxed);
                                    }
                                    ui.label(egui::RichText::new(hint).size(11.0).weak());
                                    Self::render_item_lr(ui, self.dark_mode, &self.app_state, &mut self.config.vibration.item_lr, item);
                                    ui.add_space(6.0);
                                }
                            });
                        }
                    });
                });
                ui.add_space(10.0);
                ui.separator();
                ui.add_space(8.0);

                /* 评分特效震动 (DLL 已采集: 评分等级/击杀点/闪避/暴击/破招/背击等) */
                Self::vib_section(ui, self.dark_mode, "评分特效震动 (等级 + 细分事件)", |ui| {
                    ui.label(
                        egui::RichText::new(
                            "评分事件由 DLL 采集, 每项强度独立可调 (0 = 关闭)。\n\
                             括号内为累计触发次数, 用于区分哪个事件真正触发了。",
                        )
                        .size(11.0)
                        .weak(),
                    );
                    ui.add_space(4.0);
                    /* 状态: 最近等级 + 事件计数 */
                    let rl = self.app_state.vibration_rank_last.load(Ordering::Relaxed);
                    let re = self.app_state.vibration_rank_events.load(Ordering::Relaxed);
                    let (rtext, rcolor) = if rl >= 8 {
                        (
                            format!("★ 最近评分: 等级 {} (特殊动作!)  累计 {} 次", rl, re),
                            th.warn,
                        )
                    } else if rl >= 2 {
                        (
                            format!("● 最近评分: 等级 {}  累计 {} 次", rl, re),
                            th.info,
                        )
                    } else {
                        (
                            "○ 暂无评分事件 (进副本打怪触发)".to_string(),
                            th.hint,
                        )
                    };
                    ui.label(egui::RichText::new(rtext).size(12.0).color(rcolor).strong());
                    ui.add_space(4.0);
                    /* 评分等级强度 + 满幅时长 */
                    let mut lvl_gain = self.app_state.vibration_rank_level_gain.load(Ordering::Relaxed) as f32;
                    if ui
                        .add(egui::Slider::new(&mut lvl_gain, 0.0..=100.0).text("评分等级强度 % (等级 F~SSS)"))
                        .changed()
                    {
                        self.app_state.vibration_rank_level_gain.store(lvl_gain as u32, Ordering::Relaxed);
                        self.config.vibration.rank_level_gain = lvl_gain as u32;
                        let _ = self.config.save_vibration_to_file(crate::config::AppConfig::vibration_path_for("Config.toml"));
                    }
                    let mut rank_dur = self.app_state.vibration_rank_duration.load(Ordering::Relaxed) as f32;
                    if ui
                        .add(egui::Slider::new(&mut rank_dur, 20.0..=1000.0).text("满幅脉冲时长 ms (所有评分事件)"))
                        .changed()
                    {
                        self.app_state.vibration_rank_duration.store(rank_dur as u32, Ordering::Relaxed);
                        self.config.vibration.rank_duration = rank_dur as u32;
                        let _ = self.config.save_vibration_to_file(crate::config::AppConfig::vibration_path_for("Config.toml"));
                    }
                    ui.add_space(4.0);
                    ui.label(
                        egui::RichText::new("细分事件独立强度 (触发次数, 0 = 关闭):")
                            .size(12.0)
                            .color(th.accent_text),
                    );
                    /* 细分事件独立强度 + 触发计数 */
                    let rank_names = [
                        ("评分点", 0usize, "每次命中累积的评分点"),
                        ("极限闪避", 1, "闪避/反击成功瞬间"),
                        ("暴击", 2, "暴击命中瞬间 (未定位)"),
                        /* 破招/背击 已搁置 (2026-08-15): 客户端判定未定位, 推断服务器判定
                         * 见 docs/背击破招定位最终结论_v19.md
                         * ("破招", 3, "破招控制命中 (未定位)"),
                         * ("背击", 4, "背后攻击命中 (未定位)"), */
                        ("最终击杀", 5, "BOSS 击杀/结算 (未定位)"),
                        ("凌空追击", 6, "空中追击命中 (槽评分)"),
                        ("命中第一击", 7, "命中怪物第一击 (原\"破甲\"槽, 实测语义)"),
                        ("增益叠加", 8, "增益层数叠加 (未定位)"),
                        ("释放技能", 9, "释放技能瞬间 (n2500[5747] 计数)"),
                        /* 镜头震动/技能震动 已搁置 (2026-08-15): 见 docs/震屏事件搁置记录_v16.md
                         * ("镜头震动", 10, "真 [shake screen] 词条屏幕震动"),
                         * ("技能震动", 12, "技能释放/命中时的屏幕震动"), */
                        /* 移动 (持续震动) 已移至"全局总闸"卡片节奏感下方 (v22) */
                        ("暴击特写", 13, "暴击/击杀时的镜头特写震屏"),
                        ("怪物死亡", 14, "怪物死亡 (OnTargetDie 纯目标死亡信号)"),
                    ];
                    let col_w = (ui.available_width() - theme::SP_L) / 2.0;
                    ui.horizontal(|ui| {
                        let half = rank_names.len().div_ceil(2);
                        for chunk in [&rank_names[..half], &rank_names[half..]] {
                            ui.vertical(|ui| {
                                ui.set_min_width(col_w);
                                ui.set_max_width(col_w);
                                for (name, idx, hint) in chunk.iter().copied() {
                                /* 名称(计数) 独立标签在数值上方 (与飘字类型强度一致) */
                                let cnt = self.app_state.vibration_rank_type_events[idx].load(Ordering::Relaxed);
                                ui.label(
                                    egui::RichText::new(format!("{} ({})", name, cnt))
                                        .size(13.0)
                                        .color(th.text),
                                );
                                let mut g = self.app_state.vibration_rank_type_gain[idx].load(Ordering::Relaxed) as f32;
                                if ui
                                    .add(egui::Slider::new(&mut g, 0.0..=100.0))
                                    .changed()
                                {
                                    self.app_state.vibration_rank_type_gain[idx].store(g as u32, Ordering::Relaxed);
                                    self.config.vibration.rank_type_gain[idx] = g as u32;
                                    let _ = self.config.save_vibration_to_file(crate::config::AppConfig::vibration_path_for("Config.toml"));
                                }
                                ui.label(egui::RichText::new(hint).size(11.0).weak());
                                /* 每个评分事件的独立 L/R 马达权重 */
                                Self::render_rank_lr(ui, self.dark_mode, &self.app_state, &mut self.config.vibration.rank_lr, idx);
                                ui.add_space(6.0);
                                }
                            });
                        }
                    });
                    ui.add_space(4.0);
                    if ui.add(th.secondary_button("测试评分震动 (模拟等级 8)")).clicked()
                    {
                        self.app_state.vibration_rank_test.store(true, Ordering::Relaxed);
                    }
                });
                ui.add_space(8.0);
                ui.separator();
                ui.add_space(8.0);

                /* 评分动态衰减 (类鬼泣, v29): 独立卡片, 位于评分特效震动下方 */
                Self::vib_section(ui, self.dark_mode, "评分动态衰减 (类鬼泣): 越打越猛, 停手跌分", |ui| {
                    let rd = self.app_state.vibration_rank_decay_enabled.load(Ordering::Relaxed);
                    ui.label(
                        egui::RichText::new(
                            "攻击/命中提升评级 (0-8), 停手超过延迟后逐级衰减。\n\
                             最高评级反馈 = 当前预设参数 ×120%; 最低档保底 60% (不失去基础反馈)。",
                        )
                        .size(11.0)
                        .weak(),
                    );
                    ui.add_space(4.0);
                    let mut rd_on = rd;
                    if ui
                        .checkbox(&mut rd_on, "启用评分动态衰减 (类鬼泣)")
                        .on_hover_text(
                            "攻击/命中提升评级 (0-8), 停手超过延迟后逐级衰减。\n\
                             最高评级反馈 = 当前预设参数 ×1.2; 最低档保底 0.6 (不失去基础反馈)。",
                        )
                        .changed()
                    {
                        self.app_state
                            .vibration_rank_decay_enabled
                            .store(rd_on, Ordering::Relaxed);
                        self.config.vibration.rank_decay_enabled = rd_on;
                        let _ = self.config.save_vibration_to_file(crate::config::AppConfig::vibration_path_for("Config.toml"));
                    }
                    let mut rdd = self
                        .app_state
                        .vibration_rank_decay_delay
                        .load(Ordering::Relaxed) as f32;
                    if ui
                        .add_enabled(
                            rd_on,
                            egui::Slider::new(&mut rdd, 500.0..=5000.0)
                                .text("停手衰减延迟 ms"),
                        )
                        .changed()
                    {
                        self.app_state
                            .vibration_rank_decay_delay
                            .store(rdd as u32, Ordering::Relaxed);
                        self.config.vibration.rank_decay_delay_ms = rdd as u32;
                        let _ = self.config.save_vibration_to_file(crate::config::AppConfig::vibration_path_for("Config.toml"));
                    }
                    let mut rds = self
                        .app_state
                        .vibration_rank_decay_speed
                        .load(Ordering::Relaxed) as f32;
                    if ui
                        .add_enabled(
                            rd_on,
                            egui::Slider::new(&mut rds, 1.0..=5.0)
                                .text("衰减速度 级/秒"),
                        )
                        .changed()
                    {
                        self.app_state
                            .vibration_rank_decay_speed
                            .store(rds as u32, Ordering::Relaxed);
                        self.config.vibration.rank_decay_speed = rds as u32;
                        let _ = self.config.save_vibration_to_file(crate::config::AppConfig::vibration_path_for("Config.toml"));
                    }
                    let mut rdmin = self
                        .app_state
                        .vibration_rank_decay_min
                        .load(Ordering::Relaxed) as f32;
                    if ui
                        .add_enabled(
                            rd_on,
                            egui::Slider::new(&mut rdmin, 40.0..=100.0)
                                .text("最低反馈倍率 % (评级 0, 保底)"),
                        )
                        .changed()
                    {
                        self.app_state
                            .vibration_rank_decay_min
                            .store(rdmin as u32, Ordering::Relaxed);
                        self.config.vibration.rank_decay_min_mul = rdmin as u32;
                        let _ = self.config.save_vibration_to_file(crate::config::AppConfig::vibration_path_for("Config.toml"));
                    }
                    let mut rdmax = self
                        .app_state
                        .vibration_rank_decay_max
                        .load(Ordering::Relaxed) as f32;
                    if ui
                        .add_enabled(
                            rd_on,
                            egui::Slider::new(&mut rdmax, 100.0..=200.0)
                                .text("最高反馈倍率 % (满评分, 预设×此值)"),
                        )
                        .changed()
                    {
                        self.app_state
                            .vibration_rank_decay_max
                            .store(rdmax as u32, Ordering::Relaxed);
                        self.config.vibration.rank_decay_max_mul = rdmax as u32;
                        let _ = self.config.save_vibration_to_file(crate::config::AppConfig::vibration_path_for("Config.toml"));
                    }
                });
                ui.add_space(8.0);
                ui.separator();
                ui.add_space(8.0);

/* 高级震动调校 (六类 L/R 比例已并入基础区每项权重, 此处不再重复) */
                Self::vib_section(
                    ui,
                    self.dark_mode,
                    "高级震动调校 (精细微调, 玩家可选)",
                    |ui| {
                egui::CollapsingHeader::new(
                    egui::RichText::new("展开高级参数分区 (波形/衰减/窗口/连击/脉冲)")
                        .size(12.0)
                        .strong(),
                )
                .default_open(false)
                .show(ui, |ui| {
                    let mut adv_on = if job_mode {
                        self.vib_job_adv_on
                    } else {
                        self.app_state
                            .vibration_advanced_enabled
                            .load(Ordering::Relaxed)
                    };
                    ui.horizontal(|ui| {
                        if ui
                            .checkbox(&mut adv_on, "允许高级调校")
                            .on_hover_text(
                                "默认关闭: 只用基础分区即可, 高级参数保持当前配置值。\n\
                                 开启后可在下方精细调整 L/R 马达比例/衰减/窗口/连击/脉冲。",
                            )
                            .changed()
                        {
                            if job_mode {
                                self.vib_job_adv_on = adv_on;
                            } else {
                                self.app_state
                                    .vibration_advanced_enabled
                                    .store(adv_on, Ordering::Relaxed);
                                self.config.vibration.advanced_enabled = adv_on;
                                let _ = self.config.save_vibration_to_file(crate::config::AppConfig::vibration_path_for("Config.toml"));
                            }
                        }
                        ui.label(
                            egui::RichText::new(
                                if adv_on {
                                    "已开启: 可精细调整下方参数"
                                } else {
                                    "已关闭: 高级参数保持当前配置 (默认)"
                                },
                            )
                            .size(11.0)
                            .color(if adv_on { th.good } else { th.hint }),
                        );
                    });
                    ui.add_space(4.0);

                    if !adv_on {
                        ui.label(
                            egui::RichText::new(
                                "玩家通常只用基础分区即可获得良好手感。\n\
                                 高级调校基于当前配置做精细化微调, 关闭时数值保持不变。",
                            )
                            .size(11.0)
                            .weak(),
                        );
                        ui.add_space(4.0);
                    }

                    /* A 波形 */
                    ui.label(
                        egui::RichText::new("A. 输出波形曲线 (数值越大越饱和, 100 = 线性)")
                            .size(12.0)
                            .strong(),
                    );
                    let a_rows = [
                        ("左马达曲线 %", 19, 30.0, 300.0),
                        ("右马达曲线 %", 20, 30.0, 300.0),
                    ];
                    for (name, idx, lo, hi) in a_rows {
                        let mut v = p[idx].load(Ordering::Relaxed) as f32;
                        if ui
                            .add_enabled(adv_on, egui::Slider::new(&mut v, lo..=hi).text(name))
                            .changed()
                        {
                            p[idx].store(v as u32, Ordering::Relaxed);
                        }
                    }
                    /* 输出平滑 (v22.3: 独立参数, 一阶低通抑制低频嗡嗡声) */
                    {
                        let mut v = self
                            .app_state
                            .vibration_out_smooth
                            .load(Ordering::Relaxed) as f32;
                        if ui
                            .add_enabled(
                                adv_on,
                                egui::Slider::new(&mut v, 0.0..=100.0)
                                    .text("输出平滑 % (抑制嗡嗡声, 高=柔)"),
                            )
                            .changed()
                        {
                            self.app_state
                                .vibration_out_smooth
                                .store(v as u32, Ordering::Relaxed);
                            self.config.vibration.out_smooth = v as u32;
                            let _ = self.config.save_vibration_to_file(crate::config::AppConfig::vibration_path_for("Config.toml"));
                        }
                    }
                    /* 输出低强度死区 (v29.4: ERM 转子马达启动区高死区, 低于归 0 消除转子嗡声) */
                    {
                        let mut v = self
                            .app_state
                            .vibration_out_threshold
                            .load(Ordering::Relaxed) as f32;
                        if ui
                            .add_enabled(
                                adv_on,
                                egui::Slider::new(&mut v, 0.0..=60.0)
                                    .text("输出低强度抑制 % (转子马达建议 25-35)"),
                            )
                            .changed()
                        {
                            self.app_state
                                .vibration_out_threshold
                                .store(v as u32, Ordering::Relaxed);
                            self.config.vibration.out_threshold = v as u32;
                            let _ = self.config.save_vibration_to_file(crate::config::AppConfig::vibration_path_for("Config.toml"));
                        }
                    }
                    /* 输出动态范围重映射 (v30, ERM/Xbox360: 非零必转, 轻反馈不被死区吞掉) */
                    {
                        let mut rm = self
                            .app_state
                            .vibration_remap_enabled
                            .load(Ordering::Relaxed);
                        if ui
                            .checkbox(&mut rm, "输出动态范围重映射 (Xbox360/ERM)")
                            .on_hover_text(
                                "把非零输出映射到 [最小输出, 100%] 区间:\n\
                                 任何非零反馈至少以最小输出驱动马达 (转子一定转起来),\n\
                                 相对强弱保留, 轻反馈不再被死区吞掉。",
                            )
                            .changed()
                        {
                            self.app_state
                                .vibration_remap_enabled
                                .store(rm, Ordering::Relaxed);
                            self.config.vibration.remap_enabled = rm;
                            let _ = self.config.save_vibration_to_file(crate::config::AppConfig::vibration_path_for("Config.toml"));
                        }
                        let mut rmn = self
                            .app_state
                            .vibration_remap_min
                            .load(Ordering::Relaxed) as f32;
                        if ui
                            .add_enabled(
                                adv_on && rm,
                                egui::Slider::new(&mut rmn, 10.0..=60.0)
                                    .text("重映射最小输出 % (与死区一致)"),
                            )
                            .changed()
                        {
                            self.app_state
                                .vibration_remap_min
                                .store(rmn as u32, Ordering::Relaxed);
                            self.config.vibration.remap_min = rmn as u32;
                            let _ = self.config.save_vibration_to_file(crate::config::AppConfig::vibration_path_for("Config.toml"));
                        }
                    }
                    /* 马达分工 (v30, Xbox360 风格: 轻反馈单马达, 重反馈双马达) */
                    {
                        let mut sp = self
                            .app_state
                            .vibration_split_enabled
                            .load(Ordering::Relaxed);
                        if ui
                            .checkbox(&mut sp, "马达分工 (Xbox360 风格)")
                            .on_hover_text(
                                "轻反馈 (峰值低于分界) 仅驱动主导马达 (转子声/功耗更低);\n\
                                 重反馈双马达满幅 (大马达低频重击 + 小马达高频细节)。",
                            )
                            .changed()
                        {
                            self.app_state
                                .vibration_split_enabled
                                .store(sp, Ordering::Relaxed);
                            self.config.vibration.split_enabled = sp;
                            let _ = self.config.save_vibration_to_file(crate::config::AppConfig::vibration_path_for("Config.toml"));
                        }
                        let mut spt = self
                            .app_state
                            .vibration_split_thr
                            .load(Ordering::Relaxed) as f32;
                        if ui
                            .add_enabled(
                                adv_on && sp,
                                egui::Slider::new(&mut spt, 20.0..=90.0)
                                    .text("分工分界 % (低于为轻反馈)"),
                            )
                            .changed()
                        {
                            self.app_state
                                .vibration_split_thr
                                .store(spt as u32, Ordering::Relaxed);
                            self.config.vibration.split_thr = spt as u32;
                            let _ = self.config.save_vibration_to_file(crate::config::AppConfig::vibration_path_for("Config.toml"));
                        }
                    }
                    /* 震动节流 (v24: 限定时间窗口内最大注入次数, 防狂震) */
                    {
                        if job_mode
                            && self.app_state.vibration_throttle_window.load(Ordering::Relaxed) > 0
                        {
                            ui.label(
                                egui::RichText::new("【当前职业专属】狂震节流: 窗口内只震几次, 杜绝狂震")
                                    .size(11.0)
                                    .color(th.accent_text)
                                    .strong(),
                            );
                        }
                        let mut tw = self
                            .app_state
                            .vibration_throttle_window
                            .load(Ordering::Relaxed) as f32;
                        if ui
                            .add_enabled(
                                adv_on,
                                egui::Slider::new(&mut tw, 0.0..=10000.0)
                                    .text("震动节流窗口 ms (0=禁用)"),
                            )
                            .changed()
                        {
                            self.app_state
                                .vibration_throttle_window
                                .store(tw as u32, Ordering::Relaxed);
                            self.config.vibration.throttle_window_ms = tw as u32;
                            let _ = self.config.save_vibration_to_file(crate::config::AppConfig::vibration_path_for("Config.toml"));
                        }
                        let mut tm = self
                            .app_state
                            .vibration_throttle_max
                            .load(Ordering::Relaxed) as f32;
                        if ui
                            .add_enabled(
                                adv_on,
                                egui::Slider::new(&mut tm, 1.0..=30.0)
                                    .text("窗口内最大震动次数"),
                            )
                            .changed()
                        {
                            self.app_state
                                .vibration_throttle_max
                                .store(tm as u32, Ordering::Relaxed);
                            self.config.vibration.throttle_max_hits = tm as u32;
                            let _ = self.config.save_vibration_to_file(crate::config::AppConfig::vibration_path_for("Config.toml"));
                        }
                        let mut tr = self
                            .app_state
                            .vibration_throttle_dense_ratio
                            .load(Ordering::Relaxed) as f32;
                        if ui
                            .add_enabled(
                                adv_on,
                                egui::Slider::new(&mut tr, 1.0..=100.0)
                                    .text("狂震期次数比例 % (自适应收紧, 100=不收紧)"),
                            )
                            .changed()
                        {
                            self.app_state
                                .vibration_throttle_dense_ratio
                                .store(tr as u32, Ordering::Relaxed);
                            self.config.vibration.throttle_dense_ratio = tr as u32;
                            let _ = self.config.save_vibration_to_file(crate::config::AppConfig::vibration_path_for("Config.toml"));
                        }
                    }
                    ui.add_space(4.0);
                    ui.separator();
                    ui.add_space(4.0);

                    /* A2 移动走路质感 (v20.2: 轻量步伐参数化) */
                    ui.label(
                        egui::RichText::new("A2. 移动走路质感 (轻量步伐, 不抢主震动)")
                            .size(12.0)
                            .strong(),
                    );
                    let m_rows = [
                        ("移动步频 ms (自然步频 ~380)", 23, 200.0, 800.0),
                        ("移动着地脉冲 % (柔和, 不宜高)", 24, 10.0, 100.0),
                        ("移动抬脚保持 % (极轻)", 25, 5.0, 50.0),
                        ("移动整体增益 % (轻音量)", 26, 10.0, 100.0),
                        ("移动平滑系数 % (越大过渡越柔, 消除嗡嗡声)", 27, 5.0, 100.0),
                        ("移动最低输出阈值 % (低于归0, 消除沙沙声)", 28, 0.0, 20.0),
                    ];
                    for (name, idx, lo, hi) in m_rows {
                        let mut v = p[idx].load(Ordering::Relaxed) as f32;
                        if ui
                            .add_enabled(adv_on, egui::Slider::new(&mut v, lo..=hi).text(name))
                            .changed()
                        {
                            p[idx].store(v as u32, Ordering::Relaxed);
                        }
                    }
                    ui.add_space(4.0);
                    ui.separator();
                    ui.add_space(4.0);

                    /* A3 移动积累增强 (v22): 长时间移动积累走位能量, 停手后窗口内攻击增强
                     * 走位型职业 (漫游/剑魂/刺客/影舞/决战者) 强化; 站桩职业弱化 */
                    ui.label(
                        egui::RichText::new("A3. 移动积累增强 (走位能量 → 下次攻击增强)")
                            .size(12.0)
                            .strong(),
                    );
                    if job_mode {
                        let m_rate = p[21].load(Ordering::Relaxed);
                        if m_rate >= 12 {
                            ui.label(
                                egui::RichText::new("【走位职业】移动积累快, 停手窗口内攻击增强明显")
                                    .size(11.0)
                                    .color(th.accent_text)
                                    .strong(),
                            );
                        } else if m_rate <= 3 {
                            ui.label(
                                egui::RichText::new("【站桩职业】移动积累慢, 增强弱")
                                    .size(11.0)
                                    .weak(),
                            );
                        }
                    }
                    let c_rows2 = [
                        ("移动积累速率 %/秒 (0=禁用)", 21, 0.0, 20.0),
                        ("攻击增强上限 % (最多 ×(1+上限))", 22, 0.0, 100.0),
                        ("增强窗口 ms (停手后有效)", 39, 0.0, 3000.0),
                    ];
                    for (name, idx, lo, hi) in c_rows2 {
                        let mut v = p[idx].load(Ordering::Relaxed) as f32;
                        if ui
                            .add_enabled(adv_on, egui::Slider::new(&mut v, lo..=hi).text(name))
                            .changed()
                        {
                            p[idx].store(v as u32, Ordering::Relaxed);
                        }
                    }
                    ui.add_space(4.0);
                    ui.separator();
                    ui.add_space(4.0);

                    /* A3 连击密度自适应 (v22): 短时间连击暴增窗口期自动降强度
                     * 高连击职业专属算法 (召唤/精灵骑士/剑魂/蓝拳等), 全职业页仅该职业显示 */
                    let d_thr = p[29].load(Ordering::Relaxed);
                    if job_mode && d_thr >= 100 {
                        ui.label(
                            egui::RichText::new("A4. 连击密度自适应: 该职业无此专属算法 (仅高连击职业启用)")
                                .size(11.0)
                                .weak(),
                        );
                    } else {
                        ui.label(
                            egui::RichText::new("A4. 连击密度自适应 (短时间连击暴增窗口期自动降强度, 防震手)")
                                .size(12.0)
                                .strong(),
                        );
                        if job_mode {
                            ui.label(
                                egui::RichText::new("【当前职业专属】高连击职业窗口期自适应降强度")
                                    .size(11.0)
                                    .color(th.accent_text)
                                    .strong(),
                            );
                        }
                        let d_rows = [
                            ("密度触发阈值 hits/窗口 (100=禁用)", 29, 5.0, 100.0),
                            ("密度检测窗口 ms", 30, 200.0, 1500.0),
                            ("窗口期降幅 % (强度×降幅)", 31, 0.0, 80.0),
                            ("恢复判定 ms (停手后恢复)", 32, 300.0, 3000.0),
                            ("最低保留 % (降幅上限)", 33, 30.0, 100.0),
                            ("恢复平滑 ms (0=立即)", 34, 0.0, 800.0),
                        ];
                        for (name, idx, lo, hi) in d_rows {
                            let mut v = p[idx].load(Ordering::Relaxed) as f32;
                            if ui
                                .add_enabled(adv_on, egui::Slider::new(&mut v, lo..=hi).text(name))
                                .changed()
                            {
                                p[idx].store(v as u32, Ordering::Relaxed);
                            }
                        }
                    }
                    ui.add_space(4.0);
                    ui.separator();
                    ui.add_space(4.0);

                    /* B 衰减 (原 C, 六类 L/R 比例已移至基础区每项权重) */
                    ui.label(
                        egui::RichText::new("B. 各类事件衰减时长 (越大震感越持久)")
                            .size(12.0)
                            .strong(),
                    );
                    let c_rows = [
                        ("普通命中衰减 ms", 35, 15.0, 150.0),
                        ("特殊攻击衰减 ms", 36, 15.0, 150.0),
                        ("受击衰减 ms", 37, 10.0, 150.0),
                        ("状态变化衰减 ms", 38, 15.0, 150.0),
                        ("装备特效衰减 ms", 39, 15.0, 200.0),
                    ];
                    for (name, idx, lo, hi) in c_rows {
                        let mut v = p[idx].load(Ordering::Relaxed) as f32;
                        if ui
                            .add_enabled(adv_on, egui::Slider::new(&mut v, lo..=hi).text(name))
                            .changed()
                        {
                            p[idx].store(v as u32, Ordering::Relaxed);
                        }
                    }
                    ui.add_space(4.0);
                    ui.separator();
                    ui.add_space(4.0);

                    /* D 时长窗口 */
                    ui.label(
                        egui::RichText::new("C. 持续/节奏/爆发/连击窗口时长")
                            .size(12.0)
                            .strong(),
                    );
                    let d_rows = [
                        ("DOT 持续反馈时长 ms", 40, 50.0, 1000.0),
                        ("装备特效节奏周期 ms", 41, 60.0, 600.0),
                        ("爆发窗口 ms (窗口内 6 连触发爆发)", 42, 100.0, 1000.0),
                        ("爆发模式最低强度 %", 43, 0.0, 100.0),
                        ("反击窗口 ms (受击后攻击强化)", 44, 300.0, 2000.0),
                        ("连击统计窗口 ms", 45, 500.0, 5000.0),
                        ("空闲判定 ms (回战斗前状态)", 46, 1000.0, 10000.0),
                    ];
                    for (name, idx, lo, hi) in d_rows {
                        let mut v = p[idx].load(Ordering::Relaxed) as f32;
                        if ui
                            .add_enabled(adv_on, egui::Slider::new(&mut v, lo..=hi).text(name))
                            .changed()
                        {
                            p[idx].store(v as u32, Ordering::Relaxed);
                        }
                    }
                    ui.add_space(4.0);
                    ui.separator();
                    ui.add_space(4.0);

                    /* E 连击/自适应 */
                    ui.label(
                        egui::RichText::new("D. 连击增强与命中自适应 (防叠加饱和)")
                            .size(12.0)
                            .strong(),
                    );
                    let e_rows = [
                        ("连击放大上限 x", 47, 100.0, 500.0),
                        ("连击增强斜率 %/百连", 48, 1.0, 300.0),
                        ("中断收尾阈值 连击数", 49, 10.0, 100.0),
                        ("自适应阈值 ms (低于此间隔开始减弱)", 50, 30.0, 300.0),
                        ("自适应降幅 %", 51, 0.0, 50.0),
                        ("自适应上限间隔 ms (超过则满强度)", 52, 100.0, 1000.0),
                        ("自适应最低强度 %", 53, 0.0, 100.0),
                    ];
                    for (name, idx, lo, hi) in e_rows {
                        let mut v = p[idx].load(Ordering::Relaxed) as f32;
                        if ui
                            .add_enabled(adv_on, egui::Slider::new(&mut v, lo..=hi).text(name))
                            .changed()
                        {
                            p[idx].store(v as u32, Ordering::Relaxed);
                        }
                    }
                    ui.add_space(4.0);
                    ui.separator();
                    ui.add_space(4.0);

                    /* F/G/H 脉冲与静默 */
                    ui.label(
                        egui::RichText::new("E. 静默/反击/脉冲/测试")
                            .size(12.0)
                            .strong(),
                    );
                    let f_rows = [
                        ("特殊攻击后静默 ms (突显暴击破招)", 54, 0.0, 500.0),
                        ("反击强化倍数 %", 55, 100.0, 300.0),
                        ("唤醒脉冲强度 %", 56, 0.0, 100.0),
                        ("里程碑脉冲强度 %", 57, 0.0, 100.0),
                        ("连击中断收尾脉冲 %", 58, 0.0, 100.0),
                        ("测试震动强度 %", 59, 1.0, 100.0),
                    ];
                    for (name, idx, lo, hi) in f_rows {
                        let mut v = p[idx].load(Ordering::Relaxed) as f32;
                        if ui
                            .add_enabled(adv_on, egui::Slider::new(&mut v, lo..=hi).text(name))
                            .changed()
                        {
                            p[idx].store(v as u32, Ordering::Relaxed);
                        }
                    }
                    ui.label(
                        egui::RichText::new(
                            "里程碑档位固定 50/100/200/400 连击。\n\
                             自适应: 低于阈值间隔的连续命中按降幅减弱, 防止叠加饱和。",
                        )
                        .size(11.0)
                        .weak(),
                    );
                });
                });
                ui.add_space(8.0);
                ui.separator();
                ui.add_space(8.0);

                
                ui.label(
                    egui::RichText::new(if job_mode {
                        "提示: 所有滑块实时生效; 全职业预设页修改会自动保存到 JobVibration.toml"
                    } else {
                        "提示: 所有滑块实时生效, 无需保存。连发功能在\"连发映射\"页。"
                    })
                        .size(11.0)
                        .weak(),
                );
            });
            /* 全职业预设页: 滑块修改后自动落盘 (快照检测) */
            if job_mode {
                if let Some((b, c)) = self.vib_job_active.clone() {
                    let changed = self.vib_job_snapshot.len() != 60
                        || (0..60).any(|i| {
                            self.app_state.vibration_params[i].load(Ordering::Relaxed) != self.vib_job_snapshot[i]
                        });
                    if changed {
                        self.vib_job_snapshot = (0..60)
                            .map(|i| self.app_state.vibration_params[i].load(Ordering::Relaxed))
                            .collect();
                        Self::save_job_vibration_state(&self.app_state, &b, &c);
                    }
                }
            }
    }


    /// 切换极简模式 (进入缩窗, 退出恢复原窗口尺寸/最大化状态)。
    /// 应用通用震动预设 (显式参数版: 供 vib_section 闭包内使用, 避免整体 &mut self 捕获)。
    /// 互斥: 若全职业预设开启, 先关闭并恢复默认。
    #[allow(clippy::too_many_arguments)]
    pub(super) fn apply_general_vibration_preset_in(
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
        let pr0 = match config.vibration_presets.iter().find(|x| x.name == name) {
            Some(x) => x.clone(),
            None => return,
        };
        let pr = crate::config::default_vibration_presets()
            .into_iter()
            .find(|x| x.name == pr0.name)
            .unwrap_or(pr0);
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
        let _ = config.save_vibration_to_file(crate::config::AppConfig::vibration_path_for("Config.toml"));
    }


    /// 关闭全职业预设 (显式参数版): 恢复默认 + 落盘 applied=false。
    pub(super) fn disable_job_vibration_preset_in(
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
    pub(super) fn apply_job_vibration_preset_in(
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
        app_state.vibration_abs_freq_enabled.store(cls.abs_freq_enabled, std::sync::atomic::Ordering::Relaxed);
        app_state.vibration_algo_id.store(cls.algo_id as u32, std::sync::atomic::Ordering::Relaxed);
        for (i, v) in cls.algo_params.iter().enumerate() {
            app_state.vibration_algo_ap[i].store(*v, std::sync::atomic::Ordering::Relaxed);
        }
        config.vibration.throttle_window_ms = cls.throttle_window;
        config.vibration.throttle_max_hits = cls.throttle_max;
        config.vibration.throttle_dense_ratio = cls.throttle_dense_ratio;
        config.vibration.abs_freq_enabled = cls.abs_freq_enabled;
        *job_snapshot = cls.params.to_vec();
        Self::save_job_vibration_state(app_state, base_job, cls.name);
        *job_enabled = true;
        *job_active = Some((base_job.to_string(), cls.name.to_string()));
    }


    /// 便捷包装 (极简模式等无借用冲突的调用点)。
    pub(super) fn apply_general_vibration_preset(&mut self, name: &str) {
        Self::apply_general_vibration_preset_in(
            name,
            &self.app_state,
            &mut self.config,
            &mut self.vib_job_enabled,
            &mut self.vib_job_active,
            &mut self.vib_job_loaded,
        );
    }


    pub(super) fn disable_job_vibration_preset(&mut self) {
        Self::disable_job_vibration_preset_in(
            &self.app_state,
            &mut self.config,
            &mut self.vib_job_enabled,
            &mut self.vib_job_active,
            &mut self.vib_job_loaded,
        );
    }


    pub(super) fn apply_job_vibration_preset(&mut self, base_job: &str, cls: &crate::job_presets::JobClass) {
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
