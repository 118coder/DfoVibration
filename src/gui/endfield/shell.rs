//! 新外壳的布局：16:9 默认窗口 + 顶栏 / 上导航 / 中央内容 / 常驻底栏。
//! 顶栏图标（深浅、引导、设置、设备、退出、最小化、关闭）全部可见可点。

use std::time::Instant;

use eframe::egui::{self, Align2, FontId, Rect, RichText, Stroke};

use crate::gui::SorahkGui;
use crate::gui::main_window::FrameState;
use crate::ui_api::{SCHEMA, UiBackend, UiCommand, UiMode, UiSnapshot};

use super::EfRoute;
use super::dial;
use super::palette::EfPalette;

fn icon_btn(ui: &mut egui::Ui, pal: &EfPalette, glyph: &str, tip: &str) -> egui::Response {
    let btn = egui::Button::new(RichText::new(glyph).size(16.0).color(pal.ink))
        .fill(pal.panel)
        .stroke(Stroke::new(1.0, pal.line))
        .corner_radius(egui::CornerRadius::same(2))
        .min_size(egui::vec2(34.0, 34.0));
    ui.add(btn).on_hover_text(tip)
}

fn flat_btn(ui: &mut egui::Ui, pal: &EfPalette, text: &str, active: bool) -> egui::Response {
    let btn = egui::Button::new(
        RichText::new(text)
            .size(12.0)
            .color(if active { pal.paper } else { pal.ink }),
    )
    .fill(if active { pal.accent } else { pal.panel })
    .stroke(Stroke::new(1.0, if active { pal.accent } else { pal.line }))
    .corner_radius(egui::CornerRadius::same(2))
    .min_size(egui::vec2(0.0, 28.0));
    ui.add(btn)
}

/// 切角面板：墨线描边 + 右下黄标，标题走「英文 eyebrow + 中文」。
fn panel(
    ui: &mut egui::Ui,
    pal: &EfPalette,
    eyebrow: &str,
    title: &str,
    add: impl FnOnce(&mut egui::Ui),
) {
    egui::Frame::NONE
        .fill(pal.panel)
        .stroke(Stroke::new(1.0, pal.line))
        .corner_radius(egui::CornerRadius::same(2))
        .inner_margin(egui::Margin::same(12))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(eyebrow).size(9.0).color(pal.ink_dim));
                ui.label(RichText::new(title).size(13.0).strong().color(pal.ink));
            });
            ui.add_space(6.0);
            add(ui);
            let r = ui.max_rect();
            let s = 9.0;
            ui.painter().add(egui::Shape::convex_polygon(
                vec![
                    egui::pos2(r.max.x - s, r.max.y),
                    egui::pos2(r.max.x, r.max.y - s),
                    egui::pos2(r.max.x, r.max.y),
                ],
                pal.accent,
                Stroke::NONE,
            ));
        });
}

fn stat(ui: &mut egui::Ui, pal: &EfPalette, value: String, label: &str) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(value).size(21.0).strong().color(pal.ink));
        ui.label(RichText::new(label).size(10.0).color(pal.ink_dim));
    });
}

fn line(ui: &mut egui::Ui, pal: &EfPalette, k: &str, v: String) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(k).size(11.0).color(pal.ink_dim));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(RichText::new(v).size(11.5).color(pal.ink));
        });
    });
}

/// 入口：由 `SorahkGui::render_shell` 在完整模式下调用。
pub fn render(gui: &mut SorahkGui, ctx: &egui::Context, fs: &FrameState) {
    // 中央圆盘的手柄线稿沿用项目原有 SVG 纹理（与手柄映射页同一份）。
    if gui.gamepad_texture.is_none() || gui.gamepad_texture_dark != gui.dark_mode {
        gui.gamepad_texture = crate::gui::gamepad_mapping::load_gamepad_texture(ctx, gui.dark_mode);
        gui.gamepad_texture_dark = gui.dark_mode;
    }

    let pal = EfPalette::new(gui.dark_mode);
    let snap = gui.snapshot();
    let mut cmds: Vec<UiCommand> = Vec::new();
    let mut open_settings = false;
    let mut close_dialog = false;

    egui::TopBottomPanel::top("ef_top")
        .exact_height(64.0)
        .frame(
            egui::Frame::NONE
                .fill(pal.paper)
                .inner_margin(egui::Margin::symmetric(18, 10)),
        )
        .show_separator_line(false)
        .show(ctx, |ui| {
            // 自绘标题栏的拖拽 / 双击最大化，必须在按钮之前注册。
            gui.render_title_bar_drag(ui, ctx);
            ui.horizontal(|ui| {
                let (seal, _) =
                    ui.allocate_exact_size(egui::vec2(38.0, 38.0), egui::Sense::hover());
                let p = ui.painter();
                p.rect_filled(seal, 2.0, pal.accent);
                p.text(
                    seal.center(),
                    Align2::CENTER_CENTER,
                    "S",
                    FontId::proportional(20.0),
                    pal.paper,
                );
                ui.add_space(10.0);
                ui.vertical(|ui| {
                    ui.label(RichText::new("SORAHK").size(16.0).strong().color(pal.ink));
                    ui.label(RichText::new("DFO · VIBRATION").size(9.0).color(pal.ink_dim));
                });
                ui.add_space(18.0);
                ui.label(
                    RichText::new(if snap.paused { "● 已暂停" } else { "● 运行中" })
                        .size(12.0)
                        .color(if snap.paused { pal.alert } else { pal.accent }),
                );
                ui.label(
                    RichText::new(format!(
                        "v{}  ·  {} 映射  ·  {} worker",
                        snap.version, snap.mappings, snap.worker_count
                    ))
                    .size(10.5)
                    .color(pal.ink_dim),
                );

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if icon_btn(ui, &pal, "⏻", "退出程序（停止键盘鼠标钩子）").clicked() {
                        cmds.push(UiCommand::Quit);
                    }
                    if icon_btn(ui, &pal, "×", "关闭到托盘").clicked() {
                        close_dialog = true;
                    }
                    if icon_btn(ui, &pal, "—", "最小化").clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                    }
                    ui.add_space(6.0);
                    if icon_btn(ui, &pal, "◐", "切换深色 / 浅色").clicked() {
                        cmds.push(UiCommand::ToggleTheme);
                    }
                    if icon_btn(ui, &pal, "?", "开局引导 / 使用说明").clicked() {
                        cmds.push(UiCommand::ShowGuide);
                    }
                    if icon_btn(ui, &pal, "⚙", "设置").clicked() {
                        open_settings = true;
                    }
                    if icon_btn(ui, &pal, "▣", "设备管理器").clicked() {
                        gui.show_device_manager = true;
                    }
                });
            });
        });

    // 品牌三色条
    egui::TopBottomPanel::top("ef_tri")
        .exact_height(3.0)
        .frame(egui::Frame::NONE)
        .show_separator_line(false)
        .show(ctx, |ui| {
            let r = ui.max_rect();
            let p = ui.painter();
            for (i, c) in pal.tri.iter().enumerate() {
                let x = r.min.x + r.width() * i as f32 / 3.0;
                p.rect_filled(
                    Rect::from_min_size(egui::pos2(x, r.min.y), egui::vec2(r.width() / 3.0, 3.0)),
                    0.0,
                    *c,
                );
            }
        });

    if gui.ef_mode == UiMode::Full {
        egui::TopBottomPanel::top("ef_nav")
            .exact_height(46.0)
            .frame(
                egui::Frame::NONE
                    .fill(pal.bg)
                    .inner_margin(egui::Margin::symmetric(18, 8)),
            )
            .show_separator_line(false)
            .show(ctx, |ui| {
                egui::ScrollArea::horizontal().show(ui, |ui| {
                    ui.horizontal(|ui| {
                        for page in SCHEMA.pages {
                            let here = page.id == gui.ef_route.id();
                            if flat_btn(
                                ui,
                                &pal,
                                &format!("{}  {}", page.glyph, page.zh),
                                here,
                            )
                            .on_hover_text(page.hint)
                            .clicked()
                            {
                                gui.ef_route = EfRoute::parse(page.id);
                            }
                            ui.add_space(4.0);
                        }
                    });
                });
            });
    }

    egui::TopBottomPanel::bottom("ef_bottom")
        .exact_height(56.0)
        .frame(
            egui::Frame::NONE
                .fill(pal.paper)
                .inner_margin(egui::Margin::symmetric(18, 10)),
        )
        .show_separator_line(false)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("CMD").size(9.0).color(pal.ink_dim));
                for act in SCHEMA.actions {
                    if flat_btn(ui, &pal, act.label, false).clicked() {
                        cmds.push(act.cmd);
                    }
                    ui.add_space(3.0);
                }
                ui.separator();
                for m in SCHEMA.modes {
                    if flat_btn(ui, &pal, m.label(), gui.ef_mode == *m).clicked() {
                        cmds.push(m.command());
                    }
                    ui.add_space(3.0);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        RichText::new(format!("{} / {}", gui.ef_mode.label(), gui.ef_route.title()))
                            .size(10.0)
                            .color(pal.ink_dim),
                    );
                });
            });
        });

    egui::CentralPanel::default()
        .frame(
            egui::Frame::NONE
                .fill(pal.bg)
                .inner_margin(egui::Margin::same(14)),
        )
        .show(ctx, |ui| {
            match gui.ef_mode {
                UiMode::Minimal => minimal(ui, gui, &pal),
                UiMode::TurboOnly => {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new("纯连发 · 只保留连发映射")
                                .size(12.0)
                                .strong()
                                .color(pal.ink),
                        );
                    });
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        gui.render_turbo_page(ui, ctx, fs);
                    });
                }
                UiMode::Full => match gui.ef_route {
                    EfRoute::Overview => overview(ui, gui, &pal),
                    EfRoute::Settings => settings_page(ui, gui, &pal),
                    EfRoute::Logs => logs_page(ui, &pal),
                    route => page_content(ui, gui, ctx, fs, &pal, route),
                },
            }

            // 轻量 toast：命令结果 / 模式切换反馈
            let toast = gui.ef_toast.clone();
            if let Some((msg, at)) = toast {
                if at.elapsed().as_secs_f32() < 2.4 {
                    let r = Rect::from_center_size(
                        egui::pos2(ui.max_rect().center().x, ui.max_rect().max.y - 26.0),
                        egui::vec2(260.0, 28.0),
                    );
                    ui.painter().rect_filled(r, 2.0, pal.accent);
                    ui.painter().text(
                        r.center(),
                        Align2::CENTER_CENTER,
                        msg,
                        FontId::proportional(12.0),
                        pal.paper,
                    );
                } else {
                    gui.ef_toast = None;
                }
            }
        });

    for cmd in cmds {
        let reply = gui.dispatch(cmd);
        gui.ef_toast = Some((reply.message, Instant::now()));
    }
    if open_settings {
        gui.open_settings_dialog();
    }
    if close_dialog {
        gui.show_close_dialog = true;
    }
}

/// 概览：左运行/输入、中央圆盘、右白名单/震动，下方映射速览 + 日志 + 快捷操作。
fn overview(ui: &mut egui::Ui, gui: &mut SorahkGui, pal: &EfPalette) {
    let snap: UiSnapshot = gui.snapshot();
    let pad = gui.gamepad_texture.clone();
    let current = gui.ef_route.id().to_string();
    let mut go: Option<String> = None;
    let mut cmds: Vec<UiCommand> = Vec::new();

    ui.columns(3, |cols| {
        cols[0].vertical(|ui| {
            panel(ui, pal, "RUNTIME", "运行", |ui| {
                stat(ui, pal, format!("{}", snap.mappings), "映射");
                stat(ui, pal, format!("{}", snap.turbo), "连发");
                line(ui, pal, "运行态", if snap.paused { "暂停".into() } else { "正常".into() });
                line(ui, pal, "worker", format!("{}", snap.worker_count));
            });
            ui.add_space(8.0);
            panel(ui, pal, "INPUT", "输入", |ui| {
                line(ui, pal, "切换键", gui.config.switch_key.clone());
                line(ui, pal, "手柄", "RawInput / XInput".into());
                line(ui, pal, "预设", format!("{} 套", snap.presets));
            });
        });

        dial::draw(
            &mut cols[1],
            pal,
            &SCHEMA.pages[1..],
            &current,
            pad.as_ref(),
            &mut go,
        );

        cols[2].vertical(|ui| {
            panel(ui, pal, "WHITELIST", "白名单", |ui| {
                stat(ui, pal, format!("{}", snap.whitelist), "条目");
                line(ui, pal, "过滤", if gui.config.whitelist_enabled { "开启".into() } else { "关闭".into() });
            });
            ui.add_space(8.0);
            panel(ui, pal, "HAPTICS", "震动", |ui| {
                line(ui, pal, "震动", if snap.vibration_enabled { "开启".into() } else { "关闭".into() });
                line(ui, pal, "音频震动", if snap.audio_enabled { "开启".into() } else { "关闭".into() });
                line(ui, pal, "版本", snap.version.clone());
            });
        });
    });

    ui.add_space(10.0);
    ui.columns(3, |cols| {
        panel(&mut cols[0], pal, "MAPPINGS", "映射速览", |ui| {
            for m in gui.config.mappings.iter().take(4) {
                line(ui, pal, &m.trigger_key, if m.turbo_enabled { "连发".into() } else { "单次".into() });
            }
            if flat_btn(ui, pal, "全部映射", false).clicked() {
                go = Some("gamepad".into());
            }
        });
        panel(&mut cols[1], pal, "LOGS", "最近日志", |ui| {
            for l in recent_logs(4) {
                ui.label(RichText::new(l).size(10.0).color(pal.ink_dim));
            }
            if flat_btn(ui, pal, "全部日志", false).clicked() {
                go = Some("logs".into());
            }
        });
        panel(&mut cols[2], pal, "QUICK", "快捷操作", |ui| {
            if flat_btn(ui, pal, "试震 1 秒", false).clicked() {
                cmds.push(UiCommand::TestVibration);
            }
            if flat_btn(ui, pal, "音频自检", false).clicked() {
                cmds.push(UiCommand::AudioSelfCheck);
            }
            if flat_btn(ui, pal, "重连手柄", false).clicked() {
                cmds.push(UiCommand::ResetDevices);
            }
            if flat_btn(ui, pal, "保存配置", false).clicked() {
                cmds.push(UiCommand::Save);
            }
        });
    });

    if let Some(id) = go {
        gui.ef_route = EfRoute::parse(&id);
    }
    for cmd in cmds {
        let reply = gui.dispatch(cmd);
        gui.ef_toast = Some((reply.message, Instant::now()));
    }
}

fn page_content(
    ui: &mut egui::Ui,
    gui: &mut SorahkGui,
    ctx: &egui::Context,
    fs: &FrameState,
    pal: &EfPalette,
    route: EfRoute,
) {
    let page = SCHEMA.page(route.id());
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(page.map(|p| p.zh).unwrap_or(""))
                .size(18.0)
                .strong()
                .color(pal.ink),
        );
        ui.label(
            RichText::new(page.map(|p| p.en).unwrap_or(""))
                .size(10.0)
                .color(pal.ink_dim),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                RichText::new(page.map(|p| p.hint).unwrap_or(""))
                    .size(10.5)
                    .color(pal.ink_dim),
            );
        });
    });
    ui.add_space(6.0);
    egui::Frame::NONE
        .fill(pal.panel)
        .stroke(Stroke::new(1.0, pal.line))
        .inner_margin(egui::Margin::same(12))
        .show(ui, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink([false; 2])
                .show(ui, |ui| match route {
                    EfRoute::Gamepad => gui.render_gamepad_page(ui, ctx),
                    EfRoute::Turbo => gui.render_turbo_page(ui, ctx, fs),
                    EfRoute::Whitelist => gui.render_whitelist_page(ui),
                    EfRoute::Vibration => gui.render_vibration_full_page(ui, fs, false),
                    EfRoute::JobPresets => gui.render_vibration_full_page(ui, fs, true),
                    EfRoute::Audio => gui.render_audio_haptics_page(ui, ctx),
                    _ => {}
                });
        });
}

fn settings_page(ui: &mut egui::Ui, gui: &mut SorahkGui, pal: &EfPalette) {
    let mut cmds: Vec<UiCommand> = Vec::new();
    panel(ui, pal, "SETTINGS", "常用开关与自检", |ui| {
        ui.label(
            RichText::new("完整设置请点顶栏齿轮；这里放最常用的全局动作。")
                .size(11.0)
                .color(pal.ink_dim),
        );
        ui.add_space(8.0);
        ui.horizontal_wrapped(|ui| {
            for cmd in [
                UiCommand::ToggleTheme,
                UiCommand::Save,
                UiCommand::CheckUpdate,
                UiCommand::ResetDevices,
                UiCommand::RestartThreads,
                UiCommand::TestVibration,
                UiCommand::TestRank,
                UiCommand::AudioCalibrate,
                UiCommand::ShowGuide,
            ] {
                if flat_btn(ui, pal, cmd.label(), false).clicked() {
                    cmds.push(cmd);
                }
                ui.add_space(4.0);
            }
        });
    });
    for cmd in cmds {
        let reply = gui.dispatch(cmd);
        gui.ef_toast = Some((reply.message, Instant::now()));
    }
}

fn logs_page(ui: &mut egui::Ui, pal: &EfPalette) {
    panel(ui, pal, "LOGS", "运行日志", |ui| {
        egui::ScrollArea::vertical()
            .auto_shrink([false; 2])
            .show(ui, |ui| {
                for l in recent_logs(160) {
                    ui.label(RichText::new(l).size(10.5).monospace().color(pal.ink));
                }
            });
    });
}

fn recent_logs(limit: usize) -> Vec<String> {
    let Ok(text) = std::fs::read_to_string("SorahkDFO_vib.log") else {
        return vec!["（暂无日志）".into()];
    };
    let mut lines: Vec<String> = text
        .lines()
        .rev()
        .take(limit)
        .map(|s| s.chars().take(160).collect())
        .collect();
    lines.reverse();
    lines
}

/// 极简模式：只留总开关与最常用动作，切换时有提示音 + 轻震动反馈。
fn minimal(ui: &mut egui::Ui, gui: &mut SorahkGui, pal: &EfPalette) {
    let mut cmds: Vec<UiCommand> = Vec::new();
    ui.vertical_centered(|ui| {
        ui.add_space(28.0);
        ui.label(RichText::new("极简模式").size(24.0).strong().color(pal.ink));
        ui.label(
            RichText::new("MINIMAL · 音频提示与震动反馈已开启")
                .size(10.0)
                .color(pal.ink_dim),
        );
        ui.add_space(18.0);
        egui::Frame::NONE
            .fill(pal.panel)
            .stroke(Stroke::new(1.0, pal.line))
            .inner_margin(egui::Margin::same(20))
            .show(ui, |ui| {
                ui.set_min_width(360.0);
                for cmd in [
                    UiCommand::TogglePause,
                    UiCommand::TestVibration,
                    UiCommand::AudioSelfCheck,
                    UiCommand::Save,
                ] {
                    if ui
                        .add(
                            egui::Button::new(
                                RichText::new(cmd.label()).size(15.0).color(pal.ink),
                            )
                            .fill(pal.panel_alt)
                            .stroke(Stroke::new(1.0, pal.line))
                            .min_size(egui::vec2(320.0, 44.0)),
                        )
                        .clicked()
                    {
                        cmds.push(cmd);
                    }
                    ui.add_space(8.0);
                }
                ui.add_space(6.0);
                if flat_btn(ui, pal, "返回完整界面", false).clicked() {
                    cmds.push(UiCommand::ModeFull);
                }
            });
    });
    for cmd in cmds {
        let reply = gui.dispatch(cmd);
        gui.ef_toast = Some((reply.message, Instant::now()));
    }
}
