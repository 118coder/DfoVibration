//! ★G 轮外观 MOD 页 (契约 v1.4 §11): egui 侧的 MOD 清单与应用/停用入口。
//!
//! * 列表 = 内置包 (终末地) + 外部 `mods/<id>/` 扫描 (复用 shell::mods);
//! * 「应用」→ `Hybrid::apply`: 无窗 = 拉 mod 窗线程 + 本窗口隐藏到托盘;
//!   已有窗 = 换 URL 不重开线程;
//! * 「停用」→ `Hybrid::request_stop`: 关 mod 窗 → mod 线程收尾还原 egui
//!   (Win32 直连 + 本页帧头 ViewportCommand 双保险);
//! * 原生 egui 与 mod 窗共享同一 `Arc<BackendService>` (配置/运行态单源)。

use std::time::{Duration, Instant};

use eframe::egui;

use crate::gui::SorahkGui;
use crate::gui::theme::{self, Theme};
use crate::gui::widgets;
use crate::shell::hybrid;
use crate::shell::mods::{self, ModMeta};

/// MOD 清单缓存有效期 (页内低频刷新; 手动「刷新」按钮即时重扫)。
const CACHE_TTL: Duration = Duration::from_secs(3);

/// 内置 + 外部 MOD 全量扫描 (shell 会话与 egui 页共用同一扫描口径)。
pub(crate) fn scan_all_mods() -> Vec<ModMeta> {
    let mut all = mods::builtin_mods();
    let ext_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()));
    if let Some(dir) = ext_dir {
        all.extend(mods::scan_external(&mods::mods_root_for_exe_dir(&dir)));
    }
    all
}

/// ★K 轮: .dvmod 导入结果三分 (取消/成功/失败)。
enum ImportOutcome {
    Cancelled,
    Installed(crate::shell::dvmod::InstallReport),
    Failed(String),
}

/// ★K 轮: .dvmod 导入 (egui 入口) —— 原生文件对话框选包 → 安全校验/解包安装。
/// 对话框阻塞 (与白名单页「浏览…」同款模式)。
fn pick_and_install_dvmod() -> ImportOutcome {
    let Some(path) = rfd::FileDialog::new()
        .set_title("选择 .dvmod 外观 MOD 包")
        .add_filter("DfoVibration MOD 包", &[crate::shell::dvmod::DVMOD_EXT])
        .pick_file()
    else {
        return ImportOutcome::Cancelled;
    };
    let dir = match std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
    {
        Some(d) => d,
        None => return ImportOutcome::Failed("无法定位 exe 目录, .dvmod 导入不可用".into()),
    };
    match crate::shell::dvmod::install_from_file(&path, &mods::mods_root_for_exe_dir(&dir)) {
        Ok(report) => ImportOutcome::Installed(report),
        Err(e) => ImportOutcome::Failed(e),
    }
}

impl SorahkGui {
    /// 外观 MOD 页 (侧栏导航最后一项; Page::AppearanceMods)。
    pub(crate) fn render_appearance_page(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let th = self.theme();

        /* 清单缓存过期 → 重扫 (3s 一次; 扫描是纯文件读, 代价可忽略) */
        let stale = self
            .mods_cache_at
            .map(|t| t.elapsed() > CACHE_TTL)
            .unwrap_or(true);
        if stale {
            self.mods_cache = scan_all_mods();
            self.mods_cache_at = Some(Instant::now());
        }

        /* ★K 轮: 「导入 .dvmod」点击标志 (对话框阻塞, 借用须先出闭包再处理) */
        let mut do_import = false;

        /* 卡1: 当前界面状态 (原生 / MOD 运行中) + 停用入口 */
        let active_id = self.hybrid.active_id();
        th.card_with_actions(ui, Some("当前界面"), |_| {}, |ui| {
            match active_id.as_deref() {
                Some(id) => {
                    let name = self
                        .mods_cache
                        .iter()
                        .find(|m| m.id == id)
                        .map(|m| m.name.clone())
                        .unwrap_or_else(|| id.to_string());
                    th.badge(ui, "MOD 运行中", th.accent_text, th.accent_soft);
                    ui.label(
                        egui::RichText::new(format!("当前外观:「{name}」"))
                            .size(13.0)
                            .color(th.heading),
                    );
                    ui.add_space(theme::SP_XS);
                    ui.label(th.hint_text(
                        "本窗口已收进托盘, 由 MOD 界面接管。点「停用」或关闭 MOD 窗口即还原原生界面。",
                    ));
                    ui.add_space(theme::SP_S);
                    if ui
                        .add(th.secondary_button("停用并还原原生界面"))
                        .clicked()
                    {
                        let _ = self.hybrid.request_stop();
                    }
                }
                None => {
                    th.badge(ui, "原生界面", th.good, th.good_soft);
                    ui.label(
                        egui::RichText::new("当前外观: 原生 (本窗口)")
                            .size(13.0)
                            .color(th.heading),
                    );
                    ui.add_space(theme::SP_XS);
                    ui.label(th.hint_text(
                        "点下方 MOD 卡片的「应用」即可切换: 本窗口收进托盘, 打开 MOD 专属界面窗口。",
                    ));
                }
            }
        });

        /* 错误提示条 (应用失败等) */
        if let Some(err) = self.mods_error.clone() {
            th.card(ui, None, |ui| {
                ui.horizontal(|ui| {
                    th.badge(ui, "提示", th.warn, th.warn_soft);
                    ui.label(th.weak(&err));
                    if widgets::icon_x(ui, &th).clicked() {
                        self.mods_error = None;
                    }
                });
            });
        }

        /* 卡2: MOD 列表 (内置标记 / 版本 / 作者 / 描述 / 应用停用钮 / 状态徽章) */
        let mut do_refresh = false;
        th.card_with_actions(
            ui,
            Some("可应用的 MOD"),
            |ui| {
                if ui.add(th.ghost_button("导入 .dvmod 包…")).clicked() {
                    do_import = true; // ★K 轮: 单文件格式包导入入口
                }
                if ui.add(th.ghost_button("刷新列表")).clicked() {
                    do_refresh = true; // 闭包借 ui/th, self 的写放卡外 (借用分离)
                }
            },
            |ui| {
                if self.mods_cache.is_empty() {
                    widgets::empty_state(
                        ui,
                        &th,
                        widgets::Icon::Palette,
                        "没有可用的外观 MOD",
                        &[
                            "点上方「导入 .dvmod 包…」安装单文件格式主题包",
                            "或把外部包目录放在程序同目录 mods/<id>/ 下 (含 mod.json 与入口文件)",
                        ],
                    );
                    return;
                }
                let mods_snapshot = self.mods_cache.clone();
                let active = self.hybrid.active_id();
                for meta in &mods_snapshot {
                    self.render_mod_card(ui, &th, ctx, meta, active.as_deref());
                }
                ui.add_space(theme::SP_XS);
                ui.label(th.hint_text(
                    "外部包: 双击 .dvmod 导入, 或目录形式 mods/<id>/{mod.json, 入口文件, preview.png?}; 格式见 docs/dvmod格式规范_v1.md",
                ));
            },
        );
        if do_import {
            match pick_and_install_dvmod() {
                ImportOutcome::Installed(report) => {
                    self.mods_cache = scan_all_mods();
                    self.mods_cache_at = Some(Instant::now());
                    self.backend.notify_info(format!(
                        "已导入 MOD「{}」(v{}) {}",
                        report.name,
                        report.version,
                        if report.replaced { "· 覆盖替换旧包" } else { "" }
                    ));
                }
                ImportOutcome::Cancelled => {}
                ImportOutcome::Failed(e) => {
                    self.mods_error = Some(e);
                }
            }
        }
        if do_refresh {
            self.mods_cache = scan_all_mods();
            self.mods_cache_at = Some(Instant::now());
        }
    }

    /// 单张 MOD 卡片 (标记/版本/作者/描述/应用停用/状态)。
    fn render_mod_card(
        &mut self,
        ui: &mut egui::Ui,
        th: &Theme,
        ctx: &egui::Context,
        meta: &ModMeta,
        active_id: Option<&str>,
    ) {
        let is_active = active_id == Some(meta.id.as_str());
        th.card(ui, None, |ui| {
            /* 行1: 名称 + 标记徽章 + 版本 */
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(&meta.name).size(14.0).strong().color(th.heading));
                th.badge(
                    ui,
                    if meta.builtin { "内置" } else { "外部" },
                    if meta.builtin { th.accent_text } else { th.text_weak },
                    if meta.builtin { th.accent_soft } else { th.faint },
                );
                th.badge(
                    ui,
                    if meta.kind == "overlay" { "overlay" } else { "shell" },
                    th.info,
                    th.faint,
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    th.badge(ui, &format!("v{}", meta.version), th.text_weak, th.faint);
                    if is_active {
                        th.badge(ui, "使用中", th.good, th.good_soft);
                    }
                });
            });
            ui.add_space(theme::SP_XS);

            /* 行2: 作者 / 引擎 / 描述 */
            ui.label(th.weak(format!(
                "作者 {} · 引擎 {} · id {}",
                if meta.author.is_empty() { "—" } else { &meta.author },
                meta.engine,
                meta.id
            )));
            if !meta.description.is_empty() {
                ui.label(th.hint_text(&meta.description));
            }
            if !meta.compatible {
                ui.label(
                    egui::RichText::new(format!(
                        "引擎不兼容 (声明 {}): 只可查看, 不可应用",
                        meta.engine
                    ))
                    .size(12.0)
                    .color(th.warn),
                );
            }
            ui.add_space(theme::SP_S);

            /* 行3: 应用 / 停用 按钮 */
            ui.horizontal(|ui| {
                if is_active {
                    if ui.add(th.secondary_button("停用")).clicked() {
                        let _ = self.hybrid.request_stop();
                    }
                } else if meta.compatible {
                    let applying = self.hybrid.is_active(); // 已有窗 = 换 URL, 语义不变
                    let label = if applying { "切换到此 MOD" } else { "应用" };
                    if ui.add(th.primary_button(label)).clicked() {
                        self.apply_mod_from_page(ctx, meta);
                    }
                } else {
                    let btn = egui::Button::new(
                        egui::RichText::new("不可应用").size(13.0).color(th.text_weak),
                    )
                    .fill(th.faint)
                    .corner_radius(egui::CornerRadius::same(theme::RADIUS_CTRL));
                    let _ = ui.add(btn).on_hover_text("引擎版本不兼容, 降级列表仅供查看");
                }
            });
        });
    }

    /// 页面「应用」入口 → Hybrid::apply (无窗 = 拉 mod 窗线程并隐藏本窗口;
    /// 已有窗 = 换 URL; 失败挂提示条)。见 shell::hybrid 与契约 §11。
    /// ★G 轮补充: `--open-mod <id>` 启动自动应用复用**同一路径** (pub(super) 供
    /// main_window 帧头延迟触发; 行为与按钮点击逐字节一致 —— 自动化验收通道前提)。
    pub(super) fn apply_mod_from_page(&mut self, ctx: &egui::Context, meta: &ModMeta) {
        crate::backend::BackendService::log(&format!("[hybrid] apply 请求: {} ({})", meta.id, meta.name));
        match self.hybrid.apply(&self.backend.clone(), meta) {
            Ok(hybrid::ApplyOutcome::WindowSpawned) => {
                // ① egui 隐藏到托盘 (进程与后端照常跑); mod 窗线程在后台拉起。
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
                self.backend.notify_info("外观 MOD 已应用, 原生窗口收进托盘");
            }
            Ok(hybrid::ApplyOutcome::Navigated) => {
                self.backend.notify_info("MOD 界面已切换");
            }
            Err(e) => {
                crate::backend::BackendService::log(&format!("[hybrid] apply 失败: {} — {e}", meta.id));
                self.mods_error = Some(e);
            }
        }
    }
}
