//! Main window implementation and rendering logic.

use crate::gui::SorahkGui;
use crate::gui::about_dialog::render_about_dialog;
use crate::gui::theme::{self, Theme, truncate_chars};
use crate::gui::widgets;
use crate::gui::types::{KeyCaptureMode, Page};
use crate::state::NotificationEvent;
use eframe::egui;

/// Cached frame state to avoid repeated atomic operations.
struct FrameState {
    is_paused: bool,
    worker_count: usize,
    should_exit: bool,
}

impl eframe::App for SorahkGui {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Cache frame state at the beginning to avoid repeated atomic operations
        let frame_state = FrameState {
            is_paused: self.app_state.is_paused(),
            worker_count: self.app_state.get_actual_worker_count(),
            should_exit: self.app_state.should_exit(),
        };

        // Check if exit was requested at the very beginning
        if frame_state.should_exit {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }

        // Check for HID device activation requests
        if self.hid_activation_dialog.is_none() {
            let requests = self.app_state.poll_hid_activation_requests();
            if let Some(request) = requests.first() {
                self.hid_activation_dialog =
                    Some(crate::gui::hid_activation_dialog::HidActivationDialog::new(
                        request.device_name.clone(),
                        request.device_handle,
                        request.vid,
                        request.pid,
                        request.usage_page,
                        request.usage,
                    ));
                // Record creation time for 100ms debounce
                self.hid_activation_creation_time = Some(std::time::Instant::now());
            }
        }

        // Render HID activation dialog if present
        if let Some(dialog) = &mut self.hid_activation_dialog {
            let is_debouncing = if let Some(creation_time) = self.hid_activation_creation_time {
                creation_time.elapsed().as_millis() < 200
            } else {
                false
            };

            if is_debouncing {
                while self
                    .app_state
                    .try_recv_hid_activation_data(dialog.device_handle())
                    .is_some()
                {
                    // Discard all data during debounce period
                }
            } else {
                while let Some(hid_data) = self
                    .app_state
                    .try_recv_hid_activation_data(dialog.device_handle())
                {
                    dialog.handle_hid_data(&hid_data);
                }
            }

            let should_close = dialog.render(ctx, self.dark_mode, &self.translations);

            if should_close {
                // Save baseline to config if successful
                if let Some(baseline) = dialog.get_baseline() {
                    crate::rawinput::activate_hid_device(dialog.device_handle(), baseline.clone());

                    // Get device info for stable identifier (VID:PID:Serial)
                    if let Some((vid, pid, serial)) =
                        crate::rawinput::get_device_info_for_handle(dialog.device_handle())
                    {
                        // Create device ID string using format: "VID:PID" or "VID:PID:Serial"
                        let device_id = if let Some(ref serial) = serial {
                            format!("{:04X}:{:04X}:{}", vid, pid, serial)
                        } else {
                            format!("{:04X}:{:04X}", vid, pid)
                        };

                        // Check if device already exists in config (avoid duplicates)
                        if !self
                            .config
                            .hid_baselines
                            .iter()
                            .any(|b| b.device_id == device_id)
                        {
                            // Add to config for persistence
                            self.config
                                .hid_baselines
                                .push(crate::config::HidDeviceBaseline {
                                    device_id,
                                    baseline_data: baseline,
                                });

                            // Save config
                            let _ = self.config.save_to_file("Config.toml");
                        }
                    }
                }

                // Clear activating device handle
                self.app_state.clear_activating_device();
                self.hid_activation_dialog = None;
                self.hid_activation_creation_time = None;
            }
        }

        // Apply cached style (visuals + type scale + spacing) based on theme
        let style = if self.dark_mode {
            &self.cached_dark_style
        } else {
            &self.cached_light_style
        };
        ctx.set_style(style.clone());

        ctx.request_repaint_after(std::time::Duration::from_millis(100));

        // 上游 eframe/glow 偶发问题: 窗口在遮挡状态下启动/停留后, 重新可见时
        // DWM 无合成内容 (半透明/白帧), 需点击一次才恢复。
        // 检测后台→前台聚焦跳变, 强制重新展示窗口重建合成内容。
        let focused = ctx.input(|i| i.viewport().focused.unwrap_or(false));
        if focused && !self.was_focused {
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
            ctx.request_repaint();
        }
        self.was_focused = focused;


        // Handle window visibility requests
        if self.app_state.check_and_clear_show_window_request() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        }

        if self.app_state.check_and_clear_show_about_request() {
            self.show_about_dialog = true;
        }

        // Handle close dialog
        self.handle_close_dialog(ctx, &frame_state);

        // Show dialogs
        if self.show_settings_dialog {
            self.render_settings_dialog(ctx);
        }

        if self.show_about_dialog {
            render_about_dialog(
                ctx,
                self.dark_mode,
                &mut self.show_about_dialog,
                &self.translations,
            );
        }

        // Show device manager dialog
        if self.show_device_manager {
            if self.device_manager_dialog.is_none() {
                let mut dialog = crate::gui::device_manager_dialog::DeviceManagerDialog::new();
                dialog.load_preferences(&self.config.device_api_preferences);
                dialog.refresh_devices();
                self.device_manager_dialog = Some(dialog);
            }

            if let Some(dialog) = &mut self.device_manager_dialog {
                let mut activated_devices =
                    std::collections::HashSet::with_capacity(self.config.hid_baselines.len());

                for baseline in &self.config.hid_baselines {
                    // Parse VID:PID from device_id (format: "VID:PID" or "VID:PID:Serial")
                    if let Some(colon_pos) = baseline.device_id.find(':') {
                        let vid_str = &baseline.device_id[..colon_pos];
                        let remaining = &baseline.device_id[colon_pos + 1..];

                        if let Some(second_colon) = remaining.find(':') {
                            let pid_str = &remaining[..second_colon];
                            if let (Ok(vid), Ok(pid)) = (
                                u16::from_str_radix(vid_str, 16),
                                u16::from_str_radix(pid_str, 16),
                            ) {
                                activated_devices.insert((vid, pid));
                            }
                        } else if let (Ok(vid), Ok(pid)) = (
                            u16::from_str_radix(vid_str, 16),
                            u16::from_str_radix(remaining, 16),
                        ) {
                            activated_devices.insert((vid, pid));
                        }
                    }
                }

                let should_close =
                    dialog.render(ctx, self.dark_mode, &self.translations, &activated_devices);

                let changed_prefs = dialog.take_changed_preferences();
                if !changed_prefs.is_empty() {
                    for (key, pref) in changed_prefs {
                        self.config.device_api_preferences.insert(key.clone(), pref);
                        if let Some((vid, pid)) = key.split_once(':')
                            && let (Ok(vid_u16), Ok(pid_u16)) =
                                (u16::from_str_radix(vid, 16), u16::from_str_radix(pid, 16))
                        {
                            crate::input_manager::set_device_api_preference(
                                (vid_u16, pid_u16),
                                pref,
                            );
                            crate::input_manager::release_device_ownership((vid_u16, pid_u16));
                        }
                    }
                    let _ = self.config.save_to_file("Config.toml");
                }

                let devices_to_reactivate = dialog.take_devices_to_reactivate();
                if !devices_to_reactivate.is_empty() {
                    for (vid, pid) in devices_to_reactivate {
                        let vid_pid_prefix = format!("{:04X}:{:04X}", vid, pid);
                        self.config
                            .hid_baselines
                            .retain(|baseline| !baseline.device_id.starts_with(&vid_pid_prefix));
                        self.app_state.clear_device_baseline(vid, pid);
                    }
                    let _ = self.config.save_to_file("Config.toml");
                    dialog.refresh_devices();
                }

                if should_close {
                    self.show_device_manager = false;
                    self.device_manager_dialog = None;
                }
            }
        }

        // Handle mouse direction dialog
        if let Some(dialog) = &mut self.mouse_direction_dialog {
            let should_close = dialog.render(ctx, self.dark_mode, &self.translations);

            if should_close {
                if let Some(selected) = dialog.get_selected_direction() {
                    // Apply the selected direction
                    if let Some(idx) = self.mouse_direction_mapping_idx {
                        // Editing existing mapping - add to existing target keys
                        if let Some(temp_config) = &mut self.temp_config
                            && let Some(mapping) = temp_config.mappings.get_mut(idx)
                        {
                            mapping.add_target_key(selected);
                        }
                    } else {
                        // New mapping - add to target keys list
                        self.new_mapping_target = selected.clone();
                        if !self.new_mapping_target_keys.contains(&selected) {
                            self.new_mapping_target_keys.push(selected);
                        }
                    }
                }
                self.mouse_direction_dialog = None;
                self.mouse_direction_mapping_idx = None;
            }
        }

        // Handle mouse scroll dialog
        if let Some(dialog) = &mut self.mouse_scroll_dialog {
            let should_close = dialog.render(ctx, self.dark_mode, &self.translations);

            if should_close {
                if let Some(selected) = dialog.get_selected_direction() {
                    if let Some(idx) = self.mouse_scroll_mapping_idx {
                        if let Some(temp_config) = &mut self.temp_config
                            && let Some(mapping) = temp_config.mappings.get_mut(idx)
                        {
                            mapping.add_target_key(selected);
                        }
                    } else {
                        self.new_mapping_target = selected.clone();
                        if !self.new_mapping_target_keys.contains(&selected) {
                            self.new_mapping_target_keys.push(selected);
                        }
                    }
                }
                self.mouse_scroll_dialog = None;
                self.mouse_scroll_mapping_idx = None;
            }
        }

        // Handle switch key
        self.handle_keyboard_input(ctx);

        // 手柄可视化页快速捕获 + 连发页内联编辑捕获（均在设置弹窗关闭时生效）
        if !self.show_settings_dialog {
            self.handle_quick_gamepad_capture(ctx);
            self.handle_turbo_edit_capture(ctx);
        }

        // Render main content
        self.render_shell(ctx, &frame_state);

        // Check exit
        if frame_state.should_exit {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.app_state.exit();
    }
}

impl SorahkGui {
    /// Handles close dialog display and interaction logic.
    fn handle_close_dialog(&mut self, ctx: &egui::Context, frame_state: &FrameState) {
        if ctx.input(|i| i.viewport().close_requested()) {
            if frame_state.should_exit {
                // Allow close
            } else if self.minimize_on_close {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);

                if !self.show_close_dialog {
                    self.show_close_dialog = true;
                    self.dialog_highlight_until = None;
                    // Restore window when showing close dialog
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                } else {
                    self.dialog_highlight_until =
                        Some(std::time::Instant::now() + std::time::Duration::from_millis(500));
                    ctx.request_repaint();
                }
            }
        }

        if self.show_close_dialog {
            self.render_close_dialog(ctx);
        }
    }

    /// Renders the close confirmation dialog.
    fn render_close_dialog(&mut self, ctx: &egui::Context) {
        let t = &self.translations;
        let th = self.theme();
        let should_highlight = self
            .dialog_highlight_until
            .map(|until| std::time::Instant::now() < until)
            .unwrap_or(false);

        if should_highlight {
            ctx.request_repaint();
        }

        let window = egui::Window::new("")
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .fixed_size([400.0, 300.0])
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .frame(
                egui::Frame::window(&ctx.style())
                    .fill(th.card)
                    .corner_radius(egui::CornerRadius::same(20))
                    .stroke(if should_highlight {
                        egui::Stroke::new(3.0, th.warn)
                    } else {
                        egui::Stroke::NONE
                    })
                    .shadow(egui::epaint::Shadow {
                        offset: [0, 4],
                        blur: 10,
                        spread: 2,
                        color: egui::Color32::from_rgba_premultiplied(0, 0, 0, 40),
                    }),
            );

        window.show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(25.0);

                ui.label(
                    egui::RichText::new(t.close_window_title())
                        .size(22.0)
                        .strong()
                        .color(th.title),
                );

                ui.add_space(8.0);
                ui.label(
                    egui::RichText::new(t.close_subtitle())
                        .size(13.0)
                        .italics()
                        .color(th.hint),
                );

                ui.add_space(30.0);

                let button_width = 320.0;
                let button_height = 32.0;
                let tray_enabled = self.config.show_tray_icon;

                if tray_enabled {
                    let minimize_btn = egui::Button::new(
                        egui::RichText::new(t.minimize_to_tray_button())
                            .size(14.0)
                            .color(egui::Color32::WHITE)
                            .strong(),
                    )
                    .fill(th.btn_primary)
                    .corner_radius(12.0);

                    if ui
                        .add_sized([button_width, button_height], minimize_btn)
                        .clicked()
                    {
                        self.show_close_dialog = false;
                        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                    }

                    ui.add_space(12.0);
                }

                let exit_btn = egui::Button::new(
                    egui::RichText::new(t.exit_program_button())
                        .size(14.0)
                        .color(egui::Color32::WHITE)
                        .strong(),
                )
                .fill(th.btn_danger)
                .corner_radius(12.0);

                if ui
                    .add_sized([button_width, button_height], exit_btn)
                    .clicked()
                {
                    self.show_close_dialog = false;
                    self.app_state.exit();
                }

                ui.add_space(12.0);

                let cancel_btn = egui::Button::new(
                    egui::RichText::new(t.cancel_close_button())
                        .size(13.0)
                        .color(th.btn_secondary_text),
                )
                .fill(th.btn_secondary)
                .corner_radius(10.0);

                if ui
                    .add_sized([button_width, button_height], cancel_btn)
                    .clicked()
                {
                    self.show_close_dialog = false;
                }

                ui.add_space(15.0);
            });
        });
    }

    /// Handles switch key input detection using GetAsyncKeyState.
    #[inline]
    fn handle_keyboard_input(&mut self, _ctx: &egui::Context) {
        use crate::gui::ParsedSwitchKey;
        use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;

        unsafe {
            match &self.parsed_switch_key {
                ParsedSwitchKey::None => (),

                ParsedSwitchKey::Single(vk) => {
                    let is_pressed = GetAsyncKeyState(*vk as i32) < 0;
                    let vk_bit = Self::vk_to_bit(*vk);
                    let was_pressed = (self.last_vk_state & vk_bit) != 0;

                    if is_pressed && !was_pressed {
                        self.last_vk_state |= vk_bit;
                        self.app_state.handle_switch_key_toggle();
                    } else if !is_pressed {
                        self.last_vk_state &= !vk_bit;
                    }
                }

                ParsedSwitchKey::Combo { modifiers, keys } => {
                    let has_ctrl = (modifiers & 0b001) != 0;
                    let has_shift = (modifiers & 0b010) != 0;
                    let has_alt = (modifiers & 0b100) != 0;

                    let ctrl_pressed =
                        !has_ctrl || GetAsyncKeyState(0xA2) < 0 || GetAsyncKeyState(0xA3) < 0;
                    let shift_pressed =
                        !has_shift || GetAsyncKeyState(0xA0) < 0 || GetAsyncKeyState(0xA1) < 0;
                    let alt_pressed =
                        !has_alt || GetAsyncKeyState(0xA4) < 0 || GetAsyncKeyState(0xA5) < 0;

                    if crate::util::unlikely(!ctrl_pressed || !shift_pressed || !alt_pressed) {
                        return;
                    }

                    for &vk in keys {
                        let is_pressed = GetAsyncKeyState(vk as i32) < 0;
                        let vk_bit = Self::vk_to_bit(vk);
                        let was_pressed = (self.last_vk_state & vk_bit) != 0;

                        if is_pressed && !was_pressed {
                            self.last_vk_state |= vk_bit;

                            let all_down = keys.iter().all(|&k| GetAsyncKeyState(k as i32) < 0);

                            if all_down {
                                self.app_state.handle_switch_key_toggle();
                                return;
                            }
                        } else if !is_pressed {
                            self.last_vk_state &= !vk_bit;
                        }
                    }
                }
            }
        }
    }

    /// Convert VK code to bit position using modulo mapping.
    /// Uses 16 bits to track key states with collision handling.
    #[inline(always)]
    fn vk_to_bit(vk: u32) -> u16 {
        1u16 << (vk % 16)
    }

    /// 将当前参数写入 config.vibration (索引映射, 与预设 params 顺序一致)
    fn sync_vib_config_from_params(
        cfg: &mut crate::config::VibrationConfig,
        p: &[std::sync::atomic::AtomicU32; 60],
    ) {
        use std::sync::atomic::Ordering;
        let g = cfg;
        g.attack_gain = p[0].load(Ordering::Relaxed);
        g.damage_gain = p[1].load(Ordering::Relaxed);
        g.shake_gain = p[2].load(Ordering::Relaxed);
        g.move_gain = p[3].load(Ordering::Relaxed);
        g.max_strength = p[4].load(Ordering::Relaxed);
        g.decay_ms = p[5].load(Ordering::Relaxed);
        g.hit_boost = p[6].load(Ordering::Relaxed);
        g.start_pulse = p[7].load(Ordering::Relaxed);
        g.kill_pulse = p[8].load(Ordering::Relaxed);
        g.master_gain = p[9].load(Ordering::Relaxed);
        g.font_strength = p[10].load(Ordering::Relaxed);
        g.font_interval = p[11].load(Ordering::Relaxed);
        g.font_hp = p[12].load(Ordering::Relaxed);
        g.font_special = p[13].load(Ordering::Relaxed);
        g.font_state = p[14].load(Ordering::Relaxed);
        g.font_effect = p[15].load(Ordering::Relaxed);
        g.font_attack = p[16].load(Ordering::Relaxed);
        g.font_hit = p[17].load(Ordering::Relaxed);
        g.rhythm = p[18].load(Ordering::Relaxed);
        for i in 19..60 {
            g.advanced[i - 19] = p[i].load(Ordering::Relaxed);
        }
    }

    /// 渲染某基础项的 L/R 马达权重滑块 + 实时输出条
    /// 权重范围 -100..+100: 0 = 保持原样(不动原参数), 负 = 减弱, 正 = 增强
    /// item: 0=总闸 1=攻击 2=上限 3=衰减 4=连击 5=节奏 6=通用 7=DOT 8=特效 9=状态 10=特殊 11=命中 12=受击
    fn render_item_lr(
        ui: &mut egui::Ui,
        dark: bool,
        state: &crate::state::AppState,
        config_item_lr: &mut [u32; 26],
        item: usize,
    ) {
        use std::sync::atomic::Ordering;
        let il = state.vibration_item_lr[item * 2].load(Ordering::Relaxed) as i32 as f32;
        let ir = state.vibration_item_lr[item * 2 + 1].load(Ordering::Relaxed) as i32 as f32;
        let ol = state.vibration_item_out[item * 2].load(Ordering::Relaxed);
        let or_ = state.vibration_item_out[item * 2 + 1].load(Ordering::Relaxed);
        let mut l = il;
        let mut r = ir;

        ui.vertical(|ui| {
            /* L = 橙色, R = 红色 (清晰区分), 每行一个马达, 宽度自适应 */
            let th = Theme::new(dark);
            let l_orange = th.motor_l;
            let r_red = th.motor_r;
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("L").size(13.0).strong().color(l_orange));
                ui.add(
                    egui::Slider::new(&mut l, -100.0..=100.0)
                        .show_value(false)
                        .trailing_fill(true)
                        .text_color(l_orange),
                );
                ui.add(
                    egui::ProgressBar::new(ol as f32 / 65535.0)
                        .desired_width(40.0)
                        .fill(l_orange),
                );
                ui.label(
                    egui::RichText::new(format!("L{:+}", l as i32))
                        .size(11.0)
                        .weak(),
                );
            });
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("R").size(13.0).strong().color(r_red));
                ui.add(
                    egui::Slider::new(&mut r, -100.0..=100.0)
                        .show_value(false)
                        .trailing_fill(true)
                        .text_color(r_red),
                );
                ui.add(
                    egui::ProgressBar::new(or_ as f32 / 65535.0)
                        .desired_width(40.0)
                        .fill(r_red),
                );
                ui.label(
                    egui::RichText::new(format!("R{:+}", r as i32))
                        .size(11.0)
                        .weak(),
                );
            });
        });
        if (l as i32) != (il as i32) {
            state.vibration_item_lr[item * 2].store((l as i32) as u32, Ordering::Relaxed);
            config_item_lr[item * 2] = l as u32;
        }
        if (r as i32) != (ir as i32) {
            state.vibration_item_lr[item * 2 + 1].store((r as i32) as u32, Ordering::Relaxed);
            config_item_lr[item * 2 + 1] = r as u32;
        }
    }

    /// 渲染评分事件的 L/R 马达权重滑块 (rank_lr, 30 项 = 15 事件 x L/R)
    fn render_rank_lr(
        ui: &mut egui::Ui,
        dark: bool,
        state: &crate::state::AppState,
        config_rank_lr: &mut [u32; 30],
        item: usize,
    ) {
        use std::sync::atomic::Ordering;
        let il = state.vibration_rank_lr[item * 2].load(Ordering::Relaxed) as i32 as f32;
        let ir = state.vibration_rank_lr[item * 2 + 1].load(Ordering::Relaxed) as i32 as f32;
        let ol = state.vibration_rank_out[item * 2].load(Ordering::Relaxed);
        let or_ = state.vibration_rank_out[item * 2 + 1].load(Ordering::Relaxed);
        let mut l = il;
        let mut r = ir;

        ui.vertical(|ui| {
            let th = Theme::new(dark);
            let l_orange = th.motor_l;
            let r_red = th.motor_r;
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("L").size(13.0).strong().color(l_orange));
                ui.add(
                    egui::Slider::new(&mut l, -100.0..=100.0)
                        .show_value(false)
                        .trailing_fill(true)
                        .text_color(l_orange),
                );
                ui.add(
                    egui::ProgressBar::new(ol as f32 / 65535.0)
                        .desired_width(40.0)
                        .fill(l_orange),
                );
                ui.label(
                    egui::RichText::new(format!("L{:+}", l as i32))
                        .size(11.0)
                        .weak(),
                );
            });
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("R").size(13.0).strong().color(r_red));
                ui.add(
                    egui::Slider::new(&mut r, -100.0..=100.0)
                        .show_value(false)
                        .trailing_fill(true)
                        .text_color(r_red),
                );
                ui.add(
                    egui::ProgressBar::new(or_ as f32 / 65535.0)
                        .desired_width(40.0)
                        .fill(r_red),
                );
                ui.label(
                    egui::RichText::new(format!("R{:+}", r as i32))
                        .size(11.0)
                        .weak(),
                );
            });
        });
        if (l as i32) != (il as i32) {
            state.vibration_rank_lr[item * 2].store((l as i32) as u32, Ordering::Relaxed);
            config_rank_lr[item * 2] = l as u32;
        }
        if (r as i32) != (ir as i32) {
            state.vibration_rank_lr[item * 2 + 1].store((r as i32) as u32, Ordering::Relaxed);
            config_rank_lr[item * 2 + 1] = r as u32;
        }
    }

    /// 保存当前职业参数快照到 JobVibration.toml (应用/滑块修改时调用)
    fn save_job_vibration_state(app_state: &crate::state::AppState, base_job: &str, class_name: &str) {
        use std::sync::atomic::Ordering;
        let params: Vec<u32> = (0..60)
            .map(|i| app_state.vibration_params[i].load(Ordering::Relaxed))
            .collect();
        crate::job_presets::save_job_vibration(&crate::job_presets::JobVibrationConfig {
            base_job: base_job.to_string(),
            class_name: class_name.to_string(),
            applied: true,
            params,
            rank_duration: app_state.vibration_rank_duration.load(Ordering::Relaxed),
            rank_level_gain: app_state.vibration_rank_level_gain.load(Ordering::Relaxed),
        });
    }

    /// 导出通用震动设定 (当前生效参数 + 评分 + 高级算法) 为 TOML 字符串
    fn export_vibration_settings(app_state: &crate::state::AppState, config: &crate::config::AppConfig) -> String {
        use std::fmt::Write;
        use std::sync::atomic::Ordering;
        let mut s = String::new();
        s.push_str("# ═══════════════════════════════════════════════════════\n");
        s.push_str("#  Sorahk 通用震动设定导出\n");
        s.push_str("# ═══════════════════════════════════════════════════════\n\n");
        s.push_str(&format!("independent_test = {}\n", config.vibration.independent_test));
        s.push_str(&format!("move_independent = {}\n", config.vibration.move_independent));
        s.push_str(&format!("out_smooth = {}\n", config.vibration.out_smooth));
        s.push_str(&format!("throttle_window_ms = {}\n", config.vibration.throttle_window_ms));
        s.push_str(&format!("throttle_max_hits = {}\n", config.vibration.throttle_max_hits));
        s.push_str(&format!("throttle_dense_ratio = {}\n", config.vibration.throttle_dense_ratio));
        s.push_str(&format!("random_gain = {}\n", config.vibration.random_gain));
        s.push_str(&format!("motor_l_gain = {}\n", config.vibration.motor_l_gain));
        s.push_str(&format!("motor_r_gain = {}\n", config.vibration.motor_r_gain));
        s.push_str(&format!("rank_level_gain = {}\n", app_state.vibration_rank_level_gain.load(Ordering::Relaxed)));
        s.push_str(&format!("rank_duration = {}\n", app_state.vibration_rank_duration.load(Ordering::Relaxed)));
        let _ = writeln!(s, "\nparams = [{}]", (0..60)
            .map(|i| app_state.vibration_params[i].load(Ordering::Relaxed).to_string())
            .collect::<Vec<_>>().join(", "));
        let _ = writeln!(s, "item_lr = [{}]", (0..26)
            .map(|i| app_state.vibration_item_lr[i].load(Ordering::Relaxed).to_string())
            .collect::<Vec<_>>().join(", "));
        let _ = writeln!(s, "rank_lr = [{}]", (0..30)
            .map(|i| app_state.vibration_rank_lr[i].load(Ordering::Relaxed).to_string())
            .collect::<Vec<_>>().join(", "));
        let _ = writeln!(s, "rank_type_gain = [{}]", (0..15)
            .map(|i| app_state.vibration_rank_type_gain[i].load(Ordering::Relaxed).to_string())
            .collect::<Vec<_>>().join(", "));
        s
    }

    /// 导出当前职业完整设置 (全职业预设页) 为 TOML 字符串
    fn export_job_settings(base: &str, cls: &crate::job_presets::JobClass) -> String {
        use std::fmt::Write;
        let mut s = String::new();
        s.push_str("# ═══════════════════════════════════════════════════════\n");
        s.push_str("#  Sorahk 全职业预设导出\n");
        s.push_str("# ═══════════════════════════════════════════════════════\n\n");
        let _ = writeln!(s, "base_job = \"{}\"", base);
        let _ = writeln!(s, "class_name = \"{}\"", cls.name);
        let _ = writeln!(s, "out_smooth = {}", cls.out_smooth);
        let _ = writeln!(s, "throttle_window = {}", cls.throttle_window);
        let _ = writeln!(s, "throttle_max = {}", cls.throttle_max);
        let _ = writeln!(s, "throttle_dense_ratio = {}", cls.throttle_dense_ratio);
        let _ = writeln!(s, "rank_level_gain = {}", cls.rank_level_gain);
        let _ = writeln!(s, "rank_duration = {}", cls.rank_duration);
        let _ = writeln!(s, "desc = \"{}\"", cls.desc);
        let _ = writeln!(s, "\nparams = [{}]", cls.params.iter()
            .map(|v| v.to_string()).collect::<Vec<_>>().join(", "));
        let _ = writeln!(s, "rank_lr = [{}]", cls.rank_lr.iter()
            .map(|v| v.to_string()).collect::<Vec<_>>().join(", "));
        let _ = writeln!(s, "rank_type_gain = [{}]", cls.rank_type_gain.iter()
            .map(|v| v.to_string()).collect::<Vec<_>>().join(", "));
        s
    }

    /// 把导出内容写入文件 (程序目录), 返回文件名
    fn save_export_file(content: &str, prefix: &str) -> String {
        use std::io::Write;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let fname = format!("{}_{}.toml", prefix, now);
        if let Ok(mut f) = std::fs::File::create(&fname) {
            let _ = f.write_all(content.as_bytes());
            let _ = f.flush();
        }
        fname
    }

    /// 【调试模式】独立测试开关 (v24.2)。
    ///
    /// 开 = 评分/移动通道独立于全局总调整 (单通道调试用, 方便逐项验证);
    /// 关 = 全局总调整统一应用到所有通道 (正常使用)。
    /// 不影响各卡片自己的测试按钮与滑块。状态持久化到 Config.toml。
    fn render_independent_test_toggle(
        ui: &mut egui::Ui,
        app_state: &crate::state::AppState,
        config: &mut crate::config::AppConfig,
    ) {
        use std::sync::atomic::Ordering;
        let dark = config.dark_mode;
        let th = Theme::new(dark);
        let mut ind = app_state.vibration_independent_test.load(Ordering::Relaxed);

        th.panel(ui, Some("🧪 调试模式 (独立测试)"), |ui| {
            ui.horizontal(|ui| {
                if ui
                    .checkbox(
                        &mut ind,
                        "评分/移动通道独立于全局总调整",
                    )
                    .on_hover_text(
                        "开启后: 全局总调整不会影响评分震动与移动持续震动 (单通道调试方便)。\n\
                         关闭后: 全局总调整统一应用到所有通道 (正常使用)。\n\
                         不影响各卡片自己的测试按钮与滑块。",
                    )
                    .changed()
                {
                    app_state
                        .vibration_independent_test
                        .store(ind, Ordering::Relaxed);
                    config.vibration.independent_test = ind;
                    let _ = config.save_vibration_to_file(crate::config::AppConfig::vibration_path_for("Config.toml"));
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let (text, fg, bg) = if ind {
                        ("调试中", th.warn, th.warn_soft)
                    } else {
                        ("正常模式", th.good, th.good_soft)
                    };
                    th.badge(ui, text, fg, bg);
                });
            });
            ui.add_space(4.0);
            ui.label(th.hint_text(if ind {
                "调试模式已开启: 评分/移动通道不吃全局总调整, 便于逐项单独验证。"
            } else {
                "正常模式: 全局总调整统一作用于所有通道。需要单通道调试时再打开。"
            }));
        });
    }

    /// 震动分区卡片: 与主窗口其他卡片视觉统一 (容器样式见 Theme::panel)。
    fn vib_section(
        ui: &mut egui::Ui,
        dark: bool,
        title: &str,
        body: impl FnOnce(&mut egui::Ui),
    ) {
        Theme::new(dark).panel(ui, Some(title), body);
        ui.add_space(10.0);
    }

    /// Full vibration settings page (all params + status + test).
    fn render_vibration_full_page(&mut self, ui: &mut egui::Ui, _frame_state: &FrameState, job_mode: bool) {
        use std::sync::atomic::Ordering;
        let th = self.theme();
        let title_color = th.heading;

        /* 放大字号与控件: 震动页全局风格; 滑块宽度随窗口自适应, 宽屏不再挤在左侧 */
        ui.style_mut().text_styles.insert(
            egui::TextStyle::Body,
            egui::FontId::new(14.0, egui::FontFamily::Proportional),
        );
        ui.style_mut().text_styles.insert(
            egui::TextStyle::Button,
            egui::FontId::new(14.0, egui::FontFamily::Proportional),
        );
        ui.style_mut().spacing.slider_width = ((ui.available_width() - 240.0) * 0.5).clamp(200.0, 340.0);
        ui.style_mut().spacing.interact_size.y = 26.0;

        /* 初始化参数(仅一次): 仅"全新安装"(master/attack 全 0 = 从未应用过
         * 任何预设)才应用内置"默认"预设。已有用户配置时 params 已由
         * AppState::new 从 config.vibration 恢复 —— 旧实现无条件覆盖, 会把
         * 用户"保存当前"落盘的参数在首渲染时替换回内置默认, 与持久化语义矛盾。 */
        {
            use std::sync::atomic::AtomicBool;
            static INIT_DONE: AtomicBool = AtomicBool::new(false);
            if !INIT_DONE.load(Ordering::Relaxed) {
                INIT_DONE.store(true, Ordering::Relaxed);
                let p = &self.app_state.vibration_params;
                let pristine = self.config.vibration.master_gain == 0
                    && self.config.vibration.attack_gain == 0;
                if pristine {
                if let Some(pr) = crate::config::default_vibration_presets().first() {
                    for (i, v) in pr.params.iter().enumerate() {
                        p[i].store(*v, Ordering::Relaxed);
                    }
                    /* 同步 L/R 权重 + 评分参数 (UI 显示与内置默认一致) */
                    for (i, v) in pr.item_lr.iter().enumerate() {
                        self.app_state.vibration_item_lr[i].store(*v, Ordering::Relaxed);
                        self.config.vibration.item_lr[i] = *v;
                    }
                    for (i, v) in pr.rank_lr.iter().enumerate() {
                        self.app_state.vibration_rank_lr[i].store(*v, Ordering::Relaxed);
                        self.config.vibration.rank_lr[i] = *v;
                    }
                    for (i, v) in pr.rank_type_gain.iter().enumerate() {
                        self.app_state.vibration_rank_type_gain[i].store(*v, Ordering::Relaxed);
                        self.config.vibration.rank_type_gain[i] = *v;
                    }
                    self.app_state.vibration_rank_level_gain.store(pr.rank_level_gain, Ordering::Relaxed);
                    self.app_state.vibration_rank_duration.store(pr.rank_duration, Ordering::Relaxed);
                    self.config.vibration.rank_level_gain = pr.rank_level_gain;
                    self.config.vibration.rank_duration = pr.rank_duration;
                        self.app_state.vibration_out_smooth.store(pr.out_smooth, Ordering::Relaxed);
                        self.config.vibration.out_smooth = pr.out_smooth;
                }
                }
                /* 全职业预设恢复 (v21): applied=true 时覆盖默认预设; 否则绝对频率强制关闭 */
                if let Some(jcfg) = crate::job_presets::load_job_vibration() {
                    self.vib_job_enabled = jcfg.applied;
                    if !jcfg.applied {
                        self.app_state.vibration_abs_freq_enabled.store(false, Ordering::Relaxed);
                        self.config.vibration.abs_freq_enabled = false;
                    }
                    if jcfg.applied {                        if let Some((bi, ci, cls)) = crate::job_presets::find_class(&jcfg.base_job, &jcfg.class_name) {
                            self.vib_job_base = bi;
                            self.vib_job_class = ci;
                            let params: Vec<u32> = if jcfg.params.len() == 60 {
                                jcfg.params.clone()
                            } else {
                                cls.params.to_vec()
                            };
                            self.vib_job_snapshot = params.clone();
                            for (i, v) in params.iter().enumerate() {
                                p[i].store(*v, Ordering::Relaxed);
                            }
                            for (i, v) in cls.rank_lr.iter().enumerate() {
                                self.app_state.vibration_rank_lr[i].store(*v as u32, Ordering::Relaxed);
                            }
                            for (i, v) in cls.rank_type_gain.iter().enumerate() {
                                self.app_state.vibration_rank_type_gain[i].store(*v, Ordering::Relaxed);
                            }
                            let dur = if jcfg.rank_duration > 0 { jcfg.rank_duration } else { cls.rank_duration };
                            let lg = if jcfg.rank_level_gain > 0 { jcfg.rank_level_gain } else { cls.rank_level_gain };
                            self.app_state.vibration_rank_duration.store(dur, Ordering::Relaxed);
                            self.app_state.vibration_rank_level_gain.store(lg, Ordering::Relaxed);
                            self.app_state.vibration_out_smooth.store(cls.out_smooth, Ordering::Relaxed);
                            self.config.vibration.out_smooth = cls.out_smooth;
                            self.app_state.vibration_throttle_window.store(cls.throttle_window, Ordering::Relaxed);
                            self.app_state.vibration_throttle_max.store(cls.throttle_max, Ordering::Relaxed);
                            self.app_state.vibration_throttle_dense_ratio.store(cls.throttle_dense_ratio, Ordering::Relaxed);
                            self.app_state.vibration_abs_freq_enabled.store(cls.abs_freq_enabled, Ordering::Relaxed);
                            self.app_state.vibration_algo_id.store(cls.algo_id as u32, Ordering::Relaxed);
                            for (i, v) in cls.algo_params.iter().enumerate() {
                                self.app_state.vibration_algo_ap[i].store(*v, Ordering::Relaxed);
                            }
                            self.config.vibration.throttle_window_ms = cls.throttle_window;
                            self.config.vibration.throttle_max_hits = cls.throttle_max;
                            self.config.vibration.throttle_dense_ratio = cls.throttle_dense_ratio;
                            self.config.vibration.abs_freq_enabled = cls.abs_freq_enabled;
                            self.vib_job_active = Some((jcfg.base_job.clone(), jcfg.class_name.clone()));
                            self.vib_job_loaded = Some((bi, ci));
                        }
                    }
                }
            }
        }

        egui::Frame::NONE
            .inner_margin(egui::Margin::same(6))
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                let p = &self.app_state.vibration_params;

                /* 标题行 */
                Self::vib_section(ui, self.dark_mode, "震动控制中心", |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("通用型震动设定 (DFO 战斗事件 → 手柄马达)")
                            .size(17.0)
                            .strong()
                            .color(title_color),
                    );
                    ui.add_space(20.0);
                    let vib_on = self
                        .app_state
                        .vibration_enabled
                        .load(Ordering::Relaxed);
                    let (vtext, vcolor) = if vib_on {
                        ("震动: 开", th.good)
                    } else {
                        ("震动: 关", th.bad)
                    };
                    if ui
                        .button(egui::RichText::new(vtext).size(14.0).color(vcolor))
                        .clicked()
                    {
                        let v = self
                            .app_state
                            .vibration_enabled
                            .load(Ordering::Relaxed);
                        self.app_state
                            .vibration_enabled
                            .store(!v, Ordering::Relaxed);
                    }
                    ui.add_space(12.0);
                    if ui
                        .button(egui::RichText::new("测试震动 1 秒").size(13.0))
                        .clicked()
                    {
                        let until = crate::vibration::now_ms_u64() + 1000;
                        self.app_state
                            .vibration_test_until
                            .store(until, Ordering::Relaxed);
                    }
                });
                ui.add_space(6.0);

                /* 独立测试开关 (v24.2): 控制中心主开关 */
                Self::render_independent_test_toggle(ui, &self.app_state, &mut self.config);
                ui.add_space(6.0);

                /* 全职业预设页: 应用开关 + 职业选择 (替换震动设置预设行) */
                if job_mode {
                    let jobs = crate::job_presets::builtin_jobs();
                    let base_sel = self.vib_job_base.min(jobs.len().saturating_sub(1));
                    let classes = &jobs[base_sel].classes;
                    let class_sel = self.vib_job_class.min(classes.len().saturating_sub(1));
                    let cur_cls = &classes[class_sel];
                    /* 应用开关 */
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("全职业预设:").size(13.0).strong());
                        let mut en = self.vib_job_enabled;
                        if ui
                            .checkbox(
                                &mut en,
                                "应用全职业预设 (开: 职业参数覆盖震动设置; 关: 不应用, 恢复震动设置默认)",
                            )
                            .changed()
                        {
                            self.vib_job_enabled = en;
                            if en {
                                Self::apply_job_vibration_preset_in(
                                    jobs[base_sel].base_job,
                                    cur_cls,
                                    &self.app_state,
                                    &mut self.config,
                                    &mut self.vib_job_enabled,
                                    &mut self.vib_job_active,
                                    &mut self.vib_job_snapshot,
                                );
                                self.vib_job_loaded = Some((base_sel, class_sel));
                            } else {
                                Self::disable_job_vibration_preset_in(
                                    &self.app_state,
                                    &mut self.config,
                                    &mut self.vib_job_enabled,
                                    &mut self.vib_job_active,
                                    &mut self.vib_job_loaded,
                                );
                            }
                        }
                    });
                    ui.add_space(4.0);
                    /* 职业选择行 */
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("基础职业:").size(13.0).strong());
                        let base_names: Vec<String> = jobs.iter().map(|j| j.base_job.to_string()).collect();
                        egui::ComboBox::from_id_salt("vib_job_base")
                            .selected_text(base_names[base_sel].clone())
                            .width(140.0)
                            .show_ui(ui, |ui| {
                                for (i, n) in base_names.iter().enumerate() {
                                    if ui.selectable_label(i == base_sel, n).clicked() {
                                        self.vib_job_base = i;
                                        self.vib_job_class = 0;
                                    }
                                }
                            });
                        ui.add_space(8.0);
                        ui.label(egui::RichText::new("转职:").size(13.0).strong());
                        let class_names: Vec<String> = classes.iter().map(|c| c.name.to_string()).collect();
                        egui::ComboBox::from_id_salt("vib_job_class")
                            .selected_text(class_names[class_sel].clone())
                            .width(140.0)
                            .show_ui(ui, |ui| {
                                for (i, n) in class_names.iter().enumerate() {
                                    if ui.selectable_label(i == class_sel, n).clicked() {
                                        self.vib_job_class = i;
                                    }
                                }
                            });
                        ui.add_space(8.0);
                        if ui
                            .button(egui::RichText::new("应用该职业").size(13.0))
                            .clicked()
                        {
                            Self::apply_job_vibration_preset_in(
                                jobs[base_sel].base_job,
                                cur_cls,
                                &self.app_state,
                                &mut self.config,
                                &mut self.vib_job_enabled,
                                &mut self.vib_job_active,
                                &mut self.vib_job_snapshot,
                            );
                            self.vib_job_loaded = Some((base_sel, class_sel));
                        }
                        ui.add_space(8.0);
                        if ui
                            .button(egui::RichText::new("导出当前职业").size(13.0))
                            .on_hover_text("导出当前选择职业的完整设置 (仅当前职业)")
                            .clicked()
                        {
                            let content = Self::export_job_settings(jobs[base_sel].base_job, cur_cls);
                            let fname = Self::save_export_file(&content, &format!("job_export_{}", cur_cls.name));
                            self.vib_export_msg = Some(fname);
                        }
                        ui.add_space(8.0);
                        /* 导入职业设置 (v33.1: 按规范 base_job/class_name 匹配内置职业) */
                        if ui
                            .button(egui::RichText::new("导入职业设置").size(13.0))
                            .on_hover_text("从 job_export_*.toml 导入 (按 base_job/class_name 匹配内置职业后应用)")
                            .clicked()
                        {
                            let files = crate::job_presets::list_export_files("job_export");
                            let sel = self.job_import_sel.min(files.len().saturating_sub(1));
                            if let Some(f) = files.get(sel) {
                                match crate::job_presets::parse_job_export(f) {
                                    Some(ij) => {
                                        /* 规范: 按 base_job/class_name 匹配内置职业 */
                                        if let Some((bi, ci, _cls)) = crate::job_presets::find_class(&ij.base_job, &ij.class_name) {
                                            self.vib_job_base = bi;
                                            self.vib_job_class = ci;
                                            /* 导入参数覆盖 (只写 AppState; 快照检测自动落盘 JobVibration.toml) */
                                            for (i, v) in ij.params.iter().enumerate() {
                                                self.app_state.vibration_params[i].store(*v, Ordering::Relaxed);
                                            }
                                            for (i, v) in ij.rank_lr.iter().enumerate() {
                                                self.app_state.vibration_rank_lr[i].store(*v as u32, Ordering::Relaxed);
                                            }
                                            for (i, v) in ij.rank_type_gain.iter().enumerate() {
                                                self.app_state.vibration_rank_type_gain[i].store(*v, Ordering::Relaxed);
                                            }
                                            if ij.rank_level_gain > 0 {
                                                self.app_state.vibration_rank_level_gain.store(ij.rank_level_gain, Ordering::Relaxed);
                                            }
                                            if ij.rank_duration > 0 {
                                                self.app_state.vibration_rank_duration.store(ij.rank_duration, Ordering::Relaxed);
                                            }
                                            if ij.out_smooth > 0 {
                                                self.app_state.vibration_out_smooth.store(ij.out_smooth, Ordering::Relaxed);
                                            }
                                            self.app_state.vibration_throttle_window.store(ij.throttle_window, Ordering::Relaxed);
                                            self.app_state.vibration_throttle_max.store(ij.throttle_max, Ordering::Relaxed);
                                            self.app_state.vibration_throttle_dense_ratio.store(ij.throttle_dense_ratio, Ordering::Relaxed);
                                            self.vib_job_active = Some((ij.base_job.clone(), ij.class_name.clone()));
                                            self.vib_job_loaded = Some((bi, ci));
                                            self.vib_job_enabled = true;
                                            self.vib_import_msg = Some(format!("已导入并应用: {} - {}", ij.base_job, ij.class_name));
                                        } else {
                                            self.vib_import_msg = Some(format!("规范不符: 未匹配内置职业 {} - {}", ij.base_job, ij.class_name));
                                        }
                                    }
                                    None => {
                                        self.vib_import_msg = Some(format!("{} 解析失败 (规范不符)", f));
                                    }
                                }
                            }
                        }
                    });
                    /* 开关打开时, 切换职业自动应用 */
                    if self.vib_job_enabled && self.vib_job_loaded != Some((base_sel, class_sel)) {
                        Self::apply_job_vibration_preset_in(
                            jobs[base_sel].base_job,
                            cur_cls,
                            &self.app_state,
                            &mut self.config,
                            &mut self.vib_job_enabled,
                            &mut self.vib_job_active,
                            &mut self.vib_job_snapshot,
                        );
                        self.vib_job_loaded = Some((base_sel, class_sel));
                    }
                    if let Some(m) = &self.vib_import_msg {
                    ui.label(
                        egui::RichText::new(format!("{}", m))
                            .size(11.0)
                            .color(th.info),
                    );
                    ui.add_space(2.0);
                }
                if let Some(m) = &self.vib_export_msg {
                        ui.label(
                            egui::RichText::new(format!("已导出: {} (程序目录)", m))
                                .size(11.0)
                                .color(th.good),
                        );
                        ui.add_space(2.0);
                    }
                    ui.add_space(4.0);
                    if let Some((b, c)) = &self.vib_job_active {
                        ui.label(
                            egui::RichText::new(format!("当前职业: {} - {} (已应用职业预设, 通用型震动设定被覆盖)", b, c))
                                .size(12.0)
                                .color(th.good)
                                .strong(),
                        );
                    } else {
                        ui.label(
                            egui::RichText::new("开关已关闭: 全职业预设不应用, 使用通用型震动设定参数")
                                .size(11.0)
                                .weak(),
                        );
                    }
                    ui.add_space(4.0);
                    ui.label(
                        egui::RichText::new(format!("{} (攻速→力度设计: 极快轻盈防叠加震手, 慢速沉稳给强度)", cur_cls.desc))
                            .size(11.0)
                            .weak(),
                    );
                    ui.add_space(4.0);
                    ui.separator();
                    ui.add_space(4.0);
                    ui.label(
                        egui::RichText::new("下方参数与高级震动调校与【通用型震动设定】一致, 修改实时生效并自动保存到 JobVibration.toml")
                            .size(11.0)
                            .weak(),
                    );
                } else {
                /* 预设行 */
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("预设:").size(13.0).strong(),
                    );
                    let names: Vec<String> = self
                        .config
                        .vibration_presets
                        .iter()
                        .map(|p| p.name.clone())
                        .collect();
                    let sel = self.vib_preset_idx.min(names.len().saturating_sub(1));
                    if !names.is_empty() {
                        egui::ComboBox::from_id_salt("vib_preset_sel")
                            .selected_text(names[sel].clone())
                            .width(140.0)
                            .show_ui(ui, |ui| {
                                for (i, n) in names.iter().enumerate() {
                                    if ui.selectable_label(i == sel, n).clicked() {
                                        self.vib_preset_idx = i;
                                    }
                                }
                            });
                    }
                    if ui
                        .button(egui::RichText::new("应用").size(13.0))
                        .clicked()
                    {
                        if let Some(pr0) = self.config.vibration_presets.get(sel) {
                            /* 内置同名预设优先: 兼容旧 Config.toml (旧预设无评分参数字段,
                             * serde 默认 [100;15]/100/300, 应用时用内置新设计值覆盖) */
                            let pr = crate::config::default_vibration_presets()
                                .into_iter()
                                .find(|p| p.name == pr0.name)
                                .unwrap_or_else(|| pr0.clone());
                            for (i, v) in pr.params.iter().enumerate() {
                                p[i].store(*v, Ordering::Relaxed);
                            }
                            /* 应用预设的 L/R 权重 */
                            for (i, v) in pr.item_lr.iter().enumerate() {
                                self.app_state.vibration_item_lr[i].store(*v, Ordering::Relaxed);
                                self.config.vibration.item_lr[i] = *v;
                            }
                            /* 应用预设的评分事件 L/R 权重 */
                            for (i, v) in pr.rank_lr.iter().enumerate() {
                                self.app_state.vibration_rank_lr[i].store(*v, Ordering::Relaxed);
                                self.config.vibration.rank_lr[i] = *v;
                            }
                            /* 应用预设的评分特效参数 (细分强度/等级强度/时长) */
                            for (i, v) in pr.rank_type_gain.iter().enumerate() {
                                self.app_state.vibration_rank_type_gain[i].store(*v, Ordering::Relaxed);
                            }
                            self.app_state.vibration_rank_level_gain.store(pr.rank_level_gain, Ordering::Relaxed);
                            self.app_state.vibration_rank_duration.store(pr.rank_duration, Ordering::Relaxed);
                            self.config.vibration.rank_type_gain = pr.rank_type_gain;
                            self.config.vibration.rank_level_gain = pr.rank_level_gain;
                            self.config.vibration.rank_duration = pr.rank_duration;
                        self.app_state.vibration_out_smooth.store(pr.out_smooth, Ordering::Relaxed);
                        self.config.vibration.out_smooth = pr.out_smooth;
                            /* 同步到 config.vibration 并落盘(重启保持) */
                            Self::sync_vib_config_from_params(&mut self.config.vibration, p);
                            let _ = self.config.save_vibration_to_file(crate::config::AppConfig::vibration_path_for("Config.toml"));
                        }
                    }
                    ui.add_space(8.0);
                    if ui
                        .button(egui::RichText::new("保存当前").size(13.0))
                        .on_hover_text("把当前滑块参数写入 [vibration] 并保存 Config.toml")
                        .clicked()
                    {
                        Self::sync_vib_config_from_params(&mut self.config.vibration, p);
                        for i in 0..26 {
                            self.config.vibration.item_lr[i] = self
                                .app_state
                                .vibration_item_lr[i]
                                .load(Ordering::Relaxed);
                        }
                        for i in 0..30 {
                            self.config.vibration.rank_lr[i] = self
                                .app_state
                                .vibration_rank_lr[i]
                                .load(Ordering::Relaxed);
                        }
                        for i in 0..15 {
                            self.config.vibration.rank_type_gain[i] = self
                                .app_state
                                .vibration_rank_type_gain[i]
                                .load(Ordering::Relaxed);
                        }
                        self.config.vibration.rank_level_gain = self
                            .app_state
                            .vibration_rank_level_gain
                            .load(Ordering::Relaxed);
                        self.config.vibration.rank_duration = self
                            .app_state
                            .vibration_rank_duration
                            .load(Ordering::Relaxed);
                        let _ = self.config.save_vibration_to_file(crate::config::AppConfig::vibration_path_for("Config.toml"));
                    }
                    ui.add_space(8.0);
                    ui.label(egui::RichText::new("保存为:").size(13.0).weak());
                    ui.add(
                        egui::TextEdit::singleline(&mut self.vib_preset_name)
                            .desired_width(90.0)
                            .hint_text("名称"),
                    );
                    if ui
                        .button(egui::RichText::new("保存").size(13.0))
                        .clicked()
                    {
                        let name = self.vib_preset_name.trim().to_string();
                        if !name.is_empty() {
                            let mut params = [0u32; 60];
                            for i in 0..60 {
                                params[i] = p[i].load(Ordering::Relaxed);
                            }
                            let mut item_lr = [0u32; 26];
                            for i in 0..26 {
                                item_lr[i] = self.app_state.vibration_item_lr[i].load(Ordering::Relaxed);
                            }
                            let mut rank_lr = [0u32; 30];
                            for i in 0..30 {
                                rank_lr[i] = self.app_state.vibration_rank_lr[i].load(Ordering::Relaxed);
                            }
                            let mut rank_type_gain = [0u32; 15];
                            for i in 0..15 {
                                rank_type_gain[i] = self.app_state.vibration_rank_type_gain[i].load(Ordering::Relaxed);
                            }
                            let rank_level_gain = self.app_state.vibration_rank_level_gain.load(Ordering::Relaxed);
                            let rank_duration = self.app_state.vibration_rank_duration.load(Ordering::Relaxed);
                            let preset = crate::config::VibrationPreset { name: name.clone(), params, item_lr, rank_lr, rank_type_gain, rank_level_gain, rank_duration, out_smooth: self.app_state.vibration_out_smooth.load(Ordering::Relaxed) };
                            // 同名替换而非追加: 追加重名预设后, 应用按名匹配会
                            // 顶替用户自存版本, 且重名项永远删不掉
                            let protected = name == "默认" || name == "测试版(全0)";
                            if let Some(pos) = self.config.vibration_presets.iter().position(|x| x.name == name) {
                                if !protected {
                                    self.config.vibration_presets[pos] = preset;
                                    self.vib_preset_name.clear();
                                    self.vib_preset_idx = pos;
                                }
                            } else {
                                self.config.vibration_presets.push(preset);
                                self.vib_preset_name.clear();
                                self.vib_preset_idx =
                                    self.config.vibration_presets.len() - 1;
                            }
                            let _ = self.config.save_vibration_to_file(crate::config::AppConfig::vibration_path_for("Config.toml"));
                        }
                    }
                    if ui
                        .button(egui::RichText::new("删除").size(13.0))
                        .on_hover_text("仅\"默认\"与\"测试版(全0)\"受保护, 其余可删除")
                        .clicked()
                    {
                        if let Some(pr) = self.config.vibration_presets.get(sel) {
                            let protected =
                                pr.name == "默认" || pr.name == "测试版(全0)";
                            if !protected {
                                self.config.vibration_presets.remove(sel);
                                self.vib_preset_idx = 0;
                                let _ = self.config.save_vibration_to_file(crate::config::AppConfig::vibration_path_for("Config.toml"));
                            }
                        }
                    }
                    ui.add_space(8.0);
                    if ui
                        .button(egui::RichText::new("导出当前设置").size(13.0))
                        .on_hover_text("导出当前页面生效的通用震动设定 (params/评分/高级算法) 为 TOML")
                        .clicked()
                    {
                        let content = Self::export_vibration_settings(&self.app_state, &self.config);
                        let fname = Self::save_export_file(&content, "vibration_export");
                        self.vib_export_msg = Some(fname);
                    }
                    ui.add_space(8.0);
                    /* 导入设置 (v33.1: 扫描 vibration_export_*.toml) */
                    {
                        let files = crate::job_presets::list_export_files("vibration_export");
                        let sel = self.vib_import_sel.min(files.len().saturating_sub(1));
                        if !files.is_empty() {
                            egui::ComboBox::from_id_salt("vib_import_sel")
                                .selected_text(files[sel].clone())
                                .width(220.0)
                                .show_ui(ui, |ui| {
                                    for (i, f) in files.iter().enumerate() {
                                        if ui.selectable_label(i == sel, f).clicked() {
                                            self.vib_import_sel = i;
                                        }
                                    }
                                });
                        }
                        if ui
                            .button(egui::RichText::new("导入设置").size(13.0))
                            .on_hover_text("从 vibration_export_*.toml 导入通用震动设定 (往返一致规范)")
                            .clicked()
                        {
                            if let Some(f) = crate::job_presets::list_export_files("vibration_export").get(self.vib_import_sel) {
                                if let Some(iv) = crate::job_presets::parse_vibration_export(f) {
                                    for (i, v) in iv.params.iter().enumerate() {
                                        self.app_state.vibration_params[i].store(*v, Ordering::Relaxed);
                                    }
                                    for (i, v) in iv.item_lr.iter().enumerate() {
                                        self.app_state.vibration_item_lr[i].store(*v, Ordering::Relaxed);
                                        self.config.vibration.item_lr[i] = *v;
                                    }
                                    for (i, v) in iv.rank_lr.iter().enumerate() {
                                        self.app_state.vibration_rank_lr[i].store(*v, Ordering::Relaxed);
                                        self.config.vibration.rank_lr[i] = *v;
                                    }
                                    for (i, v) in iv.rank_type_gain.iter().enumerate() {
                                        self.app_state.vibration_rank_type_gain[i].store(*v, Ordering::Relaxed);
                                        self.config.vibration.rank_type_gain[i] = *v;
                                    }
                                    self.app_state.vibration_rank_level_gain.store(iv.rank_level_gain, Ordering::Relaxed);
                                    self.app_state.vibration_rank_duration.store(iv.rank_duration, Ordering::Relaxed);
                                    self.app_state.vibration_out_smooth.store(iv.out_smooth, Ordering::Relaxed);
                                    self.app_state.vibration_throttle_window.store(iv.throttle_window_ms, Ordering::Relaxed);
                                    self.app_state.vibration_throttle_max.store(iv.throttle_max_hits, Ordering::Relaxed);
                                    self.app_state.vibration_throttle_dense_ratio.store(iv.throttle_dense_ratio, Ordering::Relaxed);
                                    self.app_state.vibration_independent_test.store(iv.independent_test, Ordering::Relaxed);
                                    self.app_state.vibration_move_independent.store(iv.move_independent, Ordering::Relaxed);
                                    self.app_state.vibration_random_gain.store(iv.random_gain, Ordering::Relaxed);
                                    self.app_state.vibration_motor_l_gain.store(iv.motor_l_gain, Ordering::Relaxed);
                                    self.app_state.vibration_motor_r_gain.store(iv.motor_r_gain, Ordering::Relaxed);
                                    self.config.vibration.rank_level_gain = iv.rank_level_gain;
                                    self.config.vibration.rank_duration = iv.rank_duration;
                                    self.config.vibration.out_smooth = iv.out_smooth;
                                    self.config.vibration.throttle_window_ms = iv.throttle_window_ms;
                                    self.config.vibration.throttle_max_hits = iv.throttle_max_hits;
                                    self.config.vibration.throttle_dense_ratio = iv.throttle_dense_ratio;
                                    self.config.vibration.independent_test = iv.independent_test;
                                    self.config.vibration.move_independent = iv.move_independent;
                                    self.config.vibration.random_gain = iv.random_gain;
                                    self.config.vibration.motor_l_gain = iv.motor_l_gain;
                                    self.config.vibration.motor_r_gain = iv.motor_r_gain;
                                    let _ = self.config.save_vibration_to_file(crate::config::AppConfig::vibration_path_for("Config.toml"));
                                    self.vib_import_msg = Some(f.clone());
                                } else {
                                    self.vib_import_msg = Some(format!("{} 解析失败 (规范不符)", f));
                                }
                            }
                        }
                    }
                    ui.add_space(8.0);
                    if ui
                        .button(egui::RichText::new("还原默认").size(13.0))
                        .on_hover_text("把预设恢复为出厂内置 (误删后一键还原)")
                        .clicked()
                    {
                        self.config.vibration_presets =
                            crate::config::default_vibration_presets();
                        self.vib_preset_idx = 0;
                        /* 若当前为空或损坏, 同时把参数复位为默认预设 */
                        if let Some(pr) = self.config.vibration_presets.first() {
                            if p[0].load(Ordering::Relaxed) == 0 {
                                for (i, v) in pr.params.iter().enumerate() {
                                    p[i].store(*v, Ordering::Relaxed);
                                }
                                for (i, v) in pr.item_lr.iter().enumerate() {
                                    self.app_state.vibration_item_lr[i].store(*v, Ordering::Relaxed);
                                    self.config.vibration.item_lr[i] = *v;
                                }
                                for (i, v) in pr.rank_lr.iter().enumerate() {
                                    self.app_state.vibration_rank_lr[i].store(*v, Ordering::Relaxed);
                                    self.config.vibration.rank_lr[i] = *v;
                                }
                                for (i, v) in pr.rank_type_gain.iter().enumerate() {
                                    self.app_state.vibration_rank_type_gain[i].store(*v, Ordering::Relaxed);
                                    self.config.vibration.rank_type_gain[i] = *v;
                                }
                                self.app_state.vibration_rank_level_gain.store(pr.rank_level_gain, Ordering::Relaxed);
                                self.app_state.vibration_rank_duration.store(pr.rank_duration, Ordering::Relaxed);
                                self.config.vibration.rank_level_gain = pr.rank_level_gain;
                                self.config.vibration.rank_duration = pr.rank_duration;
                        self.app_state.vibration_out_smooth.store(pr.out_smooth, Ordering::Relaxed);
                        self.config.vibration.out_smooth = pr.out_smooth;
                            }
                        }
                        let _ = self.config.save_vibration_to_file(crate::config::AppConfig::vibration_path_for("Config.toml"));
                    }
                });
                ui.add_space(6.0);
                if let Some(m) = &self.vib_export_msg {
                    ui.label(
                        egui::RichText::new(format!("已导出: {} (程序目录)", m))
                            .size(11.0)
                            .color(th.good),
                    );
                    ui.add_space(2.0);
                }
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(
                            "内置: 默认/高振幅/节奏律动/极简轻巧/实战竞技/测试版(全0)",
                        )
                        .size(11.0)
                        .weak(),
                    )
                    .wrap(),
                );
                ui.add_space(4.0);
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(
                            "提示: 仅\"默认\"与\"测试版(全0)\"不可删除; 误删可用\"还原默认\"恢复。",
                        )
                        .size(11.0)
                        .weak(),
                    )
                    .wrap(),
                );
                ui.add_space(8.0);
                ui.separator();
                ui.add_space(8.0);

                } /* job_mode else 结束 */

                /* 状态 */
                {
                    let connected = self.app_state.vibration_connected.load(Ordering::Relaxed);
                    let events = self
                        .app_state
                        .vibration_events_received
                        .load(Ordering::Relaxed);
                    let (ctext, ccolor) = if connected {
                        (
                            format!("● 已连接游戏 (累计 {} 事件)", events),
                            th.good,
                        )
                    } else {
                        (
                            "○ 未连接 (请先启动游戏)".to_string(),
                            th.info,
                        )
                    };
                    ui.label(egui::RichText::new(ctext).size(13.0).color(ccolor).strong());
                }
                });
                ui.add_space(10.0);
                ui.separator();
                ui.add_space(8.0);

                /* 召唤专属: 绝对震动频率 (v26.3, 置顶显示在马达区上方, 仅召唤职业) */
                if job_mode && self.app_state.vibration_abs_freq_enabled.load(Ordering::Relaxed) {
                    Self::vib_section(ui, self.dark_mode, "【召唤专属】绝对震动频率 (置顶)", |ui| {
                        ui.label(
                            egui::RichText::new(
                                "开启后一切震动算法失效 (密度/间隔/节流/连击倍率/评分通道等),\n\
                                 只在 [时间-震动次数] 内注入, 均匀分布固定节拍。\n\
                                 全局强度/强度上限仍可控制输出, 移动独立。",
                            )
                            .size(11.0)
                            .weak(),
                        );
                        ui.add_space(4.0);
                        let mut ae = self
                            .app_state
                            .vibration_abs_freq_enabled
                            .load(Ordering::Relaxed);
                        if ui
                            .checkbox(&mut ae, "绝对震动频率 (3 秒最多 6 次)")
                            .changed()
                        {
                            self.app_state
                                .vibration_abs_freq_enabled
                                .store(ae, Ordering::Relaxed);
                            self.config.vibration.abs_freq_enabled = ae;
                            let _ = self.config.save_vibration_to_file(crate::config::AppConfig::vibration_path_for("Config.toml"));
                        }
                        let mut aw = self
                            .app_state
                            .vibration_abs_freq_window
                            .load(Ordering::Relaxed) as f32;
                        if ui
                            .add(egui::Slider::new(&mut aw, 1000.0..=10000.0).text("绝对频率窗口 ms"))
                            .changed()
                        {
                            self.app_state
                                .vibration_abs_freq_window
                                .store(aw as u32, Ordering::Relaxed);
                            self.config.vibration.abs_freq_window_ms = aw as u32;
                            let _ = self.config.save_vibration_to_file(crate::config::AppConfig::vibration_path_for("Config.toml"));
                        }
                        let mut am = self
                            .app_state
                            .vibration_abs_freq_max
                            .load(Ordering::Relaxed) as f32;
                        if ui
                            .add(egui::Slider::new(&mut am, 1.0..=20.0).text("窗口内最大震动次数"))
                            .changed()
                        {
                            self.app_state
                                .vibration_abs_freq_max
                                .store(am as u32, Ordering::Relaxed);
                            self.config.vibration.abs_freq_max = am as u32;
                            let _ = self.config.save_vibration_to_file(crate::config::AppConfig::vibration_path_for("Config.toml"));
                        }
                    });
                    ui.add_space(10.0);
                    ui.separator();
                    ui.add_space(8.0);
                }

                /* 马达区: 实时输出检测 (增益已并入各基础功能的 L/R 调控) */
                Self::vib_section(
                    ui,
                    self.dark_mode,
                    "马达区 (实时输出检测)",
                    |ui| {
                    ui.label(
                        egui::RichText::new(
                            "左右马达实时输出检测 (测试按钮或进图战斗时观察)。",
                        )
                        .size(11.0)
                        .weak(),
                    );
                    let out_l = self.app_state.vibration_out_l.load(Ordering::Relaxed);
                    let out_r = self.app_state.vibration_out_r.load(Ordering::Relaxed);
                    let pct_l = out_l as f32 / 65535.0;
                    let pct_r = out_r as f32 / 65535.0;
                    ui.add_space(2.0);
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new("左马达").strong().size(12.0).color(th.motor_l),
                        );
                        ui.add(
                            egui::ProgressBar::new(pct_l)
                                .desired_width(220.0)
                                .fill(th.motor_l)
                                .show_percentage(),
                        );
                        ui.label(
                            egui::RichText::new(format!("{:>5}", out_l))
                                .monospace()
                                .size(11.0)
                                .weak(),
                        );
                    });
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new("右马达").strong().size(12.0).color(th.motor_r),
                        );
                        ui.add(
                            egui::ProgressBar::new(pct_r)
                                .desired_width(220.0)
                                .fill(th.motor_r)
                                .show_percentage(),
                        );
                        ui.label(
                            egui::RichText::new(format!("{:>5}", out_r))
                                .monospace()
                                .size(11.0)
                                .weak(),
                        );
                    });
                });
                ui.add_space(10.0);
                ui.separator();
                ui.add_space(8.0);

                /* 全局总调整 */
                Self::vib_section(
                    ui,
                    self.dark_mode,
                    "全局总闸 (任一为 0 则全部关闭)",
                    |ui| {
                    let rows = [
                        ("全局总调整 %", 9, 100.0, "", "总开关: 0 = 全部关闭; 调低 = 整体减弱所有震动", 0usize),
                        ("攻击频率 (总闸1)", 0, 100.0, "", "攻击/技能命中反馈总闸: 0 = 命中不震", 1),
                        ("强度上限 (总闸2)", 4, 100.0, "%", "输出强度上限: 调低 = 所有震动更弱 (保护手柄)", 2),
                        ("衰减时间", 5, 500.0, "ms", "震动衰减速度: 越小越脆快, 越大越绵长 (基线)", 3),
                        ("连击增强 (渐进至上限)", 6, 100.0, "", "连击数越高震动越强, 渐进到强度上限", 4),
                        ("节奏感 (右马达比例)", 18, 100.0, "", "右马达强度占比: 50 = 均衡, 高 = 右重左轻", 5),
                    ];
                    /* 每项独立块: 名称+滑块+说明+L/R, 宽度自适应卡片, 长文本自然换行 */
                    /* 每项独立块: 名称+滑块+说明+L/R, 宽屏双列排布 */
                    let col_w = (ui.available_width() - theme::SP_L) / 2.0;
                    ui.horizontal(|ui| {
                        let half = rows.len().div_ceil(2);
                        for chunk in [&rows[..half], &rows[half..]] {
                            ui.vertical(|ui| {
                                ui.set_min_width(col_w);
                                ui.set_max_width(col_w);
                                for (name, pidx, max, suffix, hint, item) in chunk.iter().copied() {
                                    ui.label(
                                        egui::RichText::new(name)
                                            .size(13.0)
                                            .color(th.text),
                                    );
                                    let mut v = p[pidx].load(Ordering::Relaxed) as f32;
                                    if ui
                                        .add(egui::Slider::new(&mut v, 0.0..=max).suffix(suffix))
                                        .changed()
                                    {
                                        p[pidx].store(v as u32, Ordering::Relaxed);
                                    }
                                    ui.label(egui::RichText::new(hint).size(11.0).weak());
                                    Self::render_item_lr(ui, self.dark_mode, &self.app_state, &mut self.config.vibration.item_lr, item);
                                    ui.add_space(6.0);
                                }
                            });
                        }
                    });
                    /* 震动随机性 (独立参数, 不占 60 槽) */
                    let mut rg = self
                        .app_state
                        .vibration_random_gain
                        .load(Ordering::Relaxed) as f32;
                    if ui
                        .add(
                            egui::Slider::new(&mut rg, 0.0..=100.0)
                                .text("震动随机性 %"),
                        )
                        .changed()
                    {
                        self.app_state
                            .vibration_random_gain
                            .store(rg as u32, Ordering::Relaxed);
                        self.config.vibration.random_gain = rg as u32;
                        let _ = self.config.save_vibration_to_file(crate::config::AppConfig::vibration_path_for("Config.toml"));
                    }
                    ui.label(
                        egui::RichText::new(
                            "0 = 关闭; N = 强度在原本上限附近随机增减 ±N%\n\
                             (例: 5 = 每次震动有 0~5% 随机浮动, 手感更自然)",
                        )
                        .size(11.0)
                        .weak(),
                    );
                    /* 移动 (持续震动): 位于节奏感下方 (v22 布局调整, 原在评分特效卡片) */
                    ui.add_space(8.0);
                    ui.separator();
                    ui.add_space(8.0);
                    ui.vertical(|ui| {
                        /* 移动独立开关 (v24.5): 默认开, 移动一直在走, 不吃全局强度 */
                        let mut mi = self.app_state.vibration_move_independent.load(Ordering::Relaxed);
                        if ui
                            .checkbox(&mut mi, "移动持续震动独立于全局强度 (默认开)")
                            .on_hover_text(
                                "开: 移动震动不受全局总调整/强度上限影响 (移动一直在走, 吃全局强度容易直接没有震动)。\n\
                                 关: 移动震动也受全局总调整与强度上限约束。",
                            )
                            .changed()
                        {
                            self.app_state.vibration_move_independent.store(mi, Ordering::Relaxed);
                            self.config.vibration.move_independent = mi;
                            let _ = self.config.save_vibration_to_file(crate::config::AppConfig::vibration_path_for("Config.toml"));
                        }
                        ui.set_width(ui.available_width());
                        let cnt = self.app_state.vibration_rank_type_events[11].load(Ordering::Relaxed);
                        ui.label(
                            egui::RichText::new(format!("移动 (持续震动) ({})", cnt))
                                .size(13.0)
                                .color(th.text),
                        );
                        let mut g = self.app_state.vibration_rank_type_gain[11].load(Ordering::Relaxed) as f32;
                        if ui
                            .add(egui::Slider::new(&mut g, 0.0..=100.0).suffix("").text("移动持续震动强度"))
                            .changed()
                        {
                            self.app_state.vibration_rank_type_gain[11].store(g as u32, Ordering::Relaxed);
                            self.config.vibration.rank_type_gain[11] = g as u32;
                            let _ = self.config.save_vibration_to_file(crate::config::AppConfig::vibration_path_for("Config.toml"));
                        }
                        ui.label(
                            egui::RichText::new("角色移动持续震动 (0 停)。\n与高级调校 A2 移动走路质感/A3 移动积累增强配合: 长时间移动积累走位能量, 停手后窗口内攻击增强")
                                .size(11.0)
                                .weak(),
                        );
                        Self::render_rank_lr(ui, self.dark_mode, &self.app_state, &mut self.config.vibration.rank_lr, 11);
                    });
                });
                ui.add_space(10.0);
                ui.separator();
                ui.add_space(8.0);

                /* 伤害飘字 */
                Self::vib_section(
                    ui,
                    self.dark_mode,
                    "伤害飘字震动 (事件类型细分)",
                    |ui| {
                    ui.label(
                        egui::RichText::new(
                            "DLL 采集游戏伤害飘字(每次命中必经), 按需开启。",
                        )
                        .size(11.0)
                        .weak(),
                    );
                    let mut font_on = self
                        .app_state
                        .vibration_font_hits
                        .load(Ordering::Relaxed);
                    if ui
                        .checkbox(&mut font_on, "启用伤害飘字震动 (默认关闭)")
                        .changed()
                    {
                        self.app_state
                            .vibration_font_hits
                            .store(font_on, Ordering::Relaxed);
                    }
                    ui.label(
                        egui::RichText::new("关闭后所有战斗事件都不震, 仅保留测试按钮可用")
                            .size(11.0)
                            .weak(),
                    );
                    let mut fs = p[10].load(Ordering::Relaxed) as f32;
                    if ui
                        .add(egui::Slider::new(&mut fs, 0.0..=100.0).text("通用飘字强度"))
                        .changed()
                    {
                        p[10].store(fs as u32, Ordering::Relaxed);
                    }
                    ui.label(
                        egui::RichText::new(
                            "飘字类型强度的兜底强度，设置为0的时候，全部震动类型为独立可调控。",
                        )
                        .size(11.0)
                        .weak(),
                    );
                    Self::render_item_lr(ui, self.dark_mode, &self.app_state, &mut self.config.vibration.item_lr, 6);
                    let mut fi = p[11].load(Ordering::Relaxed) as f32;
                    if ui
                        .add(egui::Slider::new(&mut fi, 10.0..=500.0).text("飘字最小间隔 ms"))
                        .changed()
                    {
                        p[11].store(fi as u32, Ordering::Relaxed);
                    }
                    ui.label(
                        egui::RichText::new("同类型事件最短触发间隔, 防止高频连打过度震动")
                            .size(11.0)
                            .weak(),
                    );
                    ui.add_space(4.0);
                    ui.label(
                        egui::RichText::new("飘字类型强度 (a6 标志分类, 默认 0 = 不震, 逐个开启测试)")
                            .size(12.0)
                            .color(th.accent_text),
                    );
                    let rows = [
                        ("持续伤害 DOT 反馈 (0x20)", 12, 7usize, "中毒/灼烧/出血等持续掉血反馈, 每次掉血持续震动"),
                        ("装备特效反馈 (0x04)", 15, 8, "装备触发效果(特效等), 例如天域之类的"),
                        ("玩家状态变化反馈 (0x08)", 14, 9, "自身状态变化(增益/减益/回复)"),
                        ("特殊攻击反馈 (0x10) 暴击/破招/背击", 13, 10, "暴击/破招/背击瞬间的重击强调"),
                        ("玩家命中反馈 (0x01)", 16, 11, "普通攻击与技能每次命中的基础反馈 (核心)"),
                        ("玩家受击反馈 (0x02)", 17, 12, "被敌人击中的反馈, 数值越大被打越有感觉"),
                    ];
                    /* 每项独立块: 名称+滑块+说明+L/R, 宽屏双列排布 */
                    let col_w = (ui.available_width() - theme::SP_L) / 2.0;
                    ui.horizontal(|ui| {
                        let half = rows.len().div_ceil(2);
                        for chunk in [&rows[..half], &rows[half..]] {
                            ui.vertical(|ui| {
                                ui.set_min_width(col_w);
                                ui.set_max_width(col_w);
                                for (name, pidx, item, hint) in chunk.iter().copied() {
                                    ui.label(
                                        egui::RichText::new(name)
                                            .size(13.0)
                                            .color(th.text),
                                    );
                                    let mut v = p[pidx].load(Ordering::Relaxed) as f32;
                                    if ui
                                        .add(egui::Slider::new(&mut v, 0.0..=100.0))
                                        .changed()
                                    {
                                        p[pidx].store(v as u32, Ordering::Relaxed);
                                    }
                                    ui.label(egui::RichText::new(hint).size(11.0).weak());
                                    Self::render_item_lr(ui, self.dark_mode, &self.app_state, &mut self.config.vibration.item_lr, item);
                                    ui.add_space(6.0);
                                }
                            });
                        }
                    });
                });
                ui.add_space(10.0);
                ui.separator();
                ui.add_space(8.0);

                /* 评分特效震动 (DLL 已采集: 评分等级/击杀点/闪避/暴击/破招/背击等) */
                Self::vib_section(ui, self.dark_mode, "评分特效震动 (等级 + 细分事件)", |ui| {
                    ui.label(
                        egui::RichText::new(
                            "评分事件由 DLL 采集, 每项强度独立可调 (0 = 关闭)。\n\
                             括号内为累计触发次数, 用于区分哪个事件真正触发了。",
                        )
                        .size(11.0)
                        .weak(),
                    );
                    ui.add_space(4.0);
                    /* 状态: 最近等级 + 事件计数 */
                    let rl = self.app_state.vibration_rank_last.load(Ordering::Relaxed);
                    let re = self.app_state.vibration_rank_events.load(Ordering::Relaxed);
                    let (rtext, rcolor) = if rl >= 8 {
                        (
                            format!("★ 最近评分: 等级 {} (特殊动作!)  累计 {} 次", rl, re),
                            th.warn,
                        )
                    } else if rl >= 2 {
                        (
                            format!("● 最近评分: 等级 {}  累计 {} 次", rl, re),
                            th.info,
                        )
                    } else {
                        (
                            "○ 暂无评分事件 (进副本打怪触发)".to_string(),
                            th.hint,
                        )
                    };
                    ui.label(egui::RichText::new(rtext).size(12.0).color(rcolor).strong());
                    ui.add_space(4.0);
                    /* 评分等级强度 + 满幅时长 */
                    let mut lvl_gain = self.app_state.vibration_rank_level_gain.load(Ordering::Relaxed) as f32;
                    if ui
                        .add(egui::Slider::new(&mut lvl_gain, 0.0..=100.0).text("评分等级强度 % (等级 F~SSS)"))
                        .changed()
                    {
                        self.app_state.vibration_rank_level_gain.store(lvl_gain as u32, Ordering::Relaxed);
                        self.config.vibration.rank_level_gain = lvl_gain as u32;
                        let _ = self.config.save_vibration_to_file(crate::config::AppConfig::vibration_path_for("Config.toml"));
                    }
                    let mut rank_dur = self.app_state.vibration_rank_duration.load(Ordering::Relaxed) as f32;
                    if ui
                        .add(egui::Slider::new(&mut rank_dur, 20.0..=1000.0).text("满幅脉冲时长 ms (所有评分事件)"))
                        .changed()
                    {
                        self.app_state.vibration_rank_duration.store(rank_dur as u32, Ordering::Relaxed);
                        self.config.vibration.rank_duration = rank_dur as u32;
                        let _ = self.config.save_vibration_to_file(crate::config::AppConfig::vibration_path_for("Config.toml"));
                    }
                    ui.add_space(4.0);
                    ui.label(
                        egui::RichText::new("细分事件独立强度 (触发次数, 0 = 关闭):")
                            .size(12.0)
                            .color(th.accent_text),
                    );
                    /* 细分事件独立强度 + 触发计数 */
                    let rank_names = [
                        ("评分点", 0usize, "每次命中累积的评分点"),
                        ("极限闪避", 1, "闪避/反击成功瞬间"),
                        ("暴击", 2, "暴击命中瞬间 (未定位)"),
                        /* 破招/背击 已搁置 (2026-08-15): 客户端判定未定位, 推断服务器判定
                         * 见 docs/背击破招定位最终结论_v19.md
                         * ("破招", 3, "破招控制命中 (未定位)"),
                         * ("背击", 4, "背后攻击命中 (未定位)"), */
                        ("最终击杀", 5, "BOSS 击杀/结算 (未定位)"),
                        ("凌空追击", 6, "空中追击命中 (槽评分)"),
                        ("命中第一击", 7, "命中怪物第一击 (原\"破甲\"槽, 实测语义)"),
                        ("增益叠加", 8, "增益层数叠加 (未定位)"),
                        ("释放技能", 9, "释放技能瞬间 (n2500[5747] 计数)"),
                        /* 镜头震动/技能震动 已搁置 (2026-08-15): 见 docs/震屏事件搁置记录_v16.md
                         * ("镜头震动", 10, "真 [shake screen] 词条屏幕震动"),
                         * ("技能震动", 12, "技能释放/命中时的屏幕震动"), */
                        /* 移动 (持续震动) 已移至"全局总闸"卡片节奏感下方 (v22) */
                        ("暴击特写", 13, "暴击/击杀时的镜头特写震屏"),
                        ("怪物死亡", 14, "怪物死亡 (OnTargetDie 纯目标死亡信号)"),
                    ];
                    let col_w = (ui.available_width() - theme::SP_L) / 2.0;
                    ui.horizontal(|ui| {
                        let half = rank_names.len().div_ceil(2);
                        for chunk in [&rank_names[..half], &rank_names[half..]] {
                            ui.vertical(|ui| {
                                ui.set_min_width(col_w);
                                ui.set_max_width(col_w);
                                for (name, idx, hint) in chunk.iter().copied() {
                                /* 名称(计数) 独立标签在数值上方 (与飘字类型强度一致) */
                                let cnt = self.app_state.vibration_rank_type_events[idx].load(Ordering::Relaxed);
                                ui.label(
                                    egui::RichText::new(format!("{} ({})", name, cnt))
                                        .size(13.0)
                                        .color(th.text),
                                );
                                let mut g = self.app_state.vibration_rank_type_gain[idx].load(Ordering::Relaxed) as f32;
                                if ui
                                    .add(egui::Slider::new(&mut g, 0.0..=100.0))
                                    .changed()
                                {
                                    self.app_state.vibration_rank_type_gain[idx].store(g as u32, Ordering::Relaxed);
                                    self.config.vibration.rank_type_gain[idx] = g as u32;
                                    let _ = self.config.save_vibration_to_file(crate::config::AppConfig::vibration_path_for("Config.toml"));
                                }
                                ui.label(egui::RichText::new(hint).size(11.0).weak());
                                /* 每个评分事件的独立 L/R 马达权重 */
                                Self::render_rank_lr(ui, self.dark_mode, &self.app_state, &mut self.config.vibration.rank_lr, idx);
                                ui.add_space(6.0);
                                }
                            });
                        }
                    });
                    ui.add_space(4.0);
                    if ui.button(egui::RichText::new("测试评分震动 (模拟等级 8)").size(12.0)).clicked()
                    {
                        self.app_state.vibration_rank_test.store(true, Ordering::Relaxed);
                    }
                });
                ui.add_space(8.0);
                ui.separator();
                ui.add_space(8.0);

                /* 评分动态衰减 (类鬼泣, v29): 独立卡片, 位于评分特效震动下方 */
                Self::vib_section(ui, self.dark_mode, "评分动态衰减 (类鬼泣): 越打越猛, 停手跌分", |ui| {
                    let rd = self.app_state.vibration_rank_decay_enabled.load(Ordering::Relaxed);
                    ui.label(
                        egui::RichText::new(
                            "攻击/命中提升评级 (0-8), 停手超过延迟后逐级衰减。\n\
                             最高评级反馈 = 当前预设参数 ×120%; 最低档保底 60% (不失去基础反馈)。",
                        )
                        .size(11.0)
                        .weak(),
                    );
                    ui.add_space(4.0);
                    let mut rd_on = rd;
                    if ui
                        .checkbox(&mut rd_on, "启用评分动态衰减 (类鬼泣)")
                        .on_hover_text(
                            "攻击/命中提升评级 (0-8), 停手超过延迟后逐级衰减。\n\
                             最高评级反馈 = 当前预设参数 ×1.2; 最低档保底 0.6 (不失去基础反馈)。",
                        )
                        .changed()
                    {
                        self.app_state
                            .vibration_rank_decay_enabled
                            .store(rd_on, Ordering::Relaxed);
                        self.config.vibration.rank_decay_enabled = rd_on;
                        let _ = self.config.save_vibration_to_file(crate::config::AppConfig::vibration_path_for("Config.toml"));
                    }
                    let mut rdd = self
                        .app_state
                        .vibration_rank_decay_delay
                        .load(Ordering::Relaxed) as f32;
                    if ui
                        .add_enabled(
                            rd_on,
                            egui::Slider::new(&mut rdd, 500.0..=5000.0)
                                .text("停手衰减延迟 ms"),
                        )
                        .changed()
                    {
                        self.app_state
                            .vibration_rank_decay_delay
                            .store(rdd as u32, Ordering::Relaxed);
                        self.config.vibration.rank_decay_delay_ms = rdd as u32;
                        let _ = self.config.save_vibration_to_file(crate::config::AppConfig::vibration_path_for("Config.toml"));
                    }
                    let mut rds = self
                        .app_state
                        .vibration_rank_decay_speed
                        .load(Ordering::Relaxed) as f32;
                    if ui
                        .add_enabled(
                            rd_on,
                            egui::Slider::new(&mut rds, 1.0..=5.0)
                                .text("衰减速度 级/秒"),
                        )
                        .changed()
                    {
                        self.app_state
                            .vibration_rank_decay_speed
                            .store(rds as u32, Ordering::Relaxed);
                        self.config.vibration.rank_decay_speed = rds as u32;
                        let _ = self.config.save_vibration_to_file(crate::config::AppConfig::vibration_path_for("Config.toml"));
                    }
                    let mut rdmin = self
                        .app_state
                        .vibration_rank_decay_min
                        .load(Ordering::Relaxed) as f32;
                    if ui
                        .add_enabled(
                            rd_on,
                            egui::Slider::new(&mut rdmin, 40.0..=100.0)
                                .text("最低反馈倍率 % (评级 0, 保底)"),
                        )
                        .changed()
                    {
                        self.app_state
                            .vibration_rank_decay_min
                            .store(rdmin as u32, Ordering::Relaxed);
                        self.config.vibration.rank_decay_min_mul = rdmin as u32;
                        let _ = self.config.save_vibration_to_file(crate::config::AppConfig::vibration_path_for("Config.toml"));
                    }
                    let mut rdmax = self
                        .app_state
                        .vibration_rank_decay_max
                        .load(Ordering::Relaxed) as f32;
                    if ui
                        .add_enabled(
                            rd_on,
                            egui::Slider::new(&mut rdmax, 100.0..=200.0)
                                .text("最高反馈倍率 % (满评分, 预设×此值)"),
                        )
                        .changed()
                    {
                        self.app_state
                            .vibration_rank_decay_max
                            .store(rdmax as u32, Ordering::Relaxed);
                        self.config.vibration.rank_decay_max_mul = rdmax as u32;
                        let _ = self.config.save_vibration_to_file(crate::config::AppConfig::vibration_path_for("Config.toml"));
                    }
                });
                ui.add_space(8.0);
                ui.separator();
                ui.add_space(8.0);

/* 高级震动调校 (六类 L/R 比例已并入基础区每项权重, 此处不再重复) */
                Self::vib_section(
                    ui,
                    self.dark_mode,
                    "高级震动调校 (精细微调, 玩家可选)",
                    |ui| {
                egui::CollapsingHeader::new(
                    egui::RichText::new("展开高级参数分区 (波形/衰减/窗口/连击/脉冲)")
                        .size(12.0)
                        .strong(),
                )
                .default_open(false)
                .show(ui, |ui| {
                    let mut adv_on = if job_mode {
                        self.vib_job_adv_on
                    } else {
                        self.app_state
                            .vibration_advanced_enabled
                            .load(Ordering::Relaxed)
                    };
                    ui.horizontal(|ui| {
                        if ui
                            .checkbox(&mut adv_on, "允许高级调校")
                            .on_hover_text(
                                "默认关闭: 只用基础分区即可, 高级参数保持当前配置值。\n\
                                 开启后可在下方精细调整 L/R 马达比例/衰减/窗口/连击/脉冲。",
                            )
                            .changed()
                        {
                            if job_mode {
                                self.vib_job_adv_on = adv_on;
                            } else {
                                self.app_state
                                    .vibration_advanced_enabled
                                    .store(adv_on, Ordering::Relaxed);
                                self.config.vibration.advanced_enabled = adv_on;
                                let _ = self.config.save_vibration_to_file(crate::config::AppConfig::vibration_path_for("Config.toml"));
                            }
                        }
                        ui.label(
                            egui::RichText::new(
                                if adv_on {
                                    "已开启: 可精细调整下方参数"
                                } else {
                                    "已关闭: 高级参数保持当前配置 (默认)"
                                },
                            )
                            .size(11.0)
                            .color(if adv_on { th.good } else { th.hint }),
                        );
                    });
                    ui.add_space(4.0);

                    if !adv_on {
                        ui.label(
                            egui::RichText::new(
                                "玩家通常只用基础分区即可获得良好手感。\n\
                                 高级调校基于当前配置做精细化微调, 关闭时数值保持不变。",
                            )
                            .size(11.0)
                            .weak(),
                        );
                        ui.add_space(4.0);
                    }

                    /* A 波形 */
                    ui.label(
                        egui::RichText::new("A. 输出波形曲线 (数值越大越饱和, 100 = 线性)")
                            .size(12.0)
                            .strong(),
                    );
                    let a_rows = [
                        ("左马达曲线 %", 19, 30.0, 300.0),
                        ("右马达曲线 %", 20, 30.0, 300.0),
                    ];
                    for (name, idx, lo, hi) in a_rows {
                        let mut v = p[idx].load(Ordering::Relaxed) as f32;
                        if ui
                            .add_enabled(adv_on, egui::Slider::new(&mut v, lo..=hi).text(name))
                            .changed()
                        {
                            p[idx].store(v as u32, Ordering::Relaxed);
                        }
                    }
                    /* 输出平滑 (v22.3: 独立参数, 一阶低通抑制低频嗡嗡声) */
                    {
                        let mut v = self
                            .app_state
                            .vibration_out_smooth
                            .load(Ordering::Relaxed) as f32;
                        if ui
                            .add_enabled(
                                adv_on,
                                egui::Slider::new(&mut v, 0.0..=100.0)
                                    .text("输出平滑 % (抑制嗡嗡声, 高=柔)"),
                            )
                            .changed()
                        {
                            self.app_state
                                .vibration_out_smooth
                                .store(v as u32, Ordering::Relaxed);
                            self.config.vibration.out_smooth = v as u32;
                            let _ = self.config.save_vibration_to_file(crate::config::AppConfig::vibration_path_for("Config.toml"));
                        }
                    }
                    /* 输出低强度死区 (v29.4: ERM 转子马达启动区高死区, 低于归 0 消除转子嗡声) */
                    {
                        let mut v = self
                            .app_state
                            .vibration_out_threshold
                            .load(Ordering::Relaxed) as f32;
                        if ui
                            .add_enabled(
                                adv_on,
                                egui::Slider::new(&mut v, 0.0..=60.0)
                                    .text("输出低强度抑制 % (转子马达建议 25-35)"),
                            )
                            .changed()
                        {
                            self.app_state
                                .vibration_out_threshold
                                .store(v as u32, Ordering::Relaxed);
                            self.config.vibration.out_threshold = v as u32;
                            let _ = self.config.save_vibration_to_file(crate::config::AppConfig::vibration_path_for("Config.toml"));
                        }
                    }
                    /* 输出动态范围重映射 (v30, ERM/Xbox360: 非零必转, 轻反馈不被死区吞掉) */
                    {
                        let mut rm = self
                            .app_state
                            .vibration_remap_enabled
                            .load(Ordering::Relaxed);
                        if ui
                            .checkbox(&mut rm, "输出动态范围重映射 (Xbox360/ERM)")
                            .on_hover_text(
                                "把非零输出映射到 [最小输出, 100%] 区间:\n\
                                 任何非零反馈至少以最小输出驱动马达 (转子一定转起来),\n\
                                 相对强弱保留, 轻反馈不再被死区吞掉。",
                            )
                            .changed()
                        {
                            self.app_state
                                .vibration_remap_enabled
                                .store(rm, Ordering::Relaxed);
                            self.config.vibration.remap_enabled = rm;
                            let _ = self.config.save_vibration_to_file(crate::config::AppConfig::vibration_path_for("Config.toml"));
                        }
                        let mut rmn = self
                            .app_state
                            .vibration_remap_min
                            .load(Ordering::Relaxed) as f32;
                        if ui
                            .add_enabled(
                                adv_on && rm,
                                egui::Slider::new(&mut rmn, 10.0..=60.0)
                                    .text("重映射最小输出 % (与死区一致)"),
                            )
                            .changed()
                        {
                            self.app_state
                                .vibration_remap_min
                                .store(rmn as u32, Ordering::Relaxed);
                            self.config.vibration.remap_min = rmn as u32;
                            let _ = self.config.save_vibration_to_file(crate::config::AppConfig::vibration_path_for("Config.toml"));
                        }
                    }
                    /* 马达分工 (v30, Xbox360 风格: 轻反馈单马达, 重反馈双马达) */
                    {
                        let mut sp = self
                            .app_state
                            .vibration_split_enabled
                            .load(Ordering::Relaxed);
                        if ui
                            .checkbox(&mut sp, "马达分工 (Xbox360 风格)")
                            .on_hover_text(
                                "轻反馈 (峰值低于分界) 仅驱动主导马达 (转子声/功耗更低);\n\
                                 重反馈双马达满幅 (大马达低频重击 + 小马达高频细节)。",
                            )
                            .changed()
                        {
                            self.app_state
                                .vibration_split_enabled
                                .store(sp, Ordering::Relaxed);
                            self.config.vibration.split_enabled = sp;
                            let _ = self.config.save_vibration_to_file(crate::config::AppConfig::vibration_path_for("Config.toml"));
                        }
                        let mut spt = self
                            .app_state
                            .vibration_split_thr
                            .load(Ordering::Relaxed) as f32;
                        if ui
                            .add_enabled(
                                adv_on && sp,
                                egui::Slider::new(&mut spt, 20.0..=90.0)
                                    .text("分工分界 % (低于为轻反馈)"),
                            )
                            .changed()
                        {
                            self.app_state
                                .vibration_split_thr
                                .store(spt as u32, Ordering::Relaxed);
                            self.config.vibration.split_thr = spt as u32;
                            let _ = self.config.save_vibration_to_file(crate::config::AppConfig::vibration_path_for("Config.toml"));
                        }
                    }
                    /* 震动节流 (v24: 限定时间窗口内最大注入次数, 防狂震) */
                    {
                        if job_mode
                            && self.app_state.vibration_throttle_window.load(Ordering::Relaxed) > 0
                        {
                            ui.label(
                                egui::RichText::new("【当前职业专属】狂震节流: 窗口内只震几次, 杜绝狂震")
                                    .size(11.0)
                                    .color(th.accent_text)
                                    .strong(),
                            );
                        }
                        let mut tw = self
                            .app_state
                            .vibration_throttle_window
                            .load(Ordering::Relaxed) as f32;
                        if ui
                            .add_enabled(
                                adv_on,
                                egui::Slider::new(&mut tw, 0.0..=10000.0)
                                    .text("震动节流窗口 ms (0=禁用)"),
                            )
                            .changed()
                        {
                            self.app_state
                                .vibration_throttle_window
                                .store(tw as u32, Ordering::Relaxed);
                            self.config.vibration.throttle_window_ms = tw as u32;
                            let _ = self.config.save_vibration_to_file(crate::config::AppConfig::vibration_path_for("Config.toml"));
                        }
                        let mut tm = self
                            .app_state
                            .vibration_throttle_max
                            .load(Ordering::Relaxed) as f32;
                        if ui
                            .add_enabled(
                                adv_on,
                                egui::Slider::new(&mut tm, 1.0..=30.0)
                                    .text("窗口内最大震动次数"),
                            )
                            .changed()
                        {
                            self.app_state
                                .vibration_throttle_max
                                .store(tm as u32, Ordering::Relaxed);
                            self.config.vibration.throttle_max_hits = tm as u32;
                            let _ = self.config.save_vibration_to_file(crate::config::AppConfig::vibration_path_for("Config.toml"));
                        }
                        let mut tr = self
                            .app_state
                            .vibration_throttle_dense_ratio
                            .load(Ordering::Relaxed) as f32;
                        if ui
                            .add_enabled(
                                adv_on,
                                egui::Slider::new(&mut tr, 1.0..=100.0)
                                    .text("狂震期次数比例 % (自适应收紧, 100=不收紧)"),
                            )
                            .changed()
                        {
                            self.app_state
                                .vibration_throttle_dense_ratio
                                .store(tr as u32, Ordering::Relaxed);
                            self.config.vibration.throttle_dense_ratio = tr as u32;
                            let _ = self.config.save_vibration_to_file(crate::config::AppConfig::vibration_path_for("Config.toml"));
                        }
                    }
                    ui.add_space(4.0);
                    ui.separator();
                    ui.add_space(4.0);

                    /* A2 移动走路质感 (v20.2: 轻量步伐参数化) */
                    ui.label(
                        egui::RichText::new("A2. 移动走路质感 (轻量步伐, 不抢主震动)")
                            .size(12.0)
                            .strong(),
                    );
                    let m_rows = [
                        ("移动步频 ms (自然步频 ~380)", 23, 200.0, 800.0),
                        ("移动着地脉冲 % (柔和, 不宜高)", 24, 10.0, 100.0),
                        ("移动抬脚保持 % (极轻)", 25, 5.0, 50.0),
                        ("移动整体增益 % (轻音量)", 26, 10.0, 100.0),
                        ("移动平滑系数 % (越大过渡越柔, 消除嗡嗡声)", 27, 5.0, 100.0),
                        ("移动最低输出阈值 % (低于归0, 消除沙沙声)", 28, 0.0, 20.0),
                    ];
                    for (name, idx, lo, hi) in m_rows {
                        let mut v = p[idx].load(Ordering::Relaxed) as f32;
                        if ui
                            .add_enabled(adv_on, egui::Slider::new(&mut v, lo..=hi).text(name))
                            .changed()
                        {
                            p[idx].store(v as u32, Ordering::Relaxed);
                        }
                    }
                    ui.add_space(4.0);
                    ui.separator();
                    ui.add_space(4.0);

                    /* A3 移动积累增强 (v22): 长时间移动积累走位能量, 停手后窗口内攻击增强
                     * 走位型职业 (漫游/剑魂/刺客/影舞/决战者) 强化; 站桩职业弱化 */
                    ui.label(
                        egui::RichText::new("A3. 移动积累增强 (走位能量 → 下次攻击增强)")
                            .size(12.0)
                            .strong(),
                    );
                    if job_mode {
                        let m_rate = p[21].load(Ordering::Relaxed);
                        if m_rate >= 12 {
                            ui.label(
                                egui::RichText::new("【走位职业】移动积累快, 停手窗口内攻击增强明显")
                                    .size(11.0)
                                    .color(th.accent_text)
                                    .strong(),
                            );
                        } else if m_rate <= 3 {
                            ui.label(
                                egui::RichText::new("【站桩职业】移动积累慢, 增强弱")
                                    .size(11.0)
                                    .weak(),
                            );
                        }
                    }
                    let c_rows2 = [
                        ("移动积累速率 %/秒 (0=禁用)", 21, 0.0, 20.0),
                        ("攻击增强上限 % (最多 ×(1+上限))", 22, 0.0, 100.0),
                        ("增强窗口 ms (停手后有效)", 39, 0.0, 3000.0),
                    ];
                    for (name, idx, lo, hi) in c_rows2 {
                        let mut v = p[idx].load(Ordering::Relaxed) as f32;
                        if ui
                            .add_enabled(adv_on, egui::Slider::new(&mut v, lo..=hi).text(name))
                            .changed()
                        {
                            p[idx].store(v as u32, Ordering::Relaxed);
                        }
                    }
                    ui.add_space(4.0);
                    ui.separator();
                    ui.add_space(4.0);

                    /* A3 连击密度自适应 (v22): 短时间连击暴增窗口期自动降强度
                     * 高连击职业专属算法 (召唤/精灵骑士/剑魂/蓝拳等), 全职业页仅该职业显示 */
                    let d_thr = p[29].load(Ordering::Relaxed);
                    if job_mode && d_thr >= 100 {
                        ui.label(
                            egui::RichText::new("A4. 连击密度自适应: 该职业无此专属算法 (仅高连击职业启用)")
                                .size(11.0)
                                .weak(),
                        );
                    } else {
                        ui.label(
                            egui::RichText::new("A4. 连击密度自适应 (短时间连击暴增窗口期自动降强度, 防震手)")
                                .size(12.0)
                                .strong(),
                        );
                        if job_mode {
                            ui.label(
                                egui::RichText::new("【当前职业专属】高连击职业窗口期自适应降强度")
                                    .size(11.0)
                                    .color(th.accent_text)
                                    .strong(),
                            );
                        }
                        let d_rows = [
                            ("密度触发阈值 hits/窗口 (100=禁用)", 29, 5.0, 100.0),
                            ("密度检测窗口 ms", 30, 200.0, 1500.0),
                            ("窗口期降幅 % (强度×降幅)", 31, 0.0, 80.0),
                            ("恢复判定 ms (停手后恢复)", 32, 300.0, 3000.0),
                            ("最低保留 % (降幅上限)", 33, 30.0, 100.0),
                            ("恢复平滑 ms (0=立即)", 34, 0.0, 800.0),
                        ];
                        for (name, idx, lo, hi) in d_rows {
                            let mut v = p[idx].load(Ordering::Relaxed) as f32;
                            if ui
                                .add_enabled(adv_on, egui::Slider::new(&mut v, lo..=hi).text(name))
                                .changed()
                            {
                                p[idx].store(v as u32, Ordering::Relaxed);
                            }
                        }
                    }
                    ui.add_space(4.0);
                    ui.separator();
                    ui.add_space(4.0);

                    /* B 衰减 (原 C, 六类 L/R 比例已移至基础区每项权重) */
                    ui.label(
                        egui::RichText::new("B. 各类事件衰减时长 (越大震感越持久)")
                            .size(12.0)
                            .strong(),
                    );
                    let c_rows = [
                        ("普通命中衰减 ms", 35, 15.0, 150.0),
                        ("特殊攻击衰减 ms", 36, 15.0, 150.0),
                        ("受击衰减 ms", 37, 10.0, 150.0),
                        ("状态变化衰减 ms", 38, 15.0, 150.0),
                        ("装备特效衰减 ms", 39, 15.0, 200.0),
                    ];
                    for (name, idx, lo, hi) in c_rows {
                        let mut v = p[idx].load(Ordering::Relaxed) as f32;
                        if ui
                            .add_enabled(adv_on, egui::Slider::new(&mut v, lo..=hi).text(name))
                            .changed()
                        {
                            p[idx].store(v as u32, Ordering::Relaxed);
                        }
                    }
                    ui.add_space(4.0);
                    ui.separator();
                    ui.add_space(4.0);

                    /* D 时长窗口 */
                    ui.label(
                        egui::RichText::new("C. 持续/节奏/爆发/连击窗口时长")
                            .size(12.0)
                            .strong(),
                    );
                    let d_rows = [
                        ("DOT 持续反馈时长 ms", 40, 50.0, 1000.0),
                        ("装备特效节奏周期 ms", 41, 60.0, 600.0),
                        ("爆发窗口 ms (窗口内 6 连触发爆发)", 42, 100.0, 1000.0),
                        ("爆发模式最低强度 %", 43, 0.0, 100.0),
                        ("反击窗口 ms (受击后攻击强化)", 44, 300.0, 2000.0),
                        ("连击统计窗口 ms", 45, 500.0, 5000.0),
                        ("空闲判定 ms (回战斗前状态)", 46, 1000.0, 10000.0),
                    ];
                    for (name, idx, lo, hi) in d_rows {
                        let mut v = p[idx].load(Ordering::Relaxed) as f32;
                        if ui
                            .add_enabled(adv_on, egui::Slider::new(&mut v, lo..=hi).text(name))
                            .changed()
                        {
                            p[idx].store(v as u32, Ordering::Relaxed);
                        }
                    }
                    ui.add_space(4.0);
                    ui.separator();
                    ui.add_space(4.0);

                    /* E 连击/自适应 */
                    ui.label(
                        egui::RichText::new("D. 连击增强与命中自适应 (防叠加饱和)")
                            .size(12.0)
                            .strong(),
                    );
                    let e_rows = [
                        ("连击放大上限 x", 47, 100.0, 500.0),
                        ("连击增强斜率 %/百连", 48, 1.0, 300.0),
                        ("中断收尾阈值 连击数", 49, 10.0, 100.0),
                        ("自适应阈值 ms (低于此间隔开始减弱)", 50, 30.0, 300.0),
                        ("自适应降幅 %", 51, 0.0, 50.0),
                        ("自适应上限间隔 ms (超过则满强度)", 52, 100.0, 1000.0),
                        ("自适应最低强度 %", 53, 0.0, 100.0),
                    ];
                    for (name, idx, lo, hi) in e_rows {
                        let mut v = p[idx].load(Ordering::Relaxed) as f32;
                        if ui
                            .add_enabled(adv_on, egui::Slider::new(&mut v, lo..=hi).text(name))
                            .changed()
                        {
                            p[idx].store(v as u32, Ordering::Relaxed);
                        }
                    }
                    ui.add_space(4.0);
                    ui.separator();
                    ui.add_space(4.0);

                    /* F/G/H 脉冲与静默 */
                    ui.label(
                        egui::RichText::new("E. 静默/反击/脉冲/测试")
                            .size(12.0)
                            .strong(),
                    );
                    let f_rows = [
                        ("特殊攻击后静默 ms (突显暴击破招)", 54, 0.0, 500.0),
                        ("反击强化倍数 %", 55, 100.0, 300.0),
                        ("唤醒脉冲强度 %", 56, 0.0, 100.0),
                        ("里程碑脉冲强度 %", 57, 0.0, 100.0),
                        ("连击中断收尾脉冲 %", 58, 0.0, 100.0),
                        ("测试震动强度 %", 59, 1.0, 100.0),
                    ];
                    for (name, idx, lo, hi) in f_rows {
                        let mut v = p[idx].load(Ordering::Relaxed) as f32;
                        if ui
                            .add_enabled(adv_on, egui::Slider::new(&mut v, lo..=hi).text(name))
                            .changed()
                        {
                            p[idx].store(v as u32, Ordering::Relaxed);
                        }
                    }
                    ui.label(
                        egui::RichText::new(
                            "里程碑档位固定 50/100/200/400 连击。\n\
                             自适应: 低于阈值间隔的连续命中按降幅减弱, 防止叠加饱和。",
                        )
                        .size(11.0)
                        .weak(),
                    );
                });
                });
                ui.add_space(8.0);
                ui.separator();
                ui.add_space(8.0);

                
                ui.label(
                    egui::RichText::new(if job_mode {
                        "提示: 所有滑块实时生效; 全职业预设页修改会自动保存到 JobVibration.toml"
                    } else {
                        "提示: 所有滑块实时生效, 无需保存。连发功能在\"连发映射\"页。"
                    })
                        .size(11.0)
                        .weak(),
                );
            });
            /* 全职业预设页: 滑块修改后自动落盘 (快照检测) */
            if job_mode {
                if let Some((b, c)) = self.vib_job_active.clone() {
                    let changed = self.vib_job_snapshot.len() != 60
                        || (0..60).any(|i| {
                            self.app_state.vibration_params[i].load(Ordering::Relaxed) != self.vib_job_snapshot[i]
                        });
                    if changed {
                        self.vib_job_snapshot = (0..60)
                            .map(|i| self.app_state.vibration_params[i].load(Ordering::Relaxed))
                            .collect();
                        Self::save_job_vibration_state(&self.app_state, &b, &c);
                    }
                }
            }
    }

    /// 应用外壳: 顶栏 + 侧边栏导航 + 内容区 (现代桌面应用三段式)。
    fn render_shell(&mut self, ctx: &egui::Context, frame_state: &FrameState) {
        let th = self.theme();

        /* 首次运行: 用户指引浮窗 (浮层, 不占布局) */
        if !self.config.guide_seen {
            self.render_guide_window(ctx);
        }

        /* 极简模式: 小窗 + 顶栏(返回/主题/预设) + 两个总开关 */
        if self.minimal_mode {
            // 进入极简时自动缩窗 (记录原尺寸/位置, 退出恢复; 100% 小窗兜底)
            if self.normal_window_size.is_none() {
                self.normal_was_maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
                /* 只在大窗 (>600px) 时记录原尺寸: 防止重复切换把小窗尺寸记成"原尺寸" */
                if let Some(inner) = ctx.input(|i| i.viewport().inner_rect) {
                    if inner.width() > 600.0 {
                        self.normal_window_size = Some(inner.size());
                        /* ViewportInfo 的 rect 已是逻辑坐标, 无需 DPI 换算 */
                        if let Some(outer) = ctx.input(|i| i.viewport().outer_rect) {
                            let pos = outer.min;
                            let sz = inner.size();
                            self.normal_window_rect = Some((pos, sz));
                            self.config.window_rect_normal = Some([pos.x, pos.y, sz.x, sz.y]);
                            let _ = self.config.save_to_file("Config.toml");
                        }
                    }
                }
                if self.normal_was_maximized {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(false));
                }
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(430.0, 470.0)));
                /* 极简窗回到上次的位置 (运行时记忆 → 配置持久化记忆) + 位置锁存 */
                let mini = self
                    .minimal_window_rect
                    .or_else(|| {
                        self.config
                            .window_rect_minimal
                            .map(|r| (egui::pos2(r[0], r[1]), egui::vec2(r[2], r[3])))
                    });
                if let Some((pos, _)) = mini {
                    ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(pos));
                    self.minimal_pos_latch = 12;
                }
            } else {
                /* 每帧校验小窗尺寸: 有概率丢 InnerSize 命令时兜底, 确保百分百小窗 */
                let ppp = ctx.pixels_per_point();
                let vp = ctx.input(|i| (i.viewport().outer_rect, i.viewport().inner_rect));
                if let (Some(outer), Some(inner)) = vp {
                    let sz = inner.size();
                    if (sz.x - 430.0).abs() > 1.5 || (sz.y - 470.0).abs() > 1.5 {
                        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(430.0, 470.0)));
                    }
                    self.minimal_window_rect = Some((outer.min, sz));
                    /* 位置锁存: 与 InnerSize 竞争丢失时持续校正 (最多 12 帧) */
                    if self.minimal_pos_latch > 0 {
                        self.minimal_pos_latch -= 1;
                        if let Some((pos, _)) = self.minimal_window_rect {
                            if (outer.min.x - pos.x).abs() > 2.0 || (outer.min.y - pos.y).abs() > 2.0 {
                                ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(pos));
                            }
                        }
                    }
                }
            }
            egui::TopBottomPanel::top("top_bar")
                .frame(
                    egui::Frame::NONE
                        .fill(th.surface)
                        .inner_margin(egui::Margin::symmetric(10, 9)),
                )
                .show_separator_line(false)
                .show(ctx, |ui| {
                    self.render_minimal_top_bar(ui, ctx);
                });
            egui::CentralPanel::default()
                .frame(
                    egui::Frame::central_panel(&ctx.style())
                        .fill(th.bg)
                        .inner_margin(egui::Margin::same(16)),
                )
                .show(ctx, |ui| {
                    widgets::paint_top_glow(ui, &th);
                    egui::ScrollArea::vertical()
                        .auto_shrink([false; 2])
                        .show(ui, |ui| {
                            self.render_minimal_page(ui, frame_state);
                        });
                });
            return;
        }

        /* 顶栏: 品牌 + 预设 + 图标按钮组 */
        egui::TopBottomPanel::top("top_bar")
            .frame(
                egui::Frame::NONE
                    .fill(th.surface)
                    .inner_margin(egui::Margin::symmetric(14, 9)),
            )
            .show_separator_line(false)
            .show(ctx, |ui| {
                self.render_top_bar(ui, ctx, frame_state);
            });

        /* 侧边栏: 页面导航 + 底部全局状态 */
        egui::SidePanel::left("nav_rail")
            .exact_width(188.0)
            .resizable(false)
            .frame(
                egui::Frame::NONE
                    .fill(th.surface)
                    .inner_margin(egui::Margin::same(10)),
            )
            .show_separator_line(false)
            .show(ctx, |ui| {
                self.render_nav_rail(ui, frame_state);
            });

        /* 内容区: 每页独立滚动 */
        egui::CentralPanel::default()
            .frame(
                egui::Frame::central_panel(&ctx.style())
                    .fill(th.bg)
                    .inner_margin(egui::Margin::same(16)),
            )
            .show(ctx, |ui| {
                widgets::paint_top_glow(ui, &th);
                widgets::page_header(ui, &th, self.active_page.title(), self.active_page.hint());
                match self.active_page {
                    Page::Gamepad => {
                        egui::ScrollArea::vertical()
                            .auto_shrink([false; 2])
                            .show(ui, |ui| {
                                self.render_gamepad_page(ui, ctx);
                                ui.add_space(12.0);
                            });
                    }
                    Page::Turbo => {
                        egui::ScrollArea::vertical()
                            .auto_shrink([false; 2])
                            .show(ui, |ui| {
                                self.render_turbo_page(ui, ctx, frame_state);
                                ui.add_space(16.0);
                            });
                    }
                    Page::Vibration => {
                        egui::ScrollArea::vertical()
                            .auto_shrink([false; 2])
                            .show(ui, |ui| {
                                self.render_vibration_full_page(ui, frame_state, false);
                                ui.add_space(12.0);
                            });
                    }
                    Page::JobPresets => {
                        egui::ScrollArea::vertical()
                            .auto_shrink([false; 2])
                            .show(ui, |ui| {
                                self.render_vibration_full_page(ui, frame_state, true);
                                ui.add_space(12.0);
                            });
                    }
                }
            });
    }


    /// 预设下拉 (垂直居中版): ComboBox 内部嵌 18px 条带且 combo 在条带内顶部锚定,
    /// 直接放进居中的行会整体下沉 ~3.6px。用与 combo 等高的显式矩形包裹
    /// (高度跨帧记忆, 首帧默认 25.2), 使其最终中心与行中心重合。
    fn render_preset_switch_centered(&mut self, ui: &mut egui::Ui) -> egui::Response {
        let combo_h = self.preset_combo_h.unwrap_or(25.2);
        // x: RTL + main Min → 贴当前光标右缘 (正常流式排布);
        // y: 全部 Min — 包裹块被父布局放置后, 内嵌 18px 条带居中于块内,
        //    combo 顶部锚定于条带 → 实测 combo 中心收敛于栏内容中线 (对 combo_h 微差鲁棒)
        let out = ui
            .allocate_ui_with_layout(
                egui::vec2(130.0, combo_h),
                egui::Layout::right_to_left(egui::Align::Min).with_main_align(egui::Align::Min),
                |ui| self.render_preset_switch(ui),
            )
            .response;
        self.preset_combo_h = Some(out.rect.height());
        out
    }

    /// 顶栏: 品牌 | 预设快切 …… 图标按钮组 (设置/设备/关于/主题)。
    fn render_top_bar(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, _frame_state: &FrameState) {
        let app_title = self.translations.app_title().to_owned();
        let page_label = self.active_page.label();
        let th = self.theme();

        // 拖动区必须先于按钮注册: egui hit-test 平局取"后注册者"(最上层)胜,
        // 若后注册会盖住按钮导致全部点不了 (极简顶栏同理)
        self.render_title_bar_drag(ui, ctx);

        // 品牌: 强调色方块 + 名称
        // horizontal_centered: 占满整条顶栏高度再垂直居中 —— ui.horizontal 只有
        // interact_size.y (18px) 的条带且顶在内容区顶部, 各控件会以不同基准居中
        ui.horizontal_centered(|ui| {
            ui.add_space(2.0);
            let (rect, _) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
            ui.painter()
                .rect_filled(rect, 5, th.accent);
            // 标题与页面标签合并为单一 galley: 共享一条基线,
            // 标题用真粗体族, 消除不同字号/字体各自 rect 居中导致的视觉错位
            let mut title_job = egui::text::LayoutJob::default();
            title_job.append(
                &app_title,
                0.0,
                egui::TextFormat::simple(
                    egui::FontId::new(15.0, Theme::font_bold()),
                    th.title,
                ),
            );
            title_job.append(
                &page_label,
                10.0,
                egui::TextFormat::simple(egui::FontId::proportional(12.0), th.hint),
            );
            let title_galley = ui.painter().layout_job(title_job);
            let (galley_rect, _) =
                ui.allocate_exact_size(title_galley.size(), egui::Sense::hover());
            ui.painter()
                .galley(galley_rect.left_top(), title_galley, th.title);

            // 预设快切 (右对齐: 按钮组最右, 预设在其左)
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                self.render_window_controls(ui, ctx);
                ui.add_space(theme::SP_S);
                self.render_top_bar_actions(ui, ctx);
                if !self.config.presets.is_empty() {
                    self.render_preset_switch_centered(ui);
                }
            });
        });
    }

    /// 顶栏右侧图标按钮组。
    fn render_top_bar_actions(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let minimal_tip = "极简模式: 只保留连发/震动两个开关的小窗";
        let theme_tip = self.translations.light_theme().to_owned();
        let th = self.theme();

        // 主题切换 (半月图标)
        if widgets::icon_button(ui, &th, widgets::Icon::Theme, &theme_tip).clicked() {
            self.dark_mode = !self.dark_mode;
            self.config.dark_mode = self.dark_mode;
            let _ = self.config.save_to_file("Config.toml");
            if let Some(temp_config) = &mut self.temp_config {
                temp_config.dark_mode = self.dark_mode;
            }
        }
        if widgets::icon_button(ui, &th, widgets::Icon::Info, &self.translations.about_button().to_owned()).clicked() {
            self.show_about_dialog = true;
        }
        if widgets::icon_button(ui, &th, widgets::Icon::Devices, &self.translations.devices_button().to_owned()).clicked() {
            self.show_device_manager = true;
        }

        // 极简模式入口: 文字按钮 (图标语义不清晰)
        if ui
            .add(
                egui::Button::new(egui::RichText::new("简").size(14.0).strong().color(th.btn_secondary_text))
                    .fill(th.btn_secondary)
                    .corner_radius(egui::CornerRadius::same(theme::RADIUS_CTRL))
                    .min_size(egui::vec2(30.0, 30.0)),
            )
            .on_hover_text(minimal_tip)
            .clicked()
        {
            self.set_minimal_mode(ctx, true);
        }

        // 设置 = 主操作 (强调色填充圆角按钮)
        let settings_btn = egui::Button::new(
            egui::RichText::new(format!(
                "{} {}",
                egui_phosphor::variants::regular::GEAR_SIX,
                self.translations.settings_button()
            ))
                .size(12.5)
                .strong()
                .color(egui::Color32::WHITE),
        )
        .fill(th.btn_primary)
        .corner_radius(egui::CornerRadius::same(theme::RADIUS_CTRL))
        .min_size(egui::vec2(0.0, 30.0));
        let settings_resp = ui.add(settings_btn);
        if settings_resp.clicked() {
            let was_paused = self.app_state.is_paused();
            self.was_paused_before_settings = Some(was_paused);
            if !was_paused {
                self.app_state.set_paused(true);
            }
            self.show_settings_dialog = true;
            self.temp_config = Some(self.config.clone());
            self.preset_rename_target.clear();
            self.preset_rename_input.clear();
        }
    }

    /// 预设快切下拉 (切换即保存并热重载; 完整顶栏与极简页面共用)。
    fn render_preset_switch(&mut self, ui: &mut egui::Ui) -> egui::Response {
        let no_preset = self.translations.no_preset().to_owned();
        let current_name = if self.config.current_preset.is_empty() {
            no_preset.clone()
        } else {
            self.config.current_preset.clone()
        };
        let previous_preset = self.config.current_preset.clone();
        let previous_mapping_count = self.config.mappings.len();
        let presets = self.config.presets.clone(); // 每帧仅一次克隆
        let combo_ir = egui::ComboBox::from_id_salt("titlebar_preset_selector")
            .selected_text(current_name)
            .width(130.0)
            .show_ui(ui, |ui| {
                if ui
                    .selectable_label(self.config.current_preset.is_empty(), &no_preset)
                    .clicked()
                {
                    /* 切"无": 仅清空当前预设名, 保留现有映射 (防止误删用户配置) */
                    self.config.current_preset.clear();
                }
                for preset in &presets {
                    let is_selected = self.config.current_preset == preset.name;
                    if ui.selectable_label(is_selected, &preset.name).clicked() {
                        self.config.current_preset = preset.name.clone();
                        /* 空预设不覆盖 (防止把当前映射清空); 非空才加载 */
                        if !preset.mappings.is_empty() {
                            self.config.mappings = preset.mappings.clone();
                        }
                    }
                }
            });
        if previous_preset != self.config.current_preset
            || previous_mapping_count != self.config.mappings.len()
        {
            if let Err(e) = self.config.save_to_file("Config.toml") {
                eprintln!("Failed to save preset switch: {}", e);
            }
            if let Err(e) = self.app_state.reload_config(self.config.clone()) {
                eprintln!("Failed to reload config after preset switch: {}", e);
            }
            if let Some(temp_config) = &mut self.temp_config {
                temp_config.current_preset = self.config.current_preset.clone();
                temp_config.presets = self.config.presets.clone();
            }
        }
        combo_ir.response
    }

    /// 侧边栏: 页面导航 (手柄映射第一) + 底部全局运行状态。
    fn render_nav_rail(&mut self, ui: &mut egui::Ui, frame_state: &FrameState) {
        let th = self.theme();

        ui.add_space(4.0);
        ui.label(th.hint_text("导航"));
        ui.add_space(theme::SP_XS);

        for page in Page::TABS {
            let icon = page.icon();
            if widgets::nav_item(ui, &th, icon, page.label(), self.active_page == *page) {
                self.active_page = *page;
            }
            ui.add_space(2.0);
        }

        /* 底部: 全局运行状态 (常驻, 任何页面都能看到) */
        ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
            ui.add_space(theme::SP_S);

            let th = self.theme();
            th.panel(ui, None, |ui| {
                ui.horizontal(|ui| {
                    let (color, text, pulsing) = if frame_state.is_paused {
                        (th.warn, "已暂停", false)
                    } else {
                        (th.good, "运行中", true)
                    };
                    widgets::status_dot(ui, color, pulsing, 4.5);
                    ui.label(egui::RichText::new(text).size(12.5).strong().color(th.heading));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let (icon, tip) = if frame_state.is_paused {
                            (widgets::Icon::Play, "恢复连发")
                        } else {
                            (widgets::Icon::Pause, "暂停连发")
                        };
                        if widgets::icon_button(ui, &th, icon, tip).clicked() {
                            self.toggle_with_notify();
                        }
                    });
                });
                ui.add_space(theme::SP_XS);
                // 震动状态小胶囊
                let vib_on = self
                    .app_state
                    .vibration_enabled
                    .load(std::sync::atomic::Ordering::Relaxed);
                let (vtext, vcolor) = if vib_on {
                    ("震动 开", th.good)
                } else {
                    ("震动 关", th.hint)
                };
                th.badge(ui, vtext, vcolor, if vib_on { th.good_soft } else { th.faint });
            });
        });
    }

    /// 暂停/恢复连发并发送系统通知 (侧边栏与 hero 共用)。
    fn toggle_with_notify(&mut self) {
        let was_paused = self.app_state.toggle_paused();
        if let Some(sender) = self.app_state.get_notification_sender() {
            let msg = if was_paused {
                "Sorahk activating"
            } else {
                "Sorahk paused"
            };
            let _ = sender.send(NotificationEvent::Info(msg.to_string()));
        }
    }

    /* ═══════════════════ 连发映射页 (v2) ═══════════════════ */

    /// 连发页: 状态 hero + 映射列表 + 全局参数。
    fn render_turbo_page(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, frame_state: &FrameState) {
        let _ = ctx;
        self.render_turbo_hero(ui, frame_state);
        self.render_turbo_preset_manager(ui);
        self.render_turbo_mappings(ui);
        self.render_turbo_params(ui);
    }

    /// 首次运行用户指引 (config.guide_seen = false 时显示)。
    fn render_guide_window(&mut self, ctx: &egui::Context) {
        let th = self.theme();
        egui::Window::new(" ")
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
                            ui.label(th.hint_text("三步上手 · 所有修改实时生效, 无需手动保存"));
                            ui.add_space(theme::SP_M);
                        });
                        let steps: Vec<(&str, &str)> = vec![
                            ("① 连发映射", "「+ 新增映射」把手柄/键盘/鼠标按键连发成任意目标键, 切换热键 DELETE 一键启停"),
                            ("② 手柄映射", "直接点手柄图上的按键绑定目标键, 即点即用"),
                            ("③ 通用震动 / 全职业预设", "DFO 战斗事件实时驱动手柄马达; 强度滑块即时可调"),
                            ("④ 极简模式", "顶栏「简」一键切到小窗挂机, 「完整界面」随时返回"),
                            ("⑤ 关闭窗口", "最小化到托盘继续运行; 托盘图标右键可退出"),
                        ];
                        for (t, d) in &steps {
                            ui.horizontal(|ui| {
                                ui.label(
                                    egui::RichText::new(*t)
                                        .size(14.0)
                                        .strong()
                                        .family(Theme::font_bold())
                                        .color(th.accent_text),
                                );
                            });
                            ui.label(th.hint_text(*d));
                            ui.add_space(theme::SP_XS);
                        }
                        ui.add_space(theme::SP_M);
                        ui.vertical_centered(|ui| {
                            if ui
                                .add_sized(
                                    [180.0, 34.0],
                                    egui::Button::new(
                                        egui::RichText::new("开始使用")
                                            .size(14.0)
                                            .strong()
                                            .color(egui::Color32::WHITE),
                                    )
                                    .fill(th.btn_primary)
                                    .corner_radius(egui::CornerRadius::same(theme::RADIUS_CTRL)),
                                )
                                .clicked()
                            {
                                self.config.guide_seen = true;
                                let _ = self.config.save_to_file("Config.toml");
                            }
                            ui.add_space(theme::SP_XS);
                            ui.label(th.hint_text("随时可在「设置 → 预设管理」配置更多预设"));
                        });
                    });
            });
    }

    /// 连发映射页 · 预设管理卡: 切换 / 保存 / 重命名 / 删除 (两次确认防误删)。
    fn render_turbo_preset_manager(&mut self, ui: &mut egui::Ui) {
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
    fn render_turbo_hero(&mut self, ui: &mut egui::Ui, frame_state: &FrameState) {
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
    fn render_turbo_mappings(&mut self, ui: &mut egui::Ui) {
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
    fn add_new_mapping(&mut self) {
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
    fn begin_mapping_edit(&mut self, idx: usize) {
        if let Some(m) = self.config.mappings.get(idx) {
            self.edit_mapping_snapshot = Some(m.clone());
            self.edit_mapping_is_new = false;
            self.edit_mapping_idx = Some(idx);
        }
    }

    /// 行内编辑面板: 触发/目标/间隔/时长/连发/备注 + 保存/取消/删除。
    fn render_mapping_edit_row(
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
    fn start_mapping_capture(&mut self, idx: usize, is_trigger: bool) {
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
    fn handle_turbo_edit_capture(&mut self, ctx: &egui::Context) {
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

    /// 切换极简模式 (进入缩窗, 退出恢复原窗口尺寸/最大化状态)。
    /// 应用通用震动预设 (显式参数版: 供 vib_section 闭包内使用, 避免整体 &mut self 捕获)。
    /// 互斥: 若全职业预设开启, 先关闭并恢复默认。
    #[allow(clippy::too_many_arguments)]
    fn apply_general_vibration_preset_in(
        name: &str,
        app_state: &crate::state::AppState,
        config: &mut crate::config::AppConfig,
        job_enabled: &mut bool,
        job_active: &mut Option<(String, String)>,
        job_loaded: &mut Option<(usize, usize)>,
    ) {
        if *job_enabled {
            Self::disable_job_vibration_preset_in(app_state, config, job_enabled, job_active, job_loaded);
        }
        let p = &app_state.vibration_params;
        let pr0 = match config.vibration_presets.iter().find(|x| x.name == name) {
            Some(x) => x.clone(),
            None => return,
        };
        let pr = crate::config::default_vibration_presets()
            .into_iter()
            .find(|x| x.name == pr0.name)
            .unwrap_or(pr0);
        for (i, v) in pr.params.iter().enumerate() {
            p[i].store(*v, std::sync::atomic::Ordering::Relaxed);
        }
        for (i, v) in pr.item_lr.iter().enumerate() {
            app_state.vibration_item_lr[i].store(*v, std::sync::atomic::Ordering::Relaxed);
            config.vibration.item_lr[i] = *v;
        }
        for (i, v) in pr.rank_lr.iter().enumerate() {
            app_state.vibration_rank_lr[i].store(*v, std::sync::atomic::Ordering::Relaxed);
            config.vibration.rank_lr[i] = *v;
        }
        for (i, v) in pr.rank_type_gain.iter().enumerate() {
            app_state.vibration_rank_type_gain[i].store(*v, std::sync::atomic::Ordering::Relaxed);
        }
        app_state.vibration_rank_level_gain.store(pr.rank_level_gain, std::sync::atomic::Ordering::Relaxed);
        app_state.vibration_rank_duration.store(pr.rank_duration, std::sync::atomic::Ordering::Relaxed);
        config.vibration.rank_type_gain = pr.rank_type_gain;
        config.vibration.rank_level_gain = pr.rank_level_gain;
        config.vibration.rank_duration = pr.rank_duration;
        app_state.vibration_out_smooth.store(pr.out_smooth, std::sync::atomic::Ordering::Relaxed);
        config.vibration.out_smooth = pr.out_smooth;
        // 职业专属通道复位 (与 disable_job 同语义): 切回通用预设后节流/算法
        // 不能残留上一职业的值 (apply_job 会写入它们, 三个函数复位集合必须一致)
        app_state.vibration_throttle_window.store(0, std::sync::atomic::Ordering::Relaxed);
        app_state.vibration_throttle_max.store(0, std::sync::atomic::Ordering::Relaxed);
        app_state.vibration_throttle_dense_ratio.store(100, std::sync::atomic::Ordering::Relaxed);
        app_state.vibration_algo_id.store(0, std::sync::atomic::Ordering::Relaxed);
        for ap in &app_state.vibration_algo_ap {
            ap.store(0, std::sync::atomic::Ordering::Relaxed);
        }
        config.vibration.throttle_window_ms = 0;
        config.vibration.throttle_max_hits = 0;
        config.vibration.throttle_dense_ratio = 100;
        Self::sync_vib_config_from_params(&mut config.vibration, p);
        let _ = config.save_vibration_to_file(crate::config::AppConfig::vibration_path_for("Config.toml"));
    }

    /// 关闭全职业预设 (显式参数版): 恢复默认 + 落盘 applied=false。
    fn disable_job_vibration_preset_in(
        app_state: &crate::state::AppState,
        config: &mut crate::config::AppConfig,
        job_enabled: &mut bool,
        job_active: &mut Option<(String, String)>,
        job_loaded: &mut Option<(usize, usize)>,
    ) {
        *job_enabled = false;
        *job_active = None;
        *job_loaded = None;
        let p = &app_state.vibration_params;
        if let Some(pr) = crate::config::default_vibration_presets().first() {
            for (i, v) in pr.params.iter().enumerate() {
                p[i].store(*v, std::sync::atomic::Ordering::Relaxed);
            }
            for (i, v) in pr.item_lr.iter().enumerate() {
                app_state.vibration_item_lr[i].store(*v, std::sync::atomic::Ordering::Relaxed);
            }
            for (i, v) in pr.rank_lr.iter().enumerate() {
                app_state.vibration_rank_lr[i].store(*v, std::sync::atomic::Ordering::Relaxed);
            }
            for (i, v) in pr.rank_type_gain.iter().enumerate() {
                app_state.vibration_rank_type_gain[i].store(*v, std::sync::atomic::Ordering::Relaxed);
            }
            app_state.vibration_rank_level_gain.store(pr.rank_level_gain, std::sync::atomic::Ordering::Relaxed);
            app_state.vibration_rank_duration.store(pr.rank_duration, std::sync::atomic::Ordering::Relaxed);
            app_state.vibration_out_smooth.store(pr.out_smooth, std::sync::atomic::Ordering::Relaxed);
        }
        app_state.vibration_abs_freq_enabled.store(false, std::sync::atomic::Ordering::Relaxed);
        // 职业专属通道复位: 节流/专属算法由 apply_job 写入, 关闭职业预设必须清零,
        // 否则 UI 已显示"默认"而引擎仍按旧职业的节流+算法运行, 且残留值随保存持久化
        app_state.vibration_throttle_window.store(0, std::sync::atomic::Ordering::Relaxed);
        app_state.vibration_throttle_max.store(0, std::sync::atomic::Ordering::Relaxed);
        app_state.vibration_throttle_dense_ratio.store(100, std::sync::atomic::Ordering::Relaxed);
        app_state.vibration_algo_id.store(0, std::sync::atomic::Ordering::Relaxed);
        for ap in &app_state.vibration_algo_ap {
            ap.store(0, std::sync::atomic::Ordering::Relaxed);
        }
        config.vibration.throttle_window_ms = 0;
        config.vibration.throttle_max_hits = 0;
        config.vibration.throttle_dense_ratio = 100;
        config.vibration.abs_freq_enabled = false;
        crate::job_presets::save_job_vibration(&crate::job_presets::JobVibrationConfig {
            base_job: String::new(),
            class_name: String::new(),
            applied: false,
            params: Vec::new(),
            rank_duration: 0,
            rank_level_gain: 0,
        });
    }

    /// 应用全职业预设 (显式参数版): 置启用态并落盘。
    #[allow(clippy::too_many_arguments)]
    fn apply_job_vibration_preset_in(
        base_job: &str,
        cls: &crate::job_presets::JobClass,
        app_state: &crate::state::AppState,
        config: &mut crate::config::AppConfig,
        job_enabled: &mut bool,
        job_active: &mut Option<(String, String)>,
        job_snapshot: &mut Vec<u32>,
    ) {
        let p = &app_state.vibration_params;
        for (i, v) in cls.params.iter().enumerate() {
            p[i].store(*v, std::sync::atomic::Ordering::Relaxed);
        }
        for (i, v) in cls.rank_lr.iter().enumerate() {
            app_state.vibration_rank_lr[i].store(*v as u32, std::sync::atomic::Ordering::Relaxed);
            config.vibration.rank_lr[i] = *v as u32;
        }
        for (i, v) in cls.rank_type_gain.iter().enumerate() {
            app_state.vibration_rank_type_gain[i].store(*v, std::sync::atomic::Ordering::Relaxed);
            config.vibration.rank_type_gain[i] = *v;
        }
        app_state.vibration_rank_level_gain.store(cls.rank_level_gain, std::sync::atomic::Ordering::Relaxed);
        app_state.vibration_rank_duration.store(cls.rank_duration, std::sync::atomic::Ordering::Relaxed);
        config.vibration.rank_level_gain = cls.rank_level_gain;
        config.vibration.rank_duration = cls.rank_duration;
        app_state.vibration_out_smooth.store(cls.out_smooth, std::sync::atomic::Ordering::Relaxed);
        config.vibration.out_smooth = cls.out_smooth;
        app_state.vibration_throttle_window.store(cls.throttle_window, std::sync::atomic::Ordering::Relaxed);
        app_state.vibration_throttle_max.store(cls.throttle_max, std::sync::atomic::Ordering::Relaxed);
        app_state.vibration_throttle_dense_ratio.store(cls.throttle_dense_ratio, std::sync::atomic::Ordering::Relaxed);
        app_state.vibration_abs_freq_enabled.store(cls.abs_freq_enabled, std::sync::atomic::Ordering::Relaxed);
        app_state.vibration_algo_id.store(cls.algo_id as u32, std::sync::atomic::Ordering::Relaxed);
        for (i, v) in cls.algo_params.iter().enumerate() {
            app_state.vibration_algo_ap[i].store(*v, std::sync::atomic::Ordering::Relaxed);
        }
        config.vibration.throttle_window_ms = cls.throttle_window;
        config.vibration.throttle_max_hits = cls.throttle_max;
        config.vibration.throttle_dense_ratio = cls.throttle_dense_ratio;
        config.vibration.abs_freq_enabled = cls.abs_freq_enabled;
        *job_snapshot = cls.params.to_vec();
        Self::save_job_vibration_state(app_state, base_job, cls.name);
        *job_enabled = true;
        *job_active = Some((base_job.to_string(), cls.name.to_string()));
    }

    /// 便捷包装 (极简模式等无借用冲突的调用点)。
    fn apply_general_vibration_preset(&mut self, name: &str) {
        Self::apply_general_vibration_preset_in(
            name,
            &self.app_state,
            &mut self.config,
            &mut self.vib_job_enabled,
            &mut self.vib_job_active,
            &mut self.vib_job_loaded,
        );
    }

    fn disable_job_vibration_preset(&mut self) {
        Self::disable_job_vibration_preset_in(
            &self.app_state,
            &mut self.config,
            &mut self.vib_job_enabled,
            &mut self.vib_job_active,
            &mut self.vib_job_loaded,
        );
    }

    fn apply_job_vibration_preset(&mut self, base_job: &str, cls: &crate::job_presets::JobClass) {
        Self::apply_job_vibration_preset_in(
            base_job,
            cls,
            &self.app_state,
            &mut self.config,
            &mut self.vib_job_enabled,
            &mut self.vib_job_active,
            &mut self.vib_job_snapshot,
        );
    }

    fn set_minimal_mode(&mut self, ctx: &egui::Context, on: bool) {
        self.minimal_mode = on;
        self.config.minimal_mode = on;
        if !on {
            /* 退出极简: 极简矩形落盘 + 精确还原完整模式上次的窗口位置+尺寸 */
            if let Some((pos, sz)) = self.minimal_window_rect {
                self.config.window_rect_minimal = Some([pos.x, pos.y, sz.x, sz.y]);
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
            // 无论走哪条恢复路径都清掉"已进入"标记: 否则第二次进极简时
            // is_none() 守卫短路, 不再重新采样完整窗口尺寸/最大化状态,
            // 退出恢复的是上一次的旧矩形
            self.normal_window_size = None;
        }
        let _ = self.config.save_to_file("Config.toml");
    }

    /// 极简模式顶栏: 品牌 + 预设快切 + 主题 + 返回完整界面 (永远可见)。
    fn render_minimal_top_bar(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let th = self.theme();
        // 拖动区先于按钮注册 (同完整顶栏)
        self.render_title_bar_drag(ui, ctx);
        ui.horizontal_centered(|ui| {
            ui.add_space(2.0);
            let (rect, _) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
            ui.painter().rect_filled(rect, 5, th.accent);
            // 短品牌名 (小窗放不下完整标题)
            ui.label(
                egui::RichText::new("DfoVibration-V3")
                    .size(13.0)
                    .strong()
                    .family(Theme::font_bold())
                    .color(th.title),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                self.render_window_controls(ui, ctx);
                ui.add_space(theme::SP_S);
                if widgets::icon_button(ui, &th, widgets::Icon::Theme, "切换主题").clicked() {
                    self.dark_mode = !self.dark_mode;
                    self.config.dark_mode = self.dark_mode;
                    let _ = self.config.save_to_file("Config.toml");
                    if let Some(temp_config) = &mut self.temp_config {
                        temp_config.dark_mode = self.dark_mode;
                    }
                }
                ui.add_space(theme::SP_XS);
                // 返回完整界面 (顶栏常驻)
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
                    .clicked()
                {
                    self.set_minimal_mode(ctx, false);
                }
            });
        });
    }

    /// 极简模式页面: 连发开关 + 预设 + 震动开关。
    ///
    /// 布局用单层 `top_down(Align::Center)`: 所有元素自动水平居中。
    /// 不要在此处嵌套 horizontal_centered —— 它是 centered_and_justified 语义,
    /// 会垂直占满剩余空间并把后续内容挤出视口 (曾导致文字全部消失)。
    fn render_minimal_page(&mut self, ui: &mut egui::Ui, frame_state: &FrameState) {
        let th = self.theme();

        ui.with_layout(egui::Layout::top_down(egui::Align::Center), |ui| {
            ui.add_space(theme::SP_L);

            // 品牌
            let (rect, _) = ui.allocate_exact_size(egui::vec2(32.0, 32.0), egui::Sense::hover());
            crate::gui::widgets::Icon::Gamepad.paint(ui.painter(), rect, th.accent);
            ui.add_space(theme::SP_XS);
            ui.label(th.h1("极简模式"));
            ui.add_space(theme::SP_XS);
            ui.label(th.hint_text("只保留两个总开关, 一键启停"));
            ui.add_space(theme::SP_M);

            // 连发大开关
            let running = !frame_state.is_paused;
            let (ltext, lfg, lbg) = if running {
                ("连发 · 开启中", th.good, th.good_soft)
            } else {
                ("连发 · 已暂停", th.hint, th.faint)
            };
            let turbo_btn = egui::Button::new(
                egui::RichText::new(ltext).size(16.0).strong().color(lfg),
            )
            .fill(lbg)
            .corner_radius(egui::CornerRadius::same(12))
            .min_size(egui::vec2(260.0, 50.0));
            if ui.add_sized([280.0, 50.0], turbo_btn).clicked() {
                self.toggle_with_notify();
            }
            ui.add_space(theme::SP_S);

            // 预设快切 (极简模式中间)
            if self.config.presets.is_empty() {
                ui.label(th.hint_text("无预设"));
            } else {
                // egui 的 main_align 不作用于水平行的摆放 (只对齐按钮内文字),
                // 行级居中须自测内容宽度 → 定宽子块交给 top_down(Center) 父级居中。
                // 块高取 interact_size.y (= ComboBox 内部 horizontal 条带高度):
                // combo 在条带内顶部锚定, 块与条带同高则位置确定;
                // label 用 ui.put 精确对齐到 combo 垂直中心 (两者不再差 ~2.5px)。
                // RTL: 先画 combo (占右端), 再按其 rect 反推 label 位置。
                let label_w = ui
                    .painter()
                    .layout_no_wrap(
                        "连发与映射".to_owned(),
                        egui::FontId::proportional(12.0),
                        egui::Color32::WHITE,
                    )
                    .size()
                    .x;
                let combo_w = 130.0; // render_preset_switch 里 ComboBox::width(130)
                let gap = theme::SP_XS + ui.spacing().item_spacing.x;
                let row_w = label_w + gap + combo_w;
                let preset_row = ui.allocate_ui_with_layout(
                    egui::vec2(row_w, ui.spacing().interact_size.y),
                    egui::Layout::right_to_left(egui::Align::Center),
                    |ui| {
                        let combo = self.render_preset_switch(ui);
                        let label_rect = egui::Rect::from_center_size(
                            egui::pos2(
                                combo.rect.left() - gap - label_w / 2.0,
                                combo.rect.center().y,
                            ),
                            egui::vec2(label_w, combo.rect.height()),
                        );
                        ui.put(label_rect, egui::Label::new(th.weak("连发与映射")));
                    },
                );
            }
            ui.add_space(theme::SP_S);

            // 震动大开关
            let vib_on = self
                .app_state
                .vibration_enabled
                .load(std::sync::atomic::Ordering::Relaxed);
            let (vtext, vfg, vbg) = if vib_on {
                ("震动 · 开启", th.good, th.good_soft)
            } else {
                ("震动 · 关闭", th.hint, th.faint)
            };
            let vib_btn = egui::Button::new(
                egui::RichText::new(vtext).size(16.0).strong().color(vfg),
            )
            .fill(vbg)
            .corner_radius(egui::CornerRadius::same(12))
            .min_size(egui::vec2(260.0, 50.0));
            if ui.add_sized([280.0, 50.0], vib_btn).clicked() {
                let v = self
                    .app_state
                    .vibration_enabled
                    .load(std::sync::atomic::Ordering::Relaxed);
                self.app_state
                    .vibration_enabled
                    .store(!v, std::sync::atomic::Ordering::Relaxed);
            }
            ui.add_space(theme::SP_S);

            /* ── 震动预设: 通用预设 / 全职业预设 互斥切换 ──
             * 两者互斥: 应用任一侧预设会自动关闭另一侧 (应用层互斥) */
            let mode_job = self.minimal_vib_preset_job;
            let seg_id = egui::Id::new("minimal_vib_preset_mode");
            let seg_w = 130.0_f32;
            let seg_h = 26.0_f32;
            let (seg_rect, _) =
                ui.allocate_exact_size(egui::vec2(seg_w * 2.0 + 4.0, seg_h), egui::Sense::hover());
    
            let left_rect = egui::Rect::from_x_y_ranges(
                seg_rect.left()..=seg_rect.center().x,
                seg_rect.top()..=seg_rect.bottom(),
            );
            let right_rect = egui::Rect::from_x_y_ranges(
                seg_rect.center().x..=seg_rect.right(),
                seg_rect.top()..=seg_rect.bottom(),
            );
            let l_resp = ui
                .interact(left_rect, seg_id.with("l"), egui::Sense::click())
                .on_hover_cursor(egui::CursorIcon::PointingHand);
            let r_resp = ui
                .interact(right_rect, seg_id.with("r"), egui::Sense::click())
                .on_hover_cursor(egui::CursorIcon::PointingHand);
            /* 容器底 + 滑块 (150ms 动画) */
            ui.painter().rect_filled(seg_rect, 8, th.faint);
            let t = ui.ctx().animate_value_with_time(seg_id, if mode_job { 1.0 } else { 0.0 }, 0.15);
            let thumb_w = seg_w - 6.0;
            let thumb_x = seg_rect.left() + 3.0 + t * (seg_rect.width() - thumb_w - 6.0);
            let thumb = egui::Rect::from_min_size(
                egui::pos2(thumb_x, seg_rect.top() + 3.0),
                egui::vec2(thumb_w, seg_h - 6.0),
            );
            ui.painter().rect_filled(thumb, 6, th.accent);
            /* 两段文字: 选中侧 = 滑块上的白色; 未选侧 = text_weak (按主题, 浅色下白字不可见) */
            let unselected = |a: f32| -> egui::Color32 {
                let mut c = th.text_weak;
                c = egui::Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), a as u8);
                c
            };
            let selected_white = |a: f32| -> egui::Color32 {
                egui::Color32::from_rgba_unmultiplied(255, 255, 255, a as u8)
            };
            ui.painter().text(
                egui::pos2(left_rect.center().x, seg_rect.center().y),
                egui::Align2::CENTER_CENTER,
                "通用预设",
                egui::FontId::proportional(12.0),
                if mode_job { unselected(200.0) } else { selected_white(255.0) },
            );
            ui.painter().text(
                egui::pos2(right_rect.center().x, seg_rect.center().y),
                egui::Align2::CENTER_CENTER,
                "全职业预设",
                egui::FontId::proportional(12.0),
                if mode_job { selected_white(255.0) } else { unselected(200.0) },
            );
            if l_resp.clicked() {
                self.minimal_vib_preset_job = false;
                let _ = self.config.save_to_file("Config.toml");
            }
            if r_resp.clicked() {
                self.minimal_vib_preset_job = true;
                /* 切到全职业预设: 直接启用当前选中职业 (无需再点应用) */
                let jobs = crate::job_presets::builtin_jobs();
                let base_sel = self.vib_job_base.min(jobs.len().saturating_sub(1));
                let class_sel = self.vib_job_class.min(jobs[base_sel].classes.len().saturating_sub(1));
                self.apply_job_vibration_preset(jobs[base_sel].base_job, &jobs[base_sel].classes[class_sel]);
                self.vib_job_loaded = Some((base_sel, class_sel));
                let _ = self.config.save_to_file("Config.toml");
            }
            ui.add_space(theme::SP_XS);

            /* 预设选择 (来源由上面的分段开关决定) */
            if !mode_job {
                let names: Vec<String> = self
                    .config
                    .vibration_presets
                    .iter()
                    .map(|x| x.name.clone())
                    .collect();
                let sel = self.vib_preset_idx.min(names.len().saturating_sub(1));
                let selected = names.get(sel).cloned().unwrap_or_default();
                /* ComboBox 内嵌 horizontal 不被父级居中 → 定宽子块居中法 (同连发与映射行) */
                let combo_w = 200.0_f32;
                ui.allocate_ui_with_layout(
                    egui::vec2(combo_w, ui.spacing().interact_size.y),
                    egui::Layout::right_to_left(egui::Align::Center),
                    |ui| {
                        egui::ComboBox::from_id_salt("minimal_vib_general")
                            .selected_text(selected)
                            .width(200.0)
                            .show_ui(ui, |ui| {
                                for (i, n) in names.iter().enumerate() {
                                    if ui.selectable_label(i == sel, n).clicked() {
                                        self.vib_preset_idx = i;
                                        self.apply_general_vibration_preset(n);
                                    }
                                }
                            });
                    },
                );
            } else {
                /* 全职业: 基础职业 → 转职 两级选择 (选中转职立即应用) */
                let jobs = crate::job_presets::builtin_jobs();
                let base_sel = self.vib_job_base.min(jobs.len().saturating_sub(1));
                let classes = &jobs[base_sel].classes;
                let class_sel = self.vib_job_class.min(classes.len().saturating_sub(1));
                let cur_active = self.vib_job_active.clone();
                let job_enabled = self.vib_job_enabled;
                let combo_w = 150.0_f32;
                /* 块宽用上一帧实测值 (首帧兜底 316): combo 实际渲染宽度 ≠ width() 设定值,
                 * 手工估算总有几像素偏差, 实测值才能让内容严丝合缝填满居中的块 */
                let gap = theme::SP_XS + ui.spacing().item_spacing.x;
                let block_w = if self.minimal_job_block_w > 0.0 {
                    self.minimal_job_block_w
                } else {
                    combo_w * 2.0 + gap
                };
                let combo_h = ui.spacing().interact_size.y;
                let slot_h = 26.0_f32; // 两 combo 统一槽高
                let class_nudge = -1.5_f32; // 转职槽累计上调 (用户微调: 原 -3.0, 回调 1.5)
                let block = ui.allocate_ui_with_layout(
                    egui::vec2(block_w, slot_h),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        let inner = ui.max_rect();
                        /* 基础职业: 左槽 */
                        ui.allocate_new_ui(
                            egui::UiBuilder::new()
                                .max_rect(egui::Rect::from_min_size(
                                    egui::pos2(inner.left(), inner.top()),
                                    egui::vec2(combo_w, slot_h),
                                ))
                                .layout(egui::Layout::right_to_left(egui::Align::Center)),
                            |ui| {
                                egui::ComboBox::from_id_salt("minimal_vib_job_base")
                                    .selected_text(jobs[base_sel].base_job)
                                    .width(combo_w)
                                    .show_ui(ui, |ui| {
                                        for (i, j) in jobs.iter().enumerate() {
                                            if ui.selectable_label(i == base_sel, j.base_job).clicked() {
                                                self.vib_job_base = i;
                                                self.vib_job_class = 0;
                                            }
                                        }
                                    });
                            },
                        );
                        /* 转职: 右槽, 累计上调 (class_nudge) */
                        ui.allocate_new_ui(
                            egui::UiBuilder::new()
                                .max_rect(egui::Rect::from_min_size(
                                    egui::pos2(inner.left() + combo_w + gap, inner.top() + class_nudge),
                                    egui::vec2(combo_w, slot_h),
                                ))
                                .layout(egui::Layout::right_to_left(egui::Align::Center)),
                            |ui| {
                                egui::ComboBox::from_id_salt("minimal_vib_job_class")
                                    .selected_text(classes[class_sel].name.clone())
                                    .width(combo_w)
                                    .show_ui(ui, |ui| {
                                        for (i, c) in classes.iter().enumerate() {
                                            if ui.selectable_label(i == class_sel, c.name).clicked() {
                                                self.vib_job_class = i;
                                                self.apply_job_vibration_preset(jobs[base_sel].base_job, c);
                                            }
                                        }
                                    });
                            },
                        );
                    },
                );

                /* 全职业预设启用状态由分段开关自动管理 (切到全职业即启用当前职业; 切回通用自动恢复) */
                ui.add_space(theme::SP_S);
            }

            // 状态行 (居中原理同预设行: 测宽 + 定宽子块)
            let (dot, stat_text) = if vib_on && running {
                (th.good, "连发与震动均已就绪")
            } else if vib_on {
                (th.warn, "震动开启 · 连发已暂停")
            } else if running {
                (th.info, "连发运行中 · 震动关闭")
            } else {
                (th.hint, "全部关闭")
            };
            let dot_w = 4.5 * 2.4; // status_dot 内部 allocate 的宽度
            let stat_w = ui
                .painter()
                .layout_no_wrap(
                    stat_text.to_owned(),
                    egui::FontId::proportional(12.0),
                    egui::Color32::WHITE,
                )
                .size()
                .x;
            let gap = theme::SP_XS + ui.spacing().item_spacing.x;
            let status_row = ui.allocate_ui_with_layout(
                egui::vec2(dot_w + gap + stat_w, 18.0),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    widgets::status_dot(ui, dot, running, 4.5);
                    ui.add_space(theme::SP_XS);
                    ui.label(th.weak(stat_text));
                },
            );
            ui.add_space(theme::SP_S);
            ui.label(th.hint_text("顶栏「⤺ 完整界面」返回"));
        });
    }

    /* ═══════════════════ 自绘窗口标题栏 (无边框窗口) ═══════════════════ */

    /// 顶栏拖动区 + 双击最大化 (在顶栏内容渲染完成后调用)。
    fn render_title_bar_drag(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
        let drag_rect = ui.max_rect();
        let drag = ui.interact(
            drag_rect,
            ui.id().with("title_bar_drag"),
            egui::Sense::click_and_drag(),
        );
        if drag.dragged_by(egui::PointerButton::Primary) {
            ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
        }
        if drag.double_clicked() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
        }
    }

    /// 窗口控制三按钮: 最小化 / 最大化还原 / 关闭 (关闭走确认弹窗)。
    fn render_window_controls(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let th = self.theme();
        let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));

        use egui_phosphor::variants::regular as ic;
        let ctrl = |ui: &mut egui::Ui, glyph: &str, hover: &str, danger: bool| -> bool {
            let fg = if danger { th.bad } else { th.text_weak };
            let btn = egui::Button::new(
                egui::RichText::new(glyph).size(13.0).color(fg),
            )
            .fill(egui::Color32::TRANSPARENT)
            .corner_radius(egui::CornerRadius::same(6))
            .min_size(egui::vec2(34.0, 26.0));
            let resp = ui.add(btn);
            let hovered = resp.hovered();
            // 悬停底色渐入渐出 (150ms)
            let hover_t = ui.ctx().animate_bool_with_time(
                egui::Id::new("win_ctrl_hover").with(resp.rect.min.x.to_bits()).with(resp.rect.min.y.to_bits()),
                hovered,
                0.15,
            );
            if hover_t > 0.01 {
                let (r, g, b) = if danger { (th.bad.r(), th.bad.g(), th.bad.b()) } else { (th.faint.r(), th.faint.g(), th.faint.b()) };
                let a = (hover_t * if danger { 230.0 } else { 26.0 }) as u8;
                let bg = egui::Color32::from_rgba_unmultiplied(r, g, b, a);
                ui.painter()
                    .rect_filled(resp.rect, egui::CornerRadius::same(6), bg);
                // 重画字形 (背景覆盖后); 悬停色随 hover_t 淡入淡出
                // 危险钮/深色 → 白字; 浅色普通钮 → 白字在浅灰底上不可见, 用 title
                let target = if danger || th.dark {
                    egui::Color32::WHITE
                } else {
                    th.title
                };
                let glyph_alpha = (hover_t * 255.0) as u8;
                ui.painter().text(
                    resp.rect.center(),
                    egui::Align2::CENTER_CENTER,
                    glyph,
                    egui::FontId::proportional(13.0),
                    target.gamma_multiply(f32::from(glyph_alpha) / 255.0),
                );
            }
            resp.on_hover_text(hover).clicked()
        };

        // RTL 布局: 先添加的在最右。Windows 惯例从左到右 = ─ □ ✕
        if ctrl(ui, ic::X, "关闭", true) {
            // 触发 close_requested → 走已有的关闭确认弹窗
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if ctrl(
            ui,
            if maximized { ic::CORNERS_OUT } else { ic::SQUARE },
            if maximized { "还原" } else { "最大化" },
            false,
        ) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
        }
        if ctrl(ui, ic::MINUS, "最小化", false) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
        }
    }

    /// 无边框窗口的边缘缩放热区 (8 方向, 最大化时不启用)。
    fn render_resize_hotspots(&mut self, ctx: &egui::Context) {
        use egui::viewport::ResizeDirection;
        let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
        if maximized {
            return;
        }
        let screen = ctx.screen_rect();
        let w = 6.0;
        let c = 16.0;
        let spots: Vec<(ResizeDirection, egui::Rect)> = vec![
            (ResizeDirection::North, egui::Rect::from_min_size(screen.left_top(), egui::vec2(screen.width(), w))),
            (ResizeDirection::South, egui::Rect::from_min_size(egui::pos2(screen.left(), screen.bottom() - w), egui::vec2(screen.width(), w))),
            (ResizeDirection::West, egui::Rect::from_min_size(screen.left_top(), egui::vec2(w, screen.height()))),
            (ResizeDirection::East, egui::Rect::from_min_size(egui::pos2(screen.right() - w, screen.top()), egui::vec2(w, screen.height()))),
            (ResizeDirection::NorthWest, egui::Rect::from_min_size(screen.left_top(), egui::vec2(c, c))),
            (ResizeDirection::NorthEast, egui::Rect::from_min_size(egui::pos2(screen.right() - c, screen.top()), egui::vec2(c, c))),
            (ResizeDirection::SouthWest, egui::Rect::from_min_size(egui::pos2(screen.left(), screen.bottom() - c), egui::vec2(c, c))),
            (ResizeDirection::SouthEast, egui::Rect::from_min_size(egui::pos2(screen.right() - c, screen.bottom() - c), egui::vec2(c, c))),
        ];
        for (i, (dir, rect)) in spots.iter().enumerate() {
            let cursor = match dir {
                ResizeDirection::North => egui::CursorIcon::ResizeNorth,
                ResizeDirection::South => egui::CursorIcon::ResizeSouth,
                ResizeDirection::West => egui::CursorIcon::ResizeWest,
                ResizeDirection::East => egui::CursorIcon::ResizeEast,
                ResizeDirection::NorthWest => egui::CursorIcon::ResizeNorthWest,
                ResizeDirection::NorthEast => egui::CursorIcon::ResizeNorthEast,
                ResizeDirection::SouthWest => egui::CursorIcon::ResizeSouthWest,
                ResizeDirection::SouthEast => egui::CursorIcon::ResizeSouthEast,
            };
            egui::Area::new(egui::Id::new("resize_hotspot").with(i))
                .fixed_pos(rect.min)
                .order(egui::Order::Foreground)
                .interactable(true)
                .show(ctx, |ui| {
                    ui.set_min_size(rect.size());
                    ui.set_max_size(rect.size());
                    let resp = ui.allocate_rect(ui.max_rect(), egui::Sense::drag());
                    if resp.dragged_by(egui::PointerButton::Primary) {
                        ctx.send_viewport_cmd(egui::ViewportCommand::BeginResize(*dir));
                    }
                    resp.on_hover_cursor(cursor);
                });
        }
    }

    /// 全局参数卡。
    fn render_turbo_params(&self, ui: &mut egui::Ui) {
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
    fn param_row(ui: &mut egui::Ui, th: &Theme, label: &str, value: &str) {
        ui.label(th.weak(label));
        ui.label(egui::RichText::new(value).size(13.0).strong().color(th.accent_text));
        ui.end_row();
    }

    /// 布尔参数行: 标签 | 状态徽章 (网格内)。
    fn param_flag(ui: &mut egui::Ui, th: &Theme, label: &str, on: bool) {
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
