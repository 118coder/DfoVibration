//! 独立测试开关 (v24.17) + vib_section 分区包装 —— 原 vibration_page.rs 399-463, 2026-09-27 架构重构 C1 归位。

use crate::gui::SorahkGui;
use crate::gui::theme::Theme;

use eframe::egui;

impl SorahkGui {
    /// 【调试模式】独立测试开关 (v24.2)。
    ///
    /// 开 = 评分/移动通道独立于全局总调整 (单通道调试用, 方便逐项验证);
    /// 关 = 全局总调整统一应用到所有通道 (正常使用)。
    /// 不影响各卡片自己的测试按钮与滑块。状态持久化到 Config.toml。
    pub(in crate::gui) fn render_independent_test_toggle(
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
    pub(in crate::gui) fn vib_section(
        ui: &mut egui::Ui,
        dark: bool,
        title: &str,
        body: impl FnOnce(&mut egui::Ui),
    ) {
        Theme::new(dark).panel(ui, Some(title), body);
        ui.add_space(10.0);
    }
}
