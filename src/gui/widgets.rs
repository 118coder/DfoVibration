//! 复合组件库: 自绘矢量图标 + 页面骨架组件。
//!
//! 图标全部用 painter 矢量绘制 (而非 emoji), 保证跨主题、跨字体的
//! 一致观感; 组件均为无状态函数, 状态由调用方持有。
//! 绘制约定: 线宽 = (s*0.09).max(1.5), 图标细节孔洞用图标色 gamma_multiply(0.4)
//! (双主题成立); 凸多边形才可走 convex_polygon。

use crate::gui::theme::{RADIUS_CTRL, RADIUS_NAV, SP_2XL, SP_L, SP_M, SP_S, SP_XS, Theme};
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
    /// ★v24.50 音频震动页 (声波)
    SoundWave,
    /// ★v24.40b 关闭 ✕ (弹窗角标)
    X,
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
            Icon::SoundWave => ic::WAVEFORM,
            Icon::X => ic::X,
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
/// ★v24.40b 关闭 ✕ 小圆钮 (弹窗右上角)。
pub fn icon_x(ui: &mut egui::Ui, th: &Theme) -> egui::Response {
    icon_button(ui, th, Icon::X, "关闭")
}

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

/// 开/关拨动开关 (46×26 胶囊 / 20px 滑块 / 120ms 过渡)。
///
/// 用于"长期状态"型开关 (总开关), 比一个大按钮更符合直觉: 位置本身就是状态。
/// 点击自动翻转 `*on` 并 `mark_changed()`; 焦点环/指针光标与其它自绘控件一致。
pub fn switch(ui: &mut egui::Ui, th: &Theme, on: &mut bool) -> egui::Response {
    let size = egui::vec2(46.0, 26.0);
    let (rect, mut response) = ui.allocate_exact_size(size, egui::Sense::click());
    response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }

    let anim = ui.ctx().animate_bool_with_time(
        egui::Id::new("switch").with(rect.min.x.to_bits()).with(rect.min.y.to_bits()),
        *on,
        0.12,
    );

    // 轨道: 关 = faint, 开 = accent; 悬停整体提亮一档
    let mut track = lerp_color(th.faint, th.accent, anim);
    if response.hovered() {
        track = lerp_color(track, egui::Color32::WHITE, if th.dark { 0.10 } else { 0.05 });
    }
    ui.painter().rect_filled(rect, size.y / 2.0, track);

    // 滑块: 关 = 中性灰, 开 = 白
    let knob_r = 10.0;
    let x = egui::lerp(
        (rect.left() + 3.0 + knob_r)..=(rect.right() - 3.0 - knob_r),
        anim,
    );
    ui.painter().circle_filled(
        egui::pos2(x, rect.center().y),
        knob_r,
        lerp_color(th.text_weak, egui::Color32::WHITE, anim),
    );

    if response.has_focus() {
        ui.painter().rect_stroke(
            rect.expand(2.0),
            size.y / 2.0 + 2.0,
            egui::Stroke::new(2.0, th.accent),
            egui::StrokeKind::Outside,
        );
    }
    response
}

/// 两色线性插值 (不透明, 用于开关过渡这类必须逐帧算的颜色)。
fn lerp_color(a: egui::Color32, b: egui::Color32, t: f32) -> egui::Color32 {
    let t = t.clamp(0.0, 1.0);
    let mix = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    egui::Color32::from_rgb(mix(a.r(), b.r()), mix(a.g(), b.g()), mix(a.b(), b.b()))
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

// ───────────────────────── 表单行组件 (v24.61 音频/震动页共用) ─────────────────────────
//
// 一套行 = 图标 + 名称 + 右对齐数值 / 满宽滑轨 / 一句人话说明。
// 目的: 全应用的参数页只有一种行样式 —— 版式统一靠组件, 不靠各页各写一遍。
// 说明文字是否显示由页面在帧首用 `set_form_hints` 决定 (默认显示, 关掉即紧凑模式)。

/// 表单说明开关 (页面帧首设一次, 行组件读)。
fn hints_id() -> egui::Id {
    egui::Id::new("wb_form_hints")
}

/// 设置本帧是否显示参数说明 (11.5px 小字)。缺省 = 显示。
pub fn set_form_hints(ui: &mut egui::Ui, show: bool) {
    ui.data_mut(|d| d.insert_temp(hints_id(), show));
}

fn form_hints(ui: &egui::Ui) -> bool {
    ui.data(|d| d.get_temp::<bool>(hints_id()).unwrap_or(true))
}

/// 参数行描述 (链式构造, 避免 10 参数函数)。
pub struct Row<'a> {
    icon: &'a str,
    label: &'a str,
    tag: &'a str,
    hint: &'a str,
    tip: &'a str,
    suffix: &'a str,
    enabled: bool,
    integer: bool,
}

impl<'a> Row<'a> {
    pub fn new(icon: &'a str, label: &'a str) -> Self {
        Self {
            icon,
            label,
            tag: "",
            hint: "",
            tip: "",
            suffix: "",
            enabled: true,
            integer: false,
        }
    }
    /// 名称后的弱化小标记 (实时计数等诊断信息)。
    pub fn tag(mut self, t: &'a str) -> Self {
        self.tag = t;
        self
    }
    /// 滑轨下的一句人话 (紧凑模式隐藏)。
    pub fn hint(mut self, h: &'a str) -> Self {
        self.hint = h;
        self
    }
    /// 悬停完整解释。
    pub fn tip(mut self, t: &'a str) -> Self {
        self.tip = t;
        self
    }
    pub fn suffix(mut self, s: &'a str) -> Self {
        self.suffix = s;
        self
    }
    /// 置灰 (受上级开关约束时), 仍可看值。
    pub fn off(mut self) -> Self {
        self.enabled = false;
        self
    }
    /// 整数步进。
    pub fn int(mut self) -> Self {
        self.integer = true;
        self
    }
}

fn fmt_val(v: f32, integer: bool) -> String {
    if integer {
        format!("{}", v.round() as i64)
    } else if (v - v.round()).abs() < 0.05 {
        format!("{}", v.round() as i64)
    } else {
        format!("{v:.1}")
    }
}

/// 数值胶囊: 让"这是当前值"和"这是说明文字"一眼分开。
fn value_chip(ui: &mut egui::Ui, th: &Theme, text: &str, active: bool) {
    let fg = if active { th.accent_text } else { th.text_weak };
    let bg = if active { th.faint } else { egui::Color32::TRANSPARENT };
    egui::Frame::NONE
        .fill(bg)
        .corner_radius(egui::CornerRadius::same(6))
        .inner_margin(egui::Margin::symmetric(8, 2))
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new(text)
                    .size(12.0)
                    .strong()
                    .family(Theme::font_bold())
                    .color(fg),
            );
        });
}

/// 自绘滑轨 (替代 `egui::Slider` 的默认皮肤)。
///
/// 为什么自绘: egui 滑块的**轨道与手柄共用 `widgets.inactive.bg_fill` 同一个颜色**,
/// 深色卡片上就是"一条灰线顶着同色圆点", 用户反馈原话——"不然用户怎么知道哪些参数
/// 是可以调的"。这里把 轨道 / 已选段 / 手柄 拆成三层色, 悬停再加光晕与横向拖拽光标,
/// 让"这是控件"不靠猜。
///
/// 交互与原 Slider 对齐: 点轨道跳转、拖手柄连续改、方向键步进、Home/End 到端点。
pub fn rail(
    ui: &mut egui::Ui,
    th: &Theme,
    value: &mut f32,
    range: std::ops::RangeInclusive<f32>,
    step: f64,
    accent: egui::Color32,
    height: f32,
) -> egui::Response {
    let (lo, hi) = (*range.start(), *range.end());
    let span = (hi - lo).max(f32::EPSILON);
    let width = ui.available_width().max(60.0);
    let (rect, mut response) =
        ui.allocate_exact_size(egui::vec2(width, height.max(20.0)), egui::Sense::click_and_drag());
    let enabled = ui.is_enabled();
    let st = (step as f32).max(0.0001);

    let knob_r = (height * 0.34).clamp(6.0, 9.5);
    let usable = (rect.width() - knob_r * 2.0).max(1.0);

    /* ── 指针拖动 ── */
    if enabled && (response.dragged() || response.clicked()) {
        ui.memory_mut(|m| m.request_focus(response.id));
        if let Some(p) = response.interact_pointer_pos() {
            let t = ((p.x - rect.left() - knob_r) / usable).clamp(0.0, 1.0);
            let snapped = (((lo + span * t) / st).round() * st).clamp(lo, hi);
            if (snapped - *value).abs() > st * 0.25 {
                *value = snapped;
                response.mark_changed();
            }
        }
    }

    /* ── 键盘 (与原 Slider 一致: ←/→ 一步, Home/End 到端点) ── */
    if enabled && response.has_focus() {
        let (left, right, home, end) = ui.input(|i| {
            (
                i.key_pressed(egui::Key::ArrowLeft),
                i.key_pressed(egui::Key::ArrowRight),
                i.key_pressed(egui::Key::Home),
                i.key_pressed(egui::Key::End),
            )
        });
        let mut v = *value;
        if right {
            v += st;
        }
        if left {
            v -= st;
        }
        if home {
            v = lo;
        }
        if end {
            v = hi;
        }
        let v = (((v / st).round() * st).clamp(lo, hi));
        if (v - *value).abs() > f32::EPSILON {
            *value = v;
            response.mark_changed();
        }
    }
    *value = value.clamp(lo, hi);

    /* ── 绘制 ── */
    if ui.is_rect_visible(rect) {
        let hot = ui
            .ctx()
            .animate_bool_with_time(response.id.with("hot"), response.hovered() || response.dragged(), 0.12);
        let cy = rect.center().y;
        let track_h = 5.0;
        let track = egui::Rect::from_center_size(
            egui::pos2(rect.center().x, cy),
            egui::vec2(rect.width(), track_h),
        );
        let rail_bg = if th.dark {
            th.faint.gamma_multiply(1.25)
        } else {
            egui::Color32::from_rgb(203, 209, 222)
        };
        ui.painter().rect_filled(track, track_h / 2.0, rail_bg);

        let t = ((*value - lo) / span).clamp(0.0, 1.0);
        let kx = rect.left() + knob_r + usable * t;
        let fill = if enabled {
            accent
        } else {
            th.text_weak.gamma_multiply(0.5)
        };
        /* 双极滑轨 (±100 的 L/R 权重): 从**中点**向两侧填, 0 = 无填充;
         * 单极滑轨 (0..100): 从左端填。混着用会让"0"看起来像"填了一半"。 */
        let bipolar = lo < 0.0 && hi > 0.0;
        let base_x = if bipolar {
            rect.left() + knob_r + usable * ((0.0 - lo) / span).clamp(0.0, 1.0)
        } else {
            rect.left()
        };
        let mut done = track;
        done.min.x = base_x.min(kx);
        done.max.x = base_x.max(kx);
        if (done.width()) > 0.5 {
            ui.painter().rect_filled(done, track_h / 2.0, fill);
        }
        if bipolar {
            /* 中点刻度: 让"回到 0"有个落点 */
            let cx = rect.left() + knob_r + usable * 0.5;
            ui.painter().vline(
                cx,
                (cy - 5.0)..=(cy + 5.0),
                egui::Stroke::new(1.0, th.stroke_strong.gamma_multiply(0.7)),
            );
        }

        /* 手柄: 卡片底实心圆 + 强调描边 (与轨道明确分层) + 悬停光晕 */
        let knob_fill = if th.dark { th.card } else { egui::Color32::WHITE };
        if hot > 0.01 {
            ui.painter()
                .circle_filled(egui::pos2(kx, cy), knob_r + 5.0 * hot, fill.gamma_multiply(0.22 * hot));
        }
        let r = knob_r + hot * 0.8;
        ui.painter().circle_filled(egui::pos2(kx, cy), r, knob_fill);
        ui.painter().circle_stroke(
            egui::pos2(kx, cy),
            r,
            egui::Stroke::new(2.0, fill),
        );

        if response.has_focus() {
            ui.painter().rect_stroke(
                rect.shrink(2.0),
                egui::CornerRadius::same(6),
                egui::Stroke::new(1.0, th.accent.gamma_multiply(0.6)),
                egui::StrokeKind::Inside,
            );
        }
    }

    if enabled {
        response = response.on_hover_cursor(egui::CursorIcon::ResizeHorizontal);
    }
    response
}

/// 参数卡: 每个参数一张独立小卡 (圆角 10 / card_alt 底 / hairline 描边)。
///
/// ★v24.63 用户要求"每个选项加入卡片, 辨认度才高": 原先靠 1px 分隔线切行,
/// 在密集参数页里仍然糊成一片; 现在一行 = 一张卡, 卡与卡之间 8px 间距,
/// 视线自然按"块"跳, 不再需要逐行找起点。悬停时描边提亮一档。
fn card_fill(th: &Theme) -> egui::Color32 {
    th.card_alt
}

pub fn row_card(ui: &mut egui::Ui, th: &Theme, add: impl FnOnce(&mut egui::Ui)) {
    let resp = egui::Frame::NONE
        .fill(card_fill(th))
        .stroke(egui::Stroke::new(1.0, th.stroke))
        .corner_radius(egui::CornerRadius::same(10))
        .inner_margin(egui::Margin::symmetric(10, 8))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            add(ui);
        });
    if resp.response.hovered() {
        ui.painter().rect_stroke(
            resp.response.rect,
            egui::CornerRadius::same(10),
            egui::Stroke::new(1.0, th.stroke_strong),
            egui::StrokeKind::Inside,
        );
    }
    ui.add_space(SP_S);
}

/// 行体: (图标 名称 … 数值胶囊) + 控件 + 说明。卡片由 `row_card` 负责。
fn row_body(
    ui: &mut egui::Ui,
    th: &Theme,
    row: &Row,
    text: String,
    add_control: impl FnOnce(&mut egui::Ui) -> egui::Response,
) -> egui::Response {
    let active = row.enabled && ui.is_enabled();
    let label_color = if active { th.text } else { th.hint };

    ui.horizontal(|ui| {
        if !row.icon.is_empty() {
            let c = if active { th.accent } else { th.hint };
            ui.label(egui::RichText::new(row.icon).size(13.0).color(c));
        }
        /* 名称占住"扣掉数值胶囊之后"的宽度, 超长就省略号 (悬停有全文):
         * 否则长名字会把胶囊顶出卡片, 整页最小宽度被撑大 → 右侧被裁 (1080 窗口实测) */
        let chip_w = 20.0 + text.chars().count() as f32 * 7.6;
        let name_w = (ui.available_width() - chip_w - 6.0).max(60.0);
        ui.scope(|ui| {
            ui.set_max_width(name_w);
            ui.horizontal(|ui| {
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(row.label).size(12.5).color(label_color),
                    )
                    .truncate(),
                );
                if !row.tag.is_empty() {
                    ui.add(
                        egui::Label::new(egui::RichText::new(row.tag).size(11.0).color(th.hint))
                            .truncate(),
                    );
                }
            });
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            value_chip(ui, th, &text, active);
        });
    });
    ui.add_space(3.0);

    let resp = add_control(ui);
    let resp = if row.tip.is_empty() {
        resp
    } else {
        resp.on_hover_text(row.tip)
    };
    if form_hints(ui) && !row.hint.is_empty() {
        ui.label(egui::RichText::new(row.hint).size(11.5).color(th.hint));
    }
    resp
}

/// 浮点参数行 (自带一张卡)。
pub fn row_f32(
    ui: &mut egui::Ui,
    th: &Theme,
    row: &Row,
    value: &mut f32,
    range: std::ops::RangeInclusive<f32>,
    step: f64,
) -> bool {
    let mut changed = false;
    row_card(ui, th, |ui| {
        changed = row_f32_inner(ui, th, row, value, range, step);
    });
    changed
}

/// 浮点参数行 (只画行体, 卡片由调用方提供 —— 需要把 L/R 等附加内容一起装进同一张卡时用)。
pub fn row_f32_inner(
    ui: &mut egui::Ui,
    th: &Theme,
    row: &Row,
    value: &mut f32,
    range: std::ops::RangeInclusive<f32>,
    step: f64,
) -> bool {
    if !row.enabled {
        *value = value.clamp(*range.start(), *range.end());
    }
    let text = format!("{}{}", fmt_val(*value, row.integer), row.suffix);
    let mut changed = false;
    ui.add_enabled_ui(row.enabled, |ui| {
        let resp = row_body(ui, th, row, text, |ui| {
            rail(ui, th, value, range.clone(), step, th.accent, 22.0)
        });
        changed = resp.changed();
    });
    changed
}

/// 整数参数行 (自带一张卡)。
pub fn row_i32(
    ui: &mut egui::Ui,
    th: &Theme,
    row: &Row,
    value: &mut i32,
    range: std::ops::RangeInclusive<i32>,
) -> bool {
    let mut changed = false;
    row_card(ui, th, |ui| {
        changed = row_i32_inner(ui, th, row, value, range);
    });
    changed
}

/// 整数参数行 (只画行体; 卡片由调用方提供)。
pub fn row_i32_inner(
    ui: &mut egui::Ui,
    th: &Theme,
    row: &Row,
    value: &mut i32,
    range: std::ops::RangeInclusive<i32>,
) -> bool {
    let text = if *value > 0 {
        format!("+{value}")
    } else {
        format!("{value}")
    };
    let mut changed = false;
    ui.add_enabled_ui(row.enabled, |ui| {
        let resp = row_body(ui, th, row, text, |ui| {
            let mut v = *value as f32;
            let r = rail(
                ui,
                th,
                &mut v,
                *range.start() as f32..=*range.end() as f32,
                1.0,
                th.accent,
                22.0,
            );
            if r.changed() {
                *value = v.round() as i32;
            }
            r
        });
        changed = resp.changed();
    });
    changed
}

/// 复选框行: 勾选 + 一句人话。
pub fn check_row(
    ui: &mut egui::Ui,
    th: &Theme,
    value: &mut bool,
    label: &str,
    hint: &str,
    tip: &str,
) -> bool {
    let changed = ui
        .checkbox(value, egui::RichText::new(label).size(12.5))
        .on_hover_text(tip)
        .changed();
    if form_hints(ui) && !hint.is_empty() {
        ui.label(egui::RichText::new(hint).size(11.5).color(th.hint));
    }
    changed
}

/// 等宽多列网格 (列间距 `gap`), 每列内容由 `add` 按列下标填充。
pub fn split_cols(ui: &mut egui::Ui, n: usize, gap: f32, mut add: impl FnMut(&mut egui::Ui, usize)) {
    let total = ui.available_width();
    let n_f = n as f32;
    let w = ((total - gap * (n_f - 1.0)) / n_f).max(120.0);
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        for i in 0..n {
            if i > 0 {
                ui.add_space(gap);
            }
            ui.vertical(|ui| {
                ui.set_width(w);
                ui.spacing_mut().item_spacing.x = SP_S;
                add(ui, i);
            });
        }
    });
}

/// 1px 分隔线 (占满当前宽度)。
pub fn hairline(ui: &mut egui::Ui, th: &Theme) {
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), egui::Sense::hover());
    ui.painter()
        .hline(rect.x_range(), rect.center().y, egui::Stroke::new(1.0, th.stroke));
}

/// 组内小标题 + 右侧延伸细线 (分组感靠这条线, 不靠色块)。
pub fn group_head(ui: &mut egui::Ui, th: &Theme, text: &str) {
    ui.add_space(SP_L);
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(text)
                .size(12.0)
                .strong()
                .family(Theme::font_bold())
                .color(th.text_weak),
        );
        let rest = ui.available_width();
        if rest > 8.0 {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(rest, 1.0), egui::Sense::hover());
            ui.painter()
                .hline(rect.x_range(), rect.center().y, egui::Stroke::new(1.0, th.stroke));
        }
    });
    ui.add_space(SP_S);
}

/// 键值小字 (状态行用): 左侧暗标签, 右侧亮数值, 单行不换行。
pub fn kv_line(ui: &mut egui::Ui, th: &Theme, key: &str, value: &str, color: egui::Color32) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(key).size(11.5).color(th.hint));
        ui.label(
            egui::RichText::new(value)
                .size(12.0)
                .strong()
                .color(color),
        );
    });
}
