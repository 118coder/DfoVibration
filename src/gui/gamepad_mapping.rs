//! 手柄可视化快速映射页。
//!
//! 左侧渲染内嵌的 Xbox 手柄 SVG, 按键区域做成可点击热点;
//! 右侧显示选中槽位的当前映射, 并支持两步快速设置:
//! 1. 捕获物理手柄按键作为触发键;
//! 2. 捕获键盘/鼠标按键作为目标键。
//!
//! 热点坐标标定: SVG viewBox 为 "22 135 545 401" (无文字横版), 槽位坐标 = SVG
//! 元素中心经图层变换后归一化到该 viewBox。修改 `resources/gamepad.svg` 后必须
//! 重新标定 [`SLOTS`] (带文字的原始备份见 docs/gamepad-original-with-text.svg.bak)。

use crate::config::{AppConfig, KeyMapping};
use crate::gui::SorahkGui;
use crate::gui::theme::{self, Theme};
use crate::gui::types::KeyCaptureMode;
use eframe::egui;

/// SVG 渲染倍率 (2x 保证 HiDPI 下不模糊)。
const SVG_RENDER_SCALE: f32 = 2.0;

/// SVG 原始尺寸 (viewBox), 用于计算显示宽高比。
const SVG_SIZE: egui::Vec2 = egui::vec2(545.0, 401.0);

/// 槽位类别, 用于热点配色与右侧图例。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotKind {
    /// 扳机 (LT/RT)
    Trigger,
    /// 肩键 (LB/RB)
    Shoulder,
    /// 十字键
    Dpad,
    /// 摇杆方向
    Stick,
    /// 摇杆按下
    StickClick,
    /// ABXY 面键
    Button,
    /// 系统键 (Back/Start)
    System,
}

impl SlotKind {
    /// 图例文案。
    pub fn legend(self) -> &'static str {
        match self {
            SlotKind::Trigger => "扳机",
            SlotKind::Shoulder => "肩键",
            SlotKind::Dpad => "十字键",
            SlotKind::Stick => "摇杆方向",
            SlotKind::StickClick => "摇杆按下",
            SlotKind::Button => "ABXY 面键",
            SlotKind::System => "系统键",
        }
    }

    /// 热点基础色 (RGB), 明暗主题通用。
    pub fn base_rgb(self) -> (u8, u8, u8) {
        match self {
            SlotKind::Trigger => (235, 146, 52),
            SlotKind::Shoulder => (175, 122, 224),
            SlotKind::Dpad => (72, 170, 235),
            SlotKind::Stick => (80, 200, 130),
            SlotKind::StickClick => (235, 200, 80),
            SlotKind::Button => (124, 140, 248),
            SlotKind::System => (150, 165, 180),
        }
    }
}

/// 手柄上的一个可配置槽位。
#[derive(Debug, Clone, Copy)]
pub struct GamepadSlot {
    pub id: usize,
    pub label: &'static str,
    /// 显示在手柄图热点内的短标签 (方向用箭头, 完整名称见悬停提示)
    pub short: &'static str,
    pub default_trigger: &'static str,
    /// 热点中心在图片中的归一化坐标 (0..1, 0..1)
    pub pos: (f32, f32),
    /// 热点半径 (归一化, 以图片宽度为基准)
    pub radius: f32,
    pub kind: SlotKind,
}

/// 所有可配置手柄槽位。
///
/// 坐标按 `resources/gamepad.svg` (无文字版, viewBox "22 135 545 401") 精确标定:
/// - 扳机/肩键热点骑跨机身顶部肩线 (前视图惯例);
/// - 摇杆/十字键/面键/系统键热点位于机身对应按钮中心。
/// 注意: XInput 层不支持 Guide (Xbox 徽标) 键, 故不提供该槽位。
pub const SLOTS: &[GamepadSlot] = &[
    // ── 扳机 / 肩键 (机身顶部边缘, 前视图惯例: 骑跨肩线) ──
    GamepadSlot { id: 0, label: "LT", short: "LT", default_trigger: "GAMEPAD_045E_LT", pos: (0.2826, 0.0923), radius: 0.038, kind: SlotKind::Trigger },
    GamepadSlot { id: 1, label: "LB", short: "LB", default_trigger: "GAMEPAD_045E_LB", pos: (0.3963, 0.1172), radius: 0.038, kind: SlotKind::Shoulder },
    GamepadSlot { id: 2, label: "RT", short: "RT", default_trigger: "GAMEPAD_045E_RT", pos: (0.7376, 0.0923), radius: 0.038, kind: SlotKind::Trigger },
    GamepadSlot { id: 3, label: "RB", short: "RB", default_trigger: "GAMEPAD_045E_RB", pos: (0.6239, 0.1172), radius: 0.038, kind: SlotKind::Shoulder },
    // ── 十字键 (中心 0.380/0.527, 臂长 0.048x/0.059y) ──
    GamepadSlot { id: 4, label: "十字键·上", short: "↑", default_trigger: "GAMEPAD_045E_DPad_Up", pos: (0.3802, 0.4678), radius: 0.0284, kind: SlotKind::Dpad },
    GamepadSlot { id: 5, label: "十字键·下", short: "↓", default_trigger: "GAMEPAD_045E_DPad_Down", pos: (0.3802, 0.5856), radius: 0.0284, kind: SlotKind::Dpad },
    GamepadSlot { id: 6, label: "十字键·左", short: "←", default_trigger: "GAMEPAD_045E_DPad_Left", pos: (0.3325, 0.5267), radius: 0.0284, kind: SlotKind::Dpad },
    GamepadSlot { id: 7, label: "十字键·右", short: "→", default_trigger: "GAMEPAD_045E_DPad_Right", pos: (0.4279, 0.5267), radius: 0.0284, kind: SlotKind::Dpad },
    // ── 左摇杆 (中心 0.249/0.316) ──
    GamepadSlot { id: 8, label: "左摇杆·上", short: "↑", default_trigger: "GAMEPAD_045E_LS_Up", pos: (0.2492, 0.2611), radius: 0.0251, kind: SlotKind::Stick },
    GamepadSlot { id: 9, label: "左摇杆·下", short: "↓", default_trigger: "GAMEPAD_045E_LS_Down", pos: (0.2492, 0.3709), radius: 0.0251, kind: SlotKind::Stick },
    GamepadSlot { id: 10, label: "左摇杆·左", short: "←", default_trigger: "GAMEPAD_045E_LS_Left", pos: (0.2088, 0.3160), radius: 0.0251, kind: SlotKind::Stick },
    GamepadSlot { id: 11, label: "左摇杆·右", short: "→", default_trigger: "GAMEPAD_045E_LS_Right", pos: (0.2896, 0.3160), radius: 0.0251, kind: SlotKind::Stick },
    // ── 右摇杆 (中心 0.628/0.517) ──
    GamepadSlot { id: 12, label: "右摇杆·上", short: "↑", default_trigger: "GAMEPAD_045E_RS_Up", pos: (0.6283, 0.4696), radius: 0.0251, kind: SlotKind::Stick },
    GamepadSlot { id: 13, label: "右摇杆·下", short: "↓", default_trigger: "GAMEPAD_045E_RS_Down", pos: (0.6283, 0.5644), radius: 0.0251, kind: SlotKind::Stick },
    GamepadSlot { id: 14, label: "右摇杆·左", short: "←", default_trigger: "GAMEPAD_045E_RS_Left", pos: (0.5934, 0.5170), radius: 0.0251, kind: SlotKind::Stick },
    GamepadSlot { id: 15, label: "右摇杆·右", short: "→", default_trigger: "GAMEPAD_045E_RS_Right", pos: (0.6632, 0.5170), radius: 0.0251, kind: SlotKind::Stick },
    // ── ABXY 面键 ──
    GamepadSlot { id: 16, label: "X 键", short: "X", default_trigger: "GAMEPAD_045E_X", pos: (0.6833, 0.3155), radius: 0.036, kind: SlotKind::Button },
    GamepadSlot { id: 17, label: "Y 键", short: "Y", default_trigger: "GAMEPAD_045E_Y", pos: (0.7516, 0.2239), radius: 0.036, kind: SlotKind::Button },
    GamepadSlot { id: 18, label: "B 键", short: "B", default_trigger: "GAMEPAD_045E_B", pos: (0.8220, 0.3155), radius: 0.036, kind: SlotKind::Button },
    GamepadSlot { id: 19, label: "A 键", short: "A", default_trigger: "GAMEPAD_045E_A", pos: (0.7516, 0.4067), radius: 0.036, kind: SlotKind::Button },
    // ── 系统键 ──
    GamepadSlot { id: 20, label: "Back", short: "Back", default_trigger: "GAMEPAD_045E_Back", pos: (0.4284, 0.3145), radius: 0.0257, kind: SlotKind::System },
    GamepadSlot { id: 21, label: "Start", short: "Start", default_trigger: "GAMEPAD_045E_Start", pos: (0.5723, 0.3145), radius: 0.0257, kind: SlotKind::System },
    // ── 摇杆按下 (最后注册, 命中优先于方向) ──
    GamepadSlot { id: 22, label: "左摇杆·按下", short: "", default_trigger: "GAMEPAD_045E_LS_Click", pos: (0.2492, 0.3160), radius: 0.020, kind: SlotKind::StickClick },
    GamepadSlot { id: 23, label: "右摇杆·按下", short: "", default_trigger: "GAMEPAD_045E_RS_Click", pos: (0.6283, 0.5170), radius: 0.020, kind: SlotKind::StickClick },
];

pub fn get_slot(id: usize) -> Option<&'static GamepadSlot> {
    SLOTS.iter().find(|s| s.id == id)
}

/// 槽位在 Config.toml 中使用的持久化标记 (写入 mapping.note)。
pub fn slot_note(label: &str) -> String {
    format!("手柄·{}", label)
}

/// 查找某个槽位对应的现有映射。
pub fn find_slot_mapping_index(config: &AppConfig, slot: &GamepadSlot) -> Option<usize> {
    config.mappings.iter().position(|m| {
        m.note == slot_note(slot.label) || m.trigger_key == slot.default_trigger
    })
}

fn mapping_matches_slot(m: &KeyMapping, slot: &GamepadSlot) -> bool {
    m.note == slot_note(slot.label) || m.trigger_key == slot.default_trigger
}

/// 设置槽位的触发键: 已有映射则更新, 没有则新建。
/// 返回映射在 config.mappings 中的下标。
pub fn set_slot_trigger(config: &mut AppConfig, slot: &GamepadSlot, trigger: String) -> usize {
    if let Some(idx) = config
        .mappings
        .iter()
        .position(|m| mapping_matches_slot(m, slot))
    {
        config.mappings[idx].trigger_key = trigger;
        config.mappings[idx].note = slot_note(slot.label);
        idx
    } else {
        config.mappings.push(KeyMapping {
            trigger_key: trigger,
            target_keys: Default::default(),
            interval: None,
            event_duration: None,
            turbo_enabled: true,
            move_speed: 5,
            double_tap_enabled: false,
            double_tap_gap_ms: 50,
            note: slot_note(slot.label),
        });
        config.mappings.len() - 1
    }
}

/// 向槽位添加一个目标键 (不重复)。返回是否发生修改。
pub fn add_slot_target(config: &mut AppConfig, slot: &GamepadSlot, target: String) -> bool {
    let idx = if let Some(idx) = find_slot_mapping_index(config, slot) {
        idx
    } else {
        set_slot_trigger(config, slot, slot.default_trigger.to_string())
    };
    let len_before = config.mappings[idx].target_keys.len();
    config.mappings[idx].add_target_key(target);
    config.mappings[idx].note = slot_note(slot.label);
    config.mappings[idx].target_keys.len() != len_before
}

/// 清空槽位的目标键。
pub fn clear_slot_targets(config: &mut AppConfig, slot: &GamepadSlot) {
    if let Some(idx) = find_slot_mapping_index(config, slot) {
        config.mappings[idx].clear_target_keys();
        config.mappings[idx].note = slot_note(slot.label);
    }
}

/// 删除槽位对应的映射。
pub fn remove_slot_mapping(config: &mut AppConfig, slot: &GamepadSlot) {
    if let Some(idx) = find_slot_mapping_index(config, slot) {
        config.mappings.remove(idx);
    }
}

/// 获取槽位当前映射的触发键。
pub fn slot_trigger_display(config: &AppConfig, slot: &GamepadSlot) -> String {
    if let Some(idx) = find_slot_mapping_index(config, slot) {
        config.mappings[idx].trigger_key.clone()
    } else {
        slot.default_trigger.to_string()
    }
}

/// 获取槽位当前映射的目标键列表。
pub fn slot_targets(config: &AppConfig, slot: &GamepadSlot) -> Vec<String> {
    if let Some(idx) = find_slot_mapping_index(config, slot) {
        config.mappings[idx].target_keys.to_vec()
    } else {
        Vec::new()
    }
}

/// 使用 resvg 将内嵌手柄 SVG 渲染为 egui 纹理 (2x 超采样, HiDPI 清晰)。
pub fn load_gamepad_texture(ctx: &egui::Context, dark: bool) -> Option<egui::TextureHandle> {
    // 双主题变体: 亮色 = resources/gamepad-light.svg (仅 style 颜色不同, 几何一致,
    // 热点坐标共用同一 viewBox)
    let svg_data: &[u8] = if dark {
        include_bytes!("../../resources/gamepad.svg")
    } else {
        include_bytes!("../../resources/gamepad-light.svg")
    };

    let mut opt = resvg::usvg::Options::default();
    opt.fontdb_mut().load_system_fonts();
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
    let color_image = egui::ColorImage::from_rgba_unmultiplied(image_size, pixmap.data());
    Some(ctx.load_texture(
        if dark { "gamepad_svg_dark" } else { "gamepad_svg_light" },
        color_image,
        egui::TextureOptions::LINEAR,
    ))
}

/// 计算热点在指定显示矩形内的屏幕圆心与半径。
fn hotspot_rect(rect: egui::Rect, slot: &GamepadSlot) -> (egui::Pos2, f32) {
    let center = egui::pos2(
        rect.min.x + slot.pos.0 * rect.width(),
        rect.min.y + slot.pos.1 * rect.height(),
    );
    let radius = slot.radius * rect.width();
    (center, radius)
}

impl SorahkGui {
    /// 手柄可视化快速映射页主体: 左 (大图+图例) / 右 (槽位面板) + 底部已配置映射。
    pub(super) fn render_gamepad_page(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        // 主题切换时重渲染对应变体 (按 dark 标记缓存)
        if self.gamepad_texture.is_none() || self.gamepad_texture_dark != self.dark_mode {
            self.gamepad_texture = load_gamepad_texture(ctx, self.dark_mode);
            self.gamepad_texture_dark = self.dark_mode;
        }
        let th = Theme::new(self.dark_mode);

        th.card(ui, None, |ui| {
            ui.horizontal_top(|ui| {
                // 左: 手柄图 + 热点 (横版构图占主视觉)
                ui.vertical(|ui| {
                    // 宽屏下同步放大 (上限 800), 保持左右面板视觉平衡
                    let display_w = (ui.available_width() * 0.52).min(800.0);
                    let display_h = display_w * (SVG_SIZE.y / SVG_SIZE.x);
                    let clicked_slot =
                        self.render_gamepad_svg(ui, egui::vec2(display_w, display_h), &th);
                    if let Some(id) = clicked_slot {
                        self.gamepad_selected_slot = Some(id);
                    }
                });

                ui.add_space(theme::SP_L);

                // 右: 槽位详情 (占满剩余宽度)
                ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                    ui.set_min_width(ui.available_width());
                    self.render_gamepad_slot_panel(ui, &th);
                });
            });
        });

        // 底部: 已配置手柄映射总览
        self.render_gamepad_overview(ui, &th);
    }

    /// 已配置手柄映射 chips 总览 (点击可跳选对应槽位)。
    fn render_gamepad_overview(&mut self, ui: &mut egui::Ui, th: &Theme) {
        let configured: Vec<&GamepadSlot> = SLOTS
            .iter()
            .filter(|s| find_slot_mapping_index(&self.config, s).is_some())
            .collect();
        let has_any_targets = configured.iter().any(|s| {
            find_slot_mapping_index(&self.config, s)
                .map(|i| !self.config.mappings[i].target_keys.is_empty())
                .unwrap_or(false)
        });

        let title = if configured.is_empty() {
            "已配置的按键".to_string()
        } else {
            format!("已配置的按键 ({})", configured.len())
        };
        let empty = configured.is_empty();

        th.card_with_actions(ui, Some(&title), |ui| {
            if has_any_targets {
                ui.label(th.hint_text("点击下方按键可快速跳转编辑"));
            }
        }, |ui| {
            if empty {
                ui.label(th.hint_text("还没有配置任何手柄按键 — 点击左侧手柄图上的按钮开始"));
                return;
            }
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
                for slot in &configured {
                    let idx = find_slot_mapping_index(&self.config, slot).unwrap();
                    let m = &self.config.mappings[idx];
                    let target_count = m.target_keys.len();
                    let label = if target_count > 0 {
                        format!("{} · {}", slot.label, target_count)
                    } else {
                        slot.label.to_string()
                    };
                    let selected = self.gamepad_selected_slot == Some(slot.id);
                    let (fg, bg) = if selected {
                        (egui::Color32::WHITE, th.accent)
                    } else if target_count > 0 {
                        (th.target_fg, th.target_bg)
                    } else {
                        (th.text_weak, th.faint)
                    };
                    if th.badge_clickable(ui, &label, fg, bg).clicked() {
                        self.gamepad_selected_slot = Some(slot.id);
                    }
                }
            });
        });
    }

    /// 绘制手柄图与热点, 返回被点击的槽位 id。
    fn render_gamepad_svg(
        &mut self,
        ui: &mut egui::Ui,
        desired_size: egui::Vec2,
        th: &Theme,
    ) -> Option<usize> {
        let (rect, _) = ui.allocate_exact_size(desired_size, egui::Sense::hover());
        let painter = ui.painter();

        if let Some(texture) = &self.gamepad_texture {
            painter.image(
                texture.id(),
                rect,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
        } else {
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "手柄图片加载失败",
                egui::FontId::proportional(14.0),
                th.hint,
            );
        }

        let mut clicked = None;
        let selected_id = self.gamepad_selected_slot;
        let capture_slot = match self.key_capture_mode {
            KeyCaptureMode::QuickGamepadTrigger(id) => Some((id, true)),
            KeyCaptureMode::QuickGamepadTarget(id) => Some((id, false)),
            _ => None,
        };
        // 捕获等待态: 各槽位独立雷达动画 (见下方 is_capturing 分支)

        for slot in SLOTS {
            let (center, radius) = hotspot_rect(rect, slot);
            let hit_rect = egui::Rect::from_center_size(
                center,
                egui::vec2(radius * 2.0, radius * 2.0),
            );
            let id = ui.id().with("gamepad_hotspot").with(slot.id);
            let response = ui.interact(hit_rect, id, egui::Sense::click());
            if response.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }

            let is_selected = selected_id == Some(slot.id);
            let is_capturing = matches!(capture_slot, Some((cid, _)) if cid == slot.id);
            let (base_r, base_g, base_b) = slot.kind.base_rgb();
            let slot_col = |a: u8| egui::Color32::from_rgba_unmultiplied(base_r, base_g, base_b, a);
            let configured = find_slot_mapping_index(&self.config, slot).is_some();
            let is_click = matches!(slot.kind, SlotKind::StickClick);

            // 悬停/选中/捕获的放大动画 (150ms 缓动)
            let hot = is_capturing || is_selected || response.hovered();
            let scale = 1.0 + 0.10 * ui.ctx().animate_bool_with_time(
                ui.id().with("gamepad_hotspot_anim").with(slot.id),
                hot,
                0.15,
            );
            let r_eff = radius * scale;

            if !is_click {
                if is_capturing {
                    /* 捕获态: 雷达扩散环 + 琥珀主环 */
                    let phase = (ui.ctx().input(|i| i.time) * 1.6) % 1.0;
                    painter.circle_stroke(
                        center,
                        r_eff * (1.05 + 0.40 * phase as f32),
                        egui::Stroke::new(
                            2.0,
                            egui::Color32::from_rgba_unmultiplied(
                                255,
                                190,
                                60,
                                (190.0 * (1.0 - phase as f32)) as u8,
                            ),
                        ),
                    );
                    painter.circle_filled(center, r_eff, egui::Color32::from_rgba_unmultiplied(255, 190, 60, 60));
                    painter.circle_stroke(center, r_eff, egui::Stroke::new(2.2, egui::Color32::from_rgb(255, 200, 80)));
                } else {
                    /* 悬停/选中: 外柔光 */
                    if hot {
                        painter.circle_filled(center, r_eff * 1.5, slot_col(if is_selected { 55 } else { 40 }));
                    }
                    /* 玻璃底 (深机身增透, 亮机身轻压暗) */
                    painter.circle_filled(
                        center,
                        r_eff,
                        if th.dark {
                            egui::Color32::from_black_alpha(64)
                        } else {
                            egui::Color32::from_black_alpha(30)
                        },
                    );
                    /* 主环: 选中=强调色 / 已配置=类色实线 / 空槽=类色细线 */
                    painter.circle_stroke(
                        center,
                        r_eff,
                        if is_selected {
                            egui::Stroke::new(2.6, th.accent)
                        } else if configured {
                            egui::Stroke::new(2.0, slot_col(235))
                        } else {
                            egui::Stroke::new(1.4, slot_col(125))
                        },
                    );
                    /* 中心点 (单字符槽位; Back/Start 保留文字空间) */
                    if slot.short.chars().count() == 1 {
                        painter.circle_filled(
                            center,
                            r_eff * 0.15,
                            if is_selected {
                                th.accent
                            } else {
                                slot_col(if configured { 255 } else { 140 })
                            },
                        );
                    }
                }
            }

            /* 短标签 (方向箭头/肩键文字) 带微投影; 摇杆按下为隐形热点不绘字;
             * ABXY 面键与 Back/Start 系统键圈内不绘字 (2026-09-08 用户要求留白) */
            if !slot.short.is_empty()
                && !is_click
                && !matches!(slot.kind, SlotKind::Button | SlotKind::System)
            {
                let font_size = match slot.short.chars().count() {
                    1 => 13.0,
                    2 => 9.5,
                    _ => 8.5,
                };
                let tcol = if is_capturing || is_selected || response.hovered() {
                    egui::Color32::WHITE
                } else if configured {
                    slot_col(255)
                } else if th.dark {
                    egui::Color32::from_rgba_unmultiplied(255, 255, 255, 190)
                } else {
                    egui::Color32::from_rgba_unmultiplied(58, 65, 82, 235)
                };
                painter.text(
                    center + egui::vec2(0.0, 1.0),
                    egui::Align2::CENTER_CENTER,
                    slot.short,
                    egui::FontId::proportional(font_size),
                    egui::Color32::from_black_alpha(if th.dark { 130 } else { 60 }),
                );
                painter.text(
                    center,
                    egui::Align2::CENTER_CENTER,
                    slot.short,
                    egui::FontId::proportional(font_size),
                    tcol,
                );
            }

            response.clone().on_hover_text(format!(
                "{} — 默认触发: {}
点击选中后在右侧配置映射",
                slot.label, slot.default_trigger
            ));

            if response.clicked() {
                clicked = Some(slot.id);
            }
        }

        clicked
    }

    /// 右栏快捷卡 (空槽态显示): 连发开关 / 映射选择 / 震动开关 / 职业选择。
    /// 控件语义与极简模式一致 (toggle_with_notify / 选中即应用), egui id 用
    /// "gp_" 前缀避免与极简模式冲突; 震动段仅 DFO 玩家可见。
    fn render_quick_connect_card(&mut self, ui: &mut egui::Ui, th: &Theme) {
        use std::sync::atomic::Ordering;
        th.panel(ui, Some("快速连接"), |ui| {
            /* 连发大开关 */
            let running = !self.app_state.is_paused();
            let (ltext, lfg, lbg) = if running {
                ("连发 · 开启中", th.good, th.good_soft)
            } else {
                ("连发 · 已暂停", th.hint, th.faint)
            };
            let turbo_btn = egui::Button::new(
                egui::RichText::new(ltext).size(14.0).strong().color(lfg),
            )
            .fill(lbg)
            .corner_radius(egui::CornerRadius::same(10));
            if ui
                .add_sized([ui.available_width(), 36.0], turbo_btn)
                .clicked()
            {
                self.toggle_with_notify();
            }
            ui.add_space(theme::SP_XS);

            /* 映射选择 (连发预设, 与极简/顶栏同一套切换逻辑) */
            if self.config.presets.is_empty() {
                ui.label(th.hint_text("映射预设: 无 — 到「连发映射修改」页保存一个"));
            } else {
                ui.horizontal(|ui| {
                    ui.label(th.weak("映射选择"));
                    self.render_preset_switch(ui);
                });
            }
            ui.add_space(theme::SP_S);

            /* 震动段 (仅 DFO 玩家) */
            if self.config.dfo_player {
                let vib_on = self
                    .app_state
                    .vibration_enabled
                    .load(Ordering::Relaxed);
                let (vtext, vfg, vbg) = if vib_on {
                    ("震动 · 开启", th.good, th.good_soft)
                } else {
                    ("震动 · 关闭", th.hint, th.faint)
                };
                let vib_btn = egui::Button::new(
                    egui::RichText::new(vtext).size(14.0).strong().color(vfg),
                )
                .fill(vbg)
                .corner_radius(egui::CornerRadius::same(10));
                if ui
                    .add_sized([ui.available_width(), 36.0], vib_btn)
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
                ui.add_space(theme::SP_XS);

                /* 通用 / 全职业 分段开关 (与极简模式共享 minimal_vib_preset_job 状态) */
                let mode_job = self.minimal_vib_preset_job;
                let seg_w = ui.available_width();
                let seg_h = 26.0_f32;
                let (seg_rect, _) =
                    ui.allocate_exact_size(egui::vec2(seg_w, seg_h), egui::Sense::hover());
                let half = seg_w / 2.0;
                let left_rect = egui::Rect::from_x_y_ranges(
                    seg_rect.left()..=seg_rect.center().x,
                    seg_rect.top()..=seg_rect.bottom(),
                );
                let right_rect = egui::Rect::from_x_y_ranges(
                    seg_rect.center().x..=seg_rect.right(),
                    seg_rect.top()..=seg_rect.bottom(),
                );
                let l_resp = ui
                    .interact(left_rect, egui::Id::new("gp_vib_preset_mode").with("l"), egui::Sense::click())
                    .on_hover_cursor(egui::CursorIcon::PointingHand);
                let r_resp = ui
                    .interact(right_rect, egui::Id::new("gp_vib_preset_mode").with("r"), egui::Sense::click())
                    .on_hover_cursor(egui::CursorIcon::PointingHand);
                ui.painter().rect_filled(seg_rect, 8, th.faint);
                let t = ui.ctx().animate_value_with_time(
                    egui::Id::new("gp_vib_preset_mode"),
                    if mode_job { 1.0 } else { 0.0 },
                    0.15,
                );
                let thumb_w = half - 6.0;
                let thumb_x = seg_rect.left() + 3.0 + t * (half - 6.0);
                let thumb = egui::Rect::from_min_size(
                    egui::pos2(thumb_x, seg_rect.top() + 3.0),
                    egui::vec2(thumb_w, seg_h - 6.0),
                );
                ui.painter().rect_filled(thumb, 6, th.accent);
                let unselected = |a: f32| -> egui::Color32 {
                    let mut c = th.text_weak;
                    egui::Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), a as u8)
                };
                let selected_white = |a: f32| -> egui::Color32 {
                    egui::Color32::from_rgba_unmultiplied(255, 255, 255, a as u8)
                };
                ui.painter().text(
                    left_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "通用预设",
                    egui::FontId::proportional(12.0),
                    if mode_job { unselected(200.0) } else { selected_white(255.0) },
                );
                ui.painter().text(
                    right_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "全职业预设",
                    egui::FontId::proportional(12.0),
                    if mode_job { selected_white(255.0) } else { unselected(200.0) },
                );
                if l_resp.clicked() {
                    self.minimal_vib_preset_job = false;
                    /* ★保险B (2026-09-09, 用户定稿): 切回通用预设段 = 放弃全职业
                     * 预设 —— 自动停用 (关开关+回滚参数+JobVibration.toml applied=false),
                     * 防止职业参数在通用模式下继续静默生效 */
                    if self.vib_job_enabled {
                        self.disable_job_vibration_preset();
                    }
                    let _ = self.config.save_to_file("Config.toml");
                }
                if r_resp.clicked() {
                    self.minimal_vib_preset_job = true;
                    let jobs = crate::job_presets::available_jobs(self.config.vib_legacy_client);
                    let base_sel = self.vib_job_base.min(jobs.len().saturating_sub(1));
                    let class_sel =
                        self.vib_job_class.min(jobs[base_sel].classes.len().saturating_sub(1));
                    self.apply_job_vibration_preset(
                        jobs[base_sel].base_job,
                        &jobs[base_sel].classes[class_sel],
                    );
                    self.vib_job_loaded = Some((base_sel, class_sel));
                    let _ = self.config.save_to_file("Config.toml");
                }
                ui.add_space(theme::SP_XS);

                /* 选择行 (选中即应用); ★v19.3 与上方分段控件拉开间距 (用户: 贴太紧不美观) */
                if !mode_job {
                    ui.add_space(9.0);
                    ui.horizontal(|ui| {
                        ui.label(th.weak("通用预设"));
                        self.ensure_act1_preset_listed();
                        /* ★v19: 用真实下标, 修过滤下标错位 */
                        let entries = crate::config::visible_preset_entries(
                            &self.config.vibration_presets,
                            self.config.vib_legacy_client,
                        );
                        let sel_pos = entries
                            .iter()
                            .position(|(real, _)| *real == self.vib_preset_idx)
                            .unwrap_or(0);
                        if let Some((real, _)) = entries.get(sel_pos) {
                            self.vib_preset_idx = *real;
                        }
                        let selected = entries.get(sel_pos).map(|(_, n)| n.clone()).unwrap_or_default();
                        egui::ComboBox::from_id_salt("gp_vib_general")
                            .selected_text(selected)
                            .width(150.0)
                            .show_ui(ui, |ui| {
                                for (pos, (real, n)) in entries.iter().enumerate() {
                                    if ui.selectable_label(pos == sel_pos, n).clicked() {
                                        self.vib_preset_idx = *real;
                                        self.apply_general_vibration_preset(n);
                                    }
                                }
                            });
                    });
                } else {
                    let jobs = crate::job_presets::available_jobs(self.config.vib_legacy_client);
                    let base_sel = self.vib_job_base.min(jobs.len().saturating_sub(1));
                    let classes = &jobs[base_sel].classes;
                    let class_sel = self.vib_job_class.min(classes.len().saturating_sub(1));
                    /* ★v19.3 与上方分段控件拉开间距 (用户: 贴太紧不美观);
                     * 转职槽仍按原微调上调 1.5px (定块 + 绝对定位子 Ui) */
                    ui.add_space(9.0);
                    ui.horizontal(|ui| {
                        ui.label(th.weak("职业选择"));
                        /* ★v19.6 S1 专属 (用户定稿): "-ACT" 职业名比槽位宽 → 转职槽
                         * 自然排在职业框真实宽度之后, 再右移 30px, 彻底消除左右重叠。
                         * 改为自然流式布局 (不再绝对定位), S4 分支不受影响。 */
                        egui::ComboBox::from_id_salt("gp_vib_job_base")
                            .selected_text(jobs[base_sel].base_job)
                            .width(124.0)
                            .show_ui(ui, |ui| {
                                for (i, j) in jobs.iter().enumerate() {
                                    if ui.selectable_label(i == base_sel, j.base_job).clicked() {
                                        self.vib_job_base = i;
                                        self.vib_job_class = 0;
                                    }
                                }
                            });
                        /* ★v19.7 用户定稿: 转职槽再左移 25px (30 → 5) 并上移 3px
                         * (自然流式预留空间 + 抬高 3px 的矩形内绘制, 行高不变) */
                        ui.add_space(5.0);
                        let combo_h = ui.spacing().interact_size.y;
                        let (class_slot, _) = ui.allocate_exact_size(
                            egui::vec2(96.0, combo_h),
                            egui::Sense::hover(),
                        );
                        ui.allocate_new_ui(
                            egui::UiBuilder::new().max_rect(egui::Rect::from_min_size(
                                egui::pos2(class_slot.min.x, class_slot.min.y - 3.0),
                                egui::vec2(96.0, combo_h),
                            )),
                            |ui| {
                                egui::ComboBox::from_id_salt("gp_vib_job_class")
                                    .selected_text(classes[class_sel].name.clone())
                                    .width(96.0)
                                    .show_ui(ui, |ui| {
                                        for (i, c) in classes.iter().enumerate() {
                                            if ui.selectable_label(i == class_sel, c.name).clicked() {
                                                self.vib_job_class = i;
                                                self.apply_job_vibration_preset(
                                                    jobs[base_sel].base_job,
                                                    c,
                                                );
                                            }
                                        }
                                    });
                            },
                        );
                    });
                }

                /* ★震动模块状态行: 按版本分流 (ACT1=自动注入 / S4+=模块已启用)
                 * ★v19.5 S1 专属: 职业选择行与状态行之间多留一点间距 (S4 不变) */
                ui.add_space(if mode_job { 10.0 } else { theme::SP_XS });
                if self.config.vib_legacy_client {
                    let ready = crate::auto_inject::host_dir_dll();
                    let (itext, ifg, ibg) = if ready {
                        ("ACT1 自动注入 · 已启用 (DLL: 主程序目录)", th.good, th.good_soft)
                    } else {
                        ("ACT1 自动注入 · 已启用 (DLL: 游戏目录)", th.good, th.good_soft)
                    };
                    let inj_btn = egui::Button::new(
                        egui::RichText::new(itext).size(12.5).strong().color(ifg),
                    )
                    .fill(ibg)
                    .corner_radius(egui::CornerRadius::same(10));
                    ui.add_sized([ui.available_width(), 30.0], inj_btn);
                    ui.label(th.hint_text(
                        "把 DfoVibration_OLD.dll 放入游戏目录后开游戏即自动注入; 未检测到 DLL 时不注入",
                    ));
                } else {
                    let s4_btn = egui::Button::new(
                        egui::RichText::new("S4 震动模块自动注入 · 已启用")
                            .size(12.5)
                            .strong()
                            .color(th.good),
                    )
                    .fill(th.good_soft)
                    .corner_radius(egui::CornerRadius::same(10));
                    ui.add_sized([ui.available_width(), 30.0], s4_btn);
                }
            }
        });
    }

    /// 右侧槽位详情面板。
    fn render_gamepad_slot_panel(&mut self, ui: &mut egui::Ui, th: &Theme) {
        /* 待确认捕获条: 捕获结果不再立即生效, 防止误操作 (确认应用 / 取消) */
        if let Some((slot_id, is_trigger, captured)) = self.quick_gamepad_pending.clone() {
            let slot_label = crate::gui::gamepad_mapping::get_slot(slot_id)
                .map(|s| s.label)
                .unwrap_or("未知槽位");
            th.panel(ui, None, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("⚠ 待确认")
                            .size(13.0)
                            .strong()
                            .color(th.warn),
                    );
                    ui.label(th.body(format!(
                        "「{}」{} = {}",
                        slot_label,
                        if is_trigger { "触发键" } else { "目标键" },
                        captured
                    )));
                });
                ui.add_space(theme::SP_XS);
                ui.horizontal(|ui| {
                    if ui.add(th.primary_button("✓ 确认应用")).clicked() {
                        self.quick_gamepad_pending = None;
                        if let Some(sl) = crate::gui::gamepad_mapping::get_slot(slot_id) {
                            if is_trigger {
                                crate::gui::gamepad_mapping::set_slot_trigger(
                                    &mut self.config,
                                    sl,
                                    captured.clone(),
                                );
                            } else {
                                crate::gui::gamepad_mapping::add_slot_target(
                                    &mut self.config,
                                    sl,
                                    captured,
                                );
                            }
                            let _ = self.config.save_to_file("Config.toml");
                            if let Err(e) =
                                self.app_state.reload_config(self.config.clone())
                            {
                                eprintln!(
                                    "Failed to reload config after quick gamepad mapping: {}",
                                    e
                                );
                            }
                        }
                    }
                    if ui.add(th.secondary_button("✕ 取消")).clicked() {
                        self.quick_gamepad_pending = None;
                        self.just_captured_input = false;
                    }
                    ui.label(th.hint_text("确认前不会写入任何映射"));
                });
            });
            ui.add_space(theme::SP_S);
        }

        let Some(slot_id) = self.gamepad_selected_slot else {
            render_slot_empty_state(ui, th);
            /* 右栏空位宽裕: 快捷卡 (连发/映射/震动/职业) 让玩家开箱即连 */
            self.render_quick_connect_card(ui, th);
            return;
        };
        let Some(slot) = get_slot(slot_id) else {
            return;
        };

        let is_capturing_trigger = matches!(
            self.key_capture_mode,
            KeyCaptureMode::QuickGamepadTrigger(id) if id == slot.id
        );
        let is_capturing_target = matches!(
            self.key_capture_mode,
            KeyCaptureMode::QuickGamepadTarget(id) if id == slot.id
        );

        th.panel(ui, None, |ui| {
            // 标题行: 槽位名 + 类别徽章
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(slot.label)
                        .size(18.0)
                        .strong()
                        .color(th.title),
                );
                ui.add_space(6.0);
                let (r, g, b) = slot.kind.base_rgb();
                th.badge(
                    ui,
                    slot.kind.legend(),
                    egui::Color32::WHITE,
                    egui::Color32::from_rgb(r, g, b),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(th.hint_text("已配置映射自动保存并即时生效"));
                });
            });
            ui.add_space(12.0);

            // 当前配置: 触发键 + 目标键徽章
            let trigger = slot_trigger_display(&self.config, slot);
            let targets = slot_targets(&self.config, slot);

            ui.horizontal(|ui| {
                ui.label(th.weak("触发键"));
                ui.add_space(4.0);
                if trigger == slot.default_trigger && targets.is_empty() {
                    th.badge(ui, &trigger, th.hint, th.faint)
                        .on_hover_text("默认键位 (尚未自定义)");
                } else {
                    th.trigger_badge(ui, &trigger);
                }
            });
            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                ui.label(th.weak("目标键"));
                ui.add_space(4.0);
                if targets.is_empty() {
                    ui.label(th.hint_text("未设置 (点击下方 ② 开始添加)"));
                } else {
                    for t in &targets {
                        th.target_badge(ui, t);
                    }
                }
            });

            ui.add_space(14.0);
            ui.separator();
            ui.add_space(12.0);

            // 步骤按钮
            let trigger_btn = if is_capturing_trigger {
                egui::Button::new(
                    egui::RichText::new("请按下手柄物理按键…").size(13.0).strong().color(egui::Color32::BLACK),
                )
                .fill(th.warn)
                .corner_radius(10.0)
                .min_size(egui::vec2(230.0, 34.0))
            } else {
                th.primary_button("① 捕获手柄触发键").min_size(egui::vec2(230.0, 34.0))
            };
            if ui
                .add_enabled(!is_capturing_target, trigger_btn)
                .clicked()
            {
                self.start_quick_trigger_capture(slot.id);
            }

            ui.add_space(8.0);

            let target_btn = if is_capturing_target {
                egui::Button::new(
                    egui::RichText::new("请按下键盘/鼠标按键…").size(13.0).strong().color(egui::Color32::BLACK),
                )
                .fill(th.warn)
                .corner_radius(10.0)
                .min_size(egui::vec2(230.0, 34.0))
            } else {
                egui::Button::new(
                    egui::RichText::new("② 捕获目标按键 (键盘/鼠标)").size(13.0).strong().color(egui::Color32::WHITE),
                )
                .fill(if self.dark_mode {
                    egui::Color32::from_rgb(72, 170, 235)
                } else {
                    egui::Color32::from_rgb(35, 130, 205)
                })
                .corner_radius(10.0)
                .min_size(egui::vec2(230.0, 34.0))
            };
            if ui
                .add_enabled(!is_capturing_trigger, target_btn)
                .clicked()
            {
                self.start_quick_target_capture(slot.id);
            }

            /* 绑定 Esc 本身: 物理 Esc = 取消捕获, 因此绑定 Esc 走显式按钮,
             * 结果同样进入「待确认」条 (确认应用才生效) */
            if is_capturing_trigger || is_capturing_target {
                if ui
                    .add(th.secondary_button("或直接绑定 Esc 键"))
                    .on_hover_text("物理 Esc 键用于取消捕获; 点此按钮可把 Esc 绑定为触发键/目标键")
                    .clicked()
                {
                    self.quick_gamepad_pending =
                        Some((slot.id, is_capturing_trigger, "ESC".to_string()));
                    self.key_capture_mode = KeyCaptureMode::None;
                    self.capture_pressed_keys.clear();
                    self.app_state.set_raw_input_capture_mode(false);
                    self.just_captured_input = false;
                }
            }

            ui.add_space(12.0);

            // 管理按钮行
            ui.horizontal(|ui| {
                if ui
                    .add(th.secondary_button("清空目标键"))
                    .clicked()
                {
                    clear_slot_targets(&mut self.config, slot);
                    let _ = self.config.save_to_file("Config.toml");
                    if let Err(e) = self.app_state.reload_config(self.config.clone()) {
                        eprintln!("Failed to reload config after clear targets: {}", e);
                    }
                }
                if ui.add(th.danger_button("删除该映射")).clicked() {
                    remove_slot_mapping(&mut self.config, slot);
                    let _ = self.config.save_to_file("Config.toml");
                    if let Err(e) = self.app_state.reload_config(self.config.clone()) {
                        eprintln!("Failed to reload config after remove mapping: {}", e);
                    }
                }
            });

            ui.add_space(12.0);
            ui.separator();
            ui.add_space(8.0);

            // 状态提示
            let hint = if is_capturing_trigger {
                ("⏳ 捕获中: 按下手柄上要绑定到该槽位的物理按键 (Esc 取消请重按槽位)", th.warn)
            } else if is_capturing_target {
                ("⏳ 捕获中: 按下键盘/鼠标按键, 松手后自动写入目标键", th.warn)
            } else if targets.is_empty() {
                ("提示: 只想用默认 Xbox 键位时, 直接点 ② 即可 (触发键保持默认)。", th.hint)
            } else {
                ("✓ 该槽位已就绪, 修改会实时写入 Config.toml。", th.good)
            };
            ui.label(egui::RichText::new(hint.0).size(11.5).color(hint.1));
        });
    }

    fn start_quick_trigger_capture(&mut self, slot_id: usize) {
        self.key_capture_mode = KeyCaptureMode::QuickGamepadTrigger(slot_id);
        self.quick_gamepad_pending = None; // 新捕获开始, 丢弃旧待确认
        self.capture_pressed_keys.clear();
        self.capture_initial_pressed = Self::poll_all_pressed_keys();
        self.app_state.set_raw_input_capture_mode(true);
        self.just_captured_input = true;
    }

    fn start_quick_target_capture(&mut self, slot_id: usize) {
        self.key_capture_mode = KeyCaptureMode::QuickGamepadTarget(slot_id);
        self.quick_gamepad_pending = None; // 新捕获开始, 丢弃旧待确认
        self.capture_pressed_keys.clear();
        self.capture_initial_pressed = Self::poll_all_pressed_keys();
        self.just_captured_input = true;
    }
}

/// 空状态矢量图标 (52px Gamepad, text_weak 保证细节可辨)。
fn render_slot_empty_icon(ui: &mut egui::Ui, th: &Theme) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(52.0, 52.0), egui::Sense::hover());
    crate::gui::widgets::Icon::Gamepad.paint(ui.painter(), rect, th.text_weak);
}

/// 未选中槽位时的引导空状态。
fn render_slot_empty_state(ui: &mut egui::Ui, th: &Theme) {
    th.panel(ui, None, |ui| {
        ui.add_space(24.0);
        ui.vertical_centered(|ui| {
            ui.set_min_width(ui.available_width());
            render_slot_empty_icon(ui, th);
            ui.add_space(8.0);
            ui.label(
                egui::RichText::new("点击左侧手柄上的任意按键开始")
                    .size(16.0)
                    .strong()
                    .color(th.text),
            );
            ui.add_space(10.0);
            ui.label(th.hint_text("① 选择手柄按键 (触发源)\n② 绑定键盘/鼠标目标键\n③ 实时生效, 无需手动保存"));
            ui.add_space(16.0);
        });
    });
}
