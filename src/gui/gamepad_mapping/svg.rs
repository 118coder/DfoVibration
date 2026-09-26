//! 手柄 SVG 光栅化与热点几何 (★v24.36 后台预热/无字体扫描) —— 原 18-23 常量 + 627-676, C3 归位。

use eframe::egui;
use super::*;

/// SVG 渲染倍率 (2x 保证 HiDPI 下不模糊)。
pub(super) const SVG_RENDER_SCALE: f32 = 2.0;

/// SVG 原始尺寸 (viewBox), 用于计算显示宽高比。
pub(super) const SVG_SIZE: egui::Vec2 = egui::vec2(545.0, 401.0);


/// ★v24.36 光栅化手柄 SVG → egui 图像 (纯 CPU, **可在后台线程执行**)。
/// 手柄 SVG 不含 `<text>` (纯几何路径), 因此**不需要** `load_system_fonts()`
/// —— 旧代码在这里白扫了一遍全系统字体 (首次进页额外 ~100ms+)。
pub(crate) fn gamepad_color_image(dark: bool) -> Option<egui::ColorImage> {
    // 双主题变体: 亮色 = resources/gamepad-light.svg (仅 style 颜色不同, 几何一致,
    // 热点坐标共用同一 viewBox)
    let svg_data: &[u8] = if dark {
        include_bytes!("../../../resources/gamepad.svg")
    } else {
        include_bytes!("../../../resources/gamepad-light.svg")
    };

    let opt = resvg::usvg::Options::default();
    let tree = resvg::usvg::Tree::from_data(svg_data, &opt).ok()?;

    let size = tree.size().to_int_size();
    let scale = SVG_RENDER_SCALE;
    let width = (size.width() as f32 * scale) as u32;
    let height = (size.height() as f32 * scale) as u32;
    let mut pixmap = resvg::tiny_skia::Pixmap::new(width, height)?;
    let transform =
        resvg::tiny_skia::Transform::from_scale(scale, scale);
    resvg::render(&tree, transform, &mut pixmap.as_mut());

    let image_size = [pixmap.width() as usize, pixmap.height() as usize];
    /* ★v24.36 性能: tiny_skia 输出即预乘 RGBA, 直通避免逐像素反预乘 */
    Some(egui::ColorImage::from_rgba_premultiplied(
        image_size,
        pixmap.data(),
    ))
}

/// 使用 resvg 将内嵌手柄 SVG 渲染为 egui 纹理 (2x 超采样, HiDPI 清晰)。
pub fn load_gamepad_texture(ctx: &egui::Context, dark: bool) -> Option<egui::TextureHandle> {
    let color_image = gamepad_color_image(dark)?;
    Some(ctx.load_texture(
        if dark { "gamepad_svg_dark" } else { "gamepad_svg_light" },
        color_image,
        egui::TextureOptions::LINEAR,
    ))
}

/// 计算热点在指定显示矩形内的屏幕圆心与半径。
pub(super) fn hotspot_rect(rect: egui::Rect, slot: &GamepadSlot) -> (egui::Pos2, f32) {
    let center = egui::pos2(
        rect.min.x + slot.pos.0 * rect.width(),
        rect.min.y + slot.pos.1 * rect.height(),
    );
    let radius = slot.radius * rect.width();
    (center, radius)
}
