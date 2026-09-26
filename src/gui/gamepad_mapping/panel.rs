//! 槽位面板状态机渲染 (map_done/calibrate/selected/await_kb/await_pad/confirm/confirm_delete/advanced/识别) —— 原 1814-2744, C3 归位。

use crate::gui::SorahkGui;
use crate::gui::theme::{self, Theme};
use crate::gui::types::{GpCaptureKind, GpEvent, GpFlow};
use eframe::egui;
use super::*;
impl SorahkGui {
    pub(super) fn gp_save_and_reload(&mut self) {
        let _ = self.config.save_to_file("Config.toml");
        if let Err(e) = self.app_state.reload_config(self.config.clone()) {
            eprintln!("Failed to reload config after gamepad calibration: {e}");
        }
    }

    /// 当前"应该高亮"的槽位: 选中槽位, 或校准向导的当前目标键。
    pub(super) fn gp_selected_slot(&self) -> Option<usize> {
        self.gp_flow.slot().or_else(|| match self.gp_flow {
            GpFlow::Calibrate { step, .. } => calibration_slot(step),
            _ => None,
        })
    }

    /// 右侧分步面板 —— 按 [`GpFlow`] 分派, 每一步只呈现一件事 ("一步一步来")。
    pub(super) fn render_gamepad_slot_panel(&mut self, ui: &mut egui::Ui, th: &Theme) {
        match self.gp_flow.clone() {
            GpFlow::Idle => {
                let identified = self.app_state.live_hid_pad().is_some();
                match render_slot_empty_state(ui, th, identified) {
                    EmptyAction::Map => self.start_identify_first_pad(),
                    EmptyAction::Calibrate => self.start_calibration(),
                    EmptyAction::None => {}
                }
                /* 右栏空位宽裕: 快捷卡 (连发/映射/震动/职业) 让玩家开箱即连 */
                self.render_quick_connect_card(ui, th);
            }
            GpFlow::Selected { slot } => self.render_slot_selected(ui, th, slot),
            GpFlow::AwaitKb { slot, keys } => self.render_slot_await_kb(ui, th, slot, keys),
            GpFlow::AwaitPad { slot } => self.render_slot_await_pad(ui, th, slot),
            GpFlow::ConfirmCapture { slot, kind, value } => {
                self.render_slot_confirm(ui, th, slot, kind, value)
            }
            GpFlow::ConfirmDelete { slot } => self.render_slot_confirm_delete(ui, th, slot),
            GpFlow::MapDone { slot } => self.render_slot_map_done(ui, th, slot),
            GpFlow::Calibrate { step, total } => self.render_slot_calibrate(ui, th, step, total),
        }
    }

    /// ★v24.4: 快速映射完成页 —— 三/四步走完后的落点。
    ///
    /// 提示用户可以继续按其他手柄键 (监听态已在 [`Self::gp_confirm_capture`] 里重新打开),
    /// 或用【编辑本映射】回到本键的第 1 步继续调整。
    pub(super) fn render_slot_map_done(&mut self, ui: &mut egui::Ui, th: &Theme, slot_id: usize) {
        let label = get_slot(slot_id).map(|s| s.label).unwrap_or("?");
        let mut edit_again = false;
        th.panel(ui, None, |ui| {
            ui.add_space(8.0);
            th.badge(ui, "已完成", egui::Color32::WHITE, th.good);
            ui.add_space(8.0);
            ui.label(
                egui::RichText::new(format!("「{label}」已更新"))
                    .size(16.0)
                    .strong()
                    .color(th.good),
            );
            ui.add_space(6.0);
            ui.label(th.hint_text(
                "本次按键已完成修改。您可以继续按下其他手柄按键，为其编辑新的映射键位；\
                 也可以留在当前页面，继续调整此按键的设置。",
            ));
            ui.add_space(10.0);
            if ui.add(th.primary_button("编辑本映射")).clicked() {
                edit_again = true;
            }
            ui.add_space(8.0);
        });
        if edit_again {
            /* 回到本键第 1 步 (Selected 面板: 看现状 / 设键盘键 / 校键) */
            self.gp_quick_listen = false;
            self.app_state.set_raw_input_capture_mode(false);
            self.select_slot(slot_id);
        }
    }

    /// ★v22.4 · 一键校准向导: 按提示逐个按键, 自动记录 (连发同款识别)。
    /// ★v22.5: **必须把全部键按一遍才能完成, 不提供跳过** —— 保证全覆盖、不漏键。
    pub(super) fn render_slot_calibrate(
        &mut self,
        ui: &mut egui::Ui,
        th: &Theme,
        step: usize,
        total: usize,
    ) {
        let current_slot = calibration_slot(step);
        let current_label = current_slot
            .and_then(get_slot)
            .map(|s| s.label)
            .unwrap_or("?");
        th.panel(ui, None, |ui| {
            ui.horizontal(|ui| {
                /* ★v5 配色: 标题徽章用"柔底 + 强调文字" (旧为白字压饱和紫底, 太吵) */
                th.badge(ui, "快速校对手柄按键", th.accent_text, th.accent_soft);
                ui.add_space(6.0);
                ui.label(
                    egui::RichText::new(format!("{}/{}", step + 1, total))
                        .size(15.0)
                        .strong()
                        .color(th.title),
                );
            });
            ui.add_space(10.0);
            ui.label(
                egui::RichText::new(format!("请操作「{current_label}」然后松开"))
                    .size(17.0)
                    .strong()
                    .color(th.accent),
            );
            ui.add_space(6.0);
            /* ★v24.2: 按槽位类型给出更明确的提示 —— 摇杆"按下"与"推方向"要分开, 乱动就是串键的来源 */
            let tip = match current_slot.and_then(get_slot).map(|s| s.kind) {
                Some(SlotKind::StickClick) => {
                    "摇杆按下: 垂直按下去再松开, 不要推动摇杆（推动了会被判为不干净, 让你重按）"
                }
                Some(SlotKind::Stick) => {
                    "摇杆方向: 推到底并保持一下再松开, 一次只推一个方向（斜着推会让你重按）"
                }
                _ => "按键/扳机/十字键: 按一下再松开（别同时碰摇杆）",
            };
            ui.label(th.hint_text(format!(
                "{tip}\n必须把全部热点都操作一遍才能完成。"
            )));
            ui.add_space(8.0);
            /* ★v22.7: 完成进度 —— 已完成项**变绿**、当前项强调色、未完成灰; 一眼识别还差哪些。
             * 用 LayoutJob 实现"单段可换行 + 多颜色" (徽章/多 label 会被面板右缘裁掉)。 */
            let font = egui::FontId::proportional(12.0);
            let mut job = egui::text::LayoutJob::default();
            job.wrap.max_width = ui.available_width();
            for (i, id) in CALIBRATION_ORDER.iter().enumerate() {
                let label = calibration_short_label(*id);
                let (text, color) = if i < step {
                    /* ★v24.2: 已完成项直接显示"记下的触发键简称" —— 串键 / 串进 045E 命名一眼可见,
                     * 不用再跑去连发映射区逐条核对。 */
                    let rec = get_slot(*id)
                        .map(|s| slot_trigger_display(&self.config, s))
                        .filter(|s| !s.is_empty())
                        .map(|s| {
                            let short = short_trigger_name(&s);
                            if short.chars().count() > 22 {
                                format!("={}…", short.chars().take(22).collect::<String>())
                            } else {
                                format!("={short}")
                            }
                        })
                        .unwrap_or_default();
                    (format!("✓{label}{rec}"), th.good)
                } else if i == step {
                    (format!("▶{label}"), th.accent)
                } else {
                    (label.to_string(), th.hint)
                };
                job.append(
                    &text,
                    0.0,
                    egui::TextFormat {
                        font_id: font.clone(),
                        color,
                        ..Default::default()
                    },
                );
                job.append(
                    "   ",
                    0.0,
                    egui::TextFormat {
                        font_id: font.clone(),
                        color: th.hint,
                        ..Default::default()
                    },
                );
            }
            ui.label(job);
            ui.add_space(12.0);
            if ui.add(th.secondary_button("取消校准")).clicked() {
                self.gp_flow = self.gp_flow.transition(GpEvent::Cancel);
                self.end_calibration("已取消校准 (已按过的键仍然保留)");
            }
        });
    }

    /// 第 1 步 · 已选中一个手柄键: 看现状, 给出唯一的下一步。
    pub(super) fn render_slot_selected(&mut self, ui: &mut egui::Ui, th: &Theme, slot_id: usize) {
        let Some(slot) = get_slot(slot_id) else { return };
        let trigger = slot_trigger_display(&self.config, slot);
        let targets = slot_targets(&self.config, slot);
        let mapped = find_slot_mapping_index(&self.config, slot).is_some();
        let uncalibrated = !mapped;

        th.panel(ui, None, |ui| {
            step_header(ui, th, 1, &slot.label);
            ui.add_space(4.0);
            let (r, g, b) = slot.kind.base_rgb();
            th.badge(
                ui,
                slot.kind.legend(),
                egui::Color32::WHITE,
                egui::Color32::from_rgb(r, g, b),
            );
            ui.add_space(8.0);

            /* 一句话结论 (新手最先看这行) */
            let summary = if targets.is_empty() {
                egui::RichText::new(format!("{} 键  →  还没设键盘键", slot.label))
                    .size(15.0)
                    .strong()
                    .color(th.warn)
            } else {
                egui::RichText::new(format!(
                    "{} 键  →  键盘 {}",
                    slot.label,
                    targets.join(" + ")
                ))
                .size(15.0)
                .strong()
                .color(th.good)
            };
            ui.label(summary);
            ui.add_space(8.0);

            ui.horizontal(|ui| {
                ui.label(th.weak("手柄按键"));
                ui.add_space(4.0);
                if uncalibrated {
                    th.badge(ui, "未校对", th.warn, th.faint)
                        .on_hover_text("请先点右上「快速校对手柄按键」把手柄键位校一遍");
                } else {
                    th.trigger_badge(ui, &trigger);
                }
            });
            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                ui.label(th.weak("键盘按键"));
                ui.add_space(4.0);
                if targets.is_empty() {
                    ui.label(th.hint_text("还没设 — 点下面的按钮后按键盘"));
                } else {
                    for t in &targets {
                        th.target_badge(ui, t);
                    }
                }
                /* ★v24.35 序列接管明示: 该槽位挂了序列宏时键盘按键不生效;
                 * 序列本身坏了 (解析失败 = 整条不生效) 时给红字, 别说成"已接管" */
                let slot_seq_text = find_slot_mapping_index(&self.config, slot)
                    .map(|i| self.config.mappings[i].sequence_text.trim().to_string())
                    .unwrap_or_default();
                if !slot_seq_text.is_empty() {
                    let macros: std::collections::HashMap<String, String> = self
                        .config
                        .universal_macros
                        .iter()
                        .map(|m| (m.name.to_lowercase(), m.text.clone()))
                        .collect();
                    if crate::sequence::parse_sequence(&slot_seq_text, &macros).is_err() {
                        ui.label(
                            egui::RichText::new("✗ 序列宏解析失败 — 本条映射不生效")
                                .size(12.0)
                                .color(th.bad),
                        )
                        .on_hover_text("展开下方「更多设置」修正序列 (或清空它恢复键盘按键)");
                    } else if !targets.is_empty() {
                        ui.label(
                            egui::RichText::new("🔒 已由序列宏接管 (下方「更多设置」)")
                                .size(12.0)
                                .color(th.warn),
                        );
                    }
                }
            });

            ui.add_space(14.0);
            ui.separator();
            ui.add_space(12.0);

            /* 第 2 步入场: 只有一个主按钮, 不并列多个入口 (未校对 → 置灰) */
            let main_label = if targets.is_empty() {
                "第 ② 步: 按键盘键 (设置要代替的键)"
            } else {
                "修改键盘键"
            };
            let main_hint = if uncalibrated {
                "这个键还没校对 — 请先点右上「快速校对手柄按键」"
            } else {
                "按一下键盘上要代替的键 → 再点「确认」; 想设组合键可在确认页点「＋ 再加一个键」"
            };
            if ui
                .add_enabled(
                    !uncalibrated,
                    th.primary_button(main_label)
                        .min_size(egui::vec2(280.0, 36.0)),
                )
                .on_hover_text(main_hint)
                .clicked()
            {
                self.begin_set_kb(slot_id);
            }
            ui.add_space(8.0);
            ui.horizontal_wrapped(|ui| {
                if ui
                    .add(th.secondary_button("校对手柄键位"))
                    .on_hover_text("按一个手柄物理键, 指定这个槽位是哪个键 (只取一个键)")
                    .clicked()
                {
                    self.begin_set_pad(slot_id);
                }
                if ui
                    .add(th.secondary_button("鼠标映射…"))
                    .on_hover_text("把手柄这个键映射成鼠标: 滚动 或 点击")
                    .clicked()
                {
                    self.gamepad_mouse_menu = true;
                }
            });

            /* 更多设置 (连发 / 奔跑) —— 就地展开, 不再跳去连发页 */
            if mapped {
                ui.add_space(10.0);
                self.render_slot_advanced(ui, th, slot);
            }

            ui.add_space(12.0);
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(!targets.is_empty(), th.secondary_button("清空键盘按键"))
                    .clicked()
                {
                    clear_slot_targets(&mut self.config, slot);
                    let _ = self.config.save_to_file("Config.toml");
                    if let Err(e) = self.app_state.reload_config(self.config.clone()) {
                        eprintln!("Failed to reload config after clear targets: {}", e);
                    }
                }
                if ui
                    .add_enabled(mapped, th.danger_button("删除该映射"))
                    .on_hover_text("删除需要二次确认")
                    .clicked()
                {
                    self.gp_flow = self.gp_flow.transition(GpEvent::RequestDelete);
                }
            });

            /* ★v24.21 「完成修改」: 结束本键编辑 → 回正常界面 (右侧恢复快速校对/快速连接卡)。
             * 悬停文案带操作路径 —— 这个页面所有出口都按"新手看得懂"标准写。 */
            ui.add_space(12.0);
            if ui
                .add(th.primary_button("✓ 完成修改").min_size(egui::vec2(280.0, 36.0)))
                .on_hover_text(
                    "结束这个键的修改, 回到正常界面
(随时点图上的任意键可以再进来改)",
                )
                .clicked()
            {
                self.gp_flow = self.gp_flow.transition(GpEvent::FinishEdit);
                self.gp_capture_cleanup();
            }
        });
    }

    /// 第 2 步 · 等键盘按键 (可累加组合)。
    pub(super) fn render_slot_await_kb(
        &mut self,
        ui: &mut egui::Ui,
        th: &Theme,
        slot_id: usize,
        keys: Vec<String>,
    ) {
        let Some(slot) = get_slot(slot_id) else { return };
        th.panel(ui, None, |ui| {
            step_header(
                ui,
                th,
                2,
                &format!("{}: 按键盘上你想让它代替的那个键", slot.label),
            );
            ui.add_space(8.0);
            if !keys.is_empty() {
                ui.horizontal_wrapped(|ui| {
                    ui.label(th.weak("已捕获"));
                    for k in &keys {
                        th.target_badge(ui, k);
                    }
                });
                ui.add_space(6.0);
            }
            ui.label(th.hint_text(
                "按下后先出现「确认」按钮, 确认后才写入。想设组合键: 在确认页点「＋ 再加一个键」。\n\
                 本步只认键盘键 —— 想换手柄键请先点「取消」回到上一步。",
            ));
            ui.add_space(10.0);
            if ui.add(th.secondary_button("✕ 取消")).clicked() {
                self.cancel_gp();
            }
        });
    }

    /// 第 2 步 · 等手柄物理键 (校对该槽位是哪个键)。
    pub(super) fn render_slot_await_pad(&mut self, ui: &mut egui::Ui, th: &Theme, slot_id: usize) {
        let Some(slot) = get_slot(slot_id) else { return };
        let identified = self.app_state.live_hid_pad().is_some();
        th.panel(ui, None, |ui| {
            step_header(ui, th, 2, &format!("{}: 请按手柄上的一个键", slot.label));
            ui.add_space(8.0);
            ui.label(th.hint_text(
                "按一下手柄上的这个键**再松开** (扳机也照扣一次), 然后点「确认」。\n\
                 程序会记住这个槽位对应你手柄上的哪个键 (与连发映射同一套识别)。",
            ));
            if !identified {
                ui.add_space(4.0);
                ui.label(th.hint_text(
                    "按了没反应? 第三方手柄请先在设备管理里激活该手柄。",
                ));
            }
            ui.add_space(10.0);
            if ui.add(th.secondary_button("✕ 取消")).clicked() {
                self.cancel_gp();
            }
        });
    }

    /// 第 3 步 · 已捕获, 等确认。
    pub(super) fn render_slot_confirm(
        &mut self,
        ui: &mut egui::Ui,
        th: &Theme,
        slot_id: usize,
        kind: GpCaptureKind,
        value: String,
    ) {
        if get_slot(slot_id).is_none() {
            return;
        }
        th.panel(ui, None, |ui| {
            step_header(ui, th, 3, "确认");
            ui.add_space(8.0);
            ui.label(
                egui::RichText::new(format!("已捕获: {value}"))
                    .size(16.0)
                    .strong()
                    .color(th.accent),
            );
            ui.add_space(4.0);
            ui.label(th.hint_text(match kind {
                GpCaptureKind::Trigger => "点「确认」把该手柄键记为这个槽位",
                GpCaptureKind::Target => "点「确认」把它记成这个键位的键盘映射",
            }));
            ui.add_space(10.0);
            ui.horizontal_wrapped(|ui| {
                if ui
                    .add(th.primary_button("✓ 确认").min_size(egui::vec2(120.0, 34.0)))
                    .clicked()
                {
                    self.gp_confirm_capture();
                }
                if kind == GpCaptureKind::Target
                    && ui
                        .add(
                            th.secondary_button("＋ 再加一个键")
                                .min_size(egui::vec2(130.0, 34.0)),
                        )
                        .on_hover_text("继续按键盘, 组成组合键 (如 CTRL+F6)")
                        .clicked()
                {
                    self.gp_add_another_key();
                }
                if ui
                    .add(th.secondary_button("✕ 取消").min_size(egui::vec2(90.0, 34.0)))
                    .clicked()
                {
                    self.cancel_gp();
                }
            });
        });
    }

    /// 删除二次确认。
    pub(super) fn render_slot_confirm_delete(&mut self, ui: &mut egui::Ui, th: &Theme, slot_id: usize) {
        let Some(slot) = get_slot(slot_id) else { return };
        th.panel(ui, None, |ui| {
            ui.label(
                egui::RichText::new(format!("确定删除「{}」的映射?", slot.label))
                    .size(15.0)
                    .strong()
                    .color(th.warn),
            );
            ui.add_space(4.0);
            ui.label(th.hint_text("删除后需要重新设置才能恢复。"));
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                if ui.add(th.danger_button("删除")).clicked() {
                    self.gp_confirm_delete();
                }
                if ui.add(th.secondary_button("取消")).clicked() {
                    self.cancel_gp();
                }
            });
        });
    }

    /// ★v24.0: 该槽位此刻是否被按下 —— 直接看**它自己那条映射的触发键**与当前原始位组合是否相等。
    /// (与连发映射同一套判定; 未校对的槽位不亮。)
    pub(super) fn gp_slot_is_live(&self, slot_id: usize, live: &crate::state::LiveHidState) -> bool {
        let Some(slot) = get_slot(slot_id) else {
            return false;
        };
        match find_slot_mapping_index(&self.config, slot) {
            Some(i) => trigger_matches_raw(
                &self.config.mappings[i].trigger_key,
                live.vid,
                live.pid,
                live.raw_position,
            ),
            None => false,
        }
    }

    /// ★v24.0: 这一帧新按下的槽位 (识别态自动选中用)。
    pub(super) fn gp_newly_pressed_slot(
        &self,
        prev: &crate::state::LiveHidState,
        now: &crate::state::LiveHidState,
    ) -> Option<usize> {
        if now.raw_position == 0 || now.raw_position == prev.raw_position {
            return None;
        }
        SLOTS
            .iter()
            .find(|s| self.gp_slot_is_live(s.id, now))
            .map(|s| s.id)
    }

    /// ★v24.6: 该槽位此刻是否被 XInput 输入位图按下 (XInput 命名槽位, 如摇杆方向/按下)。
    pub(super) fn gp_slot_is_live_xinput(&self, slot_id: usize, vid: u16, mask: u32) -> bool {
        let Some(slot) = get_slot(slot_id) else {
            return false;
        };
        match find_slot_mapping_index(&self.config, slot) {
            Some(i) => trigger_matches_xinput(&self.config.mappings[i].trigger_key, vid, mask),
            None => false,
        }
    }

    /// ★v24.6: 这一帧新按下的槽位 (XInput live 位图版) —— 要求该槽位在上一帧还没被按下,
    /// 避免松手/位图收缩时把已按下的槽位重新选中。
    pub(super) fn gp_newly_pressed_slot_xinput(
        &self,
        vid: u16,
        mask: u32,
        prev: Option<(u16, u32)>,
    ) -> Option<usize> {
        SLOTS
            .iter()
            .find(|s| {
                self.gp_slot_is_live_xinput(s.id, vid, mask)
                    && !prev.is_some_and(|(pv, pm)| {
                        pv == vid && self.gp_slot_is_live_xinput(s.id, vid, pm)
                    })
            })
            .map(|s| s.id)
    }

    /// ★v22.0: 当前正在"实时识别"的手柄 (从已枚举列表里按 vid:pid 找)。
    pub(super) fn live_pad(&self) -> Option<crate::rawinput::RawHidGamepad> {
        let (v, p) = self.app_state.live_hid_pad()?;
        self.raw_hid_pads
            .iter()
            .find(|d| d.vid == v && d.pid == p)
            .cloned()
    }

    /// ★v24.0: 槽位映射由"一键校对"创建 (触发键 = 捕获到的原始命名)。
    /// 手柄页不再自行臆造触发键 —— 未校对就是没映射。
    /// 就近警告 (橙色, 数秒后消失)。
    pub(super) fn set_gamepad_warn(&mut self, msg: impl Into<String>) {
        self.gamepad_warn = Some(msg.into());
        self.gamepad_warn_until =
            Some(std::time::Instant::now() + std::time::Duration::from_secs(6));
    }

    /// 成功提示 (绿色, 数秒后消失)。
    pub(super) fn set_gamepad_toast(&mut self, msg: impl Into<String>) {
        self.gamepad_toast = Some(msg.into());
        self.gamepad_toast_until =
            Some(std::time::Instant::now() + std::time::Duration::from_secs(4));
    }

    /// 选中槽位 (不自动进入捕获)。
    pub(super) fn select_slot(&mut self, slot_id: usize) {
        self.gp_flow = self.gp_flow.transition(GpEvent::SelectSlot(slot_id));
    }

    /// 选中槽位; 若该键还没设键盘键 → 自动进入第 2 步 (识别态按手柄键的两步走)。
    pub(super) fn select_slot_autostep(&mut self, slot_id: usize) {
        self.select_slot(slot_id);
        let Some(slot) = get_slot(slot_id) else { return };
        let has_target = find_slot_mapping_index(&self.config, slot)
            .map(|i| !self.config.mappings[i].target_keys.is_empty())
            .unwrap_or(false);
        if !has_target {
            self.begin_set_kb(slot_id);
        }
    }

    /// 进入第 2 步 (设键盘键)。
    ///
    /// ★v24.0: 手柄页只是连发映射的可视化 UI —— **映射必须已由校对创建**。
    /// 未校对的槽位不给设键盘键 (先校对, 才有触发键)。
    pub(super) fn begin_set_kb(&mut self, slot_id: usize) {
        let Some(slot) = get_slot(slot_id) else { return };
        if find_slot_mapping_index(&self.config, slot).is_none() {
            self.set_gamepad_warn("这个键还没校对 — 请先点右上「快速校对手柄按键」");
            return;
        }
        self.gp_flow = self
            .gp_flow
            .transition(GpEvent::SelectSlot(slot_id))
            .transition(GpEvent::BeginSetKb);
        /* ★v24.4: 进入键盘捕获 → 退出"按手柄键继续映射"的监听态 (捕获通道也关掉) */
        self.gp_quick_listen = false;
        self.app_state.set_raw_input_capture_mode(false);
        self.gp_pad_capture.reset();
        self.capture_pressed_keys.clear();
        self.capture_initial_pressed = Self::poll_all_pressed_keys();
        self.just_captured_input = true;
    }

    /// 进入"校对手柄键位"。
    ///
    /// ★v22.3: 改走**连发同款原始报文捕获通道** (与是否识别过手柄无关) ——
    /// 语义 usage 通道对非标准手柄的 LT/RT 根本没有事件, 而原始位组合通道能精准抓到。
    /// 也不预先建映射 (触发键要等抓到真实按键名才知道)。
    pub(super) fn begin_set_pad(&mut self, slot_id: usize) {
        if get_slot(slot_id).is_none() {
            return;
        }
        self.gp_flow = self
            .gp_flow
            .transition(GpEvent::SelectSlot(slot_id))
            .transition(GpEvent::BeginSetPad);
        self.capture_pressed_keys.clear();
        self.just_captured_input = true;
        self.gp_pad_capture.reset();
        self.gp_quick_listen = false;
        self.app_state.set_raw_input_capture_mode(true);
    }

    /// 取消: **只回退一层**并关掉捕获通道 —— 保留手柄识别 (旧版"取消即退出识别"已废弃)。
    pub(super) fn cancel_gp(&mut self) {
        self.gp_flow = self.gp_flow.transition(GpEvent::Cancel);
        self.gp_capture_cleanup();
    }

    /// ★v24.21 捕获相关临时状态清理 (「取消」与「完成修改」共用)。
    pub(super) fn gp_capture_cleanup(&mut self) {
        self.capture_pressed_keys.clear();
        self.just_captured_input = false;
        self.gp_pad_capture.reset();
        self.gp_quick_listen = false;
        self.app_state.set_raw_input_capture_mode(false);
    }

    /// 停止识别 → 回初始状态 (现在唯一的"退出识别"入口)。
    pub(super) fn stop_identify(&mut self) {
        self.app_state.set_live_hid_pad(None);
        self.app_state.set_raw_input_capture_mode(false);
        self.gp_flow = self.gp_flow.transition(GpEvent::Reset);
        self.capture_pressed_keys.clear();
        self.just_captured_input = false;
        self.gp_pad_capture.reset();
        self.gp_quick_listen = false;
        self.gp_prev_xinput = None;
        self.gamepad_toast = None;
        self.gamepad_toast_until = None;
    }

    /// 确认写入捕获到的触发键/目标键。
    pub(super) fn gp_confirm_capture(&mut self) {
        let GpFlow::ConfirmCapture {
            slot: slot_id,
            kind,
            value,
        } = self.gp_flow.clone()
        else {
            return;
        };
        let Some(slot) = get_slot(slot_id) else { return };
        match kind {
            GpCaptureKind::Trigger => {
                /* ★v24.0: 校准 = 直接写这条映射的触发键 (备注已是系统备注)。
                 * 与连发映射完全一致, 不再有独立的校准表。 */
                set_slot_trigger(&mut self.config, slot, value.clone());
                self.set_gamepad_toast(format!("已校对: {} 键", slot.label));
            }
            GpCaptureKind::Target => {
                if find_slot_mapping_index(&self.config, slot).is_none() {
                    self.set_gamepad_warn("这个键还没校对 — 请先点「快速校对手柄按键」");
                    self.gp_flow = self.gp_flow.transition(GpEvent::Cancel);
                    return;
                }
                /* ★v24.18: **替换**而不是追加 —— 捕获结果就是这个槽位完整的目标键。
                 * (原来是 add: 已有「X+A」时再捕获「X」因为 X 已在集合里, 看着像没生效) */
                set_slot_targets(&mut self.config, slot, value.clone());
                self.set_gamepad_toast(format!("已设置: {} → {}", slot.label, value));
            }
        }
        let _ = self.config.save_to_file("Config.toml");
        if let Err(e) = self.app_state.reload_config(self.config.clone()) {
            eprintln!("Failed to reload config after capture confirm: {e}");
        }
        self.gp_flow = self.gp_flow.transition(GpEvent::ConfirmCapture);
        /* ★v24.4: 完成页允许"直接按其他手柄键继续映射"。
         * ★v24.6: 用 live 位图 (identify 状态已开) 自动选中, **不再启用捕获模式** ——
         * 捕获模式会抑制正常映射派发, 导致进游戏后奔跑/方向失效。 */
        if matches!(self.gp_flow, GpFlow::MapDone { .. }) {
            self.gp_pad_capture.reset();
            self.gp_quick_listen = true;
        }
    }

    /// 确认页「再加一个键」→ 回第 2 步继续捕获 (组成组合键)。
    pub(super) fn gp_add_another_key(&mut self) {
        self.gp_flow = self.gp_flow.transition(GpEvent::AddAnotherKey);
        self.capture_pressed_keys.clear();
        self.capture_initial_pressed = Self::poll_all_pressed_keys();
        self.just_captured_input = true;
    }

    /// 删除二次确认 → 真删。
    pub(super) fn gp_confirm_delete(&mut self) {
        let GpFlow::ConfirmDelete { slot: slot_id } = self.gp_flow.clone() else {
            return;
        };
        let Some(slot) = get_slot(slot_id) else { return };
        remove_slot_mapping(&mut self.config, slot);
        let _ = self.config.save_to_file("Config.toml");
        if let Err(e) = self.app_state.reload_config(self.config.clone()) {
            eprintln!("Failed to reload config after remove mapping: {e}");
        }
        self.set_gamepad_toast(format!("已删除: {}", slot.label));
        self.gp_flow = self.gp_flow.transition(GpEvent::ConfirmDelete);
    }

    /// 内联「更多设置」(连发 / 简易奔跑 / 重推奔跑) —— 不再跳去连发页。
    pub(super) fn render_slot_advanced(
        &mut self,
        ui: &mut egui::Ui,
        th: &Theme,
        slot: &'static GamepadSlot,
    ) {
        let Some(idx) = find_slot_mapping_index(&self.config, slot) else {
            return;
        };
        let (mut turbo, mut dtap, mut run, mut run_thr) = {
            let m = &self.config.mappings[idx];
            (
                m.turbo_enabled,
                m.double_tap_enabled,
                m.run_enabled,
                m.run_threshold,
            )
        };
        egui::CollapsingHeader::new("更多设置 (连发 / 奔跑)")
            .id_salt(("gp_slot_advanced", slot.id))
            .show(ui, |ui| {
                let mut changed = false;
                ui.horizontal_wrapped(|ui| {
                    changed |= ui
                        .checkbox(&mut turbo, "⚡ 连发")
                        .on_hover_text("勾选 = 此键触发连发; 不勾选 = 单发")
                        .changed();
                    ui.add_enabled_ui(!run, |ui| {
                        changed |= ui
                            .checkbox(&mut dtap, "简易奔跑")
                            .on_hover_text(if run {
                                "与「重推奔跑」互斥"
                            } else {
                                "按一次自动补一次敲击 (DNF 简易双击跑)"
                            })
                            .changed();
                    });
                    ui.add_enabled_ui(!dtap, |ui| {
                        changed |= ui
                            .checkbox(&mut run, "🏃 重推奔跑")
                            .on_hover_text(if dtap {
                                "与「简易奔跑」互斥"
                            } else {
                                "摇杆轻推=走, 推过阈值=自动补一次 (游戏判定双击→奔跑)"
                            })
                            .changed();
                    });
                });
                if run {
                    changed |= ui
                        .add(egui::Slider::new(&mut run_thr, 10..=100).text("重推阈值"))
                        .changed();
                }
                if changed {
                    let m = &mut self.config.mappings[idx];
                    m.turbo_enabled = turbo;
                    m.double_tap_enabled = dtap && !run;
                    m.run_enabled = run;
                    m.run_threshold = run_thr;
                    let _ = self.config.save_to_file("Config.toml");
                    if let Err(e) = self.app_state.reload_config(self.config.clone()) {
                        eprintln!("Failed to reload config after slot advanced edit: {e}");
                    }
                }
            });

            /* ★v24.32 序列宏 (连招): 手柄键也能挂序列 —— 按下 = 依次自动按键盘键 */
            ui.add_space(6.0);
            egui::CollapsingHeader::new(
                egui::RichText::new("序列宏 (按一下自动打一套连招)").size(13.0),
            )
            .id_salt(("gp_slot_sequence", slot.id))
            .show(ui, |ui| {
                ui.label(th.hint_text(
                    "例: 手柄按一下 B = 自动依次按 下、右、Z (顺序指令技)
                     录制或逐步添加; 触发键就是当前手柄键, 输出是键盘动作",
                ));
                ui.add_space(4.0);
                self.render_sequence_steps_editor(ui, idx, th);
            });
    }

    /// ★v22.0/★v22.6: 「手柄按键快速映射」→ 枚举并读取选中的手柄 (原「开始识别手柄」)。
    pub(super) fn start_identify_first_pad(&mut self) {
        if self.raw_hid_pads.is_empty() {
            self.raw_hid_pads = crate::rawinput::enumerate_raw_hid_gamepads();
            self.raw_hid_pads_fetched = Some(std::time::Instant::now());
        }
        if self.raw_hid_pads.is_empty() {
            self.set_gamepad_warn("没找到第三方手柄 — 请确认已连接 (官方 Xbox 手柄无需识别)");
            return;
        }
        if self.raw_hid_selected >= self.raw_hid_pads.len() {
            self.raw_hid_selected = 0;
        }
        let pad = self.raw_hid_pads[self.raw_hid_selected].clone();
        self.app_state.set_live_hid_pad(Some((pad.vid, pad.pid)));
        self.raw_hid_status = Some(pad.device_name.clone());
        /* ★v24.2: 进入"监听"态 —— 按手柄上任意已校对的键, 直接选中该槽位并开始设键盘键。
         * ★v24.6: 只开 live 状态匹配 (raw + XInput 位图), **不启用捕获模式**, 不干扰正常映射派发。 */
        self.gp_quick_listen = true;
        self.set_gamepad_toast(format!(
            "已开始识别: {} — 按手柄上已校对的键, 会直接跳到它的键盘键设置",
            pad.device_name
        ));
    }
}

/// ★v22.0: 步骤标记 —— 「第 N 步」小徽章 + 一句话标题 (一步一步来)。
fn step_header(ui: &mut egui::Ui, th: &Theme, step: u8, title: &str) {
    ui.horizontal(|ui| {
        th.badge(ui, &format!("第 {step} 步"), th.accent_text, th.accent_soft);
        ui.add_space(6.0);
        ui.label(
            egui::RichText::new(title)
                .size(15.0)
                .strong()
                .color(th.title),
        );
    });
}

/// ★v22.6: 空状态里用户点了哪个入口。
#[derive(PartialEq, Eq)]
enum EmptyAction {
    None,
    /// 「手柄按键快速映射」(读取手柄 → 按手柄键快速设映射)
    Map,
    /// 「快速校对手柄按键」
    Calibrate,
}

/// 未选中槽位时的引导空状态。
/// ★v24.9 压缩: 去掉 64px 大图标与竖排大按钮 —— 两个入口**并排一行**, 让整页一屏放下。
/// 未识别 → 「快速校对手柄按键」+「手柄按键快速映射」并排; 已识别 → 一句提示。
fn render_slot_empty_state(ui: &mut egui::Ui, th: &Theme, identified: bool) -> EmptyAction {
    let mut action = EmptyAction::None;
    /* ★v24.9: 不再单独成卡 (省一层内边距) —— 直接排在「快速连接」卡上方 */
    {
        if identified {
            ui.label(
                egui::RichText::new("点左边手柄图上的按键, 或直接按手柄上的键")
                    .size(14.0)
                    .strong()
                    .color(th.title),
            );
        } else {
            let gap = 8.0;
            let w = ((ui.available_width() - gap) * 0.5).max(120.0);
            ui.horizontal(|ui| {
                if ui
                    .add_sized(
                        [w, 36.0],
                        egui::Button::new(
                            egui::RichText::new("快速校对手柄按键")
                                .size(14.0)
                                .strong()
                                .color(egui::Color32::WHITE),
                        )
                        /* ★v5 配色: 大块行动按钮用深档主色 (亮档强调色大面积填充会发飘) */
                        .fill(th.btn_primary)
                        .corner_radius(egui::CornerRadius::same(theme::RADIUS_CTRL)),
                    )
                    .on_hover_text("按提示把手柄上全部热点校对一遍 (约 20 秒), 自动记住键位")
                    .clicked()
                {
                    action = EmptyAction::Calibrate;
                }
                if ui
                    .add_sized(
                        [w, 36.0],
                        egui::Button::new(
                            egui::RichText::new("手柄按键快速映射")
                                .size(14.0)
                                .strong()
                                .color(th.accent),
                        )
                        .fill(egui::Color32::TRANSPARENT)
                        .stroke(egui::Stroke::new(1.2, th.accent_soft))
                        .corner_radius(egui::CornerRadius::same(theme::RADIUS_CTRL)),
                    )
                    .on_hover_text("读取你的手柄; 之后按手柄上的键即可快速设置映射")
                    .clicked()
                {
                    action = EmptyAction::Map;
                }
            });
        }
    }
    action
}
