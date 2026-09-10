//! 统一设计系统 (Design System) v4 — "Violet Night 霓虹紫夜空" (ui-ux-pro-max 检索方向: 霓虹紫 + 深靛夜空)。
//! 规范文档: 源码根目录 design.md (新页面开发前必读)。
//!
//! 设计原则 (源自 egui 生态侦察 + Fluent/Linear 规范研究 + 设计审计):
//! 1. 三阶灰度骨架: bg(最深) → surface(侧栏/顶栏) → card(卡片) → card_alt(嵌套), 蓝调深灰,
//!    相邻层亮度差 ≥1.15 (Linear/Raycast 标准);
//! 2. 单一强调色: 全 UI 只有一个 indigo accent, 只出现在选中态/主按钮/激活滑块/状态点;
//! 3. 语义色低饱和 (Radix dark-11 系), 仅作状态标注, 不参与装饰;
//! 4. 8pt 间距网格 + 同心圆角族 (6/8/10/12);
//! 5. 对比度达标: 正文 ≥7:1, 12px 小字 ≥4.5:1 (微软雅黑 12px 起步才可读, egui 无 hinting);
//! 6. 一切颜色/字号/间距从这里取, 业务代码禁止散落字面量。

use eframe::egui;

// ───────────────────────── 间距 token (8pt 网格) ─────────────────────────

pub const SP_XS: f32 = 4.0;
pub const SP_S: f32 = 8.0;
pub const SP_M: f32 = 12.0;
pub const SP_L: f32 = 16.0;
#[allow(dead_code)]
pub const SP_XL: f32 = 20.0;
pub const SP_2XL: f32 = 24.0;

/// 卡片圆角
pub const RADIUS_CARD: u8 = 12;
/// 控件圆角
pub const RADIUS_CTRL: u8 = 8;
/// 徽章/小元素圆角
#[allow(dead_code)]
pub const RADIUS_CHIP: u8 = 6;
/// 侧边栏导航项圆角
pub const RADIUS_NAV: u8 = 10;

/// 明暗两套语义色板。
/// 色板要求明暗两套字段一一对应, 即使个别字段暂未被页面引用也必须保留。
#[allow(dead_code)]
#[derive(Debug, Clone, Copy)]
pub struct Theme {
    pub dark: bool,
    // ── 灰阶骨架 (三阶, 相邻亮度差 ≥1.15) ──
    /// 应用底色 (最深层)
    pub bg: egui::Color32,
    /// 侧边栏/顶栏面
    pub surface: egui::Color32,
    /// 卡片面
    pub card: egui::Color32,
    /// 卡片内嵌套面板 (raised)
    pub card_alt: egui::Color32,
    /// 悬停/条带底
    pub faint: egui::Color32,
    /// 输入框/键帽底
    pub extreme: egui::Color32,
    /// 描边 (hairline)
    pub stroke: egui::Color32,
    /// 强描边 (悬停边/浮层边)
    pub stroke_strong: egui::Color32,
    // ── 强调色 ──
    pub accent: egui::Color32,
    pub accent_hover: egui::Color32,
    /// 强调色柔化底 (选中行/软按钮)
    pub accent_soft: egui::Color32,
    /// 强调色文字变体 (标题/链接)
    pub accent_text: egui::Color32,
    // ── 文字层次 ──
    pub title: egui::Color32,
    pub heading: egui::Color32,
    pub text: egui::Color32,
    pub text_weak: egui::Color32,
    pub hint: egui::Color32,
    // ── 语义色 (Radix dark-11 / 低饱和) ──
    pub good: egui::Color32,
    pub good_soft: egui::Color32,
    pub bad: egui::Color32,
    pub bad_soft: egui::Color32,
    pub warn: egui::Color32,
    pub warn_soft: egui::Color32,
    pub info: egui::Color32,
    // ── 专用键帽/徽章 ──
    pub trigger_fg: egui::Color32,
    pub trigger_bg: egui::Color32,
    pub target_fg: egui::Color32,
    pub target_bg: egui::Color32,
    // ── 按钮档位 ──
    pub btn_primary: egui::Color32,
    pub btn_secondary: egui::Color32,
    pub btn_secondary_text: egui::Color32,
    pub btn_danger: egui::Color32,
    // ── 马达语义 ──
    pub motor_l: egui::Color32,
    pub motor_r: egui::Color32,
}

impl Theme {
    pub fn new(dark: bool) -> Self {
        if dark {
            Self::dark()
        } else {
            Self::light()
        }
    }

    /// 真粗体字体族 (fonts.rs 注册: Segoe UI Bold + 雅黑 Bold + Phosphor)。
    /// egui 的 `strong()` 只是变色, 标题层级必须用这个族才有真实字重。
    pub fn font_bold() -> egui::FontFamily {
        egui::FontFamily::Name("Bold".into())
    }

    /// 暗色: 深靛夜空三阶 + 霓虹紫强调 (v4, 设计系统检索方向)。
    pub fn dark() -> Self {
        Self {
            dark: true,
            bg: egui::Color32::from_rgb(15, 15, 35),        // #0F0F23 深靛夜空
            surface: egui::Color32::from_rgb(22, 21, 43),   // #16152B
            card: egui::Color32::from_rgb(30, 28, 53),      // #1E1C35 (raised)
            card_alt: egui::Color32::from_rgb(39, 37, 64),  // #272540
            faint: egui::Color32::from_rgb(44, 42, 72),     // #2C2A48 (悬停层)
            extreme: egui::Color32::from_rgb(10, 10, 26),   // #0A0A1A
            stroke: egui::Color32::from_rgba_unmultiplied(255, 255, 255, 22),        // 白 8.6% 分隔/描边
            stroke_strong: egui::Color32::from_rgba_unmultiplied(255, 255, 255, 50), // 白 20% 悬停/浮层
            accent: egui::Color32::from_rgb(139, 92, 246),    // #8B5CF6 霓虹紫 (暗底更亮)
            accent_hover: egui::Color32::from_rgb(167, 139, 250), // #A78BFA
            accent_soft: egui::Color32::from_rgba_unmultiplied(139, 92, 246, 46), // subtle bg
            accent_text: egui::Color32::from_rgb(196, 181, 253), // #C4B5FD
            title: egui::Color32::from_rgb(244, 242, 251),  // #F4F2FB
            heading: egui::Color32::from_rgb(226, 222, 239), // #E2DEEF
            text: egui::Color32::from_rgb(200, 196, 220),   // #C8C4DC
            text_weak: egui::Color32::from_rgb(162, 157, 189), // #A29DBD
            hint: egui::Color32::from_rgb(139, 134, 168),   // #8B86A8 (card 上 ≥5:1)
            good: egui::Color32::from_rgb(52, 211, 153),    // emerald-400
            good_soft: egui::Color32::from_rgba_unmultiplied(52, 211, 153, 26),
            bad: egui::Color32::from_rgb(244, 63, 94),      // #F43F5E (rose-500)
            bad_soft: egui::Color32::from_rgba_unmultiplied(244, 63, 94, 26),
            warn: egui::Color32::from_rgb(251, 191, 36),    // amber-400
            warn_soft: egui::Color32::from_rgba_unmultiplied(251, 191, 36, 26),
            info: egui::Color32::from_rgb(56, 189, 248),    // sky-400
            trigger_fg: egui::Color32::from_rgb(252, 211, 77),  // amber-300
            trigger_bg: egui::Color32::from_rgba_unmultiplied(252, 211, 77, 24),
            target_fg: egui::Color32::from_rgb(125, 211, 252),  // sky-300
            target_bg: egui::Color32::from_rgba_unmultiplied(125, 211, 252, 22),
            btn_primary: egui::Color32::from_rgb(124, 58, 237), // #7C3AED (白字 5.9:1)
            btn_secondary: egui::Color32::from_rgb(44, 42, 72), // faint
            btn_secondary_text: egui::Color32::from_rgb(214, 210, 232),
            btn_danger: egui::Color32::from_rgb(225, 29, 72), // #E11D48 (白字 4.7:1)
            motor_l: egui::Color32::from_rgb(251, 146, 60),   // orange-400
            motor_r: egui::Color32::from_rgb(248, 113, 113),  // red-400
        }
    }

    /// 亮色: 蓝白基调 (用户偏好) + 靛蓝强调 (深档保证对比)。
    pub fn light() -> Self {
        Self {
            dark: false,
            bg: egui::Color32::from_rgb(244, 245, 247),      // #F4F5F7
            surface: egui::Color32::from_rgb(236, 238, 241), // #ECEEF1
            card: egui::Color32::from_rgb(255, 255, 255),
            card_alt: egui::Color32::from_rgb(246, 247, 249),
            faint: egui::Color32::from_rgb(231, 233, 238),   // #E7E9EE
            extreme: egui::Color32::from_rgb(255, 255, 255),
            stroke: egui::Color32::from_rgb(226, 228, 231),   // #E2E4E7
            stroke_strong: egui::Color32::from_rgb(208, 213, 224), // #D0D5E0 hover 边
            accent: egui::Color32::from_rgb(79, 85, 232),    // #4F55E8 电光靛蓝 (白底 5.2:1)
            accent_hover: egui::Color32::from_rgb(91, 98, 242), // #5B62F2
            accent_soft: egui::Color32::from_rgba_unmultiplied(79, 85, 232, 26),
            accent_text: egui::Color32::from_rgb(63, 69, 208), // #3F45D0 (白底 6.4:1)
            title: egui::Color32::from_rgb(16, 19, 28),      // #10131C
            heading: egui::Color32::from_rgb(35, 40, 58),    // #23283A
            text: egui::Color32::from_rgb(62, 68, 83),       // #3E4453
            text_weak: egui::Color32::from_rgb(98, 106, 124), // #626A7C
            hint: egui::Color32::from_rgb(107, 115, 134),    // #6B7386 (白底 4.7:1)
            good: egui::Color32::from_rgb(22, 163, 74),      // green-600
            good_soft: egui::Color32::from_rgba_unmultiplied(22, 163, 74, 22),
            bad: egui::Color32::from_rgb(225, 29, 72),       // rose-600
            bad_soft: egui::Color32::from_rgba_unmultiplied(225, 29, 72, 20),
            warn: egui::Color32::from_rgb(217, 119, 6),      // amber-600
            warn_soft: egui::Color32::from_rgba_unmultiplied(217, 119, 6, 22),
            info: egui::Color32::from_rgb(2, 132, 199),      // sky-600
            trigger_fg: egui::Color32::from_rgb(180, 83, 9),   // amber-700
            trigger_bg: egui::Color32::from_rgba_unmultiplied(251, 191, 36, 42),
            target_fg: egui::Color32::from_rgb(3, 105, 161),   // sky-700
            target_bg: egui::Color32::from_rgba_unmultiplied(56, 189, 248, 40),
            btn_primary: egui::Color32::from_rgb(79, 85, 232), // #4F55E8 (白字 5.2:1)
            btn_secondary: egui::Color32::from_rgb(231, 233, 238),
            btn_secondary_text: egui::Color32::from_rgb(74, 80, 96),
            btn_danger: egui::Color32::from_rgb(225, 29, 72),
            motor_l: egui::Color32::from_rgb(234, 88, 12),    // orange-600
            motor_r: egui::Color32::from_rgb(220, 38, 38),    // red-600
        }
    }

    // ───────────────────────── 文本 ─────────────────────────

    /// 页面大标题 (Display 22 bold — 真粗体族)。
    pub fn h1(&self, s: impl Into<String>) -> egui::RichText {
        egui::RichText::new(s.into())
            .size(22.0)
            .strong()
            .family(Self::font_bold())
            .color(self.title)
    }

    /// 卡片标题 (Title 16 bold — 真粗体族)。
    pub fn h2(&self, s: impl Into<String>) -> egui::RichText {
        egui::RichText::new(s.into())
            .size(16.0)
            .strong()
            .family(Self::font_bold())
            .color(self.heading)
    }

    /// 分组小标题 (Heading 12 bold — 真粗体族)。
    #[allow(dead_code)]
    pub fn h3(&self, s: impl Into<String>) -> egui::RichText {
        egui::RichText::new(s.into())
            .size(12.0)
            .strong()
            .family(Self::font_bold())
            .color(self.text_weak)
    }

    /// 正文 (Body 13)。
    #[allow(dead_code)]
    pub fn body(&self, s: impl Into<String>) -> egui::RichText {
        egui::RichText::new(s.into()).size(13.0).color(self.text)
    }

    /// 次要正文 (12)。
    pub fn weak(&self, s: impl Into<String>) -> egui::RichText {
        egui::RichText::new(s.into()).size(12.0).color(self.text_weak)
    }

    /// 提示/说明 (Caption 12 — 微软雅黑 12px 起步才可读, egui 无 hinting)。
    pub fn hint_text(&self, s: impl Into<String>) -> egui::RichText {
        egui::RichText::new(s.into()).size(12.0).color(self.hint)
    }

    // ───────────────────────── 容器 ─────────────────────────

    /// 主卡片: 圆角 12 / 内边距 16 / hairline 描边; 亮色附极浅投影。
    pub fn card(
        &self,
        ui: &mut egui::Ui,
        title: Option<&str>,
        add_contents: impl FnOnce(&mut egui::Ui),
    ) {
        self.card_with_actions(ui, title, |_| {}, add_contents);
    }

    /// 带标题行动作区的卡片 (动作绘制在标题行右端)。
    pub fn card_with_actions(
        &self,
        ui: &mut egui::Ui,
        title: Option<&str>,
        actions: impl FnOnce(&mut egui::Ui),
        add_contents: impl FnOnce(&mut egui::Ui),
    ) {
        egui::Frame::NONE
            .fill(self.card)
            .stroke(egui::Stroke::new(1.0, self.stroke))
            .shadow(if self.dark {
                egui::epaint::Shadow::NONE
            } else {
                // M3 ambient 换算: offset(0,1~2) + blur 5~8 + 低 alpha, 柔和不脏
                egui::epaint::Shadow {
                    offset: [0, 2],
                    blur: 6,
                    spread: 0,
                    color: egui::Color32::from_black_alpha(18),
                }
            })
            .corner_radius(egui::CornerRadius::same(RADIUS_CARD))
            .inner_margin(egui::Margin::same(16))
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                if let Some(t) = title {
                    ui.horizontal(|ui| {
                        ui.label(self.h2(t));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), actions);
                    });
                    ui.add_space(SP_L);
                }
                add_contents(ui);
            });
        ui.add_space(SP_M);
    }

    /// 卡片内嵌套面板 (二级容器): 圆角 8 / 内边距 12 (同心圆角: 内径≈外径-内距)。
    pub fn panel(
        &self,
        ui: &mut egui::Ui,
        title: Option<&str>,
        add_contents: impl FnOnce(&mut egui::Ui),
    ) {
        egui::Frame::NONE
            .fill(self.card_alt)
            .stroke(egui::Stroke::new(1.0, self.stroke))
            .corner_radius(egui::CornerRadius::same(8))
            .inner_margin(egui::Margin::same(12))
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                if let Some(t) = title {
                    ui.label(self.h2(t));
                    ui.add_space(SP_S);
                }
                add_contents(ui);
            });
    }

    // ───────────────────────── 徽章 ─────────────────────────

    /// 通用徽章 (胶囊底色 + 彩色文字, 展示用)。
    pub fn badge(
        &self,
        ui: &mut egui::Ui,
        text: &str,
        fg: egui::Color32,
        bg: egui::Color32,
    ) -> egui::Response {
        egui::Frame::NONE
            .fill(bg)
            .corner_radius(egui::CornerRadius::same(100))
            .inner_margin(egui::Margin::symmetric(9, 3))
            .show(ui, |ui| {
                ui.label(egui::RichText::new(text).size(12.0).strong().color(fg));
            })
            .response
    }

    /// 可点击徽章 (用于"已配置按键"跳转等交互场景)。
    pub fn badge_clickable(
        &self,
        ui: &mut egui::Ui,
        text: &str,
        fg: egui::Color32,
        bg: egui::Color32,
    ) -> egui::Response {
        let rect = egui::Frame::NONE
            .fill(bg)
            .corner_radius(egui::CornerRadius::same(100))
            .inner_margin(egui::Margin::symmetric(9, 3))
            .show(ui, |ui| {
                ui.label(egui::RichText::new(text).size(12.0).strong().color(fg));
            })
            .response
            .rect;
        ui.interact(
            rect,
            egui::Id::new("badge_click").with(rect.min.x.to_bits()).with(rect.min.y.to_bits()),
            egui::Sense::click(),
        )
    }

    /// 触发键徽章 (琥珀)。
    pub fn trigger_badge(&self, ui: &mut egui::Ui, text: &str) -> egui::Response {
        self.badge(ui, text, self.trigger_fg, self.trigger_bg)
    }

    /// ★v20.6: 显性文本输入框 —— 描边清晰可辨, 让"这里可以输入"一眼可见。
    /// (原 TextEdit 默认样式与卡片底色几乎融为一体, 用户不知道能点能输。)
    /// id_salt: 稳定控件 id (调用方可用 `ui.memory(|m| m.has_focus(id))` 查焦点)。
    pub fn text_input(
        &self,
        ui: &mut egui::Ui,
        text: &mut String,
        hint: &str,
        width: f32,
        id_salt: egui::Id,
    ) -> egui::Response {
        egui::Frame::NONE
            .fill(self.extreme)
            .stroke(egui::Stroke::new(1.3, self.accent_soft))
            .corner_radius(egui::CornerRadius::same(8))
            .inner_margin(egui::Margin::symmetric(8, 5))
            .show(ui, |ui| {
                ui.add(
                    egui::TextEdit::singleline(text)
                        .hint_text(hint)
                        .desired_width(width)
                        .frame(false)
                        .id_salt(id_salt),
                )
            })
            .inner
    }

    /// 目标键徽章 (天蓝)。
    pub fn target_badge(&self, ui: &mut egui::Ui, text: &str) -> egui::Response {
        self.badge(ui, text, self.target_fg, self.target_bg)
    }

    // ───────────────────────── 弹窗脚手架 ─────────────────────────

    /// 模态弹窗脚手架: 居中 / 无边框 / 固定尺寸 / 统一圆角投影。
    /// 填充色由调用方传入 (个别弹窗有特调底色), 其余 chrome 全部归这里管。
    /// 新弹窗一律用这个, 不要再手写 egui::Window + Frame 样板。
    pub fn modal_window(
        &self,
        ctx: &egui::Context,
        id: &str,
        size: [f32; 2],
        fill: egui::Color32,
        add_contents: impl FnOnce(&mut egui::Ui),
    ) {
        egui::Window::new(id)
            .id(egui::Id::new(id).with("modal_win"))
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .fixed_size(size)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .frame(
                egui::Frame::window(&ctx.style())
                    .fill(fill)
                    .corner_radius(egui::CornerRadius::same(16))
                    .stroke(egui::Stroke::NONE)
                    .shadow(egui::epaint::Shadow {
                        offset: [0, 8],
                        blur: 24,
                        spread: 0,
                        color: egui::Color32::from_rgba_premultiplied(0, 0, 0, 60),
                    }),
            )
            .show(ctx, |ui| add_contents);
    }

    // ───────────────────────── 按钮 ─────────────────────────

    /// 主操作按钮 (强调色填充 + 白字)。
    pub fn primary_button(&self, text: &str) -> egui::Button<'_> {
        egui::Button::new(egui::RichText::new(text).size(13.0).strong().color(egui::Color32::WHITE))
            .fill(self.btn_primary)
            .corner_radius(egui::CornerRadius::same(RADIUS_CTRL))
    }

    /// 次级按钮 (柔灰填充)。
    pub fn secondary_button(&self, text: &str) -> egui::Button<'_> {
        egui::Button::new(
            egui::RichText::new(text)
                .size(13.0)
                .color(self.btn_secondary_text),
        )
        .fill(self.btn_secondary)
        .corner_radius(egui::CornerRadius::same(RADIUS_CTRL))
    }

    /// 危险按钮。
    pub fn danger_button(&self, text: &str) -> egui::Button<'_> {
        egui::Button::new(egui::RichText::new(text).size(13.0).color(egui::Color32::WHITE))
            .fill(self.btn_danger)
            .corner_radius(egui::CornerRadius::same(RADIUS_CTRL))
    }

    /// 强调文字按钮 (软底强调色)。
    #[allow(dead_code)]
    pub fn ghost_button(&self, text: &str) -> egui::Button<'_> {
        egui::Button::new(egui::RichText::new(text).size(12.0).color(self.accent))
            .fill(self.accent_soft)
            .corner_radius(egui::CornerRadius::same(RADIUS_CTRL))
    }

    /// 状态胶囊按钮 (如 "震动: 开")。
    pub fn status_pill(&self, text: &str, fg: egui::Color32, bg: egui::Color32) -> egui::Button<'_> {
        egui::Button::new(egui::RichText::new(text).size(12.5).strong().color(fg))
            .fill(bg)
            .corner_radius(egui::CornerRadius::same(12))
            .min_size(egui::vec2(0.0, 28.0))
    }
}

/// UTF-8 安全的按"字符数"截断 (按字节切片会在中文上 panic)。
pub fn truncate_chars(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max_chars).collect();
        out.push('…');
        out
    }
}

/// 全局样式: Visuals + 字号类型系统 + 间距, 明暗各一份缓存。
pub fn build_style(dark: bool) -> egui::Style {
    let th = Theme::new(dark);
    let mut style = egui::Style::default();
    let mut v = if dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };

    // 控件状态: 安静默认态 + 悬停半透明叠加 (8pt 圆角族)
    let hover_overlay = if dark {
        egui::Color32::from_white_alpha(12)
    } else {
        egui::Color32::from_black_alpha(6)
    };
    for w in [
        &mut v.widgets.noninteractive,
        &mut v.widgets.inactive,
        &mut v.widgets.hovered,
        &mut v.widgets.active,
        &mut v.widgets.open,
    ] {
        w.corner_radius = egui::CornerRadius::same(RADIUS_CTRL);
        w.bg_stroke = egui::Stroke::NONE;
    }
    v.widgets.noninteractive.corner_radius = egui::CornerRadius::same(6);
    v.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0, th.hint);
    // 浮层边界: 深色下阴影不够, popup 必须有描边
    v.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0, th.stroke_strong);
    v.widgets.inactive.bg_fill = th.faint;
    v.widgets.inactive.fg_stroke = egui::Stroke::new(1.0, th.text);
    v.widgets.inactive.weak_bg_fill = th.faint;
    v.widgets.hovered.bg_fill = hover_overlay;
    v.widgets.hovered.fg_stroke = egui::Stroke::new(1.0, if dark { egui::Color32::WHITE } else { egui::Color32::BLACK });
    v.widgets.active.bg_fill = th.accent_soft;
    v.widgets.active.fg_stroke = egui::Stroke::new(1.0, th.accent);

    v.selection.stroke = egui::Stroke::new(1.0, th.accent);
    v.selection.bg_fill = th.accent_soft;

    v.window_fill = th.surface;
    v.panel_fill = th.bg;
    v.faint_bg_color = th.faint;
    v.extreme_bg_color = th.extreme;

    // 阴影: 深色减负 (α100 是脏灰晕), 亮色轻投影
    v.window_shadow = egui::epaint::Shadow {
        offset: [0, 14],
        blur: 30,
        spread: 0,
        color: egui::Color32::from_black_alpha(if dark { 64 } else { 26 }),
    };
    v.popup_shadow = egui::epaint::Shadow {
        offset: [0, 6],
        blur: 20,
        spread: 0,
        color: egui::Color32::from_black_alpha(if dark { 48 } else { 20 }),
    };
    v.window_corner_radius = egui::CornerRadius::same(12);
    v.menu_corner_radius = egui::CornerRadius::same(10);
    v.interact_cursor = Some(egui::CursorIcon::PointingHand);
    v.slider_trailing_fill = true; // 滑块滑过段着色

    style.visuals = v;

    // 字号类型系统 (全局): Display 22 / Title 16 / Heading 18 / Body 13 / Caption 12
    style.text_styles = [
        (egui::TextStyle::Heading, egui::FontId::proportional(18.0)),
        (egui::TextStyle::Body, egui::FontId::proportional(13.0)),
        (egui::TextStyle::Button, egui::FontId::proportional(13.0)),
        (egui::TextStyle::Small, egui::FontId::proportional(12.0)),
        (egui::TextStyle::Monospace, egui::FontId::monospace(12.0)),
        (
            egui::TextStyle::Name("Display".into()),
            egui::FontId::proportional(22.0),
        ),
        (
            egui::TextStyle::Name("Title".into()),
            egui::FontId::proportional(16.0),
        ),
        (
            egui::TextStyle::Name("Caption".into()),
            egui::FontId::proportional(12.0),
        ),
    ]
    .into_iter()
    .collect();

    // 间距 token 注入
    style.spacing.item_spacing = egui::vec2(SP_S, SP_S);
    style.spacing.button_padding = egui::vec2(SP_M, 5.0);
    style.spacing.menu_margin = egui::Margin::same(6);
    style.spacing.slider_rail_height = 4.0; // 细滑轨

    // 浮动滚动条: 悬停才浮现 (Raycast 同款)
    style.spacing.scroll = egui::style::ScrollStyle {
        floating: true,
        bar_width: 6.0,
        bar_inner_margin: 4.0,
        bar_outer_margin: 4.0,
        dormant_handle_opacity: 0.0,
        active_handle_opacity: 0.4,
        interact_handle_opacity: 0.65,
        handle_min_length: 32.0,
        ..Default::default()
    };

    style
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_chars_is_utf8_safe() {
        let s = "手柄·测试按键X"; // 9 个字符, 22 字节
        let t = truncate_chars(s, 5);
        assert_eq!(t.chars().count(), 6); // 5 字符 + 省略号
        assert!(t.ends_with('…'));

        assert_eq!(truncate_chars("abc", 10), "abc");
        assert_eq!(truncate_chars("", 3), "");
    }

    #[test]
    fn theme_palettes_are_distinct() {
        let d = Theme::dark();
        let l = Theme::light();
        assert!(d.dark && !l.dark);
        assert_ne!(d.bg, l.bg);
    }
}
