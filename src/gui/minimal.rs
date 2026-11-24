//! Main window implementation and rendering logic.
//!
//! ★R2.14 前后端分离: 配置读写全部走 backend (with_config / update_config)，
//! 本文件不再直持 AppConfig / AppState。

use crate::gui::SorahkGui;
use crate::gui::theme::{self, Theme};
use crate::gui::widgets;

use eframe::egui;
use super::main_window::FrameState;

impl SorahkGui {
    pub(super) fn set_minimal_mode(&mut self, ctx: &egui::Context, on: bool) {
        self.minimal_mode = on;
        if on {
            /* ★v20.4: 与经典模式互斥 —— 进入极简即退经典 (双标记同真会让外壳分支打架);
             * 同时解除经典模式的 820×600 最小尺寸, 否则小窗缩不下去 */
            self.classic_mode = false;
            ctx.send_viewport_cmd(egui::ViewportCommand::MinInnerSize(egui::Vec2::ZERO));
        }
        if !on {
            /* 退出极简: 极简矩形落盘 + 精确还原完整模式上次的窗口位置+尺寸 */
            if let Some((pos, sz)) = self.minimal_window_rect {
                let _ = self.backend.update_config(
                    crate::ui_api::Commit::Save,
                    |c| c.window_rect_minimal = Some([pos.x, pos.y, sz.x, sz.y]),
                );
            }
            let normal_cfg = self.backend.with_config(|c| c.window_rect_normal);
            let restore = self.normal_window_rect.take().or_else(|| {
                normal_cfg.map(|r| (egui::pos2(r[0], r[1]), egui::vec2(r[2], r[3])))
            });
            if let Some((pos, sz)) = restore {
                let _ = self.backend.update_config(
                    crate::ui_api::Commit::Save,
                    |c| c.window_rect_normal = Some([pos.x, pos.y, sz.x, sz.y]),
                );
                ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(pos));
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(sz));
            } else if let Some(sz) = self.normal_window_size.take() {
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(sz));
            }
            if self.normal_was_maximized {
                ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(true));
            }
            self.normal_was_maximized = false;
            // 无论走哪条恢复路径都清掉"已进入"标记: 否则第二次进极简时
            // is_none() 守卫短路, 不再重新采样完整窗口尺寸/最大化状态,
            // 退出恢复的是上一次的旧矩形
            self.normal_window_size = None;
        }
        /* ★R2.14: 模式标记落盘走 backend (Save 档) */
        let (minimal, classic) = (self.minimal_mode, self.classic_mode);
        let _ = self.backend.update_config(crate::ui_api::Commit::Save, |c| {
            c.minimal_mode = minimal;
            c.classic_mode = classic;
        });
    }


    /// 极简模式顶栏: 品牌 + 预设快切 + 主题 + 返回完整界面 (永远可见)。
    pub(super) fn render_minimal_top_bar(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let th = self.theme();
        // 拖动区先于按钮注册 (同完整顶栏)
        self.render_title_bar_drag(ui, ctx);
        ui.horizontal_centered(|ui| {
            ui.add_space(2.0);
            let (rect, _) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
            ui.painter().rect_filled(rect, 5, th.accent);
            // 短品牌名 (小窗放不下完整标题)
            ui.label(
                egui::RichText::new("DfoVibration-V3")
                    .size(13.0)
                    .strong()
                    .family(Theme::font_bold())
                    .color(th.title),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                self.render_window_controls(ui, ctx);
                ui.add_space(theme::SP_S);
                if widgets::icon_button(ui, &th, widgets::Icon::Theme, "切换主题").clicked() {
                    self.dark_mode = !self.dark_mode;
                    /* ★R2.14: 主题持久化走 backend (Save 档) */
                    let _ = self.backend.update_config(
                        crate::ui_api::Commit::Save,
                        |c| c.dark_mode = self.dark_mode,
                    );
                    if let Some(temp_config) = &mut self.temp_config {
                        temp_config.dark_mode = self.dark_mode;
                    }
                }
                ui.add_space(theme::SP_XS);
                // 返回完整界面 (顶栏常驻)
                if ui
                    .add(
                        egui::Button::new(
                            egui::RichText::new("⤺ 完整界面")
                                .size(11.5)
                                .strong()
                                .color(egui::Color32::WHITE),
                        )
                        .fill(th.btn_primary)
                        .corner_radius(egui::CornerRadius::same(theme::RADIUS_CTRL))
                        .min_size(egui::vec2(0.0, 26.0)),
                    )
                    .clicked()
                {
                    self.set_minimal_mode(ctx, false);
                }
            });
        });
    }


    /// 极简页音频区: 总开关 + 实时状态 + A/B/C 算法选择。
    ///
    /// 所有玩家可见（不跟随 `dfo_player` 隐藏）；配置写入统一走
    /// `backend.update_audio_config`，A/B/C 选择由 backend 同步双字段。
    fn render_minimal_audio_section(&mut self, ui: &mut egui::Ui) {
        use crate::state::audio_state::{
            audio_status_name, resolve_algorithm_mode, AUDIO_ALGO_A, AUDIO_ALGO_B, AUDIO_ALGO_C,
            AUDIO_STATUS_NO_DEVICE, AUDIO_STATUS_OFF, AUDIO_STATUS_PAUSED, AUDIO_STATUS_RUNNING,
            AUDIO_STATUS_WAIT_GAME, AUDIO_STATUS_WAIT_SOUND,
        };
        let th = self.theme();
        let (mut audio_on, mut mode) = self.backend.with_config(|c| {
            let a = &c.audio_haptics;
            (
                a.enabled,
                resolve_algorithm_mode(a.audio_algorithm_mode.as_deref(), a.smart_enabled),
            )
        });
        let status = self.backend.audio_status_code();

        // 音频总开关 (与连发/震动大开关同规格)
        let (text, fg, bg) = if audio_on {
            ("音频震动 · 开启", th.good, th.good_soft)
        } else {
            ("音频震动 · 关闭", th.hint, th.faint)
        };
        let audio_btn = egui::Button::new(egui::RichText::new(text).size(16.0).strong().color(fg))
            .fill(bg)
            .corner_radius(egui::CornerRadius::same(12))
            .min_size(egui::vec2(260.0, 46.0));
        if ui.add_sized([280.0, 46.0], audio_btn).clicked() {
            let next = !audio_on;
            self.backend.update_audio_config(|a| a.enabled = next);
            audio_on = next;
        }
        ui.add_space(theme::SP_XS);

        // 状态行: 关闭时说明未开启; 开启后直接显示引擎状态
        let (dot, pulsing, stat_text) = if !audio_on {
            (th.hint, false, "音频震动未开启".to_string())
        } else {
            match status {
                AUDIO_STATUS_RUNNING => (
                    th.good,
                    true,
                    format!("音频震动 · {}", audio_status_name(status)),
                ),
                AUDIO_STATUS_WAIT_GAME | AUDIO_STATUS_WAIT_SOUND => (
                    th.warn,
                    false,
                    format!("音频震动 · {}", audio_status_name(status)),
                ),
                AUDIO_STATUS_NO_DEVICE | AUDIO_STATUS_PAUSED => (
                    th.warn,
                    false,
                    format!("音频震动 · {}", audio_status_name(status)),
                ),
                AUDIO_STATUS_OFF => (th.info, false, "音频震动 · 启动中".to_string()),
                _ => (
                    th.hint,
                    false,
                    format!("音频震动 · {}", audio_status_name(status)),
                ),
            }
        };
        let dot_w = 4.5 * 2.4;
        let stat_w = ui
            .painter()
            .layout_no_wrap(
                stat_text.clone(),
                egui::FontId::proportional(12.0),
                egui::Color32::WHITE,
            )
            .size()
            .x;
        let gap = theme::SP_XS + ui.spacing().item_spacing.x;
        let _ = ui.allocate_ui_with_layout(
            egui::vec2(dot_w + gap + stat_w, 18.0),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                widgets::status_dot(ui, dot, pulsing, 4.5);
                ui.add_space(theme::SP_XS);
                ui.label(th.weak(stat_text.clone()));
            },
        );
        ui.add_space(theme::SP_XS);

        // A/B/C 三段选择 (定宽 300, 与上面两个大开关同宽域)
        let seg_id = egui::Id::new("minimal_audio_mode");
        let labels = ["A 经典", "B 智能", "C 事件"];
        let seg_w = 300.0_f32;
        let seg_h = 26.0_f32;
        let (seg_rect, _) =
            ui.allocate_exact_size(egui::vec2(seg_w, seg_h), egui::Sense::hover());
        let third = seg_rect.width() / 3.0;
        let mut picked: Option<u8> = None;
        for i in 0..labels.len() {
            let rect = egui::Rect::from_x_y_ranges(
                (seg_rect.left() + third * i as f32)
                    ..=(seg_rect.left() + third * (i + 1) as f32),
                seg_rect.top()..=seg_rect.bottom(),
            );
            let resp = ui
                .interact(rect, seg_id.with(i), egui::Sense::click())
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text(match i {
                    0 => "A 经典：原手感",
                    1 => "B 智能：远近高低自适应",
                    _ => "C 事件：BGM 筛选 + 事件包络",
                });
            if resp.clicked() {
                picked = Some(match i {
                    0 => AUDIO_ALGO_A,
                    1 => AUDIO_ALGO_B,
                    _ => AUDIO_ALGO_C,
                });
            }
        }
        if let Some(next) = picked {
            if next != mode {
                self.backend.set_audio_algorithm_mode(next);
                mode = next;
            }
        }
        let mode_idx = match mode {
            AUDIO_ALGO_B => 1,
            AUDIO_ALGO_C => 2,
            _ => 0,
        };
        ui.painter().rect_filled(seg_rect, 8, th.faint);
        let thumb_w = third - 4.0;
        let thumb_x = seg_rect.left() + 2.0 + third * mode_idx as f32;
        let thumb = egui::Rect::from_min_size(
            egui::pos2(thumb_x, seg_rect.top() + 3.0),
            egui::vec2(thumb_w, seg_h - 6.0),
        );
        ui.painter().rect_filled(thumb, 6, th.accent);
        for (i, label) in labels.iter().enumerate() {
            ui.painter().text(
                egui::pos2(
                    seg_rect.left() + third * (i as f32 + 0.5),
                    seg_rect.center().y,
                ),
                egui::Align2::CENTER_CENTER,
                label.to_string(),
                egui::FontId::proportional(12.0),
                if i == mode_idx {
                    egui::Color32::WHITE
                } else {
                    th.text_weak
                },
            );
        }
        ui.add_space(theme::SP_XS);
        ui.label(th.hint_text(match mode_idx {
            0 => "A 经典：原手感，逐字节不变",
            1 => "B 智能：远近高低自适应",
            _ => "C 事件：BGM 筛选 + 事件包络",
        }));
    }


    /// 极简模式页面: 连发开关 + 预设 + 震动开关。
    ///
    /// 布局用单层 `top_down(Align::Center)`: 所有元素自动水平居中。
    /// 不要在此处嵌套 horizontal_centered —— 它是 centered_and_justified 语义,
    /// 会垂直占满剩余空间并把后续内容挤出视口 (曾导致文字全部消失)。
    pub(super) fn render_minimal_page(&mut self, ui: &mut egui::Ui, frame_state: &FrameState) {
        let th = self.theme();
        /* ★R2.14: 帧首一次性收集 (震动运行态 + dfo_player + 预设来源路线) */
        let (vib_on, dfo_player, vib_legacy) = self.backend.with_config(|c| {
            (
                c.vibration.enabled,
                c.dfo_player,
                c.vib_legacy_client,
            )
        });

        ui.with_layout(egui::Layout::top_down(egui::Align::Center), |ui| {
            ui.add_space(theme::SP_L);

            // 品牌
            let (rect, _) = ui.allocate_exact_size(egui::vec2(32.0, 32.0), egui::Sense::hover());
            crate::gui::widgets::Icon::Gamepad.paint(ui.painter(), rect, th.accent);
            ui.add_space(theme::SP_XS);
            ui.label(th.h1("极简模式"));
            ui.add_space(theme::SP_XS);
            ui.label(th.hint_text("连发 / 震动 / 音频，一页启停"));
            ui.add_space(theme::SP_M);

            // 连发大开关
            let running = !frame_state.is_paused;
            let (ltext, lfg, lbg) = if running {
                ("连发 · 开启中", th.good, th.good_soft)
            } else {
                ("连发 · 已暂停", th.hint, th.faint)
            };
            let turbo_btn = egui::Button::new(
                egui::RichText::new(ltext).size(16.0).strong().color(lfg),
            )
            .fill(lbg)
            .corner_radius(egui::CornerRadius::same(12))
            .min_size(egui::vec2(260.0, 50.0));
            if ui.add_sized([280.0, 50.0], turbo_btn).clicked() {
                self.toggle_with_notify();
            }
            ui.add_space(theme::SP_S);

            // 预设快切 (极简模式中间)
            let presets_empty = self.backend.with_config(|c| c.presets.is_empty());
            if presets_empty {
                ui.label(th.hint_text("无预设"));
            } else {
                // egui 的 main_align 不作用于水平行的摆放 (只对齐按钮内文字),
                // 行级居中须自测内容宽度 → 定宽子块交给 top_down(Center) 父级居中。
                // 块高取 interact_size.y (= ComboBox 内部 horizontal 条带高度):
                // combo 在条带内顶部锚定, 块与条带同高则位置确定;
                // label 用 ui.put 精确对齐到 combo 垂直中心 (两者不再差 ~2.5px)。
                // RTL: 先画 combo (占右端), 再按其 rect 反推 label 位置。
                let label_w = ui
                    .painter()
                    .layout_no_wrap(
                        "连发与映射".to_owned(),
                        egui::FontId::proportional(12.0),
                        egui::Color32::WHITE,
                    )
                    .size()
                    .x;
                let combo_w = 130.0; // render_preset_switch 里 ComboBox::width(130)
                let gap = theme::SP_XS + ui.spacing().item_spacing.x;
                let row_w = label_w + gap + combo_w;
                let _preset_row = ui.allocate_ui_with_layout(
                    egui::vec2(row_w, ui.spacing().interact_size.y),
                    egui::Layout::right_to_left(egui::Align::Center),
                    |ui| {
                        let combo = self.render_preset_switch(ui);
                        let label_rect = egui::Rect::from_center_size(
                            egui::pos2(
                                combo.rect.left() - gap - label_w / 2.0,
                                combo.rect.center().y,
                            ),
                            egui::vec2(label_w, combo.rect.height()),
                        );
                        ui.put(label_rect, egui::Label::new(th.weak("连发与映射")));
                    },
                );
            }
            ui.add_space(theme::SP_S);

            // 震动大开关
            // 非 DFO 玩家: 整段震动 UI 隐藏 (连发功能不受影响)
            if dfo_player {
            let (vtext, vfg, vbg) = if vib_on {
                ("震动 · 开启", th.good, th.good_soft)
            } else {
                ("震动 · 关闭", th.hint, th.faint)
            };
            let vib_btn = egui::Button::new(
                egui::RichText::new(vtext).size(16.0).strong().color(vfg),
            )
            .fill(vbg)
            .corner_radius(egui::CornerRadius::same(12))
            .min_size(egui::vec2(260.0, 50.0));
            if ui.add_sized([280.0, 50.0], vib_btn).clicked() {
                /* ★R2.14: 总开关切换走 backend (config + 运行态镜像 + 落盘,
                 * 替代旧的"只翻原子"写法 —— 单一写路径, 重启不再回滚) */
                self.backend
                    .dispatch(crate::ui_api::UiCommand::SetVibrationEnabled(!vib_on));
            }
            ui.add_space(theme::SP_S);

            /* ── 震动预设: 通用预设 / 全职业预设 互斥切换 ──
             * 两者互斥: 应用任一侧预设会自动关闭另一侧 (应用层互斥) */
            let mode_job = self.minimal_vib_preset_job;
            let seg_id = egui::Id::new("minimal_vib_preset_mode");
            let seg_w = 130.0_f32;
            let seg_h = 26.0_f32;
            let (seg_rect, _) =
                ui.allocate_exact_size(egui::vec2(seg_w * 2.0 + 4.0, seg_h), egui::Sense::hover());

            let left_rect = egui::Rect::from_x_y_ranges(
                seg_rect.left()..=seg_rect.center().x,
                seg_rect.top()..=seg_rect.bottom(),
            );
            let right_rect = egui::Rect::from_x_y_ranges(
                seg_rect.center().x..=seg_rect.right(),
                seg_rect.top()..=seg_rect.bottom(),
            );
            let l_resp = ui
                .interact(left_rect, seg_id.with("l"), egui::Sense::click())
                .on_hover_cursor(egui::CursorIcon::PointingHand);
            let r_resp = ui
                .interact(right_rect, seg_id.with("r"), egui::Sense::click())
                .on_hover_cursor(egui::CursorIcon::PointingHand);
            /* 容器底 + 滑块 (150ms 动画) */
            ui.painter().rect_filled(seg_rect, 8, th.faint);
            let t = ui.ctx().animate_value_with_time(seg_id, if mode_job { 1.0 } else { 0.0 }, 0.15);
            let thumb_w = seg_w - 6.0;
            let thumb_x = seg_rect.left() + 3.0 + t * (seg_rect.width() - thumb_w - 6.0);
            let thumb = egui::Rect::from_min_size(
                egui::pos2(thumb_x, seg_rect.top() + 3.0),
                egui::vec2(thumb_w, seg_h - 6.0),
            );
            ui.painter().rect_filled(thumb, 6, th.accent);
            /* 两段文字: 选中侧 = 滑块上的白色; 未选侧 = text_weak (按主题, 浅色下白字不可见) */
            let unselected = |a: f32| -> egui::Color32 {
                let mut c = th.text_weak;
                c = egui::Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), a as u8);
                c
            };
            let selected_white = |a: f32| -> egui::Color32 {
                egui::Color32::from_rgba_unmultiplied(255, 255, 255, a as u8)
            };
            ui.painter().text(
                egui::pos2(left_rect.center().x, seg_rect.center().y),
                egui::Align2::CENTER_CENTER,
                "通用预设",
                egui::FontId::proportional(12.0),
                if mode_job { unselected(200.0) } else { selected_white(255.0) },
            );
            ui.painter().text(
                egui::pos2(right_rect.center().x, seg_rect.center().y),
                egui::Align2::CENTER_CENTER,
                "全职业预设",
                egui::FontId::proportional(12.0),
                if mode_job { selected_white(255.0) } else { unselected(200.0) },
            );
            if l_resp.clicked() {
                self.minimal_vib_preset_job = false;
                /* ★保险B: 切回通用预设段 = 自动停用全职业预设 (与快捷卡一致) */
                if self.vib_job_enabled {
                    self.disable_job_vibration_preset();
                }
                let _ = self.backend.update_config(
                    crate::ui_api::Commit::Save,
                    |c| c.minimal_vib_preset_job = false,
                );
            }
            if r_resp.clicked() {
                self.minimal_vib_preset_job = true;
                /* 切到全职业预设: 直接启用当前选中职业 (无需再点应用) */
                let jobs = crate::job_presets::available_jobs(vib_legacy);
                let base_sel = self.vib_job_base.min(jobs.len().saturating_sub(1));
                let class_sel = self.vib_job_class.min(jobs[base_sel].classes.len().saturating_sub(1));
                self.apply_job_vibration_preset(jobs[base_sel].base_job, &jobs[base_sel].classes[class_sel]);
                self.vib_job_loaded = Some((base_sel, class_sel));
                let _ = self.backend.update_config(
                    crate::ui_api::Commit::Save,
                    |c| c.minimal_vib_preset_job = true,
                );
            }
            ui.add_space(theme::SP_XS);

            /* 预设选择 (来源由上面的分段开关决定) */
            if !mode_job {
                self.ensure_act1_preset_listed();
                /* ★v19: 用真实下标, 修过滤下标错位
                 * ★R2.14: 自定义预设表/路线标记从 backend 读 */
                let (vpresets, legacy) = self.backend.with_config(|c| {
                    (c.vibration_presets.clone(), c.vib_legacy_client)
                });
                let entries = crate::config::visible_preset_entries(&vpresets, legacy);
                let sel_pos = entries
                    .iter()
                    .position(|(real, _)| *real == self.vib_preset_idx)
                    .unwrap_or(0);
                if let Some((real, _)) = entries.get(sel_pos) {
                    self.vib_preset_idx = *real;
                }
                let selected = entries.get(sel_pos).map(|(_, n)| n.clone()).unwrap_or_default();
                /* ComboBox 内嵌 horizontal 不被父级居中 → 定宽子块居中法 (同连发与映射行) */
                let combo_w = 200.0_f32;
                ui.allocate_ui_with_layout(
                    egui::vec2(combo_w, ui.spacing().interact_size.y),
                    egui::Layout::right_to_left(egui::Align::Center),
                    |ui| {
                        egui::ComboBox::from_id_salt("minimal_vib_general")
                            .selected_text(selected)
                            .width(200.0)
                            .show_ui(ui, |ui| {
                                for (pos, (real, n)) in entries.iter().enumerate() {
                                    if ui.selectable_label(pos == sel_pos, n).clicked() {
                                        self.vib_preset_idx = *real;
                                        self.apply_general_vibration_preset(n);
                                    }
                                }
                            });
                    },
                );
            } else {
                /* 全职业: 基础职业 → 转职 两级选择 (选中转职立即应用) */
                let jobs = crate::job_presets::available_jobs(vib_legacy);
                let base_sel = self.vib_job_base.min(jobs.len().saturating_sub(1));
                let classes = &jobs[base_sel].classes;
                let class_sel = self.vib_job_class.min(classes.len().saturating_sub(1));
                let _cur_active = self.vib_job_active.clone();
                let _job_enabled = self.vib_job_enabled;
                let combo_w = 150.0_f32;
                /* 块宽用上一帧实测值 (首帧兜底 316): combo 实际渲染宽度 ≠ width() 设定值,
                 * 手工估算总有几像素偏差, 实测值才能让内容严丝合缝填满居中的块 */
                let gap = theme::SP_XS + ui.spacing().item_spacing.x;
                let block_w = if self.minimal_job_block_w > 0.0 {
                    self.minimal_job_block_w
                } else {
                    combo_w * 2.0 + gap
                };
                let _combo_h = ui.spacing().interact_size.y;
                let slot_h = 26.0_f32; // 两 combo 统一槽高
                let class_nudge = -1.5_f32; // 转职槽累计上调 (用户微调: 原 -3.0, 回调 1.5)
                let _block = ui.allocate_ui_with_layout(
                    egui::vec2(block_w, slot_h),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        let inner = ui.max_rect();
                        /* 基础职业: 左槽 */
                        ui.allocate_new_ui(
                            egui::UiBuilder::new()
                                .max_rect(egui::Rect::from_min_size(
                                    egui::pos2(inner.left(), inner.top()),
                                    egui::vec2(combo_w, slot_h),
                                ))
                                .layout(egui::Layout::right_to_left(egui::Align::Center)),
                            |ui| {
                                egui::ComboBox::from_id_salt("minimal_vib_job_base")
                                    .selected_text(jobs[base_sel].base_job)
                                    .width(combo_w)
                                    .show_ui(ui, |ui| {
                                        for (i, j) in jobs.iter().enumerate() {
                                            if ui.selectable_label(i == base_sel, j.base_job).clicked() {
                                                self.vib_job_base = i;
                                                self.vib_job_class = 0;
                                            }
                                        }
                                    });
                            },
                        );
                        /* 转职: 右槽, 累计上调 (class_nudge) */
                        ui.allocate_new_ui(
                            egui::UiBuilder::new()
                                .max_rect(egui::Rect::from_min_size(
                                    egui::pos2(inner.left() + combo_w + gap, inner.top() + class_nudge),
                                    egui::vec2(combo_w, slot_h),
                                ))
                                .layout(egui::Layout::right_to_left(egui::Align::Center)),
                            |ui| {
                                egui::ComboBox::from_id_salt("minimal_vib_job_class")
                                    .selected_text(classes[class_sel].name)
                                    .width(combo_w)
                                    .show_ui(ui, |ui| {
                                        for (i, c) in classes.iter().enumerate() {
                                            if ui.selectable_label(i == class_sel, c.name).clicked() {
                                                self.vib_job_class = i;
                                                self.apply_job_vibration_preset(jobs[base_sel].base_job, c);
                                            }
                                        }
                                    });
                            },
                        );
                    },
                );

                /* 全职业预设启用状态由分段开关自动管理 (切到全职业即启用当前职业; 切回通用自动恢复) */
                ui.add_space(theme::SP_S);
            }
            }

            // 音频震动: 所有玩家可见（不跟随 dfo_player 隐藏）
            ui.add_space(theme::SP_S);
            self.render_minimal_audio_section(ui);

            // 状态行 (居中原理同预设行: 测宽 + 定宽子块)
            let audio_on = self.backend.with_config(|c| c.audio_haptics.enabled);
            let game_on = dfo_player && vib_on;
            let dot = if running || game_on || audio_on { th.good } else { th.hint };
            let stat_text = format!(
                "连发{} · 游戏震动{} · 音频震动{}",
                if running { "运行" } else { "暂停" },
                if game_on { "开" } else { "关" },
                if audio_on { "开" } else { "关" },
            );
            let dot_w = 4.5 * 2.4; // status_dot 内部 allocate 的宽度
            let stat_w = ui
                .painter()
                .layout_no_wrap(
                    stat_text.to_owned(),
                    egui::FontId::proportional(12.0),
                    egui::Color32::WHITE,
                )
                .size()
                .x;
            let gap = theme::SP_XS + ui.spacing().item_spacing.x;
            let _status_row = ui.allocate_ui_with_layout(
                egui::vec2(dot_w + gap + stat_w, 18.0),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    widgets::status_dot(ui, dot, running, 4.5);
                    ui.add_space(theme::SP_XS);
                    ui.label(th.weak(stat_text));
                },
            );
            ui.add_space(theme::SP_S);
            ui.label(th.hint_text("顶栏「⤺ 完整界面」返回"));
        });
    }

    /* ═══════════════════ 自绘窗口标题栏 (无边框窗口) ═══════════════════ */

}
