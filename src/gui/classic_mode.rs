//! 经典模式窗口 (v20.4, 用户定稿): 与极简模式同级的**独立窗口模式**。
//!
//! 布局/尺寸复刻老宿主 1.5 (`【90US】…DfoVibration 1.5版`, main.rs 里
//! `with_inner_size([820.0, 600.0])`): 单窗 = 标题栏(品牌+预设下拉+图标按钮组)
//! + 三页签「连发映射 / 通用型震动设定 / 全职业预设」。
//! 页签内容**全部复用现有渲染器** (turbo 页 / 通用震动页 / 全职业页), 零重复实现 ——
//! 连发区新功能 (方向/滚动/双击/预设切换键) 自动出现在经典模式。
//!
//! 入口: 完整模式标题栏【典】字按钮; 出口: 经典标题栏「⤺ 完整界面」。
//! 非 DFO 玩家 (dfo_player=false) 只显示「连发映射」页签。
//! 与极简模式互斥: 进入一方先退出另一方。

use super::main_window::FrameState;
use super::SorahkGui;
use crate::gui::theme;
use crate::gui::theme::Theme;
use crate::gui::widgets;
use eframe::egui;

/// 老宿主 1.5 的窗口尺寸按用户定稿调整为 **3:2 比例** (900×600):
/// 内容放不下时横向拉长, 竖向适当收一点, 比例保持 3:2。
pub(super) const CLASSIC_SIZE: egui::Vec2 = egui::vec2(900.0, 600.0);

impl SorahkGui {
    /// 进入/退出经典模式 (与 `set_minimal_mode` 同构: 采样完整窗矩形, 退出精确还原)。
    pub(super) fn set_classic_mode(&mut self, ctx: &egui::Context, on: bool) {
        self.classic_mode = on;
        self.config.classic_mode = on;
        if on {
            /* 与极简模式互斥: 进入经典即退极简 (双标记同真会让外壳分支打架) */
            self.minimal_mode = false;
            self.config.minimal_mode = false;
            /* 老宿主 min_inner_size 820×600: 进入时设最小尺寸 (可放大, 不逐帧钳死) */
            ctx.send_viewport_cmd(egui::ViewportCommand::MinInnerSize(CLASSIC_SIZE));
        }
        if !on {
            /* 解除经典的最小尺寸限制 (完整模式/极简模式自由缩放) */
            ctx.send_viewport_cmd(egui::ViewportCommand::MinInnerSize(egui::Vec2::ZERO));
            /* 退出经典: 经典矩形落盘 + 精确还原完整模式上次的窗口位置+尺寸 */
            if let Some((pos, sz)) = self.classic_window_rect {
                self.config.window_rect_classic = Some([pos.x, pos.y, sz.x, sz.y]);
            }
            let restore = self.normal_window_rect.take().or_else(|| {
                self.config
                    .window_rect_normal
                    .map(|r| (egui::pos2(r[0], r[1]), egui::vec2(r[2], r[3])))
            });
            if let Some((pos, sz)) = restore {
                self.config.window_rect_normal = Some([pos.x, pos.y, sz.x, sz.y]);
                ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(pos));
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(sz));
            } else if let Some(sz) = self.normal_window_size.take() {
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(sz));
            }
            if self.normal_was_maximized {
                ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(true));
            }
            self.normal_was_maximized = false;
            /* 清"已进入"标记: 否则再次进入时 is_none() 守卫短路, 退出恢复旧矩形
             * (与 set_minimal_mode 同一条注释的坑) */
            self.normal_window_size = None;
        }
        let _ = self.config.save_to_file("Config.toml");
    }

    /// 经典标题栏: 拖动区 + 品牌 + 预设快切 (老宿主同位) + 图标按钮组 + 返回完整界面。
    pub(super) fn render_classic_top_bar(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let th = self.theme();
        /* 拖动区先于按钮注册 (同完整顶栏, 反了按钮全灭) */
        self.render_title_bar_drag(ui, ctx);
        ui.horizontal_centered(|ui| {
            ui.add_space(2.0);
            let (rect, _) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
            ui.painter().rect_filled(rect, 5, th.accent);
            ui.label(
                egui::RichText::new("DfoVibration-V3")
                    .size(13.0)
                    .strong()
                    .family(Theme::font_bold())
                    .color(th.title),
            );
            ui.label(th.hint_text("经典模式"));
            ui.add_space(theme::SP_L);
            /* 老宿主: 预设下拉在标题栏 */
            self.render_preset_switch(ui);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                self.render_window_controls(ui, ctx);
                ui.add_space(theme::SP_S);
                /* 返回完整界面 */
                if ui
                    .add(
                        egui::Button::new(
                            egui::RichText::new("⤺ 完整界面")
                                .size(11.5)
                                .strong()
                                .color(egui::Color32::WHITE),
                        )
                        .fill(th.btn_primary)
                        .corner_radius(egui::CornerRadius::same(theme::RADIUS_CTRL))
                        .min_size(egui::vec2(0.0, 26.0)),
                    )
                    .on_hover_text("返回完整模式 (左侧导航五页)")
                    .clicked()
                {
                    self.set_classic_mode(ctx, false);
                }
                ui.add_space(theme::SP_XS);
                if widgets::icon_button(ui, &th, widgets::Icon::Info, "关于").clicked() {
                    self.show_about_dialog = true;
                }
                if widgets::icon_button(ui, &th, widgets::Icon::Devices, "设备").clicked() {
                    self.show_device_manager = true;
                }
                if widgets::icon_button(ui, &th, widgets::Icon::Question, "使用说明 / 重新选择向导")
                    .clicked()
                {
                    self.first_run_rerun = true;
                    self.dfo_ask_answered = false;
                    self.show_edition_ask = false;
                    self.show_guide = false;
                    self.modal_defer = 0;
                }
                if widgets::icon_button(ui, &th, widgets::Icon::Theme, "切换主题").clicked() {
                    self.dark_mode = !self.dark_mode;
                    self.config.dark_mode = self.dark_mode;
                    let _ = self.config.save_to_file("Config.toml");
                    if let Some(temp_config) = &mut self.temp_config {
                        temp_config.dark_mode = self.dark_mode;
                    }
                }
                /* ★v20.5: 共用打开入口 (自带 temp_config 快照/暂停前置, 缺了会崩) */
                if widgets::icon_button(ui, &th, widgets::Icon::Gear, "设置").clicked() {
                    self.open_settings_dialog();
                }
                /* ★v20.5: 经典窗口也能切极简 (互斥逻辑在 set_minimal_mode 内) */
                if ui
                    .add(
                        egui::Button::new(
                            egui::RichText::new("简")
                                .size(14.0)
                                .strong()
                                .color(th.btn_secondary_text),
                        )
                        .fill(th.btn_secondary)
                        .corner_radius(egui::CornerRadius::same(theme::RADIUS_CTRL))
                        .min_size(egui::vec2(30.0, 26.0)),
                    )
                    .on_hover_text("极简模式: 只保留连发/震动两个开关的小窗")
                    .clicked()
                {
                    self.set_minimal_mode(ctx, true);
                }
            });
        });
    }

    /// 经典页签行 (老宿主为标题栏下三颗 110×26 圆角按钮; ★v20.5 补齐 手柄映射/白名单,
    /// 与完整版五页互通 —— 内容全部复用同一渲染器, 改一处两边同步)。
    pub(super) fn render_classic_tabs(&mut self, ui: &mut egui::Ui) {
        let th = self.theme();
        /* 页签定义: (稳定索引, 名称)。1/2 为震动页, 非 DFO 玩家隐藏 (与侧边栏过滤一致) */
        const TAB_NAMES: [&str; 5] = [
            "连发映射",
            "通用型震动设定",
            "全职业预设",
            "手柄映射",
            "白名单",
        ];
        let tabs: Vec<(usize, &str)> = TAB_NAMES
            .iter()
            .enumerate()
            .filter(|&(i, _)| {
                /* ★v24.4: 「手柄映射」(i=3) 已从经典模式移除 (用户要求: 经典就应该经典);
                 * 手柄映射仍在完整模式侧边栏可用。1/2 为震动页, 非 DFO 玩家隐藏。 */
                i != 3 && (self.config.dfo_player || (i != 1 && i != 2))
            })
            .map(|(i, n)| (i, *n))
            .collect();
        if !tabs.iter().any(|(i, _)| *i == self.classic_tab) {
            self.classic_tab = 0;
        }
        ui.horizontal(|ui| {
            ui.add_space(2.0);
            for (idx, name) in &tabs {
                let selected = self.classic_tab == *idx;
                let btn = egui::Button::new(
                    egui::RichText::new(*name)
                        .size(12.5)
                        .strong()
                        .color(if selected {
                            egui::Color32::WHITE
                        } else {
                            th.btn_secondary_text
                        }),
                )
                .fill(if selected { th.btn_primary } else { th.faint })
                .corner_radius(egui::CornerRadius::same(theme::RADIUS_CTRL))
                .min_size(egui::vec2(110.0, 26.0));
                if ui.add(btn).clicked() {
                    self.classic_tab = *idx;
                }
                ui.add_space(theme::SP_XS);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                /* ★v24.10: 「仅用连发」不显示震动相关文案 */
                if self.config.dfo_player {
                    ui.label(
                        egui::RichText::new("震动参数实时生效, 进图自动驱动")
                            .size(11.0)
                            .color(th.hint),
                    );
                }
            });
        });
    }

    /// ★v24.4: 经典模式 · 连发映射页的震动快捷条 (置于「预设管理」上方, 用户要求)。
    ///
    /// 震动 开/关 + 震动预设切换/应用 + 一键跳到「通用型震动设定」—— 不用切页就能调震动。
    /// 预设数据与应用逻辑复用震动页那一套 (`apply_general_vibration_preset`), 两处永远一致。
    pub(super) fn render_classic_vib_quickbar(&mut self, ui: &mut egui::Ui) {
        let th = self.theme();
        /* ★v24.10: 「仅用连发」(dfo_player=false) 时整条隐藏 —— 用户要求:
         * 不用震动功能的人不该看到这块, 功能也默认关闭。 */
        if !self.config.dfo_player {
            return;
        }
        let entries = crate::config::visible_preset_entries(
            &self.config.vibration_presets,
            self.config.vib_legacy_client,
        );
        if entries.is_empty() {
            return;
        }
        /* 选择始终落在可见条目上 (与震动页同一约定: 过滤后真实下标) */
        let sel_pos = entries
            .iter()
            .position(|(real, _)| *real == self.vib_preset_idx)
            .unwrap_or(0);
        if let Some((real, _)) = entries.get(sel_pos) {
            self.vib_preset_idx = *real;
        }
        let cur_name = entries[sel_pos].1.clone();
        let mut next_pos = sel_pos;
        let mut apply_name: Option<String> = None;
        let mut goto_tune = false;
        th.card(ui, None, |ui| {
            ui.horizontal(|ui| {
                let vib_on = self
                    .app_state
                    .vibration_enabled
                    .load(std::sync::atomic::Ordering::Relaxed);
                let (vtext, vcolor, vbg) = if vib_on {
                    ("震动: 开", th.good, th.good_soft)
                } else {
                    ("震动: 关", th.bad, th.bad_soft)
                };
                if ui.add(th.status_pill(vtext, vcolor, vbg)).clicked() {
                    let v = self
                        .app_state
                        .vibration_enabled
                        .load(std::sync::atomic::Ordering::Relaxed);
                    self.app_state
                        .vibration_enabled
                        .store(!v, std::sync::atomic::Ordering::Relaxed);
                }
                ui.add_space(theme::SP_S);
                ui.label(th.weak("震动预设"));
                egui::ComboBox::from_id_salt("classic_vib_quick_preset")
                    .selected_text(cur_name.clone())
                    .width(150.0)
                    .show_ui(ui, |ui| {
                        for (pos, (_, n)) in entries.iter().enumerate() {
                            if ui.selectable_label(pos == sel_pos, n).clicked() {
                                next_pos = pos;
                            }
                        }
                    });
                if ui.add(th.secondary_button("应用")).clicked() {
                    apply_name = Some(entries[next_pos].1.clone());
                }
                ui.add_space(theme::SP_S);
                if ui.add(th.secondary_button("震动调校 →")).clicked() {
                    goto_tune = true;
                }
                ui.label(th.hint_text("与「通用型震动设定」同一套预设, 应用后实时生效"));
            });
        });
        if let Some((real, _)) = entries.get(next_pos) {
            self.vib_preset_idx = *real;
        }
        if let Some(name) = apply_name {
            self.apply_general_vibration_preset(&name);
        }
        if goto_tune {
            self.classic_tab = 1;
        }
    }

    /// 经典模式内容区 (按页签分发; 全部复用现有页面渲染器 —— 与完整版互通)。
    pub(super) fn render_classic_content(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        frame_state: &FrameState,
    ) {
        match self.classic_tab {
            1 => self.render_vibration_full_page(ui, frame_state, false),
            2 => self.render_vibration_full_page(ui, frame_state, true),
            3 => self.render_gamepad_page(ui, ctx),
            4 => self.render_whitelist_page(ui),
            /* 连发映射: 状态 hero + 预设管理(含切换键) + 映射列表 + 可编辑全局配置 */
            _ => self.render_turbo_page(ui, ctx, frame_state),
        }
    }
}
