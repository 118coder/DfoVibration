//! Mouse direction selection dialog.

use crate::i18n::CachedTranslations;
use crate::gui::theme::Theme;
use eframe::egui;

/// Mouse direction selection dialog
pub struct MouseDirectionDialog {
    selected_direction: Option<String>,
}

impl MouseDirectionDialog {
    pub fn new() -> Self {
        Self {
            selected_direction: None,
        }
    }

    /// Get the selected direction and consume the dialog
    pub fn get_selected_direction(&self) -> Option<String> {
        self.selected_direction.clone()
    }

    /// Render the dialog, returns true if should close
    pub fn render(
        &mut self,
        ctx: &egui::Context,
        dark_mode: bool,
        translations: &CachedTranslations,
    ) -> bool {
        let t = translations;

        // 现代化主题: 暗色 #11131C/#232636, 亮色 #FFFFFF/#EEEFF6, 强调紫 #7C8CF8/#6C5CE7
        let (bg_color, title_color, button_bg, button_hover_bg, text_color, text_hover_color) =
            if dark_mode {
                (
                    egui::Color32::from_rgb(20, 22, 28),     // 对话框背景 #11131C
                    egui::Color32::from_rgb(196, 181, 253),  // 标题强调紫 #7C8CF8
                    egui::Color32::from_rgb(41, 45, 57),     // 按钮底
                    egui::Color32::from_rgb(51, 56, 70),     // 按钮悬停
                    egui::Color32::from_rgb(237, 238, 240),  // 高对比文字
                    egui::Color32::from_rgb(255, 255, 255),  // 悬停文字
                )
            } else {
                (
                    egui::Color32::from_rgb(255, 255, 255),  // 对话框背景 #FFFFFF
                    egui::Color32::from_rgb(124, 58, 237),   // 标题强调紫 #6C5CE7
                    egui::Color32::from_rgb(231, 233, 238),  // 按钮底
                    egui::Color32::from_rgb(208, 213, 224),  // 按钮悬停
                    egui::Color32::from_rgb(74, 80, 96),     // 高对比文字
                    egui::Color32::from_rgb(22, 24, 31),     // 悬停文字
                )
            };

        let mut should_close = false;

        /* 内联 egui::Window 样板 (照 about_dialog 同款, 已验证渲染正常)。
         * 勿改回 Theme::modal_window 构建器 —— 该构建器有点状塌缩 bug (HANDOFF 第 17 条,
         * 本弹窗 2026-09-10 用户实测复现: 弹窗塌成 ~24px 小圆点), 修复前禁用。 */
        egui::Window::new("mouse_direction_dialog")
            .id(egui::Id::new("mouse_direction_dialog_window"))
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .fixed_size([380.0, 380.0])
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .frame(
                egui::Frame::window(&ctx.style())
                    .fill(bg_color)
                    .corner_radius(egui::CornerRadius::same(16))
                    .stroke(egui::Stroke::NONE)
                    .shadow(egui::epaint::Shadow {
                        offset: [0, 8],
                        blur: 24,
                        spread: 0,
                        color: egui::Color32::from_rgba_premultiplied(0, 0, 0, 60),
                    }),
            )
            .show(ctx, |ui| {
                ui.with_layout(egui::Layout::top_down(egui::Align::Center), |ui| {
                    ui.add_space(20.0);

                    // 标题: 18px strong + 紫色强调
                    ui.label(
                        egui::RichText::new(t.mouse_move_direction_label())
                            .size(18.0)
                            .strong()
                            .color(title_color),
                    );

                    ui.add_space(25.0);

                    // Direction grid - centered
                    ui.horizontal(|ui| {
                        ui.add_space((ui.available_width() - (100.0 * 3.0 + 8.0 * 2.0)) / 2.0);
                        egui::Grid::new("mouse_direction_grid_dialog")
                            .spacing([8.0, 8.0])
                            .show(ui, |ui| {
                                // Row 1: Up-Left, Up, Up-Right
                                if render_direction_button(
                                    ui,
                                    t.mouse_move_up_left(),
                                    dark_mode,
                                    button_bg,
                                    button_hover_bg,
                                    text_color,
                                    text_hover_color,
                                )
                                .clicked()
                                {
                                    self.selected_direction = Some("MOUSE_UP_LEFT".to_string());
                                    should_close = true;
                                }
                                if render_direction_button(
                                    ui,
                                    t.mouse_move_up(),
                                    dark_mode,
                                    button_bg,
                                    button_hover_bg,
                                    text_color,
                                    text_hover_color,
                                )
                                .clicked()
                                {
                                    self.selected_direction = Some("MOUSE_UP".to_string());
                                    should_close = true;
                                }
                                if render_direction_button(
                                    ui,
                                    t.mouse_move_up_right(),
                                    dark_mode,
                                    button_bg,
                                    button_hover_bg,
                                    text_color,
                                    text_hover_color,
                                )
                                .clicked()
                                {
                                    self.selected_direction = Some("MOUSE_UP_RIGHT".to_string());
                                    should_close = true;
                                }
                                ui.end_row();

                                // Row 2: Left, (Center), Right
                                if render_direction_button(
                                    ui,
                                    t.mouse_move_left(),
                                    dark_mode,
                                    button_bg,
                                    button_hover_bg,
                                    text_color,
                                    text_hover_color,
                                )
                                .clicked()
                                {
                                    self.selected_direction = Some("MOUSE_LEFT".to_string());
                                    should_close = true;
                                }

                                // Center mouse icon
                                let (rect, _response) = ui.allocate_exact_size(
                                    egui::vec2(100.0, 60.0),
                                    egui::Sense::hover(),
                                );
                                ui.painter().text(
                                    rect.center(),
                                    egui::Align2::CENTER_CENTER,
                                    "🖱",
                                    egui::FontId::proportional(32.0),
                                    text_color,
                                );

                                if render_direction_button(
                                    ui,
                                    t.mouse_move_right(),
                                    dark_mode,
                                    button_bg,
                                    button_hover_bg,
                                    text_color,
                                    text_hover_color,
                                )
                                .clicked()
                                {
                                    self.selected_direction = Some("MOUSE_RIGHT".to_string());
                                    should_close = true;
                                }
                                ui.end_row();

                                // Row 3: Down-Left, Down, Down-Right
                                if render_direction_button(
                                    ui,
                                    t.mouse_move_down_left(),
                                    dark_mode,
                                    button_bg,
                                    button_hover_bg,
                                    text_color,
                                    text_hover_color,
                                )
                                .clicked()
                                {
                                    self.selected_direction = Some("MOUSE_DOWN_LEFT".to_string());
                                    should_close = true;
                                }
                                if render_direction_button(
                                    ui,
                                    t.mouse_move_down(),
                                    dark_mode,
                                    button_bg,
                                    button_hover_bg,
                                    text_color,
                                    text_hover_color,
                                )
                                .clicked()
                                {
                                    self.selected_direction = Some("MOUSE_DOWN".to_string());
                                    should_close = true;
                                }
                                if render_direction_button(
                                    ui,
                                    t.mouse_move_down_right(),
                                    dark_mode,
                                    button_bg,
                                    button_hover_bg,
                                    text_color,
                                    text_hover_color,
                                )
                                .clicked()
                                {
                                    self.selected_direction = Some("MOUSE_DOWN_RIGHT".to_string());
                                    should_close = true;
                                }
                                ui.end_row();
                            });
                    });

                    ui.add_space(20.0);

                    // 取消按钮: 强调紫填充 + 圆角12 + 高度36
                    if ui
                        .add_sized(
                            [180.0, 36.0],
                            egui::Button::new(
                                egui::RichText::new(t.cancel_close_button())
                                    .size(14.0)
                                    .color(egui::Color32::WHITE),
                            )
                            .fill(title_color)
                            .corner_radius(12.0),
                        )
                        .clicked()
                    {
                        should_close = true;
                    }

                    ui.add_space(15.0);
                });
            });

        should_close
    }
}

/// Renders a direction button with consistent styling
fn render_direction_button(
    ui: &mut egui::Ui,
    label: &str,
    dark_mode: bool,
    button_bg: egui::Color32,
    button_hover_bg: egui::Color32,
    text_color: egui::Color32,
    text_hover_color: egui::Color32,
) -> egui::Response {
    let (desired_size, corner_radius) = ([100.0, 60.0], 12.0);
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(desired_size[0], desired_size[1]),
        egui::Sense::click(),
    );

    if ui.is_rect_visible(rect) {
        let is_hovered = response.hovered();
        let is_pressed = response.is_pointer_button_down_on();

        let bg_color = if is_pressed {
            Theme::new(dark_mode).accent_soft
        } else if is_hovered {
            button_hover_bg
        } else {
            button_bg
        };

        let fg_color = if is_hovered || is_pressed {
            text_hover_color
        } else {
            text_color
        };

        ui.painter().rect_filled(rect, corner_radius, bg_color);

        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            label,
            egui::FontId::proportional(13.0),
            fg_color,
        );
    }

    response
}
