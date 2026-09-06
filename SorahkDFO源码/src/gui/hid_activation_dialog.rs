//! HID device activation dialog for establishing device baseline.

use crate::gui::device_info::{get_device_model, get_hid_device_type, get_vendor_name};
use crate::i18n::CachedTranslations;
use crate::gui::theme::Theme;
use eframe::egui;
use std::time::Instant;

/// State for HID device activation process
#[derive(Debug, Clone, PartialEq)]
pub enum ActivationState {
    WaitingForPress,   // Waiting for user to press a button
    WaitingForRelease, // Waiting for user to release the button
    Success,           // Activation successful
    Failed(String),    // Activation failed with error message
}

/// HID device activation dialog
pub struct HidActivationDialog {
    device_name: String,
    device_handle: isize,
    vid: u16,
    pid: u16,
    usage_page: u16,
    usage: u16,
    pub state: ActivationState,
    pub pressed_data: Option<Vec<u8>>,
    pub released_data: Option<Vec<u8>>,
    success_time: Option<Instant>,
    animation_progress: f32,
}

impl HidActivationDialog {
    #[inline]
    pub fn new(
        device_name: String,
        device_handle: isize,
        vid: u16,
        pid: u16,
        usage_page: u16,
        usage: u16,
    ) -> Self {
        Self {
            device_name,
            device_handle,
            vid,
            pid,
            usage_page,
            usage,
            state: ActivationState::WaitingForPress,
            pressed_data: None,
            released_data: None,
            success_time: None,
            animation_progress: 0.0,
        }
    }

    #[inline(always)]
    pub fn device_handle(&self) -> isize {
        self.device_handle
    }

    /// Render the activation dialog, returns true if should close
    pub fn render(
        &mut self,
        ctx: &egui::Context,
        dark_mode: bool,
        translations: &CachedTranslations,
    ) -> bool {
        let t = translations;

        // 现代化主题: 暗色 #11131C/#1C1F2D, 亮色 #FFFFFF/#EEEFF6, 强调紫 #7C8CF8/#6C5CE7
        let (
            bg_color,
            accent_color,
            text_color,
            text_secondary,
            success_color,
            warning_color,
            card_bg,
            warning_bg,
            danger_color,
        ) = {
            let th = Theme::new(dark_mode);
            (
                th.surface,
                th.accent,
                th.text,
                th.text_weak,
                th.good,
                th.warn,
                th.card_alt,
                th.warn_soft,
                th.bad,
            )
        };

        let mut should_close = false;

        egui::Window::new("hid_activation_dialog")
            .id(egui::Id::new("hid_activation_window"))
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .fixed_size([500.0, 450.0])
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .order(egui::Order::Foreground) // Always on top
            .frame(
                egui::Frame::window(&ctx.style())
                    .fill(bg_color)
                    .corner_radius(egui::CornerRadius::same(12)),
            )
            .show(ctx, |ui| {
                ui.with_layout(egui::Layout::top_down(egui::Align::Center), |ui| {
                    ui.add_space(30.0);

                    // 标题: 18px strong, 等待态紫色强调 / 成功绿 / 失败红 (语义色)
                    match &self.state {
                        ActivationState::WaitingForPress | ActivationState::WaitingForRelease => {
                            let time = ui.input(|i| i.time);
                            let bounce = (time * 2.0).sin() * 3.0;
                            ui.add_space(bounce as f32);

                            ui.label(
                                egui::RichText::new(t.hid_activation_title())
                                    .size(18.0)
                                    .color(accent_color)
                                    .strong(),
                            );
                        }
                        ActivationState::Success => {
                            ui.label(
                                egui::RichText::new(t.hid_activation_success_title())
                                    .size(18.0)
                                    .color(success_color)
                                    .strong(),
                            );
                        }
                        ActivationState::Failed(_) => {
                            ui.label(
                                egui::RichText::new(t.hid_activation_failed_title())
                                    .size(18.0)
                                    .color(danger_color)
                                    .strong(),
                            );
                        }
                    }

                    ui.add_space(20.0);

                    // Device name card with enhanced information
                    egui::Frame::NONE
                        .fill(card_bg)
                        .corner_radius(egui::CornerRadius::same(14))
                        .inner_margin(egui::Margin::same(16))
                        .show(ui, |ui| {
                            ui.set_min_width(ui.available_width());

                            ui.with_layout(egui::Layout::top_down(egui::Align::Center), |ui| {
                                // Display device icon based on type
                                let device_type = get_hid_device_type(self.usage_page, self.usage);
                                let device_icon = match device_type {
                                    "Gamepad" | "Joystick" | "Multi-axis Controller" => "🎮",
                                    "Keyboard" => "⌨",
                                    "Mouse" => "🖱",
                                    _ => "📱",
                                };

                                ui.label(egui::RichText::new(device_icon).size(20.0));
                                ui.add_space(6.0);

                                // Display model name or device name
                                let display_name =
                                    if let Some(model) = get_device_model(self.vid, self.pid) {
                                        model.to_string()
                                    } else if !self.device_name.is_empty() {
                                        self.device_name.clone()
                                    } else {
                                        device_type.to_string()
                                    };

                                ui.label(
                                    egui::RichText::new(&display_name)
                                        .size(16.0)
                                        .strong()
                                        .color(text_color),
                                );

                                ui.add_space(4.0);

                                // Display vendor and technical info
                                let mut info_parts = vec![];
                                if let Some(vendor) = get_vendor_name(self.vid) {
                                    info_parts.push(vendor.to_string());
                                }
                                info_parts.push(format!("{:04X}:{:04X}", self.vid, self.pid));

                                if !info_parts.is_empty() {
                                    ui.label(
                                        egui::RichText::new(info_parts.join(" │ "))
                                            .size(12.0)
                                            .color(text_secondary),
                                    );
                                }
                            });
                        });

                    ui.add_space(25.0);

                    // State-specific content
                    match &self.state {
                        ActivationState::WaitingForPress => {
                            ui.label(
                                egui::RichText::new(t.hid_activation_press_prompt())
                                    .size(16.0)
                                    .color(accent_color)
                                    .strong(),
                            );

                            ui.add_space(15.0);

                            // Warning box
                            egui::Frame::NONE
                                .fill(warning_bg)
                                .corner_radius(egui::CornerRadius::same(12))
                                .inner_margin(egui::Margin::same(16))
                                .stroke(egui::Stroke::new(1.0, warning_color))
                                .show(ui, |ui| {
                                    ui.vertical_centered(|ui| {
                                        ui.label(
                                            egui::RichText::new(t.hid_activation_warning_title())
                                                .size(15.0)
                                                .color(warning_color)
                                                .strong(),
                                        );
                                        ui.add_space(8.0);
                                        ui.label(
                                            egui::RichText::new(t.hid_activation_warning_1())
                                                .size(13.0)
                                                .color(text_color),
                                        );
                                        ui.label(
                                            egui::RichText::new(t.hid_activation_warning_2())
                                                .size(13.0)
                                                .color(text_color),
                                        );
                                        ui.label(
                                            egui::RichText::new(t.hid_activation_warning_3())
                                                .size(13.0)
                                                .color(text_color),
                                        );
                                    });
                                });
                        }

                        ActivationState::WaitingForRelease => {
                            let time = ui.input(|i| i.time);
                            let pulse = ((time * 3.0).sin() + 1.0) / 2.0;
                            let pulse_color = egui::Color32::from_rgb(
                                (110.0 + 90.0 * pulse) as u8,
                                (231.0 + 24.0 * pulse) as u8,
                                (183.0 + 60.0 * pulse) as u8,
                            );

                            ui.label(
                                egui::RichText::new(t.hid_activation_release_prompt())
                                    .size(16.0)
                                    .color(pulse_color)
                                    .strong(),
                            );
                        }

                        ActivationState::Success => {
                            if let Some(success_time) = self.success_time {
                                self.animation_progress =
                                    success_time.elapsed().as_secs_f32().min(2.0);
                            } else {
                                self.success_time = Some(Instant::now());
                            }

                            // Stars animation
                            let stars = "✨ ⭐ 💫 🌟 ✨ ⭐ 💫 🌟";
                            ui.label(egui::RichText::new(stars).size(24.0).color(success_color));

                            ui.add_space(10.0);

                            ui.label(
                                egui::RichText::new(t.hid_activation_success_message())
                                    .size(16.0)
                                    .color(text_color),
                            );

                            ui.label(
                                egui::RichText::new(t.hid_activation_success_hint())
                                    .size(14.0)
                                    .color(text_color),
                            );

                            if self.animation_progress >= 1.0 {
                                ui.add_space(15.0);
                                ui.label(
                                    egui::RichText::new(t.hid_activation_auto_close())
                                        .size(12.0)
                                        .color(text_secondary)
                                        .italics(),
                                );

                                if self.animation_progress >= 2.0 {
                                    should_close = true;
                                }
                            }
                        }

                        ActivationState::Failed(error_msg) => {
                            ui.label(
                                egui::RichText::new(format!(
                                    "{}: {}",
                                    t.hid_activation_error(),
                                    error_msg
                                ))
                                .size(14.0)
                                .color(danger_color),
                            );

                            ui.add_space(15.0);

                            let retry_btn = egui::Button::new(
                                egui::RichText::new(t.hid_activation_retry())
                                    .size(15.0)
                                    .color(egui::Color32::WHITE),
                            )
                            .fill(accent_color)
                            .corner_radius(12.0);

                            if ui.add_sized([140.0, 36.0], retry_btn).clicked() {
                                self.state = ActivationState::WaitingForPress;
                                self.pressed_data = None;
                                self.released_data = None;
                            }
                        }
                    }

                    ui.add_space(30.0);

                    // Bottom buttons
                    if matches!(
                        self.state,
                        ActivationState::WaitingForPress | ActivationState::WaitingForRelease
                    ) {
                        ui.add_space(20.0);

                        // 取消按钮: 强调紫填充 + 圆角12 + 高度36
                        let cancel_btn = egui::Button::new(
                            egui::RichText::new(t.hid_activation_cancel())
                                .size(15.0)
                                .color(egui::Color32::WHITE)
                                .strong(),
                        )
                        .fill(accent_color)
                        .corner_radius(12.0);

                        if ui.add_sized([260.0, 36.0], cancel_btn).clicked() {
                            should_close = true;
                        }
                    }
                });
            });

        ctx.request_repaint();

        should_close
    }

    /// Handle incoming HID data during activation
    #[inline]
    pub fn handle_hid_data(&mut self, data: &[u8]) {
        const MAX_CHANGE_BITS: u32 = 24; // 单个按键 + 少量噪声/扳机漂移容差
        match self.state {
            ActivationState::WaitingForPress => {
                match &self.pressed_data {
                    // 第一帧作为"空闲参照"暂存, 等待用户按下时出现差异
                    None => self.pressed_data = Some(data.to_vec()),
                    Some(reference) => {
                        let diff = xor_diff_bits(reference, data);
                        // 检测到小幅变化 = 按键按下(极性无关: 高/低电平有效均可)
                        if (1..=MAX_CHANGE_BITS).contains(&diff) {
                            self.pressed_data = Some(data.to_vec());
                            self.state = ActivationState::WaitingForRelease;
                        }
                        // diff == 0(未变化)或过大(多键/大幅噪声) → 继续等待
                    }
                }
            }
            ActivationState::WaitingForRelease => {
                if let Some(pressed) = &self.pressed_data {
                    let diff = xor_diff_bits(pressed, data);
                    // 检测到小幅变化 = 按键松开, 该帧即空闲基线(极性无关)
                    if (1..=MAX_CHANGE_BITS).contains(&diff) {
                        self.released_data = Some(data.to_vec());
                        self.state = ActivationState::Success;
                    }
                }
            }
            _ => {}
        }
    }

    /// Get the established baseline data
    #[inline]
    pub fn get_baseline(&self) -> Option<Vec<u8>> {
        if matches!(self.state, ActivationState::Success) {
            self.released_data.clone()
        } else {
            None
        }
    }
}

/// 计算两帧数据在比较区(跳过协议头与模拟轴)内的差分位数。
/// 与"置位数多少"无关 —— 兼容高电平有效(按下=1)与低电平有效(按下=0)两种手柄布局。
#[inline]
fn xor_diff_bits(data1: &[u8], data2: &[u8]) -> u32 {
    const SKIP_BYTES: usize = 5; // 跳过协议头与模拟轴
    data1
        .iter()
        .zip(data2.iter())
        .skip(SKIP_BYTES)
        .map(|(b1, b2)| (b1 ^ b2).count_ones())
        .sum()
}
