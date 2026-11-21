//! ★K 轮 —— .dvmod 单文件格式包 (外观 MOD 分发格式, 格式规范见 docs/dvmod格式规范_v1.md)。
//!
//! 容器选型 (K 轮调研定案, 详见交接_双UI壳_K轮.md §1):
//! **标准 ZIP 容器 + 自研精简只读解析器**。.dvmod 文件本体就是普通 ZIP (第三方作者
//! 用任意压缩工具「压缩文件夹 + 改后缀」即可打包, 生态零门槛), 但壳内解析不用 zip crate
//! (需新增下载), 而是 ~260 行精简只读解析器, 解码只用 flate2 (deflate) + crc32fast
//! (完整性) —— 两者都已在依赖树内 (png/usvg/tiff 传递依赖), 零新增下载、零新编译单元。
//!
//! 安全清单 (导入即校验, 任一不过 → 整包拒绝, 磁盘零落盘):
//! * 包体大小 / 条目数 / 单文件解压后大小 / 解压总量 四重上限 (ZIP 炸弹防护;
//!   解压按声明大小硬 cap, 超限即断, 不吃满内存);
//! * 路径安全: UTF-8 / 禁绝对路径 / 禁 `..` `.` 段 / 禁反斜杠与盘符 (复用
//!   safe_extmod_rel 同款思路), 重名条目拒绝;
//! * 加密位 / zip64 标记 / 未知压缩方法 → 明确报错拒绝 (支持 stored + deflate);
//! * CRC-32 逐文件校验, 声明大小与实际解压大小不一致拒绝;
//! * manifest (mod.toml) 必需且字段校验: id 规则同 mods/<id> (valid_mod_id) +
//!   保留字拒绝 (endfield/classic/host/extmod/mod) + entry 必须真实存在于包内;
//! * 安装原子性: 先解包到 `mods/.import-<id>-<序号>` 临时目录, 全部校验通过后才
//!   换名替换 `mods/<id>/` (同 id = 覆盖替换; 失败路径旧包不动, 临时目录直删)。
//!
//! 运行时服务零侵入: 安装结果就是一个普通 `mods/<id>/` 目录, 复用既有 extmod 扫描
//! (mods.rs scan_external) 与 `sorahk://extmod/` 协议服务, 双 UI 与 CLI 共用本模块。

use std::io::Read;
use std::path::Path;

use serde::Serialize;

/// 包内清单文件名 (固定在 ZIP 根, 大小写敏感 —— 规范 §2)。
pub const MANIFEST_NAME: &str = "mod.toml";
/// .dvmod 文件对话框过滤器用扩展名。
pub const DVMOD_EXT: &str = "dvmod";

// ── 安全上限 (规范 §4; 全部可在常量层审计) ────────────────────────────────
/// .dvmod 包体 (ZIP 文件本身) 上限: 100 MB。
pub const MAX_ARCHIVE_BYTES: u64 = 100 * 1024 * 1024;
/// 包内条目上限 (含目录条目): 500。
pub const MAX_ENTRIES: usize = 500;
/// 单文件解压后上限: 64 MB。
pub const MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;
/// 解压总量上限: 128 MB。
pub const MAX_TOTAL_BYTES: u64 = 128 * 1024 * 1024;

/// 保留 id (与壳内 URL 寻址/内置包冲突, 导入一律拒绝)。
pub const RESERVED_IDS: &[&str] = &["endfield", "classic", "host", "extmod", "mod"];

/// 包内清单 (mod.toml; 字段语义照 MOD开发文档.md §2 mod.json, 格式改 TOML 便于手写)。
#[derive(Debug, Clone, serde::Deserialize)]
pub struct DvmodManifest {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub engine: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub description: String,
    /// "shell" (整页主题, 缺省) | "overlay" (注入式 css/js)。
    #[serde(rename = "type", default)]
    pub kind: String,
    /// 包内入口相对路径 (shell = html; overlay = css/js)。
    pub entry: String,
    /// 预览图包内相对路径 (可选; 推荐根目录 preview.png)。
    #[serde(default)]
    pub preview: Option<String>,
}

/// 导入结果回执 (IPC reply data / CLI 输出 / egui 提示共用)。
#[derive(Debug, Clone, Serialize)]
pub struct InstallReport {
    pub id: String,
    pub name: String,
    pub version: String,
    /// true = 覆盖替换了已存在的同 id 包。
    pub replaced: bool,
    /// 解包写入的文件数 (不含目录)。
    pub files: usize,
    /// 解压后总字节数。
    pub bytes: u64,
    /// 预览图 URL (绝对 sorahk://extmod/… 形态; bridge.js 统一规范化后给 <img>; 无 = null)。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preview_url: Option<String>,
}

/// ZIP 中央目录条目 (解析自 central directory; 声明大小/CRC 以 CD 为准 ——
/// data descriptor 形态的本地头大小字段为 0, 从 CD 读才是权威值)。
#[derive(Debug)]
struct CdEntry {
    name: String,
    method: u16,
    crc: u32,
    comp_size: u64,
    uncomp_size: u64,
    local_offset: u64,
}

// ═══════════════════════════════════════════════════════════════════════
// ZIP 精简只读解析器 (标准 PKZIP 布局; 只读, 不写 ZIP)
// ═══════════════════════════════════════════════════════════════════════

fn rd_u16(b: &[u8], off: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(off..off + 2)?.try_into().ok()?))
}
fn rd_u32(b: &[u8], off: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(off..off + 4)?.try_into().ok()?))
}

/// 从包体字节解析中央目录 (EOCD 从尾部向前搜, 最多 65557 字节注释区)。
/// 任一结构异常 (签名/截断/zip64/加密/未知方法) → Err(原因)。
fn parse_central_directory(data: &[u8]) -> Result<Vec<CdEntry>, String> {
    if data.len() < 22 {
        return Err("文件太小, 不是 ZIP/.dvmod 包".into());
    }
    // ① EOCD: 从尾部向前搜签名 0x06054b50 (最多扫 65535 字节注释区)
    let mut eocd = None;
    let scan_start = data.len().saturating_sub(22 + 65535);
    let mut i = data.len() - 22;
    while i >= scan_start {
        if rd_u32(data, i) == Some(0x0605_4b50) {
            eocd = Some(i);
            break;
        }
        if i == 0 {
            break;
        }
        i -= 1;
    }
    let Some(eocd) = eocd else {
        return Err("找不到 ZIP 目录结尾 (EOCD), 文件损坏或不是 .dvmod 包".into());
    };
    let disk = rd_u16(data, eocd + 4).unwrap_or(0);
    let cd_disk = rd_u16(data, eocd + 6).unwrap_or(0);
    if disk != 0 || cd_disk != 0 {
        return Err("多卷 ZIP 不支持 (.dvmod 必须是单文件包)".into());
    }
    let count = rd_u16(data, eocd + 10).unwrap_or(0) as usize;
    if count > MAX_ENTRIES {
        return Err(format!("包内条目数 {count} 超上限 {MAX_ENTRIES} (ZIP 炸弹防护)"));
    }
    let cd_off = rd_u32(data, eocd + 16).unwrap_or(0) as usize;
    if cd_off >= data.len() {
        return Err("ZIP 目录偏移越界, 文件损坏".into());
    }

    // ② 逐条解析中央目录条目 (固定 46 字节头 + 变长名/extra/注释)
    let mut out = Vec::with_capacity(count);
    let mut p = cd_off;
    for idx in 0..count {
        if p + 46 > data.len() {
            return Err(format!("ZIP 目录第 {idx} 条截断, 文件损坏"));
        }
        if rd_u32(data, p) != Some(0x0201_4b50) {
            return Err(format!("ZIP 目录第 {idx} 条签名不符, 文件损坏"));
        }
        let flags = rd_u16(data, p + 8).unwrap_or(0);
        if flags & 0x0001 != 0 {
            return Err(format!("条目 {} 已加密, .dvmod 不支持加密包", idx));
        }
        let method = rd_u16(data, p + 10).unwrap_or(0);
        if method != 0 && method != 8 {
            return Err(format!(
                "条目 {} 压缩方法 {method} 不支持 (仅支持 store/deflate; 请用常规 ZIP 设置重新打包)",
                idx
            ));
        }
        let crc = rd_u32(data, p + 16).unwrap_or(0);
        let comp_size = rd_u32(data, p + 20).unwrap_or(0) as u64;
        let uncomp_size = rd_u32(data, p + 24).unwrap_or(0) as u64;
        // 0xFFFFFFFF = zip64 标记 (≥4GB 条目/偏移) —— 上限内不可能出现, 明确拒绝
        if comp_size == u32::MAX as u64 || uncomp_size == u32::MAX as u64 {
            return Err("ZIP64 格式不支持 (.dvmod 上限 100MB, 不可能用到 zip64)".into());
        }
        let name_len = rd_u16(data, p + 28).unwrap_or(0) as usize;
        let extra_len = rd_u16(data, p + 30).unwrap_or(0) as usize;
        let cmt_len = rd_u16(data, p + 32).unwrap_or(0) as usize;
        let local_offset = rd_u32(data, p + 42).unwrap_or(0) as u64;
        let name_bytes = data
            .get(p + 46..p + 46 + name_len)
            .ok_or_else(|| format!("条目 {idx} 文件名截断, 文件损坏"))?;
        let name = String::from_utf8(name_bytes.to_vec())
            .map_err(|_| format!("条目 {idx} 文件名不是 UTF-8, .dvmod 要求 UTF-8 文件名"))?;
        out.push(CdEntry { name, method, crc, comp_size, uncomp_size, local_offset });
        p += 46 + name_len + extra_len + cmt_len;
    }
    Ok(out)
}

/// 条目路径安全校验 (与 safe_extmod_rel 同款思路, 但面向包内布局):
/// 禁绝对路径 / `..` / `.` / 空段 / 反斜杠 / 盘符 / 控制字符。合法返回分段切片。
fn safe_entry_segments(name: &str) -> Result<Vec<&str>, String> {
    if name.is_empty() {
        return Err("存在空文件名条目".into());
    }
    if name.contains('\\') {
        return Err(format!("条目「{name}」含反斜杠 (打包时必须用正斜杠)"));
    }
    if name.contains(':') {
        return Err(format!("条目「{name}」含盘符/冒号"));
    }
    if name.starts_with('/') {
        return Err(format!("条目「{name}」是绝对路径"));
    }
    if name.chars().any(|c| c.is_control()) {
        return Err(format!("条目「{name}」含控制字符"));
    }
    let segs: Vec<&str> = name.split('/').collect();
    if segs.iter().any(|s| s.is_empty() || *s == "." || *s == "..") {
        return Err(format!("条目「{name}」含空段/./.. (路径穿越防护)"));
    }
    Ok(segs)
}

/// 解码单条目: 从本地头定位数据区 → stored 直取 / deflate 解码 (按声明大小硬 cap)。
fn decode_entry(data: &[u8], e: &CdEntry) -> Result<Vec<u8>, String> {
    if e.uncomp_size > MAX_FILE_BYTES {
        return Err(format!(
            "文件「{}」解压后 {} 字节超单文件上限 {MAX_FILE_BYTES}",
            e.name, e.uncomp_size
        ));
    }
    let off = e.local_offset as usize;
    if off + 30 > data.len() || rd_u32(data, off) != Some(0x0403_4b50) {
        return Err(format!("文件「{}」本地头损坏", e.name));
    }
    // 本地头的名/extra 长度可能与 CD 不同 (CD 才有正确大小), 必须按本地头跳过
    let ln = rd_u16(data, off + 26).unwrap_or(0) as usize;
    let le = rd_u16(data, off + 28).unwrap_or(0) as usize;
    let data_start = off + 30 + ln + le;
    let data_end = data_start.checked_add(e.comp_size as usize).ok_or_else(|| {
        format!("文件「{}」大小字段异常", e.name)
    })?;
    let raw = data
        .get(data_start..data_end)
        .ok_or_else(|| format!("文件「{}」数据区越界 (截断包)", e.name))?;
    let decoded = match e.method {
        0 => raw.to_vec(),
        8 => {
            let mut out = Vec::with_capacity(e.uncomp_size.min(MAX_FILE_BYTES) as usize);
            let mut dec = flate2::read::DeflateDecoder::new(raw).take(MAX_FILE_BYTES + 1);
            dec.read_to_end(&mut out)
                .map_err(|_| format!("文件「{}」deflate 解码失败 (数据损坏)", e.name))?;
            out
        }
        m => return Err(format!("文件「{}」压缩方法 {m} 不支持", e.name)),
    };
    if decoded.len() as u64 != e.uncomp_size {
        return Err(format!(
            "文件「{}」解压后 {} 字节 ≠ 声明 {} (包损坏或炸弹防护触发)",
            e.name,
            decoded.len(),
            e.uncomp_size
        ));
    }
    let crc = crc32fast::hash(&decoded);
    if crc != e.crc {
        return Err(format!("文件「{}」CRC-32 校验失败 (包损坏)", e.name));
    }
    Ok(decoded)
}

// ═══════════════════════════════════════════════════════════════════════
// manifest 校验 + 安装
// ═══════════════════════════════════════════════════════════════════════

fn validate_manifest(m: &DvmodManifest) -> Result<(), String> {
    if !super::mods::valid_mod_id(&m.id) {
        return Err(format!(
            "manifest id「{}」非法 (要求: 小写字母/数字/下划线/连字符, 1-64 位)",
            m.id
        ));
    }
    if RESERVED_IDS.contains(&m.id.as_str()) {
        return Err(format!("manifest id「{}」是保留字, 不能使用", m.id));
    }
    let kind = if m.kind.is_empty() { "shell" } else { m.kind.as_str() };
    if kind != "shell" && kind != "overlay" {
        return Err(format!("manifest type「{}」非法 (只支持 shell/overlay)", m.kind));
    }
    if m.entry.trim().is_empty() {
        return Err("manifest 缺少 entry (包入口文件路径)".into());
    }
    safe_entry_segments(m.entry.trim().trim_start_matches('/'))
        .map(|_| ())
        .map_err(|e| format!("manifest entry 非法: {e}"))?;
    if let Some(p) = &m.preview {
        if !p.trim().is_empty() {
            safe_entry_segments(p.trim().trim_start_matches('/'))
                .map(|_| ())
                .map_err(|e| format!("manifest preview 非法: {e}"))?;
        }
    }
    Ok(())
}

/// 导入一个 .dvmod 文件到 `mods_root` (同 id 已存在 = 覆盖替换)。
/// 全量校验先行, 旧包在解包成功前绝不触碰; 失败时临时目录直删不留垃圾。
pub fn install_from_file(archive_path: &Path, mods_root: &Path) -> Result<InstallReport, String> {
    let shown = archive_path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| archive_path.display().to_string());

    // ① 包体读入 (上限防护; .dvmod 定位为小体积主题包, 100MB 足够宽裕)
    let meta = std::fs::metadata(archive_path)
        .map_err(|e| format!("读不到包文件「{shown}」: {e}"))?;
    if !meta.is_file() {
        return Err(format!("「{shown}」不是文件"));
    }
    if meta.len() > MAX_ARCHIVE_BYTES {
        return Err(format!(
            "包体 {} MB 超上限 {} MB",
            meta.len() / 1024 / 1024,
            MAX_ARCHIVE_BYTES / 1024 / 1024
        ));
    }
    let data = std::fs::read(archive_path)
        .map_err(|e| format!("读取包文件「{shown}」失败: {e}"))?;
    if data.is_empty() {
        return Err(format!("包文件「{shown}」是空的"));
    }

    // ② ZIP 目录解析 + 条目级安全校验 (结构/加密/方法/路径/数量/大小)
    let entries = parse_central_directory(&data)?;
    let mut total_uncomp: u64 = 0;
    for e in &entries {
        safe_entry_segments(&e.name)?;
        total_uncomp += e.uncomp_size;
    }
    if total_uncomp > MAX_TOTAL_BYTES {
        return Err(format!(
            "解压总量 {total_uncomp} 字节超上限 {MAX_TOTAL_BYTES} (ZIP 炸弹防护)"
        ));
    }

    // ③ manifest: 必须存在于包根
    let Some(manifest_idx) = entries.iter().position(|e| e.name == MANIFEST_NAME) else {
        return Err(format!(
            "包内缺少清单 {MANIFEST_NAME} (规范见 docs/dvmod格式规范_v1.md)"
        ));
    };
    let manifest_bytes = decode_entry(&data, &entries[manifest_idx])?;
    let manifest_text = String::from_utf8(manifest_bytes)
        .map_err(|_| format!("{MANIFEST_NAME} 不是 UTF-8 文本"))?;
    let manifest: DvmodManifest = toml::from_str(&manifest_text)
        .map_err(|e| format!("{MANIFEST_NAME} 解析失败: {e}"))?;
    validate_manifest(&manifest)?;

    // ④ entry / preview 必须在包内真实存在
    let entry_rel = manifest.entry.trim().trim_start_matches('/').to_string();
    if !entries.iter().any(|e| e.name == entry_rel) {
        return Err(format!("manifest entry「{entry_rel}」在包内不存在"));
    }
    let preview_rel = manifest
        .preview
        .as_ref()
        .map(|p| p.trim().trim_start_matches('/').to_string())
        .filter(|p| !p.is_empty());
    if let Some(p) = &preview_rel {
        if !entries.iter().any(|e| e.name == *p) {
            return Err(format!("manifest preview「{p}」在包内不存在"));
        }
    }

    // ⑤ 解包到临时目录 (全部写入前不做任何替换)
    let tag = format!(
        "import-{}-{}-{}",
        manifest.id,
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .subsec_nanos()
    );
    let temp_dir = mods_root.join(format!(".{tag}.tmp"));
    let cleanup = |d: &Path| {
        let _ = std::fs::remove_dir_all(d);
    };
    if let Err(e) = std::fs::create_dir_all(&temp_dir) {
        return Err(format!("创建临时目录失败: {e}"));
    }
    let kind = if manifest.kind.is_empty() {
        "shell".to_string()
    } else {
        manifest.kind.clone()
    };
    // ★运行时适配 mod.json 先生成 (既有 scan_external 认 mod.json —— 零侵入, 目录
    // 机制不变); 包内若自带 mod.json 以本生成版为准。mod.toml 保留作规范真源。
    let mut mod_json = serde_json::json!({
        "id": manifest.id,
        "name": if manifest.name.is_empty() { &manifest.id } else { &manifest.name },
        "version": manifest.version,
        "engine": manifest.engine,
        "author": manifest.author,
        "description": manifest.description,
        "type": kind,
        "entry": entry_rel,
    });
    // 预览图存在性: manifest 声明了, 或包内自带根 preview.png (两者都会落到包根)
    let has_preview = preview_rel.is_some()
        || entries.iter().any(|e| e.name == "preview.png");
    if has_preview {
        mod_json
            .as_object_mut()
            .expect("json! 宏必为对象")
            .insert("preview".into(), "preview.png".into());
    }
    let mod_json_txt = serde_json::to_string_pretty(&mod_json)
        .map_err(|e| format!("生成 mod.json 失败: {e}"))?;
    let result = (|| -> Result<(usize, u64), String> {
        let mut files = 0usize;
        let mut bytes = 0u64;
        let mut write_one = |rel_from_temp: &Path, content: &[u8]| -> Result<(), String> {
            if let Some(p) = rel_from_temp.parent() {
                std::fs::create_dir_all(p)
                    .map_err(|err| format!("建目录 {} 失败: {err}", p.display()))?;
            }
            std::fs::write(rel_from_temp, content)
                .map_err(|err| format!("写文件 {} 失败: {err}", rel_from_temp.display()))?;
            Ok(())
        };
        for e in &entries {
            // 目录条目 (末尾 '/'): 只建目录不写文件
            if e.name.ends_with('/') {
                continue;
            }
            let decoded = decode_entry(&data, e)?;
            let segs = safe_entry_segments(&e.name)?; // 已校验过, 双保险
            let mut dest = temp_dir.clone();
            for s in &segs {
                dest.push(s);
            }
            write_one(&dest, &decoded)?;
            files += 1;
            bytes += decoded.len() as u64;
        }
        // ★预览图统一落包根 preview.png (运行时扫描器/前端卡片的取图约定):
        // manifest 声明了别的路径 → 复制一份到包根; 声明了 preview.png → 原样已在。
        if let Some(p) = &preview_rel {
            if p != "preview.png" {
                let src = temp_dir.join(p);
                let bytes2 =
                    std::fs::read(&src).map_err(|err| format!("复制预览图失败: {err}"))?;
                write_one(&temp_dir.join("preview.png"), &bytes2)?;
                files += 1;
                bytes += bytes2.len() as u64;
            }
        }
        write_one(&temp_dir.join("mod.json"), mod_json_txt.as_bytes())?;
        files += 1;
        bytes += mod_json_txt.len() as u64;
        Ok((files, bytes))
    })();

    let (files, bytes) = match result {
        Ok(v) => v,
        Err(e) => {
            cleanup(&temp_dir);
            return Err(e);
        }
    };

    // ⑥ 换名安装: 旧包先挪走 (覆盖替换), 成功后直删; 失败把旧包挪回来
    let final_dir = mods_root.join(&manifest.id);
    let replaced = final_dir.is_dir();
    let backup = mods_root.join(format!(".{tag}.old"));
    if replaced {
        if let Err(e) = std::fs::rename(&final_dir, &backup) {
            cleanup(&temp_dir);
            return Err(format!("替换旧包失败 (目录被占用?): {e}"));
        }
    }
    if let Err(e) = std::fs::rename(&temp_dir, &final_dir) {
        if replaced {
            let _ = std::fs::rename(&backup, &final_dir); // 尽力还原旧包
        }
        cleanup(&temp_dir);
        return Err(format!("安装目录换名失败: {e}"));
    }
    cleanup(&backup);

    // 预览图 URL 固定指向包根 preview.png (导入时已统一落到该位置)
    let preview_url = if has_preview {
        Some(format!("sorahk://extmod/{}/preview.png", manifest.id))
    } else {
        None
    };
    Ok(InstallReport {
        name: if manifest.name.is_empty() { manifest.id.clone() } else { manifest.name },
        id: manifest.id,
        version: manifest.version,
        replaced,
        files,
        bytes,
        preview_url,
    })
}

/// 解析 `--install-mod <路径>` / `--install-mod=<路径>` (可多个, 按出现顺序; 自动化验收通道)。
pub fn parse_install_mod_args(argv: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < argv.len() {
        let a = &argv[i];
        if let Some(rest) = a.strip_prefix("--install-mod=") {
            let v = rest.trim().trim_matches('"');
            if !v.is_empty() {
                out.push(v.to_string());
            } else {
                eprintln!("[sorahk] --install-mod= 缺少非空路径, 参数被忽略");
            }
        } else if a == "--install-mod" {
            match argv.get(i + 1) {
                Some(v) if !v.trim().is_empty() && !v.starts_with("--") => {
                    out.push(v.trim().trim_matches('"').to_string());
                    i += 1;
                }
                _ => eprintln!(
                    "[sorahk] --install-mod 缺少 <路径.dvmod> 参数 (用法: --install-mod <路径>), 参数被忽略"
                ),
            }
        }
        i += 1;
    }
    out
}

/// CLI 导入执行 (main.rs --install-mod 分流; 不起钩子/窗口/托盘): 逐包安装并打印结果,
/// 任一失败 → 返回 Err (进程非 0 退出)。
pub fn run_install_mod(paths: &[String]) -> anyhow::Result<()> {
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .ok_or_else(|| anyhow::anyhow!("--install-mod: 无法定位 exe 目录"))?;
    let mods_root = super::mods::mods_root_for_exe_dir(&exe_dir);
    println!(
        "[install-mod] 目标目录: {}",
        mods_root.display()
    );
    let mut failed = false;
    for p in paths {
        match install_from_file(Path::new(p), &mods_root) {
            Ok(r) => {
                println!(
                    "[install-mod] OK id={} name=\"{}\" v={} files={} bytes={} {}",
                    r.id,
                    r.name,
                    r.version,
                    r.files,
                    r.bytes,
                    if r.replaced { "(覆盖替换)" } else { "(新安装)" }
                );
            }
            Err(e) => {
                println!("[install-mod] FAIL 「{p}」: {e}");
                failed = true;
            }
        }
    }
    if failed {
        anyhow::bail!("--install-mod: 存在导入失败的包 (见上方输出)");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// 测试沙箱守卫: Drop 时直删整目录 (panic 回收也走 Drop, 零残留)。
    struct TmpDir(PathBuf);
    impl std::ops::Deref for TmpDir {
        type Target = PathBuf;
        fn deref(&self) -> &PathBuf {
            &self.0
        }
    }
    impl Drop for TmpDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    /// .dvmod 测试包 = 文件 + 所在沙箱目录 (同守卫)。
    struct Pkg {
        path: PathBuf,
        _dir: TmpDir,
    }
    impl std::ops::Deref for Pkg {
        type Target = Path;
        fn deref(&self) -> &Path {
            &self.path
        }
    }

    /// 测试沙箱 (E 盘直建, C 盘零写入; 用完直删)。
    fn tmpdir(tag: &str) -> TmpDir {
        let d = std::env::temp_dir().join(format!(
            "sorahk_dvmod_{tag}_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .subsec_nanos()
        ));
        std::fs::create_dir_all(&d).unwrap();
        TmpDir(d)
    }

    // ── 测试用最小 ZIP 写入器 (stored; 无 extra/注释, 名为 UTF-8) ────────────
    struct StoredWriter {
        body: Vec<u8>,
        cd: Vec<u8>,
        count: u16,
    }
    impl StoredWriter {
        fn new() -> Self {
            Self { body: Vec::new(), cd: Vec::new(), count: 0 }
        }
        fn add(&mut self, name: &str, data: &[u8], crc_override: Option<u32>) {
            self.add_raw(name, data, data, 0, crc_override);
        }
        /// method=8 deflate 条目 (锁解码路径覆盖)。
        fn add_deflate(&mut self, name: &str, data: &[u8]) {
            let mut enc =
                flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
            std::io::Write::write_all(&mut enc, data).unwrap();
            let comp = enc.finish().unwrap();
            self.add_raw(name, &comp, data, 8, None);
        }
        /// 统一布局写入 (local header + data + central directory 条目)。
        fn add_raw(
            &mut self,
            name: &str,
            stored: &[u8],
            raw: &[u8],
            method: u16,
            crc_override: Option<u32>,
        ) {
            let off = self.body.len() as u32;
            let nb = name.as_bytes();
            let crc = crc_override.unwrap_or_else(|| crc32fast::hash(raw));
            self.body.extend_from_slice(&0x0403_4b50u32.to_le_bytes()); // local sig
            self.body.extend_from_slice(&20u16.to_le_bytes()); // version
            self.body.extend_from_slice(&0u16.to_le_bytes()); // flags
            self.body.extend_from_slice(&method.to_le_bytes());
            self.body.extend_from_slice(&0u16.to_le_bytes()); // time
            self.body.extend_from_slice(&0u16.to_le_bytes()); // date
            self.body.extend_from_slice(&crc.to_le_bytes());
            self.body.extend_from_slice(&(stored.len() as u32).to_le_bytes());
            self.body.extend_from_slice(&(raw.len() as u32).to_le_bytes());
            self.body.extend_from_slice(&(nb.len() as u16).to_le_bytes());
            self.body.extend_from_slice(&0u16.to_le_bytes()); // extra
            self.body.extend_from_slice(nb);
            self.body.extend_from_slice(stored);
            // central directory entry
            self.cd.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
            self.cd.extend_from_slice(&20u16.to_le_bytes());
            self.cd.extend_from_slice(&20u16.to_le_bytes());
            self.cd.extend_from_slice(&0u16.to_le_bytes()); // flags
            self.cd.extend_from_slice(&method.to_le_bytes());
            self.cd.extend_from_slice(&0u16.to_le_bytes()); // time
            self.cd.extend_from_slice(&0u16.to_le_bytes()); // date
            self.cd.extend_from_slice(&crc.to_le_bytes());
            self.cd.extend_from_slice(&(stored.len() as u32).to_le_bytes());
            self.cd.extend_from_slice(&(raw.len() as u32).to_le_bytes());
            self.cd.extend_from_slice(&(nb.len() as u16).to_le_bytes());
            self.cd.extend_from_slice(&0u16.to_le_bytes()); // extra
            self.cd.extend_from_slice(&0u16.to_le_bytes()); // comment
            self.cd.extend_from_slice(&0u16.to_le_bytes()); // disk
            self.cd.extend_from_slice(&0u16.to_le_bytes()); // int attrs
            self.cd.extend_from_slice(&0u32.to_le_bytes()); // ext attrs
            self.cd.extend_from_slice(&off.to_le_bytes()); // local offset
            self.cd.extend_from_slice(nb);
            self.count += 1;
        }
        fn finish(mut self) -> Vec<u8> {
            let cd_off = self.body.len() as u32;
            let cd_len = self.cd.len() as u32;
            self.body.extend_from_slice(&self.cd);
            // EOCD
            self.body.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
            self.body.extend_from_slice(&0u16.to_le_bytes());
            self.body.extend_from_slice(&0u16.to_le_bytes());
            self.body.extend_from_slice(&self.count.to_le_bytes());
            self.body.extend_from_slice(&self.count.to_le_bytes());
            self.body.extend_from_slice(&cd_len.to_le_bytes());
            self.body.extend_from_slice(&cd_off.to_le_bytes());
            self.body.extend_from_slice(&0u16.to_le_bytes()); // comment len
            self.body
        }
    }

    /// deflate 压缩单文件 ZIP (复用 flate2, 锁 deflate 路径)。
    fn deflate_zip(name: &str, data: &[u8]) -> Vec<u8> {
        let mut w = StoredWriter::new();
        w.add_deflate(name, data);
        w.finish()
    }

    const MANIFEST_OK: &str = "\
id = \"mytheme\"\n\
name = \"我的主题\"\n\
version = \"1.2.0\"\n\
engine = \"R2.14+\"\n\
author = \"tester\"\n\
description = \"d\"\n\
type = \"shell\"\n\
entry = \"index.html\"\n\
preview = \"preview.png\"\n";

    /// 无 preview 声明的极简 manifest (deflate 子包用)。
    const MANIFEST_NOPREV: &str = "\
id = \"mytheme\"\n\
name = \"我的主题\"\n\
version = \"1.2.0\"\n\
engine = \"R2.14+\"\n\
type = \"shell\"\n\
entry = \"index.html\"\n";

    fn good_zip() -> Vec<u8> {
        let mut w = StoredWriter::new();
        w.add(MANIFEST_NAME, MANIFEST_OK.as_bytes(), None);
        w.add("index.html", b"<h1>hello dvmod</h1>", None);
        w.add("preview.png", &[0x89, 0x50, 0x4E, 0x47, 1, 2, 3], None);
        w.add("assets/sub/x.css", b"body{}", None);
        w.finish()
    }

    fn write_pkg(tag: &str, bytes: &[u8]) -> Pkg {
        let d = tmpdir(tag);
        let p = d.join(format!("{tag}.dvmod"));
        std::fs::write(&p, bytes).unwrap();
        Pkg { path: p, _dir: d }
    }

    #[test]
    fn install_ok_stored_and_deflate() {
        // stored 全链路
        let p = write_pkg("ok", &good_zip());
        let root = tmpdir("ok_root");
        let r = install_from_file(&p, &root).unwrap();
        assert_eq!(r.id, "mytheme");
        assert_eq!(r.name, "我的主题");
        assert_eq!(r.version, "1.2.0");
        assert!(!r.replaced);
        assert_eq!(r.files, 5, "manifest + 入口 + 预览图 + 子目录文件 + 生成 mod.json");
        assert!(r.bytes > 0);
        assert_eq!(
            r.preview_url.as_deref(),
            Some("sorahk://extmod/mytheme/preview.png")
        );
        // 磁盘形态 = 普通 mods/<id>/ 目录, scan_external 必须能识别
        assert!(root.join("mytheme/index.html").is_file());
        assert!(root.join("mytheme/assets/sub/x.css").is_file());
        assert!(root.join("mytheme/mod.toml").is_file());
        assert!(root.join("mytheme/mod.json").is_file(), "运行时适配 mod.json 必须生成");
        let scanned = super::super::mods::scan_external(&root);
        assert_eq!(scanned.len(), 1, "dvmod 装出来必须能被既有扫描识别");
        assert_eq!(scanned[0].id, "mytheme");
        assert!(scanned[0].compatible);

        // deflate 路径 (manifest stored + 入口/资源 deflate; 无预览图变体)
        let mut w = StoredWriter::new();
        w.add(MANIFEST_NAME, MANIFEST_NOPREV.as_bytes(), None);
        w.add_deflate("index.html", b"<h1>deflate entry</h1>");
        w.add_deflate("assets/sub/deep.css", b"body{color:red}");
        let p3 = write_pkg("ok_deflate", &w.finish());
        let root3 = tmpdir("ok_deflate_root");
        let r3 = install_from_file(&p3, &root3).unwrap();
        assert_eq!(r3.id, "mytheme");
        assert_eq!(
            std::fs::read(root3.join("mytheme/index.html")).unwrap(),
            b"<h1>deflate entry</h1>"
        );
        assert_eq!(
            std::fs::read(root3.join("mytheme/assets/sub/deep.css")).unwrap(),
            b"body{color:red}"
        );

        // deflate_zip 辅助器 (单文件包 → 缺 manifest, 报缺清单)
        let p4 = write_pkg("onlydeflate", &deflate_zip("index.html", b"x"));
        let err = install_from_file(&p4, &tmpdir("onlydeflate_root")).unwrap_err();
        assert!(err.contains(MANIFEST_NAME));
    }

    #[test]
    fn install_overwrite_replaces() {
        let p = write_pkg("ow", &good_zip());
        let root = tmpdir("ow_root");
        let r1 = install_from_file(&p, &root).unwrap();
        assert!(!r1.replaced);
        std::fs::write(root.join("mytheme/stale.txt"), b"old").unwrap();
        let r2 = install_from_file(&p, &root).unwrap();
        assert!(r2.replaced, "同 id 第二次导入 = 覆盖替换");
        assert!(
            !root.join("mytheme/stale.txt").exists(),
            "旧包内容必须整体清掉, 不能残留"
        );
        assert!(root.join("mytheme/index.html").is_file());
        // 临时目录/备份目录零残留
        let leftovers: Vec<_> = root.read_dir()
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| n.starts_with('.'))
            .collect();
        assert!(leftovers.is_empty(), "临时/备份目录必须清干净: {leftovers:?}");
    }

    #[test]
    fn reject_traversal_and_dangerous_names() {
        for name in ["../evil.txt", "..\\evil.txt", "/abs.txt", "a: b.txt", "a/../b.txt", "a/./b.txt"] {
            let mut w = StoredWriter::new();
            w.add(MANIFEST_NAME, MANIFEST_OK.as_bytes(), None);
            w.add("index.html", b"x", None);
            w.add(name, b"evil", None);
            let p = write_pkg("trav", &w.finish());
            let root = tmpdir("trav_root");
            let err = install_from_file(&p, &root).unwrap_err();
            assert!(
                err.contains("反斜杠") || err.contains("绝对路径") || err.contains("盘符")
                    || err.contains("./.."),
                "「{name}」应被路径校验拒绝, 实际报错: {err}"
            );
            assert!(!root.join(".import-mytheme.tmp").exists(), "失败不留临时目录");
        }
    }

    #[test]
    fn reject_encrypted_zip64_bad_method() {
        // 加密位 (flags bit0)
        let mut data = good_zip();
        // 找 central directory 签名, 改第一处 flags bit0 (CD 条目偏移 +8)
        let cd_pos = data
            .windows(4)
            .position(|w| w == 0x0201_4b50u32.to_le_bytes())
            .unwrap();
        let flags = u16::from_le_bytes([data[cd_pos + 8], data[cd_pos + 9]]);
        data[cd_pos + 8] = (flags | 1) as u8;
        let p = write_pkg("enc", &data);
        let err = install_from_file(&p, &tmpdir("enc_root")).unwrap_err();
        assert!(err.contains("加密"), "实际: {err}");

        // 未知压缩方法 (method=12 bzip2)
        let mut data = good_zip();
        let cd_pos = data
            .windows(4)
            .position(|w| w == 0x0201_4b50u32.to_le_bytes())
            .unwrap();
        data[cd_pos + 10] = 12;
        data[cd_pos + 11] = 0;
        let p = write_pkg("meth", &data);
        let err = install_from_file(&p, &tmpdir("meth_root")).unwrap_err();
        assert!(err.contains("压缩方法"), "实际: {err}");

        // zip64 标记 (uncomp_size = 0xFFFFFFFF)
        let mut data = good_zip();
        let cd_pos = data
            .windows(4)
            .position(|w| w == 0x0201_4b50u32.to_le_bytes())
            .unwrap();
        for i in 0..4 {
            data[cd_pos + 24 + i] = 0xFF;
        }
        let p = write_pkg("z64", &data);
        let err = install_from_file(&p, &tmpdir("z64_root")).unwrap_err();
        assert!(err.contains("zip64"), "实际: {err}");
    }

    #[test]
    fn reject_bad_crc_and_size_mismatch() {
        // CRC 篡改 (改第一条 CD 条目的 CRC 字段 —— 解码以 CD 为准)
        let mut data = good_zip();
        let cd_pos = data
            .windows(4)
            .position(|w| w == 0x0201_4b50u32.to_le_bytes())
            .unwrap();
        let crc_off = cd_pos + 16; // CD 头布局: sig(4) vm(2) vn(2) flags(2) method(2) time(2) date(2) crc@16
        data[crc_off] = data[crc_off].wrapping_add(1);
        let p = write_pkg("crc", &data);
        let err = install_from_file(&p, &tmpdir("crc_root")).unwrap_err();
        assert!(err.contains("CRC"), "实际: {err}");

        // 声明大小与实际不符 (CD uncomp_size 改小 → decode mismatch)
        let mut data = good_zip();
        let cd_pos = data
            .windows(4)
            .position(|w| w == 0x0201_4b50u32.to_le_bytes())
            .unwrap();
        // 第一条 CD = manifest 条目, uncomp_size 在 CD 头 +24
        data[cd_pos + 24] = 1;
        let p = write_pkg("sz", &data);
        let err = install_from_file(&p, &tmpdir("sz_root")).unwrap_err();
        assert!(
            err.contains("解压后") || err.contains("≠") || err.contains("manifest"),
            "实际: {err}"
        );
    }

    #[test]
    fn reject_manifest_problems() {
        // 无 manifest
        let mut w = StoredWriter::new();
        w.add("index.html", b"x", None);
        let p = write_pkg("noman", &w.finish());
        let err = install_from_file(&p, &tmpdir("noman_root")).unwrap_err();
        assert!(err.contains(MANIFEST_NAME), "实际: {err}");

        // id 非法 / 保留字 / entry 缺失 / entry 不存在
        for (toml_text, expect) in [
            ("id = \"Endfield\"\nentry = \"i.html\"", "非法"),
            ("id = \"endfield\"\nentry = \"i.html\"", "保留字"),
            ("id = \"goodid\"\n", "entry"),
            ("id = \"goodid\"\nentry = \"missing.html\"", "不存在"),
            ("id = \"goodid\"\nentry = \"../up.html\"", "非法"),
            ("entry = \"i.html\"", "解析失败"),
        ] {
            let mut w = StoredWriter::new();
            w.add(MANIFEST_NAME, toml_text.as_bytes(), None);
            w.add("i.html", b"x", None);
            let p = write_pkg("man", &w.finish());
            let err = install_from_file(&p, &tmpdir("man_root")).unwrap_err();
            assert!(
                err.contains(expect),
                "toml[{toml_text}] 应报「{expect}」, 实际: {err}"
            );
        }
    }

    #[test]
    fn reject_not_zip_and_garbage() {
        let root = tmpdir("garb_root");
        // 纯文本
        let p = write_pkg("txt", b"this is not a zip file at all.......");
        let err = install_from_file(&p, &root).unwrap_err();
        assert!(err.contains("EOCD") || err.contains("ZIP"), "实际: {err}");
        // 空文件
        let p = write_pkg("empty", b"");
        let err = install_from_file(&p, &root).unwrap_err();
        assert!(err.contains("空") || err.contains("太小"), "实际: {err}");
        // 不存在的文件
        let err = install_from_file(&root.join("ghost.dvmod"), &root).unwrap_err();
        assert!(err.contains("读不到"), "实际: {err}");
        // 旧包不被垃圾包破坏 (先装好包, 再导垃圾, 旧包仍在)
        let p_good = write_pkg("survive", &good_zip());
        install_from_file(&p_good, &root).unwrap();
        let p_bad = write_pkg("bad", b"garbage");
        assert!(install_from_file(&p_bad, &root).is_err());
        assert!(root.join("mytheme/index.html").is_file(), "失败导入不得破坏已装包");
    }

    #[test]
    fn reserved_ids_and_install_args() {
        assert!(RESERVED_IDS.contains(&"endfield"));
        assert!(RESERVED_IDS.contains(&"classic"));
        // CLI 参数解析
        let argv: Vec<String> = [
            "--install-mod",
            "E:\\a\\x.dvmod",
            "--shell",
            "--install-mod=E:\\b\\y.dvmod",
            "--install-mod",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let got = parse_install_mod_args(&argv);
        assert_eq!(got, vec!["E:\\a\\x.dvmod", "E:\\b\\y.dvmod"]);
        // 缺参数不 panic 只忽略
        assert!(parse_install_mod_args(&["--install-mod".to_string()]).is_empty());
        assert!(parse_install_mod_args(&[]).is_empty());
    }
}
