//! About dialog implementation.

use crate::gui::theme::Theme;
use crate::i18n::CachedTranslations;
use eframe::egui;

/// Renders the about dialog showing application information.
pub fn render_about_dialog(
    ctx: &egui::Context,
    dark_mode: bool,
    show_about_dialog: &mut bool,
    translations: &CachedTranslations,
) {
    let t = translations;
    // 配色统一由设计系统提供
    let th = Theme::new(dark_mode);
    let (dialog_bg, card_bg, accent_color, text_color, text_secondary, label_color, inspired_color) =
        (th.surface, th.card_alt, th.accent, th.text, th.text_weak, th.text_weak, th.hint);

    egui::Window::new("about_sorahk")
        .id(egui::Id::new("about_dialog_window"))
        .title_bar(false)
        .collapsible(false)
        .resizable(false)
        .fixed_size([500.0, 550.0])
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .frame(
            egui::Frame::window(&ctx.style())
                .fill(dialog_bg)
                .corner_radius(egui::CornerRadius::same(12)),
        )
        .show(ctx, |ui| {
            // Use a simpler layout without excessive centering
            ui.with_layout(egui::Layout::top_down(egui::Align::Center), |ui| {
                ui.add_space(20.0);

                // 主标题: 18px strong + 紫色强调
                ui.label(
                    egui::RichText::new("DfoVibration-Sorahk")
                        .size(18.0)
                        .strong()
                        .color(accent_color),
                );
                ui.add_space(6.0);
                ui.label(
                    egui::RichText::new("~ Auto Key Press Tool ~")
                        .size(14.0)
                        .italics()
                        .color(text_secondary),
                );
                ui.add_space(24.0);

                // Version card - simplified Frame
                ui.scope(|ui| {
                    egui::Frame::NONE
                        .fill(card_bg)
                        .corner_radius(12.0)
                        .inner_margin(egui::Margin::same(16))
                        .show(ui, |ui| {
                            ui.set_min_width(ui.available_width());
                            ui.with_layout(egui::Layout::top_down(egui::Align::Center), |ui| {
                                ui.label(
                                    egui::RichText::new(t.about_version())
                                        .size(16.0)
                                        .strong()
                                        .color(accent_color),
                                );
                                ui.add_space(12.0);
                                ui.label(
                                    egui::RichText::new(t.about_description_line1())
                                        .size(13.0)
                                        .color(text_secondary),
                                );
                                ui.label(
                                    egui::RichText::new(t.about_description_line2())
                                        .size(13.0)
                                        .color(text_secondary),
                                );
                            });
                        });
                });
                ui.add_space(22.0);

                // Info section - flattened layout
                ui.scope(|ui| {
                    ui.spacing_mut().item_spacing.y = 12.0;
                    ui.set_max_width(420.0);

                    // Use Grid for better performance
                    egui::Grid::new("about_info_grid")
                        .num_columns(2)
                        .spacing([12.0, 12.0])
                        .show(ui, |ui| {
                            // Author
                            ui.label(
                                egui::RichText::new(t.about_author())
                                    .size(14.0)
                                    .strong()
                                    .color(label_color),
                            );
                            ui.label(egui::RichText::new("118coder").size(14.0).color(text_color));
                            ui.end_row();

                            // GitHub
                            ui.label(
                                egui::RichText::new(t.about_github())
                                    .size(14.0)
                                    .strong()
                                    .color(label_color),
                            );
                            ui.hyperlink_to(
                                egui::RichText::new("https://github.com/118coder/DfoVibration")
                                    .size(14.0)
                                    .color(label_color),
                                "https://github.com/118coder/DfoVibration/tree/main",
                            );
                            ui.end_row();

                            // License
                            ui.label(
                                egui::RichText::new(t.about_license())
                                    .size(14.0)
                                    .strong()
                                    .color(label_color),
                            );
                            ui.label(
                                egui::RichText::new(t.about_mit_license())
                                    .size(14.0)
                                    .color(text_color),
                            );
                            ui.end_row();

                            // Built with
                            ui.label(
                                egui::RichText::new(t.about_built_with())
                                    .size(14.0)
                                    .strong()
                                    .color(label_color),
                            );
                            ui.label(
                                egui::RichText::new(t.about_rust_egui())
                                    .size(14.0)
                                    .color(text_color),
                            );
                            ui.end_row();
                        });
                });
                ui.add_space(24.0);

                // Inspired note
                ui.label(
                    egui::RichText::new(t.about_inspired())
                        .size(12.0)
                        .italics()
                        .color(inspired_color),
                );
                ui.add_space(20.0);

                // Close button
                if ui
                    .add_sized(
                        [200.0, 36.0],
                        egui::Button::new(
                            egui::RichText::new(t.error_close_button())
                                .size(15.0)
                                .color(egui::Color32::WHITE)
                                .strong(),
                        )
                        .fill(accent_color)
                        .corner_radius(12.0),
                    )
                    .clicked()
                {
                    *show_about_dialog = false;
                }
                ui.add_space(20.0);
            });
        });
}
