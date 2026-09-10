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
    /// 连发页: 状态 hero + 映射列表 + 全局参数。
    pub(super) fn render_turbo_page(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, frame_state: &FrameState) {
        self.render_turbo_hero(ui, frame_state);
        self.render_turbo_preset_manager(ui);
        self.render_turbo_mappings(ui);
        self.render_turbo_params(ui, ctx);
    }


    /// 连发映射页 · 预设管理卡: 切换 / 保存 / 重命名 / 删除 (两次确认防误删)。
    pub(super) fn render_turbo_preset_manager(&mut self, ui: &mut egui::Ui) {
        let th = self.theme();
        th.card(ui, Some("预设管理"), |ui| {
            /* 行1: 切换预设 (选择即应用) */
            ui.horizontal(|ui| {
                ui.label(th.weak("切换预设:"));
                let cur_name = if self.config.current_preset.is_empty() {
                    "(无)".to_owned()
                } else {
                    self.config.current_preset.clone()
                };
                let mut next = self.config.current_preset.clone();
                let mut switched = false;
                egui::ComboBox::from_id_salt("page_preset_switch")
                    .selected_text(cur_name)
                    .width(180.0)
                    .show_ui(ui, |ui| {
                        if ui
                            .selectable_label(self.config.current_preset.is_empty(), "(无)")
                            .clicked()
                        {
                            next = String::new();
                            switched = true;
                        }
                        for pr in &self.config.presets {
                            let is_sel = self.config.current_preset == pr.name;
                            if ui.selectable_label(is_sel, &pr.name).clicked() {
                                next = pr.name.clone();
                                switched = true;
                            }
                        }
                    });
                if switched {
                    self.config.current_preset = next.clone();
                    if let Some(pr) = self.config.presets.iter().find(|p| p.name == next) {
                        if !pr.mappings.is_empty() {
                            self.config.mappings = pr.mappings.clone();
                        }
                    }
                    self.page_preset_delete_arm = false;
                    let _ = self.config.save_to_file("Config.toml");
                    // 切换即热重载: 运行时钩子只消费 AppState.input_mappings,
                    // 不 reload 的话 UI 显示新映射而钩子仍按旧映射连发
                    if let Err(e) = self.app_state.reload_config(self.config.clone()) {
                        eprintln!("Failed to reload config after preset switch: {}", e);
                    }
                }
                ui.label(th.hint_text("选择后立即应用映射"));
            });
            ui.add_space(theme::SP_S);

            /* 行2: 保存预设 (留空名称 = 覆盖当前预设) */
            ui.horizontal(|ui| {
                ui.label(th.weak("保存预设:"));
                ui.add(
                    egui::TextEdit::singleline(&mut self.page_preset_name_input)
                        .hint_text("名称 (留空 = 覆盖当前预设)")
                        .desired_width(170.0),
                );
                if ui
                    .add(egui::Button::new(
                        egui::RichText::new("保存预设").size(13.0).color(egui::Color32::WHITE),
                    )
                    .fill(th.btn_primary)
                    .corner_radius(egui::CornerRadius::same(theme::RADIUS_CTRL)))
                    .clicked()
                {
                    let typed = self.page_preset_name_input.trim().to_string();
                    let name = if typed.is_empty() {
                        self.config.current_preset.trim().to_string()
                    } else {
                        typed
                    };
                    if !name.is_empty() {
                        /* ★v20.3: 同名覆盖时保留已绑定的切换键 */
                        let switch_key = self
                            .config
                            .presets
                            .iter()
                            .find(|p| p.name == name)
                            .map(|p| p.switch_key.clone())
                            .unwrap_or_default();
                        self.config.presets.retain(|p| p.name != name);
                        self.config.presets.push(crate::config::Preset {
                            name: name.clone(),
                            mappings: self.config.mappings.clone(),
                            switch_key,
                        });
                        self.config.current_preset = name;
                        self.page_preset_name_input.clear();
                        self.page_preset_delete_arm = false;
                        let _ = self.config.save_to_file("Config.toml");
                    }
                }
                ui.label(th.hint_text("保存当前整页映射为预设"));
            });
            ui.add_space(theme::SP_S);

            /* 行3: 重命名 + 删除 (两次确认), 仅当前预设非空时可用 */
            if !self.config.current_preset.is_empty() {
                ui.horizontal(|ui| {
                    ui.label(th.weak("当前预设:"));
                    if ui
                        .add(egui::Button::new(
                            egui::RichText::new("重命名预设").size(13.0).color(th.btn_secondary_text),
                        )
                        .fill(th.faint)
                        .corner_radius(egui::CornerRadius::same(theme::RADIUS_CTRL)))
                        .clicked()
                    {
                        self.page_preset_rename_input = self.config.current_preset.clone();
                        self.page_preset_rename_show = true;
                    }
                    let del_label = if self.page_preset_delete_arm {
                        "⚠ 确认删除? (再次点击)"
                    } else {
                        "删除预设"
                    };
                    let del_btn = egui::Button::new(
                        egui::RichText::new(del_label).size(13.0).color(egui::Color32::WHITE),
                    )
                    .fill(if self.page_preset_delete_arm { th.bad } else { th.faint })
                    .corner_radius(egui::CornerRadius::same(theme::RADIUS_CTRL));
                    if ui.add(del_btn).clicked() {
                        if self.page_preset_delete_arm {
                            let name = self.config.current_preset.clone();
                            self.config.presets.retain(|p| p.name != name);
                            self.config.current_preset.clear();
                            self.page_preset_delete_arm = false;
                            let _ = self.config.save_to_file("Config.toml");
                        } else {
                            self.page_preset_delete_arm = true;
                        }
                    }
                    if self.page_preset_delete_arm && ui.button("取消").clicked() {
                        self.page_preset_delete_arm = false;
                    }
                });

                /* 重命名输入行 */
                if self.page_preset_rename_show {
                    ui.add_space(theme::SP_XS);
                    ui.horizontal(|ui| {
                        ui.label(th.weak("新名称:"));
                        ui.add(
                            egui::TextEdit::singleline(&mut self.page_preset_rename_input)
                                .hint_text("输入新名称")
                                .desired_width(170.0),
                        );
                        if ui
                            .add(egui::Button::new(
                                egui::RichText::new("✓ 确认").size(13.0).color(egui::Color32::WHITE),
                            )
                            .fill(th.good)
                            .corner_radius(egui::CornerRadius::same(theme::RADIUS_CTRL)))
                            .clicked()
                        {
                            let old = self.config.current_preset.clone();
                            let new_name = self.page_preset_rename_input.trim().to_string();
                            if !new_name.is_empty() && new_name != old {
                                if let Some(pr) =
                                    self.config.presets.iter_mut().find(|p| p.name == old)
                                {
                                    pr.name = new_name.clone();
                                }
                                self.config.current_preset = new_name;
                                let _ = self.config.save_to_file("Config.toml");
                            }
                            self.page_preset_rename_show = false;
                            self.page_preset_rename_input.clear();
                        }
                        if ui.button("✕").clicked() {
                            self.page_preset_rename_show = false;
                            self.page_preset_rename_input.clear();
                        }
                    });
                }
            } else {
                ui.label(th.hint_text("顶栏选择「(无)」时映射未入档: 用上方「保存预设」起名入档后可切换/重命名/删除"));
            }

            /* ★v20.3 行4: 预设切换键 —— 游戏内按组合键直接切预设 (保存时防冲突) */
            ui.add_space(theme::SP_S);
            ui.separator();
            ui.add_space(theme::SP_XS);
            if self.config.presets.is_empty() {
                ui.label(th.hint_text("组合键切换预设: 先用上方「保存预设」入档, 再在这里绑定切换键"));
            } else {
                if self.preset_key_target >= self.config.presets.len() {
                    self.preset_key_target = self.config.presets.len() - 1;
                }
                let cur_key = self.config.presets[self.preset_key_target].switch_key.clone();
                /* 输入框未聚焦时跟随所选预设回显 (聚焦时保留用户正在输入的内容) */
                let input_id = egui::Id::new("preset_switch_key_input");
                let editing = ui.memory(|m| m.has_focus(input_id));
                if !editing && self.preset_key_input != cur_key {
                    self.preset_key_input = cur_key;
                }
                ui.horizontal_wrapped(|ui| {
                    ui.label(th.weak("切换键:"));
                    let target_name = self.config.presets[self.preset_key_target].name.clone();
                    let mut next_idx = self.preset_key_target;
                    egui::ComboBox::from_id_salt("preset_switch_key_target")
                        .selected_text(target_name)
                        .width(130.0)
                        .show_ui(ui, |ui| {
                            for (i, pr) in self.config.presets.iter().enumerate() {
                                if ui
                                    .selectable_label(i == self.preset_key_target, &pr.name)
                                    .clicked()
                                {
                                    next_idx = i;
                                }
                            }
                        });
                    if next_idx != self.preset_key_target {
                        self.preset_key_target = next_idx;
                        self.preset_key_input = self.config.presets[next_idx].switch_key.clone();
                        self.preset_key_error = None;
                    }
                    ui.add(
                        egui::TextEdit::singleline(&mut self.preset_key_input)
                            .id_salt(input_id)
                            .hint_text("如 F6 或 CTRL+F6")
                            .desired_width(120.0),
                    );
                    if ui.add(th.secondary_button("✓ 保存")).clicked() {
                        self.save_preset_switch_key();
                    }
                    if ui.add(th.secondary_button("清除")).clicked() {
                        self.preset_key_input.clear();
                        self.save_preset_switch_key();
                    }
                    if let Some(err) = &self.preset_key_error {
                        ui.label(
                            egui::RichText::new(format!("❌ {}", err))
                                .size(11.0)
                                .color(th.bad),
                        );
                    } else {
                        ui.label(th.hint_text("游戏内/任意界面按该键即切到此预设; 各预设键不得重复"));
                    }
                });
            }
        });
    }

    /// ★v20.3: 保存所选预设的切换键。
    /// 校验: ①键名可被解析 ②不与其他预设的切换键重复 ③不与「连发切换键」重复。
    pub(super) fn save_preset_switch_key(&mut self) {
        if self.preset_key_target >= self.config.presets.len() {
            return;
        }
        let target_name = self.config.presets[self.preset_key_target].name.clone();
        let key = self.preset_key_input.trim().to_uppercase();
        if key.is_empty() {
            self.config.presets[self.preset_key_target].switch_key.clear();
            self.preset_key_error = None;
        } else if matches!(
            Self::parse_switch_key(&key),
            crate::gui::ParsedSwitchKey::None
        ) {
            self.preset_key_error =
                Some("无法识别的按键名 (示例: F6 / CTRL+F6 / ALT+1)".to_string());
            return;
        } else if let Some(other) = self.config.presets.iter().find(|p| {
            p.name != target_name
                && !p.switch_key.trim().is_empty()
                && p.switch_key.trim().to_uppercase() == key
        }) {
            self.preset_key_error = Some(format!("与预设「{}」的切换键冲突", other.name));
            return;
        } else if !self.config.switch_key.trim().is_empty()
            && self.config.switch_key.trim().to_uppercase() == key
        {
            self.preset_key_error = Some("与「连发切换键」冲突, 请换一个键".to_string());
            return;
        } else {
            self.config.presets[self.preset_key_target].switch_key = key;
            self.preset_key_error = None;
        }
        let _ = self.config.save_to_file("Config.toml");
    }


    /// 状态 hero: 运行状态 + 震动开关 + 主操作按钮 + 统计。
    pub(super) fn render_turbo_hero(&mut self, ui: &mut egui::Ui, frame_state: &FrameState) {
        // 提前取值, 闭包内 &mut self 与 translations 借用不冲突
        let status_paused = self.translations.paused_status().to_owned();
        let status_running = self.translations.running_status().to_owned();
        let exit_label = self.translations.exit_button().to_owned();
        let start_label = self.translations.start_button().to_owned();
        let pause_label = self.translations.pause_button().to_owned();
        let th = self.theme();
        th.card(ui, None, |ui| {
            ui.horizontal(|ui| {
                // 左: 状态
                ui.vertical(|ui| {
                    ui.add_space(2.0);
                    ui.horizontal(|ui| {
                        let (color, text, pulsing) = if frame_state.is_paused {
                            (th.warn, status_paused.as_str(), false)
                        } else {
                            (th.good, status_running.as_str(), true)
                        };
                        widgets::status_dot(ui, color, pulsing, 6.0);
                        ui.add_space(2.0);
                        ui.label(
                            egui::RichText::new(text)
                                .size(17.0)
                                .strong()
                                .color(if frame_state.is_paused { th.warn } else { th.good }),
                        );
                    });
                    ui.add_space(theme::SP_XS);
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
                        ui.label(th.hint_text("由 DfoVibration.dll 战斗事件驱动 (进图后自动)"));
                    });
                });

                // 右: 主操作按钮
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let exit_btn = egui::Button::new(
                        egui::RichText::new(format!("\u{23f9} {}", exit_label))
                            .size(13.0)
                            .strong()
                            .color(egui::Color32::WHITE),
                    )
                    .fill(th.btn_danger)
                    .corner_radius(egui::CornerRadius::same(theme::RADIUS_CTRL))
                    .min_size(egui::vec2(96.0, 34.0));
                    if ui.add(exit_btn).clicked() {
                        // 只走 app_state.exit(): 经 should_exit → ViewportCommand::Close
                        // 优雅退出, 保证托盘/震动线程清理与 on_exit 执行
                        self.app_state.exit();
                    }
                    ui.add_space(theme::SP_S);
                    let (label, color) = if frame_state.is_paused {
                        (start_label.as_str(), th.good)
                    } else {
                        (pause_label.as_str(), th.warn)
                    };
                    let toggle_btn = egui::Button::new(
                        egui::RichText::new(label).size(13.0).color(egui::Color32::WHITE).strong(),
                    )
                    .fill(color)
                    .corner_radius(egui::CornerRadius::same(theme::RADIUS_CTRL))
                    .min_size(egui::vec2(96.0, 34.0));
                    if ui.add(toggle_btn).clicked() {
                        self.toggle_with_notify();
                    }
                });
            });

            // 统计条: 映射数 / 工作线程 / 默认间隔 / 默认时长 / 切换热键
            ui.add_space(theme::SP_M);
            ui.separator();
            ui.add_space(theme::SP_S);
            ui.horizontal(|ui| {
                widgets::stat(
                    ui,
                    &th,
                    &self.config.mappings.len().to_string(),
                    "映射条目",
                    th.accent_text,
                );
                ui.separator();
                if frame_state.worker_count > 0 {
                    widgets::stat(
                        ui,
                        &th,
                        &frame_state.worker_count.to_string(),
                        "工作线程",
                        th.text,
                    );
                    ui.separator();
                }
                widgets::stat(ui, &th, &format!("{} ms", self.config.interval), "默认间隔", th.text);
                ui.separator();
                widgets::stat(
                    ui,
                    &th,
                    &format!("{} ms", self.config.event_duration),
                    "默认时长",
                    th.text,
                );
                ui.separator();
                ui.vertical(|ui| {
                    ui.set_min_width(64.0);
                    widgets::keycap(ui, &th, &self.config.switch_key);
                    ui.label(th.hint_text("切换热键"));
                });
            });
        });
    }


    /// 映射列表: 行式布局 + 行内编辑/新增 (无需再进设置弹窗)。
    pub(super) fn render_turbo_mappings(&mut self, ui: &mut egui::Ui) {
        let th = self.theme();
        let count = self.config.mappings.len();
        th.card(ui, None, |ui| {
            ui.horizontal(|ui| {
                ui.label(th.h2("连发映射"));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        egui::RichText::new(format!("{} 条", count))
                            .size(12.0)
                            .color(th.hint),
                    );
                    ui.add_space(theme::SP_S);
                    if ui.add(th.primary_button("＋ 新增映射")).clicked() {
                        self.add_new_mapping();
                    }
                });
            });
            ui.add_space(theme::SP_M);
            {
                if count == 0 {
                    widgets::empty_state(
                        ui,
                        &th,
                        widgets::Icon::Keyboard,
                        "暂无连发映射",
                        &[
                            "点击右上角「＋ 新增映射」创建第一条",
                            "配置 手柄/键盘/鼠标 触发 → 目标键",
                        ],
                    );
                    return;
                }
                let interval = self.config.interval;
                let duration = self.config.event_duration;
                let mut idx = 0usize;
                while idx < self.config.mappings.len() {
                    if self.edit_mapping_idx == Some(idx) {
                        /* ★v20.3: 新增映射置顶后, 把编辑面板滚到可视区顶部 */
                        if std::mem::take(&mut self.scroll_to_edit_row) {
                            ui.scroll_to_cursor(Some(egui::Align::TOP));
                        }
                        self.render_mapping_edit_row(ui, idx, interval, duration);
                        // 编辑行可能触发删除, 重新检查下标
                        if idx >= self.config.mappings.len() {
                            break;
                        }
                        idx += 1;
                        continue;
                    }
                    // 静态行: 拷贝显示数据避免借用冲突, 行尾挂「编辑」按钮
                    let (trigger, targets, note, turbo) = {
                        let m = &self.config.mappings[idx];
                        (
                            m.trigger_key.clone(),
                            m.target_keys.to_vec(),
                            m.note.clone(),
                            m.turbo_enabled,
                        )
                    };
                    let m_interval = self.config.mappings[idx].interval.unwrap_or(interval);
                    let m_duration = self.config.mappings[idx].event_duration.unwrap_or(duration);
                    render_mapping_row(
                        ui,
                        &th,
                        &trigger,
                        &targets,
                        m_interval,
                        m_duration,
                        turbo,
                        &note,
                        &mut |ui| {
                            /* 方向/滚动/连发/1×双击 全部收在「编辑」面板内 (v20.4 用户定稿:
                             * 不往静态行塞按钮); 点编辑即可配齐 */
                            if ui.add(th.secondary_button("编辑")).clicked() {
                                self.begin_mapping_edit(idx);
                            }
                        },
                    );
                    idx += 1;
                }
            }
        });
    }

    /* ═══════════════════ 连发映射内联编辑 (P8) ═══════════════════ */


    /// 新增一条映射并进入编辑态 (取消时自动删除)。
    /// ★v20.3: 插到列表**最前** (用户定稿: 新增映射的编辑面板必须显示在最前面),
    /// 并在下一帧滚动到编辑面板顶部。
    pub(super) fn add_new_mapping(&mut self) {
        self.config.mappings.insert(
            0,
            crate::config::KeyMapping {
                trigger_key: "A".to_string(),
                target_keys: Default::default(),
                interval: None,
                event_duration: None,
                turbo_enabled: true,
                move_speed: 5,
                double_tap_enabled: false,
                double_tap_gap_ms: 50,
                note: String::new(),
            },
        );
        self.edit_mapping_is_new = true;
        self.edit_mapping_snapshot = None;
        self.edit_mapping_idx = Some(0);
        self.scroll_to_edit_row = true;
    }


    /// 进入已有映射的编辑态 (保存快照用于取消还原)。
    pub(super) fn begin_mapping_edit(&mut self, idx: usize) {
        if let Some(m) = self.config.mappings.get(idx) {
            self.edit_mapping_snapshot = Some(m.clone());
            self.edit_mapping_is_new = false;
            self.edit_mapping_idx = Some(idx);
        }
    }


    /// ★v20.3: 在连发页内联编辑行打开「鼠标方向」选择 (设置弹窗同款对话框)。
    pub(super) fn open_mouse_direction_dialog(&mut self, idx: usize) {
        self.mouse_direction_mapping_idx = Some(idx);
        self.mouse_direction_dialog = Some(
            crate::gui::mouse_direction_dialog::MouseDirectionDialog::new(),
        );
    }

    /// ★v20.3: 在连发页内联编辑行打开「鼠标滚动」选择 (设置弹窗同款对话框)。
    pub(super) fn open_mouse_scroll_dialog(&mut self, idx: usize) {
        self.mouse_scroll_mapping_idx = Some(idx);
        self.mouse_scroll_dialog = Some(
            crate::gui::mouse_scroll_dialog::MouseScrollDialog::new(),
        );
    }

    /// ★v20.3: 按预设名切换连发预设 (热键/下拉共用; 切换即落盘 + 热重载 + 同步弹窗暂存)。
    pub(super) fn switch_to_turbo_preset(&mut self, name: &str) {
        if self.config.current_preset == name {
            return;
        }
        if let Some(pr) = self.config.presets.iter().find(|p| p.name == name) {
            self.config.current_preset = pr.name.clone();
            /* 空预设不覆盖 (防止把当前映射清空) */
            if !pr.mappings.is_empty() {
                self.config.mappings = pr.mappings.clone();
            }
            self.page_preset_delete_arm = false;
            let _ = self.config.save_to_file("Config.toml");
            if let Err(e) = self.app_state.reload_config(self.config.clone()) {
                eprintln!("Failed to reload config after preset switch: {}", e);
            }
            if let Some(tc) = &mut self.temp_config {
                tc.current_preset = self.config.current_preset.clone();
                tc.mappings = self.config.mappings.clone();
                tc.presets = self.config.presets.clone();
            }
        }
    }


    /// 行内编辑面板: 触发/目标/间隔/时长/连发/备注 + 保存/取消/删除。
    pub(super) fn render_mapping_edit_row(
        &mut self,
        ui: &mut egui::Ui,
        idx: usize,
        default_interval: u64,
        default_duration: u64,
    ) {
        let th = self.theme();
        let Some(mapping) = self.config.mappings.get_mut(idx) else {
            self.edit_mapping_idx = None;
            return;
        };

        // 局部可编辑副本 (变更即回写 config; 取消由快照还原)
        let mut trigger = mapping.trigger_key.clone();
        let mut targets: Vec<String> = mapping.target_keys.to_vec();
        let mut interval = mapping.interval.unwrap_or(default_interval) as f64;
        let mut duration = mapping.event_duration.unwrap_or(default_duration) as f64;
        let mut turbo = mapping.turbo_enabled;
        let mut double_tap = mapping.double_tap_enabled;
        let mut move_speed = mapping.move_speed as f32;
        /* 滚动映射的移动速度上限更高 (与设置弹窗一致: 普通 100 / 滚动 1200) */
        let speed_hi = if targets.iter().any(|k| k.starts_with("SCROLL")) {
            1200.0
        } else {
            100.0
        };
        let mut note = mapping.note.clone();
        let mut remove_target: Option<usize> = None;
        let mut request_delete = false;

        egui::Frame::NONE
            .fill(th.card_alt)
            .stroke(egui::Stroke::new(1.0, th.accent_soft))
            .corner_radius(egui::CornerRadius::same(10))
            .inner_margin(egui::Margin::same(14))
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(format!("编辑映射 #{}", idx + 1))
                            .size(13.5)
                            .strong()
                            .color(th.heading),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.add(th.danger_button("删除该映射")).clicked() {
                            request_delete = true;
                        }
                    });
                });
                ui.add_space(theme::SP_S);

                // 触发键
                ui.horizontal(|ui| {
                    ui.label(th.weak("触发键"));
                    widgets::keycap(ui, &th, &trigger);
                    let capturing = matches!(
                        self.key_capture_mode,
                        KeyCaptureMode::MappingTrigger(i) if i == idx
                    );
                    let btn_text = if capturing {
                        "正在捕获触发键… (按下并松开)"
                    } else {
                        "捕获触发键"
                    };
                    if ui.add(th.secondary_button(btn_text)).clicked() && !capturing {
                        self.start_mapping_capture(idx, true);
                    }
                });
                ui.add_space(theme::SP_S);

                // 目标键
                ui.horizontal_wrapped(|ui| {
                    ui.label(th.weak("目标键"));
                    for (i, t) in targets.iter().enumerate() {
                        /* ★v20.5: 长名截断 35 字符 (悬停看全名), 行不溢出 */
                        let short = truncate_chars(t, 35);
                        let chip =
                            th.badge_clickable(ui, &format!("{}  ✕", short), th.target_fg, th.target_bg);
                        if chip.clicked() {
                            remove_target = Some(i);
                        }
                        chip.on_hover_text(format!("{}\n点击移除该目标键", t));
                    }
                    if targets.is_empty() {
                        ui.label(th.hint_text("尚未设置目标键"));
                    }
                    let capturing = matches!(
                        self.key_capture_mode,
                        KeyCaptureMode::MappingTarget(i) if i == idx
                    );
                    let btn_text = if capturing {
                        "正在捕获目标键… (按下并松开)"
                    } else {
                        "＋ 添加目标"
                    };
                    if ui.add(th.secondary_button(btn_text)).clicked() && !capturing {
                        self.start_mapping_capture(idx, false);
                    }
                });
                ui.add_space(theme::SP_M);

                // 参数行
                ui.horizontal_wrapped(|ui| {
                    ui.label(th.weak("间隔"));
                    if ui
                        .add(egui::DragValue::new(&mut interval).range(1.0..=5000.0).speed(1.0))
                        .changed()
                    {
                        self.config.mappings[idx].interval = Some(interval.max(1.0) as u64);
                    }
                    ui.label(th.weak("ms"));
                    ui.add_space(theme::SP_L);
                    ui.label(th.weak("时长"));
                    if ui
                        .add(egui::DragValue::new(&mut duration).range(1.0..=5000.0).speed(1.0))
                        .changed()
                    {
                        self.config.mappings[idx].event_duration = Some(duration.max(1.0) as u64);
                    }
                    ui.label(th.weak("ms"));
                    ui.add_space(theme::SP_L);
                    if ui.checkbox(&mut turbo, "连发").changed() {
                        self.config.mappings[idx].turbo_enabled = turbo;
                    }
                    ui.add_space(theme::SP_L);
                    /* ★v20.3: 1×双击补齐到连发页 (原只在设置弹窗有) */
                    if ui
                        .checkbox(&mut double_tap, "1× 双击")
                        .on_hover_text(
                            "首按自动补一次双击 (DNF 跑步用); 与设置弹窗里的「1×双击」是同一开关",
                        )
                        .changed()
                    {
                        self.config.mappings[idx].double_tap_enabled = double_tap;
                    }
                });
                ui.add_space(theme::SP_S);

                /* ★v20.3: 鼠标方向/滚动/移动速度补齐到连发页 (原只在设置弹窗有) */
                ui.horizontal_wrapped(|ui| {
                    ui.label(th.weak("鼠标"));
                    if ui
                        .add(th.secondary_button("⌖ 方向"))
                        .on_hover_text("把鼠标方向 (如 MOUSE_UP_LEFT) 加入目标键")
                        .clicked()
                    {
                        self.open_mouse_direction_dialog(idx);
                    }
                    if ui
                        .add(th.secondary_button("🎡 滚动"))
                        .on_hover_text("把鼠标滚动 (SCROLL_UP/DOWN/MBUTTON) 加入目标键")
                        .clicked()
                    {
                        self.open_mouse_scroll_dialog(idx);
                    }
                    ui.add_space(theme::SP_L);
                    ui.label(th.weak("移动速度"));
                    if ui
                        .add(
                            egui::DragValue::new(&mut move_speed)
                                .range(1.0..=speed_hi)
                                .speed(1.0),
                        )
                        .on_hover_text(format!(
                            "方向/滚动映射的每步移动量 (px/步)\n当前映射上限 {}",
                            speed_hi as i32
                        ))
                        .changed()
                    {
                        self.config.mappings[idx].move_speed = move_speed.round().max(1.0) as i32;
                    }
                });
                ui.add_space(theme::SP_S);

                // 备注
                ui.horizontal(|ui| {
                    ui.label(th.weak("备注"));
                    ui.add(
                        egui::TextEdit::singleline(&mut note)
                            .desired_width(ui.available_width() - 40.0)
                            .hint_text("给这条映射加个备注 (可选)"),
                    );
                });
                ui.add_space(theme::SP_M);

                // 保存/取消
                ui.horizontal(|ui| {
                    if ui.add(th.primary_button("保存修改")).clicked() {
                        self.config.mappings[idx].note = note.clone();
                        let _ = self.config.save_to_file("Config.toml");
                        if let Err(e) = self.app_state.reload_config(self.config.clone()) {
                            eprintln!("Failed to reload config after mapping edit: {}", e);
                        }
                        self.edit_mapping_idx = None;
                        self.edit_mapping_snapshot = None;
                        self.edit_mapping_is_new = false;
                    }
                    ui.add_space(theme::SP_S);
                    if ui.add(th.secondary_button("取消")).clicked() {
                        if self.edit_mapping_is_new {
                            self.config.mappings.remove(idx);
                        } else if let Some(snap) = self.edit_mapping_snapshot.take() {
                            if let Some(m) = self.config.mappings.get_mut(idx) {
                                *m = snap;
                            }
                        }
                        self.edit_mapping_idx = None;
                        self.edit_mapping_is_new = false;
                    }
                });
            });

        // 移除目标键 (需在 Frame 闭包外执行, 避免借用冲突)
        if let Some(i) = remove_target {
            if i < self.config.mappings[idx].target_keys.len() {
                let key = self.config.mappings[idx].target_keys[i].clone();
                self.config.mappings[idx].remove_target_key(&key);
            }
        }
        // 回写文本字段
        if self.edit_mapping_idx == Some(idx) {
            if self.config.mappings[idx].trigger_key != trigger {
                self.config.mappings[idx].trigger_key = trigger;
            }
            if self.config.mappings[idx].note != note {
                self.config.mappings[idx].note = note;
            }
        }
        // 删除请求
        if request_delete && self.edit_mapping_idx == Some(idx) {
            self.config.mappings.remove(idx);
            self.edit_mapping_idx = None;
            self.edit_mapping_snapshot = None;
            self.edit_mapping_is_new = false;
            let _ = self.config.save_to_file("Config.toml");
            if let Err(e) = self.app_state.reload_config(self.config.clone()) {
                eprintln!("Failed to reload config after mapping delete: {}", e);
            }
        }
    }


    /// 开始内联编辑的按键捕获 (触发键含手柄原始输入, 目标键含鼠标)。
    pub(super) fn start_mapping_capture(&mut self, idx: usize, is_trigger: bool) {
        self.key_capture_mode = if is_trigger {
            KeyCaptureMode::MappingTrigger(idx)
        } else {
            KeyCaptureMode::MappingTarget(idx)
        };
        self.capture_pressed_keys.clear();
        self.capture_initial_pressed = Self::poll_all_pressed_keys();
        if is_trigger {
            self.app_state.set_raw_input_capture_mode(true);
        }
        self.just_captured_input = true;
    }


    /// 内联编辑的捕获轮询 (设置弹窗关闭时由 update 调用)。
    pub(super) fn handle_turbo_edit_capture(&mut self, ctx: &egui::Context) {
        let (idx, is_trigger) = match self.key_capture_mode {
            KeyCaptureMode::MappingTrigger(i) => (i, true),
            KeyCaptureMode::MappingTarget(i) => (i, false),
            _ => return,
        };
        if self.edit_mapping_idx != Some(idx) {
            return;
        }

        // Esc 取消捕获
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.key_capture_mode = KeyCaptureMode::None;
            self.capture_pressed_keys.clear();
            self.app_state.set_raw_input_capture_mode(false);
            return;
        }

        let mut captured: Option<String> = None;

        // 键盘 (触发与目标均可)
        let current_pressed = Self::poll_all_pressed_keys();
        current_pressed
            .iter()
            .filter(|&&vk| !self.capture_initial_pressed.contains(&vk))
            .for_each(|&vk| {
                self.capture_pressed_keys.insert(vk);
            });
        let any_released = self
            .capture_pressed_keys
            .iter()
            .any(|vk| !current_pressed.contains(vk));
        if any_released {
            captured = Self::format_captured_keys(&self.capture_pressed_keys);
        }

        // 鼠标 (仅目标键)
        if captured.is_none() && !is_trigger && !self.just_captured_input {
            ctx.input(|i| {
                captured = if i.pointer.button_clicked(egui::PointerButton::Primary) {
                    Some("LBUTTON".to_string())
                } else if i.pointer.button_clicked(egui::PointerButton::Secondary) {
                    Some("RBUTTON".to_string())
                } else if i.pointer.button_clicked(egui::PointerButton::Middle) {
                    Some("MBUTTON".to_string())
                } else if i.pointer.button_clicked(egui::PointerButton::Extra1) {
                    Some("XBUTTON1".to_string())
                } else if i.pointer.button_clicked(egui::PointerButton::Extra2) {
                    Some("XBUTTON2".to_string())
                } else {
                    None
                };
            });
        }

        // 手柄原始输入 (仅触发键)
        if captured.is_none() && is_trigger {
            if let Some(device) = self.app_state.try_recv_raw_input_capture() {
                captured = Some(device.to_string());
            }
        }

        if let Some(name) = captured {
            if is_trigger {
                self.config.mappings[idx].trigger_key = name;
            } else {
                self.config.mappings[idx].add_target_key(name);
            }
            self.key_capture_mode = KeyCaptureMode::None;
            self.capture_pressed_keys.clear();
            self.app_state.set_raw_input_capture_mode(false);
            self.just_captured_input = false;
            // 捕获即持久化 (与手柄页快速捕获一致)
            let _ = self.config.save_to_file("Config.toml");
            if let Err(e) = self.app_state.reload_config(self.config.clone()) {
                eprintln!("Failed to reload config after mapping capture: {}", e);
            }
        } else if self.just_captured_input {
            self.just_captured_input = false;
        }
    }

    /* ═══════════════════ 极简模式 (P8) ═══════════════════ */


    /// 全局参数卡。
    /// 全局配置卡 (★v20.5: 从只读改为**可编辑** —— 玩家在映射页直接改, 不用跑设置弹窗)。
    /// 数值改动即保存 + 热重载; 置顶切换实时作用于窗口。
    pub(super) fn render_turbo_params(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let th = self.theme();
        /* 标签先克隆: 避免闭包内 &self.translations 与 &mut self.config 冲突 */
        let lbl_timeout = self.translations.input_timeout_display().to_owned();
        let lbl_interval = self.translations.default_interval_display().to_owned();
        let lbl_duration = self.translations.default_duration_display().to_owned();
        let lbl_tray = self.translations.show_tray_icon_display().to_owned();
        let lbl_notif = self.translations.show_notifications_display().to_owned();
        let lbl_top = self.translations.always_on_top_display().to_owned();
        let mut dirty = false;
        let mut top_changed: Option<bool> = None;
        th.card(ui, Some("全局配置"), |ui| {
            egui::Grid::new("turbo_params_grid")
                .num_columns(2)
                .spacing([theme::SP_L, theme::SP_S])
                .min_col_width(ui.available_width() * 0.42)
                .striped(false)
                .show(ui, |ui| {
                    let mut v = self.config.input_timeout as f64;
                    ui.label(th.weak(&lbl_timeout));
                    if ui
                        .add(
                            egui::DragValue::new(&mut v)
                                .range(1.0..=2000.0)
                                .speed(1.0)
                                .suffix(" ms"),
                        )
                        .on_hover_text("按键输入超时/去抖 (1-2000 ms)")
                        .changed()
                    {
                        self.config.input_timeout = (v.round().max(1.0) as u64).clamp(1, 2000);
                        dirty = true;
                    }
                    ui.end_row();

                    let mut v = self.config.interval as f64;
                    ui.label(th.weak(&lbl_interval));
                    if ui
                        .add(
                            egui::DragValue::new(&mut v)
                                .range(1.0..=5000.0)
                                .speed(1.0)
                                .suffix(" ms"),
                        )
                        .on_hover_text("连发按键的默认重复间隔 (单条映射可在编辑里覆盖)")
                        .changed()
                    {
                        self.config.interval = (v.round().max(1.0) as u64).max(1);
                        dirty = true;
                    }
                    ui.end_row();

                    let mut v = self.config.event_duration as f64;
                    ui.label(th.weak(&lbl_duration));
                    if ui
                        .add(
                            egui::DragValue::new(&mut v)
                                .range(1.0..=5000.0)
                                .speed(1.0)
                                .suffix(" ms"),
                        )
                        .on_hover_text("每次按键的默认按压时长 (单条映射可在编辑里覆盖)")
                        .changed()
                    {
                        self.config.event_duration = (v.round().max(1.0) as u64).max(1);
                        dirty = true;
                    }
                    ui.end_row();

                    ui.label(th.weak(&lbl_tray));
                    let mut flag = self.config.show_tray_icon;
                    if ui
                        .add(egui::Checkbox::new(&mut flag, ""))
                        .on_hover_text("显示系统托盘图标")
                        .changed()
                    {
                        self.config.show_tray_icon = flag;
                        dirty = true;
                    }
                    ui.end_row();

                    ui.label(th.weak(&lbl_notif));
                    let mut flag = self.config.show_notifications;
                    if ui
                        .add(egui::Checkbox::new(&mut flag, ""))
                        .on_hover_text("显示系统通知")
                        .changed()
                    {
                        self.config.show_notifications = flag;
                        dirty = true;
                    }
                    ui.end_row();

                    ui.label(th.weak(&lbl_top));
                    let mut flag = self.config.always_on_top;
                    if ui
                        .add(egui::Checkbox::new(&mut flag, ""))
                        .on_hover_text("窗口置顶 (即时生效)")
                        .changed()
                    {
                        self.config.always_on_top = flag;
                        top_changed = Some(flag);
                        dirty = true;
                    }
                    ui.end_row();
                });
            ui.add_space(theme::SP_XS);
            ui.label(th.hint_text("改动即时生效并自动保存 (拖动数值 / 勾选开关)"));
        });
        if dirty {
            let _ = self.config.save_to_file("Config.toml");
            if let Err(e) = self.app_state.reload_config(self.config.clone()) {
                eprintln!("Failed to reload config after global params edit: {}", e);
            }
            if let Some(top) = top_changed {
                ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(if top {
                    egui::WindowLevel::AlwaysOnTop
                } else {
                    egui::WindowLevel::Normal
                }));
            }
        }
    }


    /// 参数行: 标签 | 值 (网格内)。
    #[allow(dead_code)]
    pub(super) fn param_row(ui: &mut egui::Ui, th: &Theme, label: &str, value: &str) {
        ui.label(th.weak(label));
        ui.label(egui::RichText::new(value).size(13.0).strong().color(th.accent_text));
        ui.end_row();
    }


    /// 布尔参数行: 标签 | 状态徽章 (网格内)。
    #[allow(dead_code)]
    pub(super) fn param_flag(ui: &mut egui::Ui, th: &Theme, label: &str, on: bool) {
        ui.label(th.weak(label));
        let (text, fg, bg) = if on {
            ("开启", th.good, th.good_soft)
        } else {
            ("关闭", th.hint, th.faint)
        };
        th.badge(ui, text, fg, bg);
        ui.end_row();
    }
}


/// 单条映射行: 触发键帽 → 目标键帽 | 间隔/时长 | Turbo | 备注 | 行尾动作。
#[allow(clippy::too_many_arguments)]
fn render_mapping_row(
    ui: &mut egui::Ui,
    th: &Theme,
    trigger: &str,
    targets: &[String],
    interval: u64,
    duration: u64,
    turbo: bool,
    note: &str,
    extra: &mut dyn FnMut(&mut egui::Ui),
) {
    ui.horizontal(|ui| {
        // 行底: hover 高亮由 egui 默认; 圆角行容器
        egui::Frame::NONE
            .fill(if ui.ui_contains_pointer() { th.faint } else { egui::Color32::TRANSPARENT })
            .corner_radius(egui::CornerRadius::same(theme::RADIUS_CTRL))
            .inner_margin(egui::Margin::symmetric(10, 7))
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.horizontal(|ui| {
                    // 触发键帽 (多键拆分; ★v20.5 长名截断 20 字符防溢出, 悬停看全名 —— 老宿主口径)
                    for part in trigger.split('+') {
                        let short = truncate_chars(part, 20);
                        widgets::keycap(ui, th, &short).on_hover_text(part.to_string());
                    }
                    // 指向箭头
                    ui.label(egui::RichText::new("→").size(12.0).color(th.hint));
                    // 目标键帽 (★v20.5 长名截断 35 字符, 悬停看全名)
                    if targets.is_empty() {
                        ui.label(th.hint_text("(未设置目标)"));
                    } else {
                        for t in targets {
                            let short = truncate_chars(t, 35);
                            widgets::keycap(ui, th, &short).on_hover_text(t.clone());
                        }
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        extra(ui);
                        ui.add_space(theme::SP_M);
                        // 备注
                        if note.is_empty() {
                            ui.label(th.hint_text("-"));
                        } else {
                            ui.label(
                                egui::RichText::new(truncate_chars(note, 14))
                                    .size(11.5)
                                    .color(th.hint),
                            )
                            .on_hover_text(note);
                        }
                        ui.add_space(theme::SP_M);
                        // Turbo 徽章
                        if turbo {
                            th.badge(ui, "TURBO", th.accent, th.accent_soft);
                        } else {
                            th.badge(ui, "单发", th.hint, th.faint);
                        }
                        ui.add_space(theme::SP_M);
                        ui.label(
                            egui::RichText::new(format!("{interval}ms / {duration}ms"))
                                .size(11.5)
                                .color(th.text_weak),
                        )
                        .on_hover_text(format!("间隔 {interval}ms / 时长 {duration}ms"));
                    });
                });
            });
    });
}
