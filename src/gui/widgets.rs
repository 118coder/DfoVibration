//! 复合组件库: 自绘矢量图标 + 页面骨架组件。
//!
//! 图标全部用 painter 矢量绘制 (而非 emoji), 保证跨主题、跨字体的
//! 一致观感; 组件均为无状态函数, 状态由调用方持有。
//! 绘制约定: 线宽 = (s*0.09).max(1.5), 图标细节孔洞用图标色 gamma_multiply(0.4)
//! (双主题成立); 凸多边形才可走 convex_polygon。

use crate::gui::theme::{RADIUS_CTRL, RADIUS_NAV, SP_2XL, SP_L, SP_S, SP_XS, Theme};
use eframe::egui;

// ───────────────────────── 矢量图标 ─────────────────────────

/// 可绘制的图标集合。
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Gamepad,
    Keyboard,
    Wave,
    Wand,
    Gear,
    Info,
    Question,
    Shield,
    Devices,
    Theme,
    Bolt,
    Play,
    Pause,
    Exit,
    Sliders,
}

impl Icon {
    /// Phosphor 图标字形 (统一图标语言)。
    pub fn glyph(self) -> &'static str {
        use egui_phosphor::variants::regular as ic;
        match self {
            Icon::Gamepad => ic::GAME_CONTROLLER,
            Icon::Keyboard => ic::KEYBOARD,
            Icon::Wave => ic::VIBRATE,
            Icon::Wand => ic::MAGIC_WAND,
            Icon::Gear => ic::GEAR_SIX,
            Icon::Info => ic::INFO,
            Icon::Question => ic::QUESTION,
            Icon::Shield => ic::SHIELD_CHECK,
            Icon::Devices => ic::DESKTOP,
            Icon::Theme => ic::MOON_STARS,
            Icon::Bolt => ic::LIGHTNING,
            Icon::Play => ic::PLAY,
            Icon::Pause => ic::PAUSE,
            Icon::Exit => ic::SIGN_OUT,
            Icon::Sliders => ic::SLIDERS_HORIZONTAL,
        }
    }

    /// 在给定矩形内以指定颜色绘制图标 (居中, 尺寸随矩形)。
    pub fn paint(&self, painter: &egui::Painter, rect: egui::Rect, color: egui::Color32) {
        let size = (rect.width().min(rect.height()) * 0.86).max(10.0);
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            self.glyph(),
            egui::FontId::proportional(size),
            color,
        );
    }
}

// ───────────────────────── 复合组件 ─────────────────────────

/// 侧边栏导航项: 34px 高, 选中态 = 左沿强调条 + 柔和底 + 图标文字同色,
/// 带平滑过渡动画 (Linear 手感)。
pub fn nav_item(ui: &mut egui::Ui, th: &Theme, icon: Icon, label: &str, selected: bool) -> bool {
    let desired = egui::vec2(ui.available_width(), 34.0);
    let (rect, response) = ui.allocate_exact_size(desired, egui::Sense::click());
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);

    // 选中过渡 0..1
    let anim_id = egui::Id::new("navsel").with(rect.min.x.to_bits()).with(rect.min.y.to_bits());
    let t = ui.ctx().animate_bool_with_time(anim_id, selected, 0.10);

    let hovered = response.hovered() && !selected;
    if t > 0.01 {
        // 选中软底: accent alpha 随动画插值
        let a = (36.0 * t) as u8;
        let bg = egui::Color32::from_rgba_unmultiplied(th.accent.r(), th.accent.g(), th.accent.b(), a);
        ui.painter().rect_filled(rect, RADIUS_NAV, bg);
    } else if hovered {
        ui.painter().rect_filled(rect, RADIUS_NAV, th.stroke);
    }

    // 选中态左沿强调条 (高度随动画)
    if t > 0.01 {
        let bar_h = 18.0 * t;
        let bar_rect = egui::Rect::from_center_size(
            egui::pos2(rect.left(), rect.center().y),
            egui::vec2(3.0, bar_h),
        );
        ui.painter().rect_filled(bar_rect, 2, th.accent);
    }

    // 图标
    let icon_rect = egui::Rect::from_center_size(
        egui::pos2(rect.left() + 24.0, rect.center().y),
        egui::vec2(18.0, 18.0),
    );
    let icon_color = if t > 0.5 {
        th.accent
    } else if hovered {
        th.text
    } else {
        th.text_weak
    };
    icon.paint(ui.painter(), icon_rect, icon_color);

    // 文字: 选中态与图标同色 (Fluent/Linear 语言)
    let text_color = if t > 0.5 {
        th.accent_text
    } else if hovered {
        th.text
    } else {
        th.text_weak
    };
    ui.painter().text(
        egui::pos2(rect.left() + 42.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(13.0),
        text_color,
    );

    // 键盘可达: 焦点环
    if response.has_focus() {
        ui.painter().rect_stroke(
            rect.expand(2.0),
            RADIUS_NAV + 2,
            egui::Stroke::new(2.0, th.accent),
            egui::StrokeKind::Outside,
        );
    }

    response.clicked()
}

/// 页面头部: 大标题 + 描述 (内容页统一入口)。
pub fn page_header(ui: &mut egui::Ui, th: &Theme, title: &str, desc: &str) {
    ui.label(th.h1(title));
    ui.add_space(SP_XS);
    ui.label(th.hint_text(desc));
    ui.add_space(SP_L);
}

/// 深色主题内容区顶部微光: accent 5% → 透明 的分层渐变,
/// 给近黑背景一层隐性的空间纵深 (Linear 同款手法)。浅色主题不绘制。
/// (用分层半透明矩形而非 Mesh: Mesh 默认 texture 为 Invalid, 会让整帧渲染失败变白屏)
pub fn paint_top_glow(ui: &mut egui::Ui, th: &Theme) {
    if !th.dark {
        return;
    }
    let rect = ui.max_rect();
    let h = 260.0_f32.min(rect.height());
    const BANDS: usize = 10;
    let band_h = h / BANDS as f32;
    for i in 0..BANDS {
        let alpha = 13u8.saturating_sub((i * 13 / BANDS) as u8); // 13 → 1
        if alpha == 0 {
            break;
        }
        let color = egui::Color32::from_rgba_unmultiplied(
            th.accent.r(),
            th.accent.g(),
            th.accent.b(),
            alpha,
        );
        let band = egui::Rect::from_min_size(
            egui::pos2(rect.left(), rect.top() + i as f32 * band_h),
            egui::vec2(rect.width(), band_h + 1.0), // +1 防接缝
        );
        ui.painter().rect_filled(band, 0, color);
    }
}

/// 键帽: 模拟按键的立体小方块, 用于一切按键名展示。
pub fn keycap(ui: &mut egui::Ui, th: &Theme, text: &str) -> egui::Response {
    egui::Frame::NONE
        .fill(th.extreme)
        .stroke(egui::Stroke::new(1.0, th.stroke_strong))
        .shadow(egui::epaint::Shadow {
            offset: [0, 1],
            blur: 0,
            spread: 0,
            color: egui::Color32::from_black_alpha(if th.dark { 110 } else { 40 }),
        })
        .corner_radius(egui::CornerRadius::same(6))
        .inner_margin(egui::Margin::symmetric(8, 4))
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new(text)
                    .size(12.0)
                    .strong()
                    .color(th.heading),
            );
        })
        .response
}

/// 按设备类型上色的键帽 (★v20.9.1 描边式: 中性键帽底 + 彩色细描边 + 彩色字)。
/// 紫=手柄 / 橙=鼠标 / 键盘=完全原样式; 让用户一眼分清触发/目标来自哪种设备,
/// 同时保留键帽本身的白/黑质感 (彩色块填充风格已否决, 勿回退)。
pub fn keycap_typed(
    ui: &mut egui::Ui,
    th: &Theme,
    text: &str,
    kind: crate::gui::utils::KeyKind,
) -> egui::Response {
    use crate::gui::utils::KeyKind;
    let fg = match kind {
        KeyKind::Gamepad => th.gamepad_fg,
        KeyKind::Mouse => th.mouse_fg,
        KeyKind::Keyboard => return keycap(ui, th, text),
    };
    egui::Frame::NONE
        .fill(th.extreme)
        .stroke(egui::Stroke::new(1.3, fg.gamma_multiply(0.8)))
        .shadow(egui::epaint::Shadow {
            offset: [0, 1],
            blur: 0,
            spread: 0,
            color: egui::Color32::from_black_alpha(if th.dark { 110 } else { 40 }),
        })
        .corner_radius(egui::CornerRadius::same(6))
        .inner_margin(egui::Margin::symmetric(8, 4))
        .show(ui, |ui| {
            ui.label(egui::RichText::new(text).size(12.0).strong().color(fg));
        })
        .response
}

/// 状态点 (呼吸动画, 主动请求重绘防止失焦冻结)。
pub fn status_dot(ui: &mut egui::Ui, color: egui::Color32, pulsing: bool, radius: f32) {
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(radius * 2.4, radius * 2.4),
        egui::Sense::hover(),
    );
    let t = if pulsing {
        ui.ctx()
            .animate_value_with_time(egui::Id::new("statusdot").with(rect.min.x.to_bits()).with(rect.min.y.to_bits()), 1.0, 0.6)
    } else {
        0.0
    };
    let glow = 1.0 + 0.7 * t;
    ui.painter()
        .circle_filled(rect.center(), radius * glow, color.gamma_multiply(0.25));
    ui.painter().circle_filled(rect.center(), radius, color);
    if pulsing {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(33));
    }
}

/// 图标按钮 (32px 命中区, 18px 图标, 带 tooltip)。
pub fn icon_button(ui: &mut egui::Ui, th: &Theme, icon: Icon, tip: &str) -> egui::Response {
    let (rect, mut response) = ui.allocate_exact_size(egui::vec2(32.0, 32.0), egui::Sense::click());
    response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    // 悬停底色渐入渐出 (150ms ease-out, Geist/Linear 微动效节奏)
    let hover_t = ui.ctx().animate_bool_with_time(
        egui::Id::new("icon_btn_hover").with(rect.min.x.to_bits()).with(rect.min.y.to_bits()),
        response.hovered(),
        0.15,
    );
    let bg = if response.clicked() {
        th.accent_soft
    } else if hover_t > 0.01 {
        let a = (hover_t * if th.dark { 26.0 } else { 24.0 }) as u8;
        egui::Color32::from_rgba_unmultiplied(th.faint.r(), th.faint.g(), th.faint.b(), a)
    } else {
        egui::Color32::TRANSPARENT
    };
    if bg != egui::Color32::TRANSPARENT {
        ui.painter().rect_filled(rect, RADIUS_CTRL, bg);
    }
    let color = if response.hovered() { th.heading } else { th.text_weak };
    icon.paint(
        ui.painter(),
        egui::Rect::from_center_size(rect.center(), egui::vec2(18.0, 18.0)),
        color,
    );
    if response.has_focus() {
        ui.painter().rect_stroke(
            rect.expand(2.0),
            RADIUS_CTRL + 2,
            egui::Stroke::new(2.0, th.accent),
            egui::StrokeKind::Outside,
        );
    }
    response.on_hover_text(tip)
}

/// 空状态: 大图标 + 标题 + 多行说明。
pub fn empty_state(ui: &mut egui::Ui, th: &Theme, icon: Icon, title: &str, lines: &[&str]) {
    ui.vertical_centered(|ui| {
        ui.set_min_width(ui.available_width());
        ui.add_space(SP_2XL);
        let (rect, _) = ui.allocate_exact_size(egui::vec2(48.0, 48.0), egui::Sense::hover());
        icon.paint(ui.painter(), rect, th.hint);
        ui.add_space(SP_S);
        ui.label(egui::RichText::new(title).size(15.0).strong().color(th.heading));
        ui.add_space(SP_XS);
        for l in lines {
            ui.label(th.hint_text(*l));
        }
        ui.add_space(SP_2XL);
    });
}

/// 统计块: 数值 + 标签 (用于状态 hero)。
pub fn stat(ui: &mut egui::Ui, th: &Theme, value: &str, label: &str, color: egui::Color32) {
    ui.vertical(|ui| {
        ui.set_min_width(64.0);
        ui.label(egui::RichText::new(value).size(18.0).strong().color(color));
        ui.label(th.hint_text(label));
    });
}
