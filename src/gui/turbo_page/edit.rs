//! 行内编辑面板 + 编辑期按键捕获 (触发/目标, 含手柄原始与鼠标) —— 原 904-1874, C4 归位。

use crate::gui::SorahkGui;
use crate::gui::theme::{self, Theme, truncate_chars};
use crate::gui::utils;
use crate::gui::widgets;
use crate::gui::types::KeyCaptureMode;

use eframe::egui;
impl SorahkGui {
    /// 行内编辑面板: 触发/目标/间隔/时长/连发/备注 + 保存/取消/删除。
    pub(in crate::gui) fn render_mapping_edit_row(
        &mut self,
        ui: &mut egui::Ui,
        idx: usize,
        default_interval: u64,
        default_duration: u64,
    ) {
        let th = self.theme();
        let Some(mapping) = self.config.mappings.get_mut(idx) else {
            self.edit_mapping_idx = None;
            return;
        };

        // 局部可编辑副本 (变更即回写 config; 取消由快照还原)
        let trigger = mapping.trigger_key.clone();
        let targets: Vec<String> = mapping.target_keys.to_vec();
        let mut interval = mapping.interval.unwrap_or(default_interval) as f64;
        let mut duration = mapping.event_duration.unwrap_or(default_duration) as f64;
        let mut turbo = mapping.turbo_enabled;
        /* ★v21.5 简易奔跑 ↔ 重推奔跑互斥: 数据双真时重推奔跑优先 (引擎侧本就压制
         * 简易奔跑), 局部副本先按互斥取, 避免两项同时勾选后 UI 双双灰死 */
        let mut double_tap = mapping.double_tap_enabled && !mapping.run_enabled;
        /* ★v21.0 奔跑行局部副本 (二次敲击间隔与 简易奔跑共用 double_tap_gap_ms) */
        let mut run_enabled = mapping.run_enabled;
        let mut run_recheck = mapping.run_recheck;
        let mut run_threshold = mapping.run_threshold as f32;
        let mut run_gap = mapping.double_tap_gap_ms as f32;
        let mut move_speed = mapping.move_speed as f32;
        /* 滚动映射的移动速度上限更高 (与设置弹窗一致: 普通 100 / 滚动 1200) */
        let speed_hi = if targets.iter().any(|k| k.starts_with("SCROLL")) {
            1200.0
        } else {
            100.0
        };
        let mut note = mapping.note.clone();
        /* ★v21.5 互斥自愈: 历史数据双真时清掉简易奔跑 (重推奔跑优先), 防灰死锁 */
        if mapping.double_tap_enabled && mapping.run_enabled {
            self.config.mappings[idx].double_tap_enabled = false;
        }
        let mut remove_target: Option<usize> = None;
        let mut request_delete = false;
        /* ★v24.35 序列接管明示: 序列非空时目标键/连发/双击/奔跑/锁定 在引擎侧
         * 全部被压制, GUI 必须同步灰掉并提示, 消除「设置了却悄悄不生效」的双轨状态 */
        let seq_active = !self.config.mappings[idx].sequence_text.trim().is_empty();

        egui::Frame::NONE
            .fill(th.card_alt)
            .stroke(egui::Stroke::new(1.0, th.accent_soft))
            .corner_radius(egui::CornerRadius::same(10))
            .inner_margin(egui::Margin::same(14))
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(format!("编辑映射 #{}", idx + 1))
                            .size(13.5)
                            .strong()
                            .color(th.heading),
                    );
                });
                ui.add_space(theme::SP_S);

                // 触发键
                ui.horizontal(|ui| {
                    ui.label(th.weak("触发键"));
                    /* ★v20.9: 键帽按设备类型上色 (紫=手柄 / 橙=鼠标 / 键盘=中性) */
                    /* ★v24.3: 还没捕获时**不画键帽**, 显示「尚未捕获触发键」(旧默认 "A" 会误导) */
                    if trigger.trim().is_empty() {
                        ui.label(th.hint_text("尚未捕获触发键"));
                    } else {
                        widgets::keycap_typed(ui, &th, &trigger, utils::key_kind(&trigger));
                    }
                    let capturing = matches!(
                        self.key_capture_mode,
                        KeyCaptureMode::MappingTrigger(i) if i == idx
                    );
                    let btn_text = if capturing {
                        "正在捕获触发键… (按下并松开)"
                    } else {
                        "捕获触发键"
                    };
                    if ui.add(th.secondary_button(btn_text)).clicked() && !capturing {
                        self.start_mapping_capture(idx, true);
                    }
                });
                ui.add_space(theme::SP_S);

                // 目标键
                ui.horizontal_wrapped(|ui| {
                    ui.label(th.weak("目标键"));
                    for (i, t) in targets.iter().enumerate() {
                        /* ★v20.5: 长名截断 35 字符 (悬停看全名), 行不溢出
                         * ★v20.9: chip 按设备类型上色 (键盘键保持天蓝目标色) */
                        let short = truncate_chars(t, 35);
                        let (fg, bg) = match utils::key_kind(t) {
                            utils::KeyKind::Gamepad => (th.gamepad_fg, th.gamepad_bg),
                            utils::KeyKind::Mouse => (th.mouse_fg, th.mouse_bg),
                            utils::KeyKind::Keyboard => (th.target_fg, th.target_bg),
                        };
                        let chip = th.badge_clickable(ui, &format!("{}  ✕", short), fg, bg);
                        if chip.clicked() {
                            remove_target = Some(i);
                        }
                        chip.on_hover_text(format!("{}\n点击移除该目标键", t));
                    }
                    if targets.is_empty() {
                        ui.label(th.hint_text("尚未设置目标键"));
                    }
                    /* ★v24.35 序列接管明示: 目标键在序列模式下不生效 */
                    if seq_active && !targets.is_empty() {
                        ui.label(
                            egui::RichText::new("🔒 已由序列宏接管 —— 按下触发键时目标键不生效")
                                .size(12.0)
                                .color(th.warn),
                        )
                        .on_hover_text(
                            "序列宏非空时输出由序列完全接管。\n目标键保留只是数据未删, 引擎不会模拟它们。\n如需目标键生效: 清空序列 (序列编辑器「清空」按钮)。",
                        );
                    }
                    let capturing = matches!(
                        self.key_capture_mode,
                        KeyCaptureMode::MappingTarget(i) if i == idx
                    );
                    let btn_text = if capturing {
                        "正在捕获目标键… (按下并松开)"
                    } else {
                        "＋ 添加目标"
                    };
                    /* ★v24.35 序列模式下目标键不生效, 捕获按钮灰掉 (chips 仍可移除) */
                    ui.add_enabled_ui(!seq_active, |ui| {
                        if ui.add(th.secondary_button(btn_text)).clicked() && !capturing {
                            self.start_mapping_capture(idx, false);
                        }
                    });
                });
                ui.add_space(theme::SP_S);

                /* ★v24.31 抬起映射: 触发键抬起时额外发送的键 (可选) */
                ui.horizontal_wrapped(|ui| {
                    ui.label(th.weak("抬起时发送"));
                    let release_targets: Vec<String> =
                        self.config.mappings[idx].release_targets.to_vec();
                    for (_i, t) in release_targets.iter().enumerate() {
                        let short = truncate_chars(t, 35);
                        let (fg, bg) = match utils::key_kind(t) {
                            utils::KeyKind::Gamepad => (th.gamepad_fg, th.gamepad_bg),
                            utils::KeyKind::Mouse => (th.mouse_fg, th.mouse_bg),
                            utils::KeyKind::Keyboard => (th.target_fg, th.target_bg),
                        };
                        let chip = th.badge_clickable(ui, &format!("{}  ✕", short), fg, bg);
                        if chip.clicked() {
                            self.config.mappings[idx].remove_release_key(t);
                        }
                        chip.on_hover_text(format!("{}
点击移除", t));
                    }
                    if release_targets.is_empty() {
                        ui.label(th.hint_text(
                        concat!(
                            "可选 —— 松开触发键的那一瞬间额外发送这组键 (例: 收招/取消)\n",
                            "例: 触发键是 J, 抬起键设 SPACE → 松开 J 时自动按一下空格"
                        ),
                    ));
                    }
                    /* ★v24.35: 抬起映射在序列模式下同样生效 (按下跑连招, 抬起收招) */
                    if seq_active && !release_targets.is_empty() {
                        ui.label(
                            egui::RichText::new("✓ 序列模式下生效: 松开触发键时发送 (收招/取消)")
                                .size(12.0)
                                .color(th.good),
                        );
                    }
                    let capturing = matches!(
                        self.key_capture_mode,
                        KeyCaptureMode::MappingRelease(i) if i == idx
                    );
                    let btn_text = if capturing {
                        "正在捕获抬起键… (按下并松开)"
                    } else {
                        "＋ 添加抬起键"
                    };
                    if ui.add(th.secondary_button(btn_text)).clicked() && !capturing {
                        self.key_capture_mode = KeyCaptureMode::MappingRelease(idx);
                        self.capture_pressed_keys.clear();
                        self.capture_initial_pressed = Self::poll_all_pressed_keys();
                        self.just_captured_input = true;
                    }
                    if !release_targets.is_empty()
                        && ui
                            .add(th.secondary_button("清空抬起键"))
                            .on_hover_text("移除全部抬起映射目标 (恢复旧行为)")
                            .clicked()
                    {
                        self.config.mappings[idx].clear_release_keys();
                    }
                });
                ui.add_space(theme::SP_S);

                /* ★v24.32 序列宏 (可视化步骤编辑器, 连发页/手柄页共用同一组件) */
                self.render_sequence_steps_editor(ui, idx, &th);

                // 参数行
                ui.horizontal_wrapped(|ui| {
                    ui.label(th.weak("间隔"));
                    if ui
                        .add(egui::DragValue::new(&mut interval).range(1.0..=5000.0).speed(1.0))
                        .changed()
                    {
                        self.config.mappings[idx].interval = Some(interval.max(1.0) as u64);
                    }
                    ui.label(th.weak("ms"));
                    ui.add_space(theme::SP_L);
                    ui.label(th.weak("时长"));
                    if ui
                        .add(egui::DragValue::new(&mut duration).range(1.0..=5000.0).speed(1.0))
                        .changed()
                    {
                        self.config.mappings[idx].event_duration = Some(duration.max(1.0) as u64);
                    }
                    ui.label(th.weak("ms"));
                    ui.add_space(theme::SP_L);
                    /* ★v24.35 序列接管: 序列模式下连发不生效, 灰掉 */
                    ui.add_enabled_ui(!seq_active, |ui| {
                        if ui.checkbox(&mut turbo, "连发").changed() {
                            self.config.mappings[idx].turbo_enabled = turbo;
                        }
                    });
                    if seq_active {
                        ui.label(
                            egui::RichText::new("🔒 连发/锁定/简易奔跑/重推奔跑 已由序列宏接管")
                                .size(12.0)
                                .color(th.warn),
                        )
                        .on_hover_text(
                            "序列宏自己管理按键节奏, 连发/锁定/双击/奔跑在其生效期间全部停用。\n清空序列后恢复。",
                        );
                    }
                    ui.add_space(theme::SP_L);
                    /* ★v24.31 锁定 Lock: 按一下=按住不松, 再按一下=松开; 可与连发叠加。
                     * 与 简易奔跑/重推奔跑 互斥 (奔跑有自身的按住语义, 引擎侧同样压制)。 */
                    ui.add_enabled_ui(!run_enabled && !double_tap && !seq_active, |ui| {
                        let mut lock = self.config.mappings[idx].lock_enabled;
                        let resp = ui
                            .checkbox(&mut lock, "锁定")
                            .on_hover_text(
                                "按一下 = 持续按住不松 (挂机刷图/长按类技能), 再按一下 = 松开\n\
                                 可与连发叠加: 锁定期间连发持续循环\n\
                                 与 简易奔跑/重推奔跑 互斥 (勾选奔跑时此项不可用)",
                            );
                        if resp.changed() {
                            self.config.mappings[idx].lock_enabled = lock;
                        }
                    });
                    ui.add_space(theme::SP_L);
                    /* ★v20.3: 简易奔跑补齐到连发页 (原只在设置弹窗有)
                     * ★v21.5: 与重推奔跑互斥 —— 重推奔跑勾选时此项灰掉 (不允许勾选)
                     * ★v24.35: 序列接管时同样灰掉 */
                    ui.add_enabled_ui(!run_enabled && !seq_active, |ui| {
                        let resp = ui
                            .checkbox(&mut double_tap, "简易奔跑")
                            .on_hover_text(if run_enabled {
                                "已勾选「重推奔跑」, 两者互斥 —— 取消重推奔跑后可勾选"
                            } else {
                                "按一次自动补一次敲击 (DNF 简易双击跑, 键盘/手柄键均可); \
                                 与设置弹窗里的「简易奔跑」是同一开关"
                            });
                        if resp.changed() {
                            self.config.mappings[idx].double_tap_enabled = double_tap;
                        }
                    });
                });
                ui.add_space(theme::SP_S);

                /* ★v20.3: 鼠标方向/滚动/移动速度补齐到连发页 (原只在设置弹窗有) */
                ui.horizontal_wrapped(|ui| {
                    ui.label(th.weak("鼠标"));
                    if ui
                        .add(th.secondary_button("⌖ 方向"))
                        .on_hover_text("把鼠标方向 (如 MOUSE_UP_LEFT) 加入目标键")
                        .clicked()
                    {
                        self.open_mouse_direction_dialog(idx);
                    }
                    if ui
                        .add(th.secondary_button("🎡 滚动"))
                        .on_hover_text("把鼠标滚动 (SCROLL_UP/DOWN/MBUTTON) 加入目标键")
                        .clicked()
                    {
                        self.open_mouse_scroll_dialog(idx);
                    }
                    ui.add_space(theme::SP_L);
                    ui.label(th.weak("移动速度"));
                    if ui
                        .add(
                            egui::DragValue::new(&mut move_speed)
                                .range(1.0..=speed_hi)
                                .speed(1.0),
                        )
                        .on_hover_text(format!(
                            "方向/滚动映射的每步移动量 (px/步)\n当前映射上限 {}",
                            speed_hi as i32
                        ))
                        .changed()
                    {
                        self.config.mappings[idx].move_speed = move_speed.round().max(1.0) as i32;
                    }
                });
                ui.add_space(theme::SP_S);

                /* ★v21.0 重推奔跑: 轻推=走(方向键按住) / 推过重推阈值=自动补一次
                 * 松开再按下 (游戏判定双击→奔跑)。勾选后本条的 连发/简易奔跑 不生效。 */
                ui.horizontal_wrapped(|ui| {
                    ui.label(th.weak("重推奔跑"));
                    /* ★v21.5: 与简易奔跑互斥 —— 简易奔跑勾选时此项灰掉 (不允许勾选)
                     * ★v24.35: 序列接管时同样灰掉 */
                    ui.add_enabled_ui(!double_tap && !seq_active, |ui| {
                        let resp = ui
                            .checkbox(&mut run_enabled, "")
                            .on_hover_text(if double_tap {
                                "已勾选「简易奔跑」, 两者互斥 —— 取消简易奔跑后可勾选"
                            } else {
                                "重推奔跑 (仅摇杆方向映射有效):\n\
                                 轻推摇杆 = 方向键按住 (走路)\n\
                                 推过「重推阈值」= 自动补一次松开再按下 → 游戏判定双击 → 奔跑\n\
                                 勾选后本条映射的 连发/简易奔跑 不生效 (奔跑改写按键节奏)"
                            });
                        if resp.changed() {
                            self.config.mappings[idx].run_enabled = run_enabled;
                        }
                    });
                    if run_enabled {
                        ui.add_space(theme::SP_L);
                        ui.label(th.weak("重推阈值"));
                        if ui
                            .add(
                                egui::DragValue::new(&mut run_threshold)
                                    .range(50.0..=95.0)
                                    .suffix("%")
                                    .speed(1.0),
                            )
                            .on_hover_text(
                                "摇杆推过多大幅度算「重推」(满量的 %)\n\
                                 轻推就误触奔跑 → 调高; 推到底还不跑 → 调低",
                            )
                            .changed()
                        {
                            self.config.mappings[idx].run_threshold =
                                run_threshold.round().clamp(50.0, 95.0) as u8;
                        }
                        ui.add_space(theme::SP_L);
                        ui.label(th.weak("二次敲击间隔"));
                        if ui
                            .add(
                                egui::DragValue::new(&mut run_gap)
                                    .range(20.0..=200.0)
                                    .suffix("ms")
                                    .speed(1.0),
                            )
                            .on_hover_text(
                                "双重作用: ① 急推判定窗口 (在这么短时间内推过重推线才算「瞬间推入」)\n\
                                 ② 走→跑 切换时两次敲击之间的等待 (与 简易奔跑 共用)\n\
                                 缓推被误判成奔跑 → 调小; 游戏判定不出双击 → 调大",
                            )
                            .changed()
                        {
                            self.config.mappings[idx].double_tap_gap_ms =
                                run_gap.round().clamp(20.0, 200.0) as u64;
                        }
                        ui.add_space(theme::SP_L);
                        /* ★v21.1 重推阈值再检测: 完整双击序列 (DNF 实测定案) */
                        if ui
                            .checkbox(&mut run_recheck, "再检测")
                            .on_hover_text(
                                "重推阈值再检测 (推荐开启) —— 只认「瞬间推入」:\n\
                                 缓慢推进越过重推线 = 继续走路 (只是想走深一点, 不触发)\n\
                                 在二次敲击间隔内从轻推区猛冲过线 = 明确的奔跑意图 → 模拟完整双击\n\
                                 (松开 → 敲一下 → 再敲一下并保持) → 游戏判定双击 → 奔跑\n\
                                 判定窗口 = 「二次敲击间隔」; 关闭则退回「推过线就补一次松按」的旧行为",
                            )
                            .changed()
                        {
                            self.config.mappings[idx].run_recheck = run_recheck;
                        }
                    }
                });
                ui.add_space(theme::SP_S);

                // 备注
                ui.horizontal(|ui| {
                    ui.label(th.weak("备注"));
                    ui.add(
                        egui::TextEdit::singleline(&mut note)
                            .desired_width(ui.available_width() - 40.0)
                            .hint_text("给这条映射加个备注 (可选)"),
                    );
                });
                ui.add_space(theme::SP_M);

                // 保存/取消 + 删除 (★v21.4: 删除从标题行右端移到底部最右端, 防误触)
                ui.horizontal(|ui| {
                    /* ★v24.3: 未捕获触发键时不允许保存 (避免落一条永远不触发的空映射) */
                    let can_save = !trigger.trim().is_empty();
                    if ui
                        .add_enabled(can_save, th.primary_button("保存修改"))
                        .on_disabled_hover_text("请先点「捕获触发键」设一个触发键")
                        .clicked()
                    {
                        self.config.mappings[idx].note = note.clone();
                        let _ = self.config.save_to_file("Config.toml");
                        if let Err(e) = self.app_state.reload_config(self.config.clone()) {
                            eprintln!("Failed to reload config after mapping edit: {}", e);
                        }
                        self.edit_mapping_idx = None;
                        self.edit_mapping_snapshot = None;
                        self.edit_mapping_is_new = false;
                    }
                    ui.add_space(theme::SP_S);
                    if ui.add(th.secondary_button("取消")).clicked() {
                        if self.edit_mapping_is_new {
                            self.config.mappings.remove(idx);
                        } else if let Some(snap) = self.edit_mapping_snapshot.take() {
                            if let Some(m) = self.config.mappings.get_mut(idx) {
                                *m = snap;
                            }
                        }
                        self.edit_mapping_idx = None;
                        self.edit_mapping_is_new = false;
                    }
                    /* 删除推到操作行最右端: 与保存/取消拉开距离,
                     * 消除"点完编辑按钮后同位置再点即误删"的隐患 */
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add(th.danger_button("删除该映射"))
                            .on_hover_text("删除这条映射 (不可恢复)")
                            .clicked()
                        {
                            request_delete = true;
                        }
                    });
                });
            });

        // 移除目标键 (需在 Frame 闭包外执行, 避免借用冲突)
        if let Some(i) = remove_target {
            if i < self.config.mappings[idx].target_keys.len() {
                let key = self.config.mappings[idx].target_keys[i].clone();
                self.config.mappings[idx].remove_target_key(&key);
            }
        }
        // 回写文本字段
        if self.edit_mapping_idx == Some(idx) {
            if self.config.mappings[idx].trigger_key != trigger {
                self.config.mappings[idx].trigger_key = trigger;
            }
            if self.config.mappings[idx].note != note {
                self.config.mappings[idx].note = note;
            }
        }
        // 删除请求
        if request_delete && self.edit_mapping_idx == Some(idx) {
            self.config.mappings.remove(idx);
            self.edit_mapping_idx = None;
            self.edit_mapping_snapshot = None;
            self.edit_mapping_is_new = false;
            let _ = self.config.save_to_file("Config.toml");
            if let Err(e) = self.app_state.reload_config(self.config.clone()) {
                eprintln!("Failed to reload config after mapping delete: {}", e);
            }
        }
    }


    /// 开始内联编辑的按键捕获 (触发键含手柄原始输入, 目标键含鼠标)。
    pub(in crate::gui) fn start_mapping_capture(&mut self, idx: usize, is_trigger: bool) {
        self.key_capture_mode = if is_trigger {
            KeyCaptureMode::MappingTrigger(idx)
        } else {
            KeyCaptureMode::MappingTarget(idx)
        };
        self.capture_pressed_keys.clear();
        self.capture_initial_pressed = Self::poll_all_pressed_keys();
        if is_trigger {
            self.app_state.set_raw_input_capture_mode(true);
        }
        self.just_captured_input = true;
    }


    /// 内联编辑的捕获轮询 (设置弹窗关闭时由 update 调用)。
    pub(in crate::gui) fn handle_turbo_edit_capture(&mut self, ctx: &egui::Context) {
        let (idx, is_trigger, _is_release) = match self.key_capture_mode {
            KeyCaptureMode::MappingTrigger(i) => (i, true, false),
            KeyCaptureMode::MappingTarget(i) => (i, false, false),
            KeyCaptureMode::MappingRelease(i) => (i, false, true),
            /* ★v24.32 序列步骤捕获由 handle_sequence_step_capture 独立轮询 */
            KeyCaptureMode::SequenceStepKey(_, _) => return,
            _ => return,
        };
        if self.edit_mapping_idx != Some(idx) {
            return;
        }

        // Esc 取消捕获
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.key_capture_mode = KeyCaptureMode::None;
            self.capture_pressed_keys.clear();
            self.app_state.set_raw_input_capture_mode(false);
            return;
        }

        let mut captured: Option<String> = None;

        // 键盘 (触发与目标均可)
        let current_pressed = Self::poll_all_pressed_keys();
        current_pressed
            .iter()
            .filter(|&&vk| !self.capture_initial_pressed.contains(&vk))
            .for_each(|&vk| {
                self.capture_pressed_keys.insert(vk);
            });
        let any_released = self
            .capture_pressed_keys
            .iter()
            .any(|vk| !current_pressed.contains(vk));
        if any_released {
            captured = Self::format_captured_keys(&self.capture_pressed_keys);
        }

        // 鼠标 (仅目标键)
        if captured.is_none() && !is_trigger && !self.just_captured_input {
            ctx.input(|i| {
                captured = if i.pointer.button_clicked(egui::PointerButton::Primary) {
                    Some("LBUTTON".to_string())
                } else if i.pointer.button_clicked(egui::PointerButton::Secondary) {
                    Some("RBUTTON".to_string())
                } else if i.pointer.button_clicked(egui::PointerButton::Middle) {
                    Some("MBUTTON".to_string())
                } else if i.pointer.button_clicked(egui::PointerButton::Extra1) {
                    Some("XBUTTON1".to_string())
                } else if i.pointer.button_clicked(egui::PointerButton::Extra2) {
                    Some("XBUTTON2".to_string())
                } else {
                    None
                };
            });
        }

        // 手柄原始输入 (仅触发键)
        if captured.is_none() && is_trigger {
            if let Some(device) = self.app_state.try_recv_raw_input_capture() {
                captured = Some(device.to_string());
            }
        }

        if let Some(name) = captured {
            if is_trigger {
                /* ★v21.7d: 触发键组合去重+规范排序 (上+上+空格 → 上+空格; 下+上+空格 → 上+下+空格) */
                self.config.mappings[idx].trigger_key = crate::util::normalize_key_combo(&name);
            } else {
                self.config.mappings[idx].add_target_key(name);
            }
            self.key_capture_mode = KeyCaptureMode::None;
            self.capture_pressed_keys.clear();
            self.app_state.set_raw_input_capture_mode(false);
            self.just_captured_input = false;
            // 捕获即持久化 (与手柄页快速捕获一致)
            let _ = self.config.save_to_file("Config.toml");
            if let Err(e) = self.app_state.reload_config(self.config.clone()) {
                eprintln!("Failed to reload config after mapping capture: {}", e);
            }
        } else if self.just_captured_input {
            self.just_captured_input = false;
        }
    }

    /* ═══════════════════ 极简模式 (P8) ═══════════════════ */


    /// ★v24.32 序列宏可视化步骤编辑器 (连发页 / 手柄映射页共用)。
    /// 步骤列表 + 添加/录制/引用通用宏 + 高级文本, 全部无需记语法。
    pub(crate) fn render_sequence_steps_editor(
        &mut self,
        ui: &mut egui::Ui,
        idx: usize,
        th: &Theme,
    ) {
        let macros: std::collections::HashMap<String, String> = self
            .config
            .universal_macros
            .iter()
            .map(|m| (m.name.to_lowercase(), m.text.clone()))
            .collect();
        let seq_now = self.config.mappings[idx].sequence_text.clone();
        let parse_result = crate::sequence::parse_sequence(&seq_now, &macros);
        let parse_ok = parse_result.is_ok();
        let mut steps = parse_result.clone().unwrap_or_default();
        let default_hold = self.config.mappings[idx].event_duration.unwrap_or(20);
        let recording = self
            .app_state
            .key_record_active
            .load(std::sync::atomic::Ordering::Relaxed);
        let capturing_step = matches!(
            self.key_capture_mode,
            KeyCaptureMode::SequenceStepKey(i, _) if i == idx
        );

        ui.horizontal_wrapped(|ui| {
            ui.label(th.weak("序列宏"));

            /* ★v24.35 解析失败红条: 引擎侧不再回退目标键, 整条映射不生效 ——
             * 必须让用户当场看到, 否则「按了没反应」无从排查 */
            if !seq_now.trim().is_empty() {
                if let Err(e) = &parse_result {
                    ui.label(
                        egui::RichText::new(format!("✗ 序列解析失败: {e} —— 本条映射当前不生效!"))
                            .size(12.5)
                            .color(th.bad),
                    )
                    .on_hover_text(
                        "引擎不再回退到目标键模式 (防行为突变)。\n常见原因: 语法错 / 引用的通用宏被改名或删除。\n修正后自动恢复; 或点下方「清空序列」退回普通目标键模式。",
                    );
                }
            }
            /* ● 录制 / ■ 停止 (GetAsyncKeyState 轮询线程, 不依赖 LL 钩子) */
            if recording {
                ui.label(
                    egui::RichText::new("● 录制中… (按键盘按键, 完了点右边停止)")
                        .size(12.0)
                        .color(th.bad),
                );
                if ui.add(th.secondary_button("■ 停止并填入")).clicked() {
                    if let Some(events) = self.app_state.stop_key_record() {
                        let text = crate::sequence::recorded_to_sequence(
                            &events,
                            &crate::sequence::vk_to_seq_name,
                        );
                        if text.is_empty() {
                            let ime_hits = events.iter().filter(|e| e.vk == 0xE5).count();
                            let msg = if ime_hits > 0 {
                                "✗ 没录到按键: 按键被中文输入法接管了 —— 请切到英文输入法再重新录制".to_string()
                            } else {
                                "✗ 没有录到任何按键 —— 请按【键盘】键录制 (手柄按键无法录: 序列的输出就是键盘动作)".to_string()
                            };
                            self.seq_record_hint = Some((msg, std::time::Instant::now()));
                        } else {
                            let existing = self.config.mappings[idx].sequence_text.clone();
                            let combined = if existing.trim().is_empty() {
                                text
                            } else {
                                format!("{existing}»{text}")
                            };
                            self.config.mappings[idx].sequence_text = combined;
                            self.seq_record_hint = Some((
                                "✓ 已填入 (下方步骤列表)".to_string(),
                                std::time::Instant::now(),
                            ));
                        }
                    } else {
                        self.seq_record_hint = Some((
                            "✗ 录制状态丢失 (再录一次即可)".to_string(),
                            std::time::Instant::now(),
                        ));
                    }
                }
            } else if ui
                .add(th.secondary_button("● 录制按键"))
                .on_hover_text(
                    "录制【键盘】按键 → 自动生成步骤 (手柄按键无法录)\n录完再点一次停止",
                )
                .clicked()
            {
                self.app_state.start_key_record();
                let st = self.app_state.clone();
                let _ = std::thread::Builder::new()
                    .name("key-record-poller".into())
                    .spawn(move || crate::sequence::run_key_record_poller(st));
                self.seq_record_hint = None;
            }

            /* ＋ 按键步: 点击进入捕获, 有明确的"正在捕获中"反馈 */
            if capturing_step {
                ui.label(
                    egui::RichText::new("正在捕获中… (按一个键盘键, Esc 取消)")
                        .size(12.0)
                        .color(th.good),
                );
            } else if ui
                .add(th.secondary_button("＋ 按键步"))
                .on_hover_text("新增一个按键步骤: 点按钮 → 按一个键盘键 → 自动加入")
                .clicked()
            {
                self.key_capture_mode = KeyCaptureMode::SequenceStepKey(idx, steps.len());
                self.capture_pressed_keys.clear();
                self.capture_initial_pressed = Self::poll_all_pressed_keys();
                self.just_captured_input = true;
            }

            /* ＋ 等待步 */
            if parse_ok
                && ui
                    .add(th.secondary_button("＋ 等待步"))
                    .on_hover_text("新增一个等待 (毫秒), 用在两步之间")
                    .clicked()
            {
                steps.push(crate::sequence::SeqStep {
                    keys: Vec::new(),
                    hold_ms: Some(200),
                });
                self.config.mappings[idx].sequence_text =
                    crate::sequence::steps_to_text(&steps, default_hold);
            }

            /* ＋ 引用通用宏: 展开通用宏列表, 点一个就插入 */
            if !self.config.universal_macros.is_empty()
                && ui
                    .add(th.secondary_button("＋ 引用通用宏"))
                    .on_hover_text("把保存过的通用宏插入为一步 (如 宏(觉醒连招))")
                    .clicked()
            {
                self.seq_macro_picker_open = !self.seq_macro_picker_open;
            }
            if !seq_now.trim().is_empty()
                && ui
                    .add(th.secondary_button("✕ 清空"))
                    .on_hover_text("清空序列 (退回普通目标键模式)")
                    .clicked()
                {
                    self.config.mappings[idx].sequence_text = String::new();
                    self.seq_macro_picker_open = false;
                    self.seq_record_hint = None;
                }
        });

        /* 通用宏引用列表 (展开时) */
        if self.seq_macro_picker_open && !self.config.universal_macros.is_empty() {
            ui.horizontal_wrapped(|ui| {
                ui.label(th.weak("选一个通用宏:"));
                let mut picked: Option<usize> = None;
                for (i, m) in self.config.universal_macros.iter().enumerate() {
                    if ui
                        .add(th.secondary_button(&format!("宏({})", m.name)))
                        .on_hover_text(format!("插入: {}", truncate_chars(&m.text, 40)))
                        .clicked()
                    {
                        picked = Some(i);
                    }
                }
                if let Some(i) = picked {
                    let m = &self.config.universal_macros[i];
                    let existing = self.config.mappings[idx].sequence_text.clone();
                    self.config.mappings[idx].sequence_text =
                        if existing.trim().is_empty() {
                            format!("宏({})", m.name)
                        } else {
                            format!("{existing}»宏({})", m.name)
                        };
                    self.seq_macro_picker_open = false;
                }
            });
        }

        match parse_result {
            Err(e) => {
                ui.add_space(theme::SP_S);
                ui.label(egui::RichText::new(format!("✗ {e}")).size(12.0).color(th.bad));
                ui.label(th.hint_text("可在下方「高级」里修正, 或点「✕ 清空」重来"));
                let mut seq_edit = seq_now;
                ui.add(
                    egui::TextEdit::multiline(&mut seq_edit)
                        .desired_rows(2)
                        .desired_width(ui.available_width()),
                );
                ui.add_space(theme::SP_M);
            }
            Ok(steps_parsed) => {
                if !steps_parsed.is_empty() {
                    let mut remove_step: Option<usize> = None;
                    let mut remove_key: Option<(usize, usize)> = None;
                    let mut hold_changed = false;
                    ui.add_space(theme::SP_XS);
                    for (si, step) in steps_parsed.iter().enumerate() {
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new(format!("{}. ", si + 1))
                                    .size(12.0)
                                    .color(th.text_weak),
                            );
                            if step.keys.is_empty() {
                                ui.label(th.weak("等待"));
                            } else {
                                for (ki, k) in step.keys.iter().enumerate() {
                                    let (fg, bg) = match utils::key_kind(k) {
                                        utils::KeyKind::Gamepad => (th.gamepad_fg, th.gamepad_bg),
                                        utils::KeyKind::Mouse => (th.mouse_fg, th.mouse_bg),
                                        utils::KeyKind::Keyboard => (th.target_fg, th.target_bg),
                                    };
                                    let chip = th.badge_clickable(
                                        ui,
                                        &format!("{}  ✕", k),
                                        fg,
                                        bg,
                                    );
                                    if chip.clicked() {
                                        remove_key = Some((si, ki));
                                    }
                                    chip.on_hover_text(format!("{k}\n点击移除该键"));
                                }
                            }
                            ui.label(th.weak(if step.keys.is_empty() { "时长" } else { "按住" }));
                            let mut hold = step.hold_ms.unwrap_or(default_hold) as f32;
                            if ui
                                .add(
                                    egui::DragValue::new(&mut hold)
                                        .range(10.0..=60000.0)
                                        .suffix("ms")
                                        .speed(10.0),
                                )
                                .changed()
                            {
                                steps[si].hold_ms =
                                    Some(hold.round().clamp(10.0, 60000.0) as u64);
                                hold_changed = true;
                            }
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui
                                        .add_sized(
                                            [26.0, 22.0],
                                            egui::Button::new(
                                                egui::RichText::new("🗑")
                                                    .size(12.0)
                                                    .color(th.text_weak),
                                            )
                                            .fill(th.faint)
                                            .corner_radius(egui::CornerRadius::same(6)),
                                        )
                                        .on_hover_text("删除该步骤")
                                        .clicked()
                                    {
                                        remove_step = Some(si);
                                    }
                                },
                            );
                        });
                        ui.add_space(theme::SP_XS);
                    }
                    if let Some((si, ki)) = remove_key {
                        steps[si].keys.remove(ki);
                        if steps[si].keys.is_empty() {
                            steps.remove(si);
                        }
                    }
                    if let Some(si) = remove_step {
                        steps.remove(si);
                    }
                    if remove_step.is_some() || remove_key.is_some() || hold_changed {
                        self.config.mappings[idx].sequence_text =
                            crate::sequence::steps_to_text(&steps, default_hold);
                    }
                } else {
                    ui.vertical_centered(|ui| {
                        ui.add_space(theme::SP_XS);
                        ui.label(th.hint_text(
                            "空序列 —— 点「＋ 按键步」逐步添加, 或「● 录制按键」直接录",
                        ));
                        ui.add_space(theme::SP_XS);
                    });
                }
                /* 录制结果反馈 (4 秒后消失) */
                if let Some((msg, at)) = &self.seq_record_hint {
                    if at.elapsed().as_secs() < 4 {
                        let bad = msg.starts_with('✗');
                        ui.label(
                            egui::RichText::new(msg)
                                .size(12.0)
                                .color(if bad { th.bad } else { th.good }),
                        );
                    }
                }
                /* 高级: 直接编辑文本 (兼容粘贴 QKeyMapper 格式) */
                egui::CollapsingHeader::new(
                    egui::RichText::new("高级: 直接编辑文本").size(12.0),
                )
                .id_source(("seq_advanced_text", idx))
                .show(ui, |ui| {
                    let mut seq_edit = seq_now;
                    let resp = egui::TextEdit::multiline(&mut seq_edit)
                        .desired_rows(2)
                        .desired_width(ui.available_width())
                        .hint_text("A+B:50 > NONE:200 > C:50 (等价写法: ⏱ » @)")
                        .show(ui);
                    if resp.response.changed() {
                        self.config.mappings[idx].sequence_text = seq_edit.trim().to_string();
                    }
                });
            }
        }
    }

    /// ★v24.32 序列步骤捕获轮询 (任意页面; Esc 取消; 松开按键即填入)。
    /// 独立于 handle_turbo_edit_capture —— 手柄映射页没有 edit_mapping_idx。
    pub(crate) fn handle_sequence_step_capture(&mut self, ctx: &egui::Context) {
        let (idx, step) = match self.key_capture_mode {
            KeyCaptureMode::SequenceStepKey(i, s) => (i, s),
            _ => return,
        };

        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.key_capture_mode = KeyCaptureMode::None;
            self.capture_pressed_keys.clear();
            return;
        }

        let mut captured: Option<String> = None;
        let current_pressed = Self::poll_all_pressed_keys();
        current_pressed
            .iter()
            .filter(|&&vk| !self.capture_initial_pressed.contains(&vk))
            .for_each(|&vk| {
                self.capture_pressed_keys.insert(vk);
            });
        let any_released = self
            .capture_pressed_keys
            .iter()
            .any(|vk| !current_pressed.contains(vk));
        if any_released {
            captured = Self::format_captured_keys(&self.capture_pressed_keys);
        }

        if let Some(name) = captured {
            let default_hold = self.config.mappings[idx].event_duration.unwrap_or(20);
            let macros: std::collections::HashMap<String, String> = self
                .config
                .universal_macros
                .iter()
                .map(|m| (m.name.to_lowercase(), m.text.clone()))
                .collect();
            let mut steps = crate::sequence::parse_sequence(
                &self.config.mappings[idx].sequence_text,
                &macros,
            )
            .unwrap_or_default();
            let upper = name.to_uppercase();
            match steps.get_mut(step) {
                Some(st) => {
                    if !st.keys.contains(&upper) {
                        st.keys.push(upper);
                    }
                }
                None => steps.push(crate::sequence::SeqStep {
                    keys: vec![upper],
                    hold_ms: Some(default_hold),
                }),
            }
            self.config.mappings[idx].sequence_text =
                crate::sequence::steps_to_text(&steps, default_hold);
            self.key_capture_mode = KeyCaptureMode::None;
            self.capture_pressed_keys.clear();
            self.just_captured_input = false;
            /* 即时落盘 + 热重载 (手柄页/连发页都要立即生效) */
            let _ = self.config.save_to_file("Config.toml");
            if let Err(e) = self.app_state.reload_config(self.config.clone()) {
                eprintln!("Failed to reload config after sequence step capture: {}", e);
            }
        } else if self.just_captured_input {
            self.just_captured_input = false;
        }
    }

}
