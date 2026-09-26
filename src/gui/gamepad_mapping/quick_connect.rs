//! 右栏快捷卡 (空槽态): 连发/映射/震动/职业 快捷控件 (v24.9) —— 原 1314-1581, C3 归位。

use crate::gui::SorahkGui;
use crate::gui::theme::{self, Theme};
use eframe::egui;

impl SorahkGui {
    /// 右栏快捷卡 (空槽态显示): 连发开关 / 映射选择 / 震动开关 / 职业选择。
    /// 控件语义与极简模式一致 (toggle_with_notify / 选中即应用), egui id 用
    /// "gp_" 前缀避免与极简模式冲突; 震动段仅 DFO 玩家可见。
    pub(super) fn render_quick_connect_card(&mut self, ui: &mut egui::Ui, th: &Theme) {
        use std::sync::atomic::Ordering;
        /* ★v24.9: 压缩行距/控件高 —— 一屏放下优先 (默认 interact 26 + 行距 8 太占高) */
        ui.spacing_mut().interact_size.y = 22.0;
        ui.spacing_mut().item_spacing.y = 4.0;
        th.panel(ui, Some("快速连接"), |ui| {
            /* ★v24.9: 连发 / 震动 并排一行 —— 一屏放下优先 (旧版两块各占一行) */
            let running = !self.app_state.is_paused();
            let (ltext, lfg, lbg) = if running {
                ("连发 · 开启中", th.good, th.good_soft)
            } else {
                ("连发 · 已暂停", th.hint, th.faint)
            };
            let vib_on = self.app_state.vibration_enabled.load(Ordering::Relaxed);
            let dfo = self.config.dfo_player;
            let gap = 8.0;
            let half = if dfo {
                (ui.available_width() - gap) * 0.5
            } else {
                ui.available_width()
            };
            ui.horizontal(|ui| {
                let turbo_btn = egui::Button::new(
                    egui::RichText::new(ltext).size(13.5).strong().color(lfg),
                )
                .fill(lbg)
                .corner_radius(egui::CornerRadius::same(10));
                if ui.add_sized([half, 32.0], turbo_btn).clicked() {
                    self.toggle_with_notify();
                }
                if dfo {
                    let (vtext, vfg, vbg) = if vib_on {
                        ("震动 · 开启", th.good, th.good_soft)
                    } else {
                        ("震动 · 关闭", th.hint, th.faint)
                    };
                    let vib_btn = egui::Button::new(
                        egui::RichText::new(vtext).size(13.5).strong().color(vfg),
                    )
                    .fill(vbg)
                    .corner_radius(egui::CornerRadius::same(10));
                    if ui.add_sized([half, 32.0], vib_btn).clicked() {
                        let v = self.app_state.vibration_enabled.load(Ordering::Relaxed);
                        self.app_state
                            .vibration_enabled
                            .store(!v, Ordering::Relaxed);
                    }
                }
            });
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
            ui.add_space(theme::SP_XS);

            /* 震动预设段 (仅 DFO 玩家; 震动开关已在上方与「连发」并排) */
            if self.config.dfo_player {
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
                /* ★v5: 分段滑块是"大面积行动区" —— 用深档主色 (白字 5.5:1);
                 * 亮档 accent 压白字只有 2.2:1, 之前那块页签看着发白。 */
                ui.painter().rect_filled(thumb, 6, th.btn_primary);
                let unselected = |a: f32| -> egui::Color32 {
                    let c = th.text_weak;
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
                                    .selected_text(classes[class_sel].name)
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
                    ui.add_sized([ui.available_width(), 26.0], inj_btn)
                        .on_hover_text("DfoVibration_OLD.dll 放入游戏目录即可自动注入 (未检测到则不注入)");
                } else {
                    let s4_btn = egui::Button::new(
                        egui::RichText::new("S4 震动模块自动注入 · 已启用")
                            .size(12.5)
                            .strong()
                            .color(th.good),
                    )
                    .fill(th.good_soft)
                    .corner_radius(egui::CornerRadius::same(10));
                    ui.add_sized([ui.available_width(), 26.0], s4_btn);
                }
            }
        });
    }
}
