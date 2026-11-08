//! ★v24.50 音频震动页 —— 独立可选模式的开关/调参/实时监测。
//!
//! 数据流: 滑块 → `config.audio_haptics` 镜像 + `AudioHapState` 原子量即时生效 +
//! 脏标 (主循环 1s 去抖写 Config.toml); 引擎遥测 (`telemetry`) → 频谱/电平/语义显示。

use std::sync::atomic::Ordering;
use std::time::Instant;

use eframe::egui;

use crate::gui::{widgets, SorahkGui};
use crate::state::audio_state::{
    audio_status_name, AudioTelemetry, AUDIO_STATUS_RUNNING, AUDIO_STATUS_WAIT_GAME,
    AUDIO_STATUS_WAIT_SOUND,
};

impl SorahkGui {
    pub(super) fn render_audio_haptics_page(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let th = self.theme();
        ui.heading(egui::RichText::new("音频震动").size(20.0).strong().color(th.heading));
        ui.add_space(4.0);
        ui.label(th.hint_text("听游戏声音自动驱动手柄震动 —— 免注入、任何游戏。独立开关: 不影响连发与 DLL 震动。"));

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
        ui.horizontal(|ui| {
            widgets::status_dot(ui, dot_color, pulsing, 4.5);
            let sem_active = self
                .app_state
                .audio_hap
                .telemetry
                .try_lock()
                .map(|t| t.sem_active)
                .unwrap_or(false);
            let sem_tag = if sem_active { " · 语义层在线" } else { "" };
            ui.label(egui::RichText::new(format!(
                "状态: {}{}",
                audio_status_name(status),
                if enabled { sem_tag } else { "" }
            ))
            .color(th.heading));
            ui.separator();
            ui.label(th.hint_text(format!("模型: {}", crate::audio_haptics::sem::model_source_info())));
            if ui.small_button("打开模型文件夹").clicked() {
                let dir = crate::audio_haptics::sem::mod_dir();
                let _ = std::fs::create_dir_all(&dir);
                let _ = std::process::Command::new("explorer").arg(&dir).spawn();
            }
        });

        /* ── 实时监测 (频谱 + 电平 + 语义) ── */
        ui.add_space(8.0);
        let tele: Option<AudioTelemetry> = self.app_state.audio_hap.telemetry.try_lock().ok().map(|t| t.clone());
        th.panel(ui, Some("实时监测"), |ui| {
            if enabled {
                if let Some(t) = &tele {
                    draw_spectrum(ui, t);
                    ui.add_space(4.0);
                    draw_levels(ui, t);
                    ui.add_space(4.0);
                    if t.sem_active {
                        ui.label(th.hint_text("AI 识别 (YAMNet, ~0.5s 判断窗):"));
                        for (name, score) in t.sem.top3.iter().take(3) {
                            ui.horizontal(|ui| {
                                let w = (ui.available_width() * 0.4).max(80.0);
                                let bar = egui::ProgressBar::new(*score).show_percentage();
                                ui.add_sized([w, 14.0], bar);
                                ui.label(name);
                            });
                        }
                        if t.gate_factor < 0.999 {
                            ui.label(th.hint_text(format!(
                                "纯净反馈生效中: 门控系数 {:.0}%",
                                t.gate_factor * 100.0
                            )));
                        }
                    } else {
                        ui.label(th.hint_text("语义层未启用 —— 快通道 (频带识别) 独立工作, 音乐/人声可能误触发"));
                    }
                } else {
                    ui.label(th.hint_text("(遥测被引擎占用, 本帧跳过)"));
                }
            } else {
                ui.label(th.hint_text("开关未开启 —— 打开后这里显示实时频谱、马达电平与 AI 识别结果"));
            }
        });

        /* ── 参数滑块 ── */
        ui.add_space(8.0);
        let mut dirty = false;
        th.panel(ui, Some("调参"), |ui| {
            macro_rules! slider {
                ($field:ident, $range:expr, $label:expr, $hint:expr) => {{
                    let mut v = self.config.audio_haptics.$field;
                    let r = ui
                        .add(egui::Slider::new(&mut v, $range).text($label))
                        .on_hover_text($hint);
                    if r.changed() {
                        self.config.audio_haptics.$field = v;
                        dirty = true;
                    }
                }};
            }
            slider!(master_gain_pct, 0..=200, "总强度 %", "全局缩放");
            slider!(low_gain_pct, 0..=200, "低频增益 %", "爆炸/重击的沉闷感");
            slider!(high_gain_pct, 0..=200, "高频增益 %", "枪声/打击的脆响 (默认略压)");
            slider!(onset_sensitivity, 1..=8, "onset 灵敏度", "越小越容易触发; 太小会把 BGM 鼓点当枪声");
            slider!(onset_release_ms, 50..=500, "冲击衰减 ms", "一下响声震多久; 太长连打会粘成一坨");
            slider!(sustain_gain_pct, 0..=100, "持续音强度 %", "引擎/环境低鸣的跟震");
            slider!(silence_threshold, 0..=100, "静音阈值", "比这更小的声音不震");
            slider!(max_onsets_per_sec, 4..=30, "每秒触发上限", "DNF 技能连放保护");
            let mut wl = self.config.audio_haptics.whitelist_filter;
            if ui
                .checkbox(&mut wl, "只听白名单进程 (默认关: 全局听系统声音, 开箱即用)")
                .on_hover_text("开启后与连发白名单同语义: 前台不在白名单时引擎闲置。\n测试或玩白名单外的游戏请保持关闭。")
                .changed()
            {
                self.config.audio_haptics.whitelist_filter = wl;
                dirty = true;
            }
        });
        th.panel(ui, Some("纯净反馈 (语义门控)"), |ui| {
            let mut sem_enabled = self.config.audio_haptics.sem_enabled;
            if ui
                .checkbox(&mut sem_enabled, "启用 YAMNet 语义层 (音乐/人声不乱震 + 枪声/爆炸专属震感)")
                .changed()
            {
                self.config.audio_haptics.sem_enabled = sem_enabled;
                dirty = true;
            }
            if !sem_enabled {
                ui.disable(); // 语义层关 → 门控滑块禁用
            }
            macro_rules! slider {
                ($field:ident, $range:expr, $label:expr, $hint:expr) => {{
                    let mut v = self.config.audio_haptics.$field;
                    let r = ui
                        .add(egui::Slider::new(&mut v, $range).text($label))
                        .on_hover_text($hint);
                    if r.changed() {
                        self.config.audio_haptics.$field = v;
                        dirty = true;
                    }
                }};
            }
            slider!(gate_music_pct, 0..=100, "音乐命中时保留 %", "Music 类置信度 >60% 时快通道保留比; 0=完全过滤, 100=不过滤");
            slider!(gate_speech_pct, 0..=100, "人声命中时保留 %", "Speech 类置信度 >60% 时快通道保留比");
        });
        if dirty {
            // 原子量即时生效 + 打脏标 (主循环 1s 静默后写 Config.toml)
            self.app_state.audio_hap.apply_config(&self.config.audio_haptics);
            self.audio_params_dirty_since = Some(Instant::now());
        }

        /* ── 预期管理 ── */
        ui.add_space(8.0);
        th.panel(ui, Some("说明"), |ui| {
            ui.label(th.hint_text("· 快通道 (频带识别) 延迟 20-40ms, 负责\"枪响手震/爆炸猛震\"; AI 识别判断窗约 0.5-1s, 只做\"纯净反馈\"裁判, 不承担瞬态触发。"));
            ui.label(th.hint_text("· Xbox 手柄是 2 颗转子马达 (只有低频+高频两路), \"枪响手震、爆炸猛震、放歌不乱震\"与飞智一致; 震动细腻层次受马达物理限制, 不是本工具算法问题。"));
            ui.label(th.hint_text("· 与 DLL 震动同时开启时两者取最大值合并 (互不覆盖); DNF 内建议只用 DLL 模式 (受击更精准), 音频模式适合免注入场景与其他游戏。"));
            ui.label(th.hint_text("· 马达保护: 持续高输出 10 秒自动限幅 60% 持续 5 秒 (防马达过热)。"));
        });
        let _ = ctx; // 预留: 需要连续重绘时 request_repaint
    }
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
