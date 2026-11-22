//! 震动页 (通用型震动设定 / 全职业预设) —— ★v24.61 版式重排。
//!
//! 版面自上而下:
//! ①**状态头**: 连接状态 + 累计事件 + 最近评分 + 左右马达实时电平 + 总开关 + 测试;
//! ②**预设/职业工具条**: 选预设/选职业 + 全部操作按钮 (分组排列, 不再糊成一长串);
//! ③**参数双列**: 左 (力度总闸 / 评分特效) 右 (马达电平 / 评分衰减 / 伤害飘字);
//! ④**高级调校**: 折叠, 展开后按 6 个主题面板双列排布。
//!
//! 版式口径 (与音频震动页同一套语言):
//! * 一行一个参数 = 图标 + 名称 + 右对齐数值 / 满宽滑轨 / 一句人话; 不再"标签 + 内置
//!   数值滑轨 + 三行说明"堆成文字墙;
//! * L/R 马达权重从"两项各占一行"收成"一行两半", 左右对比一眼可见;
//! * 卡片之间不再插 `separator` 横线 (卡片本身就是分隔), 页面靠留白呼吸;
//! * 全部功能/按钮/参数一个不少 —— 只改排布与行样式, 写入路径与语义逐字未变。
//!
//! 数据流 (★R2.14 起): 帧首 gather config 快照 + 运行态快照, 渲染期间只读局部;
//! 写入一律走 backend 单一写路径 (vib_slot_update / vib_slider_update)。

use crate::backend::Backend;
use crate::config::VibrationConfig;
use crate::gui::SorahkGui;
use crate::gui::main_window::FrameState;
use crate::gui::theme::{self, Theme};
use crate::gui::widgets::{self, Row};
use crate::ui_api::VibSnap;

use eframe::egui;
use egui_phosphor::variants::regular as ic;

/// 参数行: 图标 + 名称, tip 自动取 `param_hint` 的"调高/调低"说明。
fn vib_row<'a>(icon: &'a str, label: &'a str) -> Row<'a> {
    let tip = SorahkGui::param_hint(label);
    Row::new(icon, label).tip(tip)
}

/// 60 槽参数行 (分数档 0-100 之类)。
fn slot_row(
    ui: &mut egui::Ui,
    th: &Theme,
    backend: &Backend,
    job_mode: bool,
    row: Row<'_>,
    slot: usize,
    value: u32,
    lo: f32,
    hi: f32,
) {
    let mut v = value as f32;
    if widgets::row_f32(ui, th, &row, &mut v, lo..=hi, 1.0) {
        SorahkGui::vib_slot_update(backend, job_mode, slot, v as u32);
    }
}

/// 非槽位 (config 字段) 参数行。
fn cfg_row(
    ui: &mut egui::Ui,
    th: &Theme,
    backend: &Backend,
    job_mode: bool,
    row: Row<'_>,
    value: u32,
    lo: f32,
    hi: f32,
    set: impl FnOnce(&mut VibrationConfig, u32),
) {
    let mut v = value as f32;
    if widgets::row_f32(ui, th, &row, &mut v, lo..=hi, 1.0) {
        let n = v as u32;
        SorahkGui::vib_slider_update(backend, job_mode, |c| set(c, n));
    }
}

/// 卡片标题行右侧的"当前状态"摘要: 让每张卡在收起/扫读时也能回答"现在是多少"。
fn card_note(ui: &mut egui::Ui, th: &Theme, text: &str) {
    ui.label(egui::RichText::new(text).size(11.5).color(th.hint));
}

/// 60 槽参数卡 + 该参数自己的 L/R 权重 (同一个卡里, 不拆成两块)。
fn slot_row_lr(
    ui: &mut egui::Ui,
    th: &Theme,
    backend: &Backend,
    job_mode: bool,
    row: Row<'_>,
    slot: usize,
    value: u32,
    lo: f32,
    hi: f32,
    lr: impl FnOnce(&mut egui::Ui),
) {
    let mut v = value as f32;
    widgets::row_card(ui, th, |ui| {
        if widgets::row_f32_inner(ui, th, &row, &mut v, lo..=hi, 1.0) {
            SorahkGui::vib_slot_update(backend, job_mode, slot, v as u32);
        }
        lr(ui);
    });
}

/// config 字段参数卡 + 该参数自己的 L/R 权重。
fn cfg_row_lr(
    ui: &mut egui::Ui,
    th: &Theme,
    backend: &Backend,
    job_mode: bool,
    row: Row<'_>,
    value: u32,
    lo: f32,
    hi: f32,
    set: impl FnOnce(&mut VibrationConfig, u32),
    lr: impl FnOnce(&mut egui::Ui),
) {
    let mut v = value as f32;
    widgets::row_card(ui, th, |ui| {
        if widgets::row_f32_inner(ui, th, &row, &mut v, lo..=hi, 1.0) {
            let n = v as u32;
            SorahkGui::vib_slider_update(backend, job_mode, |c| set(c, n));
        }
        lr(ui);
    });
}

/// 竖向细分隔线 (状态头里分列用)。
fn vsep(ui: &mut egui::Ui, th: &Theme) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(1.0, 22.0), egui::Sense::hover());
    ui.painter()
        .vline(rect.center().x, rect.y_range(), egui::Stroke::new(1.0, th.stroke));
}

/// 状态头里的一项统计: 小标签 + 数值。
fn head_stat(ui: &mut egui::Ui, th: &Theme, label: &str, value: &str, color: egui::Color32) {
    ui.vertical(|ui| {
        ui.label(egui::RichText::new(label).size(10.5).color(th.hint));
        ui.label(
            egui::RichText::new(value)
                .size(13.0)
                .strong()
                .family(Theme::font_bold())
                .color(color),
        );
    });
}

/// 实时电平条: 小标题 + 8px 轨道 + 百分比 (+可选原始读数)。
fn level_bar(
    ui: &mut egui::Ui,
    th: &Theme,
    label: &str,
    out: u32,
    color: egui::Color32,
    width: f32,
    raw: bool,
) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(label).size(11.5).color(color).strong());
        let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 10.0), egui::Sense::hover());
        ui.painter().rect_filled(rect, 5.0, th.faint.gamma_multiply(if th.dark { 1.15 } else { 1.0 }));
        let pct = (out as f32 / 65535.0).clamp(0.0, 1.0);
        if pct > 0.0 {
            let mut fill = rect;
            fill.set_width((rect.width() * pct).max(3.0));
            ui.painter().rect_filled(fill, 5.0, color);
        }
        ui.label(
            egui::RichText::new(format!("{:>3}%", (pct * 100.0).round() as i32))
                .size(11.0)
                .monospace()
                .color(th.text_weak),
        );
        if raw {
            ui.label(
                egui::RichText::new(format!("{out:>5}"))
                    .size(10.5)
                    .monospace()
                    .color(th.hint),
            );
        }
    });
}

impl SorahkGui {
    /// 振动设置整页 (job_mode=false: 通用型震动设定 / true: 全职业预设)。
    pub(in crate::gui) fn render_vibration_full_page(
        &mut self,
        ui: &mut egui::Ui,
        _frame_state: &FrameState,
        job_mode: bool,
    ) {
        let th = self.theme();

        /* ★R2.14 帧首 gather: config 子结构 clone + 运行态快照各取一次,
         * 之后整帧渲染只读局部、随时发 backend 命令。 */
        let vib = self.backend.with_config(|c| c.vibration.clone());
        let legacy = self.backend.with_config(|c| c.vib_legacy_client);
        let snap = self.backend.snapshot().vibration;

        /* 字号与控件密度 (整页统一, 行样式由 widgets::Row 负责) */
        ui.style_mut().text_styles.insert(
            egui::TextStyle::Body,
            egui::FontId::new(13.0, egui::FontFamily::Proportional),
        );
        ui.style_mut().text_styles.insert(
            egui::TextStyle::Button,
            egui::FontId::new(13.0, egui::FontFamily::Proportional),
        );
        ui.style_mut().spacing.interact_size.y = 24.0;
        ui.style_mut().spacing.slider_rail_height = 5.0;
        /* 滑轨已由 widgets::rail 自绘 (轨道/已选段/手柄三层色), 不再需要改全局
         * visuals —— 旧做法把 inactive.bg_fill 提亮会把检查框/下拉一起染灰,
         * 而且滑块手柄与轨道同色 = 看不出能拖。 */
        widgets::set_form_hints(ui, self.vib_show_hints);

        self.vib_first_render_init();

        /* 宽屏居中 + 内容限宽 (窄窗口铺满, 不引入横向滚动) */
        let avail_w = ui.available_width();
        let content_w = 1240.0_f32.min(avail_w);
        let side = ((avail_w - content_w) * 0.5).max(0.0);

        ui.horizontal_top(|ui| {
            if side > 0.0 {
                ui.add_space(side);
            }
            ui.vertical(|ui| {
                ui.set_max_width(content_w);

                self.vib_status_card(ui, job_mode, &vib, &snap);
                if job_mode {
                    self.vib_job_card(ui, legacy, &vib, &snap);
                } else {
                    self.vib_preset_card(ui, legacy, &vib, &snap);
                }
                self.vib_params_section(ui, job_mode, legacy, &vib, &snap);
                self.vib_advanced_section(ui, job_mode, legacy, &vib, &snap);

                ui.add_space(theme::SP_S);
                ui.label(
                    egui::RichText::new(if job_mode {
                        "所有滑块实时生效；修改自动保存到 JobVibration.toml"
                    } else {
                        "所有滑块拖动即生效，松手约 1 秒后自动保存（重启不丢）。连发功能在「连发映射」页。"
                    })
                    .size(11.0)
                    .color(th.hint),
                );
            });
        });

        /* 全职业预设页: 滑块修改后自动落盘 (快照检测) */
        if job_mode {
            if let Some((b, c)) = self.vib_job_active.clone() {
                let snap_end = self.backend.snapshot().vibration;
                let changed = self.vib_job_snapshot.len() != 60
                    || (0..60).any(|i| snap_end.params[i] != self.vib_job_snapshot[i]);
                if changed {
                    self.vib_job_snapshot = snap_end.params.clone();
                    self.save_job_vibration_state(&b, &c);
                }
            }
        }
    }

    /// 首帧一次性初始化 (原 36-130): 全新装默认预设 + 职业预设恢复 (路线判定 v24.15)。
    fn vib_first_render_init(&mut self) {
        {
            use std::sync::atomic::{AtomicBool, Ordering};
            static INIT_DONE: AtomicBool = AtomicBool::new(false);
            if !INIT_DONE.load(Ordering::Relaxed) {
                INIT_DONE.store(true, Ordering::Relaxed);
                self.ensure_act1_preset_listed();
                /* pristine 判定走 config (权威) */
                let pristine = self
                    .backend
                    .with_config(|c| c.vibration.master_gain == 0 && c.vibration.attack_gain == 0);
                if pristine {
                    if let Some(pr) = crate::config::default_vibration_presets().first() {
                        let _ = self.backend.apply_vibration_preset_by_name(&pr.name);
                    }
                }
                /* 全职业预设恢复 (v21): applied=true 时覆盖默认预设; 否则绝对频率强制关闭 */
                if let Some(jcfg) = crate::job_presets::load_job_vibration() {
                    /* ★v24.15: 职业预设按**当前路线**判定 —— ACT 名册的职业在 S4 路线不存在,
                     * 不能因为 JobVibration.toml 记着 applied=true 就继续生效。
                     * 只重算内存态, **不改写** JobVibration.toml: 切回原路线仍会恢复。 */
                    let legacy = self.backend.with_config(|c| c.vib_legacy_client);
                    let route_job = crate::job_presets::find_class_for_route(
                        &jcfg.base_job,
                        &jcfg.class_name,
                        legacy,
                    );
                    self.vib_job_enabled = jcfg.applied && route_job.is_some();
                    if !jcfg.applied {
                        self.backend
                            .update_vib_config_keep_params(|v| v.abs_freq_enabled = false);
                    }
                    if let Some((bi, ci, cls)) = route_job {
                        self.vib_job_base = bi;
                        self.vib_job_class = ci;
                        self.vib_job_snapshot = if jcfg.params.len() == 60 {
                            jcfg.params.clone()
                        } else {
                            cls.params.to_vec()
                        };
                        self.backend.restore_job_vibration_session(&jcfg, &cls);
                        self.vib_job_active = Some((jcfg.base_job.clone(), jcfg.class_name.clone()));
                        self.vib_job_loaded = Some((bi, ci));
                    }
                }
            }
        }
    }

    /// ① 状态头 (概览条): 连接 / 累计事件 / **当前预设或职业** / 最近评分 / 总开关。
    /// 目的: 进页面先回答"我现在在用什么、生没生效", 而不是让玩家自己去下拉框里找。
    fn vib_status_card(
        &mut self,
        ui: &mut egui::Ui,
        job_mode: bool,
        _vib: &VibrationConfig,
        snap: &VibSnap,
    ) {
        let th = self.theme();
        /* 当前预设 / 职业 (标题栏一眼可见的状态, 不用滚回工具条确认) */
        let (cur_name, cur_color) = if job_mode {
            match &self.vib_job_active {
                Some((b, c)) => (format!("{b} · {c}"), th.good),
                None => ("未应用职业预设".to_string(), th.hint),
            }
        } else {
            let name = self.backend.with_config(|cfg| {
                cfg.vibration_presets
                    .get(self.vib_preset_idx)
                    .map(|p| p.name.clone())
                    .unwrap_or_else(|| "默认".to_string())
            });
            (name, th.accent_text)
        };
        th.card(ui, None, |ui| {
            ui.horizontal(|ui| {
                /* 连接状态 */
                let (color, text) = if snap.connected {
                    (th.good, "已连接游戏")
                } else {
                    (th.info, "等待游戏")
                };
                widgets::status_dot(ui, color, snap.connected, 4.5);
                ui.add_space(2.0);
                ui.label(
                    egui::RichText::new(text)
                        .size(14.0)
                        .strong()
                        .family(Theme::font_bold())
                        .color(th.heading),
                );
                ui.add_space(theme::SP_M);
                head_stat(
                    ui,
                    &th,
                    "累计事件",
                    &snap.events_received.to_string(),
                    th.text,
                );

                ui.add_space(theme::SP_L);
                vsep(ui, &th);
                ui.add_space(theme::SP_L);
                head_stat(
                    ui,
                    &th,
                    if job_mode { "当前职业" } else { "当前预设" },
                    &cur_name,
                    cur_color,
                );

                ui.add_space(theme::SP_L);
                vsep(ui, &th);
                ui.add_space(theme::SP_L);

                /* 评分 */
                let rl = snap.rank_last;
                let (rtext, rcolor) = if rl >= 8 {
                    (format!("等级 {rl} · 特殊动作"), th.warn)
                } else if rl >= 2 {
                    (format!("等级 {rl}"), th.info)
                } else {
                    ("暂无".to_string(), th.hint)
                };
                head_stat(ui, &th, "最近评分", &rtext, rcolor);
                ui.add_space(theme::SP_M);
                head_stat(
                    ui,
                    &th,
                    "评分累计",
                    &snap.rank_events.to_string(),
                    th.text,
                );

                /* 右侧: 总开关 (测试按钮归位到预设/职业工具条 —— 应用完顺手试听) */
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let mut on = snap.enabled;
                    if widgets::switch(ui, &th, &mut on).changed() {
                        /* ★R2.14: 总开关走 UiCommand (config → 镜像 → 立即落盘) */
                        self.backend
                            .dispatch(crate::ui_api::UiCommand::SetVibrationEnabled(on));
                    }
                    ui.add_space(2.0);
                    ui.label(
                        egui::RichText::new(if on { "震动已开" } else { "震动已关" })
                            .size(12.0)
                            .strong()
                            .color(if on { th.good } else { th.bad }),
                    );
                });
            });

            ui.add_space(theme::SP_M);
            widgets::hairline(ui, &th);
            ui.add_space(theme::SP_M);
            /* 实时遥测: 与状态同处一行区, 不再单独占一张卡 */
            ui.horizontal(|ui| {
                /* 预留宽度 = 两条标签(≈42) + 百分比(≈26) + 原始读数(≈30) + 3 段间距(24) ×2 + 中缝 24
                 * ★v24.63: 旧值 230 算小了 —— 1080 窗口下这一行超出 40px, 把整页最小宽度顶大,
                 * 右侧数值全被 ScrollArea 裁掉 (用户实测截图)。宁可留白, 也不许溢出。 */
                let bar_w = ((ui.available_width() - 310.0) / 2.0).clamp(80.0, 420.0);
                level_bar(ui, &th, "左马达", snap.out_l, th.motor_l, bar_w, true);
                ui.add_space(theme::SP_2XL);
                level_bar(ui, &th, "右马达", snap.out_r, th.motor_r, bar_w, true);
            });
        });
    }

    /// ② 通用页工具条: 预设选择/应用/保存/另存/删除 + 导出/导入/还原 + 调试模式。
    fn vib_preset_card(
        &mut self,
        ui: &mut egui::Ui,
        legacy: bool,
        vib: &VibrationConfig,
        snap: &VibSnap,
    ) {
        let th = self.theme();
        th.card(ui, Some("震动预设"), |ui| {
            /* 预设选择行 */
            let entries = self
                .backend
                .with_config(|c| crate::config::visible_preset_entries(&c.vibration_presets, legacy));
            let sel_pos = entries
                .iter()
                .position(|(real, _)| *real == self.vib_preset_idx)
                .unwrap_or(0);
            /* ★v19: 选择始终落在可见条目上 (真实下标) —— 修过滤下标错位 */
            if let Some((real, _)) = entries.get(sel_pos) {
                self.vib_preset_idx = *real;
            }

            ui.horizontal_wrapped(|ui| {
                ui.label(egui::RichText::new("预设").size(12.0).color(th.text_weak));
                if !entries.is_empty() {
                    egui::ComboBox::from_id_salt("vib_preset_sel")
                        .selected_text(entries[sel_pos].1.clone())
                        .width(170.0)
                        .show_ui(ui, |ui| {
                            for (pos, (real, n)) in entries.iter().enumerate() {
                                if ui.selectable_label(pos == sel_pos, n).clicked() {
                                    self.vib_preset_idx = *real;
                                }
                            }
                        });
                }
                if ui.add(th.primary_button("应用")).clicked() {
                    /* ★R2.14: 预设应用 = backend.apply_vibration_preset_by_name
                     * (★v18 用户覆盖优先 / ★保险B 自动停用全职业预设 都在 backend 内) */
                    if let Some((_, name)) = entries.get(sel_pos) {
                        self.apply_general_vibration_preset(name);
                    }
                }
                /* 应用完立刻试听: 玩家的真实路径是"选 → 用 → 听", 三个动作放一条线上 */
                if ui
                    .add(th.secondary_button("试听 1 秒"))
                    .on_hover_text("按当前参数震一下, 不用进游戏就能判断手感")
                    .clicked()
                {
                    self.backend.dispatch(crate::ui_api::UiCommand::TestVibration);
                }
                ui.add_space(theme::SP_L);
                if ui
                    .add(th.secondary_button("保存当前"))
                    .on_hover_text("把当前滑块参数写入 [vibration] 并保存 Config.toml")
                    .clicked()
                {
                    let _ = self.backend.save_vib_runtime_to_config();
                }
                ui.add_space(theme::SP_M);
                ui.label(egui::RichText::new("另存为").size(12.0).color(th.text_weak));
                ui.add(
                    egui::TextEdit::singleline(&mut self.vib_preset_name)
                        .desired_width(96.0)
                        .hint_text("名称"),
                );
                if ui.add(th.secondary_button("保存")).clicked() {
                    let name = self.vib_preset_name.trim().to_string();
                    if !name.is_empty() {
                        let mut params = [0u32; 60];
                        params.copy_from_slice(&snap.params);
                        let mut item_lr = [0u32; 26];
                        item_lr.copy_from_slice(&snap.item_lr);
                        let mut rank_lr = [0u32; 30];
                        rank_lr.copy_from_slice(&snap.rank_lr);
                        let mut rank_type_gain = [0u32; 15];
                        rank_type_gain.copy_from_slice(&snap.rank_type_gain);
                        let preset = crate::config::VibrationPreset {
                            name: name.clone(),
                            params,
                            item_lr,
                            rank_lr,
                            rank_type_gain,
                            rank_level_gain: snap.rank_level_gain,
                            rank_duration: snap.rank_duration,
                            out_smooth: vib.out_smooth,
                            user_modified: true,
                        };
                        // 同名替换而非追加: 追加重名预设后, 应用按名匹配会
                        // 顶替用户自存版本, 且重名项永远删不掉
                        let protected = name == "默认" || name == "测试版(全0)";
                        let mut new_idx: Option<usize> = None;
                        let _ = self.backend.update_config(crate::ui_api::Commit::SaveVib, |c| {
                            match c.vibration_presets.iter().position(|x| x.name == name) {
                                Some(pos) => {
                                    if !protected {
                                        c.vibration_presets[pos] = preset;
                                        new_idx = Some(pos);
                                    }
                                }
                                None => {
                                    c.vibration_presets.push(preset);
                                    new_idx = Some(c.vibration_presets.len() - 1);
                                }
                            }
                        });
                        if let Some(i) = new_idx {
                            self.vib_preset_name.clear();
                            self.vib_preset_idx = i;
                        }
                    }
                }
            });

            ui.add_space(theme::SP_S);
            widgets::hairline(ui, &th);
            ui.add_space(theme::SP_S);

            /* 导入 / 导出 / 还原 */
            let files = crate::job_presets::list_export_files_for_route("vibration_export", legacy);
            let sel = self.vib_import_sel.min(files.len().saturating_sub(1));
            ui.horizontal_wrapped(|ui| {
                if ui
                    .add(th.secondary_button("导出当前设置"))
                    .on_hover_text(
                        "导出当前页面生效的通用震动设定 (params/评分/高级算法) 为 TOML; ACT 路线导出文件名带 -ACT",
                    )
                    .clicked()
                {
                    let content = Self::export_vibration_settings(&self.backend);
                    /* ★v24.14: ACT 路线导出带 -ACT (文件名 + 导入列表按路线过滤) */
                    let prefix = crate::job_presets::export_prefix("vibration_export", legacy);
                    let fname = Self::save_export_file(&content, &prefix);
                    self.vib_export_msg = Some(fname);
                }
                ui.add_space(theme::SP_M);
                ui.label(egui::RichText::new("导入文件").size(12.0).color(th.text_weak));
                if !files.is_empty() {
                    egui::ComboBox::from_id_salt("vib_import_sel")
                        .selected_text(files[sel].clone())
                        .width(220.0)
                        .show_ui(ui, |ui| {
                            for (i, f) in files.iter().enumerate() {
                                if ui.selectable_label(i == sel, f).clicked() {
                                    self.vib_import_sel = i;
                                }
                            }
                        });
                } else {
                    ui.label(egui::RichText::new("当前路线暂无导出文件").size(11.0).color(th.hint));
                }
                if ui
                    .add(th.secondary_button("导入设置"))
                    .on_hover_text(
                        "从当前路线的 vibration_export*.toml 导入通用震动设定 (往返一致规范)",
                    )
                    .clicked()
                {
                    let route_files =
                        crate::job_presets::list_export_files_for_route("vibration_export", legacy);
                    if let Some(f) = route_files.get(self.vib_import_sel) {
                        if let Some(iv) = crate::job_presets::parse_vibration_export(f) {
                            /* ★R2.14: 参数+权重+标量写进 config 并全量镜像 + 落盘 */
                            self.backend.import_vib_export(&iv);
                            self.vib_import_msg = Some(f.clone());
                        } else {
                            self.vib_import_msg =
                                Some(format!("{} 解析失败 (规范不符)", f));
                        }
                    }
                }
                if ui
                    .add(th.secondary_button("还原默认"))
                    .on_hover_text("把预设恢复为出厂内置 (误删后一键还原)")
                    .clicked()
                {
                    let _ = self.backend.update_config(crate::ui_api::Commit::SaveVib, |c| {
                        c.vibration_presets = crate::config::default_vibration_presets();
                    });
                    self.vib_preset_idx = 0;
                    /* 若当前为空或损坏 (60 槽第一格为 0), 同时把参数复位为默认预设 */
                    if snap.params[0] == 0 {
                        if let Some(pr) = crate::config::default_vibration_presets().first() {
                            let _ = self.backend.apply_vibration_preset_by_name(&pr.name);
                        }
                    }
                }
                /* 删除放在管理行最末: 与保存/导出拉开, 且最不容易误触 */
                if ui
                    .add(th.danger_ghost_button("删除"))
                    .on_hover_text("仅\"默认\"与\"测试版(全0)\"受保护, 其余可删除")
                    .clicked()
                {
                    if let Some((_, name)) = entries.get(sel_pos) {
                        let protected = name == "默认" || name == "测试版(全0)";
                        if !protected {
                            let del = name.clone();
                            let _ = self
                                .backend
                                .update_config(crate::ui_api::Commit::SaveVib, |c| {
                                    c.vibration_presets.retain(|x| x.name != del);
                                });
                            self.vib_preset_idx = 0;
                        }
                    }
                }
            });

            /* 提示消息 */
            if let Some(m) = &self.vib_export_msg {
                ui.add_space(2.0);
                ui.label(
                    egui::RichText::new(format!("已导出: {} (程序目录)", m))
                        .size(11.0)
                        .color(th.good),
                );
            }
            if let Some(m) = &self.vib_import_msg {
                ui.add_space(2.0);
                ui.label(
                    egui::RichText::new(m.clone())
                        .size(11.0)
                        .color(th.info),
                );
            }

            ui.add_space(theme::SP_M);
            widgets::hairline(ui, &th);
            ui.add_space(theme::SP_M);

            /* 观测开关: 调试模式 + 参数说明 */
            self.vib_view_toggles(ui, vib);

            ui.add_space(theme::SP_XS);
            ui.label(
                egui::RichText::new(if legacy {
                    "内置: 默认/ACT1 特供/低频攻击职业/高频攻击职业/高振幅/节奏律动/极简轻巧/实战竞技/测试版(全0)；ACT 专属「-ACT」变体紧随其后"
                } else {
                    "内置: 默认/低频攻击职业/高频攻击职业/高振幅/节奏律动/极简轻巧/实战竞技/测试版(全0)；仅「默认」与「测试版(全0)」不可删除"
                })
                .size(11.0)
                .color(th.hint),
            );
        });
    }

    /// ② 全职业页工具条: 应用开关 + 职业选择 + 应用/导出/导入 + 状态说明。
    fn vib_job_card(
        &mut self,
        ui: &mut egui::Ui,
        legacy: bool,
        vib: &VibrationConfig,
        _snap: &VibSnap,
    ) {
        let th = self.theme();
        let jobs = crate::job_presets::available_jobs(legacy);
        let base_sel = self.vib_job_base.min(jobs.len().saturating_sub(1));
        let classes = &jobs[base_sel].classes;
        let class_sel = self.vib_job_class.min(classes.len().saturating_sub(1));
        let cur_cls = &classes[class_sel];

        th.card(ui, Some("职业预设"), |ui| {
            /* 应用开关 + 当前状态徽章 */
            ui.horizontal(|ui| {
                let mut en = self.vib_job_enabled;
                if ui
                    .checkbox(&mut en, egui::RichText::new("应用全职业预设").size(12.5))
                    .on_hover_text(
                        "开: 职业参数覆盖震动设置; 关: 不应用, 恢复震动设置默认。\n\
                         切换职业时若开关为开, 会自动应用该职业。",
                    )
                    .changed()
                {
                    self.vib_job_enabled = en;
                    if en {
                        self.apply_job_vibration_preset(jobs[base_sel].base_job, cur_cls);
                        self.vib_job_loaded = Some((base_sel, class_sel));
                    } else {
                        self.disable_job_vibration_preset();
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let (text, fg, bg) = if self.vib_job_enabled {
                        ("职业预设已生效", th.good, th.good_soft)
                    } else {
                        ("未应用 · 用通用参数", th.hint, th.faint)
                    };
                    th.badge(ui, text, fg, bg);
                });
            });

            ui.add_space(theme::SP_S);
            widgets::hairline(ui, &th);
            ui.add_space(theme::SP_M);

            /* 选择 + 操作 (左右两半) */
            widgets::split_cols(ui, 2, theme::SP_2XL, |ui, col| {
                if col == 0 {
                    widgets::group_head(ui, &th, "选择职业");
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("基础职业").size(12.0).color(th.text_weak));
                        let base_names: Vec<String> =
                            jobs.iter().map(|j| j.base_job.to_string()).collect();
                        egui::ComboBox::from_id_salt("vib_job_base")
                            .selected_text(base_names[base_sel].clone())
                            .width(130.0)
                            .show_ui(ui, |ui| {
                                for (i, n) in base_names.iter().enumerate() {
                                    if ui.selectable_label(i == base_sel, n).clicked() {
                                        self.vib_job_base = i;
                                        self.vib_job_class = 0;
                                    }
                                }
                            });
                    });
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("转职").size(12.0).color(th.text_weak));
                        let class_names: Vec<String> =
                            classes.iter().map(|c| c.name.to_string()).collect();
                        egui::ComboBox::from_id_salt("vib_job_class")
                            .selected_text(class_names[class_sel].clone())
                            .width(130.0)
                            .show_ui(ui, |ui| {
                                for (i, n) in class_names.iter().enumerate() {
                                    if ui.selectable_label(i == class_sel, n).clicked() {
                                        self.vib_job_class = i;
                                    }
                                }
                            });
                    });
                } else {
                    widgets::group_head(ui, &th, "操作");
                    ui.horizontal_wrapped(|ui| {
                        if ui.add(th.primary_button("应用该职业")).clicked() {
                            self.apply_job_vibration_preset(jobs[base_sel].base_job, cur_cls);
                            self.vib_job_loaded = Some((base_sel, class_sel));
                        }
                        if ui
                            .add(th.secondary_button("试听 1 秒"))
                            .on_hover_text("按当前职业参数震一下")
                            .clicked()
                        {
                            self.backend.dispatch(crate::ui_api::UiCommand::TestVibration);
                        }
                        ui.add_space(theme::SP_S);
                        if ui
                            .add(th.secondary_button("导出当前职业"))
                            .on_hover_text(
                                "导出当前选择职业的完整设置 (仅当前职业; ACT 路线职业名自带 -ACT)",
                            )
                            .clicked()
                        {
                            let content =
                                Self::export_job_settings(jobs[base_sel].base_job, cur_cls);
                            /* ★v24.14: 职业名自带 -ACT (ACT 名册), 导出名随之带标记 */
                            let fname = Self::save_export_file(
                                &content,
                                &format!("job_export_{}", cur_cls.name),
                            );
                            self.vib_export_msg = Some(fname);
                        }
                        if ui
                            .add(th.secondary_button("导入职业设置"))
                            .on_hover_text(
                                "从当前路线的 job_export_*.toml 导入 (按 base_job/class_name 匹配内置职业后应用)",
                            )
                            .clicked()
                        {
                            let files = crate::job_presets::list_export_files_for_route(
                                "job_export",
                                legacy,
                            );
                            let sel = self.job_import_sel.min(files.len().saturating_sub(1));
                            if let Some(f) = files.get(sel) {
                                match crate::job_presets::parse_job_export(f) {
                                    Some(ij) => {
                                        /* 规范: 按 base_job/class_name 匹配**当前路线**内置职业 */
                                        if let Some((bi, ci, _cls)) =
                                            crate::job_presets::find_class_for_route(
                                                &ij.base_job,
                                                &ij.class_name,
                                                legacy,
                                            )
                                        {
                                            self.vib_job_base = bi;
                                            self.vib_job_class = ci;
                                            self.vib_job_enabled = true;
                                            self.vib_import_msg = Some(format!(
                                                "已导入 {} - {}",
                                                ij.base_job, ij.class_name
                                            ));
                                        } else {
                                            self.vib_import_msg = Some(format!(
                                                "规范不符: 当前路线没有这个职业 {} - {}",
                                                ij.base_job, ij.class_name
                                            ));
                                        }
                                    }
                                    None => {
                                        self.vib_import_msg =
                                            Some(format!("{} 解析失败 (规范不符)", f));
                                    }
                                }
                            }
                        }
                    });
                }
            });

            /* 开关打开时, 切换职业自动应用 */
            if self.vib_job_enabled && self.vib_job_loaded != Some((base_sel, class_sel)) {
                self.apply_job_vibration_preset(jobs[base_sel].base_job, cur_cls);
                self.vib_job_loaded = Some((base_sel, class_sel));
            }

            ui.add_space(theme::SP_S);
            /* 状态行 */
            if let Some((b, c)) = &self.vib_job_active {
                ui.label(
                    egui::RichText::new(format!("当前职业: {b} - {c}"))
                        .size(12.0)
                        .color(th.good)
                        .strong(),
                );
                ui.label(
                    egui::RichText::new("职业参数已覆盖「通用型震动设定」；下方参数改动实时生效并自动保存")
                        .size(11.0)
                        .color(th.hint),
                );
            } else {
                ui.label(
                    egui::RichText::new("开关已关闭: 不应用职业预设, 使用通用型震动设定参数")
                        .size(11.5)
                        .color(th.hint),
                );
            }
            if let Some(m) = &self.vib_import_msg {
                ui.label(egui::RichText::new(m.clone()).size(11.0).color(th.info));
            }
            if let Some(m) = &self.vib_export_msg {
                ui.label(
                    egui::RichText::new(format!("已导出: {} (程序目录)", m))
                        .size(11.0)
                        .color(th.good),
                );
            }
            ui.add_space(theme::SP_XS);
            ui.label(
                egui::RichText::new(format!(
                    "{} (攻速→力度设计: 极快轻盈防叠加震手, 慢速沉稳给强度)",
                    cur_cls.desc
                ))
                .size(11.0)
                .color(th.hint),
            );

            ui.add_space(theme::SP_M);
            widgets::hairline(ui, &th);
            ui.add_space(theme::SP_M);
            self.vib_view_toggles(ui, vib);
        });
    }

    /// 观测开关行 (两页共用): 调试模式 (单通道验证)。
    fn vib_view_toggles(&mut self, ui: &mut egui::Ui, vib: &VibrationConfig) {
        let th = self.theme();
        let mut ind = vib.independent_test;
        ui.horizontal(|ui| {
            let (text, fg, bg) = if ind {
                ("调试中", th.warn, th.warn_soft)
            } else {
                ("正常模式", th.good, th.good_soft)
            };
            if ui
                .checkbox(
                    &mut ind,
                    egui::RichText::new("调试模式 (评分/移动不吃全局总调整)").size(12.0),
                )
                .on_hover_text(
                    "开启后: 全局总调整不会影响评分震动与移动持续震动 (单通道调试方便)。\n\
                     关闭后: 全局总调整统一应用到所有通道 (正常使用)。\n\
                     不影响各卡片自己的测试按钮与滑块。",
                )
                .changed()
            {
                self.backend.update_vibration_config(|v| {
                    v.independent_test = ind;
                });
            }
            th.badge(ui, text, fg, bg);
        });
        ui.add_space(theme::SP_XS);
        ui.label(
            egui::RichText::new(if ind {
                "调试模式已开启: 评分/移动通道不吃全局总调整, 便于逐项单独验证。"
            } else {
                "正常模式: 全局总调整统一作用于所有通道。需要单通道调试时再打开。"
            })
            .size(11.0)
            .color(th.hint),
        );
    }

    /// ③ 参数区: 标题行 (只有"参数说明"开关) + 全宽卡片纵列。
    ///
    /// ★v24.62 版式: 取消顶部双列 —— 卡片宽度随内容职责定:
    /// 力度总闸/评分特效/伤害飘字/评分动态衰减 都是全宽卡, 卡内自己分两列;
    /// 好处是每张卡的宽度不再被"旁边那张卡有多高"绑架, 也就不再出现列尾空洞。
    /// L/R(左右马达权重)是这套工具的核心调校,**常显不折叠**。
    fn vib_params_section(
        &mut self,
        ui: &mut egui::Ui,
        job_mode: bool,
        legacy: bool,
        vib: &VibrationConfig,
        snap: &VibSnap,
    ) {
        let th = self.theme();
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("参数调节")
                    .size(12.0)
                    .strong()
                    .family(Theme::font_bold())
                    .color(th.text_weak),
            );
            /* 视图选项放在参数区标题行: 它管的就是下面这一坨参数的呈现方式 */
            let mut hints = self.vib_show_hints;
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                /* 6px 内缩: 右对齐的控件别贴着内容右边界 (窄窗口下会显得像被裁掉) */
                ui.add_space(6.0);
                ui.checkbox(
                    &mut hints,
                    egui::RichText::new("参数说明").size(11.5).color(th.text_weak),
                )
                .on_hover_text("关掉即紧凑模式: 每行只留 名称 + 数值 + 滑轨; 说明仍可悬停查看。");
            });
            if hints != self.vib_show_hints {
                self.vib_show_hints = hints;
                widgets::set_form_hints(ui, hints);
            }
            let rest = ui.available_width();
            if rest > 8.0 {
                let (rect, _) = ui.allocate_exact_size(egui::vec2(rest, 1.0), egui::Sense::hover());
                ui.painter().hline(
                    rect.x_range(),
                    rect.center().y,
                    egui::Stroke::new(1.0, th.stroke),
                );
            }
        });
        ui.add_space(theme::SP_S);

        /* 力度总闸 (含召唤专属) → 评分特效 → 伤害飘字 → 评分动态衰减 → 高级 */
        if job_mode {
            self.vib_summon_abs_freq_section(ui, job_mode, vib);
        }
        self.vib_global_gain_section(ui, job_mode, vib, snap);
        self.vib_score_effect_section(ui, job_mode, snap);
        self.vib_damage_font_section(ui, job_mode, legacy, vib, snap);
        self.vib_score_decay_section(ui, job_mode, vib);
    }

    /// 召唤专属: 绝对震动频率 (仅职业页且已启用)。
    fn vib_summon_abs_freq_section(
        &mut self,
        ui: &mut egui::Ui,
        job_mode: bool,
        vib: &VibrationConfig,
    ) {
        if !vib.abs_freq_enabled {
            return;
        }
        let th = self.theme();
        th.card(ui, Some("召唤专属 · 绝对震动频率"), |ui| {
            ui.label(
                egui::RichText::new(
                    "开启后一切震动算法失效 (密度/间隔/节流/连击倍率/评分通道等)，只在 [时间-震动次数] 内均匀注入固定节拍；全局强度/强度上限仍可控制输出，移动独立。",
                )
                .size(11.0)
                .color(th.hint),
            );
            ui.add_space(theme::SP_S);
            let mut ae = vib.abs_freq_enabled;
            if widgets::check_row(
                ui,
                &th,
                &mut ae,
                "绝对震动频率 (3 秒最多 6 次)",
                "",
                "开：3 秒内最多震 6 次（防连成一片）；关：完全不限次数。",
            ) {
                Self::vib_slider_update(&self.backend, job_mode, |v| v.abs_freq_enabled = ae);
            }
            cfg_row(
                ui,
                &th,
                &self.backend,
                job_mode,
                vib_row(ic::TIMER, "绝对频率窗口 ms").hint("统计窗口，越长判定越宽容"),
                vib.abs_freq_window_ms,
                1000.0,
                10000.0,
                |v, n| v.abs_freq_window_ms = n,
            );
            cfg_row(
                ui,
                &th,
                &self.backend,
                job_mode,
                vib_row(ic::HASH, "窗口内最大震动次数").hint("允许更密 / 更克制"),
                vib.abs_freq_max,
                1.0,
                20.0,
                |v, n| v.abs_freq_max = n,
            );
        });
    }

    /// 力度总闸 (master / attack / max / 连击增强 / 随机性 / 移动)。
    fn vib_global_gain_section(
        &mut self,
        ui: &mut egui::Ui,
        job_mode: bool,
        vib: &VibrationConfig,
        snap: &VibSnap,
    ) {
        let th = self.theme();
        let item_lr: [u32; 26] = snap.item_lr.as_slice().try_into().unwrap();
        let item_out: [u32; 26] = snap.item_out.as_slice().try_into().unwrap();
        let rank_lr: [u32; 30] = snap.rank_lr.as_slice().try_into().unwrap();
        let rank_out: [u32; 30] = snap.rank_out.as_slice().try_into().unwrap();

        th.card_with_actions(
            ui,
            Some("力度总闸"),
            |ui| {
                card_note(
                    ui,
                    &th,
                    &format!("总强度 {}% · 攻击 {}", snap.params[9], snap.params[0]),
                );
            },
            |ui| {
            ui.label(
                egui::RichText::new("任一为 0 则全部关闭。想整体变轻先调这里，不要去动细节参数。")
                    .size(11.0)
                    .color(th.hint),
            );
            ui.add_space(theme::SP_S);

            /* 三闸 + 连击/随机性: 卡内左右两列 (全宽单列的话滑轨会拉到 1200px, 空转手感差) */
            let backend = &self.backend;
            let dark = self.dark_mode;
            widgets::split_cols(ui, 2, theme::SP_2XL, |ui, col| {
                if col == 0 {
                    slot_row(
                        ui,
                        &th,
                        backend,
                        job_mode,
                        vib_row(ic::FADERS, "全局总调整").suffix("%")
                            .hint("0 = 全部关闭；越低整体越轻"),
                        9,
                        snap.params[9],
                        0.0,
                        100.0,
                    );
                    slot_row(
                        ui,
                        &th,
                        backend,
                        job_mode,
                        vib_row(ic::SWORD, "攻击频率 (总闸1)")
                            .hint("命中反馈总闸：0 = 命中不震"),
                        0,
                        snap.params[0],
                        0.0,
                        100.0,
                    );
                } else {
                    slot_row(
                        ui,
                        &th,
                        backend,
                        job_mode,
                        vib_row(ic::ARROW_UP, "强度上限 (总闸2)")
                            .suffix("%")
                            .hint("给所有震动压一个天花板"),
                        4,
                        snap.params[4],
                        0.0,
                        100.0,
                    );
                    slot_row(
                        ui,
                        &th,
                        backend,
                        job_mode,
                        vib_row(ic::FIRE, "连击增强 (渐进至上限)")
                            .hint("连得越久越强，上限/斜率在高级调校"),
                        6,
                        snap.params[6],
                        0.0,
                        100.0,
                    );
                }
            });

            /* 随机性整宽一张卡: 塞在右列会让左右两列底部差出一整张卡的高度 */
            cfg_row(
                ui,
                &th,
                backend,
                job_mode,
                vib_row(ic::CIRCLE_NOTCH, "震动随机性")
                    .suffix("%")
                    .hint("每次强弱随机浮动，手感更自然"),
                vib.random_gain,
                0.0,
                100.0,
                |v, n| v.random_gain = n,
            );

            /* 左右马达总权重 (item0): 它是"总闸的左右分配", 与上面四个滑块同级,
             * 单列一行比塞进某一格更清楚, 也不会把格子挤得一高一低 */
            widgets::row_card(ui, &th, |ui| {
                ui.label(
                    egui::RichText::new("左右马达总权重（L / R，0 = 不动原参数）")
                        .size(11.5)
                        .color(th.text_weak),
                );
                ui.add_space(4.0);
                Self::render_item_lr(&self.backend, ui, self.dark_mode, &item_lr, &item_out, 0);
            });

            widgets::group_head(ui, &th, "移动 (持续震动)");
            let mut mi = vib.move_independent;
            if widgets::check_row(
                ui,
                &th,
                &mut mi,
                "移动震动独立于全局强度",
                "开：走路不吃全局总调整；关：跟着一起变小",
                "开: 移动震动不受全局总调整/强度上限影响 (移动一直在走, 吃全局强度容易直接没有震动)。\n关: 移动震动也受全局总调整与强度上限约束。",
            ) {
                Self::vib_slider_update(&self.backend, job_mode, |v| v.move_independent = mi);
            }
            let cnt = snap.rank_type_events[11];
            let backend = &self.backend;
            let dark = self.dark_mode;
            cfg_row_lr(
                ui,
                &th,
                backend,
                job_mode,
                vib_row(ic::FOOTPRINTS, "移动持续震动强度")
                    .tag(&format!("触发{cnt}"))
                    .hint("0 = 走路不震；配合高级调校的走路质感"),
                snap.rank_type_gain[11],
                0.0,
                100.0,
                |v, n| v.rank_type_gain[11] = n,
                |ui| Self::render_rank_lr(backend, ui, dark, &rank_lr, &rank_out, 11),
            );
        });
    }

    /// 伤害飘字震动 (事件类型细分)。
    fn vib_damage_font_section(
        &mut self,
        ui: &mut egui::Ui,
        job_mode: bool,
        legacy: bool,
        vib: &VibrationConfig,
        snap: &VibSnap,
    ) {
        let th = self.theme();
        let item_lr: [u32; 26] = snap.item_lr.as_slice().try_into().unwrap();
        let item_out: [u32; 26] = snap.item_out.as_slice().try_into().unwrap();

        th.card_with_actions(
            ui,
            Some("伤害飘字震动"),
            |ui| {
                card_note(ui, &th, if snap.font_hits { "已启用 · 6 类" } else { "已关闭" });
            },
            |ui| {
            ui.label(
                egui::RichText::new("DLL 采集游戏战斗飘字（命中/受击/技能/DOT 等），每类强度独立可调。")
                    .size(11.0)
                    .color(th.hint),
            );
            ui.add_space(theme::SP_S);
            let mut font_on = snap.font_hits;
            if widgets::check_row(
                ui,
                &th,
                &mut font_on,
                "启用伤害飘字震动",
                "关掉后命中/受击/技能都不震；评分与移动不受影响",
                "关闭后命中/受击/技能等伤害飘字不再震动; 评分特效与移动走路不受此开关影响",
            ) {
                self.backend.set_vib_font_hits(font_on);
            }

            let backend = &self.backend;
            let dark = self.dark_mode;
            widgets::split_cols(ui, 2, theme::SP_2XL, |ui, col| {
                if col == 0 {
                    slot_row_lr(
                        ui,
                        &th,
                        backend,
                        job_mode,
                        vib_row(ic::TEXT_T, "通用飘字强度")
                            .hint("所有飘字类型的兜底强度；0 = 各类型独立"),
                        10,
                        snap.params[10],
                        0.0,
                        100.0,
                        |ui| Self::render_item_lr(backend, ui, dark, &item_lr, &item_out, 6),
                    );
                } else {
                    slot_row(
                        ui,
                        &th,
                        backend,
                        job_mode,
                        vib_row(ic::TIMER, "飘字最小间隔 ms")
                            .hint("同类型事件最短触发间隔，防高频连打糊成一片"),
                        11,
                        snap.params[11],
                        10.0,
                        500.0,
                    );
                }
            });

            widgets::group_head(ui, &th, "飘字类型强度 (0 = 关闭)");
            let rows: [(&str, usize, usize, &str); 6] = [
                ("持续伤害 DOT 反馈 (0x20)", 12, 7, "中毒/灼烧/出血等掉血反馈"),
                ("装备特效反馈 (0x04)", 15, 8, if legacy {
                    "ACT 版本此通道已由怪物异常反馈承载"
                } else {
                    "装备触发效果（例如天域）"
                }),
                ("玩家状态变化反馈 (0x08)", 14, 9, "自身增益/减益/回复"),
                ("特殊攻击反馈 (0x10) 暴击/破招/背击", 13, 10, "暴击/破招/背击的重击强调"),
                ("玩家命中反馈 (0x01)", 16, 11, "普攻与技能每次命中的基础反馈"),
                ("玩家受击反馈 (0x02)", 17, 12, "被敌人击中的反馈"),
            ];
            let half = rows.len().div_ceil(2);
            widgets::split_cols(ui, 2, theme::SP_2XL, |ui, col| {
                let slice = if col == 0 {
                    &rows[..half]
                } else {
                    &rows[half..]
                };
                for (name, slot, item, hint) in slice.iter().copied() {
                    slot_row_lr(
                        ui,
                        &th,
                        backend,
                        job_mode,
                        vib_row(ic::DOT_OUTLINE, name).hint(hint),
                        slot,
                        snap.params[slot],
                        0.0,
                        100.0,
                        |ui| Self::render_item_lr(backend, ui, dark, &item_lr, &item_out, item),
                    );
                }
            });
            if legacy {
                cfg_row_lr(
                    ui,
                    &th,
                    &self.backend,
                    job_mode,
                    vib_row(ic::SKULL, "怪物异常反馈 (0x04) 出血/中毒/感电跳字")
                        .hint("仅 ACT 版本：怪物异常状态跳字"),
                    vib.monster_abnormal_gain,
                    0.0,
                    100.0,
                    |v, n| v.monster_abnormal_gain = n,
                    |ui| Self::render_item_lr(&self.backend, ui, self.dark_mode, &item_lr, &item_out, 8),
                );
            }
        });
    }

    /// 评分特效震动 (等级 + 细分事件)。
    fn vib_score_effect_section(&mut self, ui: &mut egui::Ui, job_mode: bool, snap: &VibSnap) {
        let th = self.theme();
        let rank_lr: [u32; 30] = snap.rank_lr.as_slice().try_into().unwrap();
        let rank_out: [u32; 30] = snap.rank_out.as_slice().try_into().unwrap();

        th.card_with_actions(
            ui,
            Some("评分特效震动"),
            |ui| {
                card_note(
                    ui,
                    &th,
                    &format!("等级 {} · 累计 {} 次", snap.rank_last, snap.rank_events),
                );
            },
            |ui| {
            /* 最近评分状态 */
            let rl = snap.rank_last;
            let (rtext, rcolor) = if rl >= 8 {
                (format!("★ 最近评分: 等级 {rl}（特殊动作!）  累计 {} 次", snap.rank_events), th.warn)
            } else if rl >= 2 {
                (format!("● 最近评分: 等级 {rl}  累计 {} 次", snap.rank_events), th.info)
            } else {
                ("○ 暂无评分事件（进副本打怪触发）".to_string(), th.hint)
            };
            ui.label(egui::RichText::new(rtext).size(11.5).color(rcolor));
            ui.add_space(theme::SP_S);

            /* 两个总旋钮并排: 全宽单行滑轨太长, 左右各一个更好扫读 */
            let backend = &self.backend;
            widgets::split_cols(ui, 2, theme::SP_2XL, |ui, col| {
                if col == 0 {
                    cfg_row(
                        ui,
                        &th,
                        backend,
                        job_mode,
                        vib_row(ic::STAR, "评分等级强度").suffix("%")
                            .hint("等级 F~SSS 的整体力度"),
                        snap.rank_level_gain,
                        0.0,
                        100.0,
                        |v, n| v.rank_level_gain = n,
                    );
                } else {
                    cfg_row(
                        ui,
                        &th,
                        backend,
                        job_mode,
                        vib_row(ic::PULSE, "满幅脉冲时长 ms")
                            .hint("评分脉冲多厚；0 = 只闪一下"),
                        snap.rank_duration,
                        20.0,
                        1000.0,
                        |v, n| v.rank_duration = n,
                    );
                }
            });

            widgets::group_head(ui, &th, "细分事件强度 (0 = 关闭)");
            let rank_names = [
                ("评分点", 0usize, "每次命中累积的评分点"),
                ("极限闪避", 1, "闪避/反击成功瞬间"),
                ("暴击", 2, "暴击命中瞬间（未定位）"),
                ("最终击杀", 5, "BOSS 击杀/结算（未定位）"),
                ("凌空追击", 6, "空中追击命中（槽评分）"),
                ("命中第一击", 7, "命中怪物第一击（原破甲槽）"),
                ("增益叠加", 8, "增益层数叠加（未定位）"),
                ("释放技能", 9, "释放技能瞬间"),
                ("暴击特写", 13, "暴击/击杀时的镜头特写震屏"),
                ("怪物死亡", 14, "怪物死亡（OnTargetDie）"),
            ];
            let half = rank_names.len().div_ceil(2);
            let backend = &self.backend;
            let dark = self.dark_mode;
            widgets::split_cols(ui, 2, theme::SP_2XL, |ui, col| {
                let slice = if col == 0 {
                    &rank_names[..half]
                } else {
                    &rank_names[half..]
                };
                for (name, idx, hint) in slice.iter().copied() {
                    let cnt = snap.rank_type_events[idx];
                    cfg_row_lr(
                        ui,
                        &th,
                        backend,
                        job_mode,
                        vib_row(ic::TARGET, name).tag(&format!("触发{cnt}")).hint(hint),
                        snap.rank_type_gain[idx],
                        0.0,
                        100.0,
                        |v, n| v.rank_type_gain[idx] = n,
                        |ui| Self::render_rank_lr(backend, ui, dark, &rank_lr, &rank_out, idx),
                    );
                }
            });
            ui.add_space(theme::SP_XS);
            if ui.add(th.secondary_button("测试评分震动 (模拟等级 8)")).clicked() {
                self.backend.dispatch(crate::ui_api::UiCommand::TestRank);
            }
        });
    }

    /// 评分动态衰减 (类鬼泣)。
    fn vib_score_decay_section(
        &mut self,
        ui: &mut egui::Ui,
        job_mode: bool,
        vib: &VibrationConfig,
    ) {
        let th = self.theme();
        let rd = vib.rank_decay_enabled;
        th.card_with_actions(
            ui,
            Some("评分动态衰减 · 类鬼泣"),
            |ui| {
                card_note(ui, &th, if rd { "已启用" } else { "未启用" });
            },
            |ui| {
            ui.label(
                egui::RichText::new(
                    "攻击/命中提升评级（0-8），停手超过延迟后逐级衰减。最高评级反馈 = 当前预设 ×120%；最低档保底 60%。",
                )
                .size(11.0)
                .color(th.hint),
            );
            ui.add_space(theme::SP_S);
            let mut rd_on = rd;
            if widgets::check_row(
                ui,
                &th,
                &mut rd_on,
                "启用评分动态衰减",
                "开：越打越猛、停手跌分；关：固定强度",
                "攻击/命中提升评级 (0-8), 停手超过延迟后逐级衰减。\n最高评级反馈 = 当前预设参数 ×1.2; 最低档保底 0.6 (不失去基础反馈)。",
            ) {
                Self::vib_slider_update(&self.backend, job_mode, |v| v.rank_decay_enabled = rd_on);
            }
            let gate = |r: Row<'static>| if rd_on { r } else { r.off() };
            /* 全宽卡内两列: 左 = 节奏 (多久开始跌 / 跌多快), 右 = 幅度 (保底 / 上限) */
            let backend = &self.backend;
            widgets::split_cols(ui, 2, theme::SP_2XL, |ui, col| {
                if col == 0 {
                    cfg_row(
                        ui,
                        &th,
                        backend,
                        job_mode,
                        gate(vib_row(ic::HOURGLASS, "停手衰减延迟 ms")),
                        vib.rank_decay_delay_ms,
                        500.0,
                        5000.0,
                        |v, n| v.rank_decay_delay_ms = n,
                    );
                    cfg_row(
                        ui,
                        &th,
                        backend,
                        job_mode,
                        gate(vib_row(ic::TREND_DOWN, "衰减速度 级/秒 (整数)")),
                        vib.rank_decay_speed,
                        1.0,
                        5.0,
                        |v, n| v.rank_decay_speed = n,
                    );
                } else {
                    cfg_row(
                        ui,
                        &th,
                        backend,
                        job_mode,
                        gate(vib_row(ic::ARROW_DOWN, "最低反馈倍率 % (评级 0, 保底)")),
                        vib.rank_decay_min_mul,
                        40.0,
                        100.0,
                        |v, n| v.rank_decay_min_mul = n,
                    );
                    cfg_row(
                        ui,
                        &th,
                        backend,
                        job_mode,
                        gate(vib_row(ic::ARROW_UP, "最高反馈倍率 % (满评分, 预设×此值)")),
                        vib.rank_decay_max_mul,
                        100.0,
                        200.0,
                        |v, n| v.rank_decay_max_mul = n,
                    );
                }
            });
        });
    }

    /// ④ 高级震动调校 (折叠; 展开后 6 个主题面板双列)。
    fn vib_advanced_section(
        &mut self,
        ui: &mut egui::Ui,
        job_mode: bool,
        legacy: bool,
        vib: &VibrationConfig,
        snap: &VibSnap,
    ) {
        let th = self.theme();

        th.card(ui, Some("高级震动调校"), |ui| {
            ui.label(
                egui::RichText::new(
                    "基础分区已能获得良好手感；这里做精细微调（群怪治理/连击/移动/输出整形/算法窗口/脉冲）。",
                )
                .size(11.0)
                .color(th.hint),
            );
            ui.add_space(theme::SP_S);

            egui::CollapsingHeader::new(
                egui::RichText::new("展开高级参数分区")
                    .size(12.0)
                    .strong(),
            )
            .default_open(false)
            .show(ui, |ui| {
                let mut adv_on = if job_mode {
                    self.vib_job_adv_on
                } else {
                    vib.advanced_enabled
                };
                ui.horizontal(|ui| {
                    if ui
                        .checkbox(&mut adv_on, egui::RichText::new("允许高级调校").size(12.5))
                        .on_hover_text(
                            "默认关闭: 只用基础分区即可, 高级参数保持当前配置值。\n\
                             开启后可在下方精细调整 L/R 马达比例/衰减/窗口/连击/脉冲。",
                        )
                        .changed()
                    {
                        if job_mode {
                            self.vib_job_adv_on = adv_on;
                        } else {
                            Self::vib_slider_update(&self.backend, job_mode, |v| {
                                v.advanced_enabled = adv_on
                            });
                        }
                    }
                    ui.label(
                        egui::RichText::new(if adv_on {
                            "已开启：可精细调整下方参数"
                        } else {
                            "已关闭：高级参数保持当前配置"
                        })
                        .size(11.0)
                        .color(if adv_on { th.good } else { th.hint }),
                    );
                    ui.add_space(theme::SP_L);
                    ui.checkbox(
                        &mut self.vib_show_expert,
                        egui::RichText::new("显示专家参数").size(12.5),
                    )
                    .on_hover_text(
                        "默认关闭: 每个算法只显示最关键的旋钮, 减少干扰。\n\
                         开启后显示全部微调滑块 (窗口时长/封顶/补发/恢复快慢等), 关闭隐藏不影响参数值。",
                    );
                });

                /* S1 输出引擎模式 (引擎级选择, 置于顶部) */
                let s1_route = legacy;
                let s1_mode = vib.legacy_output_mode.min(2);
                if s1_route {
                    ui.add_space(theme::SP_S);
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new("S1 输出引擎模式")
                                .size(12.0)
                                .strong()
                                .color(th.text_weak),
                        );
                        let mut sel = s1_mode;
                        ui.selectable_value(&mut sel, 0, "经典");
                        ui.selectable_value(&mut sel, 1, "S4 纯");
                        ui.selectable_value(&mut sel, 2, "S4+不丢震动");
                        if sel != s1_mode {
                            Self::vib_slider_update(&self.backend, job_mode, |v| {
                                v.legacy_output_mode = sel;
                            });
                        }
                        ui.label(
                            egui::RichText::new("经典 = 现行 ACT1 管线；S4 = 输出整形；S4+ = 轻反馈攒厚再出")
                                .size(11.0)
                                .color(th.hint),
                        );
                    });
                }

                ui.add_space(theme::SP_S);
                let show_expert = self.vib_show_expert;

                /* ★v24.64: 面板改全宽纵排 —— 半宽面板展开后是一条 2000px 的长条,
                 * 旁边那列还空着; 全宽 + 面板内部两列, 长度直接砍半且不再有空洞 */
                /* 高级小节内容统一随「允许高级调校」置灰 (含检查框):
                 * 旧实现只有滑杆挂 gate, 检查框仍可点, 一屏里一半亮一半灰 */
                ui.data_mut(|d| d.insert_temp(egui::Id::new("vib_adv_on"), adv_on));
                let order: &[u8] = if legacy {
                    &[0, 1, 2, 3, 4, 5, 6] // 群怪 / 连击 / 走路 / 输出 / 窗口 / 脉冲 / 预留
                } else {
                    &[1, 2, 3, 4, 5, 6]
                };
                for k in order {
                    match *k {
                        0 => self.vib_adv_crowd_panel(ui, job_mode, adv_on, vib, show_expert),
                        1 => self.vib_adv_combo_panel(ui, job_mode, adv_on, vib, snap, show_expert),
                        2 => self.vib_adv_walk_panel(ui, job_mode, adv_on, vib, snap),
                        3 => self.vib_adv_output_panel(ui, job_mode, adv_on, legacy, vib, snap),
                        4 => self.vib_adv_window_panel(ui, job_mode, adv_on, vib, snap),
                        5 => self.vib_adv_pulse_panel(ui, job_mode, adv_on, vib, snap),
                        _ => self.vib_adv_reserved_panel(ui, job_mode, adv_on, snap),
                    }
                }

                ui.add_space(theme::SP_XS);
                ui.label(
                    egui::RichText::new(
                        "衰减 0-5ms = 单帧脉冲（命中帧全幅直出、下一帧硬归零，最脆的\"一顿一顿\"）；6ms 起为指数衰减尾巴。里程碑档位固定 50/100/200/400 连击。",
                    )
                    .size(11.0)
                    .color(th.hint),
                );
            });
        });
    }

    /// 高级小节: 平铺折叠 (收起 = 一行标题 + 一条分隔线, 不再套一层卡片)。
    ///
    /// ★v24.64: 之前每个小节是一张 card_alt 小卡, 卡片套卡片再套参数卡, 三层嵌套既重
    /// 又高; 改平铺后收起状态只有 30px, 7 个小节一整列也不过一屏, 展开哪一个都不会把
    /// 另一列顶出空洞。
    fn adv_panel(
        ui: &mut egui::Ui,
        th: &Theme,
        title: &str,
        desc: &str,
        add: impl FnOnce(&mut egui::Ui),
    ) {
        widgets::hairline(ui, th);
        ui.add_space(theme::SP_XS);
        egui::CollapsingHeader::new(
            egui::RichText::new(title)
                .size(12.5)
                .strong()
                .family(Theme::font_bold())
                .color(th.heading),
        )
        .id_salt(title)
        .default_open(false)
        .show(ui, |ui| {
            if !desc.is_empty() {
                ui.label(egui::RichText::new(desc).size(11.0).color(th.hint));
                ui.add_space(theme::SP_S);
            }
            let adv_on = ui
                .data(|d| d.get_temp::<bool>(egui::Id::new("vib_adv_on")))
                .unwrap_or(true);
            ui.add_enabled_ui(adv_on, |ui| add(ui));
        });
        ui.add_space(theme::SP_XS);
    }

    /// 群怪不打手 (S1 命中治理)。
    fn vib_adv_crowd_panel(
        &mut self,
        ui: &mut egui::Ui,
        job_mode: bool,
        adv_on: bool,
        vib: &VibrationConfig,
        show_expert: bool,
    ) {
        let th = self.theme();
        let gate = |r: Row<'static>| if adv_on { r } else { r.off() };
        Self::adv_panel(
            ui,
            &th,
            "群怪不打手 (命中治理)",
            "打群怪时同一瞬间的多次命中合成一下更厚的震动；打久了自动再变轻。单挑完全不受影响。",
            |ui| {
                let backend = &self.backend;
                widgets::split_cols(ui, 2, theme::SP_2XL, |ui, col| {
                    if col == 0 {
                let mut merge = vib.merge_enabled;
                if widgets::check_row(
                    ui,
                    &th,
                    &mut merge,
                    "一刀多怪合并",
                    "开：群怪一刀只震一下；关：回到旧行为",
                    "关掉后: 命中聚合窗/合并记账/补发全部回到旧行为 (不做合并, 不补发)。默认开。",
                ) {
                    Self::vib_slider_update(&self.backend, job_mode, |v| v.merge_enabled = merge);
                }
                cfg_row(
                    ui,
                    &th,
                    &self.backend,
                    job_mode,
                    gate(vib_row(ic::ARROWS_MERGE, "命中聚合窗 ms")),
                    vib.hitmerge_ms,
                    10.0,
                    200.0,
                    |v, n| v.hitmerge_ms = n,
                );
                if show_expert {
                    cfg_row(
                        ui,
                        &th,
                        &self.backend,
                        job_mode,
                        gate(vib_row(ic::PERCENT, "合成一下时保留多少力度 % (0=关闭)")),
                        vib.merge_keep,
                        0.0,
                        100.0,
                        |v, n| v.merge_keep = n,
                    );
                    cfg_row(
                        ui,
                        &th,
                        &self.backend,
                        job_mode,
                        gate(vib_row(ic::ARROW_UP, "合成一下的最大力度 % (100=满力)")),
                        vib.merge_cap,
                        20.0,
                        150.0,
                        |v, n| v.merge_cap = n,
                    );
                    cfg_row(
                        ui,
                        &th,
                        &self.backend,
                        job_mode,
                        gate(vib_row(ic::CLOCK, "漏掉的震动多久后补一下 (ms, 0=不补)")),
                        vib.merge_hold,
                        0.0,
                        300.0,
                        |v, n| v.merge_hold = n,
                    );
                }

                widgets::group_head(ui, &th, "命中限频 (每秒最多震几下)");
                let mut cap = vib.hitcap_enabled;
                if widgets::check_row(
                    ui,
                    &th,
                    &mut cap,
                    "启用命中限频",
                    "超额命中不消失，会并成更厚的一下",
                    "关掉后: 命中不再受每秒次数上限约束 (超额不再记账合并)。默认开。",
                ) {
                    Self::vib_slider_update(&self.backend, job_mode, |v| v.hitcap_enabled = cap);
                }
                cfg_row(
                    ui,
                    &th,
                    &self.backend,
                    job_mode,
                    gate(vib_row(ic::CIRCLE_NOTCH, "每秒最多震几下 (0=不限制)")),
                    vib.hitcap_max,
                    0.0,
                    30.0,
                    |v, n| v.hitcap_max = n,
                );
                if show_expert {
                    cfg_row(
                        ui,
                        &th,
                        &self.backend,
                        job_mode,
                        gate(vib_row(ic::TIMER, "限频的计时窗口 ms")),
                        vib.hitcap_win_ms,
                        200.0,
                        5000.0,
                        |v, n| v.hitcap_win_ms = n,
                    );
                }

                                    } else {widgets::group_head(ui, &th, "命中风暴抽样");
                let mut storm = vib.storm_enabled;
                if widgets::check_row(
                    ui,
                    &th,
                    &mut storm,
                    "启用命中风暴抽样",
                    "命中暴增时只震一部分，停手即恢复刀刀震",
                    "关掉后: 命中不再被抽样丢弃 (每一击都震), 但「风暴期」检测继续运行。默认开。",
                ) {
                    Self::vib_slider_update(&self.backend, job_mode, |v| v.storm_enabled = storm);
                }
                cfg_row(
                    ui,
                    &th,
                    &self.backend,
                    job_mode,
                    gate(vib_row(ic::LIGHTNING, "一秒内命中超过几下算风暴")),
                    vib.storm_thr,
                    2.0,
                    30.0,
                    |v, n| v.storm_thr = n,
                );
                cfg_row(
                    ui,
                    &th,
                    &self.backend,
                    job_mode,
                    gate(vib_row(ic::PERCENT, "风暴期保留多少 % 命中震动")),
                    vib.storm_keep_pct,
                    10.0,
                    100.0,
                    |v, n| v.storm_keep_pct = n,
                );
                cfg_row(
                    ui,
                    &th,
                    &self.backend,
                    job_mode,
                    gate(vib_row(ic::HOURGLASS, "停手多久恢复刀刀震 ms")),
                    vib.storm_pause_ms,
                    100.0,
                    2000.0,
                    |v, n| v.storm_pause_ms = n,
                );
                if show_expert {
                    cfg_row(
                        ui,
                        &th,
                        &self.backend,
                        job_mode,
                        gate(vib_row(ic::TIMER, "风暴的统计窗口 ms")),
                        vib.storm_win_ms,
                        200.0,
                        5000.0,
                        |v, n| v.storm_win_ms = n,
                    );
                }
                let mut uni = vib.storm_unified_enabled;
                if widgets::check_row(
                    ui,
                    &th,
                    &mut uni,
                    "统合衰减期 (风暴期每击重新起振)",
                    "一击接一击的干净持续震动，旧尾巴不再叠糊",
                    "风暴期普通命中改用一条统合包络: 每命中一次就把旧包络瞬间清零、重新起振。默认不勾选。",
                ) {
                    Self::vib_slider_update(&self.backend, job_mode, |v| {
                        v.storm_unified_enabled = uni
                    });
                }
                if show_expert {
                    cfg_row(
                        ui,
                        &th,
                        &self.backend,
                        job_mode,
                        gate(vib_row(ic::WAVE_SINE, "统合衰减期 ms (越大越连贯, 40-400)")),
                        vib.storm_unified_ms,
                        40.0,
                        400.0,
                        |v, n| v.storm_unified_ms = n,
                    );
                }
                let mut sma = vib.storm_mute_abnormal;
                if widgets::check_row(
                    ui,
                    &th,
                    &mut sma,
                    "风暴期关闭怪物异常反馈",
                    "出血/中毒跳字在风暴期静音，风暴结束自动恢复",
                    "勾选后: 命中风暴期间, 怪物出血/中毒等跳字的震动暂时关掉。默认不勾选。",
                ) {
                    Self::vib_slider_update(&self.backend, job_mode, |v| v.storm_mute_abnormal = sma);
                }
                let mut smr = vib.storm_mute_rank;
                if widgets::check_row(
                    ui,
                    &th,
                    &mut smr,
                    "风暴期关闭评分点系统",
                    "评分/击杀/怪物死亡等整族静音",
                    "勾选后: 命中风暴期间, 评分点系统整族事件暂时不震; 风暴结束自动恢复。默认不勾选。",
                ) {
                    Self::vib_slider_update(&self.backend, job_mode, |v| v.storm_mute_rank = smr);
                }

                widgets::group_head(ui, &th, "持续降温 / 脉冲落地");
                let mut sus = vib.sustain_enabled;
                if widgets::check_row(
                    ui,
                    &th,
                    &mut sus,
                    "启用持续降温",
                    "一直打群怪自动变轻，停手恢复",
                    "关掉后: 打久了不再自动变轻 (持续压制关闭)。默认开。",
                ) {
                    Self::vib_slider_update(&self.backend, job_mode, |v| v.sustain_enabled = sus);
                }
                cfg_row(
                    ui,
                    &th,
                    &self.backend,
                    job_mode,
                    gate(vib_row(ic::FIRE, "打群怪几秒后自动降温 (秒, 0=关闭)")),
                    vib.sustain_secs,
                    0.0,
                    10.0,
                    |v, n| v.sustain_secs = n,
                );
                cfg_row(
                    ui,
                    &th,
                    &self.backend,
                    job_mode,
                    gate(vib_row(ic::ARROW_DOWN, "自动降温幅度 %")),
                    vib.sustain_reduce,
                    10.0,
                    80.0,
                    |v, n| v.sustain_reduce = n,
                );
                let mut tail = vib.tail_land_enabled;
                if widgets::check_row(
                    ui,
                    &th,
                    &mut tail,
                    "启用脉冲落地",
                    "余震提前切断，一击更干脆",
                    "关掉后: 每下的余震不再被提前切断 (保持自然余音)。默认开。",
                ) {
                    Self::vib_slider_update(&self.backend, job_mode, |v| {
                        v.tail_land_enabled = tail
                    });
                }
                if show_expert {
                    cfg_row(
                        ui,
                        &th,
                        &self.backend,
                        job_mode,
                        gate(vib_row(ic::SCISSORS, "震尾多快切断 % (0=自然衰减)")),
                        vib.tail_land_pct,
                        0.0,
                        60.0,
                        |v, n| v.tail_land_pct = n,
                    );
                }
                    }
                });

            },
        );
    }

    /// 连击与自适应。
    fn vib_adv_combo_panel(
        &mut self,
        ui: &mut egui::Ui,
        job_mode: bool,
        adv_on: bool,
        vib: &VibrationConfig,
        snap: &VibSnap,
        show_expert: bool,
    ) {
        let th = self.theme();
        let gate = |r: Row<'static>| if adv_on { r } else { r.off() };
        Self::adv_panel(
            ui,
            &th,
            "连击与自适应",
            "连击强度 = 总闸的「连击增强」× 每 100 连加强，渐进到上限；打太快自动减轻、打太慢自动恢复。",
            |ui| {
                let backend = &self.backend;
                widgets::split_cols(ui, 2, theme::SP_2XL, |ui, col| {
                    if col == 0 {
                /* 连击密度自适应 (高连击职业专属) */
                let d_thr = snap.params[29];
                if job_mode && d_thr >= 100 {
                    ui.label(
                        egui::RichText::new("连击密度自适应: 该职业无此专属算法（仅高连击职业启用）")
                            .size(11.0)
                            .color(th.hint),
                    );
                } else {
                    let mut den = vib.density_enabled;
                    if widgets::check_row(
                        ui,
                        &th,
                        &mut den,
                        "启用连击密度自适应",
                        "短时间连击暴增时自动降强度，防震手",
                        "关掉后: 连击暴增时不再自动降强度 (等效阈值 100)。默认开。",
                    ) {
                        Self::vib_slider_update(&self.backend, job_mode, |v| {
                            v.density_enabled = den
                        });
                    }
                    if job_mode {
                        ui.label(
                            egui::RichText::new("【当前职业专属】高连击职业窗口期自适应降强度")
                                .size(11.0)
                                .color(th.accent_text),
                        );
                    }
                    let d_rows = [
                        ("多快算太密: 连击次数 (100=关闭)", 29usize, 5.0, 100.0, false),
                        ("统计时长 ms", 30, 200.0, 1500.0, true),
                        ("太密时减轻多少 %", 31, 0.0, 80.0, false),
                        ("停手多久恢复力度 ms", 32, 300.0, 3000.0, true),
                        ("最轻不低于原来的 %", 33, 30.0, 100.0, false),
                        ("恢复快慢 ms (0=立刻)", 34, 0.0, 800.0, true),
                    ];
                    for (name, idx, lo, hi, expert_only) in d_rows {
                        if expert_only && !show_expert {
                            continue;
                        }
                        slot_row(
                            ui,
                            &th,
                            &self.backend,
                            job_mode,
                            gate(vib_row(ic::ARROWS_IN_LINE_VERTICAL, name)),
                            idx,
                            snap.params[idx],
                            lo,
                            hi,
                        );
                    }
                }

                                    } else {widgets::group_head(ui, &th, "命中自适应 (打太快自动减轻)");
                let mut adapt = vib.adapt_enabled;
                if widgets::check_row(
                    ui,
                    &th,
                    &mut adapt,
                    "启用命中自适应",
                    "打太快自动减轻、打太慢自动恢复",
                    "关掉后: 打太快时不再自动减轻 (等效降幅 0)。默认开。",
                ) {
                    Self::vib_slider_update(&self.backend, job_mode, |v| v.adapt_enabled = adapt);
                }
                let adapt_rows = [
                    ("连击加成的上限 (倍数×100)", 47usize, 100.0, 500.0, false),
                    ("连击越长震越强: 每 100 连加强多少 %", 48, 1.0, 300.0, true),
                    ("连击很久没连上时补一记的门槛", 49, 10.0, 100.0, true),
                    ("打多快算太快: 间隔低于此开始减弱 ms", 50, 30.0, 300.0, false),
                    ("打太快时减轻多少 %", 51, 0.0, 50.0, false),
                    ("打多慢算正常: 间隔超过此恢复满强度 ms", 52, 100.0, 1000.0, true),
                    ("减弱后最轻不低于原来的 %", 53, 0.0, 100.0, true),
                ];
                for (name, idx, lo, hi, expert_only) in adapt_rows {
                    if expert_only && !show_expert {
                        continue;
                    }
                    slot_row(
                        ui,
                        &th,
                        &self.backend,
                        job_mode,
                        gate(vib_row(ic::ARROWS_CLOCKWISE, name)),
                        idx,
                        snap.params[idx],
                        lo,
                        hi,
                    );
                }
                    }
                });

            },
        );
    }

    /// 走路质感与走位能量。
    fn vib_adv_walk_panel(
        &mut self,
        ui: &mut egui::Ui,
        job_mode: bool,
        adv_on: bool,
        vib: &VibrationConfig,
        snap: &VibSnap,
    ) {
        let th = self.theme();
        let gate = |r: Row<'static>| if adv_on { r } else { r.off() };
        Self::adv_panel(
            ui,
            &th,
            "走路质感与走位能量",
            "轻量步伐，不抢主震动；走位越久，停手窗口内的下一次攻击越强。",
            |ui| {
                let backend = &self.backend;
                widgets::split_cols(ui, 2, theme::SP_2XL, |ui, col| {
                    if col == 0 {
                let m_rows = [
                    ("移动步频 ms (自然步频 ~380)", 23usize, 200.0, 800.0),
                    ("移动着地脉冲 % (柔和, 不宜高)", 24, 10.0, 100.0),
                    ("移动抬脚保持 % (极轻)", 25, 5.0, 50.0),
                    ("移动整体增益 % (轻音量)", 26, 10.0, 100.0),
                    ("移动平滑系数 % (越大过渡越柔, 消除嗡嗡声)", 27, 5.0, 100.0),
                    ("移动最低输出阈值 % (低于归0, 消除沙沙声)", 28, 0.0, 20.0),
                ];
                for (name, idx, lo, hi) in m_rows {
                    slot_row(
                        ui,
                        &th,
                        &self.backend,
                        job_mode,
                        gate(vib_row(ic::SNEAKER, name)),
                        idx,
                        snap.params[idx],
                        lo,
                        hi,
                    );
                }

                                    } else {widgets::group_head(ui, &th, "走位能量积累");
                let mut mc = vib.move_charge_enabled;
                if widgets::check_row(
                    ui,
                    &th,
                    &mut mc,
                    "启用走位能量积累",
                    "走位越久，下次攻击越强",
                    "关掉后: 长时间走位不再积累攻击增强 (等效速率 0)。默认开。",
                ) {
                    Self::vib_slider_update(&self.backend, job_mode, |v| v.move_charge_enabled = mc);
                }
                if job_mode {
                    let m_rate = snap.params[21];
                    if m_rate >= 12 {
                        ui.label(
                            egui::RichText::new("【走位职业】移动积累快，停手窗口内攻击增强明显")
                                .size(11.0)
                                .color(th.accent_text),
                        );
                    } else if m_rate <= 3 {
                        ui.label(
                            egui::RichText::new("【站桩职业】移动积累慢，增强弱")
                                .size(11.0)
                                .color(th.hint),
                        );
                    }
                }
                let c_rows = [
                    ("移动积累速率 %/秒 (0=禁用)", 21usize, 0.0, 20.0),
                    ("攻击增强上限 % (最多 ×(1+上限))", 22, 0.0, 100.0),
                    ("增强窗口 ms (停手后有效)", 39, 0.0, 3000.0),
                ];
                for (name, idx, lo, hi) in c_rows {
                    slot_row(
                        ui,
                        &th,
                        &self.backend,
                        job_mode,
                        gate(vib_row(ic::PERSON_SIMPLE, name)),
                        idx,
                        snap.params[idx],
                        lo,
                        hi,
                    );
                }
                    }
                });

            },
        );
    }

    /// 输出质感 (波形/平滑/节奏/马达分工/衰减/S4 整形)。
    fn vib_adv_output_panel(
        &mut self,
        ui: &mut egui::Ui,
        job_mode: bool,
        adv_on: bool,
        legacy: bool,
        vib: &VibrationConfig,
        snap: &VibSnap,
    ) {
        let th = self.theme();
        let item_lr: [u32; 26] = snap.item_lr.as_slice().try_into().unwrap();
        let item_out: [u32; 26] = snap.item_out.as_slice().try_into().unwrap();
        let gate = |r: Row<'static>| if adv_on { r } else { r.off() };
        let s1_route = legacy;
        let s1_mode = vib.legacy_output_mode.min(2);
        Self::adv_panel(
            ui,
            &th,
            "输出质感 (波形/平滑/分工/衰减)",
            "决定「震起来像什么」：波形饱和度、平滑度、左右分工、各类事件余震长度。",
            |ui| {
                let backend = &self.backend;
                widgets::split_cols(ui, 2, theme::SP_2XL, |ui, col| {
                    if col == 0 {
                slot_row(
                    ui,
                    &th,
                    &self.backend,
                    job_mode,
                    gate(vib_row(ic::WAVE_SINE, "左马达曲线 %").hint("越大越饱和，100 = 线性")),
                    19,
                    snap.params[19],
                    30.0,
                    300.0,
                );
                slot_row(
                    ui,
                    &th,
                    &self.backend,
                    job_mode,
                    gate(vib_row(ic::WAVE_SINE, "右马达曲线 %").hint("越大越饱和，100 = 线性")),
                    20,
                    snap.params[20],
                    30.0,
                    300.0,
                );
                cfg_row(
                    ui,
                    &th,
                    &self.backend,
                    job_mode,
                    gate(vib_row(ic::WAVEFORM, "输出平滑 %")
                        .hint("高 = 柔（消除嗡嗡声）；低 = 锐利")),
                    vib.out_smooth,
                    0.0,
                    100.0,
                    |v, n| v.out_smooth = n,
                );
                slot_row(
                    ui,
                    &th,
                    &self.backend,
                    job_mode,
                    gate(vib_row(ic::ARROWS_LEFT_RIGHT, "节奏感 (右马达比例)")
                        .hint("50 = 均衡；高 = 右重左轻")),
                    18,
                    snap.params[18],
                    0.0,
                    100.0,
                );

                let split_active = !legacy || s1_mode != 0;
                let mut sp = vib.split_enabled;
                if widgets::check_row(
                    ui,
                    &th,
                    &mut sp,
                    "马达分工 (Xbox360 风格)",
                    if split_active {
                        "轻反馈只驱动主导马达，重反馈双马达满幅"
                    } else {
                        "经典 ACT1 管线不参与分工（会抹平左右差异）"
                    },
                    "轻反馈 (峰值低于分界) 仅驱动主导马达 (转子声/功耗更低); 重反馈双马达满幅。\n(经典 ACT1 管线不参与分工; 切到 S4 模式后此开关生效)",
                ) {
                    Self::vib_slider_update(&self.backend, job_mode, |v| v.split_enabled = sp);
                }
                let row = vib_row(ic::ARROWS_SPLIT, "分工分界 % (低于为轻反馈)");
                cfg_row(
                    ui,
                    &th,
                    &self.backend,
                    job_mode,
                    if sp && split_active { row } else { row.off() },
                    vib.split_thr,
                    20.0,
                    90.0,
                    |v, n| v.split_thr = n,
                );

                widgets::group_head(ui, &th, "各类事件衰减时长");
                let mut dec = vib.decay_enabled;
                if widgets::check_row(
                    ui,
                    &th,
                    &mut dec,
                    "启用自定义衰减时长",
                    "关 = 使用内置默认（命中25/特殊55/受击25/状态60）",
                    "关掉后: 四个衰减滑块不生效, 改用内置「默认」预设的衰减时长。默认开。",
                ) {
                    Self::vib_slider_update(&self.backend, job_mode, |v| v.decay_enabled = dec);
                }
                let dec_rows = [
                    ("普通命中衰减 ms", 35usize),
                    ("特殊攻击衰减 ms", 36),
                    ("受击衰减 ms", 37),
                    ("状态变化衰减 ms", 38),
                ];
                for (name, idx) in dec_rows {
                    slot_row(
                        ui,
                        &th,
                        &self.backend,
                        job_mode,
                        gate(vib_row(ic::TIMER, name)),
                        idx,
                        snap.params[idx],
                        0.0,
                        150.0,
                    );
                }
                widgets::row_card(ui, &th, |ui| {
                    ui.label(
                        egui::RichText::new("衰减时长 L/R 权重 (正 = 更持久, 负 = 更短)")
                            .size(11.0)
                            .color(th.hint),
                    );
                    ui.add_space(4.0);
                    Self::render_item_lr(&self.backend, ui, self.dark_mode, &item_lr, &item_out, 3);
                });

                    } else {
                /* S4 输出整形组 */
                let s4_shape_on = !s1_route || s1_mode != 0;
                widgets::group_head(
                    ui,
                    &th,
                    if !s4_shape_on {
                        "S4+ 新方案专属 (经典 ACT1 不受影响)"
                    } else {
                        "S4 输出整形"
                    },
                );
                cfg_row(
                    ui,
                    &th,
                    &self.backend,
                    job_mode,
                    if s4_shape_on {
                        vib_row(ic::ARROW_DOWN, "输出低强度抑制 % (转子马达建议 25-35)")
                    } else {
                        vib_row(ic::ARROW_DOWN, "输出低强度抑制 %").off()
                    },
                    vib.out_threshold,
                    0.0,
                    60.0,
                    |v, n| v.out_threshold = n,
                );
                let mut rm = vib.remap_enabled;
                if ui
                    .add_enabled(
                        s4_shape_on,
                        egui::Checkbox::new(&mut rm, egui::RichText::new("输出动态范围重映射").size(12.5)),
                    )
                    .on_hover_text(
                        "把非零输出映射到 [最小输出, 100%] 区间: 任何非零反馈至少以最小输出驱动马达, 相对强弱保留。",
                    )
                    .changed()
                {
                    Self::vib_slider_update(&self.backend, job_mode, |v| v.remap_enabled = rm);
                }
                let row = vib_row(ic::ARROW_UP, "重映射最小输出 % (与死区一致)");
                cfg_row(
                    ui,
                    &th,
                    &self.backend,
                    job_mode,
                    if rm && s4_shape_on { row } else { row.off() },
                    vib.remap_min,
                    10.0,
                    60.0,
                    |v, n| v.remap_min = n,
                );

                widgets::group_head(ui, &th, "震动节流 (仅 S4)");
                if s1_route {
                    ui.label(
                        egui::RichText::new("节流仅 S4 新方案生效（ACT 版本由「群怪不打手」接管）")
                            .size(11.0)
                            .color(th.hint),
                    );
                } else if job_mode && vib.throttle_window_ms > 0 {
                    ui.label(
                        egui::RichText::new("【当前职业专属】狂震节流: 窗口内只震几次，杜绝狂震")
                            .size(11.0)
                            .color(th.accent_text),
                    );
                }
                let gate_s4 = |r: Row<'static>| if !s1_route { r } else { r.off() };
                cfg_row(
                    ui,
                    &th,
                    &self.backend,
                    job_mode,
                    gate_s4(vib_row(ic::CIRCLE_NOTCH, "震动节流窗口 ms (0=禁用)")),
                    vib.throttle_window_ms,
                    0.0,
                    10000.0,
                    |v, n| v.throttle_window_ms = n,
                );
                cfg_row(
                    ui,
                    &th,
                    &self.backend,
                    job_mode,
                    gate_s4(vib_row(ic::CIRCLE_NOTCH, "窗口内最大震动次数")),
                    vib.throttle_max_hits,
                    1.0,
                    30.0,
                    |v, n| v.throttle_max_hits = n,
                );
                cfg_row(
                    ui,
                    &th,
                    &self.backend,
                    job_mode,
                    gate_s4(vib_row(ic::PERCENT, "狂震期次数比例 % (100=不收紧)")),
                    vib.throttle_dense_ratio,
                    1.0,
                    100.0,
                    |v, n| v.throttle_dense_ratio = n,
                );
                    }
                });

            },
        );
    }

    /// 算法窗口时长。
    fn vib_adv_window_panel(
        &mut self,
        ui: &mut egui::Ui,
        job_mode: bool,
        adv_on: bool,
        vib: &VibrationConfig,
        snap: &VibSnap,
    ) {
        let th = self.theme();
        let gate = |r: Row<'static>| if adv_on { r } else { r.off() };
        Self::adv_panel(
            ui,
            &th,
            "算法窗口时长",
            "持续/节奏/爆发/连击统计的时间窗口 —— 一般不用动，动了会改变所有算法的节奏感。",
            |ui| {
                let mut aw = vib.algo_windows_enabled;
                if widgets::check_row(
                    ui,
                    &th,
                    &mut aw,
                    "启用自定义算法窗口",
                    "关 = 使用内置默认窗口",
                    "关掉后: 本组七个窗口滑块不生效, 改用内置「默认」预设的窗口值。默认开。",
                ) {
                    Self::vib_slider_update(&self.backend, job_mode, |v| {
                        v.algo_windows_enabled = aw
                    });
                }
                let d_rows = [
                    ("DOT 持续反馈时长 ms", 40usize, 50.0, 1000.0),
                    ("装备特效节奏周期 ms", 41, 60.0, 600.0),
                    ("爆发窗口 ms (窗口内 6 连触发爆发)", 42, 100.0, 1000.0),
                    ("爆发模式最低强度 %", 43, 0.0, 100.0),
                    ("反击窗口 ms (受击后攻击强化)", 44, 300.0, 2000.0),
                    ("连击统计窗口 ms", 45, 500.0, 5000.0),
                    ("空闲判定 ms (回战斗前状态)", 46, 1000.0, 10000.0),
                ];
                for (name, idx, lo, hi) in d_rows {
                    slot_row(
                        ui,
                        &th,
                        &self.backend,
                        job_mode,
                        gate(vib_row(ic::CLOCK, name)),
                        idx,
                        snap.params[idx],
                        lo,
                        hi,
                    );
                }
            },
        );
    }

    /// 静默 / 反击 / 脉冲 / 测试强度。
    fn vib_adv_pulse_panel(
        &mut self,
        ui: &mut egui::Ui,
        job_mode: bool,
        adv_on: bool,
        vib: &VibrationConfig,
        snap: &VibSnap,
    ) {
        let th = self.theme();
        let gate = |r: Row<'static>| if adv_on { r } else { r.off() };
        Self::adv_panel(
            ui,
            &th,
            "静默 / 反击 / 脉冲",
            "特殊攻击后的静默、受击后的反击加成、唤醒/里程碑/断连脉冲与测试按钮强度。",
            |ui| {
                let mut pe = vib.pulse_enabled;
                if widgets::check_row(
                    ui,
                    &th,
                    &mut pe,
                    "启用静默/反击/脉冲",
                    "关 = 全关：更平、更连续（测试强度不受影响）",
                    "关掉后: 特殊攻击后不再静默、受击后不再反击加成、唤醒/里程碑/连击收尾脉冲全部关闭。默认开。",
                ) {
                    Self::vib_slider_update(&self.backend, job_mode, |v| v.pulse_enabled = pe);
                }
                let f_rows = [
                    ("特殊攻击后静默 ms (突显暴击破招)", 54usize, 0.0, 500.0),
                    ("反击强化倍数 %", 55, 100.0, 300.0),
                    ("唤醒脉冲强度 %", 56, 0.0, 100.0),
                    ("里程碑脉冲强度 %", 57, 0.0, 100.0),
                    ("连击中断收尾脉冲 %", 58, 0.0, 100.0),
                    ("测试震动强度 %", 59, 1.0, 100.0),
                ];
                for (name, idx, lo, hi) in f_rows {
                    slot_row(
                        ui,
                        &th,
                        &self.backend,
                        job_mode,
                        gate(vib_row(ic::PULSE, name)),
                        idx,
                        snap.params[idx],
                        lo,
                        hi,
                    );
                }
            },
        );
    }

    /// 预留槽位 (引擎无消费点, 保留显示防误解)。
    fn vib_adv_reserved_panel(
        &mut self,
        ui: &mut egui::Ui,
        job_mode: bool,
        adv_on: bool,
        snap: &VibSnap,
    ) {
        let th = self.theme();
        Self::adv_panel(
            ui,
            &th,
            "预留槽位",
            "当前版本引擎不读取，改动无效 —— 保留显示以免误以为是新参数。",
            |ui| {
                let row = vib_row(ic::HOURGLASS, "衰减时间 (预留)").suffix("ms");
                slot_row(
                    ui,
                    &th,
                    &self.backend,
                    job_mode,
                    if adv_on { row } else { row.off() },
                    5,
                    snap.params[5],
                    0.0,
                    500.0,
                );
            },
        );
    }
}
