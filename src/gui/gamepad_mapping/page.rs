//! 手柄页主体: 页面入口/鼠标映射弹窗/状态栏/总览/SVG 渲染 —— 原 682-1312 + 1115 区段, C3 归位。

use crate::gui::SorahkGui;
use crate::gui::theme::{self, Theme};
use crate::gui::types::GpFlow;
use eframe::egui;
use super::*;
impl SorahkGui {
    /// ★v22.0: 交互全部由 [`GpFlow`] 状态机驱动 (唯一真相)。
    /// 输入边沿 (手柄实时按键 / 键盘捕获) 在 `handle_gamepad_flow` 里处理, 本函数只渲染。
    pub(in crate::gui) fn render_gamepad_page(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        /* ★v24.0: 首次进入手柄页 → 把旧备注 (`手柄·X`) 迁移为系统备注 (`手柄X（系统）`),
         * 触发键/目标键原样保留 (会话内一次)。 */
        if !self.gp_cleanup_done {
            self.gp_cleanup_done = true;
            if migrate_slot_notes(&mut self.config) {
                let _ = self.config.save_to_file("Config.toml");
                if let Err(e) = self.app_state.reload_config(self.config.clone()) {
                    eprintln!("Failed to reload config after slot note migration: {e}");
                }
            }
        }
        /* ★v24.36 性能: 正常由启动预热线程备好 (首帧零成本); 主题切换期间沿用旧
         * 纹理, 后台重渲染完成后自动替换 —— 既不卡一下也不闪空白。 */
        if self.gamepad_texture.is_none() {
            self.gamepad_texture = load_gamepad_texture(ctx, self.dark_mode);
            self.gamepad_texture_dark = self.dark_mode;
        }
        let th = Theme::new(self.dark_mode);

        /* ★v22.0: 顶部识别状态条 + 就近提示 + 步骤引导 (一步一步来) */
        self.render_gamepad_status_bar(ui, &th);

        /* ★v24.8 排版重构: 两栏改用**显式宽度**约束。
         * 旧版右栏 `set_min_width(ui.available_width())` 在水平布局里会把整行撑破窗口 →
         * 整页出现横向滚动条 → 底部 chips 的 horizontal_wrapped 失去换行宽度被截断。 */
        th.card(ui, None, |ui| {
            let total_w = ui.available_width();
            let sp = theme::SP_L;
            let left_w = (total_w * 0.5).clamp(300.0, 400.0);
            let right_w = (total_w - left_w - sp).max(260.0);
            ui.horizontal_top(|ui| {
                // 左: 手柄图 + 热点 (横版构图; 宽度显式给定, 高度按原图比例)
                ui.vertical(|ui| {
                    ui.set_min_width(left_w);
                    ui.set_max_width(left_w);
                    let display_h = left_w * (SVG_SIZE.y / SVG_SIZE.x);
                    let clicked_slot =
                        self.render_gamepad_svg(ui, egui::vec2(left_w, display_h), &th);
                    /* 确认/删除等待态不响应改选, 避免打断用户确认 */
                    if let Some(id) = clicked_slot
                        && !self.gp_flow.is_busy()
                    {
                        self.select_slot_autostep(id);
                    }
                });

                ui.add_space(sp);

                // 右: 分步面板 (宽度显式给定 → 内部长文本才会正常换行)
                ui.vertical(|ui| {
                    ui.set_min_width(right_w);
                    ui.set_max_width(right_w);
                    self.render_gamepad_slot_panel(ui, &th);
                });
            });
        });

        self.render_gamepad_overview(ui, &th);
        self.render_mouse_map_dialog(ctx, &th);
    }

    /// ★v21.9: 「鼠标映射」小弹窗 —— 明确选择【滚动】或【点击】, 不再靠鼠标捕获 (避免污染)。
    pub(super) fn render_mouse_map_dialog(&mut self, ctx: &egui::Context, th: &Theme) {
        if !self.gamepad_mouse_menu {
            return;
        }
        let Some(slot_id) = self.gp_flow.slot() else {
            self.gamepad_mouse_menu = false;
            return;
        };
        let Some(slot) = get_slot(slot_id) else {
            self.gamepad_mouse_menu = false;
            return;
        };
        let mut close = false;
        let mut pick: Option<&'static str> = None;
        egui::Window::new("mouse_map_dialog")
            .id(egui::Id::new("gamepad_mouse_map_dialog"))
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .frame(
                egui::Frame::window(&ctx.style())
                    .fill(th.surface)
                    .corner_radius(egui::CornerRadius::same(14))
                    .stroke(egui::Stroke::new(1.0, th.stroke)),
            )
            .show(ctx, |ui| {
                ui.set_min_width(320.0);
                ui.label(
                    egui::RichText::new(format!("鼠标映射 · {}", slot.label))
                        .size(15.0)
                        .strong()
                        .color(th.title),
                );
                ui.add_space(8.0);
                ui.label(th.weak("滚动"));
                ui.horizontal_wrapped(|ui| {
                    for (label, key) in [
                        ("↑ 上滚", "SCROLL_UP"),
                        ("↓ 下滚", "SCROLL_DOWN"),
                        ("← 左滚", "SCROLL_LEFT"),
                        ("→ 右滚", "SCROLL_RIGHT"),
                    ] {
                        if ui.add(th.secondary_button(label)).clicked() {
                            pick = Some(key);
                        }
                    }
                });
                ui.add_space(10.0);
                ui.label(th.weak("点击"));
                ui.horizontal_wrapped(|ui| {
                    for (label, key) in
                        [("鼠标左键", "LBUTTON"), ("鼠标右键", "RBUTTON"), ("鼠标中键", "MBUTTON")]
                    {
                        if ui.add(th.secondary_button(label)).clicked() {
                            pick = Some(key);
                        }
                    }
                });
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    if ui.add(th.secondary_button("关闭")).clicked() {
                        close = true;
                    }
                    ui.label(th.hint_text("选一个即加入这个键位的映射"));
                });
            });
        if let Some(key) = pick {
            /* ★v24.0: 映射必须已由校对创建 */
            if find_slot_mapping_index(&self.config, slot).is_some() {
                crate::gui::gamepad_mapping::add_slot_target(&mut self.config, slot, key.to_string());
                let _ = self.config.save_to_file("Config.toml");
                if let Err(e) = self.app_state.reload_config(self.config.clone()) {
                    eprintln!("Failed to reload config after mouse map: {e}");
                }
                self.set_gamepad_toast(format!("已设置: {} → {key}", slot.label));
            } else {
                self.set_gamepad_warn("这个键还没校对 — 请先点「快速校对手柄按键」");
            }
            self.gp_flow = GpFlow::Selected { slot: slot.id };
            close = true;
        }
        if close {
            self.gamepad_mouse_menu = false;
        }
    }

    /// ★v22.0: 手柄页顶部状态区 —— 识别状态条 + 就近警告 + 成功提示 + 步骤引导。
    /// 每一步只讲一件事; 识别开关于此处显式可见/可停 (取代旧的"取消即退出识别")。
    pub(super) fn render_gamepad_status_bar(&mut self, ui: &mut egui::Ui, th: &Theme) {
        /* 提示自动淡出 */
        let now = std::time::Instant::now();
        if let Some(until) = self.gamepad_toast_until
            && now > until
        {
            self.gamepad_toast = None;
            self.gamepad_toast_until = None;
        }
        if let Some(until) = self.gamepad_warn_until
            && now > until
        {
            self.gamepad_warn = None;
            self.gamepad_warn_until = None;
        }

        /* ① 识别状态条: 一眼看到"现在识别的是哪台手柄", 并显式提供停止入口 */
        let identified = self.app_state.live_hid_pad().is_some();
        let device_name = self
            .live_pad()
            .map(|p| p.device_name)
            .or_else(|| self.raw_hid_status.clone());
        th.panel(ui, None, |ui| {
            ui.horizontal(|ui| {
                if identified {
                    let name = device_name.unwrap_or_else(|| "手柄".to_string());
                    /* ★v24.0: 已校对过 (存在系统备注映射) 的设备直接标绿, 方便识别 */
                    let calibrated = crate::gui::gamepad_mapping::SLOTS
                        .iter()
                        .any(|s| find_slot_mapping_index(&self.config, s).is_some());
                    let text = if calibrated {
                        format!("🎮 已校对 · 识别中: {name}")
                    } else {
                        format!("🎮 识别中: {name}")
                    };
                    ui.label(
                        egui::RichText::new(text)
                            .size(13.0)
                            .strong()
                            .color(th.good),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add(th.secondary_button("停止识别"))
                            .on_hover_text("停止读取手柄, 回到初始状态")
                            .clicked()
                        {
                            self.stop_identify();
                        }
                        if !matches!(self.gp_flow, GpFlow::Calibrate { .. })
                            && ui
                                .add(th.primary_button("快速校对手柄按键"))
                                .on_hover_text(
                                    "新手先点这个: 按提示把手柄上全部热点校对一遍 (约 20 秒), 自动记住键位; 中途不能跳过
完整流程: ① 一键校对一次 → ② 手柄按键快速映射设/改键位 → ③ 有遗漏按同一个手柄键重设",
                                )
                                .clicked()
                        {
                            self.start_calibration();
                        }
                    });
                } else {
                    ui.label(
                        egui::RichText::new("🎮 未校对 / 未识别")
                            .size(13.0)
                            .strong()
                            .color(th.hint),
                    );
                    ui.label(th.hint_text("新手请先「快速校对手柄按键」; 老手可直接点图上的键"));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        /* 空状态已有一个大按钮; 非空状态才在状态条上补一个 */
                        if self.gp_flow != GpFlow::Idle
                            && ui
                                .add(th.secondary_button("手柄按键快速映射"))
                                .on_hover_text("读取你的手柄, 之后按手柄上的键即可快速设置映射")
                                .clicked()
                        {
                            self.start_identify_first_pad();
                        }
                        if !matches!(self.gp_flow, GpFlow::Calibrate { .. })
                            && ui
                                .add(th.primary_button("快速校对手柄按键"))
                                .on_hover_text(
                                    "新手先点这个: 按提示把手柄上全部热点校对一遍 (约 20 秒), 自动记住键位; 中途不能跳过
完整流程: ① 一键校对一次 → ② 手柄按键快速映射设/改键位 → ③ 有遗漏按同一个手柄键重设",
                                )
                                .clicked()
                        {
                            self.start_calibration();
                        }
                    });
                }
            });
            /* 就近警告 (如"还没识别手柄"): 错误提示就放在出错的入口旁边 */
            if let Some(w) = self.gamepad_warn.clone() {
                ui.add_space(2.0);
                ui.label(
                    egui::RichText::new(format!("⚠ {w}"))
                        .size(12.0)
                        .color(th.warn),
                );
            }
            /* ★v24.9: 原「三步引导」独立一行已删 (一屏放下优先) —— 改挂在按钮悬停提示里 */
        });
        ui.add_space(theme::SP_XS);

        /* ② 成功提示 (纯成功反馈, 不再挂"退出识别"按钮) */
        if let Some(msg) = self.gamepad_toast.clone() {
            th.panel(ui, None, |ui| {
                ui.label(
                    egui::RichText::new(format!("✓ {msg}"))
                        .size(12.5)
                        .strong()
                        .color(th.good),
                );
            });
            ui.add_space(theme::SP_XS);
        }
        ui.add_space(theme::SP_XS);
    }

    /// 已配置手柄映射 chips 总览 (点击可跳选对应槽位)。
    /// ★v21.7d: 只列"已设目标键"的槽位 (真正生效的映射), 避免一堆未设目标的空映射把卡片撑爆;
    /// 已绑定触发键但还没设目标键的数量用一行提示带过。
    pub(super) fn render_gamepad_overview(&mut self, ui: &mut egui::Ui, th: &Theme) {
        let mut configured: Vec<&GamepadSlot> = Vec::new();
        let mut pending_target = 0usize;
        for s in SLOTS {
            if let Some(i) = find_slot_mapping_index(&self.config, s) {
                if self.config.mappings[i].target_keys.is_empty() {
                    pending_target += 1;
                } else {
                    configured.push(s);
                }
            }
        }

        let title = if configured.is_empty() {
            "我设好的映射".to_string()
        } else {
            format!("我设好的映射 ({})", configured.len())
        };
        let empty = configured.is_empty();

        th.card_with_actions(
            ui,
            Some(&title),
            |ui| {
                if pending_target > 0 {
                    ui.label(th.hint_text(format!(
                        "{pending_target} 个键还没设键盘键 (点手柄图上该键即可设置)"
                    )));
                } else if !configured.is_empty() {
                    ui.label(th.hint_text("点下面任一条可回去修改"));
                }
            },
            |ui| {
                if empty {
                    ui.label(th.hint_text(
                        "还没有可用的映射 — 点左边手柄图上的按键, 再按键盘上要代替的键, 两步即可",
                    ));
                    return;
                }
                /* ★v24.8: **手动按可见宽度分行** —— `horizontal_wrapped` 在父级宽度不受限时不会换行,
                 * 实机表现为 chips 一路向右溢出、被窗口裁掉。这里用 `clip_rect` 的有限宽度自行分行,
                 * 保证超出的 chip 一定落到下一行。 */
                let mut chips: Vec<(usize, String, String, bool)> = configured
                    .iter()
                    .map(|slot| {
                        let idx = find_slot_mapping_index(&self.config, slot).unwrap();
                        let m = &self.config.mappings[idx];
                        let full = format!("{} → {}", slot.label, m.target_keys.join("+"));
                        let label = theme::truncate_chars(&full, 14);
                        (slot.id, label, full, self.gp_selected_slot() == Some(slot.id))
                    })
                    .collect();

                let clip_w = ui.clip_rect().width();
                let avail_w = ui.available_width();
                let finite = |v: f32| v.is_finite() && v > 0.0;
                let wrap_w = {
                    let cap = if finite(clip_w) { clip_w - 28.0 } else { avail_w };
                    if finite(avail_w) { avail_w.min(cap) } else { cap }
                }
                .max(160.0);

                let font = egui::FontId::proportional(11.0);
                /* 先量每条 chip 宽度 (与渲染同字号) */
                let widths: Vec<f32> = chips
                    .iter()
                    .map(|c| {
                        ui.painter()
                            .layout_no_wrap(c.1.clone(), font.clone(), egui::Color32::WHITE)
                            .size()
                            .x
                            + 23.0 /* 内边距 9×2 + 行内间距 5 (实测值) */
                    })
                    .collect();
                let spacing = 5.0_f32;
                let n = chips.len();
                let total: f32 =
                    widths.iter().sum::<f32>() + spacing * n.saturating_sub(1) as f32;
                let rows_needed = ((total / wrap_w).ceil() as usize).max(1);
                let mut rows: Vec<Vec<usize>> = Vec::new();
                if rows_needed <= 1 {
                    rows.push((0..n).collect());
                } else if rows_needed == 2 {
                    /* ★v24.9: 两行**平衡切分** (各占一半) —— 贪心填充会把最后一条挤到第三行,
                     * 明明总宽 ≤ 两行容量却多出一行 (实机 20 条时出现过「+1」)。 */
                    let target = total / 2.0;
                    let mut cur = Vec::new();
                    let mut w = 0.0_f32;
                    for i in 0..n {
                        if !cur.is_empty() && rows.is_empty() && w + widths[i] > target {
                            rows.push(std::mem::take(&mut cur));
                            w = 0.0;
                        }
                        cur.push(i);
                        w += widths[i] + spacing;
                    }
                    if !cur.is_empty() {
                        rows.push(cur);
                    }
                } else {
                    /* 超过两行 (槽位上限 24, 通常到不了): 填满两行, 其余并成「+N」保持高度恒定 */
                    let mut cur = Vec::new();
                    let mut w = 0.0_f32;
                    for i in 0..n {
                        if rows.len() < 2 && !cur.is_empty() && w + widths[i] > wrap_w {
                            rows.push(std::mem::take(&mut cur));
                            w = 0.0;
                        }
                        cur.push(i);
                        w += widths[i] + spacing;
                    }
                    if !cur.is_empty() {
                        if rows.len() < 2 {
                            rows.push(cur);
                        } else {
                            let extra = cur.len();
                            chips.push((
                                usize::MAX,
                                format!("+{extra}"),
                                format!("还有 {extra} 条映射, 未在本卡显示"),
                                false,
                            ));
                            let last = rows.len() - 1;
                            rows[last].push(chips.len() - 1);
                        }
                    }
                }

                for row in rows {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing = egui::vec2(5.0, 4.0);
                        for &i in &row {
                            let (slot_id, label, full, selected) = &chips[i];
                            let (fg, bg) = if *selected {
                                /* ★v5 配色: 选中映射块改用深档主色 (白字 5.5:1) ——
                                 * 亮档强调色作大面积饱和填充会发飘。 */
                                (egui::Color32::WHITE, th.btn_primary)
                            } else {
                                (th.target_fg, th.target_bg)
                            };
                            let resp =
                                th.badge_clickable_sized(ui, label, fg, bg, 11.0)
                                .on_hover_text(full.clone());
                            if *slot_id != usize::MAX && resp.clicked() && !self.gp_flow.is_busy() {
                                /* 捕获/确认中不响应跳转, 保证流程不被带偏 */
                                self.gp_flow = GpFlow::Selected { slot: *slot_id };
                            }
                        }
                    });
                }
            },
        );
    }

    /// 绘制手柄图与热点, 返回被点击的槽位 id。
    pub(super) fn render_gamepad_svg(
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
        let selected_id = self.gp_selected_slot();
        let live_state = self.app_state.live_hid_state();
        /* ★v22.0: 捕获高亮来自状态机 (AwaitKb/AwaitPad) */
        let capture_slot = match &self.gp_flow {
            GpFlow::AwaitKb { slot, .. } | GpFlow::AwaitPad { slot } => Some(*slot),
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
            let is_capturing = matches!(capture_slot, Some(cid) if cid == slot.id);
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

            /* ★v20.2: 摇杆按下 (StickClick) 热点恢复显示 —— 与其余槽位同款
             * 玻璃底+主环+中心点 (09-08 曾改隐形热点, 用户要求加回) */
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
                /* ★v21.7b 实体手柄实时按下: 亮色填充 + 白环 (最醒目, 一眼看出按了哪个键) */
                let live = live_state
                    .as_ref()
                    .is_some_and(|l| self.gp_slot_is_live(slot.id, l));
                if live {
                    painter.circle_filled(
                        center,
                        r_eff * 1.45,
                        slot_col(if th.dark { 120 } else { 90 }),
                    );
                    painter.circle_filled(center, r_eff, slot_col(245));
                    painter.circle_stroke(
                        center,
                        r_eff,
                        egui::Stroke::new(2.6, egui::Color32::WHITE),
                    );
                    painter.circle_filled(center, r_eff * 0.15, egui::Color32::WHITE);
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
                /* 中心点 (单字符槽位与摇杆按下; Back/Start 保留文字空间) */
                if slot.short.chars().count() == 1 || is_click {
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

            /* 短标签 (方向箭头/肩键文字) 带微投影; ABXY 面键与 Back/Start 系统键
             * 圈内不绘字 (2026-09-08 用户要求留白); 摇杆按下 short 为空天然不绘 */
            if !slot.short.is_empty()
                && !is_click
                && !matches!(slot.kind, SlotKind::Button | SlotKind::System)
            {
                let font_size = match slot.short.chars().count() {
                    1 => 13.0,
                    2 => 9.5,
                    _ => 8.5,
                };
                let tcol = if is_capturing
                    || is_selected
                    || response.hovered()
                    || live_state
                        .as_ref()
                        .is_some_and(|l| self.gp_slot_is_live(slot.id, l))
                {
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
                "{} — 点它开始配置 (或直接按手柄上这个键)",
                slot.label
            ));

            if response.clicked() {
                clicked = Some(slot.id);
            }
        }

        clicked
    }
}
