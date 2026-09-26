//! 通用宏管理卡 (v24.32 序列公共片段) —— 原 1875-1982, C4 归位。

use crate::gui::SorahkGui;
use crate::gui::theme::{self, truncate_chars};

use eframe::egui;
impl SorahkGui {
    /// ★v24.32 通用宏管理卡: 序列文本里用 宏(名字) 引用的公共片段。
    pub(in crate::gui) fn render_universal_macros_card(&mut self, ui: &mut egui::Ui) {
        let th = self.theme();
        th.card(ui, Some("通用宏"), |ui| {
            ui.label(th.hint_text(
                "公共的序列片段: 在任意映射的「序列宏」里用 宏(名字) 引用 (名字不分大小写)",
            ));
            ui.add_space(theme::SP_S);

            let mut to_remove: Option<usize> = None;
            for (i, m) in self.config.universal_macros.iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(format!("宏({})", m.name))
                            .size(12.0)
                            .strong()
                            .color(th.accent_text),
                    );
                    ui.label(th.hint_text(truncate_chars(&m.text, 44)));
                    ui.with_layout(
                        egui::Layout::right_to_left(egui::Align::Center),
                        |ui| {
                            if ui
                                .add_sized(
                                    [26.0, 22.0],
                                    egui::Button::new(
                                        egui::RichText::new("✕").size(12.0).color(th.text_weak),
                                    )
                                    .fill(th.faint)
                                    .corner_radius(egui::CornerRadius::same(6)),
                                )
                                .on_hover_text("删除该通用宏")
                                .clicked()
                            {
                                to_remove = Some(i);
                            }
                        },
                    );
                });
                ui.add_space(theme::SP_XS);
            }
            if self.config.universal_macros.is_empty() {
                ui.vertical_centered(|ui| {
                    ui.add_space(theme::SP_S);
                    ui.label(th.hint_text("还没有通用宏 —— 下面新增一个试试"));
                    ui.add_space(theme::SP_S);
                });
            }
            if let Some(i) = to_remove {
                self.config.universal_macros.remove(i);
                let _ = self.config.save_to_file("Config.toml");
                if let Err(e) = self.app_state.reload_config(self.config.clone()) {
                    eprintln!("Failed to reload config after macro removal: {}", e);
                }
            }

            /* 新增行: 名字 + 序列文本 */
            let mut new_name = self.new_macro_name.clone();
            let mut new_text = self.new_macro_text.clone();
            ui.horizontal(|ui| {
                ui.label(th.weak("名字"));
                ui.add(
                    egui::TextEdit::singleline(&mut new_name)
                        .desired_width(90.0)
                        .hint_text("如 觉醒连招"),
                );
                ui.label(th.weak("序列"));
                ui.add(
                    egui::TextEdit::singleline(&mut new_text)
                        .desired_width(ui.available_width() - 120.0)
                        .hint_text("LCTRL⏱30>>K⏱30 (» 可写 >>)"),
                );
                let can_add = !new_name.trim().is_empty() && !new_text.trim().is_empty();
                if ui
                    .add_enabled(can_add, th.secondary_button("＋ 添加"))
                    .on_disabled_hover_text("名字和序列都要填")
                    .clicked()
                {
                    let name = new_name.trim().to_string();
                    let text = new_text.trim().to_string();
                    /* 同名覆盖 (大小写不敏感), 与运行时解析规则一致 */
                    if let Some(slot) = self
                        .config
                        .universal_macros
                        .iter_mut()
                        .find(|m| m.name.eq_ignore_ascii_case(&name))
                    {
                        slot.text = text;
                    } else {
                        self.config
                            .universal_macros
                            .push(crate::config::UniversalMacro { name, text });
                    }
                    let _ = self.config.save_to_file("Config.toml");
                    if let Err(e) = self.app_state.reload_config(self.config.clone()) {
                        eprintln!("Failed to reload config after macro add: {}", e);
                    }
                    new_name.clear();
                    new_text.clear();
                }
            });
            self.new_macro_name = new_name;
            self.new_macro_text = new_text;
        });
    }

}
