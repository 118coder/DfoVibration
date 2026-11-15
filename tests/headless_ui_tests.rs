//! 无头 UI 渲染的回归测试。
//!
//! 这些测试证明"整套界面能在没有窗口的环境里渲染出**有内容的**图":
//! 不是只看不 panic, 而是检查像素层面确实画出了东西 (多色、有明暗层次、
//! 五个页面互不相同、明暗主题互为反相)。UI 排版调整若把某页画成空白,
//! 这里会直接失败。
//!
//! 跑法: `cargo test --profile iter --test headless_ui_tests`

use sorahk::config::AppConfig;
use sorahk::gui::headless::{HeadlessGui, HeadlessOptions};
use sorahk::gui::types::Page;

/// 测试用无头配置: 小尺寸 + 1x, 保证 CI 上也秒级完成。
fn opts(out: &str) -> HeadlessOptions {
    HeadlessOptions {
        width: 1180.0,
        height: 760.0,
        pixels_per_point: 1.0,
        out_dir: std::env::temp_dir().join(out),
    }
}

fn app(dark: bool, dir: &str) -> HeadlessGui {
    let mut cfg = AppConfig::default();
    cfg.dark_mode = dark;
    cfg.guide_seen = true;
    cfg.minimal_mode = false;
    cfg.classic_mode = false;
    cfg.dfo_player = true;
    let mut a = HeadlessGui::new(cfg, opts(dir)).expect("构建无头界面");
    a.gui.skip_first_run_for_shot();
    a
}

/// 图像"有内容"的判据: 多于一屏纯色。
struct Stats {
    unique: usize,
    mean_lum: f32,
}

fn stats(rgb: &[u8]) -> Stats {
    let mut set = std::collections::HashSet::new();
    let mut sum = 0.0f64;
    let mut n = 0usize;
    let mut i = 0;
    while i + 2 < rgb.len() {
        let (r, g, b) = (rgb[i] as f32, rgb[i + 1] as f32, rgb[i + 2] as f32);
        sum += 0.2126 * r as f64 + 0.7152 * g as f64 + 0.0722 * b as f64;
        n += 1;
        // 抽样统计颜色种类 (全量会慢且无必要)
        if i % (3 * 37) == 0 {
            set.insert((rgb[i], rgb[i + 1], rgb[i + 2]));
        }
        i += 3;
    }
    Stats {
        unique: set.len(),
        mean_lum: (sum / n.max(1) as f64) as f32,
    }
}

#[test]
fn headless_renders_all_pages_with_real_content() {
    let mut app = app(true, "sorahk_headless_test_pages");
    /* ★R2.12: 与 headless_shots.rs 一致 —— 必须先跑一帧把 max_texture_side
     * 提到 8192 再灌纹理。默认 Context 上限 2048 装不下 2240×840 的键盘 SVG,
     * 直接 prewarm 会 panic ("maximum texture side is 2048"), 整批截图全丢。
     * 这是既有的测试缺陷 (同源问题已在 keyboard_quick 单测修过), 与本轮改动无关。 */
    let _ = app.warmup(1);
    app.prewarm_textures();

    let mut seen = Vec::new();
    for page in [
        Page::Gamepad,
        Page::Turbo,
        Page::Whitelist,
        Page::Vibration,
        Page::JobPresets,
    ] {
        app.gui.set_page(page);
        let out = app.warmup(3);
        let rgb = app.headless.rasterize(&out);
        let s = stats(&rgb);

        assert_eq!(rgb.len(), 1180 * 760 * 3, "{page:?} 尺寸不对");
        assert!(
            s.unique > 60,
            "{page:?} 渲染结果颜色过少 ({}), 疑似空白/未绘制",
            s.unique
        );
        assert!(
            s.mean_lum < 90.0,
            "{page:?} 深色主题平均亮度 {:.1} 偏高, 疑似背景没画对",
            s.mean_lum
        );
        seen.push((page, s.unique, s.mean_lum));
    }

    // 五个页面必须互不相同 (防止"所有页面都渲染成同一张")
    let mut pairs = 0;
    for i in 0..seen.len() {
        for j in (i + 1)..seen.len() {
            let (a, b) = (&seen[i], &seen[j]);
            assert!(
                (a.1, a.2) != (b.1, b.2),
                "{:?} 与 {:?} 的像素统计完全相同, 页面切换可能未生效",
                a.0,
                b.0
            );
            pairs += 1;
        }
    }
    assert_eq!(pairs, 10, "应比较 10 组页面, 实际 {pairs}");
}

#[test]
fn headless_light_and_dark_are_mirror_images() {
    let mut dark = app(true, "sorahk_headless_test_dark");
    let mut light = app(false, "sorahk_headless_test_light");
    dark.gui.set_page(Page::Vibration);
    light.gui.set_page(Page::Vibration);

    let out_d = dark.warmup(3);
    let d = stats(&dark.headless.rasterize(&out_d));
    let out_l = light.warmup(3);
    let l = stats(&light.headless.rasterize(&out_l));

    assert!(d.mean_lum < 90.0, "深色主题应偏暗, 实测 {:.1}", d.mean_lum);
    assert!(l.mean_lum > 180.0, "浅色主题应偏亮, 实测 {:.1}", l.mean_lum);
    assert!(
        (l.mean_lum - d.mean_lum) > 120.0,
        "明暗主题亮度差过小 (深 {:.1} / 浅 {:.1}), 可能是同一套色板",
        d.mean_lum,
        l.mean_lum
    );
}

#[test]
fn headless_shot_writes_valid_png() {
    let mut app = app(true, "sorahk_headless_test_png");
    app.gui.set_page(Page::Vibration);
    let out = app.warmup(3);
    let shot = app.headless.shot(&out, "probe").expect("写 PNG");

    assert_eq!((shot.width, shot.height), (1180, 760));
    let bytes = std::fs::read(&shot.path).expect("PNG 应已落盘");
    assert!(bytes.len() > 5_000, "PNG 过小 ({} 字节), 疑似空白图", bytes.len());
    // PNG magic
    assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n", "不是合法 PNG");

    let _ = std::fs::remove_file(&shot.path);
}

/// 无头路径必须与真机路径共用同一段渲染代码 —— `update_ctx` 一旦被塞回
/// `eframe::Frame` 参数, 无头就再也调不到, 这个测试会立刻失败。
#[test]
fn headless_shares_render_path_with_app() {
    let mut a = app(true, "sorahk_headless_test_shared");
    // 只要 render_once 能跑且产出非空形状, 就证明 update_ctx 可用
    let out = a.warmup(2);
    assert!(
        !out.shapes.is_empty(),
        "无头渲染没有产出任何 shape —— update_ctx 可能已与无头路径脱钩"
    );
}
