//! 批量导出: 一次跑完"全部页面 × 明暗主题 + 全部弹窗 + 长页面分段滚动"。
//!
//! 这是给 UI 调整用的**总入口** —— 调完布局跑一条命令, 拿到一整个目录的 PNG,
//! 不用启动窗口、不用手动点、不依赖桌面会话。
//!
//! 用法:
//! ```text
//! sorahk.exe --headless-shots                   # 默认 work/headless_shots, 1180x760 深色
//! sorahk.exe --headless-shots=D:\shots --headless-size=1600x1000 --headless-ppp=2
//! sorahk.exe --headless-shots --headless-themes=dark,light
//! ```

use std::path::PathBuf;

use eframe::egui;

use crate::config::AppConfig;
use crate::gui::DialogForShot;
use crate::gui::headless::{HeadlessGui, HeadlessOptions, Shot};
use crate::gui::types::Page;

/// 导出清单配置。
#[derive(Debug, Clone)]
pub struct ShotPlan {
    pub opts: HeadlessOptions,
    /// 要导出哪些主题 (true = 深色)。
    pub themes: Vec<bool>,
    /// 要导出哪些页面。
    pub pages: Vec<Page>,
    /// 是否导出弹窗。
    pub dialogs: bool,
    /// 长页面 (震动/职业预设) 的分段滚动截图段数。
    pub scroll_steps: usize,
}

impl Default for ShotPlan {
    fn default() -> Self {
        Self {
            opts: HeadlessOptions::default(),
            themes: vec![true],
            pages: vec![
                Page::Gamepad,
                Page::Turbo,
                Page::Whitelist,
                Page::Vibration,
                Page::JobPresets,
            ],
            dialogs: true,
            scroll_steps: 3,
        }
    }
}

impl ShotPlan {
    /// 从命令行参数解析。
    ///
    /// 支持的开关 (都可省略, 省略即用默认):
    /// * `--headless-shots[=目录]`
    /// * `--headless-size=1440x900`
    /// * `--headless-ppp=1.0|2.0`
    /// * `--headless-themes=dark,light`
    /// * `--headless-scroll=3`  (0 = 不导分段滚动)
    /// * `--headless-no-dialogs`
    pub fn from_args(args: &[String]) -> Option<Self> {
        let mut plan = Self::default();
        let mut found = false;

        for arg in args {
            if let Some(v) = arg.strip_prefix("--headless-shots") {
                found = true;
                let dir = v.strip_prefix('=').unwrap_or("");
                if !dir.is_empty() {
                    plan.opts.out_dir = PathBuf::from(dir);
                }
            } else if let Some(v) = arg.strip_prefix("--headless-size=") {
                if let Some((w, h)) = v.split_once('x')
                    && let (Ok(w), Ok(h)) = (w.parse::<f32>(), h.parse::<f32>())
                    && w > 0.0
                    && h > 0.0
                {
                    plan.opts.width = w;
                    plan.opts.height = h;
                }
            } else if let Some(v) = arg.strip_prefix("--headless-ppp=") {
                if let Ok(ppp) = v.parse::<f32>()
                    && ppp > 0.0
                {
                    plan.opts.pixels_per_point = ppp;
                }
            } else if let Some(v) = arg.strip_prefix("--headless-themes=") {
                plan.themes = v
                    .split(',')
                    .filter_map(|t| match t.trim() {
                        "dark" | "深色" => Some(true),
                        "light" | "浅色" => Some(false),
                        _ => None,
                    })
                    .collect();
                if plan.themes.is_empty() {
                    plan.themes = vec![true];
                }
            } else if let Some(v) = arg.strip_prefix("--headless-scroll=") {
                plan.scroll_steps = v.parse::<usize>().unwrap_or(3);
            } else if arg == "--headless-no-dialogs" {
                plan.dialogs = false;
            }
        }

        found.then_some(plan)
    }
}

/// 跑完整套导出, 返回全部截图。
pub fn run(plan: &ShotPlan, base_config: &AppConfig) -> anyhow::Result<Vec<Shot>> {
    let mut shots = Vec::new();

    for &dark in &plan.themes {
        let mut cfg = base_config.clone();
        cfg.dark_mode = dark;
        cfg.guide_seen = true;
        cfg.minimal_mode = false;
        cfg.classic_mode = false;
        // 震动页在两个导航项里; DFO 非玩家会被隐藏 → 截图固定按 DFO 玩家导出
        cfg.dfo_player = true;

        let mut app = HeadlessGui::new(cfg, plan.opts.clone())?;
        app.gui.skip_first_run_for_shot();
        // SVG 纹理 (手柄/键盘图) 必须同步灌入: 无头没有后台线程往返
        app.prewarm_textures();

        let suffix = if dark { "dark" } else { "light" };

        for &page in &plan.pages {
            app.gui.set_page(page);
            // 切页后先跑 3 帧稳定 (滚动区域/动画/尺寸缓存都在首帧建立)
            let mut out = app.warmup(3);

            let name = format!("page-{}-{suffix}", page_slug(page));
            shots.push(app.headless.shot(&out, &name)?);

            // 长页面: 分段滚动 —— 每段滚一屏, 覆盖"完整滚动内容"。
            // 到底就停: 滚不动时渲染结果与上一段逐像素相同, 再导只是重复文件。
            if plan.scroll_steps > 0 && is_long_page(page) {
                let mut prev_hash = 0u64;
                for step in 1..=plan.scroll_steps {
                    let moved = scroll_page(&mut app);
                    out = app.warmup(2);

                    let rgb = app.headless.rasterize(&out);
                    let hash = fnv1a(&rgb);
                    if hash == prev_hash || !moved {
                        // 内容没动 = 已到底; 丢弃这一张
                        break;
                    }
                    prev_hash = hash;

                    let name = format!("page-{}-{suffix}-scroll{step}", page_slug(page));
                    shots.push(write_shot(&app, &out, &rgb, &name)?);
                }
                // 复位, 避免影响后续页面
                reset_scroll(&mut app);
            }
        }

        if plan.dialogs {
            // 弹窗都开在震动页上 (内容最满, 也最能看出遮挡关系)
            app.gui.set_page(Page::Vibration);
            app.warmup(2);

            for (dialog, slug) in [
                (DialogForShot::Settings, "dialog-settings"),
                (DialogForShot::About, "dialog-about"),
                (DialogForShot::Close, "dialog-close"),
            ] {
                app.gui.set_dialog(dialog, true);
                let out = app.warmup(3);
                shots.push(app.headless.shot(&out, &format!("{slug}-{suffix}"))?);
                app.gui.set_dialog(dialog, false);
                app.warmup(1);
            }
        }
    }

    Ok(shots)
}

fn page_slug(page: Page) -> &'static str {
    match page {
        Page::Gamepad => "gamepad",
        Page::Turbo => "turbo",
        Page::Whitelist => "whitelist",
        Page::Vibration => "vibration",
        Page::JobPresets => "jobs",
    }
}

fn is_long_page(page: Page) -> bool {
    matches!(page, Page::Vibration | Page::JobPresets)
}

/// 让页面内的滚动区域滚一段, 返回**是否真的滚动了**。
///
/// egui 没有"编程式滚动某页面"的统一接口, 这里对整页发滚轮事件 —— 指针位置放在
/// 内容区中线, 滚轮会被最内层可滚动区域消费, 正是"整页往下翻"。
/// 返回 false = 已到底 (或页面不可滚), 调用方应停止继续导分段图。
fn scroll_page(app: &mut HeadlessGui) -> bool {
    let (w, h) = (app.headless.options().width, app.headless.options().height);
    let pos = egui::pos2(w * 0.5, h * 0.5);

    let out = app.render_once();
    let before_hash = fnv1a(&app.headless.rasterize(&out));

    app.headless
        .pending_events
        .push(egui::Event::PointerMoved(pos));
    app.headless.pending_events.push(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: egui::vec2(0.0, -(h * 0.75)),
        modifiers: egui::Modifiers::NONE,
    });
    let _ = app.render_once();

    let out = app.render_once();
    fnv1a(&app.headless.rasterize(&out)) != before_hash
}

/// 滚回顶部 (切页/换主题前复位, 保证每页都从同一位置开始)。
fn reset_scroll(app: &mut HeadlessGui) {
    let (w, h) = (app.headless.options().width, app.headless.options().height);
    let pos = egui::pos2(w * 0.5, h * 0.5);
    app.headless
        .pending_events
        .push(egui::Event::PointerMoved(pos));
    // 一次给足: 远大于任何页面的内容高度
    app.headless.pending_events.push(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: egui::vec2(0.0, h * 100.0),
        modifiers: egui::Modifiers::NONE,
    });
    let _ = app.render_once();
    let _ = app.render_once();
}

/// 写一张截图 (复用已算好的像素, 避免重复光栅化)。
fn write_shot(
    app: &HeadlessGui,
    _out: &egui::FullOutput,
    rgb: &[u8],
    name: &str,
) -> anyhow::Result<Shot> {
    let opts = app.headless.options();
    let ppp = opts.pixels_per_point;
    let w = (opts.width * ppp).round().max(1.0) as u32;
    let h = (opts.height * ppp).round().max(1.0) as u32;
    std::fs::create_dir_all(&opts.out_dir)?;
    let path = opts.out_dir.join(format!("{name}.png"));
    crate::gui::headless::write_png(&path, w, h, rgb)?;
    Ok(Shot {
        name: name.to_string(),
        path,
        width: w,
        height: h,
    })
}

/// FNV-1a: 只要"内容有没有变"的快速判等, 不需要密码学强度。
fn fnv1a(data: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for &b in data {
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}
