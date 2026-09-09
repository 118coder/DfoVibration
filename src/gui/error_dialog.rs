//! Error dialog for displaying critical errors.

use crate::gui::fonts;
use crate::gui::utils::create_icon;
use crate::i18n::{CachedTranslations, Language};
use eframe::egui;

/// Error dialog structure for displaying configuration errors.
struct ErrorDialog {
    /// Error message text
    error_msg: String,
    /// Cached translations
    translations: CachedTranslations,
}

impl eframe::App for ErrorDialog {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let t = &self.translations;

        // 配色统一由设计系统提供
        let th = crate::gui::theme::Theme::dark();
        ctx.set_style(crate::gui::theme::build_style(true));

        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(th.bg))
            .show(ctx, |ui| {
                ui.add_space(20.0);

                // 标题: 18px strong + 紫色强调
                ui.vertical_centered(|ui| {
                    ui.label(
                        egui::RichText::new(t.error_title())
                            .size(18.0)
                            .color(th.accent)
                            .strong(),
                    );
                });

                ui.add_space(20.0);

                // Error message card
                egui::Frame::NONE
                    .fill(th.card)
                    .corner_radius(egui::CornerRadius::same(14))
                    .inner_margin(egui::Margin::same(16))
                    .show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        ui.label(
                            egui::RichText::new(&self.error_msg)
                                .size(14.0)
                                .color(th.bad),
                        );
                    });

                ui.add_space(20.0);

                // Close button
                ui.vertical_centered(|ui| {
                    let close_btn = egui::Button::new(
                        egui::RichText::new(t.error_close_button())
                            .size(16.0)
                            .color(egui::Color32::WHITE),
                    )
                    .fill(th.btn_primary)
                    .corner_radius(12.0);

                    if ui.add_sized([140.0, 36.0], close_btn).clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });

                ui.add_space(10.0);
            });
    }
}

/// Displays an error dialog in a separate window.
///
/// # Errors
///
/// Returns an error if the GUI framework fails to initialize.
pub fn show_error(error_msg: &str) -> anyhow::Result<()> {
    let icon = create_icon();
    let viewport = egui::ViewportBuilder::default()
        .with_inner_size([450.0, 280.0])
        .with_resizable(false)
        .with_title("Sorahk - Error")
        .with_icon(icon)
        .with_always_on_top();

    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    // Error dialog always uses English for better compatibility
    let language = Language::English;

    eframe::run_native(
        "Sorahk Error",
        options,
        Box::new(move |cc| {
            // Load fonts for proper text rendering
            fonts::load_fonts(&cc.egui_ctx, language);

            Ok(Box::new(ErrorDialog {
                error_msg: error_msg.to_string(),
                translations: CachedTranslations::new(language),
            }))
        }),
    )
    .map_err(|e| anyhow::anyhow!("Failed to show error dialog: {}", e))
}
