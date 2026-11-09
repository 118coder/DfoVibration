//! ★v24.60 音频震动页 —— 开关 / 预设 / 调参 / 实时监测。
//!
//! 设计口径 (用户验收要求: **所有设置必须看得懂**): 每个滑条都配一句"调大/调小会怎样"
//! 的人话提示, 术语 (门槛/gamma/attack) 一律不出现在界面上。
//!
//! 数据流: 滑块 → `config.audio_haptics` 镜像 + `AudioHapState` 原子量即时生效 +
//! 脏标 (主循环 1s 去抖写 Config.toml); 引擎遥测 (`telemetry`) → 频谱/电平/判定显示。

use std::sync::atomic::Ordering;
use std::time::Instant;

use eframe::egui;

use crate::audio_haptics::mapping::{self, PRESETS};
use crate::gui::{widgets, SorahkGui};
use crate::update::APP_VERSION;
use crate::state::audio_state::{
    audio_status_name, AudioTelemetry, AUDIO_STATUS_RUNNING, AUDIO_STATUS_WAIT_GAME,
    AUDIO_STATUS_WAIT_SOUND,
};

impl SorahkGui {
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

    pub(super) fn render_audio_haptics_page(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let th = self.theme();
        ui.heading(egui::RichText::new("音频震动").size(20.0).strong().color(th.heading));
        ui.add_space(4.0);
        ui.label(th.hint_text("听着游戏的声音自动震手柄 —— 不用注入，任何游戏都能用。独立开关：不影响连发和 DLL 震动。"));

        /* ── 总开关大按钮 ── */
        ui.add_space(8.0);
        let enabled = self.config.audio_haptics.enabled;
        let (text, tint) = if enabled {
            ("关闭音频震动", th.warn)
        } else {
            ("开启音频震动", th.good)
        };
        let btn = egui::Button::new(
            egui::RichText::new(text).size(17.0).strong().color(egui::Color32::WHITE),
        )
        .fill(tint.gamma_multiply(if enabled { 0.55 } else { 0.85 }))
        .min_size(egui::vec2(ui.available_width(), 40.0));
        if ui.add(btn).clicked() {
            self.config.audio_haptics.enabled = !enabled;
            let _ = self.config.save_to_file("Config.toml");
            let _ = self.app_state.reload_config(self.config.clone());
        }

        /* ── 状态行 ── */
        ui.add_space(6.0);
        let status = self.app_state.audio_hap.status.load(Ordering::Relaxed);
        let (dot_color, pulsing) = match status {
            AUDIO_STATUS_RUNNING => (th.good, true),
            AUDIO_STATUS_WAIT_GAME | AUDIO_STATUS_WAIT_SOUND => (th.warn, false),
            _ => (ui.visuals().weak_text_color(), false),
        };
        let tele: Option<AudioTelemetry> = self
            .app_state
            .audio_hap
            .telemetry
            .try_lock()
            .ok()
            .map(|t| t.clone());
        ui.horizontal(|ui| {
            widgets::status_dot(ui, dot_color, pulsing, 4.5);
            let mode_tag = if enabled {
                tele.as_ref()
                    .map(|t| format!(" · {}", mapping::mode_name(t.mode)))
                    .unwrap_or_default()
            } else {
                String::new()
            };
            ui.label(egui::RichText::new(format!(
                "状态: {}{}",
                audio_status_name(status),
                mode_tag
            ))
            .color(th.heading));
            ui.separator();
            if ui
                .button("马达自检 (0.5 秒)")
                .on_hover_text("不用放声音，直接让手柄震一下。\n震了 = 手柄那条线没问题；不震 = 检查手柄连接；震了但放声音时不震 = 检查上面的开关和声音设置。")
                .clicked()
            {
                self.app_state.audio_hap.request_self_check();
            }
            if ui
                .button("屏蔽校准 (3 秒)")
                .on_hover_text("点一下，然后保持当前环境安静 3 秒（别放游戏音效）。\n软件会自动测出背景噪音有多大，把屏蔽门槛调到刚好——换个新环境（新电脑/新耳机）时建议重新校准一次。")
                .clicked()
            {
                self.app_state.audio_hap.request_calibration();
            }
        });

        /* ── 校准结果: 引擎给出建议值, 用户一键应用 ── */
        let calib = self.app_state.audio_hap.calib_result.load(Ordering::Relaxed);
        if calib > 0 {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(format!("校准完成：检测到当前环境噪音，屏蔽门槛建议调整为 {calib}。"))
                        .color(th.good),
                );
                if ui.button("应用").clicked() {
                    self.config.audio_haptics.mask_threshold = calib;
                    self.app_state
                        .audio_hap
                        .mask_threshold
                        .store(calib, Ordering::Relaxed);
                    self.app_state.audio_hap.calib_result.store(0, Ordering::Relaxed);
                    dirty_mark(self);
                }
                if ui.button("忽略").clicked() {
                    self.app_state.audio_hap.calib_result.store(0, Ordering::Relaxed);
                }
            });
        }

        /* ═══════ 预设（一键切换 + 自定义保存/导出导入, ★v24.61 对齐震动区） ═══════ */
        ui.add_space(8.0);
        th.panel(ui, Some("预设"), |ui| {
            ui.label(th.hint_text("点一个场景就够了，选完还能在下面微调。调好之后可以「保存为」自己的预设，也能导出分享。"));
            let current = self.config.audio_haptics.preset.clone();
            ui.add_space(4.0);
            /* ── 内置预设 (第一项 = 实测最佳推荐档) ── */
            for (i, p) in PRESETS.iter().enumerate() {
                let (scene, feel) = match p.name {
                    "人声清晰" => ("打游戏 / 看剧情（实测最佳，优先推荐）", "只跟人声频段，音乐杂音几乎不理，干净利落"),
                    "轻柔细腻" => ("轻声内容（实测次佳）", "细腻微颤，轻声细语也有反馈，不吵"),
                    "游戏强劲" => ("打击感拉满", "招招猛震，BGM 压制最强"),
                    "低频沉浸" => ("看电影 / 大场面", "爆炸和低音轰隆感拉满"),
                    "全频强劲" => ("要最猛的裸手感", "所有声音都直接震，不屏蔽任何东西"),
                    _ => ("排障用", "引擎在跑但完全不震（验证马达链是否正常）"),
                };
                let rec = if i == 0 { " ⭐推荐" } else { "" };
                let selected = current == p.name;
                let label = format!("{}{}{}", p.name, rec, if selected { " ✓" } else { "" });
                if ui
                    .selectable_label(selected, egui::RichText::new(label).strong())
                    .on_hover_text(format!("{scene} —— {feel}"))
                    .clicked()
                {
                    if self.apply_audio_preset(p.name) {
                        dirty_mark(self);
                    }
                }
                ui.label(th.hint_text(format!("　└ {scene}：{feel}")));
            }

            /* ── 我的自定义预设 (保存为 / 应用 / 删除) ── */
            ui.add_space(8.0);
            ui.separator();
            let user = &self.config.audio_haptics.presets;
            if !user.is_empty() {
                ui.label(egui::RichText::new("我的预设").strong().color(th.heading));
                let sel = self.audio_user_sel.min(user.len() - 1);
                egui::ComboBox::from_id_salt("audio_user_preset_sel")
                    .selected_text(user[sel].name.clone())
                    .width(180.0)
                    .show_ui(ui, |ui| {
                        for (i, e) in user.iter().enumerate() {
                            if ui.selectable_label(i == sel, &e.name).clicked() {
                                self.audio_user_sel = i;
                            }
                        }
                    });
                if ui.button("应用").on_hover_text("应用选中的自定义预设").clicked() {
                    let e = user[sel].clone();
                    self.apply_preset_entry(&e);
                    dirty_mark(self);
                }
                if ui
                    .button("删除")
                    .on_hover_text("删除选中的自定义预设（内置预设不受影响）")
                    .clicked()
                {
                    self.config.audio_haptics.presets.remove(sel);
                    self.audio_user_sel = 0;
                    dirty_mark(self);
                }
                ui.add_space(4.0);
            }
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("保存当前设置为:").color(ui.visuals().weak_text_color()));
                ui.add(
                    egui::TextEdit::singleline(&mut self.audio_preset_name)
                        .desired_width(120.0)
                        .hint_text("预设名"),
                );
                let name_ok = !self.audio_preset_name.trim().is_empty()
                    && !mapping::is_protected_preset(self.audio_preset_name.trim());
                if ui
                    .add_enabled(name_ok, egui::Button::new("保存"))
                    .on_hover_text(if name_ok {
                        "把当前所有滑条的值存成一个预设（同名直接覆盖）"
                    } else if self.audio_preset_name.trim().is_empty() {
                        "先起个名字"
                    } else {
                        "这个名字和内置预设重名了，换一个（内置预设永远可用，不能被覆盖）"
                    })
                    .clicked()
                {
                    let name = self.audio_preset_name.trim().to_string();
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
                    dirty_mark(self);
                }
            });

            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui
                    .button("导出当前设置")
                    .on_hover_text("把当前所有音频震动参数导出成一个 .toml 文件（存在程序目录），可以发给别人或换电脑用")
                    .clicked()
                {
                    let entry = self.snapshot_config_entry(&self.config.audio_haptics.preset.clone());
                    match toml::to_string_pretty(&entry) {
                        Ok(body) => {
                            let content = format!(
                                "# ═══════════════════════════════════════════════════════\n\
                                 #  Sorahk 音频震动设定导出 (v{APP_VERSION})\n\
                                 #  导入: 音频震动页 → 预设 → 导入设置\n\
                                 # ═══════════════════════════════════════════════════════\n{body}"
                            );
                            let fname =
                                Self::save_export_file(&content, "audio_export");
                            self.audio_export_msg = Some(format!("已导出: {fname} (程序目录)"));
                        }
                        Err(e) => self.audio_export_msg = Some(format!("导出失败: {e}")),
                    }
                }
                /* 导入: 扫描程序目录 audio_export*.toml */
                let files = crate::job_presets::list_export_files_for_route("audio_export", false);
                if !files.is_empty() {
                    let sel = self.audio_import_sel.min(files.len() - 1);
                    egui::ComboBox::from_id_salt("audio_import_sel")
                        .selected_text(files[sel].clone())
                        .width(160.0)
                        .show_ui(ui, |ui| {
                            for (i, f) in files.iter().enumerate() {
                                if ui.selectable_label(i == sel, f).clicked() {
                                    self.audio_import_sel = i;
                                }
                            }
                        });
                    if ui
                        .button("导入")
                        .on_hover_text("从导出文件读取预设并应用 (名字与内置重名的会被拒绝)")
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
                                    Some(pos) => {
                                        self.config.audio_haptics.presets[pos] = e.clone()
                                    }
                                    None => self.config.audio_haptics.presets.push(e.clone()),
                                }
                                self.apply_preset_entry(&e);
                                dirty_mark(self);
                                self.audio_export_msg = Some(format!("已导入并应用: {name} ({f})"));
                            }
                            Some(e) if mapping::is_protected_preset(e.name.trim()) => {
                                self.audio_export_msg = Some(format!(
                                    "导入失败: 「{}」与内置预设重名，请改名字后重新导出",
                                    e.name.trim()
                                ));
                            }
                            _ => {
                                self.audio_export_msg = Some(format!("{f} 解析失败 (文件损坏或不是音频震动导出)"));
                            }
                        }
                    }
                }
                if ui
                    .button("还原默认")
                    .on_hover_text("清空我的全部自定义预设，并回到推荐档「人声清晰」")
                    .clicked()
                {
                    self.config.audio_haptics.presets.clear();
                    self.audio_user_sel = 0;
                    self.apply_audio_preset(PRESETS[0].name);
                    dirty_mark(self);
                }
            });
            if let Some(m) = &self.audio_export_msg {
                ui.add_space(2.0);
                ui.label(egui::RichText::new(m).size(11.0).color(th.good));
            }
        });

        /* ═══════ 实时监测 ═══════ */
        ui.add_space(8.0);
        th.panel(ui, Some("实时监测"), |ui| {
            if enabled {
                if let Some(t) = &tele {
                    draw_spectrum(ui, t);
                    ui.add_space(4.0);
                    draw_levels(ui, t);
                    ui.add_space(6.0);
                    /* ★v24.60 内容判定: 用大白话告诉玩家"现在为什么不震/为什么震"。 */
                    let mode_line = match t.mode {
                        mapping::MODE_MUSIC => {
                            egui::RichText::new("🎵 当前判定：背景音乐段 —— 连续震动被压住，只有鼓点（节奏力度）和够响的音效会震")
                                .color(th.heading)
                        }
                        mapping::MODE_QUIET => {
                            egui::RichText::new("🔇 当前判定：安静段 —— 手柄保持安静，不会乱震").color(th.heading)
                        }
                        mapping::MODE_VOCAL => {
                            egui::RichText::new("🗣 当前判定：人声/细节段 —— 轻微细腻的反馈（不会吵）").color(th.heading)
                        }
                        _ => egui::RichText::new("🎮 当前判定：游戏音效段 —— 正常跟手震").color(th.good),
                    };
                    ui.label(mode_line);
                    /* 噪声底与生效门槛: 把"为什么不震"的依据摆出来 (int16 尺度数字
                     * 对玩家无意义, 换算成"满格响度的百分比")。 */
                    let map_max = self.config.audio_haptics.map_max.max(1) as f32;
                    ui.label(th.hint_text(format!(
                        "背景噪声约为满格响度的 {:.0}% · 屏蔽门槛 {:.0}% · 超过门槛的声音才会震",
                        t.noise_floor / map_max * 100.0,
                        t.threshold_eff / map_max * 100.0,
                    )));
                } else {
                    ui.label(th.hint_text("(监测数据被引擎占用，本帧跳过)"));
                }
            } else {
                ui.label(th.hint_text("开关未开启 —— 打开后这里显示实时频谱、马达电平和当前判定"));
            }
        });

        /* ═══════ 手感调节 ═══════ */
        let mut dirty = false;
        ui.add_space(8.0);
        th.panel(ui, Some("手感调节"), |ui| {
            ui.label(th.hint_text("下面每一条都即时生效。鼠标悬停在名字上能看到详细说明。"));
            ui.add_space(6.0);
            macro_rules! row {
                ($ui:ident, $field:ident, $atom:ident, $range:expr, $label:expr, $tip:expr) => {{
                    let mut v = self.config.audio_haptics.$field;
                    let r = $ui
                        .add(egui::Slider::new(&mut v, $range).suffix("%").text($label))
                        .on_hover_text($tip);
                    if r.changed() {
                        self.config.audio_haptics.$field = v;
                        self.app_state
                            .audio_hap
                            .$atom
                            .store(v, std::sync::atomic::Ordering::Relaxed);
                        dirty = true;
                    }
                }};
            }

            row!(ui, master_gain_pct, master_gain, 0..=150u32,
                "🔊 全局总强度",
                "所有震动一起变大/变小。建议 60-80；觉得整体太猛就往下调，觉得没劲就往上调。"
            );
            row!(ui, punch_pct, punch_pct, 0..=200u32,
                "👊 打击对比度",
                "100% 是标准手感，不是音量。调大 → 轻攻击变轻、只有大招和重击才猛震；调小 → 每一下攻击都有明显反馈。"
            );
            {
                /* 屏蔽门槛的量纲是 int16 尺度, 不加 % 后缀, 用人话提示 */
                let mut v = self.config.audio_haptics.mask_threshold;
                let r = ui
                    .add(egui::Slider::new(&mut v, 0..=1200u32).step_by(20.0).text("🛡 背景屏蔽门槛"))
                    .on_hover_text("比这个响度小的声音一律不震。调高 = 更安静（音乐、杂音更不容易震到手）；调低 = 更灵敏。\n默认 400 适合大多数游戏；听歌挂机嫌吵就加到 600-800；嫌震得迟钝就减到 200-300。\n不确定就点上面的「屏蔽校准」，让软件自动测。");
                if r.changed() {
                    self.config.audio_haptics.mask_threshold = v;
                    self.app_state
                        .audio_hap
                        .mask_threshold
                        .store(v, std::sync::atomic::Ordering::Relaxed);
                    dirty = true;
                }
            }
            row!(ui, rhythm_pct, rhythm_pct, 0..=100u32,
                "🥁 节奏力度（跟鼓点）",
                "听歌/挂机时跟着鼓点震的强度。0 = 听歌完全不跟鼓点；听歌想要节奏感就调到 40-70。"
            );
            row!(ui, low_gain_pct, low_gain, 0..=200u32,
                "🥊 低频量感（左马达）",
                "爆炸、重击、贝斯这种“轰”的感觉。调 0 = 左马达基本不震。"
            );
            row!(ui, high_gain_pct, high_gain, 0..=200u32,
                "⚡ 高频量感（右马达）",
                "枪声、打击、碎裂声这种“脆”的感觉。调 0 = 右马达基本不震。"
            );
            row!(ui, attack_pct, attack_pct, 5..=90u32,
                "⏱ 响应速度",
                "调高 = 声一响手立刻震，更跟手；调低 = 更柔和。打游戏建议 60 以上。"
            );
            row!(ui, release_pct, release_pct, 2..=60u32,
                "🌊 余韵",
                "声音停下后震动收尾多快。调高 = 拖尾长、更绵；调低 = 干脆利落。"
            );

            if dirty {
                /* 原子量已逐项即时写入; 这里补一次整体镜像 + 打脏标 (主循环 1s 去抖后落盘) */
                dirty_mark(self);
            }
        });

        /* ═══════ 屏蔽选项 ═══════ */
        ui.add_space(8.0);
        th.panel(ui, Some("屏蔽选项（防止 BGM 和杂音乱震）"), |ui| {
            let mut dirty = false;
            macro_rules! chk {
                ($ui:ident, $field:ident, $atom:ident, $label:expr, $tip:expr) => {{
                    let mut v = self.config.audio_haptics.$field;
                    if $ui.checkbox(&mut v, $label).on_hover_text($tip).changed() {
                        self.config.audio_haptics.$field = v;
                        self.app_state.audio_hap.$atom.store(v, Ordering::Relaxed);
                        dirty = true;
                    }
                }};
            }
            chk!(ui, auto_profile, auto_profile, "智能识别（自动分辨游戏 / 音乐 / 安静）",
                "推荐开着。软件会自动判断现在放的是游戏音效还是背景音乐：\n放 BGM 时压住连续震动只留鼓点和真音效，安静时手柄完全不动。\n嫌它误判就关掉，关掉后完全按下面的滑条来。");
            chk!(ui, adaptive_threshold, adaptive_threshold, "自动屏蔽背景噪音",
                "推荐开着。软件会一直听着背景噪音有多大，自动把门槛抬高到噪音之上。\n背景越吵门槛越高，BGM 和环境音就不容易震到手。");
            chk!(ui, quiet_mask, quiet_mask, "安静时手柄完全不动",
                "推荐开着。没有游戏音效时（挂机/读图/大厅安静下来）手柄保持零震动。\n关掉 = 安静时也保留一半的轻微跟随。");
            {
                let mut v = self.config.audio_haptics.music_mask_pct;
                let r = ui
                    .add(egui::Slider::new(&mut v, 0..=100u32).suffix("%").text("🎵 音乐段压制强度"))
                    .on_hover_text("识别到背景音乐时，连续震动额外压掉多少。默认 60%；\n嫌听歌时手还是一直震就调到 80-100；嫌压制太狠（BGM 里的大招不震了）就调低。");
                if r.changed() {
                    self.config.audio_haptics.music_mask_pct = v;
                    self.app_state
                        .audio_hap
                        .music_mask_pct
                        .store(v, Ordering::Relaxed);
                    dirty = true;
                }
            }
            {
                let mut v = self.config.audio_haptics.texture_pct;
                let r = ui
                    .add(egui::Slider::new(&mut v, 0..=100u32).suffix("%").text("🫧 细腻麻感"))
                    .on_hover_text("听人声/ASMR/轻声内容时的微弱麻颤质感。不打游戏只听声音的话可以调高一点；打游戏建议 0-30。");
                if r.changed() {
                    self.config.audio_haptics.texture_pct = v;
                    self.app_state
                        .audio_hap
                        .texture_pct
                        .store(v, Ordering::Relaxed);
                    dirty = true;
                }
            }
            if dirty {
                dirty_mark(self);
            }
        });

        /* ── ★v24.52 音频专属白名单 (玩家可完全不启用) ── */
        th.panel(ui, Some("只听指定的游戏（可选）"), |ui| {
            ui.label(th.hint_text(
                "默认关 = 听整个电脑的声音。开启后只听下面名单里的游戏，别的软件（QQ音乐、视频网站）的声音一律不理。",
            ));
            let mut on = self.config.audio_haptics.whitelist_enabled;
            if ui
                .checkbox(&mut on, "启用名单（只听名单里的应用）")
                .on_hover_text("开启后如果游戏还没启动，状态会显示「等待白名单游戏」。")
                .changed()
            {
                self.config.audio_haptics.whitelist_enabled = on;
                // 老字段只作遗留别名, 不再驱动行为
                self.config.audio_haptics.whitelist_filter = false;
                dirty = true;
            }

            // 条目来源: 默认复用连发白名单 (玩家只维护一份)
            let src_macro = !self
                .config
                .audio_haptics
                .whitelist_source
                .eq_ignore_ascii_case("custom");
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("名单来源").color(ui.visuals().weak_text_color()));
                if ui
                    .selectable_label(src_macro, "和连发白名单共用一份")
                    .on_hover_text("与「连发映射」页的白名单共用同一份列表（只读，不会改它）。推荐。")
                    .clicked()
                    && !src_macro
                {
                    self.config.audio_haptics.whitelist_source = "macro".to_string();
                    dirty = true;
                }
                if ui
                    .selectable_label(!src_macro, "单独填一份")
                    .on_hover_text("音频震动单独一份名单，和连发白名单互不影响。")
                    .clicked()
                    && src_macro
                {
                    self.config.audio_haptics.whitelist_source = "custom".to_string();
                    dirty = true;
                }
            });

            if !src_macro {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("应用名单").color(ui.visuals().weak_text_color()));
                    let r = ui.add(
                        egui::TextEdit::singleline(&mut self.audio_whitelist_edit)
                            .desired_width(ui.available_width())
                            .hint_text("DNF.exe, E:\\Games\\DFO.exe（逗号分隔，可填完整路径）"),
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
                });
            }

            // 实时判定结果 (开名单后"到底有没有命中"一眼可见)
            if let Some(t) = &tele {
                ui.add_space(2.0);
                let verdict = if !t.wl_matched {
                    egui::RichText::new("拦截中（前台不在名单里，不读声音）").color(th.warn)
                } else if t.wl_entries == 0 {
                    egui::RichText::new("未过滤（名单是空的，等于全局听）").color(th.warn)
                } else {
                    egui::RichText::new("命中，正在读取该游戏的声音").color(th.good)
                };
                ui.label(verdict);
                ui.label(th.hint_text(format!(
                    "来源 {} · 条目 {} · 前台 {}",
                    if t.wl_source.is_empty() { "-" } else { &t.wl_source },
                    t.wl_entries,
                    if t.wl_foreground.is_empty() {
                        "(未取到)"
                    } else {
                        &t.wl_foreground
                    }
                )));
                if !t.wl_error.is_empty() {
                    ui.label(
                        egui::RichText::new(format!("进程信息获取提示: {}", t.wl_error))
                            .color(th.warn)
                            .size(11.0),
                    );
                }
                if t.wl_degraded {
                    ui.label(
                        egui::RichText::new(
                            "按进程精确监听不可用，已降级为听整个设备（仍按前台判断）。\n游戏以管理员运行时，请也以管理员身份启动本工具。",
                        )
                        .color(th.warn)
                        .size(11.0),
                    );
                }
            }

            ui.horizontal(|ui| {
                let mut process_mode = self
                    .config
                    .audio_haptics
                    .whitelist_mode
                    .eq_ignore_ascii_case("process");
                if ui
                    .checkbox(&mut process_mode, "只监听名单进程的声音（Win10 2004+）")
                    .on_hover_text("开 = 只收名单里那个游戏的声音，别的程序的声音根本不进采集。\n关 = 只要前台在名单里就听整个设备的声音（兼容旧系统）。")
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
                if ui.checkbox(&mut tree, "含子进程").on_hover_text("游戏启动器拉起的子进程（登录器/反作弊）也一起算。").changed() {
                    self.config.audio_haptics.whitelist_include_tree = tree;
                    dirty = true;
                }
            });
            if dirty {
                dirty_mark(self);
            }
        });

        /* ═══════ 高级选项 (默认折叠, 默认值即最佳) ═══════ */
        ui.add_space(8.0);
        egui::CollapsingHeader::new("高级选项（默认值就够用，不建议乱调）")
            .id_salt("audio_advanced")
            .show(ui, |ui| {
                let mut dirty = false;
                macro_rules! adv_num {
                    ($ui:ident, $field:ident, $atom:ident, $range:expr, $step:expr, $label:expr, $tip:expr) => {{
                        let mut v = self.config.audio_haptics.$field;
                        let r = $ui
                            .add(
                                egui::Slider::new(&mut v, $range)
                                    .step_by($step)
                                    .text($label),
                            )
                            .on_hover_text($tip);
                        if r.changed() {
                            self.config.audio_haptics.$field = v;
                            self.app_state
                                .audio_hap
                                .$atom
                                .store(v, std::sync::atomic::Ordering::Relaxed);
                            dirty = true;
                        }
                    }};
                }
                macro_rules! adv_chk {
                    ($ui:ident, $field:ident, $atom:ident, $label:expr, $tip:expr) => {{
                        let mut v = self.config.audio_haptics.$field;
                        if $ui.checkbox(&mut v, $label).on_hover_text($tip).changed() {
                            self.config.audio_haptics.$field = v;
                            self.app_state.audio_hap.$atom.store(v, Ordering::Relaxed);
                            dirty = true;
                        }
                    }};
                }

                th.panel(ui, Some("马达与频带"), |ui| {
                    adv_chk!(ui, dual_motor, dual_motor, "双马达分频（低频走左、高频走右）",
                        "推荐开。左马达管“轰”（爆炸/贝斯），右马达管“脆”（枪声/打击），层次更分明。\n关掉 = 两颗马达一起同步震，感觉更“整”，但少了层次。");
                    adv_chk!(ui, vocal_only, vocal_only, "只跟人声频段（双马达关闭时生效）",
                        "开了以后只跟人声频段的能量，音乐/环境音基本不震。适合看剧/听书。");
                    adv_num!(ui, bass_high_hz, bass_high_hz, 200..=1000u32, 10.0,
                        "低频上限 Hz",
                        "低于这个频率的声音算“低频”走左马达。默认 750；觉得低频抢戏就调低，觉得轰得不够就调高。");
                    adv_num!(ui, vocal_low_hz, vocal_low_hz, 300..=1500u32, 10.0,
                        "人声下限 Hz", "人声频段的下边界，默认 600。一般不用动。");
                    adv_num!(ui, vocal_high_hz, vocal_high_hz, 1500..=8000u32, 50.0,
                        "人声上限 Hz", "人声频段的上边界，默认 4200。一般不用动。");
                });

                ui.add_space(6.0);
                th.panel(ui, Some("响度标尺"), |ui| {
                    adv_num!(ui, map_max, map_max, 1000..=12000u32, 100.0,
                        "满格响度",
                        "声音响到什么程度算“震满格”。默认 6000；觉得动静大了调高，觉得不够劲调低。\n（配合「背景屏蔽门槛」用：门槛决定“震不震”，这条决定“震多满”。）");
                    adv_num!(ui, dead_zone_pct, dead_zone_pct, 0..=20u32, 1.0,
                        "震动死区 %",
                        "低于这个强度的输出直接归零，消除马达细微的“嗡嗡”底噪。默认 4；嫌轻微动静太黏就加到 6-8。");
                });

                if dirty {
                    dirty_mark(self);
                }
            });

        /* ── 预期管理 ── */
        ui.add_space(8.0);
        th.panel(ui, Some("说明"), |ui| {
            ui.label(th.hint_text("· 本引擎是纯算法（频带分析 + 自动门槛），不需要下载任何模型文件。"));
            ui.label(th.hint_text("· Xbox 手柄只有 2 颗马达（低频 + 高频），震动层次受马达物理限制，不是软件问题。"));
            ui.label(th.hint_text("· 与 DLL 震动同时开启时两者取最大值合并（互不覆盖）；DNF 内建议只用 DLL 模式（受击更精准），音频模式适合免注入场景与其他游戏。"));
            ui.label(th.hint_text("· 马达保护：持续高输出 10 秒自动限幅 60% 持续 5 秒（防马达过热）。"));
            ui.label(th.hint_text("· 换了耳机/音箱/环境之后，建议重新点一次「屏蔽校准」。"));
            ui.label(th.hint_text("· 白名单条目默认直接复用连发白名单；关掉音频震动时，连发/组合键/DLL 注入行为与旧版完全一致。"));
        });
        let _ = ctx; // 预留: 需要连续重绘时 request_repaint
    }
}

/// 打脏标 + 整体镜像 (滑条拖动/预设应用后统一走这里, 主循环 1s 去抖落盘)。
fn dirty_mark(gui: &mut SorahkGui) {
    gui.app_state
        .audio_hap
        .apply_config(&gui.config.audio_haptics);
    gui.audio_params_dirty_since = Some(Instant::now());
}

/// 5 频带频谱条 (egui painter 自绘, 不引第三方绘图库)。
fn draw_spectrum(ui: &mut egui::Ui, t: &AudioTelemetry) {
    let desired = egui::vec2(ui.available_width(), 80.0);
    let (rect, _) = ui.allocate_exact_size(desired, egui::Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 3.0, egui::Color32::from_black_alpha(24));
    let names = ["超低", "低", "中", "高", "超高"];
    let n = 5.0;
    let step = rect.width() / n;
    let now_ms = crate::vibration::now_ms_u64();
    let onset_hot = t.last_onset_ms != 0 && now_ms.saturating_sub(t.last_onset_ms) < 150;
    for i in 0..5 {
        let h = (t.band_peak[i].clamp(0.0, 1.0)) * (rect.height() - 16.0);
        let x = rect.left() + step * i as f32 + step * 0.15;
        let w = step * 0.7;
        let color = if onset_hot {
            egui::Color32::from_rgb(255, 160, 60)
        } else {
            egui::Color32::from_rgb(90, 170, 255)
        };
        if h > 1.0 {
            painter.rect_filled(
                egui::Rect::from_min_size(
                    egui::pos2(x, rect.bottom() - 14.0 - h),
                    egui::vec2(w, h),
                ),
                2.0,
                color,
            );
        }
        painter.text(
            egui::pos2(x + w / 2.0, rect.bottom() - 7.0),
            egui::Align2::CENTER_CENTER,
            names[i],
            egui::FontId::proportional(10.0),
            ui.visuals().weak_text_color(),
        );
    }
}

/// L/R 马达电平条 (与真实输出同步)。
fn draw_levels(ui: &mut egui::Ui, t: &AudioTelemetry) {
    for (label, lvl) in [("L 低频马达", t.level_l), ("R 高频马达", t.level_r)] {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(label).color(ui.visuals().weak_text_color()));
            let bar = egui::ProgressBar::new(lvl.clamp(0.0, 1.0)).show_percentage();
            ui.add_sized([ui.available_width() * 0.75, 14.0], bar);
        });
    }
}
