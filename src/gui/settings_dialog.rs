//! Settings dialog implementation.

use crate::config::KeyMapping;
use crate::gui::SorahkGui;
use crate::gui::types::KeyCaptureMode;
use crate::i18n::CachedTranslations;
use crate::state::CaptureMode;
use crate::gui::theme::Theme;
use eframe::egui;
use smallvec::SmallVec;
use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;

use std::str::FromStr;

const TEXT_TRUNCATE_LEN: usize = 9;

/// Gets localized display name for a capture mode.
fn get_capture_mode_display_name(t: &CachedTranslations, mode: CaptureMode) -> &str {
    match mode {
        CaptureMode::MostSustained => t.capture_mode_most_sustained(),
        CaptureMode::AdaptiveIntelligent => t.capture_mode_adaptive_intelligent(),
        CaptureMode::MaxChangedBits => t.capture_mode_max_changed_bits(),
        CaptureMode::MaxSetBits => t.capture_mode_max_set_bits(),
        CaptureMode::LastStable => t.capture_mode_last_stable(),
        CaptureMode::HatSwitchOptimized => t.capture_mode_hat_switch_optimized(),
        CaptureMode::AnalogOptimized => t.capture_mode_analog_optimized(),
    }
}

/// Safely truncate a string by character count (not byte count) to avoid UTF-8 boundary issues.
fn truncate_text_safe(text: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }
    let char_count = text.chars().count();
    if char_count <= max_chars {
        return text.to_string();
    }
    let take_count = max_chars.saturating_sub(3);
    let truncated: String = text.chars().take(take_count).collect();
    if take_count == 0 {
        "...".chars().take(max_chars).collect()
    } else {
        format!("{}...", truncated)
    }
}

/// Check if target key is a mouse movement action.
fn is_mouse_move_target(target: &str) -> bool {
    let upper = target.to_uppercase();
    matches!(
        upper.as_str(),
        "MOUSE_UP"
            | "MOUSE_DOWN"
            | "MOUSE_LEFT"
            | "MOUSE_RIGHT"
            | "MOUSE_UP_LEFT"
            | "MOUSE_UP_RIGHT"
            | "MOUSE_DOWN_LEFT"
            | "MOUSE_DOWN_RIGHT"
            | "MOUSEUP"
            | "MOUSEDOWN"
            | "MOUSELEFT"
            | "MOUSERIGHT"
            | "MOVE_UP"
            | "MOVE_DOWN"
            | "MOVE_LEFT"
            | "MOVE_RIGHT"
            | "M_UP"
            | "M_DOWN"
            | "M_LEFT"
            | "M_RIGHT"
            | "MOUSEUPLEFT"
            | "MOUSEUPRIGHT"
            | "MOUSEDOWNLEFT"
            | "MOUSEDOWNRIGHT"
            | "M_UP_LEFT"
            | "M_UP_RIGHT"
            | "M_DOWN_LEFT"
            | "M_DOWN_RIGHT"
    )
}

/// Check if target key is a mouse scroll action.
fn is_mouse_scroll_target(target: &str) -> bool {
    let upper = target.to_uppercase();
    matches!(
        upper.as_str(),
        "SCROLL_UP"
            | "SCROLLUP"
            | "WHEEL_UP"
            | "WHEELUP"
            | "SCROLL_DOWN"
            | "SCROLLDOWN"
            | "WHEEL_DOWN"
            | "WHEELDOWN"
    )
}

impl SorahkGui {
    /// Renders the settings dialog for configuration management.
    pub(super) fn render_settings_dialog(&mut self, ctx: &egui::Context) {
        let mut should_save = false;
        let mut should_cancel = false;

        let t = &self.translations;

        // Handle key and mouse capture if in capture mode
        // Priority: Keyboard > Mouse > Raw Input (gamepad/joystick)
        // Skip input handling when HID activation dialog is active
        if self.key_capture_mode != KeyCaptureMode::None && self.hid_activation_dialog.is_none() {
            let mut captured_input: Option<String> = None;

            // Check keyboard input first
            let current_pressed = Self::poll_all_pressed_keys();

            // Filter out keys that were already pressed when capture started (noise baseline)
            current_pressed
                .iter()
                .filter(|&vk| !self.capture_initial_pressed.contains(vk))
                .for_each(|&vk| {
                    // Update tracking set with newly pressed keys only
                    self.capture_pressed_keys.insert(vk);
                });

            // Check if any tracked key has been released
            let any_released = self
                .capture_pressed_keys
                .iter()
                .any(|vk| !current_pressed.contains(vk));

            if any_released {
                // Capture complete: format the result
                captured_input = Self::format_captured_keys(&self.capture_pressed_keys);
            }

            // Check mouse button input ONLY if keyboard not captured
            // Skip mouse capture on the first frame after entering capture mode (just_captured_input flag)
            // to avoid capturing the click on the "Capture" button itself
            if captured_input.is_none() && !self.just_captured_input {
                ctx.input(|i| {
                    if i.pointer.button_clicked(egui::PointerButton::Primary) {
                        captured_input = Some("LBUTTON".to_string());
                    } else if i.pointer.button_clicked(egui::PointerButton::Secondary) {
                        captured_input = Some("RBUTTON".to_string());
                    } else if i.pointer.button_clicked(egui::PointerButton::Middle) {
                        captured_input = Some("MBUTTON".to_string());
                    } else if i.pointer.button_clicked(egui::PointerButton::Extra1) {
                        captured_input = Some("XBUTTON1".to_string());
                    } else if i.pointer.button_clicked(egui::PointerButton::Extra2) {
                        captured_input = Some("XBUTTON2".to_string());
                    }
                });
            }

            // Check Raw Input (gamepad/joystick) ONLY if neither keyboard nor mouse captured
            if captured_input.is_none() {
                let should_check_raw_input = matches!(
                    self.key_capture_mode,
                    KeyCaptureMode::ToggleKey
                        | KeyCaptureMode::MappingTrigger(_)
                        | KeyCaptureMode::NewMappingTrigger
                );

                if should_check_raw_input
                    && let Some(device) = self.app_state.try_recv_raw_input_capture()
                {
                    captured_input = Some(device.to_string());
                }
            }

            if let Some(input_name) = captured_input {
                // Update the appropriate field
                if let Some(temp_config) = &mut self.temp_config {
                    match self.key_capture_mode {
                        KeyCaptureMode::ToggleKey => {
                            temp_config.switch_key = input_name.clone();
                        }
                        KeyCaptureMode::MappingTrigger(idx) => {
                            if let Some(mapping) = temp_config.mappings.get_mut(idx) {
                                mapping.trigger_key = input_name.clone();
                            }
                        }
                        KeyCaptureMode::MappingTarget(idx) => {
                            if let Some(mapping) = temp_config.mappings.get_mut(idx) {
                                mapping.add_target_key(input_name.clone());
                            }
                        }
                        KeyCaptureMode::NewMappingTrigger => {
                            self.new_mapping_trigger = input_name.clone();
                        }
                        KeyCaptureMode::NewMappingTarget => {
                            self.new_mapping_target = input_name.clone();
                            if !self.new_mapping_target_keys.contains(&input_name) {
                                self.new_mapping_target_keys.push(input_name);
                            }
                        }
                        KeyCaptureMode::QuickGamepadTrigger(_)
                        | KeyCaptureMode::QuickGamepadTarget(_)
                        | KeyCaptureMode::None => {}
                    }
                }
                // Exit capture mode and clear capture state
                self.key_capture_mode = KeyCaptureMode::None;
                self.capture_pressed_keys.clear();
                self.app_state.set_raw_input_capture_mode(false);
                self.just_captured_input = false;
            } else {
                // In capture mode but no input captured yet - clear the flag after first frame
                if self.just_captured_input {
                    self.just_captured_input = false;
                }
            }
        } else {
            // Not in capture mode: ensure state is clean
            self.capture_pressed_keys.clear();
            self.just_captured_input = false;
        }

        let dialog_bg = if self.dark_mode {
            egui::Color32::from_rgb(20, 22, 28) // #151823
        } else {
            egui::Color32::from_rgb(255, 255, 255) // #FFFFFF
        };

        let accent_color = Theme::new(self.dark_mode).accent_text;

        egui::Window::new("")
            .title_bar(false)
            .collapsible(false)
            .resizable(true)
            .default_size([800.0, 600.0])
            .min_size([780.0, 560.0])
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .id(egui::Id::new("settings_dialog_window"))
            .frame(
                egui::Frame::window(&ctx.style())
                    .fill(dialog_bg)
                    .corner_radius(egui::CornerRadius::same(16))
                    .stroke(egui::Stroke::NONE)
                    .shadow(egui::epaint::Shadow {
                        offset: [0, 5],
                        blur: 22,
                        spread: 2,
                        color: egui::Color32::from_rgba_premultiplied(0, 0, 0, 45),
                    }),
            )
            .show(ctx, |ui| {
                ui.push_id("settings_dialog_scope", |ui| {
                    // Custom title bar (matching main window style)
                    ui.horizontal(|ui| {
                        ui.add_space(15.0);

                        // Settings title - 统一紫色强调
                        ui.label(
                            egui::RichText::new(t.settings_dialog_title())
                                .size(18.0)
                                .strong()
                                .color(accent_color),
                        );

                        // Push close button to the right
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.add_space(10.0);

                            // Close button (matching style)
                            let close_btn =
                                egui::Button::new(egui::RichText::new("x").size(16.0).color(
                                    accent_color,
                                ))
                                .corner_radius(8.0)
                                .frame(false);

                            if ui.add(close_btn).clicked() {
                                should_cancel = true;
                            }
                        });
                    });

                    ui.add_space(12.0);

                    // Wrap ScrollArea in a frame with padding (like main window)
                    let temp_config = self.temp_config.as_mut().unwrap();
                    egui::Frame::NONE
                        .inner_margin(egui::Margin::symmetric(12, 0))
                        .show(ui, |ui| {
                            egui::ScrollArea::vertical()
                                .max_height(500.0)
                                .show(ui, |ui| {
                                    // Toggle Key Section
                                    let card_bg = if self.dark_mode {
                                        egui::Color32::from_rgb(26, 29, 37) // #1C1F2D
                                    } else {
                                        egui::Color32::from_rgb(255, 255, 255) // #FFFFFF
                                    };

                                    egui::Frame::NONE
                                        .fill(card_bg)
                                        .corner_radius(egui::CornerRadius::same(14))
                                        .inner_margin(egui::Margin::same(16))
                                        .show(ui, |ui| {
                                            ui.set_min_width(ui.available_width());
                                            ui.label(
                                                egui::RichText::new(t.toggle_key())
                                                    .size(16.0)
                                                    .strong()
                                                    .color(accent_color),
                                            );
                                            ui.add_space(8.0);

                                            ui.horizontal(|ui| {
                                                ui.label(
                                                    egui::RichText::new(t.key_label())
                                                        .size(14.0)
                                                        .color(Theme::new(self.dark_mode).text),
                                                );
                                                ui.add_space(8.0);

                                                let is_capturing = self.key_capture_mode
                                                    == KeyCaptureMode::ToggleKey;
                                                let button_text = if is_capturing {
                                                    t.press_any_key()
                                                } else if temp_config.switch_key.is_empty() {
                                                    t.click_to_set()
                                                } else {
                                                    &temp_config.switch_key
                                                };

                                                let button = egui::Button::new(
                                                    egui::RichText::new(button_text).size(13.0).color(
                                                        if is_capturing {
                                                            egui::Color32::WHITE
                                                        } else { Theme::new(self.dark_mode).text },
                                                    ),
                                                )
                                                .fill(if is_capturing {
                                                    Theme::new(self.dark_mode).accent // 捕获中: 强调色
                                                } else if self.dark_mode {
                                                    egui::Color32::from_rgb(33, 36, 46) // #232838 输入框底色
                                                } else {
                                                    egui::Color32::from_rgb(231, 233, 238) // #F0F1F6
                                                })
                                                .corner_radius(8.0); // Increased rounding to match buttons

                                                if ui.add_sized([200.0, 34.0], button).clicked()
                                                    && !self.just_captured_input
                                                {
                                                    self.key_capture_mode =
                                                        KeyCaptureMode::ToggleKey;
                                                    self.capture_pressed_keys.clear();
                                                    self.capture_initial_pressed =
                                                        Self::poll_all_pressed_keys();
                                                    self.app_state.set_raw_input_capture_mode(true);
                                                    // Set flag to skip mouse capture on this frame
                                                    self.just_captured_input = true;
                                                }
                                            });
                                        });

                                    ui.add_space(10.0);

                                    // Preset Management Section
                                    let card_bg = if self.dark_mode {
                                        egui::Color32::from_rgb(26, 29, 37) // #1C1F2D
                                    } else {
                                        egui::Color32::from_rgb(255, 255, 255) // #FFFFFF
                                    };

                                    egui::Frame::NONE
                                        .fill(card_bg)
                                        .corner_radius(egui::CornerRadius::same(14))
                                        .inner_margin(egui::Margin::same(16))
                                        .show(ui, |ui| {
                                            ui.set_min_width(ui.available_width());
                                            ui.label(
                                                egui::RichText::new(t.preset_title())
                                                    .size(16.0)
                                                    .strong()
                                                    .color(accent_color),
                                            );
                                            ui.add_space(8.0);

                                            ui.horizontal(|ui| {
                                                // Current preset display / selection
                                                let current_name = if temp_config.current_preset.is_empty() {
                                                    t.no_preset().to_string()
                                                } else {
                                                    temp_config.current_preset.clone()
                                                };

                                                egui::ComboBox::from_id_salt("preset_selector")
                                                    .selected_text(&current_name)
                                                    .width(180.0)
                                                    .show_ui(ui, |ui| {
                                                        if ui.selectable_label(temp_config.current_preset.is_empty(), t.no_preset()).clicked() {
                                                            temp_config.current_preset.clear();
                                                            self.preset_rename_target.clear();
                                                            self.preset_rename_input.clear();
                                                        }
                                                        for preset in &temp_config.presets {
                                                            let is_selected = temp_config.current_preset == preset.name;
                                                            if ui.selectable_label(is_selected, &preset.name).clicked() {
                                                                temp_config.current_preset = preset.name.clone();
                                                                temp_config.mappings = preset.mappings.clone();
                                                                self.preset_rename_target.clear();
                                                                self.preset_rename_input.clear();
                                                            }
                                                        }
                                                    });

                                                ui.add_space(8.0);

                                                // Save preset button - 紫色强调
                                                let save_btn = egui::Button::new(
                                                    egui::RichText::new(t.preset_save_btn())
                                                        .size(13.0)
                                                        .color(egui::Color32::WHITE)
                                                        .strong(),
                                                )
                                                .fill(Theme::new(self.dark_mode).btn_primary)
                                                .corner_radius(8.0);

                                                if ui.add_sized([90.0, 34.0], save_btn).clicked() {
                                                    self.show_preset_name_input = true;
                                                }
                                            });

                                            // Preset name input (shown after clicking save)
                                            if self.show_preset_name_input {
                                                ui.add_space(6.0);
                                                ui.horizontal(|ui| {
                                                    let name_edit = egui::TextEdit::singleline(
                                                        &mut self.preset_name_input,
                                                    )
                                                    .background_color(if self.dark_mode {
                                                        egui::Color32::from_rgb(33, 36, 46) // #232838
                                                    } else {
                                                        egui::Color32::from_rgb(231, 233, 238) // #F0F1F6
                                                    })
                                                    .hint_text(t.preset_name_hint())
                                                    .desired_width(160.0);
                                                    ui.add_sized([160.0, 32.0], name_edit);

                                                    let confirm_btn = egui::Button::new(
                                                        egui::RichText::new("✓")
                                                            .size(14.0)
                                                            .color(egui::Color32::WHITE),
                                                    )
                                                    .fill(Theme::new(self.dark_mode).good)
                                                    .corner_radius(8.0);

                                                    if ui.add_sized([36.0, 32.0], confirm_btn).clicked() {
                                                        let name = self.preset_name_input.trim();
                                                        if !name.is_empty() {
                                                            // Remove existing preset with same name
                                                            temp_config.presets.retain(|p| p.name != name);
                                                            // Save current mappings as preset
                                                            temp_config.presets.push(crate::config::Preset {
                                                                name: name.to_string(),
                                                                mappings: temp_config.mappings.clone(),
                                                            });
                                                            temp_config.current_preset = name.to_string();
                                                            self.preset_name_input.clear();
                                                            self.show_preset_name_input = false;
                                                        }
                                                    }

                                                    let cancel_name_btn = egui::Button::new(
                                                        egui::RichText::new("✕")
                                                            .size(14.0)
                                                            .color(egui::Color32::WHITE),
                                                    )
                                                    .fill(Theme::new(self.dark_mode).btn_danger)
                                                    .corner_radius(8.0);

                                                    if ui.add_sized([36.0, 32.0], cancel_name_btn).clicked() {
                                                        self.preset_name_input.clear();
                                                        self.show_preset_name_input = false;
                                                    }
                                                });
                                            }

                                            // Delete and Rename preset buttons (only show if a preset is selected)
                                            if !temp_config.current_preset.is_empty() {
                                                ui.add_space(6.0);
                                                ui.horizontal(|ui| {
                                                    let delete_btn = egui::Button::new(
                                                        egui::RichText::new(t.preset_delete_btn())
                                                            .size(13.0)
                                                            .color(egui::Color32::WHITE)
                                                            .strong(),
                                                    )
                                                    .fill(Theme::new(self.dark_mode).btn_danger)
                                                    .corner_radius(8.0);

                                                    if ui.add_sized([88.0, 32.0], delete_btn).clicked() {
                                                        let name = temp_config.current_preset.clone();
                                                        temp_config.presets.retain(|p| p.name != name);
                                                        temp_config.current_preset.clear();
                                                        self.preset_rename_target.clear();
                                                        self.preset_rename_input.clear();
                                                    }

                                                    let rename_btn = egui::Button::new(
                                                        egui::RichText::new(t.preset_rename_btn())
                                                            .size(13.0)
                                                            .color(egui::Color32::WHITE)
                                                            .strong(),
                                                    )
                                                    .fill(Theme::new(self.dark_mode).btn_primary)
                                                    .corner_radius(8.0);

                                                    if ui.add_sized([88.0, 32.0], rename_btn).clicked() {
                                                        self.preset_rename_target = temp_config.current_preset.clone();
                                                        self.preset_rename_input = temp_config.current_preset.clone();
                                                    }
                                                });

                                                // Rename input (shown after clicking rename)
                                                if !self.preset_rename_target.is_empty() {
                                                    ui.add_space(6.0);
                                                    ui.horizontal(|ui| {
                                                        let rename_edit = egui::TextEdit::singleline(
                                                            &mut self.preset_rename_input,
                                                        )
                                                        .background_color(if self.dark_mode {
                                                            egui::Color32::from_rgb(33, 36, 46) // #232838
                                                        } else {
                                                            egui::Color32::from_rgb(231, 233, 238) // #F0F1F6
                                                        })
                                                        .hint_text(t.preset_name_hint())
                                                        .desired_width(140.0);
                                                        ui.add_sized([140.0, 32.0], rename_edit);

                                                        let confirm_btn = egui::Button::new(
                                                            egui::RichText::new("✓")
                                                                .size(14.0)
                                                                .color(egui::Color32::WHITE),
                                                        )
                                                        .fill(Theme::new(self.dark_mode).good)
                                                        .corner_radius(8.0);

                                                        if ui.add_sized([36.0, 32.0], confirm_btn).clicked() {
                                                            let new_name = self.preset_rename_input.trim();
                                                            if !new_name.is_empty() && new_name != self.preset_rename_target {
                                                                let old_name = self.preset_rename_target.clone();
                                                                if let Some(preset) = temp_config.presets.iter_mut().find(|p| p.name == old_name) {
                                                                    preset.name = new_name.to_string();
                                                                }
                                                                if temp_config.current_preset == old_name {
                                                                    temp_config.current_preset = new_name.to_string();
                                                                }
                                                            }
                                                            self.preset_rename_target.clear();
                                                            self.preset_rename_input.clear();
                                                        }

                                                        let cancel_btn = egui::Button::new(
                                                            egui::RichText::new("✕")
                                                                .size(14.0)
                                                                .color(egui::Color32::WHITE),
                                                        )
                                                        .fill(Theme::new(self.dark_mode).btn_danger)
                                                        .corner_radius(8.0);

                                                        if ui.add_sized([36.0, 32.0], cancel_btn).clicked() {
                                                            self.preset_rename_target.clear();
                                                            self.preset_rename_input.clear();
                                                        }
                                                    });
                                                }
                                            }
                                        });

                                    ui.add_space(10.0);

                                    // Global Configuration Section
                                    let card_bg = Theme::new(self.dark_mode).card;

                                    egui::Frame::NONE
                                        .fill(card_bg)
                                        .corner_radius(egui::CornerRadius::same(14))
                                        .inner_margin(egui::Margin::same(16))
                                        .show(ui, |ui| {
                                            ui.set_min_width(ui.available_width());
                                            ui.label(
                                                egui::RichText::new(t.global_config_title())
                                                    .size(16.0)
                                                    .strong()
                                                    .color(accent_color),
                                            );
                                            ui.add_space(6.0);

                                            let available = ui.available_width();
                                            egui::Grid::new("config_edit_grid")
                                                .num_columns(2)
                                                .spacing([20.0, 8.0])
                                                .min_col_width(available * 0.35)
                                                .show(ui, |ui| {
                                                    // Language
                                                    ui.label(t.language());
                                                    egui::ComboBox::from_id_salt(
                                                        "language_selector",
                                                    )
                                                    .selected_text(
                                                        temp_config.language.display_name(),
                                                    )
                                                    .width(120.0)
                                                    .show_ui(ui, |ui| {
                                                        use crate::i18n::Language;
                                                        for lang in Language::all() {
                                                            ui.selectable_value(
                                                                &mut temp_config.language,
                                                                *lang,
                                                                lang.display_name(),
                                                            );
                                                        }
                                                    });
                                                    ui.end_row();

                                                    // Raw Input Capture Mode selector
                                                    ui.label(t.rawinput_capture_mode_label());
                                                    let current_mode_str = &temp_config.rawinput_capture_mode;
                                                    let current_mode = CaptureMode::from_str(current_mode_str).unwrap();
                                                    let current_mode_name = get_capture_mode_display_name(t, current_mode);
                                                    egui::ComboBox::from_id_salt("rawinput_capture_mode")
                                                        .selected_text(current_mode_name)
                                                        .width(180.0)
                                                        .show_ui(ui, |ui| {
                                                            for &mode in CaptureMode::all_modes() {
                                                                let mode_name = get_capture_mode_display_name(t, mode);
                                                                let is_selected = temp_config.rawinput_capture_mode == mode.as_str();
                                                                if ui.selectable_label(is_selected, mode_name).clicked() {
                                                                    temp_config.rawinput_capture_mode = mode.as_str().to_string();
                                                                }
                                                            }
                                                        });
                                                    ui.end_row();

                                                    // XInput Capture Mode selector
                                                    ui.label(t.xinput_capture_mode_label());
                                                    let current_mode_str = &temp_config.xinput_capture_mode;
                                                    let current_mode = crate::config::XInputCaptureMode::from_str(current_mode_str).unwrap();
                                                    let current_mode_name = match current_mode {
                                                        crate::config::XInputCaptureMode::MostSustained => t.capture_mode_most_sustained(),
                                                        crate::config::XInputCaptureMode::LastStable => t.capture_mode_last_stable(),
                                                        crate::config::XInputCaptureMode::DiagonalPriority => t.capture_mode_diagonal_priority(),
                                                    };
                                                    egui::ComboBox::from_id_salt("xinput_capture_mode")
                                                        .selected_text(current_mode_name)
                                                        .width(180.0)
                                                        .show_ui(ui, |ui| {
                                                            for &mode in crate::config::XInputCaptureMode::all_modes() {
                                                                let mode_name = match mode {
                                                                    crate::config::XInputCaptureMode::MostSustained => t.capture_mode_most_sustained(),
                                                                    crate::config::XInputCaptureMode::LastStable => t.capture_mode_last_stable(),
                                                                    crate::config::XInputCaptureMode::DiagonalPriority => t.capture_mode_diagonal_priority(),
                                                                };
                                                                let is_selected = temp_config.xinput_capture_mode == mode.as_str();
                                                                if ui.selectable_label(is_selected, mode_name).clicked() {
                                                                    temp_config.xinput_capture_mode = mode.as_str().to_string();
                                                                }
                                                            }
                                                        });
                                                    ui.end_row();

                                                    ui.label(t.input_timeout_label());
                                                    let mut timeout_str =
                                                        temp_config.input_timeout.to_string();
                                                    ui.add_sized(
                                                        [140.0, 30.0],
                                                        egui::TextEdit::singleline(
                                                            &mut timeout_str,
                                                        )
                                                        .background_color(Theme::new(self.dark_mode).card_alt),
                                                    );
                                                    if let Ok(val) = timeout_str.parse::<u64>() {
                                                        temp_config.input_timeout = val;
                                                    }
                                                    ui.end_row();

                                                    ui.label(t.default_interval_label());
                                                    let mut interval_str =
                                                        temp_config.interval.to_string();
                                                    ui.add_sized(
                                                        [140.0, 30.0],
                                                        egui::TextEdit::singleline(
                                                            &mut interval_str,
                                                        )
                                                        .background_color(Theme::new(self.dark_mode).card_alt),
                                                    );
                                                    if let Ok(val) = interval_str.parse::<u64>() {
                                                        temp_config.interval = val.max(5);
                                                    }
                                                    ui.end_row();

                                                    ui.label(t.default_duration_label());
                                                    let mut duration_str =
                                                        temp_config.event_duration.to_string();
                                                    ui.add_sized(
                                                        [140.0, 30.0],
                                                        egui::TextEdit::singleline(
                                                            &mut duration_str,
                                                        )
                                                        .background_color(Theme::new(self.dark_mode).card_alt),
                                                    );
                                                    if let Ok(val) = duration_str.parse::<u64>() {
                                                        temp_config.event_duration = val.max(2);
                                                    }
                                                    ui.end_row();

                                                    ui.label(t.worker_count_label());
                                                    let mut worker_str =
                                                        temp_config.worker_count.to_string();
                                                    ui.add_sized(
                                                        [140.0, 30.0],
                                                        egui::TextEdit::singleline(&mut worker_str)
                                                            .hint_text("0 = auto")
                                                            .background_color(if self.dark_mode {
                                                                egui::Color32::from_rgb(33, 36, 46)
                                                            } else {
                                                                egui::Color32::from_rgb(
                                                                    240, 241, 246,
                                                                )
                                                            }),
                                                    );
                                                    if let Ok(val) = worker_str.parse::<usize>() {
                                                        temp_config.worker_count = val;
                                                    }
                                                    ui.end_row();

                                                    ui.label(t.show_tray_icon());
                                                    ui.checkbox(
                                                        &mut temp_config.show_tray_icon,
                                                        "",
                                                    );
                                                    ui.end_row();

                                                    ui.label(t.show_notifications());
                                                    ui.checkbox(
                                                        &mut temp_config.show_notifications,
                                                        "",
                                                    );
                                                    ui.end_row();

                                                    ui.label(t.always_on_top());
                                                    ui.checkbox(&mut temp_config.always_on_top, "");
                                                    ui.end_row();

                                                    ui.label(t.dfo_vibration_feature());
                                                    ui.checkbox(&mut temp_config.dfo_player, "");
                                                    ui.end_row();

                                                    /* ★客户端版本路线 (仅 DFO 玩家可用):
                                                     * S1 ACT → 老方案 (配老版 DLL 事件语义);
                                                     * S4+ 新版 → 现行新方案。下拉选择。 */
                                                    ui.label(
                                                        egui::RichText::new("客户端版本 (震动方案)")
                                                            .size(13.0),
                                                    );
                                                    ui.add_enabled_ui(
                                                        temp_config.dfo_player,
                                                        |ui| {
                                                            let routes =
                                                                ["S4+ 新版本方案 (推荐)", "S1 ACT 老版本方案"];
                                                            let idx =
                                                                if temp_config.vib_legacy_client {
                                                                    1
                                                                } else {
                                                                    0
                                                                };
                                                            egui::ComboBox::from_id_salt(
                                                                "settings_client_edition",
                                                            )
                                                            .selected_text(routes[idx])
                                                            .width(190.0)
                                                            .show_ui(ui, |ui| {
                                                                ui.selectable_value(
                                                                    &mut temp_config.vib_legacy_client,
                                                                    false,
                                                                    routes[0],
                                                                );
                                                                ui.selectable_value(
                                                                    &mut temp_config.vib_legacy_client,
                                                                    true,
                                                                    routes[1],
                                                                );
                                                            });
                                                        },
                                                    );
                                                    ui.end_row();

                                                    ui.label(t.dark_mode());
                                                    ui.checkbox(&mut temp_config.dark_mode, "");
                                                    ui.end_row();
                                                });
                                        });

                                    ui.add_space(10.0);

                                    // Key Mappings Section
                                    let card_bg = Theme::new(self.dark_mode).card;

                                    egui::Frame::NONE
                                        .fill(card_bg)
                                        .corner_radius(egui::CornerRadius::same(14))
                                        .inner_margin(egui::Margin::same(16))
                                        .show(ui, |ui| {
                                            ui.set_min_width(ui.available_width());
                                            ui.label(
                                                egui::RichText::new(t.key_mappings_title())
                                                    .size(16.0)
                                                    .strong()
                                                    .color(accent_color),
                                            );
                                            ui.add_space(6.0);

                                            ui.add_space(2.0);
                                            let hint_bg = if self.dark_mode {
                                                egui::Color32::from_rgba_premultiplied(30, 34, 50, 220)
                                            } else {
                                                egui::Color32::from_rgba_premultiplied(240, 241, 246, 230)
                                            };
                                            ui.horizontal(|ui| {
                                                egui::Frame::NONE
                                                    .fill(hint_bg)
                                                    .corner_radius(egui::CornerRadius::same(12))
                                                    .inner_margin(egui::Margin::symmetric(10, 6))
                                                    .show(ui, |ui| {
                                                        ui.set_width(ui.available_width());
                                                        egui::CollapsingHeader::new(
                                                            egui::RichText::new(t.diagonal_hint_title())
                                                                .size(12.0)
                                                                .color(accent_color),
                                                        )
                                                        .default_open(true)
                                                        .show(ui, |ui| {
                                                            ui.add_space(2.0);
                                                            ui.add(
                                                                egui::Label::new(
                                                                    egui::RichText::new(
                                                                        t.diagonal_hint(),
                                                                    )
                                                                    .size(11.0)
                                                                    .color(if self.dark_mode {
                                                                        egui::Color32::from_rgb(
                                                                            230, 230, 230,
                                                                        )
                                                                    } else {
                                                                        egui::Color32::from_rgb(
                                                                            180, 100, 50,
                                                                        )
                                                                    }),
                                                                )
                                                                .wrap(),
                                                            );
                                                        });
                                                    });
                                            });
                                            ui.add_space(4.0);
                                            // Existing mappings
                                            let mut to_remove = None;
                                            for (idx, mapping) in
                                                temp_config.mappings.iter_mut().enumerate()
                                            {
                                                let is_capturing_trigger = self
                                                    .key_capture_mode
                                                    == KeyCaptureMode::MappingTrigger(idx);
                                                let is_capturing_target = self.key_capture_mode
                                                    == KeyCaptureMode::MappingTarget(idx);
                                                let full_trigger_text = mapping.trigger_key.clone();
                                                let target_display = mapping.target_keys_display();
                                                let first_target = mapping
                                                    .get_target_keys()
                                                    .first()
                                                    .map(|s| s.as_str())
                                                    .unwrap_or("");
                                                let is_mouse_move_mapping =
                                                    is_mouse_move_target(first_target);
                                                let is_mouse_scroll_mapping =
                                                    is_mouse_scroll_target(first_target);
                                                let row_text_color = Theme::new(self.dark_mode).text;
                                                let input_bg = if self.dark_mode {
                                                    egui::Color32::from_rgb(33, 36, 46) // #232838
                                                } else {
                                                    egui::Color32::from_rgb(231, 233, 238) // #F0F1F6
                                                };
                                                let inner_bg = Theme::new(self.dark_mode).surface;

                                                // 每条映射独立分组卡片: 触发键/目标键分行展示, 长按键名不再溢出
                                                egui::Frame::NONE
                                                    .fill(inner_bg)
                                                    .corner_radius(egui::CornerRadius::same(12))
                                                    .inner_margin(egui::Margin::same(12))
                                                    .show(ui, |ui| {
                                                        // 行 1: 序号 + 触发键
                                                        ui.horizontal(|ui| {
                                                            ui.add_sized(
                                                                [26.0, 34.0],
                                                                egui::Label::new(
                                                                    egui::RichText::new(format!("{}.", idx + 1))
                                                                        .size(14.0)
                                                                        .strong()
                                                                        .color(Theme::new(self.dark_mode).text_weak),
                                                                ),
                                                            );
                                                            ui.label(
                                                                egui::RichText::new(t.trigger_short())
                                                                    .size(13.0)
                                                                    .strong()
                                                                    .color(accent_color),
                                                            );
                                                            ui.add_space(6.0);

                                                            let trigger_text = if is_capturing_trigger {
                                                                t.press_any_key()
                                                            } else {
                                                                full_trigger_text.as_str()
                                                            };
                                                            let display_text = truncate_text_safe(
                                                                trigger_text,
                                                                TEXT_TRUNCATE_LEN,
                                                            );
                                                            let trigger_btn = egui::Button::new(
                                                                egui::RichText::new(&display_text)
                                                                    .size(13.0)
                                                                    .color(if is_capturing_trigger {
                                                                        egui::Color32::WHITE
                                                                    } else {
                                                                        row_text_color
                                                                    }),
                                                            )
                                                            .fill(if is_capturing_trigger {
                                                                Theme::new(self.dark_mode).accent
                                                            } else {
                                                                input_bg
                                                            })
                                                            .corner_radius(8.0);
                                                            let mut response =
                                                                ui.add_sized([170.0, 34.0], trigger_btn);
                                                            // Show full text on hover if truncated
                                                            if !is_capturing_trigger
                                                                && trigger_text.len() > TEXT_TRUNCATE_LEN
                                                            {
                                                                response =
                                                                    response.on_hover_text(trigger_text);
                                                            }
                                                            if response.clicked()
                                                                && !self.just_captured_input
                                                            {
                                                                self.key_capture_mode =
                                                                    KeyCaptureMode::MappingTrigger(idx);
                                                                self.capture_pressed_keys.clear();
                                                                self.capture_initial_pressed =
                                                                    Self::poll_all_pressed_keys();
                                                                self.app_state
                                                                    .set_raw_input_capture_mode(true);
                                                                // Set flag to skip mouse capture on this frame
                                                                self.just_captured_input = true;
                                                            }
                                                        });
                                                        ui.add_space(6.0);

                                                        // 行 2: 目标键 + 添加/清空
                                                        ui.horizontal(|ui| {
                                                            ui.add_space(26.0);
                                                            ui.label(
                                                                egui::RichText::new(t.target_short())
                                                                    .size(13.0)
                                                                    .strong()
                                                                    .color(accent_color),
                                                            );
                                                            ui.add_space(6.0);

                                                            let target_text = if is_capturing_target {
                                                                t.press_any_key()
                                                            } else if target_display.is_empty() {
                                                                t.click_to_set()
                                                            } else {
                                                                target_display.as_str()
                                                            };
                                                            let display_target_text = if is_capturing_target {
                                                                target_text.to_string()
                                                            } else {
                                                                truncate_text_safe(
                                                                    target_text,
                                                                    TEXT_TRUNCATE_LEN,
                                                                )
                                                            };
                                                            let target_btn = egui::Button::new(
                                                                egui::RichText::new(&display_target_text)
                                                                    .size(13.0)
                                                                    .color(if is_capturing_target {
                                                                        egui::Color32::WHITE
                                                                    } else {
                                                                        row_text_color
                                                                    }),
                                                            )
                                                            .fill(if is_capturing_target {
                                                                Theme::new(self.dark_mode).accent
                                                            } else {
                                                                input_bg
                                                            })
                                                            .corner_radius(8.0);
                                                            let mut target_response = ui
                                                                .add_sized([170.0, 34.0], target_btn);
                                                            if !is_capturing_target
                                                                && target_text.len() > TEXT_TRUNCATE_LEN
                                                            {
                                                                target_response = target_response
                                                                    .on_hover_text(target_text);
                                                            }
                                                            if target_response.clicked()
                                                                && !self.just_captured_input
                                                            {
                                                                self.key_capture_mode =
                                                                    KeyCaptureMode::MappingTarget(idx);
                                                                self.capture_pressed_keys.clear();
                                                                self.capture_initial_pressed =
                                                                    Self::poll_all_pressed_keys();
                                                            }
                                                            ui.add_space(8.0);

                                                            // Button 1: Add target key with capture
                                                            let add_target_btn = egui::Button::new(
                                                                egui::RichText::new("+")
                                                                    .color(egui::Color32::WHITE)
                                                                    .size(16.0),
                                                            )
                                                            .fill(Theme::new(self.dark_mode).accent)
                                                            .corner_radius(8.0);
                                                            if ui
                                                                .add_sized([36.0, 34.0], add_target_btn)
                                                                .on_hover_text(t.add_target_key_hover())
                                                                .clicked()
                                                            {
                                                                self.key_capture_mode =
                                                                    KeyCaptureMode::MappingTarget(idx);
                                                                self.capture_pressed_keys.clear();
                                                                self.capture_initial_pressed =
                                                                    Self::poll_all_pressed_keys();
                                                            }
                                                            // Button 1.5: Clear all target keys
                                                            let clear_btn = egui::Button::new(
                                                                egui::RichText::new("✖")
                                                                    .color(egui::Color32::WHITE)
                                                                    .size(14.0),
                                                            )
                                                            .fill(Theme::new(self.dark_mode).btn_danger)
                                                            .corner_radius(8.0);
                                                            if ui
                                                                .add_sized([36.0, 34.0], clear_btn)
                                                                .on_hover_text(t.clear_all_target_keys_hover())
                                                                .clicked()
                                                            {
                                                                mapping.clear_target_keys();
                                                            }
                                                        });

                                                        // 多目标键 chips
                                                        if mapping.get_target_keys().len() > 1 {
                                                            ui.add_space(4.0);
                                                            ui.horizontal(|ui| {
                                                                ui.add_space(26.0);
                                                                ui.label(
                                                                    egui::RichText::new("🎯")
                                                                        .size(12.0)
                                                                        .color(Theme::new(self.dark_mode).text_weak),
                                                                );
                                                                ui.horizontal_wrapped(|ui| {
                                                                    ui.spacing_mut().item_spacing.x = 4.0;
                                                                    let mut key_to_remove: Option<String> = None;
                                                                    for (i, target_key) in mapping.get_target_keys().iter().enumerate() {
                                                                        if i > 0 {
                                                                            ui.label(
                                                                                egui::RichText::new("·")
                                                                                    .size(12.0)
                                                                                    .color(egui::Color32::from_rgb(150, 150, 150)),
                                                                            );
                                                                        }
                                                                        let key_chip = egui::Button::new(
                                                                            egui::RichText::new(format!("{} ✕", target_key))
                                                                                .size(12.0)
                                                                                .color(Theme::new(self.dark_mode).bad),
                                                                        )
                                                                        .fill(Theme::new(self.dark_mode).card_alt)
                                                                        .corner_radius(8.0)
                                                                        .min_size(egui::vec2(0.0, 24.0))
                                                                        .frame(true);
                                                                        if ui.add(key_chip)
                                                                            .on_hover_text(t.format_remove_target_key_hover(target_key))
                                                                            .clicked()
                                                                        {
                                                                            key_to_remove = Some(target_key.clone());
                                                                        }
                                                                    }
                                                                    if let Some(key) = key_to_remove {
                                                                        mapping.remove_target_key(&key);
                                                                    }
                                                                });
                                                            });
                                                        }

                                                        // 行 3: 鼠标移动/滚轮参数 (仅此类映射)
                                                        if is_mouse_move_mapping || is_mouse_scroll_mapping {
                                                            ui.add_space(6.0);
                                                            ui.horizontal(|ui| {
                                                                ui.add_space(26.0);
                                                                ui.label(
                                                                    egui::RichText::new(t.interval_short())
                                                                        .size(13.0)
                                                                        .color(row_text_color),
                                                                );
                                                                let mut interval_str = mapping
                                                                    .interval
                                                                    .unwrap_or(temp_config.interval)
                                                                    .to_string();
                                                                let interval_edit = egui::TextEdit::singleline(
                                                                    &mut interval_str,
                                                                )
                                                                .background_color(input_bg)
                                                                .desired_width(50.0)
                                                                .font(egui::TextStyle::Button);
                                                                if ui
                                                                    .add_sized([50.0, 30.0], interval_edit)
                                                                    .changed()
                                                                    && let Ok(val) = interval_str.parse::<u64>()
                                                                {
                                                                    mapping.interval = Some(val.max(5));
                                                                }
                                                                ui.add_space(8.0);
                                                                ui.label(
                                                                    egui::RichText::new(t.speed_label())
                                                                        .size(13.0)
                                                                        .color(row_text_color),
                                                                );
                                                                let mut speed_str = mapping
                                                                    .move_speed
                                                                    .to_string();
                                                                let speed_edit = egui::TextEdit::singleline(
                                                                    &mut speed_str,
                                                                )
                                                                .background_color(input_bg)
                                                                .desired_width(50.0)
                                                                .font(egui::TextStyle::Button);
                                                                let max_val = if is_mouse_scroll_mapping { 1200 } else { 100 };
                                                                if ui
                                                                    .add_sized([50.0, 30.0], speed_edit)
                                                                    .changed()
                                                                    && let Ok(val) = speed_str.parse::<i32>()
                                                                {
                                                                    mapping.move_speed = val.clamp(1, max_val);
                                                                }
                                                                ui.add_space(8.0);
                                                                ui.label(
                                                                    egui::RichText::new(t.duration_short())
                                                                        .size(13.0)
                                                                        .color(row_text_color),
                                                                );
                                                                let mut duration_str = mapping
                                                                    .event_duration
                                                                    .unwrap_or(temp_config.event_duration)
                                                                    .to_string();
                                                                let duration_edit = egui::TextEdit::singleline(
                                                                    &mut duration_str,
                                                                )
                                                                .background_color(input_bg)
                                                                .desired_width(50.0)
                                                                .font(egui::TextStyle::Button);
                                                                if ui
                                                                    .add_sized([50.0, 30.0], duration_edit)
                                                                    .changed()
                                                                    && let Ok(val) = duration_str.parse::<u64>()
                                                                {
                                                                    mapping.event_duration = Some(val.max(2));
                                                                }
                                                            });
                                                        }

                                                        // 行 4: 操作按钮 (方向/滚动/连发/双击/删除)
                                                        ui.add_space(6.0);
                                                        ui.horizontal(|ui| {
                                                            ui.add_space(26.0);
                                                            // Button 2: Mouse movement direction
                                                            let move_btn = egui::Button::new(
                                                                egui::RichText::new("⌖ 方向")
                                                                    .color(egui::Color32::WHITE)
                                                                    .size(13.0),
                                                            )
                                                            .fill(Theme::new(self.dark_mode).accent_hover)
                                                            .corner_radius(8.0);
                                                            if ui
                                                                .add_sized([88.0, 32.0], move_btn)
                                                                .on_hover_text(t.set_mouse_direction_hover())
                                                                .clicked()
                                                            {
                                                                self.mouse_direction_dialog = Some(
                                                                    crate::gui::mouse_direction_dialog::MouseDirectionDialog::new(),
                                                                );
                                                                self.mouse_direction_mapping_idx = Some(idx);
                                                            }
                                                            ui.add_space(4.0);
                                                            // Button 3: Mouse scroll direction
                                                            let scroll_btn = egui::Button::new(
                                                                egui::RichText::new("🎡 滚动")
                                                                    .color(egui::Color32::WHITE)
                                                                    .size(13.0),
                                                            )
                                                            .fill(Theme::new(self.dark_mode).good)
                                                            .corner_radius(8.0);
                                                            if ui
                                                                .add_sized([88.0, 32.0], scroll_btn)
                                                                .on_hover_text(t.set_mouse_scroll_direction_hover())
                                                                .clicked()
                                                            {
                                                                self.mouse_scroll_dialog = Some(
                                                                    crate::gui::mouse_scroll_dialog::MouseScrollDialog::new(),
                                                                );
                                                                self.mouse_scroll_mapping_idx = Some(idx);
                                                            }
                                                            ui.add_space(4.0);
                                                            // Turbo toggle
                                                            let turbo_enabled = mapping.turbo_enabled;
                                                            let turbo_color = if turbo_enabled {
                                                                Theme::new(self.dark_mode).accent
                                                            } else { Theme::new(self.dark_mode).hint };
                                                            let turbo_icon =
                                                                if turbo_enabled { "⚡ 连发" } else { "○ 连发" };
                                                            let turbo_btn = egui::Button::new(
                                                                egui::RichText::new(turbo_icon)
                                                                    .color(egui::Color32::WHITE)
                                                                    .size(12.0),
                                                            )
                                                            .fill(turbo_color)
                                                            .corner_radius(8.0)
                                                            .sense(egui::Sense::click());
                                                            let hover_text = if turbo_enabled {
                                                                self.translations.turbo_on_hover()
                                                            } else {
                                                                self.translations.turbo_off_hover()
                                                            };
                                                            if ui
                                                                .add_sized([96.0, 32.0], turbo_btn)
                                                                .on_hover_text(hover_text)
                                                                .clicked()
                                                            {
                                                                mapping.turbo_enabled = !mapping.turbo_enabled;
                                                            }
                                                            ui.add_space(4.0);
                                                            // Double-tap toggle (DNF run on first press)
                                                            let double_tap_enabled = mapping.double_tap_enabled;
                                                            let double_tap_color = if double_tap_enabled {
                                                                Theme::new(self.dark_mode).good
                                                            } else { Theme::new(self.dark_mode).hint };
                                                            let double_tap_icon =
                                                                if double_tap_enabled { "2× 双击" } else { "1× 双击" };
                                                            let double_tap_btn = egui::Button::new(
                                                                egui::RichText::new(double_tap_icon)
                                                                    .color(egui::Color32::WHITE)
                                                                    .size(12.0),
                                                            )
                                                            .fill(double_tap_color)
                                                            .corner_radius(8.0)
                                                            .sense(egui::Sense::click());
                                                            let double_tap_hover = if double_tap_enabled {
                                                                self.translations.double_tap_on_hover()
                                                            } else {
                                                                self.translations.double_tap_off_hover()
                                                            };
                                                            if ui
                                                                .add_sized([96.0, 32.0], double_tap_btn)
                                                                .on_hover_text(double_tap_hover)
                                                                .clicked()
                                                            {
                                                                mapping.double_tap_enabled = !mapping.double_tap_enabled;
                                                            }
                                                            ui.add_space(4.0);
                                                            // Delete mapping
                                                            let delete_btn = egui::Button::new(
                                                                egui::RichText::new("🗑 删除")
                                                                    .color(egui::Color32::WHITE)
                                                                    .size(12.0),
                                                            )
                                                            .fill(Theme::new(self.dark_mode).btn_danger)
                                                            .corner_radius(8.0);
                                                            if ui
                                                                .add_sized([88.0, 32.0], delete_btn)
                                                                .clicked()
                                                            {
                                                                to_remove = Some(idx);
                                                            }
                                                        });

                                                        // 行 5: 备注
                                                        ui.add_space(6.0);
                                                        ui.horizontal(|ui| {
                                                            ui.add_space(26.0);
                                                            ui.label(
                                                                egui::RichText::new(t.note_label())
                                                                    .size(12.0)
                                                                    .color(Theme::new(self.dark_mode).text_weak),
                                                            );
                                                            let note_width =
                                                                ((ui.available_width() - 8.0).max(120.0)).min(520.0);
                                                            let note_edit = egui::TextEdit::singleline(
                                                                &mut mapping.note,
                                                            )
                                                            .background_color(input_bg)
                                                            .hint_text(t.note_hint())
                                                            .desired_width(note_width);
                                                            ui.add(note_edit);
                                                        });
                                                    });
                                                ui.add_space(6.0);
                                            }

                                            if let Some(idx) = to_remove {
                                                temp_config.mappings.remove(idx);
                                            }

                                            ui.add_space(10.0);
                                            ui.separator();
                                            ui.add_space(10.0);
                                            // Add new mapping
                                            ui.label(
                                                egui::RichText::new(t.add_new_mapping_title())
                                                    .size(14.0)
                                                    .strong()
                                                    .color(accent_color),
                                            );
                                            ui.add_space(8.0);

                                            let new_row_text_color = Theme::new(self.dark_mode).text;
                                            let new_input_bg = if self.dark_mode {
                                                egui::Color32::from_rgb(33, 36, 46) // #232838
                                            } else {
                                                egui::Color32::from_rgb(231, 233, 238) // #F0F1F6
                                            };
                                            let new_inner_bg = Theme::new(self.dark_mode).surface;
                                            egui::Frame::NONE
                                                .fill(new_inner_bg)
                                                .corner_radius(egui::CornerRadius::same(12))
                                                .inner_margin(egui::Margin::same(12))
                                                .show(ui, |ui| {
                                                    // 行 1: 触发键
                                                    ui.horizontal(|ui| {
                                                        ui.label(
                                                            egui::RichText::new(t.trigger_short())
                                                                .size(13.0)
                                                                .strong()
                                                                .color(accent_color),
                                                        );
                                                        ui.add_space(6.0);
                                                        let is_capturing_new_trigger = self
                                                            .key_capture_mode
                                                            == KeyCaptureMode::NewMappingTrigger;
                                                        let full_new_trigger_text =
                                                            if is_capturing_new_trigger {
                                                                t.press_any_key()
                                                            } else if self.new_mapping_trigger.is_empty() {
                                                                t.click_to_set()
                                                            } else {
                                                                self.new_mapping_trigger.as_str()
                                                            };
                                                        // Truncate text to fit in button
                                                        let new_display_text = truncate_text_safe(
                                                            full_new_trigger_text,
                                                            TEXT_TRUNCATE_LEN,
                                                        );
                                                        let new_trigger_btn = egui::Button::new(
                                                            egui::RichText::new(&new_display_text)
                                                                .size(13.0)
                                                                .color(if is_capturing_new_trigger {
                                                                    egui::Color32::WHITE
                                                                } else {
                                                                    new_row_text_color
                                                                }),
                                                        )
                                                        .fill(if is_capturing_new_trigger {
                                                            Theme::new(self.dark_mode).accent
                                                        } else {
                                                            new_input_bg
                                                        })
                                                        .corner_radius(8.0);
                                                        let mut new_trigger_response =
                                                            ui.add_sized([170.0, 34.0], new_trigger_btn);
                                                        // Show full text on hover if truncated
                                                        if !is_capturing_new_trigger
                                                            && !self.new_mapping_trigger.is_empty()
                                                            && full_new_trigger_text.len() > TEXT_TRUNCATE_LEN
                                                        {
                                                            new_trigger_response = new_trigger_response
                                                                .on_hover_text(full_new_trigger_text);
                                                        }
                                                        if new_trigger_response.clicked()
                                                            && !self.just_captured_input
                                                        {
                                                            self.key_capture_mode =
                                                                KeyCaptureMode::NewMappingTrigger;
                                                            self.capture_pressed_keys.clear();
                                                            self.capture_initial_pressed =
                                                                Self::poll_all_pressed_keys();
                                                            self.app_state.set_raw_input_capture_mode(true);
                                                            // Set flag to skip mouse capture on this frame
                                                            self.just_captured_input = true;
                                                            // Clear error when user starts to modify trigger
                                                            self.duplicate_mapping_error = None;
                                                        }
                                                    });
                                                    ui.add_space(6.0);

                                                    // 行 2: 目标键 + 添加/清空
                                                    ui.horizontal(|ui| {
                                                        ui.label(
                                                            egui::RichText::new(t.target_short())
                                                                .size(13.0)
                                                                .strong()
                                                                .color(accent_color),
                                                        );
                                                        ui.add_space(6.0);
                                                        let is_capturing_new_target = self.key_capture_mode
                                                            == KeyCaptureMode::NewMappingTarget;
                                                        let new_target_text = if is_capturing_new_target {
                                                            t.press_any_key()
                                                        } else if self.new_mapping_target.is_empty() {
                                                            t.click_to_set()
                                                        } else {
                                                            self.new_mapping_target.as_str()
                                                        };
                                                        let display_new_target_text = if is_capturing_new_target
                                                            || self.new_mapping_target.is_empty()
                                                        {
                                                            new_target_text.to_string()
                                                        } else {
                                                            truncate_text_safe(
                                                                new_target_text,
                                                                TEXT_TRUNCATE_LEN,
                                                            )
                                                        };
                                                        let new_target_btn = egui::Button::new(
                                                            egui::RichText::new(&display_new_target_text)
                                                                .size(13.0)
                                                                .color(if is_capturing_new_target {
                                                                    egui::Color32::WHITE
                                                                } else {
                                                                    new_row_text_color
                                                                }),
                                                        )
                                                        .fill(if is_capturing_new_target {
                                                            Theme::new(self.dark_mode).accent
                                                        } else {
                                                            new_input_bg
                                                        })
                                                        .corner_radius(8.0);
                                                        let mut new_target_response = ui
                                                            .add_sized([170.0, 34.0], new_target_btn);
                                                        if !is_capturing_new_target
                                                            && !self.new_mapping_target.is_empty()
                                                            && new_target_text.len() > TEXT_TRUNCATE_LEN
                                                        {
                                                            new_target_response = new_target_response
                                                                .on_hover_text(new_target_text);
                                                        }
                                                        if new_target_response.clicked()
                                                            && !self.just_captured_input
                                                        {
                                                            self.key_capture_mode =
                                                                KeyCaptureMode::NewMappingTarget;
                                                            self.capture_pressed_keys.clear();
                                                            self.capture_initial_pressed =
                                                                Self::poll_all_pressed_keys();
                                                        }
                                                        ui.add_space(8.0);

                                                        // Button 1: Add target key
                                                        let add_target_btn = egui::Button::new(
                                                            egui::RichText::new("+")
                                                                .color(egui::Color32::WHITE)
                                                                .size(16.0),
                                                        )
                                                        .fill(Theme::new(self.dark_mode).accent)
                                                        .corner_radius(8.0);
                                                        if ui
                                                            .add_sized([36.0, 34.0], add_target_btn)
                                                            .on_hover_text(t.add_target_key_hover())
                                                            .clicked()
                                                        {
                                                            self.key_capture_mode = KeyCaptureMode::NewMappingTarget;
                                                            self.capture_pressed_keys.clear();
                                                            self.capture_initial_pressed = Self::poll_all_pressed_keys();
                                                            self.just_captured_input = true;
                                                        }
                                                        // Button 1.5: Clear all target keys
                                                        let clear_btn = egui::Button::new(
                                                            egui::RichText::new("✖")
                                                                .color(egui::Color32::WHITE)
                                                                .size(14.0),
                                                        )
                                                        .fill(Theme::new(self.dark_mode).btn_danger)
                                                        .corner_radius(8.0);
                                                        if ui
                                                            .add_sized([36.0, 34.0], clear_btn)
                                                            .on_hover_text(t.clear_all_target_keys_hover())
                                                            .clicked()
                                                        {
                                                            self.new_mapping_target_keys.clear();
                                                            self.new_mapping_target.clear();
                                                        }
                                                    });

                                                    // Display new mapping target keys list
                                                    if self.new_mapping_target_keys.len() > 1 {
                                                        ui.add_space(4.0);
                                                        ui.horizontal(|ui| {
                                                            ui.label(
                                                                egui::RichText::new("🎯")
                                                                    .size(12.0)
                                                                    .color(Theme::new(self.dark_mode).text_weak),
                                                            );
                                                            // Wrap target keys within max width
                                                            ui.horizontal_wrapped(|ui| {
                                                                ui.spacing_mut().item_spacing.x = 4.0;
                                                                let mut key_to_remove: Option<String> = None;
                                                                for (i, target_key) in self.new_mapping_target_keys.iter().enumerate() {
                                                                    if i > 0 {
                                                                        ui.label(
                                                                            egui::RichText::new("·")
                                                                                .size(12.0)
                                                                                .color(egui::Color32::from_rgb(150, 150, 150)),
                                                                        );
                                                                    }
                                                                    let key_chip = egui::Button::new(
                                                                        egui::RichText::new(format!("{} ✕", target_key))
                                                                            .size(12.0)
                                                                            .color(Theme::new(self.dark_mode).bad),
                                                                    )
                                                                    .fill(Theme::new(self.dark_mode).card_alt)
                                                                    .corner_radius(8.0)
                                                                    .min_size(egui::vec2(0.0, 24.0))
                                                                    .frame(true);
                                                                    if ui.add(key_chip)
                                                                        .on_hover_text(t.format_remove_target_key_hover(target_key))
                                                                        .clicked()
                                                                    {
                                                                        key_to_remove = Some(target_key.clone());
                                                                    }
                                                                }
                                                                if let Some(key) = key_to_remove {
                                                                    self.new_mapping_target_keys.retain(|k| k != &key);
                                                                    if self.new_mapping_target_keys.len() == 1 {
                                                                        self.new_mapping_target = self.new_mapping_target_keys[0].clone();
                                                                    } else if self.new_mapping_target == key {
                                                                        self.new_mapping_target.clear();
                                                                    }
                                                                }
                                                            });
                                                        });
                                                    }

                                                    // 行 3: 参数 (鼠标移动/滚轮 vs 普通按键)
                                                    let first_target = self.new_mapping_target_keys.first().map(|s| s.as_str()).unwrap_or("");
                                                    let is_mouse_move = is_mouse_move_target(first_target);
                                                    let is_mouse_scroll = is_mouse_scroll_target(first_target);
                                                    if is_mouse_move || is_mouse_scroll {
                                                        // Show interval and speed for mouse movement/scroll
                                                        ui.add_space(6.0);
                                                        ui.horizontal(|ui| {
                                                            ui.label(
                                                                egui::RichText::new(t.interval_short())
                                                                    .size(13.0)
                                                                    .color(new_row_text_color),
                                                            );
                                                            let interval_edit = egui::TextEdit::singleline(
                                                                &mut self.new_mapping_interval,
                                                            )
                                                            .background_color(new_input_bg)
                                                            .hint_text("5")
                                                            .desired_width(50.0)
                                                            .font(egui::TextStyle::Button);
                                                            ui.add_sized([50.0, 30.0], interval_edit);
                                                            ui.add_space(8.0);
                                                            ui.label(
                                                                egui::RichText::new(t.speed_label())
                                                                    .size(13.0)
                                                                    .color(new_row_text_color),
                                                            );
                                                            let hint = if is_mouse_scroll { "120" } else { "5" };
                                                            let speed_edit = egui::TextEdit::singleline(
                                                                &mut self.new_mapping_move_speed,
                                                            )
                                                            .background_color(new_input_bg)
                                                            .hint_text(hint)
                                                            .desired_width(50.0)
                                                            .font(egui::TextStyle::Button);
                                                            ui.add_sized([50.0, 30.0], speed_edit);
                                                        });
                                                    } else {
                                                        // Show interval and duration for regular keys
                                                        ui.add_space(6.0);
                                                        ui.horizontal(|ui| {
                                                            ui.label(
                                                                egui::RichText::new(t.interval_short())
                                                                    .size(13.0)
                                                                    .color(new_row_text_color),
                                                            );
                                                            let interval_edit = egui::TextEdit::singleline(
                                                                &mut self.new_mapping_interval,
                                                            )
                                                            .background_color(new_input_bg)
                                                            .hint_text("5")
                                                            .desired_width(50.0)
                                                            .font(egui::TextStyle::Button);
                                                            ui.add_sized([50.0, 30.0], interval_edit);
                                                            ui.add_space(8.0);
                                                            ui.label(
                                                                egui::RichText::new(t.duration_short())
                                                                    .size(13.0)
                                                                    .color(new_row_text_color),
                                                            );
                                                            let duration_edit = egui::TextEdit::singleline(
                                                                &mut self.new_mapping_duration,
                                                            )
                                                            .background_color(new_input_bg)
                                                            .hint_text("5")
                                                            .desired_width(50.0)
                                                            .font(egui::TextStyle::Button);
                                                            ui.add_sized([50.0, 30.0], duration_edit);
                                                        });
                                                    }

                                                    // 行 4: 操作按钮 (方向/滚动/连发/双击)
                                                    ui.add_space(6.0);
                                                    ui.horizontal(|ui| {
                                                        // Button 2: Mouse movement direction
                                                        let move_btn = egui::Button::new(
                                                            egui::RichText::new("⌖ 方向")
                                                                .color(egui::Color32::WHITE)
                                                                .size(13.0),
                                                        )
                                                        .fill(Theme::new(self.dark_mode).accent_hover)
                                                        .corner_radius(8.0);
                                                        if ui
                                                            .add_sized([88.0, 32.0], move_btn)
                                                            .on_hover_text(t.set_mouse_direction_hover())
                                                            .clicked()
                                                        {
                                                            self.mouse_direction_dialog = Some(
                                                                crate::gui::mouse_direction_dialog::MouseDirectionDialog::new(),
                                                            );
                                                            self.mouse_direction_mapping_idx = None;
                                                        }
                                                        ui.add_space(4.0);
                                                        // Button 3: Mouse scroll direction
                                                        let scroll_btn = egui::Button::new(
                                                            egui::RichText::new("🎡 滚动")
                                                                .color(egui::Color32::WHITE)
                                                                .size(13.0),
                                                        )
                                                        .fill(Theme::new(self.dark_mode).good)
                                                        .corner_radius(8.0);
                                                        if ui
                                                            .add_sized([88.0, 32.0], scroll_btn)
                                                            .on_hover_text(t.set_mouse_scroll_direction_hover())
                                                            .clicked()
                                                        {
                                                            self.mouse_scroll_dialog = Some(
                                                                crate::gui::mouse_scroll_dialog::MouseScrollDialog::new(),
                                                            );
                                                            self.mouse_scroll_mapping_idx = None;
                                                        }
                                                        ui.add_space(4.0);
                                                        // Turbo toggle for new mapping
                                                        let new_turbo_enabled = self.new_mapping_turbo;
                                                        let new_turbo_color = if new_turbo_enabled {
                                                            Theme::new(self.dark_mode).accent
                                                        } else { Theme::new(self.dark_mode).hint };
                                                        let new_turbo_icon =
                                                            if new_turbo_enabled { "⚡ 连发" } else { "○ 连发" };
                                                        let new_turbo_btn = egui::Button::new(
                                                            egui::RichText::new(new_turbo_icon)
                                                                .color(egui::Color32::WHITE)
                                                                .size(12.0),
                                                        )
                                                        .fill(new_turbo_color)
                                                        .corner_radius(8.0)
                                                        .sense(egui::Sense::click());
                                                        let new_hover_text = if new_turbo_enabled {
                                                            self.translations.turbo_on_hover()
                                                        } else {
                                                            self.translations.turbo_off_hover()
                                                        };
                                                        if ui
                                                            .add_sized([96.0, 32.0], new_turbo_btn)
                                                            .on_hover_text(new_hover_text)
                                                            .clicked()
                                                        {
                                                            self.new_mapping_turbo = !self.new_mapping_turbo;
                                                        }
                                                        ui.add_space(4.0);
                                                        // New mapping double-tap toggle (DNF run on first press)
                                                        let new_double_tap_enabled = self.new_mapping_double_tap;
                                                        let new_double_tap_color = if new_double_tap_enabled {
                                                            Theme::new(self.dark_mode).good
                                                        } else { Theme::new(self.dark_mode).hint };
                                                        let new_double_tap_icon =
                                                            if new_double_tap_enabled { "2× 双击" } else { "1× 双击" };
                                                        let new_double_tap_btn = egui::Button::new(
                                                            egui::RichText::new(new_double_tap_icon)
                                                                .color(egui::Color32::WHITE)
                                                                .size(12.0),
                                                        )
                                                        .fill(new_double_tap_color)
                                                        .corner_radius(8.0)
                                                        .sense(egui::Sense::click());
                                                        let new_double_tap_hover = if new_double_tap_enabled {
                                                            self.translations.double_tap_on_hover()
                                                        } else {
                                                            self.translations.double_tap_off_hover()
                                                        };
                                                        if ui
                                                            .add_sized([96.0, 32.0], new_double_tap_btn)
                                                            .on_hover_text(new_double_tap_hover)
                                                            .clicked()
                                                        {
                                                            self.new_mapping_double_tap = !self.new_mapping_double_tap;
                                                        }
                                                    });

                                                    // 行 5: 备注 (触发键已设置时显示)
                                                    if !self.new_mapping_trigger.is_empty() {
                                                        ui.add_space(6.0);
                                                        ui.horizontal(|ui| {
                                                            ui.label(
                                                                egui::RichText::new(t.note_label())
                                                                    .size(12.0)
                                                                    .color(Theme::new(self.dark_mode).text_weak),
                                                            );
                                                            let note_edit = egui::TextEdit::singleline(
                                                                &mut self.new_mapping_note,
                                                            )
                                                            .background_color(new_input_bg)
                                                            .hint_text(t.note_hint())
                                                            .desired_width(220.0);
                                                            ui.add(note_edit);
                                                        });
                                                    }

                                                    // 行 6: 添加按钮
                                                    ui.add_space(8.0);
                                                    ui.horizontal(|ui| {
                                                        let add_btn = egui::Button::new(
                                                            egui::RichText::new(t.add_button_text())
                                                                .color(egui::Color32::WHITE)
                                                                .size(14.0)
                                                                .strong(),
                                                        )
                                                        .fill(Theme::new(self.dark_mode).accent)
                                                        .corner_radius(8.0);
                                                        if ui.add_sized([140.0, 36.0], add_btn).clicked()
                                                            && !self.new_mapping_trigger.is_empty()
                                                            && !self.new_mapping_target_keys.is_empty()
                                                        {
                                                            let trigger_upper =
                                                                self.new_mapping_trigger.to_uppercase();

                                                            // Check for duplicate trigger key
                                                            let is_duplicate = temp_config
                                                                .mappings
                                                                .iter()
                                                                .any(|m| m.trigger_key == trigger_upper);

                                                            if is_duplicate {
                                                                self.duplicate_mapping_error = Some(
                                                                    t.duplicate_trigger_error().to_string(),
                                                                );
                                                            } else {
                                                                // Clear any previous error
                                                                self.duplicate_mapping_error = None;

                                                                let interval = self
                                                                    .new_mapping_interval
                                                                    .parse::<u64>()
                                                                    .ok()
                                                                    .map(|v| v.max(5));
                                                                let duration = self
                                                                    .new_mapping_duration
                                                                    .parse::<u64>()
                                                                    .ok()
                                                                    .map(|v| v.max(2));
                                                                let move_speed = self
                                                                    .new_mapping_move_speed
                                                                    .parse::<i32>()
                                                                    .unwrap_or(5)
                                                                    .clamp(1, 100);

                                                                let turbo_enabled = self.new_mapping_turbo;

                                                                temp_config.mappings.push(KeyMapping {
                                                                    trigger_key: trigger_upper,
                                                                    target_keys: self.new_mapping_target_keys.iter()
                                                                        .map(|k| k.to_uppercase())
                                                                        .collect(),
                                                                    interval,
                                                                    event_duration: duration,
                                                                    turbo_enabled,
                                                                    move_speed,
                                                                    double_tap_enabled: self.new_mapping_double_tap,
                                                                    double_tap_gap_ms: 50,
                                                                    note: self.new_mapping_note.clone(),
                                                                });

                                                                // Clear input fields
                                                                self.new_mapping_trigger.clear();
                                                                self.new_mapping_target.clear();
                                                                self.new_mapping_target_keys.clear();
                                                                self.new_mapping_interval.clear();
                                                                self.new_mapping_duration.clear();
                                                                self.new_mapping_move_speed = "5".to_string();
                                                                self.new_mapping_turbo = true; // Reset to default
                                                                self.new_mapping_double_tap = false; // Reset to default
                                                                self.new_mapping_note.clear();
                                                                self.preset_rename_target.clear();
                                                                self.preset_rename_input.clear();
                                                            }
                                                        }
                                                    });

                                                    // Display duplicate trigger error if exists
                                                    if let Some(ref error_msg) = self.duplicate_mapping_error {
                                                        ui.add_space(6.0);
                                                        ui.label(
                                                            egui::RichText::new(error_msg)
                                                                .color(egui::Color32::from_rgb(255, 100, 100))
                                                                .size(13.0),
                                                        );
                                                    }
                                                });
                                        });

                                    ui.add_space(10.0);

                                    // Process Whitelist Section
                                    let card_bg = Theme::new(self.dark_mode).card;

                                    egui::Frame::NONE
                                        .fill(card_bg)
                                        .corner_radius(egui::CornerRadius::same(14))
                                        .inner_margin(egui::Margin::same(16))
                                        .show(ui, |ui| {
                                            ui.set_min_width(ui.available_width());
                                            ui.label(
                                                egui::RichText::new(t.process_whitelist_hint())
                                                    .size(16.0)
                                                    .strong()
                                                    .color(accent_color),
                                            );
                                            ui.add_space(6.0);

                                            // Process list
                                            egui::ScrollArea::vertical().max_height(80.0).show(
                                                ui,
                                                |ui| {
                                                    let mut to_remove: Option<usize> = None;
                                                    for (idx, process) in temp_config
                                                        .process_whitelist
                                                        .iter()
                                                        .enumerate()
                                                    {
                                                        ui.horizontal(|ui| {
                                                            ui.label(
                                                                egui::RichText::new(process)
                                                                    .size(13.0)
                                                                    .color(if self.dark_mode {
                                                                        egui::Color32::from_rgb(
                                                                            200, 200, 255,
                                                                        )
                                                                    } else {
                                                                        egui::Color32::from_rgb(
                                                                            60, 60, 120,
                                                                        )
                                                                    }),
                                                            );

                                                            ui.with_layout(
                                                                egui::Layout::right_to_left(
                                                                    egui::Align::Center,
                                                                ),
                                                                |ui| {
                                                                    let del_btn = egui::Button::new(
                                                                    egui::RichText::new("🗑")
                                                                        .color(egui::Color32::WHITE)
                                                                        .size(11.0),
                                                                )
                                                                .fill(egui::Color32::from_rgb(
                                                                    255, 182, 193,
                                                                )) // Soft pink
                                                                .corner_radius(8.0);

                                                                    if ui
                                                                        .add_sized(
                                                                            [28.0, 28.0],
                                                                            del_btn,
                                                                        )
                                                                        .clicked()
                                                                    {
                                                                        to_remove = Some(idx);
                                                                    }
                                                                },
                                                            );
                                                        });
                                                    }

                                                    if let Some(idx) = to_remove {
                                                        temp_config.process_whitelist.remove(idx);
                                                    }
                                                },
                                            );

                                            ui.add_space(6.0);

                                            // Add new process
                                            ui.horizontal(|ui| {
                                                let process_edit = egui::TextEdit::singleline(
                                                    &mut self.new_process_name,
                                                )
                                                .background_color(Theme::new(self.dark_mode).card_alt)
                                                .hint_text(t.process_example())
                                                .desired_width(200.0);
                                                ui.add_sized([220.0, 32.0], process_edit);

                                                let add_btn = egui::Button::new(
                                                    egui::RichText::new(t.add_button_text())
                                                        .color(egui::Color32::WHITE)
                                                        .size(12.0)
                                                        .strong(),
                                                )
                                                .fill(Theme::new(self.dark_mode).good)
                                                .corner_radius(8.0);

                                                if ui.add_sized([84.0, 32.0], add_btn).clicked() {
                                                    let process_name = self.new_process_name.trim();
                                                    if !process_name.is_empty() {
                                                        // Check for duplicate process
                                                        if temp_config
                                                            .process_whitelist
                                                            .contains(&process_name.to_string())
                                                        {
                                                            self.duplicate_process_error = Some(
                                                                t.duplicate_process_error()
                                                                    .to_string(),
                                                            );
                                                        } else {
                                                            // Clear any previous error
                                                            self.duplicate_process_error = None;
                                                            temp_config
                                                                .process_whitelist
                                                                .push(process_name.to_string());
                                                            self.new_process_name.clear();
                                                        }
                                                    }
                                                }

                                                ui.add_space(8.0);

                                                // Browse button for selecting process
                                                let browse_btn = egui::Button::new(
                                                    egui::RichText::new(t.browse_button())
                                                        .color(egui::Color32::WHITE)
                                                        .size(12.0)
                                                        .strong(),
                                                )
                                                .fill(Theme::new(self.dark_mode).accent_text)
                                                .corner_radius(8.0);

                                                if ui.add_sized([96.0, 32.0], browse_btn).clicked()
                                                {
                                                    // Open file dialog to select executable
                                                    if let Some(path) = rfd::FileDialog::new()
                                                        .add_filter("Executable", &["exe"])
                                                        .set_title("Select Process")
                                                        .pick_file()
                                                        && let Some(filename) = path.file_name()
                                                    {
                                                        let process_name =
                                                            filename.to_string_lossy().to_string();
                                                        // Check for duplicate process
                                                        if temp_config
                                                            .process_whitelist
                                                            .contains(&process_name)
                                                        {
                                                            self.duplicate_process_error = Some(
                                                                t.duplicate_process_error()
                                                                    .to_string(),
                                                            );
                                                        } else {
                                                            // Clear any previous error
                                                            self.duplicate_process_error = None;
                                                            temp_config
                                                                .process_whitelist
                                                                .push(process_name);
                                                        }
                                                    }
                                                }
                                            });

                                            // Display duplicate process error if exists
                                            if let Some(ref error_msg) =
                                                self.duplicate_process_error
                                            {
                                                ui.add_space(8.0);
                                                ui.label(
                                                    egui::RichText::new(error_msg)
                                                        .color(egui::Color32::from_rgb(
                                                            255, 100, 100,
                                                        ))
                                                        .size(13.0),
                                                );
                                            }
                                        });

                                }); // End of ScrollArea
                        }); // End of Frame
                    ui.separator();

                    // Action buttons - centered (outside ScrollArea, fixed at bottom)
                    ui.vertical_centered(|ui| {
                        ui.horizontal(|ui| {
                            // Calculate total width of buttons and spacing
                            let button_width = 240.0;
                            let spacing = 15.0;
                            let total_buttons_width = button_width * 2.0 + spacing;
                            let available_width = ui.available_width();

                            // Add left padding to center the buttons
                            if available_width > total_buttons_width {
                                ui.add_space((available_width - total_buttons_width) / 2.0);
                            }

                            let save_btn = egui::Button::new(
                                egui::RichText::new(t.save())
                                    .size(14.0)
                                    .color(egui::Color32::WHITE)
                                    .strong(),
                            )
                            .fill(Theme::new(self.dark_mode).btn_primary)
                            .corner_radius(8.0);

                            if ui.add_sized([button_width, 32.0], save_btn).clicked() {
                                should_save = true;
                            }

                            ui.add_space(spacing);

                            let cancel_btn = egui::Button::new(
                                egui::RichText::new(t.cancel())
                                    .size(14.0)
                                    .color(Theme::new(self.dark_mode).btn_secondary_text),
                            )
                            .fill(Theme::new(self.dark_mode).btn_secondary)
                            .corner_radius(8.0);

                            if ui.add_sized([button_width, 32.0], cancel_btn).clicked() {
                                should_cancel = true;
                            }
                        });
                    });

                    ui.add_space(2.0);

                    // Hint
                    ui.vertical_centered(|ui| {
                        ui.label(
                            egui::RichText::new(t.changes_take_effect_hint())
                                .size(12.0)
                                .color(Theme::new(self.dark_mode).accent_text)
                                .italics(),
                        );
                    });
                }); // End of ui.push_id
            }); // End of egui::Window

        // Handle save/cancel outside the window closure
        if should_save {
            if let Some(mut temp_config) = self.temp_config.take() {
                // 弹窗非模态: 打开期间主窗口可能改过映射/预设/震动参数 (改即落盘)。
                // Save 只提交弹窗自己拥有的字段, 其余保持运行时最新值 ——
                // 旧实现整份回灌 temp_config, 会把弹窗打开后发生的修改静默回滚。
                temp_config.mappings = self.config.mappings.clone();
                temp_config.presets = self.config.presets.clone();
                temp_config.current_preset = self.config.current_preset.clone();
                temp_config.vibration = self.config.vibration.clone();
                temp_config.vibration_presets = self.config.vibration_presets.clone();
                temp_config.hid_baselines = self.config.hid_baselines.clone();
                temp_config.device_api_preferences = self.config.device_api_preferences.clone();
                temp_config.guide_seen = self.config.guide_seen;
                temp_config.vib_edition_asked = self.config.vib_edition_asked;
                temp_config.window_rect_normal = self.config.window_rect_normal;
                temp_config.window_rect_minimal = self.config.window_rect_minimal;
                temp_config.minimal_mode = self.config.minimal_mode;
                temp_config.minimal_vib_preset_job = self.config.minimal_vib_preset_job;

                // Update preset mappings if a preset is currently active
                if !temp_config.current_preset.is_empty() {
                    if let Some(preset) = temp_config.presets.iter_mut()
                        .find(|p| p.name == temp_config.current_preset)
                    {
                        preset.mappings = temp_config.mappings.clone();
                    }
                }

                // ★客户端版本切换到 S1: 预设列表出现「ACT1 特供」(玩家自选)
                // Check if always_on_top changed
                let always_on_top_changed = temp_config.always_on_top != self.config.always_on_top;
                // Check if dark_mode changed
                let dark_mode_changed = temp_config.dark_mode != self.config.dark_mode;
                // Check if language changed
                let language_changed = temp_config.language != self.config.language;

                // Save to file
                if temp_config.save_to_file("Config.toml").is_ok() {
                    // Reload configuration into AppState (takes effect immediately)
                    let _ = self.app_state.reload_config(temp_config.clone());

                    // Update GUI's config
                    self.config = temp_config.clone();

                    // DFO 震动关闭时震动页不可达: 当前页落在其上则回手柄映射
                    if !self.config.dfo_player
                        && matches!(
                            self.active_page,
                            crate::gui::types::Page::Vibration | crate::gui::types::Page::JobPresets
                        )
                    {
                        self.active_page = crate::gui::types::Page::Gamepad;
                    }

                    // Re-parse switch key after configuration update
                    self.parsed_switch_key = Self::parse_switch_key(&self.config.switch_key);

                    // Apply theme change immediately
                    if dark_mode_changed {
                        self.dark_mode = self.config.dark_mode;
                    }

                    if language_changed {
                        self.update_translations(self.config.language);
                        crate::gui::fonts::load_fonts(ctx, self.config.language);
                    }

                    // Apply always_on_top change immediately
                    if always_on_top_changed {
                        if self.config.always_on_top {
                            ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(
                                egui::WindowLevel::AlwaysOnTop,
                            ));
                        } else {
                            ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(
                                egui::WindowLevel::Normal,
                            ));
                        }
                    }
                }
            }
            self.show_settings_dialog = false;
            self.temp_config = None;
            self.key_capture_mode = KeyCaptureMode::None;
            self.duplicate_mapping_error = None;
            self.duplicate_process_error = None;
            self.preset_rename_target.clear();
            self.preset_rename_input.clear();
            self.app_state.set_raw_input_capture_mode(false);

            // Restore previous paused state after exiting settings
            if let Some(was_paused) = self.was_paused_before_settings.take()
                && !was_paused
            {
                // Resume key repeat without notification (silent resume)
                self.app_state.set_paused(false);
            }
        }

        if should_cancel {
            self.show_settings_dialog = false;
            self.temp_config = None;
            self.key_capture_mode = KeyCaptureMode::None;
            self.duplicate_mapping_error = None;
            self.duplicate_process_error = None;
            self.app_state.set_raw_input_capture_mode(false);
            // Clear input fields
            self.new_mapping_trigger.clear();
            self.new_mapping_target.clear();
            self.new_mapping_target_keys.clear();
            self.new_mapping_interval.clear();
            self.new_mapping_duration.clear();
            self.new_mapping_note.clear();
            self.preset_rename_target.clear();
            self.preset_rename_input.clear();

            // Restore previous paused state after exiting settings
            if let Some(was_paused) = self.was_paused_before_settings.take()
                && !was_paused
            {
                // Resume key repeat without notification (silent resume)
                self.app_state.set_paused(false);
            }
        }
    }

    /// 处理手柄可视化页的快速捕获（在设置弹窗未打开时由主窗口调用）。
    pub(super) fn handle_quick_gamepad_capture(&mut self, ctx: &egui::Context) {
        // Esc 取消捕获 (提示文案承诺的行为)
        if matches!(
            self.key_capture_mode,
            KeyCaptureMode::QuickGamepadTrigger(_) | KeyCaptureMode::QuickGamepadTarget(_)
        ) && ctx.input(|i| i.key_pressed(egui::Key::Escape))
        {
            self.key_capture_mode = KeyCaptureMode::None;
            self.capture_pressed_keys.clear();
            self.app_state.set_raw_input_capture_mode(false);
            self.quick_gamepad_pending = None;
            return;
        }

        let mode = match self.key_capture_mode {
            KeyCaptureMode::QuickGamepadTrigger(slot) => Some((slot, true)),
            KeyCaptureMode::QuickGamepadTarget(slot) => Some((slot, false)),
            _ => return,
        };

        let Some((slot_id, is_trigger)) = mode else {
            self.key_capture_mode = KeyCaptureMode::None;
            return;
        };
        let Some(_slot) = crate::gui::gamepad_mapping::get_slot(slot_id) else {
            self.key_capture_mode = KeyCaptureMode::None;
            return;
        };

        let mut captured_input: Option<String> = None;

        // 键盘捕获（触发键与目标键都允许，触发键也兼容键盘直连）
        let current_pressed = Self::poll_all_pressed_keys();
        current_pressed
            .iter()
            .filter(|&vk| !self.capture_initial_pressed.contains(vk))
            .for_each(|&vk| {
                self.capture_pressed_keys.insert(vk);
            });

        let any_released = self
            .capture_pressed_keys
            .iter()
            .any(|vk| !current_pressed.contains(vk));

        if any_released {
            captured_input = Self::format_captured_keys(&self.capture_pressed_keys);
        }

        // 鼠标捕获只用于目标键
        if captured_input.is_none() && !is_trigger && !self.just_captured_input {
            ctx.input(|i| {
                if i.pointer.button_clicked(egui::PointerButton::Primary) {
                    captured_input = Some("LBUTTON".to_string());
                } else if i.pointer.button_clicked(egui::PointerButton::Secondary) {
                    captured_input = Some("RBUTTON".to_string());
                } else if i.pointer.button_clicked(egui::PointerButton::Middle) {
                    captured_input = Some("MBUTTON".to_string());
                } else if i.pointer.button_clicked(egui::PointerButton::Extra1) {
                    captured_input = Some("XBUTTON1".to_string());
                } else if i.pointer.button_clicked(egui::PointerButton::Extra2) {
                    captured_input = Some("XBUTTON2".to_string());
                }
            });
        }

        // 手柄原始输入捕获只用于触发键
        if captured_input.is_none() && is_trigger {
            if let Some(device) = self.app_state.try_recv_raw_input_capture() {
                captured_input = Some(device.to_string());
            }
        }

        if let Some(input_name) = captured_input {
            // 捕获完成 → 进入待确认状态 (不立即写映射, 防误操作);
            // 用户在槽位面板点「确认应用」才落盘生效, 「取消」直接丢弃
            self.quick_gamepad_pending = Some((slot_id, is_trigger, input_name));
            self.key_capture_mode = KeyCaptureMode::None;
            self.capture_pressed_keys.clear();
            self.app_state.set_raw_input_capture_mode(false);
            self.just_captured_input = false;
        } else if self.just_captured_input {
            self.just_captured_input = false;
        }
    }

    /// Polls all currently pressed keys using Windows API
    /// Returns a set of VK codes
    #[inline]
    pub(super) fn poll_all_pressed_keys() -> std::collections::HashSet<u32> {
        let mut pressed_keys = std::collections::HashSet::new();

        unsafe {
            // Modifiers
            let modifiers = [
                0xA0, 0xA1, // LSHIFT, RSHIFT
                0xA2, 0xA3, // LCTRL, RCTRL
                0xA4, 0xA5, // LALT, RALT
                0x5B, 0x5C, // LWIN, RWIN
            ];
            for &vk in &modifiers {
                if GetAsyncKeyState(vk as i32) < 0 {
                    pressed_keys.insert(vk);
                }
            }

            // A-Z (0x41-0x5A)
            for vk in 0x41..=0x5A {
                if GetAsyncKeyState(vk) < 0 {
                    pressed_keys.insert(vk as u32);
                }
            }

            // 0-9 (0x30-0x39)
            for vk in 0x30..=0x39 {
                if GetAsyncKeyState(vk) < 0 {
                    pressed_keys.insert(vk as u32);
                }
            }

            // Numpad 0-9 (0x60-0x69)
            for vk in 0x60..=0x69 {
                if GetAsyncKeyState(vk) < 0 {
                    pressed_keys.insert(vk as u32);
                }
            }

            // F1-F24 (0x70-0x87)
            for vk in 0x70..=0x87 {
                if GetAsyncKeyState(vk) < 0 {
                    pressed_keys.insert(vk as u32);
                }
            }

            // Navigation and editing keys
            let navigation = [
                0x20, // SPACE
                0x0D, // ENTER
                0x09, // TAB
                0x1B, // ESC
                0x08, // BACKSPACE
                0x2E, // DELETE
                0x2D, // INSERT
                0x24, // HOME
                0x23, // END
                0x21, // PAGEUP
                0x22, // PAGEDOWN
                0x26, // UP
                0x28, // DOWN
                0x25, // LEFT
                0x27, // RIGHT
            ];
            for &vk in &navigation {
                if GetAsyncKeyState(vk as i32) < 0 {
                    pressed_keys.insert(vk);
                }
            }

            // Lock and special keys
            let lock_keys = [
                0x14, // CAPSLOCK
                0x90, // NUMLOCK
                0x91, // SCROLL LOCK
                0x13, // PAUSE
                0x2C, // PRINT SCREEN
            ];
            for &vk in &lock_keys {
                if GetAsyncKeyState(vk as i32) < 0 {
                    pressed_keys.insert(vk);
                }
            }

            // Numpad operators
            let numpad_ops = [
                0x6A, // MULTIPLY
                0x6B, // ADD
                0x6C, // SEPARATOR
                0x6D, // SUBTRACT
                0x6E, // DECIMAL
                0x6F, // DIVIDE
            ];
            for &vk in &numpad_ops {
                if GetAsyncKeyState(vk as i32) < 0 {
                    pressed_keys.insert(vk);
                }
            }

            // OEM keys (punctuation and symbols)
            let oem_keys = [
                0xBA, // OEM_1 (;:)
                0xBB, // OEM_PLUS (=+)
                0xBC, // OEM_COMMA (,<)
                0xBD, // OEM_MINUS (-_)
                0xBE, // OEM_PERIOD (.>)
                0xBF, // OEM_2 (/?)
                0xC0, // OEM_3 (`~)
                0xDB, // OEM_4 ([{)
                0xDC, // OEM_5 (\|)
                0xDD, // OEM_6 (]})
                0xDE, // OEM_7 ('")
                0xDF, // OEM_8
                0xE2, // OEM_102 (<>)
            ];
            for &vk in &oem_keys {
                if GetAsyncKeyState(vk as i32) < 0 {
                    pressed_keys.insert(vk);
                }
            }

            // Mouse buttons
            let mouse_buttons = [
                0x01, // LBUTTON
                0x02, // RBUTTON
                0x04, // MBUTTON
                0x05, // XBUTTON1
                0x06, // XBUTTON2
            ];
            for &vk in &mouse_buttons {
                if GetAsyncKeyState(vk as i32) < 0 {
                    pressed_keys.insert(vk);
                }
            }
        }

        pressed_keys
    }

    /// Formats a set of VK codes into a key combination string
    #[inline]
    pub(super) fn format_captured_keys(vk_codes: &std::collections::HashSet<u32>) -> Option<String> {
        if vk_codes.is_empty() {
            return None;
        }

        // Separate modifiers and main keys
        let mut modifiers: SmallVec<[u32; 8]> = SmallVec::with_capacity(8);
        let mut main_keys: SmallVec<[u32; 8]> = SmallVec::with_capacity(8);

        for &vk in vk_codes {
            match vk {
                0xA0 | 0xA1 | 0xA2 | 0xA3 | 0xA4 | 0xA5 | 0x5B | 0x5C => {
                    modifiers.push(vk);
                }
                _ => {
                    main_keys.push(vk);
                }
            }
        }

        // Build the key combination string
        let mut parts: SmallVec<[String; 8]> = SmallVec::with_capacity(8);

        // Add modifiers in consistent order
        for &vk in &modifiers {
            let name = match vk {
                0xA2 => "LCTRL",
                0xA3 => "RCTRL",
                0xA4 => "LALT",
                0xA5 => "RALT",
                0xA0 => "LSHIFT",
                0xA1 => "RSHIFT",
                0x5B => "LWIN",
                0x5C => "RWIN",
                _ => continue,
            };
            parts.push(name.to_string());
        }

        // Add main key (only use the first one if multiple)
        if let Some(&vk) = main_keys.first() {
            let name = Self::vk_to_string(vk)?;
            parts.push(name);
        }

        if !parts.is_empty() {
            Some(parts.join("+"))
        } else {
            None
        }
    }

    /// Converts VK code to key name string
    #[inline]
    fn vk_to_string(vk: u32) -> Option<String> {
        match vk {
            // A-Z
            0x41..=0x5A => Some(char::from_u32(vk).unwrap().to_string()),
            // 0-9
            0x30..=0x39 => Some(char::from_u32(vk).unwrap().to_string()),
            // Numpad 0-9
            0x60..=0x69 => Some(format!("NUMPAD{}", vk - 0x60)),
            // F1-F24
            0x70..=0x87 => Some(format!("F{}", vk - 0x70 + 1)),
            // Navigation keys
            0x20 => Some("SPACE".to_string()),
            0x0D => Some("RETURN".to_string()),
            0x09 => Some("TAB".to_string()),
            0x1B => Some("ESCAPE".to_string()),
            0x08 => Some("BACK".to_string()),
            0x2E => Some("DELETE".to_string()),
            0x2D => Some("INSERT".to_string()),
            0x24 => Some("HOME".to_string()),
            0x23 => Some("END".to_string()),
            0x21 => Some("PAGEUP".to_string()),
            0x22 => Some("PAGEDOWN".to_string()),
            0x26 => Some("UP".to_string()),
            0x28 => Some("DOWN".to_string()),
            0x25 => Some("LEFT".to_string()),
            0x27 => Some("RIGHT".to_string()),
            // Lock and special keys
            0x14 => Some("CAPITAL".to_string()),
            0x90 => Some("NUMLOCK".to_string()),
            0x91 => Some("SCROLL".to_string()),
            0x13 => Some("PAUSE".to_string()),
            0x2C => Some("SNAPSHOT".to_string()),
            // Numpad operators
            0x6A => Some("MULTIPLY".to_string()),
            0x6B => Some("ADD".to_string()),
            0x6C => Some("SEPARATOR".to_string()),
            0x6D => Some("SUBTRACT".to_string()),
            0x6E => Some("DECIMAL".to_string()),
            0x6F => Some("DIVIDE".to_string()),
            // OEM keys
            0xBA => Some("OEM_1".to_string()),
            0xBB => Some("OEM_PLUS".to_string()),
            0xBC => Some("OEM_COMMA".to_string()),
            0xBD => Some("OEM_MINUS".to_string()),
            0xBE => Some("OEM_PERIOD".to_string()),
            0xBF => Some("OEM_2".to_string()),
            0xC0 => Some("OEM_3".to_string()),
            0xDB => Some("OEM_4".to_string()),
            0xDC => Some("OEM_5".to_string()),
            0xDD => Some("OEM_6".to_string()),
            0xDE => Some("OEM_7".to_string()),
            0xDF => Some("OEM_8".to_string()),
            0xE2 => Some("OEM_102".to_string()),
            // Modifiers
            0xA2 => Some("LCTRL".to_string()),
            0xA3 => Some("RCTRL".to_string()),
            0xA4 => Some("LALT".to_string()),
            0xA5 => Some("RALT".to_string()),
            0xA0 => Some("LSHIFT".to_string()),
            0xA1 => Some("RSHIFT".to_string()),
            0x5B => Some("LWIN".to_string()),
            0x5C => Some("RWIN".to_string()),
            // Mouse buttons
            0x01 => Some("LBUTTON".to_string()),
            0x02 => Some("RBUTTON".to_string()),
            0x04 => Some("MBUTTON".to_string()),
            0x05 => Some("XBUTTON1".to_string()),
            0x06 => Some("XBUTTON2".to_string()),
            _ => None,
        }
    }
}
