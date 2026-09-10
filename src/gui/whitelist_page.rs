//! 白名单页: 进程过滤的列表管理 (启用总开关 + 添加/浏览/删除)。
//!
//! 与「设置 → 白名单」编辑同一份 config.process_whitelist;
//! 所有修改实时落盘并 reload_config, 立即生效。

use crate::gui::SorahkGui;
use crate::gui::theme::{self, Theme};
use crate::gui::widgets;
use eframe::egui;

impl SorahkGui {
    pub(super) fn render_whitelist_page(&mut self, ui: &mut egui::Ui) {
        let th = self.theme();
        let enabled = self.config.whitelist_enabled;
        let count = self.config.process_whitelist.len();

        /* 卡1: 过滤总开关 + 一句话说明 */
        th.card(ui, Some("进程过滤"), |ui| {
            ui.horizontal(|ui| {
                if ui
                    .add(th.status_pill(
                        if enabled { "过滤 · 开启" } else { "过滤 · 关闭" },
                        if enabled { th.good } else { th.hint },
                        if enabled { th.good_soft } else { th.faint },
                    ))
                    .clicked()
                {
                    self.config.whitelist_enabled = !enabled;
                    let _ = self.config.save_to_file("Config.toml");
                    let _ = self.app_state.reload_config(self.config.clone());
                }
                ui.label(th.hint_text(if enabled {
                    "只有列表内的进程会响应连发"
                } else {
                    "所有进程都会响应连发 (列表保留, 重开即恢复)"
                }));
            });
            ui.add_space(theme::SP_XS);
            ui.label(th.hint_text(
                "白名单防止连发热键在其他程序里误触: 比如只添加 dnf.exe, 切到浏览器时连发自动暂停。",
            ));
        });

        /* 卡2: 进程列表 (标题行右侧计数徽章) */
        th.card_with_actions(
            ui,
            Some("进程列表"),
            |ui| {
                th.badge(ui, &format!("{} 个", count), th.accent_text, th.accent_soft);
            },
            |ui| {
                /* 添加行: 输入 + 添加 + 浏览 */
                ui.horizontal(|ui| {
                    /* ★v20.6: 显性输入框 (描边清晰, 一眼可见可输入) */
                    th.text_input(
                        ui,
                        &mut self.new_whitelist_name,
                        "进程名, 如 dnf.exe",
                        220.0,
                        egui::Id::new("new_whitelist_name"),
                    );
                    if ui.add(th.primary_button("＋ 添加")).clicked() {
                        let name = self.new_whitelist_name.trim().to_string();
                        if name.is_empty() {
                            self.whitelist_error = Some("请输入进程名".into());
                        } else if self
                            .config
                            .process_whitelist
                            .iter()
                            .any(|x| x.eq_ignore_ascii_case(&name))
                        {
                            self.whitelist_error = Some(format!("「{}」已在白名单中", name));
                        } else {
                            self.config.process_whitelist.push(name);
                            self.new_whitelist_name.clear();
                            self.whitelist_error = None;
                            let _ = self.config.save_to_file("Config.toml");
                            let _ = self.app_state.reload_config(self.config.clone());
                        }
                    }
                    if ui.add(th.secondary_button("浏览…")).clicked() {
                        if let Some(path) = rfd::FileDialog::new()
                            .add_filter("Executable", &["exe"])
                            .set_title("选择进程的可执行文件")
                            .pick_file()
                            && let Some(filename) = path.file_name()
                        {
                            let name = filename.to_string_lossy().to_string();
                            if self
                                .config
                                .process_whitelist
                                .iter()
                                .any(|x| x.eq_ignore_ascii_case(&name))
                            {
                                self.whitelist_error = Some(format!("「{}」已在白名单中", name));
                            } else {
                                self.config.process_whitelist.push(name);
                                self.whitelist_error = None;
                                let _ = self.config.save_to_file("Config.toml");
                                let _ = self.app_state.reload_config(self.config.clone());
                            }
                        }
                    }
                });
                if let Some(err) = &self.whitelist_error {
                    ui.label(egui::RichText::new(err).size(12.0).color(th.bad));
                }
                ui.add_space(theme::SP_S);

                /* 列表: 粗体进程名 + 右对齐移除按钮 */
                if self.config.process_whitelist.is_empty() {
                    ui.vertical_centered(|ui| {
                        ui.add_space(theme::SP_S);
                        widgets::status_dot(ui, th.hint, false, 4.5);
                        ui.label(th.hint_text("列表为空 —— 当前所有进程都可以使用连发"));
                        ui.add_space(theme::SP_S);
                    });
                } else {
                    let mut remove: Option<usize> = None;
                    for (i, name) in self.config.process_whitelist.iter().enumerate() {
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new(name)
                                    .size(13.0)
                                    .color(th.text)
                                    .family(Theme::font_bold()),
                            );
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    let del = ui.add_sized(
                                        [26.0, 22.0],
                                        egui::Button::new(
                                            egui::RichText::new("✕").size(12.0).color(th.text_weak),
                                        )
                                        .fill(th.faint)
                                        .corner_radius(egui::CornerRadius::same(6)),
                                    );
                                    if del.on_hover_text("从白名单移除").clicked() {
                                        remove = Some(i);
                                    }
                                },
                            );
                        });
                        ui.add_space(theme::SP_XS);
                    }
                    if let Some(i) = remove {
                        self.config.process_whitelist.remove(i);
                        self.whitelist_error = None;
                        let _ = self.config.save_to_file("Config.toml");
                        let _ = self.app_state.reload_config(self.config.clone());
                    }
                    ui.label(th.hint_text("移除后该进程立即恢复响应连发。"));
                }
            },
        );

        ui.add_space(theme::SP_XS);
        ui.label(th.hint_text(
            "修改实时保存并立即生效, 无需重启; 与「设置 → 白名单」编辑的是同一份列表。",
        ));
    }
}
