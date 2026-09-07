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
        let _ = ctx;
        self.render_turbo_hero(ui, frame_state);
        self.render_turbo_preset_manager(ui);
        self.render_turbo_mappings(ui);
        self.render_turbo_params(ui);
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
                        self.config.presets.retain(|p| p.name != name);
                        self.config.presets.push(crate::config::Preset {
                            name: name.clone(),
                            mappings: self.config.mappings.clone(),
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
        });
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
    pub(super) fn add_new_mapping(&mut self) {
        self.config.mappings.push(crate::config::KeyMapping {
            trigger_key: "A".to_string(),
            target_keys: Default::default(),
            interval: None,
            event_duration: None,
            turbo_enabled: true,
            move_speed: 5,
            double_tap_enabled: false,
            double_tap_gap_ms: 50,
            note: String::new(),
        });
        let idx = self.config.mappings.len() - 1;
        self.edit_mapping_is_new = true;
        self.edit_mapping_snapshot = None;
        self.edit_mapping_idx = Some(idx);
    }


    /// 进入已有映射的编辑态 (保存快照用于取消还原)。
    pub(super) fn begin_mapping_edit(&mut self, idx: usize) {
        if let Some(m) = self.config.mappings.get(idx) {
            self.edit_mapping_snapshot = Some(m.clone());
            self.edit_mapping_is_new = false;
            self.edit_mapping_idx = Some(idx);
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
                        let chip = th.badge_clickable(ui, &format!("{}  ✕", t), th.target_fg, th.target_bg);
                        if chip.clicked() {
                            remove_target = Some(i);
                        }
                        chip.on_hover_text("点击移除该目标键");
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
    pub(super) fn render_turbo_params(&self, ui: &mut egui::Ui) {
        let t = &self.translations;
        let th = self.theme();
        th.card(ui, Some("全局配置"), |ui| {
            egui::Grid::new("turbo_params_grid")
                .num_columns(2)
                .spacing([theme::SP_L, theme::SP_S])
                .min_col_width(ui.available_width() * 0.42)
                .striped(false)
                .show(ui, |ui| {
                    Self::param_row(ui, &th, t.input_timeout_display(), &format!("{} ms", self.config.input_timeout));
                    Self::param_flag(ui, &th, t.show_tray_icon_display(), self.config.show_tray_icon);
                    Self::param_row(ui, &th, t.default_interval_display(), &format!("{} ms", self.config.interval));
                    Self::param_flag(ui, &th, t.show_notifications_display(), self.config.show_notifications);
                    Self::param_row(ui, &th, t.default_duration_display(), &format!("{} ms", self.config.event_duration));
                    Self::param_flag(ui, &th, t.always_on_top_display(), self.config.always_on_top);
                });
            ui.add_space(theme::SP_XS);
            ui.label(th.hint_text("以上参数在「设置」中修改并即时生效"));
        });
    }


    /// 参数行: 标签 | 值 (网格内)。
    pub(super) fn param_row(ui: &mut egui::Ui, th: &Theme, label: &str, value: &str) {
        ui.label(th.weak(label));
        ui.label(egui::RichText::new(value).size(13.0).strong().color(th.accent_text));
        ui.end_row();
    }


    /// 布尔参数行: 标签 | 状态徽章 (网格内)。
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
                    // 触发键帽 (多键拆分)
                    for part in trigger.split('+') {
                        widgets::keycap(ui, th, part);
                    }
                    // 指向箭头
                    ui.label(egui::RichText::new("→").size(12.0).color(th.hint));
                    // 目标键帽
                    if targets.is_empty() {
                        ui.label(th.hint_text("(未设置目标)"));
                    } else {
                        for t in targets {
                            widgets::keycap(ui, th, t);
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
