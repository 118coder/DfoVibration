//! 概览页中央的分区圆盘：扇形导航 + 项目原有的 360 手柄线稿。
//! 扇形用「圆心到外弧」的凸多边形绘制，再盖一层内圆做成环，避免凹多边形三角化出错。

use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, Stroke, TextureHandle};
use crate::ui_api::UiPage;
use super::palette::EfPalette;

fn polar(c: Pos2, r: f32, deg: f32) -> Pos2 {
    let a = deg.to_radians();
    Pos2::new(c.x + r * a.cos(), c.y + r * a.sin())
}

fn wedge(c: Pos2, r: f32, a0: f32, a1: f32) -> Vec<Pos2> {
    // 圆心 + 外弧，角度跨度 < 180°，一定是凸多边形。
    let mut pts = Vec::with_capacity(15);
    pts.push(c);
    for i in 0..=12 {
        pts.push(polar(c, r, a0 + (a1 - a0) * i as f32 / 12.0));
    }
    pts
}

/// 画圆盘；点击扇区把目标页 id 写进 `on_nav`，由外壳统一切路由。
pub fn draw(
    ui: &mut egui::Ui,
    pal: &EfPalette,
    pages: &[UiPage],
    current: &str,
    pad: Option<&TextureHandle>,
    on_nav: &mut Option<String>,
) {
    let size = ui.available_width().min(ui.available_height()).max(220.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    let c = rect.center();
    let r_out = size * 0.455;
    let r_in = size * 0.295;

    // 外圈墨线 + 刻度
    painter.circle_stroke(c, r_out + 7.0, Stroke::new(1.0, pal.line));
    for i in 0..72 {
        let a = i as f32 * 5.0 - 90.0;
        let long = i % 9 == 0;
        let p0 = polar(c, r_out + 9.0, a);
        let p1 = polar(c, r_out + if long { 17.0 } else { 13.0 }, a);
        painter.line_segment([p0, p1], Stroke::new(1.0, pal.ink_dim));
    }

    let step = 360.0 / pages.len() as f32;
    for (i, page) in pages.iter().enumerate() {
        let a0 = -90.0 + i as f32 * step + 1.2;
        let a1 = -90.0 + (i + 1) as f32 * step - 1.2;
        let mid = -90.0 + (i as f32 + 0.5) * step;
        let label_pos = polar(c, (r_in + r_out) * 0.5, mid);
        let hit = Rect::from_center_size(label_pos, egui::vec2(size * 0.17, size * 0.085));
        let resp = ui.interact(hit, ui.id().with(("ef-dial", page.id)), egui::Sense::click());
        if resp.clicked() {
            *on_nav = Some(page.id.to_string());
        }
        let here = page.id == current;
        let active = here || resp.hovered();
        let fill = if active { pal.accent_soft } else { pal.panel };
        painter.add(egui::Shape::convex_polygon(
            wedge(c, r_out, a0, a1),
            fill,
            Stroke::new(1.0, if active { pal.accent } else { pal.line }),
        ));
        painter.text(
            label_pos,
            Align2::CENTER_CENTER,
            page.zh,
            FontId::proportional(size * 0.030),
            if active { pal.accent } else { pal.ink },
        );
        painter.text(
            polar(c, r_out - size * 0.045, mid),
            Align2::CENTER_CENTER,
            page.en,
            FontId::proportional(size * 0.017),
            pal.ink_dim,
        );
    }

    // 内圆盖住扇形根部 → 环形；再放手柄线稿
    painter.circle_filled(c, r_in, pal.paper);
    painter.circle_stroke(c, r_in, Stroke::new(1.0, pal.line));
    if let Some(tex) = pad {
        let w = r_in * 1.72;
        let h = w * 401.0 / 545.0;
        let img = Rect::from_center_size(c, egui::vec2(w, h));
        painter.image(
            tex.id(),
            img,
            Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            Color32::WHITE,
        );
    } else {
        painter.text(
            c,
            Align2::CENTER_CENTER,
            "手柄",
            FontId::proportional(size * 0.04),
            pal.ink_dim,
        );
    }
}
