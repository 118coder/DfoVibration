//! ★v24.55 检查更新界面 —— 右下角淡入淡出小浮窗 + 手动更新面板 + 设置区入口。
//!
//! 三条硬约束 (Owner 需求原文): 有新版本只弹小浮窗、自动淡出、**不强制更新**。
//! 因此这里没有任何 Modal 遮罩、没有"必须更新才能继续"的分支; 浮窗 16 秒后自己
//! 淡掉, 用户点 ✕ 立刻消失, 之后的入口留在「设置 → 检查更新」里, 想找才找得到。
//!
//! 面板里两个发行包并列展示, 点哪个下哪个:
//!   - 基础引擎 (无音频震动, ~10MB) —— 只要连发/映射/DLL 震动的人
//!   - 完全版 (音频震动 + 后续全部 MOD, ~49MB) —— 全功能
//! 下载只落盘到 `updates\` 目录并打开文件夹, **绝不自动替换正在运行的 exe**。

use std::time::{Duration, Instant};

use eframe::egui;

use crate::gui::SorahkGui;
use crate::update::{APP_VERSION, Channel, Phase, Snapshot};

const TOAST_W: f32 = 340.0;
const TOAST_FADE_IN: f32 = 0.30;
const TOAST_HOLD: f32 = 16.0;
const TOAST_FADE_OUT: f32 = 0.60;

/// 按比例调透明度 (egui 颜色是预乘的, 必须走 unmultiplied 重建, 否则会发黑)。
fn fade(c: egui::Color32, a: f32) -> egui::Color32 {
    egui::Color32::from_rgba_unmultiplied(
        c.r(),
        c.g(),
        c.b(),
        (c.a() as f32 * a.clamp(0.0, 1.0)) as u8,
    )
}

fn fmt_size(bytes: u64) -> String {
    if bytes == 0 {
        return "体积未知".to_string();
    }
    let mb = bytes as f64 / (1024.0 * 1024.0);
    if mb >= 1.0 {
        format!("{mb:.1} MB")
    } else {
        format!("{:.0} KB", bytes as f64 / 1024.0)
    }
}

fn open_in_explorer(path: &std::path::Path) {
    /* 文件 → 选中该文件; 目录 → 直接打开。explorer 的返回码不可信, 一律忽略。 */
    let _ = std::process::Command::new("explorer").arg(path).spawn();
}

fn ease_out(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t) * (1.0 - t)
}

impl SorahkGui {
    /* ═════════════════ 每帧驱动 ═════════════════ */

    /// 拉快照 + 启动自动检查 + 触发浮窗 (逻辑与绘制分离, 绘制在 `render_update_overlay`)。
    pub(crate) fn tick_update(&mut self) {
        if !self.update_started {
            self.update_started = true;
            /* 启动自动检查: 只在开关打开且距上次成功超过 4 小时时跑 (省流量, 也避免
             * 频繁启动时把 GitHub 的匿名额度烧光)。失败不提示 —— 静默。 */
            if self.config.update.auto_check
                && crate::update::auto_check_due(self.config.update.last_check_unix)
            {
                self.update
                    .check(self.config.update.manifest_url.clone());
            }
        }

        let snap = self.update.snapshot();

        /* 成功检查的时间戳回写配置 (节流用; 只在变化时落盘) */
        if snap.last_check_unix > 0 && snap.last_check_unix != self.config.update.last_check_unix {
            self.config.update.last_check_unix = snap.last_check_unix;
            let _ = self.config.save_to_file("Config.toml");
        }

        /* 发现更新的浮窗: 同一版本每个会话只弹一次, 且尊重"忽略此版本" */
        if let Some(rel) = &snap.latest
            && crate::update::is_newer(&rel.version, APP_VERSION)
            && rel.version != self.config.update.skipped_version
            && self.update_toast_ver.as_deref() != Some(rel.version.as_str())
        {
            self.update_toast_ver = Some(rel.version.clone());
            self.update_toast_at = Some(Instant::now());
        }

        self.update_state = snap;
    }

    /// 浮窗 + 面板 (叠在主界面之上, 不拦截主界面操作)。
    pub(crate) fn render_update_overlay(&mut self, ctx: &egui::Context) {
        self.render_update_toast(ctx);
        if self.show_update_dialog {
            self.render_update_dialog(ctx);
        }
    }

    /* ═════════════════ 右下角小浮窗 ═════════════════ */

    fn render_update_toast(&mut self, ctx: &egui::Context) {
        let Some(started) = self.update_toast_at else {
            return;
        };
        /* 面板已打开 / 正在下载 / 已忽略该版本 → 浮窗没有存在意义, 直接收起 */
        if self.show_update_dialog {
            self.update_toast_at = None;
            return;
        }
        let Some(rel) = self.update_state.latest.clone() else {
            self.update_toast_at = None;
            return;
        };
        if rel.version == self.config.update.skipped_version
            || self.update_state.phase == Phase::Downloading
        {
            self.update_toast_at = None;
            return;
        }

        let t = started.elapsed().as_secs_f32();
        let (alpha, expired) = if t < TOAST_FADE_IN {
            (ease_out(t / TOAST_FADE_IN), false)
        } else if t < TOAST_FADE_IN + TOAST_HOLD {
            (1.0, false)
        } else if t < TOAST_FADE_IN + TOAST_HOLD + TOAST_FADE_OUT {
            (
                1.0 - (t - TOAST_FADE_IN - TOAST_HOLD) / TOAST_FADE_OUT,
                false,
            )
        } else {
            (0.0, true)
        };
        if expired {
            self.update_toast_at = None;
            return;
        }

        let th = self.theme();
        let screen = ctx.screen_rect();
        let dy = (1.0 - alpha) * 14.0;
        let anchor = egui::pos2(screen.right() - 18.0, screen.bottom() - 18.0 + dy);

        let mut open_dialog = false;
        let mut dismiss = false;
        let mut hovered = false;

        egui::Area::new(egui::Id::new("update_toast_area"))
            .order(egui::Order::Foreground)
            .pivot(egui::Align2::RIGHT_BOTTOM)
            .fixed_pos(anchor)
            .show(ctx, |ui| {
                egui::Frame::NONE
                    .fill(fade(th.card, alpha * 0.98))
                    .stroke(egui::Stroke::new(1.0, fade(th.accent, alpha * 0.55)))
                    .corner_radius(egui::CornerRadius::same(14))
                    .inner_margin(egui::Margin::same(14))
                    .shadow(egui::epaint::Shadow {
                        offset: [0, 6],
                        blur: 20,
                        spread: 1,
                        color: egui::Color32::from_rgba_unmultiplied(
                            0,
                            0,
                            0,
                            (60.0 * alpha) as u8,
                        ),
                    })
                    .show(ui, |ui| {
                        ui.set_width(TOAST_W);
                        ui.horizontal(|ui| {
                            let (dot_rect, _) =
                                ui.allocate_exact_size(egui::vec2(9.0, 9.0), egui::Sense::hover());
                            ui.painter().circle_filled(
                                dot_rect.center(),
                                4.5,
                                fade(th.accent, alpha),
                            );
                            ui.label(
                                egui::RichText::new(format!("发现新版本 v{}", rel.version))
                                    .size(14.0)
                                    .strong()
                                    .color(fade(th.title, alpha)),
                            );
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    let x = ui.add(
                                        egui::Button::new(
                                            egui::RichText::new("✕")
                                                .size(12.0)
                                                .color(fade(th.hint, alpha)),
                                        )
                                        .frame(false),
                                    );
                                    if x.clicked() {
                                        dismiss = true;
                                    }
                                },
                            );
                        });
                        ui.add_space(6.0);
                        ui.label(
                            egui::RichText::new("不影响当前使用 · 基础引擎 / 完全版均可选")
                                .size(12.0)
                                .color(fade(th.text_weak, alpha)),
                        );
                        ui.add_space(10.0);
                        ui.horizontal(|ui| {
                            let primary = egui::Button::new(
                                egui::RichText::new("查看更新")
                                    .size(13.0)
                                    .strong()
                                    .color(fade(egui::Color32::WHITE, alpha)),
                            )
                            .fill(fade(th.btn_primary, alpha))
                            .corner_radius(10.0);
                            if ui.add_sized([96.0, 30.0], primary).clicked() {
                                open_dialog = true;
                            }
                            let later = egui::Button::new(
                                egui::RichText::new("稍后")
                                    .size(13.0)
                                    .color(fade(th.text_weak, alpha)),
                            )
                            .fill(fade(th.faint, alpha))
                            .corner_radius(10.0);
                            if ui.add_sized([72.0, 30.0], later).clicked() {
                                dismiss = true;
                            }
                        });
                        let rect = ui.min_rect();
                        hovered = ui
                            .ctx()
                            .pointer_hover_pos()
                            .map(|p| rect.contains(p))
                            .unwrap_or(false);
                    });
            });

        if open_dialog {
            self.show_update_dialog = true;
            self.update_toast_at = None;
            return;
        }
        if dismiss {
            self.update_toast_at = None;
            return;
        }
        /* 鼠标停在浮窗上 → 重新计时, 免得读到一半淡掉 */
        if hovered && t > TOAST_FADE_IN + TOAST_HOLD - 1.0 {
            self.update_toast_at = Some(Instant::now());
        }
        ctx.request_repaint_after(Duration::from_millis(16));
    }

    /* ═════════════════ 更新面板 ═════════════════ */

    pub(crate) fn render_update_dialog(&mut self, ctx: &egui::Context) {
        let th = self.theme();
        let snap: Snapshot = self.update_state.clone();
        let current = APP_VERSION.to_string();
        let latest = snap.latest.clone();
        let newer = latest
            .as_ref()
            .map(|r| crate::update::is_newer(&r.version, &current))
            .unwrap_or(false);
        let busy_download = snap.phase == Phase::Downloading;
        let busy_check = snap.phase == Phase::Checking;

        let mut close = false;

        egui::Window::new("update_dialog")
            .id(egui::Id::new("update_dialog_window"))
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .fixed_size([560.0, 540.0])
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .frame(
                egui::Frame::window(&ctx.style())
                    .fill(th.surface)
                    .corner_radius(egui::CornerRadius::same(14))
                    .stroke(egui::Stroke::NONE)
                    .shadow(egui::epaint::Shadow {
                        offset: [0, 6],
                        blur: 24,
                        spread: 2,
                        color: egui::Color32::from_rgba_premultiplied(0, 0, 0, 50),
                    }),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("检查更新").size(18.0).strong().color(th.title));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add(egui::Button::new("✕").frame(false))
                            .on_hover_text("关闭")
                            .clicked()
                        {
                            close = true;
                        }
                    });
                });
                ui.add_space(4.0);
                ui.label(th.hint_text(
                    "只提示、不强制。下载由你决定，程序不会自动替换正在运行的 exe。",
                ));
                ui.add_space(8.0);
                ui.separator();

                /* ── 版本对比行 ── */
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(format!("当前 v{current}"))
                            .size(13.0)
                            .color(th.text_weak),
                    );
                    ui.label(egui::RichText::new("→").size(13.0).color(th.hint));
                    match (&latest, newer) {
                        (Some(rel), true) => {
                            ui.label(
                                egui::RichText::new(format!("最新 v{}", rel.version))
                                    .size(13.0)
                                    .strong()
                                    .color(th.accent),
                            );
                            th.badge(ui, "有新版本", th.good, th.good_soft);
                            if !rel.published.is_empty() {
                                ui.label(th.hint_text(format!("发布于 {}", rel.published)));
                            }
                        }
                        (Some(rel), false) => {
                            ui.label(
                                egui::RichText::new(format!("最新 v{}", rel.version))
                                    .size(13.0)
                                    .color(th.text_weak),
                            );
                            th.badge(ui, "已是最新", th.text_weak, th.faint);
                        }
                        (None, _) => {
                            ui.label(
                                egui::RichText::new(if busy_check {
                                    "正在获取版本信息…"
                                } else {
                                    "尚未获取到版本信息"
                                })
                                .size(13.0)
                                .color(th.text_weak),
                            );
                        }
                    }
                });

                /* ── 检查失败提示 (不阻断任何操作) ── */
                if let Some(err) = &snap.error {
                    ui.add_space(8.0);
                    egui::Frame::NONE
                        .fill(th.bad_soft)
                        .corner_radius(egui::CornerRadius::same(8))
                        .inner_margin(egui::Margin::same(10))
                        .show(ui, |ui| {
                            ui.set_min_width(ui.available_width());
                            ui.label(
                                egui::RichText::new(format!("检查失败：{err}"))
                                    .size(12.0)
                                    .color(th.bad),
                            );
                            ui.label(th.hint_text(
                                "无网络/被拦截都属正常，不影响软件使用。可在设置里填自定义清单地址（镜像）。",
                            ));
                        });
                }

                /* ── 更新说明 ── */
                if let Some(rel) = &latest
                    && newer
                {
                    ui.add_space(8.0);
                    th.panel(ui, Some("更新说明"), |ui| {
                        egui::ScrollArea::vertical()
                            .max_height(110.0)
                            .auto_shrink([false, true])
                            .show(ui, |ui| {
                                let body = if rel.notes.trim().is_empty() {
                                    "（该版本未提供说明）"
                                } else {
                                    rel.notes.trim()
                                };
                                ui.label(
                                    egui::RichText::new(body).size(12.0).color(th.text),
                                );
                            });
                    });

                    /* ── 两个发行包 ── */
                    ui.add_space(8.0);
                    ui.label(egui::RichText::new("选择要下载的发行包").size(13.0).strong().color(th.heading));
                    ui.add_space(6.0);
                    for ch in [Channel::Full, Channel::Basic] {
                        let Some(pkg) = rel.package(ch) else {
                            continue;
                        };
                        let is_active = snap.active == Some(ch) && busy_download;
                        let mut start = false;
                        egui::Frame::NONE
                            .fill(if is_active { th.accent_soft } else { th.card_alt })
                            .stroke(egui::Stroke::new(1.0, th.stroke))
                            .corner_radius(egui::CornerRadius::same(10))
                            .inner_margin(egui::Margin::same(10))
                            .show(ui, |ui| {
                                ui.set_min_width(ui.available_width());
                                ui.horizontal(|ui| {
                                    ui.vertical(|ui| {
                                        ui.label(
                                            egui::RichText::new(&pkg.label)
                                                .size(13.0)
                                                .strong()
                                                .color(th.heading),
                                        );
                                        let detail = match ch {
                                            Channel::Full => format!(
                                                "{} · 音频震动引擎内嵌（无需模型文件），开箱全功能",
                                                fmt_size(pkg.size)
                                            ),
                                            Channel::Basic => format!(
                                                "{} · 旧版遗留渠道，已停发",
                                                fmt_size(pkg.size)
                                            ),
                                        };
                                        ui.label(th.hint_text(detail));
                                    });
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            let btn = egui::Button::new(
                                                egui::RichText::new("下载")
                                                    .size(13.0)
                                                    .strong()
                                                    .color(egui::Color32::WHITE),
                                            )
                                            .fill(if busy_download { th.faint } else { th.btn_primary })
                                            .corner_radius(9.0);
                                            if ui
                                                .add_enabled(
                                                    !busy_download && !busy_check,
                                                    btn,
                                                )
                                                .clicked()
                                            {
                                                start = true;
                                            }
                                        },
                                    );
                                });
                            });
                        if start {
                            let dir = crate::update::resolve_download_dir(
                                &self.config.update.download_dir,
                            );
                            self.config.update.channel = ch.key().to_string();
                            let _ = self.config.save_to_file("Config.toml");
                            self.update
                                .download(rel.version.clone(), pkg.clone(), dir);
                        }
                        ui.add_space(6.0);
                    }
                }

                /* ── 下载进度 / 结果 ── */
                ui.add_space(4.0);
                match snap.phase {
                    Phase::Downloading => {
                        let frac = if snap.total > 0 {
                            (snap.done as f32 / snap.total as f32).clamp(0.0, 1.0)
                        } else {
                            0.0
                        };
                        ui.horizontal(|ui| {
                            let bar = egui::ProgressBar::new(frac).text(format!(
                                "{} / {}",
                                fmt_size(snap.done),
                                fmt_size(snap.total)
                            ));
                            ui.add_sized([ui.available_width() - 78.0, 16.0], bar);
                            if ui.button("取消").clicked() {
                                self.update.cancel_download();
                            }
                        });
                    }
                    Phase::Downloaded => {
                        if let Some(path) = &snap.saved {
                            egui::Frame::NONE
                                .fill(th.good_soft)
                                .corner_radius(egui::CornerRadius::same(8))
                                .inner_margin(egui::Margin::same(10))
                                .show(ui, |ui| {
                                    ui.set_min_width(ui.available_width());
                                    ui.label(
                                        egui::RichText::new(format!(
                                            "已下载：{}",
                                            path.file_name()
                                                .map(|n| n.to_string_lossy().to_string())
                                                .unwrap_or_default()
                                        ))
                                        .size(12.0)
                                        .strong()
                                        .color(th.good),
                                    );
                                    ui.label(th.hint_text(
                                        "放到任意位置解压，退出本程序后用新 exe 覆盖旧的即可。Config.toml / Vibration.toml 不必替换。",
                                    ));
                                    ui.horizontal(|ui| {
                                        if let Some(dir) = path.parent()
                                            && ui.button("打开所在文件夹").clicked()
                                        {
                                            open_in_explorer(dir);
                                        }
                                    });
                                });
                        }
                    }
                    _ => {}
                }

                /* ── 底部操作区 ── */
                ui.add_space(6.0);
                ui.separator();
                ui.horizontal(|ui| {
                    let mut auto = self.config.update.auto_check;
                    if ui
                        .checkbox(&mut auto, "启动时自动检查")
                        .on_hover_text("4 小时内最多检查一次；关闭后仍可点「立即检查」")
                        .changed()
                    {
                        self.config.update.auto_check = auto;
                        let _ = self.config.save_to_file("Config.toml");
                    }

                    if ui
                        .add_enabled(!busy_check && !busy_download, egui::Button::new("立即检查"))
                        .clicked()
                    {
                        self.update
                            .check(self.config.update.manifest_url.clone());
                    }

                    if let Some(rel) = &latest
                        && newer
                        && ui
                            .button("忽略此版本")
                            .on_hover_text("本版本不再弹浮窗；换新版本或手动检查时仍会显示")
                            .clicked()
                    {
                        self.config.update.skipped_version = rel.version.clone();
                        let _ = self.config.save_to_file("Config.toml");
                        self.update_toast_at = None;
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("关闭").clicked() {
                            close = true;
                        }
                        if let Some(src) = &snap.source {
                            ui.label(th.hint_text(
                                if src.contains("githubusercontent") || src.contains("github") {
                                    "源: GitHub"
                                } else {
                                    "源: 镜像"
                                },
                            ))
                            .on_hover_text(src.as_str());
                        }
                    });
                });
                ui.add_space(4.0);
            });

        if close {
            self.show_update_dialog = false;
        }
    }

    /* ═════════════════ 设置面板入口 ═════════════════ */

    /// 「设置 → 检查更新」区 (与白名单区同口径: 直接改 self.config 并即时落盘)。
    pub(super) fn settings_section_update(&mut self, ui: &mut egui::Ui) {
        let th = self.theme();
        let snap = self.update_state.clone();
        let newer = snap
            .latest
            .as_ref()
            .map(|r| crate::update::is_newer(&r.version, APP_VERSION))
            .unwrap_or(false);
        let busy = matches!(snap.phase, Phase::Checking | Phase::Downloading);

        ui.add_space(12.0);
        th.panel(ui, Some("检查更新"), |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(format!("当前版本 {APP_VERSION}"))
                        .size(13.0)
                        .strong()
                        .color(th.heading),
                );
                match (&snap.latest, newer) {
                    (Some(rel), true) => {
                        ui.label(
                            egui::RichText::new(format!("最新 v{}", rel.version))
                                .size(12.0)
                                .color(th.accent),
                        );
                        th.badge(ui, "有新版本", th.good, th.good_soft);
                    }
                    (Some(_), false) => {
                        th.badge(ui, "已是最新", th.text_weak, th.faint);
                    }
                    (None, _) => {
                        ui.label(th.hint_text(if busy {
                            "正在检查…"
                        } else {
                            "未检查过"
                        }));
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("打开更新面板").clicked() {
                        self.show_update_dialog = true;
                    }
                    if ui
                        .add_enabled(!busy, egui::Button::new("立即检查"))
                        .clicked()
                    {
                        self.update
                            .check(self.config.update.manifest_url.clone());
                    }
                });
            });

            ui.add_space(4.0);
            let mut auto = self.config.update.auto_check;
            if ui
                .checkbox(&mut auto, "启动时自动检查（只弹提示，不强制更新）")
                .changed()
            {
                self.config.update.auto_check = auto;
                let _ = self.config.save_to_file("Config.toml");
            }

            ui.add_space(2.0);
            ui.horizontal(|ui| {
                ui.label(th.hint_text("自定义清单地址"));
                let mut url = self.config.update.manifest_url.clone();
                let edit = ui.add(
                    egui::TextEdit::singleline(&mut url)
                        .hint_text("留空 = 内置源（jsDelivr / GitHub）")
                        .desired_width(300.0),
                );
                if edit.changed() {
                    self.config.update.manifest_url = url;
                    self.audio_params_dirty_since = Some(std::time::Instant::now());
                }
            });
            if let Some(err) = &snap.error {
                ui.label(egui::RichText::new(format!("上次检查失败：{err}")).size(11.0).color(th.bad));
            }
        });
    }
}
