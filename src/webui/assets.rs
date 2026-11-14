//! 前端文件随 exe 一起打包：单文件分发，不依赖安装目录里有没有 `webui/`。
//!
//! 调试时可用环境变量 `SORAHK_WEBUI_DIR` 指向本机目录，改完刷新浏览器即生效。

use std::path::{Path, PathBuf};

macro_rules! text_asset {
    ($path:literal) => {
        ($path, include_str!(concat!("../../webui/", $path)), "text")
    };
}

/// (请求路径, 内容, 类型)
fn embedded(path: &str) -> Option<(Vec<u8>, &'static str)> {
    let entry: Option<(&'static str, &'static str)> = match path {
        "/index.html" => Some(("", include_str!("../../webui/index.html"))),
        "/assets/app.css" => Some(("", include_str!("../../webui/assets/app.css"))),
        "/assets/endfield.css" => Some(("", include_str!("../../webui/assets/endfield.css"))),
        "/assets/app.js" => Some(("", include_str!("../../webui/assets/app.js"))),
        "/assets/api.js" => Some(("", include_str!("../../webui/assets/api.js"))),
        "/assets/dial.js" => Some(("", include_str!("../../webui/assets/dial.js"))),
        "/assets/pages.js" => Some(("", include_str!("../../webui/assets/pages.js"))),
        "/assets/util.js" => Some(("", include_str!("../../webui/assets/util.js"))),
        "/assets/pad-light.svg" => Some(("", include_str!("../../webui/assets/pad-light.svg"))),
        "/assets/pad-dark.svg" => Some(("", include_str!("../../webui/assets/pad-dark.svg"))),
        "/assets/pad-map-light.svg" => Some(("", include_str!("../../webui/assets/pad-map-light.svg"))),
        "/assets/pad-map-dark.svg" => Some(("", include_str!("../../webui/assets/pad-map-dark.svg"))),
        "/assets/keyboard.svg" => Some(("", include_str!("../../webui/assets/keyboard.svg"))),
        "/assets/contours/contour-a.svg" => {
            Some(("", include_str!("../../webui/assets/contours/contour-a.svg")))
        }
        "/assets/contours/contour-b.svg" => {
            Some(("", include_str!("../../webui/assets/contours/contour-b.svg")))
        }
        "/assets/fonts/Saira-OFL.txt" => Some(("", include_str!("../../webui/assets/fonts/Saira-OFL.txt"))),
        _ => None,
    };
    if let Some((_, body)) = entry {
        return Some((body.as_bytes().to_vec(), mime(path)));
    }
    if path == "/assets/fonts/Saira.ttf" {
        return Some((
            include_bytes!("../../webui/assets/fonts/Saira.ttf").to_vec(),
            "font/ttf",
        ));
    }
    None
}

pub fn mime(path: &str) -> &'static str {
    if path.ends_with(".html") {
        "text/html; charset=utf-8"
    } else if path.ends_with(".css") {
        "text/css; charset=utf-8"
    } else if path.ends_with(".js") {
        "text/javascript; charset=utf-8"
    } else if path.ends_with(".svg") {
        "image/svg+xml"
    } else if path.ends_with(".ttf") {
        "font/ttf"
    } else if path.ends_with(".txt") {
        "text/plain; charset=utf-8"
    } else {
        "application/octet-stream"
    }
}

/// 调试目录（存在就用它，边改边刷）。
pub fn dev_dir() -> Option<PathBuf> {
    let dir = std::env::var("SORAHK_WEBUI_DIR").ok()?;
    let p = PathBuf::from(dir);
    if p.join("index.html").exists() { Some(p) } else { None }
}

/// 取一个静态文件：先看调试目录，再退回内嵌资源。
pub fn load(path: &str) -> Option<(Vec<u8>, &'static str)> {
    if let Some(root) = dev_dir() {
        let rel = path.trim_start_matches('/');
        let file = root.join(rel);
        if file.starts_with(&root) && file.is_file() {
            if let Ok(bytes) = std::fs::read(&file) {
                return Some((bytes, mime(path)));
            }
        }
    }
    embedded(path)
}

/// 首页：把一次性口令写进 meta，前端带着它调接口。
pub fn index_html(token: &str) -> Option<String> {
    let (bytes, _) = load("/index.html")?;
    let html = String::from_utf8(bytes).ok()?;
    Some(html.replace("__SORAHK_TOKEN__", token))
}

#[allow(dead_code)]
pub fn exists(path: &str) -> bool {
    Path::new(path).exists()
}

// 让未使用的宏在无 dev 目录时也不报警
#[allow(unused_macros)]
macro_rules! _unused {
    () => {};
}
