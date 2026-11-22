//! ★双 UI 壳 —— 内嵌静态资源表与自定义协议分发 (契约 docs/双UI桥契约_v1.md §5-§6)。
//!
//! 三类虚拟主机:
//! * `sorahk://host/<path>`        —— 宿主本体 (原版经典, index.html 这套);
//! * `sorahk://mod/endfield/<path>` —— 内置终末地包 (同一张内嵌表);
//! * `sorahk://extmod/<id>/<path>`  —— exe 旁 `mods/<id>/` 外部包文件 (动态读盘)。
//!
//! 资源清单是**固定表** (include_bytes! 直接列出), 协议 handler 按表回 200/404。
//! Windows 下 WebView2 会把自定义协议映射为 `http://sorahk.<path>`, wry 在把请求
//! 交给 handler 前已还原成 `sorahk://...` 原始 URL (spike 实测), 解析按原始形态做。

use std::borrow::Cow;
use std::path::Path;

use wry::http::{Response, StatusCode};

// ── 内嵌资源 (构建期固定; 集成期由主线重拷原型产物后重编译即生效) ─────────

const INDEX_HTML: &[u8] = include_bytes!("../../resources/webui/index.html");
const ENDFIELD_HTML: &[u8] = include_bytes!("../../resources/webui/endfield.html");
const REPLICA_CSS: &[u8] = include_bytes!("../../resources/webui/assets/replica.css");
const REPLICA_JS: &[u8] = include_bytes!("../../resources/webui/assets/replica.js");
const PROTO_CSS: &[u8] = include_bytes!("../../resources/webui/assets/proto.css");
const PROTO_JS: &[u8] = include_bytes!("../../resources/webui/assets/proto.js");
const BRIDGE_JS: &[u8] = include_bytes!("../../resources/webui/assets/bridge.js");
const SAIRA_TTF: &[u8] = include_bytes!("../../resources/webui/fonts/Saira.ttf");
const SAIRA_OFL: &[u8] = include_bytes!("../../resources/webui/fonts/Saira-OFL.txt");
/// ★v14 像素对齐: 复刻宿主主字体 = Ubuntu-Light (egui epaint_default_fonts 同源, UFL)。
const UBU_LIGHT_TTF: &[u8] = include_bytes!("../../resources/webui/fonts/Ubuntu-Light.ttf");
const UBU_UFL: &[u8] = include_bytes!("../../resources/webui/fonts/Ubuntu-UFL.txt");
/// ★H 轮 (修「MOD 页 endfield 预览图 404」): 内置终末地包预览图 —— 旧 previewUrl
/// 指向 assets/proto.css 被 <img> 当图片用必然 404/破图; 现登记真 PNG
/// (480×270 缩略, 源图 = 原型 shots/169-overview.png)。
const PREVIEW_PNG: &[u8] = include_bytes!("../../resources/webui/preview.png");

/// (协议路径, 字节) 固定清单。路径不含 host 前缀, 相对 `sorahk://host/`。
/// ★H 轮: 字体实际请求路径 = CSS 相对 URL `./fonts/…` 以 **assets/proto.css /
/// assets/replica.css 为基准** → `assets/fonts/…` —— 旧表只有 `fonts/…` 导致壳内
/// 字体 404 回退系统字体（「mod 主题和网页原型根本不一样」的主因）。两套路径都登记。
const ASSETS: &[(&str, &[u8])] = &[
    ("index.html", INDEX_HTML),
    ("endfield.html", ENDFIELD_HTML),
    ("preview.png", PREVIEW_PNG),
    ("assets/replica.css", REPLICA_CSS),
    ("assets/replica.js", REPLICA_JS),
    ("assets/proto.css", PROTO_CSS),
    ("assets/proto.js", PROTO_JS),
    ("assets/bridge.js", BRIDGE_JS),
    ("fonts/Saira.ttf", SAIRA_TTF),
    ("fonts/Saira-OFL.txt", SAIRA_OFL),
    ("fonts/Ubuntu-Light.ttf", UBU_LIGHT_TTF),
    ("fonts/Ubuntu-UFL.txt", UBU_UFL),
    ("assets/fonts/Saira.ttf", SAIRA_TTF),
    ("assets/fonts/Ubuntu-Light.ttf", UBU_LIGHT_TTF),
];

/// 按扩展名回 Content-Type (html/css/js/ttf/png/txt/json)。
pub fn content_type(path: &str) -> &'static str {
    let lower = path.rsplit('/').next().unwrap_or(path).to_ascii_lowercase();
    if let Some(ext) = lower.rsplit_once('.').map(|(_, e)| e) {
        match ext {
            "html" | "htm" => "text/html; charset=utf-8",
            "css" => "text/css; charset=utf-8",
            "js" | "mjs" => "text/javascript; charset=utf-8",
            "ttf" => "font/ttf",
            "otf" => "font/otf",
            "woff" => "font/woff",
            "woff2" => "font/woff2",
            "png" => "image/png",
            "jpg" | "jpeg" => "image/jpeg",
            "svg" => "image/svg+xml",
            "json" => "application/json; charset=utf-8",
            "txt" => "text/plain; charset=utf-8",
            _ => "application/octet-stream",
        }
    } else {
        "application/octet-stream"
    }
}

/// 内嵌表查询 (host 与内置 mod 共用)。路径先过 [`normalize_path`]。
pub fn resolve_embedded(path: &str) -> Option<(&'static [u8], &'static str)> {
    let path = normalize_path(path);
    let path = if path.is_empty() { "index.html" } else { &path };
    ASSETS
        .iter()
        .find(|(p, _)| *p == path)
        .map(|(p, b)| (*b, content_type(p)))
}

/// 去掉查询串/锚点、剥首尾斜杠 (纯函数, 测试覆盖)。
pub fn normalize_path(path: &str) -> String {
    let no_query = path.split(['?', '#']).next().unwrap_or(path);
    no_query.trim_matches('/').to_string()
}

fn response(status: StatusCode, body: Cow<'static, [u8]>, ctype: &str) -> Response<Cow<'static, [u8]>> {
    Response::builder()
        .status(status)
        .header("Content-Type", ctype)
        .header("Cache-Control", "no-cache")
        .body(body)
        .unwrap()
}

/// 200 响应。
fn ok(body: Cow<'static, [u8]>, ctype: &str) -> Response<Cow<'static, [u8]>> {
    response(StatusCode::OK, body, ctype)
}

/// 404 响应 (协议 handler 单元测试断言用)。
fn not_found() -> Response<Cow<'static, [u8]>> {
    response(StatusCode::NOT_FOUND, Cow::Borrowed(b"404 not found"), "text/plain; charset=utf-8")
}

/// 协议分发入口。`uri` 形如 `sorahk://host/index.html`;
/// `mods_root` = exe 旁 `mods/` 目录 (None = 不提供外部包服务)。
pub fn serve(
    uri: &str,
    mods_root: Option<&Path>,
) -> Response<Cow<'static, [u8]>> {
    let rest = match uri.strip_prefix("sorahk://") {
        Some(r) => r,
        None => return not_found(),
    };
    // rest = "<host>/<path...>" 或纯 "<host>"
    let (host, path) = match rest.split_once('/') {
        Some((h, p)) => (h, p),
        None => (rest, ""),
    };
    match host {
        "host" => resolve_embedded(path)
            .map(|(b, t)| ok(Cow::Borrowed(b), t))
            .unwrap_or_else(not_found),
        // 内置包: 同一张内嵌表 (endfield.html 等)。
        "mod" => {
            let pkg = normalize_path(path);
            if pkg.starts_with("endfield/") {
                let sub = pkg.strip_prefix("endfield/").unwrap_or_default();
                resolve_embedded(sub)
                    .map(|(b, t)| ok(Cow::Borrowed(b), t))
                    .unwrap_or_else(not_found)
            } else {
                not_found()
            }
        }
        // 外部包: sorahk://extmod/<id>/<rel> → mods_root/<id>/<rel>。
        "extmod" => {
            let full = normalize_path(path);
            let Some(mods_root) = mods_root else { return not_found() };
            let Some(rel) = crate::shell::mods::safe_extmod_rel(&full) else {
                return not_found();
            };
            let file = mods_root.join(&rel);
            match std::fs::read(&file) {
                Ok(bytes) => ok(Cow::Owned(bytes), content_type(&full)),
                Err(_) => not_found(),
            }
        }
        _ => not_found(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_resolves_and_404s() {
        assert!(resolve_embedded("index.html").is_some());
        assert!(resolve_embedded("").is_some(), "空路径应回 index.html");
        assert!(resolve_embedded("assets/replica.css").is_some());
        assert!(resolve_embedded("assets/bridge.js").is_some());
        assert!(resolve_embedded("fonts/Saira.ttf").is_some());
        assert!(resolve_embedded("fonts/Ubuntu-Light.ttf").is_some(), "v14 对齐主字体必须可服务");
        assert!(resolve_embedded("nope.js").is_none(), "未知路径必须 None");
        // 查询串/锚点剥离 + 首尾斜杠容忍
        assert!(resolve_embedded("/index.html?v=2#x").is_some());
    }

    #[test]
    fn serve_404_for_unknown() {
        let r = serve("sorahk://host/nope.js", None);
        assert_eq!(r.status(), StatusCode::NOT_FOUND);
        let r = serve("sorahk://unknown-host/x", None);
        assert_eq!(r.status(), StatusCode::NOT_FOUND);
        let r = serve("sorahk://mod/otherpkg/x.js", None);
        assert_eq!(r.status(), StatusCode::NOT_FOUND, "非内置包一律 404");
        let r = serve("http://evil/x", None);
        assert_eq!(r.status(), StatusCode::NOT_FOUND);
        // 内置包命中
        let r = serve("sorahk://mod/endfield/endfield.html", None);
        assert_eq!(r.status(), StatusCode::OK);
        let r = serve("sorahk://mod/endfield/assets/proto.css", None);
        assert_eq!(r.status(), StatusCode::OK);
    }

    /// v1.3 Saira 字体修复（mod 字挤主修）的内嵌表保障: endfield.html 支持
    /// `sorahk://host/endfield.html` 与 `sorahk://mod/endfield/endfield.html`
    /// 两种加载形态, 页面相对引用 `./fonts/Saira.ttf` 在两种前缀下都必须 200。
    /// ★H 轮: CSS 相对 URL 实际请求路径 (assets/fonts/…) 必须 200 ——
    /// 「mod 主题和网页原型根本不一样」的主因修复 (壳内字体曾 404 回退系统字体)。
    #[test]
    fn fonts_served_at_css_relative_paths() {
        for uri in [
            "sorahk://mod/endfield/assets/fonts/Saira.ttf",
            "sorahk://host/assets/fonts/Saira.ttf",
            "sorahk://host/assets/fonts/Ubuntu-Light.ttf",
        ] {
            let r = serve(uri, None);
            assert_eq!(r.status(), StatusCode::OK, "{uri} 必须 200");
            assert_eq!(
                r.headers().get("Content-Type").and_then(|v| v.to_str().ok()),
                Some("font/ttf")
            );
        }
    }

    #[test]
    fn saira_font_served_both_prefixes() {
        for uri in [
            "sorahk://mod/endfield/fonts/Saira.ttf",
            "sorahk://host/fonts/Saira.ttf",
        ] {
            let r = serve(uri, None);
            assert_eq!(r.status(), StatusCode::OK, "{uri} 必须命中内嵌表");
            assert_eq!(
                r.headers().get("Content-Type").and_then(|v| v.to_str().ok()),
                Some("font/ttf"),
                "{uri} Content-Type 必须 font/ttf"
            );
        }
    }

    /// ★H 轮: 内置终末地包预览图必须可服务 (修 MOD 页预览图 404) ——
    /// mods.rs builtin previewUrl 指向本路径, Content-Type 必须 image/png。
    #[test]
    fn endfield_preview_png_served() {
        let r = serve("sorahk://mod/endfield/preview.png", None);
        assert_eq!(r.status(), StatusCode::OK, "预览图必须命中内嵌表");
        assert_eq!(
            r.headers().get("Content-Type").and_then(|v| v.to_str().ok()),
            Some("image/png"),
            "预览图 Content-Type 必须 image/png"
        );
        assert!(r.body().len() > 1000, "预览图不能是空占位: {}", r.body().len());
        // PNG 魔数 (89 50 4E 47)
        assert_eq!(&r.body()[..4], &[0x89, 0x50, 0x4E, 0x47], "必须是真 PNG 头");
    }

    #[test]
    fn extmod_blocks_traversal() {
        assert!(crate::shell::mods::safe_extmod_rel("../Config.toml").is_none());
        assert!(crate::shell::mods::safe_extmod_rel("a/../../b").is_none());
        assert!(crate::shell::mods::safe_extmod_rel("..\\secret").is_none());
        assert!(crate::shell::mods::safe_extmod_rel("mytheme/preview.png").is_some());
        assert!(crate::shell::mods::safe_extmod_rel("assets/theme.css").is_some());
        let r = serve("sorahk://extmod/whatever/../../Config.toml", None);
        assert_eq!(r.status(), StatusCode::NOT_FOUND);
    }

    #[test]
    fn content_types() {
        assert!(content_type("a/index.html").starts_with("text/html"));
        assert!(content_type("a/x.css").starts_with("text/css"));
        assert!(content_type("a/x.js").starts_with("text/javascript"));
        assert_eq!(content_type("a/Saira.ttf"), "font/ttf");
        assert_eq!(content_type("a/preview.png"), "image/png");
    }
}
