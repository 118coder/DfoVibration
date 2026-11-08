//! 无头 UI 渲染 + 图片导出 —— 没有窗口、没有 GPU、没有显示设备, 也能把整套界面
//! 逐帧渲染成 PNG。
//!
//! ## 为什么需要它
//!
//! 旧流程 (`work/offscreen_capture.ps1`) 要真启动 exe、找窗口句柄、把窗口挪到屏外、
//! 再 `PrintWindow` 抓帧: 每次数秒、强依赖交互桌面, 而且只能抓"窗口当前显示的那一屏"
//! —— 滚不动、点不了、弹窗抓不到。调整排版时尤其难受。
//!
//! 这里走纯 CPU 路径, 一次跑完可导出: 全部页面 × 明暗主题 + 全部弹窗 + 长页面分段滚动。
//!
//! ## 实现骨架
//!
//! 1. `egui::Context::run` 与窗口无关 —— 喂 `RawInput` 即得 `FullOutput`。
//!    界面本体是 `SorahkGui::update_ctx(ctx)`; 它不碰 `eframe::Frame`
//!    (拆出来就是为了这个), 所以截图与真机跑的是**同一段渲染代码**。
//! 2. `Context::tessellate` 把 `Shape` 折成三角形 (纯 CPU), 之后只剩光栅化。
//! 3. 光栅化自实现 (扫描线 + 重心坐标 + 双线性采样)。颜色必须在 **gamma 空间**
//!    按预乘 alpha 混合 —— 与 `egui_glow/src/shader/fragment.glsl` 一致
//!    (注释原话: "We multiply the colors in gamma space, because that's the only
//!    way to get text to look right")。改成线性空间会让文字出现明显色边/发灰。
//! 4. PNG 编码用 `png` crate (resvg 已带入依赖树, 不新增网络依赖)。
//!
//! ## 两种用法
//!
//! * 命令行: `sorahk.exe --headless-shots=<目录>` (见 `main.rs`), 或直接跑
//!   `cargo test --test headless_ui_tests` 做回归。
//! * 测试内: `Headless::new(...)` → `frame()` 数次 → `shot()`。
//!
//! ## 已知边界
//!
//! * `Primitive::Callback` 不渲染 (GPU 专用扩展点, 本项目未使用)。
//! * 手柄/键盘页的 SVG 走后台预热线程, 无头下需先同步灌入纹理
//!   (见 [`crate::gui::headless::HeadlessGui::prewarm_textures`])。

use std::collections::HashMap;

use eframe::egui;
use egui::epaint::Primitive;

use crate::config::AppConfig;
use crate::gui::SorahkGui;
use crate::state::AppState;

/// 无头渲染参数。
#[derive(Debug, Clone)]
pub struct HeadlessOptions {
    /// 逻辑宽度 (points)。
    pub width: f32,
    /// 逻辑高度 (points)。
    pub height: f32,
    /// 物理像素 / 逻辑点。2.0 = 2x 高清导出。
    pub pixels_per_point: f32,
    /// PNG 输出目录。
    pub out_dir: std::path::PathBuf,
}

impl Default for HeadlessOptions {
    fn default() -> Self {
        Self {
            width: 1180.0,
            height: 760.0,
            pixels_per_point: 1.0,
            out_dir: std::path::PathBuf::from("work/headless_shots"),
        }
    }
}

/// 一张导出的截图。
#[derive(Debug, Clone)]
pub struct Shot {
    pub name: String,
    pub path: std::path::PathBuf,
    pub width: u32,
    pub height: u32,
}

/// 纹理像素缓存 (egui 只发增量, 这里自己维护完整图集)。
struct Tex {
    size: [usize; 2],
    /// sRGBA **预乘**。egui 的 `ColorImage` / 字体图集 / SVG 纹理统一是这个约定
    /// (`ColorImage::from_rgba_premultiplied`, 字体 `from_white_alpha` = `[a,a,a,a]`),
    /// GPU 端也是原样上传 (egui_glow `upload_texture_srgb`), 所以这里不做任何换算。
    pixels: Vec<[f32; 4]>,
}

/// 无头渲染器: 管一个 `egui::Context` + 纹理缓存 + 画布。
pub struct Headless {
    ctx: egui::Context,
    opts: HeadlessOptions,
    textures: HashMap<egui::TextureId, Tex>,
    frame_index: u64,
    /// 下一帧要注入的输入事件 (滚轮/鼠标移入等)。`frame()` 消费后清空。
    pub pending_events: Vec<egui::Event>,
}

impl Headless {
    /// 建 context 并载入**与正式界面完全相同**的字体栈
    /// (含 CJK 系统字体、Phosphor 图标、真粗体族)。
    pub fn new(opts: HeadlessOptions, language: crate::i18n::Language) -> Self {
        let ctx = egui::Context::default();
        ctx.set_pixels_per_point(opts.pixels_per_point);
        // 本函数是 pub(crate), 与正式启动路径共用, 保证字形一致
        crate::gui::fonts::load_fonts(&ctx, language);
        Self {
            ctx,
            opts,
            textures: HashMap::new(),
            frame_index: 0,
            pending_events: Vec::new(),
        }
    }

    pub fn ctx(&self) -> &egui::Context {
        &self.ctx
    }

    pub fn options(&self) -> &HeadlessOptions {
        &self.opts
    }

    /// 跑一帧。`ui` 内放界面绘制逻辑 (通常是 `gui.update_ctx(ctx)`)。
    ///
    /// 建议连续跑 ≥3 帧再导出: egui 首帧没有上一帧的 `available_rect` / 动画状态 /
    /// 滚动量, 首帧截图常见"控件挤左上"或动画起始态。
    pub fn frame(&mut self, mut ui: impl FnMut(&egui::Context)) -> egui::FullOutput {
        let mut raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(self.opts.width, self.opts.height),
            )),
            max_texture_side: Some(8192),
            time: Some(self.frame_index as f64 / 60.0),
            predicted_dt: 1.0 / 60.0,
            ..Default::default()
        };
        // 把待注入事件 (滚轮等) 交给这一帧
        raw.events = std::mem::take(&mut self.pending_events);
        let out = self.ctx.run(raw, |ctx| ui(ctx));
        self.frame_index += 1;
        self.apply_textures(&out.textures_delta);
        out
    }

    /// 把一帧光栅化成不透明 RGB 像素 (sRGB, 白底压平)。
    pub fn rasterize(&self, out: &egui::FullOutput) -> Vec<u8> {
        let ppp = self.opts.pixels_per_point;
        let w = (self.opts.width * ppp).round().max(1.0) as usize;
        let h = (self.opts.height * ppp).round().max(1.0) as usize;

        let primitives = self.ctx.tessellate(out.shapes.clone(), ppp);
        let mut canvas = Canvas::new(w, h);
        for prim in &primitives {
            if let Primitive::Mesh(mesh) = &prim.primitive {
                self.draw_mesh(&mut canvas, mesh);
            }
            // Primitive::Callback = GPU 专用扩展点; 本项目未使用, 静默跳过。
        }
        canvas.into_rgb()
    }

    /// 光栅化并写 PNG。
    pub fn shot(&self, out: &egui::FullOutput, name: &str) -> anyhow::Result<Shot> {
        let ppp = self.opts.pixels_per_point;
        let w = (self.opts.width * ppp).round().max(1.0) as u32;
        let h = (self.opts.height * ppp).round().max(1.0) as u32;
        let rgb = self.rasterize(out);
        std::fs::create_dir_all(&self.opts.out_dir)?;
        let path = self.opts.out_dir.join(format!("{name}.png"));
        write_png(&path, w, h, &rgb)?;
        Ok(Shot {
            name: name.to_string(),
            path,
            width: w,
            height: h,
        })
    }

    fn apply_textures(&mut self, delta: &egui::TexturesDelta) {
        for (id, img) in &delta.set {
            let egui::ImageData::Color(color) = &img.image;
            let src: Vec<[f32; 4]> = color.pixels.iter().map(|c| c32_to_f32(*c)).collect();
            match img.pos {
                None => {
                    self.textures.insert(
                        *id,
                        Tex {
                            size: image_size(color),
                            pixels: src,
                        },
                    );
                }
                Some([ox, oy]) => {
                    // 补丁贴进已有图集
                    if let Some(tex) = self.textures.get_mut(id) {
                        let [pw, ph] = tex.size;
                        let [sw, sh] = image_size(color);
                        for row in 0..sh {
                            for col in 0..sw {
                                let dx = ox + col;
                                let dy = oy + row;
                                if dx < pw && dy < ph {
                                    tex.pixels[dy * pw + dx] = src[row * sw + col];
                                }
                            }
                        }
                    }
                }
            }
        }
        for id in &delta.free {
            self.textures.remove(id);
        }
    }

    fn draw_mesh(&self, canvas: &mut Canvas, mesh: &egui::Mesh) {
        let tex = self.textures.get(&mesh.texture_id);
        for tri in mesh.indices.chunks_exact(3) {
            let v = [
                mesh.vertices.get(tri[0] as usize).copied(),
                mesh.vertices.get(tri[1] as usize).copied(),
                mesh.vertices.get(tri[2] as usize).copied(),
            ];
            if let [Some(a), Some(b), Some(c)] = v {
                canvas.triangle([a, b, c], tex);
            }
        }
    }
}

/// 无头版界面: 把 `SorahkGui` 绑到无头 context 上。
pub struct HeadlessGui {
    pub headless: Headless,
    pub gui: SorahkGui,
}

impl HeadlessGui {
    /// 用给定配置起一套无头界面。
    ///
    /// ⚠ 会构造 `AppState` —— 它仅读配置、不启后台线程 (挂钩子/托盘/引擎都在
    /// `main.rs` 里做), 所以测试里安全。
    pub fn new(config: AppConfig, opts: HeadlessOptions) -> anyhow::Result<Self> {
        let language = config.language;
        let headless = Headless::new(opts, language);
        let state = std::sync::Arc::new(AppState::new(config.clone())?);
        let gui = SorahkGui::new(state, config);
        Ok(Self { headless, gui })
    }

    /// 跑 n 帧 (不含导出), 让 egui 进入稳定态。
    pub fn warmup(&mut self, n: usize) -> egui::FullOutput {
        let mut last = None;
        for _ in 0..n.max(1) {
            last = Some(self.render_once());
        }
        last.expect("warmup 至少跑一帧")
    }

    /// 跑一帧。
    pub fn render_once(&mut self) -> egui::FullOutput {
        // 拆开借用: headless 与 gui 是两个不同字段, 闭包里若整体捕获 `self`
        // 会撞上双重可变借用; 解构后各自独立。
        let Self { headless, gui } = self;
        headless.frame(|ctx| gui.update_ctx(ctx))
    }

    /// 预热并灌入 SVG 纹理 (手柄页/连发页的键位图走后台线程, 无头下没有线程往返)。
    ///
    /// 正式运行时机: 后台线程光栅化 → 通道回传 → `poll_texture_prewarm` 收编。
    /// 无头下直接同步调同一个加载函数, 结果与真机一致。
    pub fn prewarm_textures(&mut self) {
        let dark = self.gui.dark_mode();
        let ctx = self.headless.ctx().clone();
        let gp = crate::gui::gamepad_mapping::load_gamepad_texture(&ctx, dark);
        let kb = crate::gui::keyboard_quick::load_keyboard_texture(&ctx, dark);
        self.gui.set_prewarmed_textures(gp, kb, dark);
    }
}

// ───────────────────────── 画布 ─────────────────────────

/// 预乘 sRGBA 浮点画布 + 扫描线三角形填充。
struct Canvas {
    w: usize,
    h: usize,
    px: Vec<[f32; 4]>,
}

impl Canvas {
    fn new(w: usize, h: usize) -> Self {
        Self {
            w,
            h,
            px: vec![[0.0; 4]; w * h],
        }
    }

    /// 填一个三角形。顶点色是预乘 sRGBA (`Vertex.color` 契约), 纹理像素同为预乘,
    /// 因此 `src = vertex_color * texel` 天然满足 `frag = v_rgba * texture`。
    /// 混合 `dst = src + dst * (1 - src.a)` = `ONE / ONE_MINUS_SRC_ALPHA`。
    fn triangle(&mut self, v: [egui::epaint::Vertex; 3], tex: Option<&Tex>) {
        let p = [
            [v[0].pos.x, v[0].pos.y],
            [v[1].pos.x, v[1].pos.y],
            [v[2].pos.x, v[2].pos.y],
        ];
        let area = edge(p[0], p[1], p[2]);
        if area.abs() < 1e-9 {
            return;
        }

        let min_x = p.iter().map(|q| q[0]).fold(f32::INFINITY, f32::min).floor().max(0.0) as usize;
        let max_x = p.iter().map(|q| q[0]).fold(f32::NEG_INFINITY, f32::max).ceil().max(0.0) as usize;
        let min_y = p.iter().map(|q| q[1]).fold(f32::INFINITY, f32::min).floor().max(0.0) as usize;
        let max_y = p.iter().map(|q| q[1]).fold(f32::NEG_INFINITY, f32::max).ceil().max(0.0) as usize;
        let max_x = max_x.min(self.w.saturating_sub(1));
        let max_y = max_y.min(self.h.saturating_sub(1));
        if min_x > max_x || min_y > max_y {
            return;
        }

        let col = [c32_to_f32(v[0].color), c32_to_f32(v[1].color), c32_to_f32(v[2].color)];
        let uv = [
            [v[0].uv.x, v[0].uv.y],
            [v[1].uv.x, v[1].uv.y],
            [v[2].uv.x, v[2].uv.y],
        ];

        for y in min_y..=max_y {
            let fy = y as f32 + 0.5;
            for x in min_x..=max_x {
                let fx = x as f32 + 0.5;
                let q = [fx, fy];
                let w0 = edge(p[1], p[2], q) / area;
                let w1 = edge(p[2], p[0], q) / area;
                let w2 = edge(p[0], p[1], q) / area;
                if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
                    continue;
                }

                let mut src = [0.0f32; 4];
                for i in 0..4 {
                    src[i] = w0 * col[0][i] + w1 * col[1][i] + w2 * col[2][i];
                }

                if let Some(t) = tex {
                    let u = w0 * uv[0][0] + w1 * uv[1][0] + w2 * uv[2][0];
                    let vv = w0 * uv[0][1] + w1 * uv[1][1] + w2 * uv[2][1];
                    let tc = sample_bilinear(t, u, vv);
                    for i in 0..4 {
                        src[i] *= tc[i];
                    }
                }
                if src[3] <= 0.0 {
                    continue;
                }

                let dst = &mut self.px[y * self.w + x];
                let inv = 1.0 - src[3];
                dst[0] = src[0] + dst[0] * inv;
                dst[1] = src[1] + dst[1] * inv;
                dst[2] = src[2] + dst[2] * inv;
                dst[3] = src[3] + dst[3] * inv;
            }
        }
    }

    /// 压到白底, 转不透明 RGB。
    fn into_rgb(self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.w * self.h * 3);
        for p in self.px {
            let inv = 1.0 - p[3].clamp(0.0, 1.0);
            for c in p.iter().take(3) {
                let v = (c + inv).clamp(0.0, 1.0);
                out.push((v * 255.0 + 0.5) as u8);
            }
        }
        out
    }
}

#[inline]
fn edge(a: [f32; 2], b: [f32; 2], c: [f32; 2]) -> f32 {
    (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
}

#[inline]
fn c32_to_f32(c: egui::Color32) -> [f32; 4] {
    let [r, g, b, a] = c.to_array();
    [
        r as f32 / 255.0,
        g as f32 / 255.0,
        b as f32 / 255.0,
        a as f32 / 255.0,
    ]
}

#[inline]
fn image_size(img: &egui::ColorImage) -> [usize; 2] {
    [img.size[0], img.size[1]]
}

/// 双线性采样 (输入/输出均为预乘 sRGBA)。越界按边缘钳制。
fn sample_bilinear(t: &Tex, u: f32, v: f32) -> [f32; 4] {
    let tw = t.size[0] as f32;
    let th = t.size[1] as f32;
    if tw < 1.0 || th < 1.0 {
        return [0.0; 4];
    }
    let x = (u * tw - 0.5).clamp(0.0, tw - 1.0);
    let y = (v * th - 0.5).clamp(0.0, th - 1.0);
    let x0 = x.floor();
    let y0 = y.floor();
    let fx = x - x0;
    let fy = y - y0;
    let x1 = (x0 + 1.0).min(tw - 1.0);
    let y1 = (y0 + 1.0).min(th - 1.0);
    let at = |xi: f32, yi: f32| t.pixels[yi as usize * t.size[0] + xi as usize];
    let p00 = at(x0, y0);
    let p10 = at(x1, y0);
    let p01 = at(x0, y1);
    let p11 = at(x1, y1);
    let mut out = [0.0f32; 4];
    for i in 0..4 {
        let top = p00[i] + (p10[i] - p00[i]) * fx;
        let bot = p01[i] + (p11[i] - p01[i]) * fx;
        out[i] = top + (bot - top) * fy;
    }
    out
}

/// 写 8-bit RGB PNG (`png` crate, resvg 已带入依赖树)。
pub fn write_png(path: &std::path::Path, w: u32, h: u32, rgb: &[u8]) -> anyhow::Result<()> {
    use std::io::BufWriter;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let file = std::fs::File::create(path)?;
    let mut enc = png::Encoder::new(BufWriter::new(file), w, h);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    let mut writer = enc.write_header()?;
    writer.write_image_data(rgb)?;
    writer.finish()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 预乘混合规则: 50% 不透明的白 (预乘 = 128,128,128,128) 盖在**不透明黑**上,
    /// 结果应为 50% 灰 (~128), 而不是被当成"纯白半透明"叠出更亮的值。
    ///
    /// ⚠ 画布是透明的, 直接 `into_rgb()` 会压白底 → 看起来永远是 255, 所以这里
    /// 先铺一层不透明黑再叠加, 才是对预乘语义的真检验。
    #[test]
    fn triangle_blends_in_premultiplied_gamma_space() {
        let mut c = Canvas::new(4, 4);
        // 先铺满不透明黑
        let black = egui::Color32::from_rgba_premultiplied(0, 0, 0, 255);
        let v_black = |x: f32, y: f32| egui::epaint::Vertex {
            pos: egui::pos2(x, y),
            uv: egui::Pos2::ZERO,
            color: black,
        };
        c.triangle(
            [
                v_black(-2.0, -2.0),
                v_black(8.0, -2.0),
                v_black(-2.0, 8.0),
            ],
            None,
        );

        let half_white = egui::Color32::from_rgba_premultiplied(128, 128, 128, 128);
        let v = |x: f32, y: f32| egui::epaint::Vertex {
            pos: egui::pos2(x, y),
            uv: egui::Pos2::ZERO,
            color: half_white,
        };
        c.triangle([v(-1.0, -1.0), v(9.0, -1.0), v(-1.0, 9.0)], None);

        let rgb = c.into_rgb();
        let i = (2 * 4 + 2) * 3;
        let px = [rgb[i], rgb[i + 1], rgb[i + 2]];
        assert!(
            px.iter().all(|&v| (126..=130).contains(&v)),
            "预乘半透明白盖黑应得 ~128 灰, 实际 {px:?}"
        );
    }

    /// 完全透明的三角形不得改变画布。
    #[test]
    fn fully_transparent_triangle_is_noop() {
        let mut c = Canvas::new(2, 2);
        let clear = egui::Color32::TRANSPARENT;
        let v = |x: f32, y: f32| egui::epaint::Vertex {
            pos: egui::pos2(x, y),
            uv: egui::Pos2::ZERO,
            color: clear,
        };
        c.triangle([v(-1.0, -1.0), v(9.0, -1.0), v(-1.0, 9.0)], None);
        let rgb = c.into_rgb();
        assert!(rgb.iter().all(|&v| v == 255), "透明三角形应保持白底");
    }

    /// 退化三角形 (三点共线/重合) 必须安全跳过, 不能除零 panic。
    #[test]
    fn degenerate_triangle_is_skipped() {
        let mut c = Canvas::new(3, 3);
        let w = egui::Color32::WHITE;
        let v = |x: f32, y: f32| egui::epaint::Vertex {
            pos: egui::pos2(x, y),
            uv: egui::Pos2::ZERO,
            color: w,
        };
        c.triangle([v(1.0, 1.0), v(1.0, 1.0), v(1.0, 1.0)], None);
        c.triangle([v(0.0, 0.0), v(1.0, 1.0), v(2.0, 2.0)], None);
        assert!(c.into_rgb().iter().all(|&v| v == 255));
    }
}
