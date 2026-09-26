//! 连发页主体: 入口分发 + 状态 hero + 映射列表 + 全局参数卡 + 行渲染助手 —— 原 turbo_page.rs, 2026-09-27 架构重构 C4 归位。

use crate::gui::SorahkGui;
use crate::gui::theme::{self, Theme, truncate_chars};
use crate::gui::utils;
use crate::gui::widgets;

use eframe::egui;
use crate::gui::main_window::FrameState;
impl SorahkGui {
    /// 连发页: 状态 hero + 映射列表 + 全局参数。
    pub(in crate::gui) fn render_turbo_page(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, frame_state: &FrameState) {
        self.render_turbo_hero(ui, frame_state);
        /* ★v24.4: 经典模式在「预设管理」上方补一条震动快捷条 (用户要求; 完整模式 hero 已含震动开关) */
        if self.classic_mode {
            self.render_classic_vib_quickbar(ui);
        }
        self.render_turbo_preset_manager(ui);
        /* ★v24.33 键盘快捷连发 (用户要求: 预设管理下方 / 连发映射上方) */
        self.render_keyboard_quick_card(ui, ctx);
        self.render_turbo_mappings(ui);
        self.render_universal_macros_card(ui);
        self.render_turbo_params(ui, ctx);
    }


    /// 连发映射页 · 预设管理卡: 切换 / 保存 / 重命名 / 删除 (两次确认防误删)。
    /// 状态 hero: 运行状态 + 震动开关 + 主操作按钮 + 统计。
    pub(in crate::gui) fn render_turbo_hero(&mut self, ui: &mut egui::Ui, frame_state: &FrameState) {
        // 提前取值, 闭包内 &mut self 与 translations 借用不冲突
        let status_paused = self.translations.paused_status().to_owned();
        let status_running = self.translations.running_status().to_owned();
        let exit_label = self.translations.exit_button().to_owned();
        let start_label = self.translations.start_button().to_owned();
        let pause_label = self.translations.pause_button().to_owned();
        let th = self.theme();
        th.card(ui, None, |ui| {
            ui.horizontal(|ui| {
                // 左: 状态
                ui.vertical(|ui| {
                    ui.add_space(2.0);
                    ui.horizontal(|ui| {
                        let (color, text, pulsing) = if frame_state.is_paused {
                            (th.warn, status_paused.as_str(), false)
                        } else {
                            (th.good, status_running.as_str(), true)
                        };
                        widgets::status_dot(ui, color, pulsing, 6.0);
                        ui.add_space(2.0);
                        ui.label(
                            egui::RichText::new(text)
                                .size(17.0)
                                .strong()
                                .color(if frame_state.is_paused { th.warn } else { th.good }),
                        );
                    });
                    ui.add_space(theme::SP_XS);
                    /* ★v24.10: 「仅用连发」不显示震动开关 (入口/开关一起收掉, 口径统一) */
                    if self.config.dfo_player {
                        ui.horizontal(|ui| {
                            let vib_on = self
                                .app_state
                                .vibration_enabled
                                .load(std::sync::atomic::Ordering::Relaxed);
                            let (vtext, vcolor, vbg) = if vib_on {
                                ("震动: 开", th.good, th.good_soft)
                            } else {
                                ("震动: 关", th.bad, th.bad_soft)
                            };
                            if ui.add(th.status_pill(vtext, vcolor, vbg)).clicked() {
                                let v = self
                                    .app_state
                                    .vibration_enabled
                                    .load(std::sync::atomic::Ordering::Relaxed);
                                self.app_state
                                    .vibration_enabled
                                    .store(!v, std::sync::atomic::Ordering::Relaxed);
                            }
                            ui.label(th.hint_text("由 DfoVibration.dll 战斗事件驱动 (进图后自动)"));
                        });
                    }
                });

                // 右: 主操作按钮
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let exit_btn = egui::Button::new(
                        egui::RichText::new(format!("\u{23f9} {}", exit_label))
                            .size(13.0)
                            .strong()
                            .color(egui::Color32::WHITE),
                    )
                    .fill(th.btn_danger)
                    .corner_radius(egui::CornerRadius::same(theme::RADIUS_CTRL))
                    .min_size(egui::vec2(96.0, 34.0));
                    if ui.add(exit_btn).clicked() {
                        // 只走 app_state.exit(): 经 should_exit → ViewportCommand::Close
                        // 优雅退出, 保证托盘/震动线程清理与 on_exit 执行
                        self.app_state.exit();
                    }
                    ui.add_space(theme::SP_S);
                    let (label, color) = if frame_state.is_paused {
                        (start_label.as_str(), th.good)
                    } else {
                        (pause_label.as_str(), th.warn)
                    };
                    let toggle_btn = egui::Button::new(
                        /* ★v5: 琥珀(暂停)/绿(恢复)都是浅色实底 —— 白字仅 ~2:1, 改近黑 on_emphasis */
                        egui::RichText::new(label).size(13.0).color(th.on_emphasis).strong(),
                    )
                    .fill(color)
                    .corner_radius(egui::CornerRadius::same(theme::RADIUS_CTRL))
                    .min_size(egui::vec2(96.0, 34.0));
                    if ui.add(toggle_btn).clicked() {
                        self.toggle_with_notify();
                    }
                });
            });

            // 统计条: 映射数 / 工作线程 / 默认间隔 / 默认时长 / 切换热键
            ui.add_space(theme::SP_M);
            ui.separator();
            ui.add_space(theme::SP_S);
            ui.horizontal(|ui| {
                widgets::stat(
                    ui,
                    &th,
                    &self.config.mappings.len().to_string(),
                    "映射条目",
                    th.accent_text,
                );
                ui.separator();
                if frame_state.worker_count > 0 {
                    widgets::stat(
                        ui,
                        &th,
                        &frame_state.worker_count.to_string(),
                        "工作线程",
                        th.text,
                    );
                    ui.separator();
                }
                widgets::stat(ui, &th, &format!("{} ms", self.config.interval), "默认间隔", th.text);
                ui.separator();
                widgets::stat(
                    ui,
                    &th,
                    &format!("{} ms", self.config.event_duration),
                    "默认时长",
                    th.text,
                );
                ui.separator();
                ui.vertical(|ui| {
                    ui.set_min_width(64.0);
                    widgets::keycap(ui, &th, &self.config.switch_key);
                    ui.label(th.hint_text("切换热键"));
                });
            });
        });
    }


    /// 映射列表: 行式布局 + 行内编辑/新增 (无需再进设置弹窗)。
    pub(in crate::gui) fn render_turbo_mappings(&mut self, ui: &mut egui::Ui) {
        let th = self.theme();
        let count = self.config.mappings.len();
        th.card(ui, None, |ui| {
            ui.horizontal(|ui| {
                ui.label(th.h2("连发映射"));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        egui::RichText::new(format!("{} 条", count))
                            .size(12.0)
                            .color(th.hint),
                    );
                    ui.add_space(theme::SP_S);
                    if ui.add(th.primary_button("＋ 新增映射")).clicked() {
                        self.add_new_mapping();
                    }
                });
            });
            ui.add_space(theme::SP_M);
            {
                if count == 0 {
                    widgets::empty_state(
                        ui,
                        &th,
                        widgets::Icon::Keyboard,
                        "暂无连发映射",
                        &[
                            "点击右上角「＋ 新增映射」创建第一条",
                            "配置 手柄/键盘/鼠标 触发 → 目标键",
                        ],
                    );
                    return;
                }
                let interval = self.config.interval;
                let duration = self.config.event_duration;
                let mut idx = 0usize;
                while idx < self.config.mappings.len() {
                    if self.edit_mapping_idx == Some(idx) {
                        /* ★v20.3: 新增映射置顶后, 把编辑面板滚到可视区顶部 */
                        if std::mem::take(&mut self.scroll_to_edit_row) {
                            ui.scroll_to_cursor(Some(egui::Align::TOP));
                        }
                        self.render_mapping_edit_row(ui, idx, interval, duration);
                        // 编辑行可能触发删除, 重新检查下标
                        if idx >= self.config.mappings.len() {
                            break;
                        }
                        idx += 1;
                        continue;
                    }
                    // 静态行: 拷贝显示数据避免借用冲突, 行尾挂「编辑」按钮
                    let (trigger, targets, note, turbo, m_lock, m_seq) = {
                        let m = &self.config.mappings[idx];
                        (
                            m.trigger_key.clone(),
                            m.target_keys.to_vec(),
                            m.note.clone(),
                            m.turbo_enabled,
                            m.lock_enabled,
                            !m.sequence_text.trim().is_empty(),
                        )
                    };
                    let m_interval = self.config.mappings[idx].interval.unwrap_or(interval);
                    let m_duration = self.config.mappings[idx].event_duration.unwrap_or(duration);
                    render_mapping_row(
                        ui,
                        &th,
                        &trigger,
                        &targets,
                        m_interval,
                        m_duration,
                        turbo,
                        m_lock,
                        m_seq,
                        &note,
                        &mut |ui| {
                            /* 方向/滚动/连发/简易奔跑 全部收在「编辑」面板内 (v20.4 用户定稿:
                             * 不往静态行塞按钮); 点编辑即可配齐 */
                            if ui.add(th.secondary_button("编辑")).clicked() {
                                self.begin_mapping_edit(idx);
                            }
                        },
                    );
                    idx += 1;
                }
            }
        });
    }

    /* ═══════════════════ 连发映射内联编辑 (P8) ═══════════════════ */


    /// 新增一条映射并进入编辑态 (取消时自动删除)。
    /// ★v20.3: 插到列表**最前** (用户定稿: 新增映射的编辑面板必须显示在最前面),
    /// 并在下一帧滚动到编辑面板顶部。
    pub(in crate::gui) fn add_new_mapping(&mut self) {
        self.config.mappings.insert(
            0,
            crate::config::KeyMapping {
                release_targets: Default::default(),
                sequence_text: String::new(),
                /* ★v24.3: 新建映射初始**无触发键** —— UI 显示「尚未捕获触发键」,
                 * 旧默认 "A" 会让人以为已经捕获了 A 键 (用户实测困惑点)。 */
                trigger_key: String::new(),
                target_keys: Default::default(),
                interval: None,
                event_duration: None,
                turbo_enabled: true,
                move_speed: 5,
                double_tap_enabled: false,
                double_tap_gap_ms: 50,
                run_enabled: false,
                run_threshold: 80,
                run_recheck: true,
                lock_enabled: false,
                note: String::new(),
            },
        );
        self.edit_mapping_is_new = true;
        self.edit_mapping_snapshot = None;
        self.edit_mapping_idx = Some(0);
        self.scroll_to_edit_row = true;
    }


    /// 进入已有映射的编辑态 (保存快照用于取消还原)。
    pub(in crate::gui) fn begin_mapping_edit(&mut self, idx: usize) {
        if let Some(m) = self.config.mappings.get(idx) {
            self.edit_mapping_snapshot = Some(m.clone());
            self.edit_mapping_is_new = false;
            self.edit_mapping_idx = Some(idx);
        }
    }


    /// ★v20.3: 在连发页内联编辑行打开「鼠标方向」选择 (设置弹窗同款对话框)。
    pub(in crate::gui) fn open_mouse_direction_dialog(&mut self, idx: usize) {
        self.mouse_direction_mapping_idx = Some(idx);
        self.mouse_direction_dialog = Some(
            crate::gui::mouse_direction_dialog::MouseDirectionDialog::new(),
        );
    }

    /// ★v20.3: 在连发页内联编辑行打开「鼠标滚动」选择 (设置弹窗同款对话框)。
    pub(in crate::gui) fn open_mouse_scroll_dialog(&mut self, idx: usize) {
        self.mouse_scroll_mapping_idx = Some(idx);
        self.mouse_scroll_dialog = Some(
            crate::gui::mouse_scroll_dialog::MouseScrollDialog::new(),
        );
    }

    /// ★v20.3: 按预设名切换连发预设 (热键/下拉共用; 切换即落盘 + 热重载 + 同步弹窗暂存)。
    pub(in crate::gui) fn switch_to_turbo_preset(&mut self, name: &str) {
        if self.config.current_preset == name {
            return;
        }
        if let Some(pr) = self.config.presets.iter().find(|p| p.name == name) {
            self.config.current_preset = pr.name.clone();
            /* 空预设不覆盖 (防止把当前映射清空) */
            if !pr.mappings.is_empty() {
                self.config.mappings = pr.mappings.clone();
            }
            self.page_preset_delete_arm = false;
            let _ = self.config.save_to_file("Config.toml");
            if let Err(e) = self.app_state.reload_config(self.config.clone()) {
                eprintln!("Failed to reload config after preset switch: {}", e);
            }
            if let Some(tc) = &mut self.temp_config {
                tc.current_preset = self.config.current_preset.clone();
                tc.mappings = self.config.mappings.clone();
                tc.presets = self.config.presets.clone();
            }
        }
    }


    /// 行内编辑面板: 触发/目标/间隔/时长/连发/备注 + 保存/取消/删除。
    /// 全局参数卡。全局配置卡 (★v20.5: 从只读改为**可编辑** —— 玩家在映射页直接改, 不用跑设置弹窗)。
    /// 数值改动即保存 + 热重载; 置顶切换实时作用于窗口。
    pub(in crate::gui) fn render_turbo_params(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let th = self.theme();
        /* 标签先克隆: 避免闭包内 &self.translations 与 &mut self.config 冲突 */
        let lbl_timeout = self.translations.input_timeout_display().to_owned();
        let lbl_interval = self.translations.default_interval_display().to_owned();
        let lbl_duration = self.translations.default_duration_display().to_owned();
        let lbl_tray = self.translations.show_tray_icon_display().to_owned();
        let lbl_notif = self.translations.show_notifications_display().to_owned();
        let lbl_top = self.translations.always_on_top_display().to_owned();
        let mut dirty = false;
        let mut top_changed: Option<bool> = None;
        th.card(ui, Some("全局配置"), |ui| {
            egui::Grid::new("turbo_params_grid")
                .num_columns(2)
                .spacing([theme::SP_L, theme::SP_S])
                .min_col_width(ui.available_width() * 0.42)
                .striped(false)
                .show(ui, |ui| {
                    let mut v = self.config.input_timeout as f64;
                    ui.label(th.weak(&lbl_timeout));
                    if ui
                        .add(
                            egui::DragValue::new(&mut v)
                                .range(1.0..=2000.0)
                                .speed(1.0)
                                .suffix(" ms"),
                        )
                        .on_hover_text("按键输入超时/去抖 (1-2000 ms)")
                        .changed()
                    {
                        self.config.input_timeout = (v.round().max(1.0) as u64).clamp(1, 2000);
                        dirty = true;
                    }
                    ui.end_row();

                    let mut v = self.config.interval as f64;
                    ui.label(th.weak(&lbl_interval));
                    if ui
                        .add(
                            egui::DragValue::new(&mut v)
                                .range(1.0..=5000.0)
                                .speed(1.0)
                                .suffix(" ms"),
                        )
                        .on_hover_text("连发按键的默认重复间隔 (单条映射可在编辑里覆盖)")
                        .changed()
                    {
                        self.config.interval = (v.round().max(1.0) as u64).max(1);
                        dirty = true;
                    }
                    ui.end_row();

                    let mut v = self.config.event_duration as f64;
                    ui.label(th.weak(&lbl_duration));
                    if ui
                        .add(
                            egui::DragValue::new(&mut v)
                                .range(1.0..=5000.0)
                                .speed(1.0)
                                .suffix(" ms"),
                        )
                        .on_hover_text("每次按键的默认按压时长 (单条映射可在编辑里覆盖)")
                        .changed()
                    {
                        self.config.event_duration = (v.round().max(1.0) as u64).max(1);
                        dirty = true;
                    }
                    ui.end_row();

                    /* ★v24.27 组合键每键间隔 (模拟手速) */
                    let mut v = self.config.combo_key_gap_ms as f64;
                    ui.label(th.weak("组合键每键间隔"));
                    if ui
                        .add(
                            egui::DragValue::new(&mut v)
                                .range(0.0..=1000.0)
                                .speed(1.0)
                                .suffix(" ms"),
                        )
                        .on_hover_text(
                            "组合键内相邻两键的间隔 (模拟人类手速): ↓→→Z 这类顺序指令靠它逐键发出。\n                             40ms 左右 = 接近人手速; 0 = 关闭 (整组同按, 旧行为)。修改立即生效",
                        )
                        .changed()
                    {
                        self.config.combo_key_gap_ms = (v.round().max(0.0) as u64).min(1000);
                        self.app_state
                            .combo_key_gap_ms
                            .store(self.config.combo_key_gap_ms, std::sync::atomic::Ordering::Relaxed);
                        dirty = true;
                    }
                    ui.end_row();

                    ui.label(th.weak(&lbl_tray));
                    let mut flag = self.config.show_tray_icon;
                    if ui
                        .add(egui::Checkbox::new(&mut flag, ""))
                        .on_hover_text("显示系统托盘图标")
                        .changed()
                    {
                        self.config.show_tray_icon = flag;
                        dirty = true;
                    }
                    ui.end_row();

                    ui.label(th.weak(&lbl_notif));
                    let mut flag = self.config.show_notifications;
                    if ui
                        .add(egui::Checkbox::new(&mut flag, ""))
                        .on_hover_text("显示系统通知")
                        .changed()
                    {
                        self.config.show_notifications = flag;
                        dirty = true;
                    }
                    ui.end_row();

                    ui.label(th.weak(&lbl_top));
                    let mut flag = self.config.always_on_top;
                    if ui
                        .add(egui::Checkbox::new(&mut flag, ""))
                        .on_hover_text("窗口置顶 (即时生效)")
                        .changed()
                    {
                        self.config.always_on_top = flag;
                        top_changed = Some(flag);
                        dirty = true;
                    }
                    ui.end_row();
                });
            ui.add_space(theme::SP_XS);
            ui.label(th.hint_text("改动即时生效并自动保存 (拖动数值 / 勾选开关)"));
        });
        if dirty {
            let _ = self.config.save_to_file("Config.toml");
            if let Err(e) = self.app_state.reload_config(self.config.clone()) {
                eprintln!("Failed to reload config after global params edit: {}", e);
            }
            if let Some(top) = top_changed {
                ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(if top {
                    egui::WindowLevel::AlwaysOnTop
                } else {
                    egui::WindowLevel::Normal
                }));
            }
        }
    }


    /// 参数行: 标签 | 值 (网格内)。
    #[allow(dead_code)]
    pub(in crate::gui) fn param_row(ui: &mut egui::Ui, th: &Theme, label: &str, value: &str) {
        ui.label(th.weak(label));
        ui.label(egui::RichText::new(value).size(13.0).strong().color(th.accent_text));
        ui.end_row();
    }


    /// 布尔参数行: 标签 | 状态徽章 (网格内)。
    #[allow(dead_code)]
    pub(in crate::gui) fn param_flag(ui: &mut egui::Ui, th: &Theme, label: &str, on: bool) {
        ui.label(th.weak(label));
        let (text, fg, bg) = if on {
            ("开启", th.good, th.good_soft)
        } else {
            ("关闭", th.hint, th.faint)
        };
        th.badge(ui, text, fg, bg);
        ui.end_row();
    }
}



/// 单条映射行: 触发键帽 → 目标键帽 | 间隔/时长 | Turbo | 备注 | 行尾动作。
#[allow(clippy::too_many_arguments)]
fn render_mapping_row(
    ui: &mut egui::Ui,
    th: &Theme,
    trigger: &str,
    targets: &[String],
    interval: u64,
    duration: u64,
    turbo: bool,
    lock: bool,
    seq: bool,
    note: &str,
    extra: &mut dyn FnMut(&mut egui::Ui),
) {
    ui.horizontal(|ui| {
        // 行底: hover 高亮由 egui 默认; 圆角行容器
        egui::Frame::NONE
            .fill(if ui.ui_contains_pointer() { th.faint } else { egui::Color32::TRANSPARENT })
            .corner_radius(egui::CornerRadius::same(theme::RADIUS_CTRL))
            .inner_margin(egui::Margin::symmetric(10, 7))
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.horizontal(|ui| {
                    // 触发键帽 (多键拆分; ★v20.5 长名截断 20 字符防溢出, 悬停看全名 —— 老宿主口径)
                    // ★v20.9: 按设备类型上色 (紫=手柄 / 橙=鼠标 / 键盘=中性)
                    /* ★v24.3: 未捕获触发键时不画空键帽 */
                    if trigger.trim().is_empty() {
                        ui.label(th.hint_text("尚未捕获触发键"));
                    } else {
                        for part in trigger.split('+') {
                            let short = truncate_chars(part, 20);
                            widgets::keycap_typed(ui, th, &short, utils::key_kind(part))
                                .on_hover_text(part.to_string());
                        }
                    }
                    // 指向箭头
                    ui.label(egui::RichText::new("→").size(12.0).color(th.hint));
                    // 目标键帽 (★v20.5 长名截断 35 字符, 悬停看全名)
                    if targets.is_empty() {
                        ui.label(th.hint_text("(未设置目标)"));
                    } else {
                        for t in targets {
                            let short = truncate_chars(t, 35);
                            widgets::keycap_typed(ui, th, &short, utils::key_kind(t))
                                .on_hover_text(t.clone());
                        }
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        extra(ui);
                        ui.add_space(theme::SP_M);
                        // 备注
                        if note.is_empty() {
                            ui.label(th.hint_text("-"));
                        } else {
                            ui.label(
                                egui::RichText::new(truncate_chars(note, 14))
                                    .size(11.5)
                                    .color(th.hint),
                            )
                            .on_hover_text(note);
                        }
                        ui.add_space(theme::SP_M);
                        /* ★v24.31 锁定/序列徽章 */
                        if lock {
                            th.badge(ui, "🔒 锁定", th.warn, th.warn_soft);
                            ui.add_space(theme::SP_M);
                        }
                        if seq {
                            th.badge(ui, "⚡ 宏", th.accent_text, th.accent_soft);
                            ui.add_space(theme::SP_M);
                        }
                        // Turbo 徽章
                        if turbo {
                            th.badge(ui, "TURBO", th.accent, th.accent_soft);
                        } else {
                            th.badge(ui, "单发", th.hint, th.faint);
                        }
                        ui.add_space(theme::SP_M);
                        ui.label(
                            egui::RichText::new(format!("{interval}ms / {duration}ms"))
                                .size(11.5)
                                .color(th.text_weak),
                        )
                        .on_hover_text(format!("间隔 {interval}ms / 时长 {duration}ms"));
                    });
                });
            });
    });
}
