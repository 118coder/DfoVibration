//! 终末地风格的明暗色板（egui 版）。
//! 网页端是 endfield.css 的 CSS 变量，这里是同一套语义色的 Rust 对照，
//! 换渲染器时视觉口径不变：纸白 / 墨线 / 黄标 / 三色条。

use eframe::egui::Color32;

#[derive(Debug, Clone, Copy)]
pub struct EfPalette {
    pub dark: bool,
    pub bg: Color32,
    pub paper: Color32,
    pub panel: Color32,
    pub panel_alt: Color32,
    pub ink: Color32,
    pub ink_dim: Color32,
    pub line: Color32,
    pub accent: Color32,
    pub accent_soft: Color32,
    pub alert: Color32,
    pub tri: [Color32; 3],
}

impl EfPalette {
    pub fn new(dark: bool) -> Self {
        if dark {
            Self {
                dark: true,
                bg: Color32::from_rgb(0x14, 0x16, 0x1A),
                paper: Color32::from_rgb(0x1B, 0x1E, 0x24),
                panel: Color32::from_rgb(0x23, 0x27, 0x30),
                panel_alt: Color32::from_rgb(0x2C, 0x31, 0x3B),
                ink: Color32::from_rgb(0xED, 0xED, 0xE8),
                ink_dim: Color32::from_rgb(0x9A, 0xA0, 0xAA),
                line: Color32::from_rgb(0x3A, 0x40, 0x48),
                accent: Color32::from_rgb(0xE8, 0xB2, 0x3A),
                accent_soft: Color32::from_rgb(0x3A, 0x33, 0x20),
                alert: Color32::from_rgb(0xD0, 0x5A, 0x4A),
                tri: [
                    Color32::from_rgb(0xE8, 0xB2, 0x3A),
                    Color32::from_rgb(0x5B, 0x8C, 0x7B),
                    Color32::from_rgb(0xC0, 0x4B, 0x3C),
                ],
            }
        } else {
            Self {
                dark: false,
                bg: Color32::from_rgb(0xE9, 0xE6, 0xDE),
                paper: Color32::from_rgb(0xF5, 0xF3, 0xEC),
                panel: Color32::from_rgb(0xFF, 0xFE, 0xFA),
                panel_alt: Color32::from_rgb(0xF0, 0xED, 0xE4),
                ink: Color32::from_rgb(0x1B, 0x1C, 0x1E),
                ink_dim: Color32::from_rgb(0x6A, 0x6B, 0x6A),
                line: Color32::from_rgb(0xC9, 0xC6, 0xBD),
                accent: Color32::from_rgb(0xD8, 0x9B, 0x1E),
                accent_soft: Color32::from_rgb(0xF6, 0xE6, 0xBE),
                alert: Color32::from_rgb(0xC0, 0x49, 0x3A),
                tri: [
                    Color32::from_rgb(0xD8, 0x9B, 0x1E),
                    Color32::from_rgb(0x4E, 0x7D, 0x6D),
                    Color32::from_rgb(0xB4, 0x43, 0x36),
                ],
            }
        }
    }
}
