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
    /// 首次运行 · 第 1 弹: 询问是否 DFO 玩家 (决定震动入口显示; 答完进入使用说明)。
    pub(super) fn render_dfo_ask_window(&mut self, ctx: &egui::Context) {
        let th = self.theme();
        /* ★v19: 窗口 ID 必须唯一 —— 三个向导弹窗曾共用 ID " ", 同帧弹出的新窗口
         * 按钮与刚点击的按钮 ID 重合, 一次点击被吃两遍 (选完 DFO 直接跳过版本询问) */
        egui::Window::new("dfo_ask_window")
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .default_width(480.0)
            .show(ctx, |ui| {
                egui::Frame::NONE
                    .fill(th.card)
                    .stroke(egui::Stroke::new(1.0, th.stroke))
                    .corner_radius(egui::CornerRadius::same(theme::RADIUS_CARD))
                    .inner_margin(egui::Margin::same(24))
                    .show(ui, |ui| {
                        ui.set_min_width(420.0);
                        ui.vertical_centered(|ui| {
                            ui.label(
                                egui::RichText::new("❓ 你是 DFO (地下城与勇士) 玩家吗?")
                                    .size(19.0)
                                    .strong()
                                    .family(Theme::font_bold())
                                    .color(th.title),
                            );
                            ui.add_space(theme::SP_M);
                        });
                        ui.label(th.hint_text(
                            "选「是」: 显示「通用震动」与「全职业预设」两个页面 —— 游戏内攻击/受击/评分\n等战斗事件会实时驱动手柄马达震动, 震动参数与职业预设均可在页内调校。",
                        ));
                        ui.add_space(theme::SP_XS);
                        ui.label(th.hint_text("选「不是」: 仅保留连发功能, 界面更清爽。"));
                        ui.add_space(theme::SP_M);
                        ui.vertical_centered(|ui| {
                            let yes =
                                ui.add_sized([240.0, 34.0], th.primary_button("🎮 是, 我是 DFO 玩家"));
                            ui.add_space(theme::SP_XS);
                            let no =
                                ui.add_sized([240.0, 34.0], th.secondary_button("⌨️ 不是, 仅用连发"));
                            if yes.clicked() || no.clicked() {
                                self.config.dfo_player = yes.clicked();
                                self.dfo_ask_answered = true;
                                /* ★v24.10: 选「仅用连发」→ 震动功能一并默认关闭 (用户要求),
                                 * 而不是只藏入口却让震动仍在后台驱动 */
                                if !self.config.dfo_player {
                                    self.app_state.vibration_enabled.store(
                                        false,
                                        std::sync::atomic::Ordering::Relaxed,
                                    );
                                    self.config.vibration.enabled = false;
                                    if matches!(
                                        self.active_page,
                                        Page::Vibration | Page::JobPresets
                                    ) {
                                        self.active_page = Page::Gamepad;
                                    }
                                }
                                let _ = self.config.save_to_file("Config.toml");
                                /* DFO 玩家 → 第 1.5 弹问客户端版本 (S1 ACT / S4+ 新版);
                                 * 非 DFO 玩家 (无震动) → 直接进第 2 弹使用说明 */
                                if self.config.dfo_player {
                                    self.show_edition_ask = true;
                                    self.modal_defer = 1; /* 防同帧点击穿透 */
                                } else {
                                    self.show_guide = true;
                                    self.modal_defer = 1;
                                }
                            }
                            ui.add_space(theme::SP_XS);
                            ui.label(th.hint_text("该选择随时可在「设置 → DFO 震动功能」中更改"));
                        });
                    });
            });
    }


    /// 首次运行 · 第 1.5 弹 (仅 DFO 玩家): 询问客户端版本, 决定震动引擎路线。
    /// S1 ACT → 老方案 (配老版 DLL 的事件语义); S4+ 新版 → 现行新方案。设置里可随时切换。
    pub(super) fn render_edition_ask_window(&mut self, ctx: &egui::Context) {
        let th = self.theme();
        egui::Window::new("edition_ask_window")
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .default_width(480.0)
            .show(ctx, |ui| {
                egui::Frame::NONE
                    .fill(th.card)
                    .stroke(egui::Stroke::new(1.0, th.stroke))
                    .corner_radius(egui::CornerRadius::same(theme::RADIUS_CARD))
                    .inner_margin(egui::Margin::same(24))
                    .show(ui, |ui| {
                        ui.set_min_width(420.0);
                        ui.vertical_centered(|ui| {
                            ui.label(
                                egui::RichText::new("🕹️ 你玩的是哪个版本的 DFO?")
                                    .size(19.0)
                                    .strong()
                                    .family(Theme::font_bold())
                                    .color(th.title),
                            );
                            ui.add_space(theme::SP_M);
                        });
                        ui.label(th.hint_text(
                            "选「S1 ACT 老版本」: 使用老版专用采集 DLL 的震动方案 (通道保持震/\n跳字脉冲/量纲归一), 配合老客户端的事件节奏校调。",
                        ));
                        ui.add_space(theme::SP_XS);
                        ui.label(th.hint_text(
                            "选「S4 之后的新版本」: 使用现行震动方案 —— 清脆、无持续干扰,\n为本版本客户端的事件流校调 (推荐/默认)。",
                        ));
                        ui.add_space(theme::SP_M);
                        ui.vertical_centered(|ui| {
                            let s1 = ui.add_sized(
                                [260.0, 34.0],
                                th.primary_button("🕹️ S1 ACT 老版本 (2008 客户端)"),
                            );
                            ui.add_space(theme::SP_XS);
                            let s4 = ui.add_sized(
                                [260.0, 34.0],
                                th.secondary_button("🆕 S4 之后的新版本 (推荐)"),
                            );
                            if s1.clicked() || s4.clicked() {
                                let from = self.config.vib_legacy_client;
                                let to = s1.clicked();
                                self.config.vib_edition_asked = true;
                                /* ★v24.13: 换客户端路线 = 换**整套**震动参数 (S1/S4 各一套, 互不污染);
                                 * 目标路线首次使用则套该路线内置默认预设。 */
                                self.switch_vibration_edition(from, to);
                                let _ = self.config.save_to_file("Config.toml");
                                self.show_edition_ask = false;
                                /* 选完进入第 2 弹: 使用说明 (延迟一帧防点击穿透) */
                                self.show_guide = true;
                                self.modal_defer = 1;
                            }
                            ui.add_space(theme::SP_XS);
                            ui.label(th.hint_text("该选择随时可在「设置 → DFO 震动功能」中切换"));
                        });
                    });
            });
    }

    /// 首次运行 · 第 2 弹 / 标题栏「?」: 使用说明 (快速上手 + 基础功能)。
    pub(super) fn render_guide_window(&mut self, ctx: &egui::Context) {
        let th = self.theme();
        egui::Window::new("guide_window")
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .default_width(600.0)
            .show(ctx, |ui| {
                egui::Frame::NONE
                    .fill(th.card)
                    .stroke(egui::Stroke::new(1.0, th.stroke))
                    .corner_radius(egui::CornerRadius::same(theme::RADIUS_CARD))
                    .inner_margin(egui::Margin::same(24))
                    .show(ui, |ui| {
                        ui.set_min_width(520.0);
                        ui.vertical_centered(|ui| {
                            ui.label(
                                egui::RichText::new("🎮 欢迎使用 DfoVibration-V3")
                                    .size(22.0)
                                    .strong()
                                    .family(Theme::font_bold())
                                    .color(th.title),
                            );
                            ui.add_space(theme::SP_XS);
                            ui.label(th.hint_text("快速上手 · 所有修改实时生效, 无需手动保存 · 标题栏「?」可随时重跑本向导 (重选 DFO / 版本)"));
                            ui.add_space(theme::SP_M);
                        });
                        let steps: Vec<(&str, &str)> = vec![
                            ("① 连发映射 — 核心功能", "「+ 新增映射」把键盘/鼠标/手柄按键连发成任意目标键; 切换热键 DELETE 一键启停"),
                            ("② 手柄映射 — 即点即用", "直接点手柄图上的按键绑定目标键, 修改立即生效; 已配置按键列在下方, 点击可快速跳转编辑"),
                            ("③ 极简模式 — 小窗挂机", "顶栏「简」一键切到只保留连发/震动开关的小窗, 「完整界面」随时返回"),
                            ("④ 托盘常驻 — 关窗不退出", "点 × 最小化到托盘继续运行; 托盘图标右键可暂停/打开/退出"),
                        ];
                        for (t, d) in &steps {
                            ui.label(
                                egui::RichText::new(*t)
                                    .size(14.0)
                                    .strong()
                                    .family(Theme::font_bold())
                                    .color(th.accent_text),
                            );
                            ui.label(th.hint_text(*d));
                            ui.add_space(theme::SP_XS);
                        }
                        ui.add_space(theme::SP_M);
                        ui.vertical_centered(|ui| {
                            ui.label(th.hint_text(
                                "第三方手柄连不上? 先到「设置旁的手柄图标」里点「重置手柄」重新握手;\n非 XInput 手柄在手柄映射页点任意按键即可走 Raw Input 激活流程。",
                            ));
                            ui.add_space(theme::SP_XS);
                            if ui.add_sized([180.0, 34.0], th.primary_button("开始使用")).clicked() {
                                self.config.guide_seen = true;
                                self.first_run_rerun = false;
                                self.show_guide = false;
                                let _ = self.config.save_to_file("Config.toml");
                            }
                            ui.add_space(theme::SP_XS);
                            ui.label(th.hint_text("随时可在「设置 → DFO 震动功能」更改 DFO 选择"));
                        });
                    });
            });
    }

}
