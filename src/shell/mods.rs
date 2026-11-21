//! ★双 UI 壳 —— 外观 MOD 包元数据与外部包扫描 (契约 §5, MOD开发文档.md §2)。
//!
//! * 内置包: 终末地 (shell 型, 内嵌资源); 原版经典 = 宿主本体, **不在**可卸载列表。
//! * 外部包: exe 旁 `mods/<id>/`, 启动 + mods.list 时扫描; 校验失败的包跳过不上架。
//! * 不兼容包 (engine 前缀不符) `compatible=false`, 只进降级列表不自动应用 (契约 §五.4)。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// 外观 MOD 元数据 (键名照契约 §3.2 ModMeta; JSON 推给 JS)。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ModMeta {
    pub id: String,
    pub name: String,
    pub version: String,
    pub engine: String,
    pub author: String,
    #[serde(default)]
    pub description: String,
    /// "shell" | "overlay"
    #[serde(rename = "type")]
    pub kind: String,
    /// 包内入口相对路径 (shell = html; overlay = theme.css)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry: Option<String>,
    /// 预览图 URL (★H 轮: 契约 §3.2 键名 = previewUrl —— 此前漏 rename 导致前端
    /// `m.previewUrl` 恒 undefined, <img> 从未渲染, MOD 页预览图 404/空缺)
    #[serde(rename = "previewUrl", default, skip_serializing_if = "Option::is_none")]
    pub preview_url: Option<String>,
    pub builtin: bool,
    pub compatible: bool,
}

/// 外部包磁盘上的 mod.json (字段照 MOD开发文档.md §2; 缺字段宽容, 校验另行做)。
#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct ModJson {
    id: String,
    name: String,
    version: String,
    engine: String,
    author: String,
    description: String,
    #[serde(rename = "type")]
    kind: String,
    entry: String,
    #[serde(rename = "preview")]
    _preview: String,
}

/// 内置终末地包元数据 (宿主经典壳不在列表 —— 契约 §5)。
pub fn builtin_mods() -> Vec<ModMeta> {
    vec![ModMeta {
        id: "endfield".into(),
        name: "终末地风格".into(),
        version: "1.0.0".into(),
        engine: "R2.14+".into(),
        author: "118coder".into(),
        description: "工业风 / 明黄 #FFFA00 / 切角面板 / 赛博机甲 —— 面向 DNF 手柄工具的全新视觉。".into(),
        kind: "shell".into(),
        entry: Some("endfield.html".into()),
        // ★H 轮修复 (两处):
        // ① 旧值 sorahk://mod/endfield/assets/proto.css 是 CSS 被前端 <img> 当预览图
        //    用 → 404/破图; 现指向内嵌表登记的真 PNG (assets_table "preview.png")。
        // ② 必须**相对 URL**: WebView2 自定义协议经 wry workaround 改写, 页面本体
        //    在 http://sorahk.<host> 源上, 页内 <img src="sorahk://…"> 原生 scheme
        //    请求不会被拦截 (实测挂起, 永不回包); 相对 URL 随页面源解析 → 拦截 ✓
        //    (与 CSS/JS/字体同机制)。endfield 页与宿主页同表可解析, 双页通用。
        preview_url: Some("preview.png".into()),
        builtin: true,
        compatible: true,
    }]
}

/// 外部 mods 目录定位: exe 同级 `mods/`。
pub fn mods_root_for_exe_dir(exe_dir: &Path) -> PathBuf {
    exe_dir.join("mods")
}

/// id 合法字符: 小写字母/数字/下划线/连字符, 1..=64 长。
pub fn valid_mod_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
}

/// engine 前缀兼容判定: 允许 "R2" 系 (R2.14+ / R2.15 / R2.x)。
pub fn engine_compatible(engine: &str) -> bool {
    engine.trim().to_ascii_uppercase().starts_with("R2")
}

/// extmod 协议路径安全校验: 返回相对 `mods_root` 的安全相对路径。
/// 拒绝: 空/绝对路径/`..` 段/反斜杠/首段非合法 id。
pub fn safe_extmod_rel(uri_path: &str) -> Option<PathBuf> {
    let p = super::assets_table::normalize_path(uri_path);
    if p.is_empty() {
        return None;
    }
    let mut segs = p.split('/');
    let id = segs.next()?;
    if !valid_mod_id(id) {
        return None;
    }
    let mut out = PathBuf::from(id);
    for seg in segs {
        if seg.is_empty() || seg == "." || seg == ".." || seg.contains('\\') || seg.contains(':') {
            return None;
        }
        out.push(seg);
    }
    Some(out)
}

/// 扫描外部包目录 (mods_root 下每个子目录 = 一个候选包)。非法包静默跳过。
pub fn scan_external(mods_root: &Path) -> Vec<ModMeta> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(mods_root) else {
        return out;
    };
    let mut dirs: Vec<_> = entries.flatten().map(|e| e.path()).collect();
    dirs.sort();
    for dir in dirs {
        if !dir.is_dir() {
            continue;
        }
        if let Some(meta) = load_package(&dir) {
            out.push(meta);
        }
    }
    out
}

/// 读单个包目录: mod.json 必需 + entry 文件必须存在 + id==目录名 + engine 兼容。
fn load_package(dir: &Path) -> Option<ModMeta> {
    let json_path = dir.join("mod.json");
    let text = std::fs::read_to_string(&json_path).ok()?;
    let mj: ModJson = serde_json::from_str(&text).ok()?;
    let dir_name = dir.file_name()?.to_string_lossy().to_string();

    // ① id 合法 + 与目录名一致 (协议 URL 以 id 寻址, 不一致等于资源错位)
    if !valid_mod_id(&mj.id) || mj.id != dir_name {
        return None;
    }
    // ② entry 必填、相对、必须真实存在
    let entry = mj.entry.trim().trim_start_matches('/').to_string();
    if entry.is_empty() {
        return None;
    }
    let entry_path = dir.join(&entry);
    if !entry_path.is_file() {
        return None;
    }
    // ③ 类型白名单 (缺省 shell)
    let kind = match mj.kind.as_str() {
        "" => "shell",
        "shell" | "overlay" => mj.kind.as_str(),
        _ => return None,
    };
    // overlay 的 entry 是注入用的 css/js (同样必须存在, 上面已查)
    let compatible = engine_compatible(&mj.engine);
    let preview_url = if dir.join("preview.png").is_file() {
        Some(format!("sorahk://extmod/{}/preview.png", mj.id))
    } else {
        None
    };
    Some(ModMeta {
        id: mj.id,
        name: if mj.name.is_empty() { dir_name } else { mj.name },
        version: mj.version,
        engine: mj.engine,
        author: mj.author,
        description: mj.description,
        kind: kind.into(),
        entry: Some(entry),
        preview_url,
        builtin: false,
        compatible,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmpdir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "sorahk_shell_mods_{tag}_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .subsec_nanos()
        ));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn write(path: &Path, text: &str) {
        if let Some(p) = path.parent() {
            std::fs::create_dir_all(p).unwrap();
        }
        std::fs::write(path, text).unwrap();
    }

    #[test]
    fn scan_valid_and_invalid_packages() {
        let root = tmpdir("scan");
        // ✓ 合法 shell 包
        let a = root.join("mytheme");
        write(
            &a.join("mod.json"),
            r#"{"id":"mytheme","name":"我的主题","version":"1.0.0","engine":"R2.14+","author":"t","description":"d","type":"shell","entry":"index.html"}"#,
        );
        write(&a.join("index.html"), "<h1>hi</h1>");
        write(&a.join("preview.png"), "fake");
        // ✓ 合法 overlay 包 (engine 不兼容 → compatible=false 但仍上架)
        let b = root.join("oldtheme");
        write(
            &b.join("mod.json"),
            r#"{"id":"oldtheme","name":"旧主题","version":"0.1","engine":"R1.0","type":"overlay","entry":"theme.css"}"#,
        );
        write(&b.join("theme.css"), "body{}");
        // ✗ id 非法 (大写)
        let c = root.join("BadID");
        write(&c.join("mod.json"), r#"{"id":"BadID","entry":"x.html"}"#);
        // ✗ id 与目录名不一致
        let e = root.join("pkgdir");
        write(&e.join("mod.json"), r#"{"id":"other","entry":"x.html"}"#);
        // ✗ entry 文件不存在
        let f = root.join("noentry");
        write(&f.join("mod.json"), r#"{"id":"noentry","entry":"missing.html"}"#);
        // ✗ 没有 mod.json
        let _ = root.join("nojson");
        // ✗ 非目录文件
        write(&root.join("stray.txt"), "x");

        let mut got = scan_external(&root);
        got.sort_by(|x, y| x.id.cmp(&y.id));
        assert_eq!(got.len(), 2, "只收 2 个合法包: {got:?}");

        let shell = &got[0];
        assert_eq!(shell.id, "mytheme");
        assert_eq!(shell.kind, "shell");
        assert_eq!(shell.entry.as_deref(), Some("index.html"));
        assert!(shell.compatible);
        assert_eq!(
            shell.preview_url.as_deref(),
            Some("sorahk://extmod/mytheme/preview.png")
        );
        let overlay = &got[1];
        assert_eq!(overlay.id, "oldtheme");
        assert_eq!(overlay.kind, "overlay");
        assert!(!overlay.compatible, "engine R1.0 不兼容");

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn builtin_endfield_shape() {
        let m = builtin_mods();
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].id, "endfield");
        assert!(m[0].builtin && m[0].compatible);
        // serde 往返 (推给 JS 的 JSON 形态锁定: type 字段名)
        let json = serde_json::to_string(&m[0]).unwrap();
        assert!(json.contains(r#""type":"shell""#));
        // ★H 轮: 契约 §3.2 键名 previewUrl (此前漏 rename → 前端恒 undefined);
        // 值 = 相对 URL (页内 <img> 对原生 scheme 的请求不被 wry workaround 拦截)
        assert!(json.contains(r#""previewUrl":"preview.png""#), "{json}");
        let back: ModMeta = serde_json::from_str(&json).unwrap();
        assert_eq!(back, m[0]);
    }

    #[test]
    fn id_and_engine_rules() {
        assert!(valid_mod_id("endfield"));
        assert!(valid_mod_id("a-b_c1"));
        assert!(!valid_mod_id(""));
        assert!(!valid_mod_id("Endfield"));
        assert!(!valid_mod_id("a b"));
        assert!(!valid_mod_id(&"x".repeat(65)));
        assert!(engine_compatible("R2.14+"));
        assert!(engine_compatible("r2.15"));
        assert!(!engine_compatible("R1.0"));
        assert!(!engine_compatible(""));
    }
}
