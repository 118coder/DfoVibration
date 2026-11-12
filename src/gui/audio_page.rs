//! ★R2.2 音频震动页 —— 界面重排版 (只动排版与视觉, 引擎/配置语义零改动)。
//!
//! 版面自上而下: ①状态头 (开关 + 状态 + 快捷自检/校准) → ②校准建议条 →
//! ③预设宫格 → ④实时监测 (频谱 / 马达 / 统计 / 判定) → ⑤手感调节 (双列分组) →
//! ⑥屏蔽选项 → ⑦只听指定游戏 → ⑧高级 (折叠) → ⑨说明。
//!
//! 设计口径 (用户验收要求: **所有设置必须看得懂**): 每个滑条都配一句"调大/调小会怎样"
//! 的人话提示, 术语 (门槛/gamma/attack) 一律不出现在界面上。
//!
//! 排版口径 (R2.2): 单列长条 → 分组卡片 + 双列网格; 大色块按钮 → 拨动开关;
//! 裸 selectable_label 列表 → 预设宫格 (选中有描边 + 勾); 默认滑条 → 自绘行
//! (图标 + 名称 + 右对齐数值 + 满宽滑轨 + 一句人话)。
//!
//! 数据流: 滑块 → `config.audio_haptics` 镜像 + `AudioHapState` 原子量即时生效 +
//! 脏标 (主循环 1s 去抖写 Config.toml); 引擎遥测 (`telemetry`) → 频谱/电平/判定显示。

use std::ops::RangeInclusive;
use std::sync::atomic::Ordering;
use std::time::Instant;

use eframe::egui;
use egui_phosphor::variants::regular as ic;

use crate::audio_haptics::mapping::{self, PRESETS};
use crate::gui::theme::{SP_L, SP_M, SP_S, SP_XS, Theme};
use crate::gui::{widgets, SorahkGui};
use crate::update::APP_VERSION;
use crate::state::audio_state::{
    audio_status_name, AudioTelemetry, AUDIO_STATUS_OFF, AUDIO_STATUS_RUNNING,
    AUDIO_STATUS_WAIT_GAME, AUDIO_STATUS_WAIT_SOUND,
};

/// 滑条行: 名字/数值写在上一行, 滑轨满宽占下一行, 最后一句人话说明。
/// 返回是否被拖动 (调用方据此写 config + 原子量 + 打脏标)。
macro_rules! audio_slider {
    ($self:ident, $ui:ident, $th:ident, $field:ident, $atom:ident,
     $range:expr, $step:expr, $suffix:expr, $icon:expr, $label:expr, $hint:expr, $tip:expr) => {{
        let mut v = $self.config.audio_haptics.$field;
        let changed = slider_row(
            $ui, $th, $icon, $label, $hint, $tip, &mut v, $range, $step, $suffix,
        );
        if changed {
            $self.config.audio_haptics.$field = v;
            $self.app_state.audio_hap.$atom.store(v, Ordering::Relaxed);
        }
        changed
    }};
}

/// 带一句人话说明的复选框行。
macro_rules! audio_check {
    ($self:ident, $ui:ident, $th:ident, $field:ident, $atom:ident, $label:expr, $hint:expr, $tip:expr) => {{
        let mut v = $self.config.audio_haptics.$field;
        let changed = check_row($ui, $th, &mut v, $label, $hint, $tip);
        if changed {
            $self.config.audio_haptics.$field = v;
            $self.app_state.audio_hap.$atom.store(v, Ordering::Relaxed);
        }
        changed
    }};
}

impl SorahkGui {
    /// ★ AI 智能增强卡片: A 经典 / B 智能 一键切换 + 两条滑条 + 实时远近高低反馈。
    /// 纯算法, 无模型。A 模式 (smart_enabled=false) 输出与旧版逐字节一致。
    fn audio_smart_card(
        &mut self,
        ui: &mut egui::Ui,
        th: &Theme,
        tele: &Option<AudioTelemetry>,
    ) -> bool {
        let mut dirty = false;
        let on = self.config.audio_haptics.smart_enabled;
        th.card(ui, Some("AI 智能增强（远近 · 高低感知）"), |ui| {
            ui.label(th.hint_text(
                "纯算法判断声音的远近与高低：近的声音更猛更脆、收得快；远的声音更轻、拖尾更长。不装模型、不联网。",
            ));
            ui.add_space(SP_S);
            ui.horizontal(|ui| {
                let a = ui.selectable_label(!on, "A 经典模式（原手感，逐字节不变）");
                if a.clicked() && on {
                    self.config.audio_haptics.smart_enabled = false;
                    self.app_state.audio_hap.smart_enabled.store(false, Ordering::Relaxed);
                    dirty = true;
                }
                let b = ui.selectable_label(on, "B AI 智能模式（远近高低自适应）");
                if b.clicked() && !on {
                    self.config.audio_haptics.smart_enabled = true;
                    self.app_state.audio_hap.smart_enabled.store(true, Ordering::Relaxed);
                    dirty = true;
                }
            });
            ui.add_space(SP_XS);
            dirty |= audio_check!(
                self, ui, th, smart_enabled, smart_enabled,
                "AI 智能增强开关（与上方 A/B 等价）",
                "关 = A 经典；开 = B 智能",
                "这个开关和上面的 A/B 是同一个设置。关掉后输出和老版本完全一样，可以放心来回对比。"
            );
            split_cols(ui, 2, SP_L, |ui, i| {
                if i == 0 {
                    dirty |= audio_slider!(
                        self, ui, th, smart_strength_pct, smart_strength, 0..=100u32, 5.0, "%",
                        ic::LIGHTNING, "智能增强强度",
                        "调 0 = 自动回到 A 手感；调大差异更明显",
                        "智能层和经典手感的混合比例。调到 0 就等于完全关掉智能增强（和 A 模式一模一样），想对比效果就拖到 0 再拖回来。"
                    );
                } else {
                    dirty |= audio_slider!(
                        self, ui, th, smart_duration_pct, smart_duration, 0..=100u32, 5.0, "%",
                        ic::WIND, "持续时间影响",
                        "调大：近音更短促、远音余韵更长",
                        "控制智能层对震动持续时间的改写力度。调 0 = 只改强度和左右马达分配，不改收尾快慢。"
                    );
                }
            });
            if let Some(t) = tele {
                if t.smart_active {
                    use crate::audio_haptics::smart::{distance_label, pitch_label};
                    let line = format!(
                        "实时判定：距离 {}（{:.2}） · 音高 {}（{:.2}） · 预计持续 {:.0} ms · 左/右马达 {:.0}% / {:.0}%",
                        distance_label(t.smart_proximity), t.smart_proximity,
                        pitch_label(t.smart_pitch), t.smart_pitch,
                        t.smart_duration_ms, t.level_l * 100.0, t.level_r * 100.0
                    );
                    ui.label(th.hint_text(line));
                } else {
                    ui.label(th.hint_text("当前 A 经典模式 / 静音中 —— 智能层未参与输出。"));
                }
            }
        });
        dirty
    }

    /// 内置预设 → 自定义预设条目 (拷贝 21 个引擎字段)。
    fn builtin_preset_entry(p: &mapping::Preset) -> crate::config::AudioPresetConfig {
        crate::config::AudioPresetConfig {
            name: p.name.to_string(),
            user_modified: false,
            master_gain_pct: p.master_gain_pct,
            low_gain_pct: p.low_gain_pct,
            high_gain_pct: p.high_gain_pct,
            punch_pct: p.punch_pct,
            mask_threshold: p.mask_threshold,
            adaptive_threshold: p.adaptive_threshold,
            auto_profile: p.auto_profile,
            music_mask_pct: p.music_mask_pct,
            quiet_mask: p.quiet_mask,
            rhythm_pct: p.rhythm_pct,
            texture_pct: p.texture_pct,
            attack_pct: p.attack_pct,
            release_pct: p.release_pct,
            dead_zone_pct: p.dead_zone_pct,
            map_max: p.map_max,
            dual_motor: p.dual_motor,
            vocal_only: p.vocal_only,
            bass_high_hz: p.bass_high_hz,
            vocal_low_hz: p.vocal_low_hz,
            vocal_high_hz: p.vocal_high_hz,
        }
    }

    /// 当前 config 里的引擎参数 → 自定义预设条目 (「保存为」快照用)。
    fn snapshot_config_entry(&self, name: &str) -> crate::config::AudioPresetConfig {
        let c = &self.config.audio_haptics;
        crate::config::AudioPresetConfig {
            name: name.to_string(),
            user_modified: true,
            master_gain_pct: c.master_gain_pct,
            low_gain_pct: c.low_gain_pct,
            high_gain_pct: c.high_gain_pct,
            punch_pct: c.punch_pct,
            mask_threshold: c.mask_threshold,
            adaptive_threshold: c.adaptive_threshold,
            auto_profile: c.auto_profile,
            music_mask_pct: c.music_mask_pct,
            quiet_mask: c.quiet_mask,
            rhythm_pct: c.rhythm_pct,
            texture_pct: c.texture_pct,
            attack_pct: c.attack_pct,
            release_pct: c.release_pct,
            dead_zone_pct: c.dead_zone_pct,
            map_max: c.map_max,
            dual_motor: c.dual_motor,
            vocal_only: c.vocal_only,
            bass_high_hz: c.bass_high_hz,
            vocal_low_hz: c.vocal_low_hz,
            vocal_high_hz: c.vocal_high_hz,
        }
    }

    /// 把预设条目写进 config 引擎字段 (不改 enabled / 白名单), 即时生效 + 落盘。
    fn apply_preset_entry(&mut self, e: &crate::config::AudioPresetConfig) {
        let c = &mut self.config.audio_haptics;
        c.preset = e.name.clone();
        c.master_gain_pct = e.master_gain_pct;
        c.low_gain_pct = e.low_gain_pct;
        c.high_gain_pct = e.high_gain_pct;
        c.punch_pct = e.punch_pct;
        c.mask_threshold = e.mask_threshold;
        c.adaptive_threshold = e.adaptive_threshold;
        c.auto_profile = e.auto_profile;
        c.music_mask_pct = e.music_mask_pct;
        c.quiet_mask = e.quiet_mask;
        c.rhythm_pct = e.rhythm_pct;
        c.texture_pct = e.texture_pct;
        c.attack_pct = e.attack_pct;
        c.release_pct = e.release_pct;
        c.dead_zone_pct = e.dead_zone_pct;
        c.map_max = e.map_max;
        c.dual_motor = e.dual_motor;
        c.vocal_only = e.vocal_only;
        c.bass_high_hz = e.bass_high_hz;
        c.vocal_low_hz = e.vocal_low_hz;
        c.vocal_high_hz = e.vocal_high_hz;
        self.app_state.audio_hap.apply_config(c);
        let _ = self.config.save_to_file("Config.toml");
    }

    /// ★v24.61 应用预设: **同名自定义优先**, 否则内置 (对齐震动区
    /// pick_preset_for_apply 语义 —— 用户覆盖过同名预设就用用户的)。
    fn apply_audio_preset(&mut self, name: &str) -> bool {
        if let Some(e) = self
            .config
            .audio_haptics
            .presets
            .iter()
            .find(|p| p.name == name)
            .cloned()
        {
            self.apply_preset_entry(&e);
            return true;
        }
        match mapping::preset_by_name(name) {
            Some(p) => {
                let e = Self::builtin_preset_entry(p);
                self.apply_preset_entry(&e);
                true
            }
            None => false,
        }
    }

    /* ══════════════════════════════ 页面主体 ══════════════════════════════ */

    pub(super) fn render_audio_haptics_page(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let th = self.theme();
        let enabled = self.config.audio_haptics.enabled;
        let status = self.app_state.audio_hap.status.load(Ordering::Relaxed);
        let tele: Option<AudioTelemetry> = self
            .app_state
            .audio_hap
            .telemetry
            .try_lock()
            .ok()
            .map(|t| t.clone());

        let mut dirty = false;
        // ① 开不开
        dirty |= self.audio_hero(ui, &th, &tele, status, enabled);
        // ①.5 AI 智能增强 (A 经典 / B 智能) + 实时远近高低反馈
        dirty |= self.audio_smart_card(ui, &th, &tele);
        dirty |= self.audio_calib_strip(ui, &th);
        // ② 听谁的声音（白名单）: 决定"这个功能对哪个游戏生效", 排在预设之前
        dirty |= self.audio_whitelist_card(ui, &th, &tele);
        // ③ 什么场景
        dirty |= self.audio_presets_card(ui, &th);
        // ④ 预设管理 (我的预设 / 备份分享) 紧跟宫格, 并排两块
        split_cols(ui, 2, SP_L, |ui, i| {
            if i == 0 {
                dirty |= self.audio_user_presets_card(ui, &th);
            } else {
                dirty |= self.audio_backup_card(ui, &th);
            }
        });
        // ⑤ 现在震得怎么样
        self.audio_monitor_card(ui, &th, &tele, enabled);
        // ⑥ 细调 → ⑦ 屏蔽 → ⑧ 高级 → ⑨ 说明
        dirty |= self.audio_tuning_card(ui, &th);
        dirty |= self.audio_mask_card(ui, &th);
        dirty |= self.audio_advanced_card(ui, &th);
        self.audio_notes_card(ui, &th);

        if dirty {
            dirty_mark(self);
        }
        let _ = ctx; // 状态点呼吸动画已负责请求重绘
    }

    /// ① 状态头: 图标 + 称呼 + 状态点 + 总开关 + 快捷自检/校准。
    fn audio_hero(
        &mut self,
        ui: &mut egui::Ui,
        th: &Theme,
        tele: &Option<AudioTelemetry>,
        status: u8,
        enabled: bool,
    ) -> bool {
        let mut toggled = None;
        th.card(ui, None, |ui| {
            ui.horizontal(|ui| {
                // 图标片 (强调色柔底 + 矢量声波)
                let (r, _) = ui.allocate_exact_size(egui::vec2(44.0, 44.0), egui::Sense::hover());
                ui.painter().rect_filled(r, 12.0, th.accent_soft);
                widgets::Icon::SoundWave.paint(ui.painter(), r.shrink(11.0), th.accent);
                ui.add_space(SP_S);

                ui.vertical(|ui| {
                    ui.label(th.h2(if enabled { "音频震动已开启" } else { "音频震动已关闭" }));
                    ui.add_space(2.0);
                    ui.label(th.hint_text("独立开关：关掉它，连发与 DLL 震动完全不受影响"));
                });

                // 右侧: 开关 (从右往左排, 先放开关 = 最右)
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let mut on = enabled;
                    if widgets::switch(ui, th, &mut on).changed() {
                        toggled = Some(on);
                    }
                    ui.add_space(SP_XS);
                    ui.label(th.hint_text(if enabled { "已开启" } else { "已关闭" }));
                });
            });

            ui.add_space(SP_M);
            hairline(ui, th);
            ui.add_space(SP_S);

            /* 状态行: 呼吸点 + 状态 + 当前判定 + 快捷动作 */
            ui.horizontal(|ui| {
                let (dot, pulsing) = if !enabled {
                    (ui.visuals().weak_text_color(), false)
                } else {
                    match status {
                        AUDIO_STATUS_RUNNING => (th.good, true),
                        AUDIO_STATUS_WAIT_GAME | AUDIO_STATUS_WAIT_SOUND => (th.warn, false),
                        _ => (ui.visuals().weak_text_color(), false),
                    }
                };
                widgets::status_dot(ui, dot, pulsing, 4.5);
                let text = if !enabled {
                    "音频震动未开启".to_string()
                } else if status == AUDIO_STATUS_OFF {
                    // 开关刚打开、引擎还没接上声音: 说"已关闭"会和上方标题自相矛盾
                    "启动中（等引擎接上声音）".to_string()
                } else {
                    match tele.as_ref() {
                        Some(t) => format!("{} · {}", audio_status_name(status), mapping::mode_name(t.mode)),
                        None => audio_status_name(status).to_string(),
                    }
                };
                ui.label(egui::RichText::new(text).size(13.0).strong().color(th.heading));
                if enabled {
                    ui.label(th.hint_text("· 马达跟着声音实时输出"));
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add(th.secondary_button("屏蔽校准 (3 秒)"))
                        .on_hover_text(
                            "点一下，然后保持当前环境安静 3 秒（别放游戏音效）。\n软件会自动测出背景噪音有多大，把屏蔽门槛调到刚好——换个新环境（新电脑/新耳机）时建议重新校准一次。",
                        )
                        .clicked()
                    {
                        self.app_state.audio_hap.request_calibration();
                    }
                    if ui
                        .add(th.secondary_button("马达自检 (0.5 秒)"))
                        .on_hover_text(
                            "不用放声音，直接让手柄震一下。\n震了 = 手柄那条线没问题；不震 = 检查手柄连接；震了但放声音时不震 = 检查上面的开关和声音设置。",
                        )
                        .clicked()
                    {
                        self.app_state.audio_hap.request_self_check();
                    }
                });
            });
        });

        if let Some(on) = toggled {
            self.config.audio_haptics.enabled = on;
            let _ = self.config.save_to_file("Config.toml");
            let _ = self.app_state.reload_config(self.config.clone());
            return true;
        }
        false
    }

    /// ② 校准建议条 (引擎给出建议门槛, 一键应用)。无建议时不出现在版面上。
    fn audio_calib_strip(&mut self, ui: &mut egui::Ui, th: &Theme) -> bool {
        let calib = self.app_state.audio_hap.calib_result.load(Ordering::Relaxed);
        if calib == 0 {
            return false;
        }
        let mut applied = false;
        egui::Frame::NONE
            .fill(th.warn_soft)
            .stroke(egui::Stroke::new(1.0, th.warn.gamma_multiply(0.45)))
            .corner_radius(egui::CornerRadius::same(10))
            .inner_margin(egui::Margin::symmetric(14, 10))
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(ic::TARGET).size(15.0).color(th.warn));
                    ui.label(
                        egui::RichText::new(format!("校准完成：当前环境噪音已测出，屏蔽门槛建议设为 {calib}"))
                            .size(13.0)
                            .color(th.heading),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.add(th.secondary_button("忽略")).clicked() {
                            self.app_state.audio_hap.calib_result.store(0, Ordering::Relaxed);
                        }
                        if ui.add(th.primary_button("应用")).clicked() {
                            self.config.audio_haptics.mask_threshold = calib;
                            self.app_state.audio_hap.mask_threshold.store(calib, Ordering::Relaxed);
                            self.app_state.audio_hap.calib_result.store(0, Ordering::Relaxed);
                            applied = true;
                        }
                    });
                });
            });
        ui.add_space(SP_M);
        applied
    }

    /// ④ 预设宫格: 6 个场景平铺, 选中有描边 + 勾, 推荐档有星标。
    fn audio_presets_card(&mut self, ui: &mut egui::Ui, th: &Theme) -> bool {
        let current = self.config.audio_haptics.preset.clone();
        let mut clicked: Option<&'static str> = None;

        th.card_with_actions(
            ui,
            Some("预设"),
            |ui| {
                th.badge(ui, &format!("当前：{current}"), th.accent_text, th.accent_soft);
            },
            |ui| {
                ui.label(th.hint_text(
                    "先点一个场景，再到下面「手感调节」按自己的口味微调。实测推荐「人声清晰」——打游戏、看剧情都合适。",
                ));
                ui.add_space(SP_M);

                let total = ui.available_width();
                let gap = SP_S;
                // 6 档预设: 宽松 3×2（描述一行放得下, 不再断词）; 窄屏 2×3
                let cols = if total >= 640.0 {
                    3
                } else {
                    2
                }
                .min(PRESETS.len());
                let tile_w = ((total - gap * (cols as f32 - 1.0)) / cols as f32).max(96.0);

                for start in (0..PRESETS.len()).step_by(cols) {
                    ui.horizontal_top(|ui| {
                        ui.spacing_mut().item_spacing.x = gap;
                        for (i, p) in PRESETS.iter().enumerate().skip(start).take(cols) {
                            let (scene, feel) = preset_copy(p.name);
                            if preset_tile(
                                ui,
                                th,
                                p.name,
                                feel,
                                &format!("{scene} —— {feel}"),
                                i == 0,
                                current == p.name,
                                tile_w,
                            )
                            .clicked()
                            {
                                clicked = Some(p.name);
                            }
                        }
                    });
                    if start + cols < PRESETS.len() {
                        ui.add_space(gap);
                    }
                }
            },
        );

        match clicked {
            Some(name) => self.apply_audio_preset(name),
            None => false,
        }
    }

    /// ⑥ 实时监测: 频谱 + 双马达电平 + 三个统计 + 判定横幅。
    fn audio_monitor_card(
        &mut self,
        ui: &mut egui::Ui,
        th: &Theme,
        tele: &Option<AudioTelemetry>,
        enabled: bool,
    ) {
        th.card(ui, Some("实时监测"), |ui| {
            if !enabled {
                // 空态做成一行, 不占版面 (原来整块空出 ~150px)
                ui.horizontal(|ui| {
                    let (r, _) = ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::hover());
                    widgets::Icon::SoundWave.paint(ui.painter(), r, th.hint);
                    ui.add_space(SP_XS);
                    ui.label(
                        egui::RichText::new("音频震动已关闭")
                            .size(13.5)
                            .strong()
                            .color(th.heading),
                    );
                    ui.add_space(SP_XS);
                    ui.label(th.hint_text(
                        "打开上面的总开关，这里就会显示频谱、两条马达电平和当前判定。",
                    ));
                });
                return;
            }
            let Some(t) = tele else {
                ui.label(th.hint_text("（监测数据被引擎占用，本帧跳过）"));
                return;
            };

            draw_spectrum(ui, th, t);
            ui.add_space(SP_M);

            split_cols(ui, 2, SP_L, |ui, i| {
                if i == 0 {
                    motor_bar(ui, th, "左马达 低频", t.level_l, th.motor_l);
                } else {
                    motor_bar(ui, th, "右马达 高频", t.level_r, th.motor_r);
                }
            });

            ui.add_space(SP_M);
            hairline(ui, th);
            ui.add_space(SP_S);

            let map_max = self.config.audio_haptics.map_max.max(1) as f32;
            split_cols(ui, 3, SP_M, |ui, i| match i {
                0 => mini_stat(ui, th, "背景噪声", &format!("{:.0}%", t.noise_floor / map_max * 100.0), th.text_weak, "没声音时环境有多吵"),
                1 => mini_stat(ui, th, "屏蔽门槛", &format!("{:.0}%", t.threshold_eff / map_max * 100.0), th.accent_text, "比它小的声音一律不震"),
                _ => mini_stat(ui, th, "爆发度", &format!("{:.0}%", t.transient.clamp(0.0, 1.0) * 100.0), th.info, "越大越接近鼓点/重击"),
            });

            ui.add_space(SP_M);
            mode_banner(ui, th, t.mode);
        });
    }

    /// ⑦ 手感调节: 三组滑条, 组内双列。
    fn audio_tuning_card(&mut self, ui: &mut egui::Ui, th: &Theme) -> bool {
        let mut dirty = false;
        th.card_with_actions(
            ui,
            Some("手感调节"),
            |ui| {
                th.badge(ui, "拖动即时生效", th.good, th.good_soft);
            },
            |ui| {
                ui.label(th.hint_text("不确定就先用上面的预设，这里不用动。鼠标悬停在名称上能看到完整说明。"));

                group_head(ui, th, "力度与对比");
                split_cols(ui, 2, SP_L, |ui, i| {
                    if i == 0 {
                        dirty |= audio_slider!(
                            self, ui, th, master_gain_pct, master_gain, 0..=150u32, 1.0, "%",
                            ic::SPEAKER_HIGH, "全局总强度",
                            "调大整体更猛，调小更温柔 · 建议 60-80",
                            "所有震动一起变大/变小。建议 60-80；觉得整体太猛就往下调，觉得没劲就往上调。"
                        );
                    } else {
                        dirty |= audio_slider!(
                            self, ui, th, punch_pct, punch_pct, 0..=200u32, 5.0, "%",
                            ic::LIGHTNING, "打击对比度",
                            "调大只有重击才猛震；调小每一下都有反馈",
                            "100% 是标准手感，不是音量。调大 → 轻攻击变轻、只有大招和重击才猛震；调小 → 每一下攻击都有明显反馈。"
                        );
                    }
                });

                group_head(ui, th, "双马达分频");
                split_cols(ui, 2, SP_L, |ui, i| {
                    if i == 0 {
                        dirty |= audio_slider!(
                            self, ui, th, low_gain_pct, low_gain, 0..=200u32, 5.0, "%",
                            ic::WAVE_TRIANGLE, "低频量感（左马达）",
                            "爆炸、重击、贝斯的「轰」感 · 调 0 = 左马达不震",
                            "爆炸、重击、贝斯这种“轰”的感觉。调 0 = 左马达基本不震。"
                        );
                    } else {
                        dirty |= audio_slider!(
                            self, ui, th, high_gain_pct, high_gain, 0..=200u32, 5.0, "%",
                            ic::WAVE_SINE, "高频量感（右马达）",
                            "枪声、打击、碎裂的「脆」感 · 调 0 = 右马达不震",
                            "枪声、打击、碎裂声这种“脆”的感觉。调 0 = 右马达基本不震。"
                        );
                    }
                });
                split_cols(ui, 2, SP_L, |ui, i| {
                    if i == 0 {
                        dirty |= audio_slider!(
                            self, ui, th, rhythm_pct, rhythm_pct, 0..=100u32, 5.0, "%",
                            ic::METRONOME, "节奏力度（跟鼓点）",
                            "听歌/挂机时跟鼓点震 · 0 = 完全不跟鼓点",
                            "听歌/挂机时跟着鼓点震的强度。0 = 听歌完全不跟鼓点；听歌想要节奏感就调到 40-70。"
                        );
                    }
                });

                group_head(ui, th, "动态与门槛");
                split_cols(ui, 2, SP_L, |ui, i| {
                    if i == 0 {
                        dirty |= audio_slider!(
                            self, ui, th, mask_threshold, mask_threshold, 0..=1200u32, 20.0, "",
                            ic::SHIELD, "背景屏蔽门槛",
                            "调高更安静，调低更灵敏 · 默认 400 · 不确定就用上方「屏蔽校准」",
                            "比这个响度小的声音一律不震。调高 = 更安静（音乐、杂音更不容易震到手）；调低 = 更灵敏。\n默认 400 适合大多数游戏；听歌挂机嫌吵就加到 600-800；嫌震得迟钝就减到 200-300。\n不确定就点上面的「屏蔽校准」，让软件自动测。"
                        );
                    } else {
                        dirty |= audio_slider!(
                            self, ui, th, attack_pct, attack_pct, 5..=90u32, 1.0, "%",
                            ic::GAUGE, "响应速度",
                            "调高声一响立刻震、更跟手 · 打游戏建议 60 以上",
                            "调高 = 声一响手立刻震，更跟手；调低 = 更柔和。打游戏建议 60 以上。"
                        );
                    }
                });
                split_cols(ui, 2, SP_L, |ui, i| {
                    if i == 0 {
                        dirty |= audio_slider!(
                            self, ui, th, release_pct, release_pct, 2..=60u32, 1.0, "%",
                            ic::WIND, "余韵",
                            "调高拖尾更绵长，调低干脆利落",
                            "声音停下后震动收尾多快。调高 = 拖尾长、更绵；调低 = 干脆利落。"
                        );
                    }
                });
            },
        );
        dirty
    }

    /// ⑧ 屏蔽选项: 三个自动开关 + 两条压制滑条 (双列, 左开关右滑条)。
    fn audio_mask_card(&mut self, ui: &mut egui::Ui, th: &Theme) -> bool {
        let mut dirty = false;
        th.card(ui, Some("屏蔽选项"), |ui| {
            ui.label(th.hint_text("防 BGM 和杂音乱震。三个开关都建议开着，剩下两条滑条按口味调。"));
            ui.add_space(SP_M);
            split_cols(ui, 2, SP_L, |ui, i| {
                if i == 0 {
                    dirty |= audio_check!(
                        self, ui, th, auto_profile, auto_profile,
                        "智能识别（自动分辨游戏 / 音乐 / 安静）",
                        "放 BGM 时压住连续震动，只留鼓点和真音效",
                        "推荐开着。软件会自动判断现在放的是游戏音效还是背景音乐：\n放 BGM 时压住连续震动只留鼓点和真音效，安静时手柄完全不动。\n嫌它误判就关掉，关掉后完全按下面的滑条来。"
                    );
                    ui.add_space(SP_S);
                    dirty |= audio_check!(
                        self, ui, th, adaptive_threshold, adaptive_threshold,
                        "自动屏蔽背景噪音",
                        "背景越吵，门槛自动抬得越高",
                        "推荐开着。软件会一直听着背景噪音有多大，自动把门槛抬高到噪音之上。\n背景越吵门槛越高，BGM 和环境音就不容易震到手。"
                    );
                    ui.add_space(SP_S);
                    dirty |= audio_check!(
                        self, ui, th, quiet_mask, quiet_mask,
                        "安静时手柄完全不动",
                        "挂机 / 读图 / 大厅安静时保持零震动",
                        "推荐开着。没有游戏音效时（挂机/读图/大厅安静下来）手柄保持零震动。\n关掉 = 安静时也保留一半的轻微跟随。"
                    );
                } else {
                    dirty |= audio_slider!(
                        self, ui, th, music_mask_pct, music_mask_pct, 0..=100u32, 5.0, "%",
                        ic::MUSIC_NOTES, "音乐段压制强度",
                        "识别到 BGM 时额外压掉多少震动",
                        "识别到背景音乐时，连续震动额外压掉多少。默认 60%；\n嫌听歌时手还是一直震就调到 80-100；嫌压制太狠（BGM 里的大招不震了）就调低。"
                    );
                    ui.add_space(SP_XS);
                    dirty |= audio_slider!(
                        self, ui, th, texture_pct, texture_pct, 0..=100u32, 5.0, "%",
                        ic::SPARKLE, "细腻麻感",
                        "听人声 / ASMR 的微颤质感 · 打游戏建议 0-30",
                        "听人声/ASMR/轻声内容时的微弱麻颤质感。不打游戏只听声音的话可以调高一点；打游戏建议 0-30。"
                    );
                }
            });
        });
        dirty
    }

    /// ③ 听哪些声音 (音频专属白名单): 决定这个功能对谁生效 —— 开关在卡头, 命中情况右侧实时反馈。
    fn audio_whitelist_card(
        &mut self,
        ui: &mut egui::Ui,
        th: &Theme,
        tele: &Option<AudioTelemetry>,
    ) -> bool {
        let mut dirty = false;
        let on = self.config.audio_haptics.whitelist_enabled;
        let mut toggle: Option<bool> = None;

        th.card_with_actions(
            ui,
            Some("听哪些声音"),
            |ui| {
                // 标题行本身是 right_to_left, 先放开关 (最右), 再放说明
                let mut v = on;
                if widgets::switch(ui, th, &mut v).changed() {
                    toggle = Some(v);
                }
                ui.add_space(SP_XS);
                ui.label(th.hint_text(if on { "只听名单里的" } else { "听整个电脑" }));
            },
            |ui| {
                ui.label(th.hint_text(if on {
                    "只在名单里的应用跑到前台时才读声音：QQ 音乐、网页视频、语音通话的声音一律不理。"
                } else {
                    "现在听整个电脑的声音（音乐、视频、语音全算）。只想对某个游戏生效，就打开右上角的开关。"
                }));

                // 条目来源: 默认复用连发白名单 (玩家只维护一份)
                let src_macro = !self
                    .config
                    .audio_haptics
                    .whitelist_source
                    .eq_ignore_ascii_case("custom");
                ui.add_space(if on { SP_M } else { SP_S });
                ui.horizontal(|ui| {
                    ui.label(th.hint_text("名单用"));
                    if ui
                        .selectable_label(src_macro, egui::RichText::new("连发白名单那份").size(12.5))
                        .on_hover_text("与「连发映射」页的白名单共用同一份列表（只读，不会改它）。推荐。")
                        .clicked()
                        && !src_macro
                    {
                        self.config.audio_haptics.whitelist_source = "macro".to_string();
                        dirty = true;
                    }
                    if ui
                        .selectable_label(!src_macro, egui::RichText::new("单独填一份").size(12.5))
                        .on_hover_text("音频震动单独一份名单，和连发白名单互不影响。")
                        .clicked()
                        && src_macro
                    {
                        self.config.audio_haptics.whitelist_source = "custom".to_string();
                        dirty = true;
                    }

                    // 右侧: 命中判定 (开名单后"到底有没有生效"一眼可见)
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        match (on, tele.as_ref()) {
                            (false, _) => {
                                th.badge(ui, "未启用 · 全局听", th.text_weak, th.card_alt);
                            }
                            (true, Some(t)) if t.wl_matched => {
                                th.badge(ui, "命中 · 正在读这个游戏", th.good, th.good_soft);
                            }
                            (true, Some(t)) if t.wl_entries == 0 => {
                                th.badge(ui, "名单是空的", th.warn, th.warn_soft);
                            }
                            (true, Some(_)) => {
                                th.badge(ui, "拦截中 · 前台不在名单", th.warn, th.warn_soft);
                            }
                            (true, None) => {}
                        }
                    });
                });

                // 单独填一份时才给输入框; 复用连发那份就不用重复维护
                if !src_macro {
                    ui.add_space(SP_S);
                    let r = th.text_input(
                        ui,
                        &mut self.audio_whitelist_edit,
                        "DNF.exe, E:\\Games\\DFO.exe（逗号分隔，可填完整路径）",
                        ui.available_width().max(160.0),
                        egui::Id::new("audio_whitelist_input"),
                    );
                    if r.changed() {
                        self.config.audio_haptics.whitelist_processes = self
                            .audio_whitelist_edit
                            .split([',', '\n'])
                            .map(|s| s.trim().to_string())
                            .filter(|s| !s.is_empty())
                            .collect();
                        dirty = true;
                    }
                }

                // 细节行: 来源 / 条目 / 前台 (只有开了名单才有意义)
                if on {
                    if let Some(t) = tele {
                        ui.add_space(SP_S);
                        ui.label(th.hint_text(format!(
                            "来源 {} · 条目 {} · 前台 {}",
                            if t.wl_source.is_empty() { "-" } else { &t.wl_source },
                            t.wl_entries,
                            if t.wl_foreground.is_empty() {
                                "(未取到)"
                            } else {
                                &t.wl_foreground
                            },
                        )));
                        // 下一步该干什么, 直接说, 别让玩家自己猜
                        if t.wl_entries == 0 {
                            ui.label(
                                egui::RichText::new(
                                    "名单还是空的：去左边「白名单修改」加一个游戏，或上面切到「单独填一份」直接填进程名。",
                                )
                                .color(th.warn)
                                .size(11.5),
                            );
                        } else if !t.wl_matched {
                            ui.label(th.hint_text("把游戏切到前台，这里就会变成「命中」。"));
                        }
                        if !t.wl_error.is_empty() {
                            ui.label(
                                egui::RichText::new(format!("进程信息获取提示：{}", t.wl_error))
                                    .color(th.warn)
                                    .size(11.5),
                            );
                        }
                        if t.wl_degraded {
                            ui.label(
                                egui::RichText::new(
                                    "按进程精确监听不可用，已降级为听整个设备（仍按前台判断）。\n游戏以管理员运行时，请也以管理员身份启动本工具。",
                                )
                                .color(th.warn)
                                .size(11.5),
                            );
                        }
                    }
                }

                // 监听方式属于排障项, 收起, 不占版面
                ui.add_space(SP_S);
                egui::CollapsingHeader::new(
                    egui::RichText::new("监听方式（默认就够用）")
                        .size(12.0)
                        .color(th.text_weak),
                )
                .id_salt("audio_wl_advanced")
                .show(ui, |ui| {
                    ui.add_space(SP_XS);
                    let mut process_mode = self
                        .config
                        .audio_haptics
                        .whitelist_mode
                        .eq_ignore_ascii_case("process");
                    if ui
                        .checkbox(&mut process_mode, "只监听名单进程的声音（Win10 2004+）")
                        .on_hover_text(
                            "开 = 只收名单里那个游戏的声音，别的程序的声音根本不进采集。\n关 = 只要前台在名单里就听整个设备的声音（兼容旧系统）。",
                        )
                        .changed()
                    {
                        self.config.audio_haptics.whitelist_mode = if process_mode {
                            "process".to_string()
                        } else {
                            "foreground".to_string()
                        };
                        dirty = true;
                    }
                    let mut tree = self.config.audio_haptics.whitelist_include_tree;
                    if ui
                        .checkbox(&mut tree, "含子进程")
                        .on_hover_text("游戏启动器拉起的子进程（登录器/反作弊）也一起算。")
                        .changed()
                    {
                        self.config.audio_haptics.whitelist_include_tree = tree;
                        dirty = true;
                    }
                });
            },
        );

        if let Some(v) = toggle {
            self.config.audio_haptics.whitelist_enabled = v;
            // 老字段只作遗留别名, 不再驱动行为
            self.config.audio_haptics.whitelist_filter = false;
            dirty = true;
        }
        dirty
    }

    /// ⑨ 高级选项 (默认折叠, 默认值即最佳)。
    fn audio_advanced_card(&mut self, ui: &mut egui::Ui, th: &Theme) -> bool {
        let mut dirty = false;
        th.card(ui, None, |ui| {
            egui::CollapsingHeader::new(
                egui::RichText::new("高级选项（默认值就够用，不建议乱调）").size(13.0).color(th.text_weak),
            )
            .id_salt("audio_advanced")
            .show(ui, |ui| {
                ui.add_space(SP_S);
                th.panel(ui, Some("马达与频带"), |ui| {
                    dirty |= audio_check!(
                        self, ui, th, dual_motor, dual_motor,
                        "双马达分频（低频走左、高频走右）",
                        "左马达管「轰」，右马达管「脆」，层次更分明",
                        "推荐开。左马达管“轰”（爆炸/贝斯），右马达管“脆”（枪声/打击），层次更分明。\n关掉 = 两颗马达一起同步震，感觉更“整”，但少了层次。"
                    );
                    ui.add_space(SP_XS);
                    dirty |= audio_check!(
                        self, ui, th, vocal_only, vocal_only,
                        "只跟人声频段（双马达关闭时生效）",
                        "只跟人声能量，音乐/环境音基本不震 · 适合看剧听书",
                        "开了以后只跟人声频段的能量，音乐/环境音基本不震。适合看剧/听书。"
                    );
                    ui.add_space(SP_S);
                    split_cols(ui, 2, SP_L, |ui, i| match i {
                        0 => dirty |= audio_slider!(
                            self, ui, th, bass_high_hz, bass_high_hz, 200..=1000u32, 10.0, " Hz",
                            ic::WAVE_TRIANGLE, "低频上限",
                            "低于这个频率算「低频」走左马达 · 默认 750",
                            "低于这个频率的声音算“低频”走左马达。默认 750；觉得低频抢戏就调低，觉得轰得不够就调高。"
                        ),
                        1 => dirty |= audio_slider!(
                            self, ui, th, vocal_low_hz, vocal_low_hz, 300..=1500u32, 10.0, " Hz",
                            ic::MICROPHONE, "人声下限",
                            "人声频段的下边界 · 默认 600，一般不用动",
                            "人声频段的下边界，默认 600。一般不用动。"
                        ),
                        _ => {}
                    });
                    split_cols(ui, 2, SP_L, |ui, i| {
                        if i == 0 {
                            dirty |= audio_slider!(
                                self, ui, th, vocal_high_hz, vocal_high_hz, 1500..=8000u32, 50.0, " Hz",
                                ic::MICROPHONE, "人声上限",
                                "人声频段的上边界 · 默认 4200，一般不用动",
                                "人声频段的上边界，默认 4200。一般不用动。"
                            );
                        }
                    });
                });

                ui.add_space(SP_S);
                th.panel(ui, Some("响度标尺"), |ui| {
                    split_cols(ui, 2, SP_L, |ui, i| match i {
                        0 => dirty |= audio_slider!(
                            self, ui, th, map_max, map_max, 1000..=12000u32, 100.0, "",
                            ic::GAUGE, "满格响度",
                            "多响算震满格 · 默认 6000 · 门槛决定震不震，这条决定震多满",
                            "声音响到什么程度算“震满格”。默认 6000；觉得动静大了调高，觉得不够劲调低。\n（配合「背景屏蔽门槛」用：门槛决定“震不震”，这条决定“震多满”。）"
                        ),
                        1 => dirty |= audio_slider!(
                            self, ui, th, dead_zone_pct, dead_zone_pct, 0..=20u32, 1.0, "%",
                            ic::ARROW_COUNTER_CLOCKWISE, "震动死区",
                            "低于这个强度直接归零，消除马达底噪 · 默认 4",
                            "低于这个强度的输出直接归零，消除马达细微的“嗡嗡”底噪。默认 4；嫌轻微动静太黏就加到 6-8。"
                        ),
                        _ => {}
                    });
                });
            });
        });
        dirty
    }

    /// ⑩ 说明 (预期管理: 六条, 双列排布)。
    fn audio_notes_card(&mut self, ui: &mut egui::Ui, th: &Theme) {
        const NOTES: [&str; 6] = [
            "本引擎是纯算法（频带分析 + 自动门槛），不需要下载任何模型文件。",
            "Xbox 手柄只有 2 颗马达（低频 + 高频），震动层次受马达物理限制，不是软件问题。",
            "与 DLL 震动同时开启时两者取最大值合并（互不覆盖）；DNF 内建议只用 DLL 模式（受击更精准），音频模式适合免注入场景与其他游戏。",
            "马达保护：持续高输出 10 秒自动限幅 60% 持续 5 秒（防马达过热）。",
            "换了耳机/音箱/环境之后，建议重新点一次「屏蔽校准」。",
            "白名单条目默认直接复用连发白名单；关掉音频震动时，连发/组合键/DLL 注入行为与旧版完全一致。",
        ];
        th.card(ui, Some("说明"), |ui| {
            split_cols(ui, 2, SP_L, |ui, i| {
                let range = if i == 0 { 0..3 } else { 3..6 };
                for n in range {
                    ui.horizontal_top(|ui| {
                        let (r, _) = ui.allocate_exact_size(egui::vec2(14.0, 15.0), egui::Sense::hover());
                        ui.painter().circle_filled(r.center() + egui::vec2(0.0, 1.0), 2.0, th.accent.gamma_multiply(0.75));
                        ui.add(
                            egui::Label::new(egui::RichText::new(NOTES[n]).size(11.5).color(th.hint))
                                .wrap(),
                        );
                    });
                    ui.add_space(SP_XS);
                }
            });
        });
    }

    /// ⑤ 我的预设 + 备份分享 (紧跟预设宫格, 归为"预设管理")。
    /// 拆成独立卡片, 避免宫格卡片过高。
    fn audio_user_presets_card(&mut self, ui: &mut egui::Ui, th: &Theme) -> bool {
        let mut dirty = false;
        let mut to_apply: Option<usize> = None;
        let mut to_delete: Option<usize> = None;
        let mut to_select: Option<usize> = None;
        let current = self.config.audio_haptics.preset.clone();
        let sel = self.audio_user_sel;

        th.card(ui, Some("我的预设"), |ui| {
            if self.config.audio_haptics.presets.is_empty() {
                ui.label(th.hint_text(
                    "还没有自定义预设。调好上面的滑条后，在这一行起个名字保存 —— 以后一键切回来。",
                ));
            } else {
                for (i, e) in self.config.audio_haptics.presets.iter().enumerate() {
                    ui.horizontal(|ui| {
                        let is_cur = e.name == current;
                        if ui
                            .selectable_label(i == sel, egui::RichText::new(&e.name).size(13.0))
                            .clicked()
                        {
                            to_select = Some(i);
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.add(th.secondary_button("删除")).clicked() {
                                to_delete = Some(i);
                            }
                            if ui.add(th.secondary_button("应用")).clicked() {
                                to_apply = Some(i);
                            }
                            if is_cur {
                                th.badge(ui, "使用中", th.good, th.good_soft);
                            }
                        });
                    });
                }
            }

            ui.add_space(SP_M);
            hairline(ui, th);
            ui.add_space(SP_S);

            ui.horizontal(|ui| {
                ui.label(th.weak("保存当前设置为"));
                th.text_input(
                    ui,
                    &mut self.audio_preset_name,
                    "给这份设置起个名字",
                    180.0,
                    egui::Id::new("audio_preset_name_input"),
                );
                let name = self.audio_preset_name.trim().to_string();
                let name_ok = !name.is_empty() && !mapping::is_protected_preset(&name);
                let btn = if name_ok {
                    th.primary_button("保存")
                } else {
                    th.secondary_button("保存")
                };
                if ui
                    .add_enabled(name_ok, btn)
                    .on_hover_text(if name_ok {
                        "把当前所有滑条的值存成一个预设（同名直接覆盖）"
                    } else if name.is_empty() {
                        "先起个名字"
                    } else {
                        "这个名字和内置预设重名了，换一个（内置预设永远可用，不能被覆盖）"
                    })
                    .clicked()
                {
                    let entry = self.snapshot_config_entry(&name);
                    // 同名替换而非追加 (对齐震动区: 重名项永远删不掉的老坑)
                    match self
                        .config
                        .audio_haptics
                        .presets
                        .iter()
                        .position(|x| x.name == name)
                    {
                        Some(pos) => self.config.audio_haptics.presets[pos] = entry,
                        None => self.config.audio_haptics.presets.push(entry),
                    }
                    self.audio_preset_name.clear();
                    self.audio_export_msg = Some(format!("已保存预设「{name}」"));
                    dirty = true;
                }
                if !name_ok && !self.audio_preset_name.trim().is_empty() {
                    ui.label(
                        egui::RichText::new("与内置预设重名")
                            .size(11.5)
                            .color(th.warn),
                    );
                }
            });
        });

        if let Some(i) = to_select {
            self.audio_user_sel = i;
        }
        if let Some(i) = to_apply {
            if let Some(e) = self.config.audio_haptics.presets.get(i).cloned() {
                self.apply_preset_entry(&e);
                self.audio_user_sel = i;
                dirty = true;
            }
        }
        if let Some(i) = to_delete {
            if i < self.config.audio_haptics.presets.len() {
                self.config.audio_haptics.presets.remove(i);
                self.audio_user_sel = 0;
                dirty = true;
            }
        }
        dirty
    }

    /// ⑪ 备份与分享: 导出 / 导入 / 还原默认。
    fn audio_backup_card(&mut self, ui: &mut egui::Ui, th: &Theme) -> bool {
        let mut dirty = false;
        th.card(ui, Some("备份与分享"), |ui| {
            ui.label(th.hint_text("导出的 .toml 存在程序目录，可以发给别人或换电脑用。"));
            ui.add_space(SP_M);

            ui.horizontal(|ui| {
                if ui
                    .add(th.secondary_button("导出当前设置"))
                    .on_hover_text("把当前所有音频震动参数导出成一个 .toml 文件（存在程序目录），可以发给别人或换电脑用")
                    .clicked()
                {
                    let entry = self.snapshot_config_entry(&self.config.audio_haptics.preset.clone());
                    match toml::to_string_pretty(&entry) {
                        Ok(body) => {
                            let content = format!(
                                "# ═══════════════════════════════════════════════════════\n\
                                 #  Sorahk 音频震动设定导出 (v{APP_VERSION})\n\
                                 #  导入: 音频震动页 → 我的预设 → 导入\n\
                                 # ═══════════════════════════════════════════════════════\n{body}"
                            );
                            let fname = Self::save_export_file(&content, "audio_export");
                            self.audio_export_msg = Some(format!("已导出：{fname}（程序目录）"));
                        }
                        Err(e) => self.audio_export_msg = Some(format!("导出失败：{e}")),
                    }
                }

                let files = crate::job_presets::list_export_files_for_route("audio_export", false);
                if !files.is_empty() {
                    let sel = self.audio_import_sel.min(files.len() - 1);
                    egui::ComboBox::from_id_salt("audio_import_sel")
                        .selected_text(egui::RichText::new(&files[sel]).size(12.5))
                        .width(190.0)
                        .show_ui(ui, |ui| {
                            for (i, f) in files.iter().enumerate() {
                                if ui.selectable_label(i == sel, f).clicked() {
                                    self.audio_import_sel = i;
                                }
                            }
                        });
                    if ui
                        .add(th.secondary_button("导入"))
                        .on_hover_text("从导出文件读取预设并应用（名字与内置重名的会被拒绝）")
                        .clicked()
                    {
                        let f = &files[sel];
                        match std::fs::read_to_string(format!("{f}"))
                            .ok()
                            .and_then(|s| toml::from_str::<crate::config::AudioPresetConfig>(&s).ok())
                        {
                            Some(mut e)
                                if !e.name.trim().is_empty()
                                    && !mapping::is_protected_preset(e.name.trim()) =>
                            {
                                e.name = e.name.trim().to_string();
                                let name = e.name.clone();
                                match self
                                    .config
                                    .audio_haptics
                                    .presets
                                    .iter()
                                    .position(|x| x.name == name)
                                {
                                    Some(pos) => self.config.audio_haptics.presets[pos] = e.clone(),
                                    None => self.config.audio_haptics.presets.push(e.clone()),
                                }
                                self.apply_preset_entry(&e);
                                dirty = true;
                                self.audio_export_msg = Some(format!("已导入并应用：{name}（{f}）"));
                            }
                            Some(e) if mapping::is_protected_preset(e.name.trim()) => {
                                self.audio_export_msg = Some(format!(
                                    "导入失败：「{}」与内置预设重名，请改名字后重新导出",
                                    e.name.trim()
                                ));
                            }
                            _ => {
                                self.audio_export_msg =
                                    Some(format!("{f} 解析失败（文件损坏或不是音频震动导出）"));
                            }
                        }
                    }
                }
            });

            ui.add_space(SP_M);
            hairline(ui, th);
            ui.add_space(SP_S);

            ui.horizontal(|ui| {
                if ui
                    .add(th.secondary_button("还原默认"))
                    .on_hover_text("清空我的全部自定义预设，并回到推荐档「人声清晰」")
                    .clicked()
                {
                    self.config.audio_haptics.presets.clear();
                    self.audio_user_sel = 0;
                    self.audio_apply_first_preset();
                    self.audio_export_msg = Some("已清空自定义预设，回到「人声清晰」".to_string());
                    dirty = true;
                }
                ui.label(th.hint_text("会清空「我的预设」，引擎参数回到推荐档"));
            });

            if let Some(m) = &self.audio_export_msg {
                ui.add_space(SP_S);
                let bad = m.contains("失败");
                ui.label(
                    egui::RichText::new(m)
                        .size(11.5)
                        .color(if bad { th.bad } else { th.good }),
                );
            }
        });
        dirty
    }

    /// 还原默认: 应用第一档内置预设 (推荐档), 避免借用冲突单独抽出。
    fn audio_apply_first_preset(&mut self) {
        let name = PRESETS[0].name;
        self.apply_audio_preset(name);
    }
}

/// 打脏标 + 整体镜像 (滑条拖动/预设应用后统一走这里, 主循环 1s 去抖落盘)。
fn dirty_mark(gui: &mut SorahkGui) {
    gui.app_state
        .audio_hap
        .apply_config(&gui.config.audio_haptics);
    gui.audio_params_dirty_since = Some(Instant::now());
}

/* ═══════════════════════════ 版式小工具 (本页专用) ═══════════════════════════ */

/// 把当前行等宽切成 n 列, 逐列回调 (窄屏自动变窄, 由调用方决定是否回落单列)。
///
/// ⚠ 列内必须用 `ui.vertical` 起子 Ui: `Frame::show` 会继承父 Ui 的布局, 直接把卡片
/// 塞进 `horizontal_top` 会让卡片内容也横向排 (标题和正文挤成一行、卡片撑满整行)。
fn split_cols(ui: &mut egui::Ui, n: usize, gap: f32, mut add: impl FnMut(&mut egui::Ui, usize)) {
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

/// 1px 分隔线 (占满当前宽度, 上下各留 1px 视觉呼吸)。
fn hairline(ui: &mut egui::Ui, th: &Theme) {
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), 1.0),
        egui::Sense::hover(),
    );
    ui.painter()
        .hline(rect.x_range(), rect.center().y, egui::Stroke::new(1.0, th.stroke));
}

/// 组内小标题 + 右侧延伸细线 (分组感靠这条线, 不靠色块)。
fn group_head(ui: &mut egui::Ui, th: &Theme, text: &str) {
    ui.add_space(SP_M);
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

/// 滑条行: 图标 + 名称 + 右对齐数值 / 满宽滑轨 / 一句人话。
#[allow(clippy::too_many_arguments)]
fn slider_row(
    ui: &mut egui::Ui,
    th: &Theme,
    icon: &str,
    label: &str,
    hint: &str,
    tip: &str,
    value: &mut u32,
    range: RangeInclusive<u32>,
    step: f64,
    suffix: &str,
) -> bool {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(icon).size(13.0).color(th.accent));
        ui.label(egui::RichText::new(label).size(13.0).color(th.text));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                egui::RichText::new(format!("{value}{suffix}"))
                    .size(13.0)
                    .strong()
                    .family(Theme::font_bold())
                    .color(th.accent_text),
            );
        });
    });

    // 滑轨吃满整列宽 (数值已在上行自绘, 关掉内置数值文本)
    let old = ui.spacing().slider_width;
    ui.spacing_mut().slider_width = (ui.available_width() - 6.0).max(80.0);
    let resp = ui
        .add(egui::Slider::new(value, range).step_by(step).show_value(false))
        .on_hover_text(tip);
    ui.spacing_mut().slider_width = old;

    if !hint.is_empty() {
        ui.label(egui::RichText::new(hint).size(11.5).color(th.hint));
    }
    ui.add_space(SP_XS);
    resp.changed()
}

/// 复选框行: 勾选 + 一句话说明。
fn check_row(
    ui: &mut egui::Ui,
    th: &Theme,
    value: &mut bool,
    label: &str,
    hint: &str,
    tip: &str,
) -> bool {
    let changed = ui
        .checkbox(value, egui::RichText::new(label).size(13.0))
        .on_hover_text(tip)
        .changed();
    if !hint.is_empty() {
        ui.add_space(1.0);
        ui.label(egui::RichText::new(hint).size(11.5).color(th.hint));
    }
    changed
}

/// 预设宫格单块: 名称 + 一句手感 + (推荐星 / 选中勾), 整块可点。
#[allow(clippy::too_many_arguments)]
fn preset_tile(
    ui: &mut egui::Ui,
    th: &Theme,
    name: &str,
    feel: &str,
    tip: &str,
    recommended: bool,
    selected: bool,
    w: f32,
) -> egui::Response {
    let h = 66.0;
    let (rect, response) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::click());
    let response = response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(tip);
    let hover = ui.ctx().animate_bool_with_time(
        egui::Id::new("audio_preset_tile").with(name),
        response.hovered(),
        0.12,
    );

    let radius = 10.0;
    let p = ui.painter_at(rect);
    p.rect_filled(rect, radius, if selected { th.accent_soft } else { th.card_alt });
    if hover > 0.01 && !selected {
        let a = (hover * if th.dark { 12.0 } else { 7.0 }) as u8;
        p.rect_filled(
            rect,
            radius,
            if th.dark {
                egui::Color32::from_rgba_unmultiplied(255, 255, 255, a)
            } else {
                egui::Color32::from_rgba_unmultiplied(0, 0, 0, a)
            },
        );
    }
    let stroke = if selected {
        egui::Stroke::new(1.4, th.accent)
    } else if hover > 0.5 {
        egui::Stroke::new(1.0, th.stroke_strong)
    } else {
        egui::Stroke::new(1.0, th.stroke)
    };
    p.rect_stroke(rect, radius, stroke, egui::StrokeKind::Inside);

    // 名称
    p.text(
        egui::pos2(rect.left() + 12.0, rect.top() + 11.0),
        egui::Align2::LEFT_TOP,
        name,
        egui::FontId::proportional(13.5),
        if selected { th.accent_text } else { th.heading },
    );

    // 角标: 选中 → 勾; 否则推荐 → 星
    if selected {
        p.text(
            egui::pos2(rect.right() - 10.0, rect.top() + 10.0),
            egui::Align2::RIGHT_TOP,
            ic::CHECK,
            egui::FontId::proportional(13.0),
            th.accent,
        );
    } else if recommended {
        p.text(
            egui::pos2(rect.right() - 10.0, rect.top() + 10.0),
            egui::Align2::RIGHT_TOP,
            ic::STAR,
            egui::FontId::proportional(12.0),
            th.warn,
        );
    }

    // 一句手感 (自动换行)
    let galley = p.layout(
        feel.to_string(),
        egui::FontId::proportional(11.5),
        th.hint,
        (w - 26.0).max(40.0),
    );
    p.galley(egui::pos2(rect.left() + 12.0, rect.top() + 32.0), galley, th.hint);

    if response.has_focus() {
        p.rect_stroke(
            rect.expand(1.5),
            radius + 1.5,
            egui::Stroke::new(2.0, th.accent),
            egui::StrokeKind::Outside,
        );
    }
    response
}

/// 预设名 → (场景一句话, 手感一句话)。宫格用短的, 悬停出长的。
fn preset_copy(name: &str) -> (&'static str, &'static str) {
    match name {
        "人声清晰" => ("打游戏 / 看剧情（实测最佳）", "只跟人声频段，杂音不理"),
        "轻柔细腻" => ("轻声内容（实测次佳）", "细腻微颤，轻声也有反馈"),
        "游戏强劲" => ("打击感拉满", "招招猛震，压制背景音乐"),
        "低频沉浸" => ("看电影 / 大场面", "爆炸低音，轰隆感拉满"),
        "全频强劲" => ("要最猛的裸手感", "所有声音都直接震"),
        _ => ("排障用", "排障用：确认马达正常"),
    }
}

/// 小统计块: 数值 (16 粗) + 标签。
fn mini_stat(ui: &mut egui::Ui, th: &Theme, label: &str, value: &str, color: egui::Color32, tip: &str) {
    ui.vertical(|ui| {
        ui.label(
            egui::RichText::new(value)
                .size(16.0)
                .strong()
                .family(Theme::font_bold())
                .color(color),
        );
        ui.label(th.hint_text(label))
            .on_hover_text(tip);
    });
}

/// 判定横幅: 告诉玩家"现在为什么震 / 为什么不震"。
fn mode_banner(ui: &mut egui::Ui, th: &Theme, mode: u8) {
    let (glyph, text, fg, bg) = match mode {
        mapping::MODE_MUSIC => (
            ic::MUSIC_NOTES,
            "当前判定：背景音乐段 —— 连续震动被压住，只有鼓点（节奏力度）和够响的音效会震",
            th.info,
            egui::Color32::from_rgba_unmultiplied(th.info.r(), th.info.g(), th.info.b(), 30),
        ),
        mapping::MODE_QUIET => (
            ic::SPEAKER_SLASH,
            "当前判定：安静段 —— 手柄保持安静，不会乱震",
            th.warn,
            th.warn_soft,
        ),
        mapping::MODE_VOCAL => (
            ic::MICROPHONE,
            "当前判定：人声/细节段 —— 轻微细腻的反馈，不会吵",
            th.accent_text,
            th.accent_soft,
        ),
        _ => (
            ic::GAME_CONTROLLER,
            "当前判定：游戏音效段 —— 正常跟手震",
            th.good,
            th.good_soft,
        ),
    };
    egui::Frame::NONE
        .fill(bg)
        .corner_radius(egui::CornerRadius::same(8))
        .inner_margin(egui::Margin::symmetric(10, 8))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(glyph).size(14.0).color(fg));
                ui.add(
                    egui::Label::new(egui::RichText::new(text).size(12.5).color(fg)).wrap(),
                );
            });
        });
}

/// 5 频带频谱 (自绘): 底衬 + 网格 + 瞬时能量柱 + 峰值帽线。
fn draw_spectrum(ui: &mut egui::Ui, th: &Theme, t: &AudioTelemetry) {
    let desired = egui::vec2(ui.available_width(), 108.0);
    let (rect, _) = ui.allocate_exact_size(desired, egui::Sense::hover());
    let p = ui.painter_at(rect);
    let radius = 8.0;

    p.rect_filled(rect, radius, th.extreme);
    p.rect_stroke(
        rect,
        radius,
        egui::Stroke::new(1.0, th.stroke),
        egui::StrokeKind::Inside,
    );

    // 网格: 4 等分横线
    for i in 1..4 {
        let y = rect.top() + rect.height() * i as f32 / 4.0;
        p.hline(rect.x_range().shrink(10.0), y, egui::Stroke::new(1.0, th.stroke));
    }

    let label_h = 20.0;
    let base = rect.bottom() - label_h;
    let avail = (base - rect.top() - 12.0).max(8.0);
    let names = ["超低", "低", "中", "高", "超高"];
    let step = rect.width() / 5.0;

    let now_ms = crate::vibration::now_ms_u64();
    let hot = t.last_onset_ms != 0 && now_ms.saturating_sub(t.last_onset_ms) < 150;

    for i in 0..5 {
        let bw = (step * 0.52).min(44.0);
        let x = rect.left() + step * i as f32 + (step - bw) / 2.0;

        // 轨道
        p.rect_filled(
            egui::Rect::from_min_size(egui::pos2(x, base - avail), egui::vec2(bw, avail)),
            4.0,
            th.faint,
        );

        // 瞬时能量柱
        let lvl = t.bands[i].clamp(0.0, 1.0);
        if lvl > 0.01 {
            let bh = (lvl * avail).max(2.0);
            p.rect_filled(
                egui::Rect::from_min_size(egui::pos2(x, base - bh), egui::vec2(bw, bh)),
                4.0,
                if hot { th.warn } else { th.accent },
            );
            // 顶端亮帽
            p.rect_filled(
                egui::Rect::from_min_size(egui::pos2(x, base - bh), egui::vec2(bw, 2.5)),
                1.5,
                th.accent_hover,
            );
        }

        // 峰值帽线 (慢落)
        let pk = t.band_peak[i].clamp(0.0, 1.0) * avail;
        if pk > 1.0 {
            p.hline(
                egui::Rangef::new(x, x + bw),
                base - pk,
                egui::Stroke::new(1.0, th.accent_text.gamma_multiply(0.6)),
            );
        }

        p.text(
            egui::pos2(x + bw / 2.0, rect.bottom() - label_h / 2.0),
            egui::Align2::CENTER_CENTER,
            names[i],
            egui::FontId::proportional(10.5),
            th.hint,
        );
    }
}

/// 单颗马达电平条 (标签 + 满宽滑条 + 右对齐百分比)。
fn motor_bar(ui: &mut egui::Ui, th: &Theme, label: &str, lvl: f32, color: egui::Color32) {
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), 26.0),
        egui::Sense::hover(),
    );
    let p = ui.painter_at(rect);
    let lvl = lvl.clamp(0.0, 1.0);

    p.text(
        egui::pos2(rect.left(), rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(12.0),
        th.text_weak,
    );
    p.text(
        egui::pos2(rect.right(), rect.center().y),
        egui::Align2::RIGHT_CENTER,
        format!("{:.0}%", lvl * 100.0),
        egui::FontId::proportional(12.0),
        color,
    );

    let x0 = rect.left() + 72.0;
    let x1 = rect.right() - 40.0;
    if x1 - x0 > 24.0 {
        let bar_h = 8.0;
        let bar = egui::Rect::from_min_size(
            egui::pos2(x0, rect.center().y - bar_h / 2.0),
            egui::vec2(x1 - x0, bar_h),
        );
        p.rect_filled(bar, bar_h / 2.0, th.faint);
        let w = (x1 - x0) * lvl;
        if w > 1.0 {
            p.rect_filled(
                egui::Rect::from_min_size(bar.min, egui::vec2(w, bar_h)),
                bar_h / 2.0,
                color,
            );
        }
    }
}
