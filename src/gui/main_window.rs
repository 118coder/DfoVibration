//! Main window implementation and rendering logic.

use crate::gui::SorahkGui;
use crate::gui::about_dialog::render_about_dialog;
use crate::gui::theme::{self, Theme, truncate_chars};
use crate::gui::widgets;
use crate::gui::types::{KeyCaptureMode, Page};
use crate::state::NotificationEvent;
use eframe::egui;

/// Cached frame state to avoid repeated atomic operations.
pub(super) struct FrameState {
    pub(super) is_paused: bool,
    pub(super) worker_count: usize,
    pub(super) should_exit: bool,
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
    pub(super) fn handle_close_dialog(&mut self, ctx: &egui::Context, frame_state: &FrameState) {
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
    pub(super) fn render_close_dialog(&mut self, ctx: &egui::Context) {
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
                        // 真正的托盘化: 隐藏窗口 (任务栏按钮消失, 只剩托盘图标)。
                        // 旧实现 Minimized(true) 只是最小化到任务栏, 且 winit 的
                        // set_minimized(false) 在 Win11 上恢复经常静默失效。
                        // 隐藏/显示 (Visible) 是 winit 可靠路径; 恢复另有托盘侧
                        // Win32 直连 (TrayIcon::restore_main_window) 双保险。
                        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
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
    pub(super) fn handle_keyboard_input(&mut self, _ctx: &egui::Context) {
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
    pub(super) fn vk_to_bit(vk: u32) -> u16 {
        1u16 << (vk % 16)
    }


    /// 应用外壳: 顶栏 + 侧边栏导航 + 内容区 (现代桌面应用三段式)。
    pub(super) fn render_shell(&mut self, ctx: &egui::Context, frame_state: &FrameState) {
        let th = self.theme();

        /* 首次运行: 第 1 弹问「是否 DFO 玩家」→ (是) 第 1.5 弹问「S1 ACT / S4+ 新版」
         * → 第 2 弹使用说明; 非 DFO 玩家跳过版本询问; 「?」重跑完整流程 (旁路 guide_seen) */
        if (!self.config.guide_seen || self.first_run_rerun)
            && !self.dfo_ask_answered
            && !self.show_guide
        {
            self.render_dfo_ask_window(ctx);
        }
        if self.show_edition_ask {
            self.render_edition_ask_window(ctx);
        }
        if self.show_guide {
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
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(430.0, 520.0)));
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
                    if (sz.x - 430.0).abs() > 1.5 || (sz.y - 520.0).abs() > 1.5 {
                        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(430.0, 520.0)));
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
                // DFO 功能关闭时震动页不可达: 当前页落在其上则回手柄映射
                if !self.config.dfo_player
                    && matches!(self.active_page, Page::Vibration | Page::JobPresets)
                {
                    self.active_page = Page::Gamepad;
                }
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
                    Page::Whitelist => {
                        egui::ScrollArea::vertical()
                            .auto_shrink([false; 2])
                            .show(ui, |ui| {
                                self.render_whitelist_page(ui);
                                ui.add_space(12.0);
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
    pub(super) fn render_preset_switch_centered(&mut self, ui: &mut egui::Ui) -> egui::Response {
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
    pub(super) fn render_top_bar(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, _frame_state: &FrameState) {
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
    pub(super) fn render_top_bar_actions(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let minimal_tip = "极简模式: 只保留连发/震动两个开关的小窗";
        let theme_tip = self.translations.light_theme().to_owned();
        let th = self.theme();

        // 使用说明 (标题栏「?」: 重新运行首次向导 —— DFO 询问 → 版本询问 → 使用说明,
        // 方便玩家随时重选; 说明末尾「开始使用」后结束)
        if widgets::icon_button(ui, &th, widgets::Icon::Question, "使用说明 / 重新选择向导 (DFO·版本)")
            .clicked()
        {
            self.first_run_rerun = true;
            self.dfo_ask_answered = false;
            self.show_edition_ask = false;
            self.show_guide = false;
        }

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
    pub(super) fn render_preset_switch(&mut self, ui: &mut egui::Ui) -> egui::Response {
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
    pub(super) fn render_nav_rail(&mut self, ui: &mut egui::Ui, frame_state: &FrameState) {
        let th = self.theme();

        ui.add_space(4.0);
        ui.label(th.hint_text("导航"));
        ui.add_space(theme::SP_XS);

        for page in Page::TABS {
            // DFO 非玩家: 隐藏「通用震动」「全职业预设」两个入口 (设置中可开启)
            if !self.config.dfo_player
                && matches!(page, Page::Vibration | Page::JobPresets)
            {
                continue;
            }
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
    pub(super) fn toggle_with_notify(&mut self) {
        let was_paused = self.app_state.toggle_paused();
        let msg = if was_paused {
            "Sorahk activating"
        } else {
            "Sorahk paused"
        };
        self.app_state
            .send_notification(NotificationEvent::Info(msg.to_string()));
    }

    /* ═══════════════════ 连发映射页 (v2) ═══════════════════ */


    /// 顶栏拖动区 + 双击最大化 (在顶栏内容渲染完成后调用)。
    pub(super) fn render_title_bar_drag(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
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
    pub(super) fn render_window_controls(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
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
    pub(super) fn render_resize_hotspots(&mut self, ctx: &egui::Context) {
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

}
