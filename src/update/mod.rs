//! ★v24.55 检查更新 —— 版本判定 / 更新清单解析 / 后台检查与下载。
//!
//! 设计约束 (Owner 需求):
//! 1. **不强制更新**: 实现里没有任何"必须更新才能用"的分支; 有新版本只驱动一个
//!    会自动淡出的小浮窗, 用户关掉就永不再提这个版本 (除非换版本或手动检查)。
//! 2. **发行包可选**: 基础引擎 (无音频震动, ~10MB) 与 完全版 (音频震动 + 后续全部
//!    MOD, ~49MB) 同时给出, 由用户点哪个下哪个 —— 不在后台偷偷替换 exe。
//! 3. **零新增运行时依赖**: HTTP 走系统 WinHTTP (`http` 子模块), JSON 用 serde_json
//!    (本就在依赖树内)。整个功能在无网环境下只会显示一句"检查失败", 不影响主功能。
//!
//! 清单源按顺序试, 第一个成功的赢 (见 [`sources`]):
//!   自定义地址 (Config.toml `[update] manifest_url`) → jsDelivr → raw.githubusercontent
//!   → GitHub Releases API。前两个面向国内可达性, 最后一个是"只要有 Release 就能用"。

pub mod http;
pub mod sha256;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, anyhow, bail};
use serde_json::Value;

/// 本构建的发行版本号 (与 CI 产物名 / Release tag 必须一致)。
///
/// ⚠ 发版时**只改这一行** —— ★R2.12 (优化 1) 起 `.github/workflows/build-windows.yml`
/// 从本常量读取版本号生成产物名 (`DfoVibration-Sorahk_<APP_VERSION>.exe`),
/// 不再需要同步改 workflow 里写死的文件名。
/// 语义: ★R2.1 起版本系切换为 **R X.Y** (R2.0 稳定线的延续; 旧 24.x 线停止发版),
/// 与 tag `R2.1` 去掉 R 后逐段数值比较。跨系比较规则见 [`is_newer`]。
pub const APP_VERSION: &str = "R2.11";

/// 仓库 (清单源与下载地址都指向它)。
pub const REPO: &str = "118coder/DfoVibration";

/// 自动检查的节流窗口: 4 小时内不重复自动检查 (手动检查不受限)。
pub const AUTO_CHECK_INTERVAL_SECS: i64 = 4 * 3600;

/* ═════════════════════ 版本号 ═════════════════════ */

/// 把版本号切成数值段 (`R2.1` / `v24.55` / `2.1-beta` 都能吃; 前导 v/V/r/R 一律剥掉)。
fn version_parts(v: &str) -> Vec<u64> {
    let r_series = is_r_series(v);
    v.trim()
        .trim_start_matches(['v', 'V', 'r', 'R'])
        .split(['.', '-', '_', '+'])
        .enumerate()
        .map(|(i, seg)| {
            let digits: String = seg.chars().take_while(|c| c.is_ascii_digit()).collect();
            let n = digits.parse::<u64>().unwrap_or(0);
            /* ★R2.12: R 系 minor 段按「两位十进制」比较, 修掉 R2.11 之后发 R2.2
             * 会被判成旧版、老用户收不到更新提示的问题。
             *   R2.0→[2,0] < R2.1→[2,10] < R2.11→[2,11] < R2.12→[2,12] < R2.2→[2,20]
             * 只有 1 位的 minor 才 ×10 (R2.11 / R2.12 已是两位, 原样比较),
             * 旧 24.x 数字系与跨系规则完全不受影响。 */
            if r_series && i == 1 && digits.len() == 1 {
                n * 10
            } else {
                n
            }
        })
        .collect()
}

/// 版本串是否属于 R 系 (`R2.1` / `r2.1`; 前导 v/V 先剥掉再判)。
fn is_r_series(v: &str) -> bool {
    v.trim()
        .trim_start_matches(['v', 'V'])
        .starts_with(['r', 'R'])
}

/// `latest` 是否比 `current` 新。
///
/// ★R2.1 版本系切换的跨系规则: `R` 系标签与旧 `24.x` 数字系比较时 **R 系恒为新**
/// (24.x 线已停止发版, R2.x 是唯一现役线)。同系内逐段数值比较, 缺段补 0 ——
/// 所以 R2.0 稳定版用户能正常收到 R2.1 的更新提示。
pub fn is_newer(latest: &str, current: &str) -> bool {
    match (is_r_series(latest), is_r_series(current)) {
        (true, false) => return true,
        (false, true) => return false,
        _ => {}
    }
    let a = version_parts(latest);
    let b = version_parts(current);
    for i in 0..a.len().max(b.len()) {
        let x = *a.get(i).unwrap_or(&0);
        let y = *b.get(i).unwrap_or(&0);
        if x != y {
            return x > y;
        }
    }
    false
}

#[cfg(test)]
mod version_tests {
    use super::*;

    #[test]
    fn r_series_comparison() {
        // 同系: R2.0 稳定版用户 → R2.1 提示更新
        assert!(is_newer("R2.1", "R2.0"));
        assert!(is_newer("R2.1", "r2.0"));
        assert!(!is_newer("R2.1", "R2.1"));
        assert!(!is_newer("R2.1", "R2.2"));
        assert!(is_newer("R2.2", "R2.1"));
        assert!(is_newer("R3.0", "R2.9"));
    }

    #[test]
    fn cross_series_r_beats_legacy_numeric() {
        // 跨系: R 系恒新于旧 24.x 数字系 (24.x 已停发)
        assert!(is_newer("R2.1", "24.61"));
        assert!(is_newer("R2.1", "24.59"));
        assert!(is_newer("R2.0", "24.61"), "R2.0 也新于 24.x (跨系规则)");
        assert!(!is_newer("24.62", "R2.1"), "旧系标签永远不算新");
    }

    /// ★R2.12: 短 minor (R2.2) 必须比两位 minor (R2.11/R2.12) 更新 ——
    /// 修 R2.11 交接 §4 记录的「按段数值比较 11>2 会把 R2.2 判成旧版」。
    #[test]
    fn r_series_short_minor_is_a_newer_generation() {
        assert!(is_newer("R2.2", "R2.11"), "R2.2 应比 R2.11 新");
        assert!(is_newer("R2.2", "R2.12"), "R2.2 应比 R2.12 新");
        assert!(!is_newer("R2.11", "R2.2"));
        assert!(is_newer("R2.11", "R2.1"));
        assert!(is_newer("R2.12", "R2.11"));
    }

    #[test]
    fn legacy_numeric_still_works() {
        assert!(is_newer("v24.60", "24.59"));
        assert!(is_newer("24.60.1", "24.60"));
        assert!(!is_newer("24.59", "24.60"));
    }

    /// ★R2.12 (优化 10): 失败退避必须单调递增且封顶 1 小时 ——
    /// 递增太慢等于没退避, 没有上限会让网络恢复后迟迟不重试。
    #[test]
    fn backoff_grows_then_caps_at_one_hour() {
        let seq: Vec<i64> = (1..=6).map(backoff_secs).collect();
        assert_eq!(seq, vec![60, 300, 900, 1800, 3600, 3600]);
        assert_eq!(backoff_secs(0), 60, "首次失败 1 分钟后重试");
        assert!(seq.windows(2).all(|w| w[1] >= w[0]), "退避时长不能倒退");
        assert!(*seq.last().unwrap() <= 3600, "退避上限 1 小时");
    }
}

/* ═════════════════════ 发行包 / 清单 ═════════════════════ */

/// 发行渠道: ★v24.60 起为**单包发行** (音频震动引擎内嵌, 无模型文件, 三变体合并),
/// Basic 仅保留用于兼容旧配置/旧清单, 不再会有资产被判为 Basic。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channel {
    /// 遗留渠道 (旧配置里 channel="basic" 仍能解析, 但不会有对应发行包)
    Basic,
    /// 完整版: 音频震动引擎内嵌, 开箱全功能 (唯一发行包)
    Full,
}

impl Channel {
    pub fn key(self) -> &'static str {
        match self {
            Channel::Basic => "basic",
            Channel::Full => "full",
        }
    }

    pub fn from_key(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "basic" | "slim" | "core" => Some(Channel::Basic),
            "full" | "embedded" | "mod" => Some(Channel::Full),
            _ => None,
        }
    }

    /// 渠道的默认展示名 (清单里给了 name 就用清单的)。
    pub fn default_label(self) -> &'static str {
        match self {
            Channel::Basic => "基础引擎（旧版遗留，已停发）",
            Channel::Full => "完整版（含音频震动，开箱全功能）",
        }
    }
}

/// 一个可下载的发行包。
#[derive(Debug, Clone)]
pub struct Package {
    pub channel: Channel,
    pub label: String,
    pub url: String,
    /// 字节数; 0 = 清单没给 (进度条走"未知总量"形态)
    pub size: u64,
    /// ★R2.12 (优化 10): 清单可选声明的 SHA256 (小写十六进制)。
    /// 有值就下载后强制校验, 不匹配直接删包报错 —— 将来做自动替换 exe 时的安全底线。
    pub sha256: Option<String>,
}

/// 一份更新清单 (自建 JSON 或 GitHub Release 归一后的结果)。
#[derive(Debug, Clone)]
pub struct Release {
    /// 归一后的版本号 (无 v 前缀), 如 `24.55`
    pub version: String,
    pub notes: String,
    /// 发布日期 (能取到就 `YYYY-MM-DD`, 否则空)
    pub published: String,
    pub packages: Vec<Package>,
}

impl Release {
    pub fn package(&self, ch: Channel) -> Option<&Package> {
        self.packages.iter().find(|p| p.channel == ch)
    }
}

/// 按文件名猜渠道 —— GitHub Release 的资产列表里没有渠道字段, 只能看名字。
/// ★v24.60 单包发行: 本项目的 exe 一律算 Full; 只有明确带"无音频/slim/basic"字样
/// 的旧资产才判 Basic (兼容历史 Release 的更新链路)。
fn classify_asset(name: &str) -> Option<Channel> {
    let n = name.to_lowercase();
    /* 历史包: 明确的无音频/slim 字样 */
    if n.contains("无音频") || n.contains("基础引擎") || n.contains("slim") || n.contains("basic") {
        return Some(Channel::Basic);
    }
    /* ★v24.60: 本项目主 exe (单包) + 旧命名的完全/全内嵌/音频变体 */
    if n.contains("dfovibration-sorahk")
        || n.contains("sorahk-")
        || n.contains("完全")
        || n.contains("全内嵌")
        || n.contains("音频")
        || n.contains("embedded")
        || n.contains("full")
    {
        return Some(Channel::Full);
    }
    None
}

/// 解析更新清单。两种格式都吃:
/// - 自建 `update.json`: `{version, notes, published, packages:[{channel,name,url,size}]}`
/// - GitHub Releases API: `{tag_name, body, published_at, assets:[{name,browser_download_url,size}]}`
pub fn parse_release(body: &str) -> Result<Release> {
    let v: Value = serde_json::from_str(body).context("更新清单不是合法 JSON")?;
    if v.get("packages").is_some() || v.get("version").is_some() {
        parse_own_manifest(&v)
    } else if v.get("tag_name").is_some() {
        parse_github_release(&v)
    } else {
        bail!("无法识别的更新清单格式 (既没有 version 也没有 tag_name)")
    }
}

fn parse_own_manifest(v: &Value) -> Result<Release> {
    let version = v
        .get("version")
        .and_then(Value::as_str)
        .map(|s| s.trim().trim_start_matches(['v', 'V']).to_string())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow!("清单缺少 version 字段"))?;
    let notes = v.get("notes").and_then(Value::as_str).unwrap_or("").to_string();
    let published = v
        .get("published")
        .and_then(Value::as_str)
        .unwrap_or("")
        .chars()
        .take(10)
        .collect::<String>();

    let mut packages = Vec::new();
    if let Some(list) = v.get("packages").and_then(Value::as_array) {
        for item in list {
            let Some(ch) = item
                .get("channel")
                .and_then(Value::as_str)
                .and_then(Channel::from_key)
            else {
                continue;
            };
            let Some(url) = item.get("url").and_then(Value::as_str) else {
                continue;
            };
            if url.trim().is_empty() {
                continue;
            }
            let label = item
                .get("name")
                .and_then(Value::as_str)
                .map(str::to_string)
                .unwrap_or_else(|| ch.default_label().to_string());
            packages.push(Package {
                channel: ch,
                label,
                url: url.trim().to_string(),
                size: item.get("size").and_then(Value::as_u64).unwrap_or(0),
                sha256: item
                    .get("sha256")
                    .and_then(Value::as_str)
                    .and_then(sha256::normalize_expected),
            });
        }
    }
    Ok(Release {
        version,
        notes,
        published,
        packages,
    })
}

fn parse_github_release(v: &Value) -> Result<Release> {
    let tag = v
        .get("tag_name")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("Release 缺少 tag_name"))?;
    let version = tag.trim().trim_start_matches(['v', 'V']).to_string();
    if version.is_empty() {
        bail!("Release 的 tag_name 为空");
    }
    let notes = v.get("body").and_then(Value::as_str).unwrap_or("").to_string();
    let published = v
        .get("published_at")
        .and_then(Value::as_str)
        .unwrap_or("")
        .chars()
        .take(10)
        .collect::<String>();

    let mut packages: Vec<Package> = Vec::new();
    if let Some(assets) = v.get("assets").and_then(Value::as_array) {
        for asset in assets {
            let Some(name) = asset.get("name").and_then(Value::as_str) else {
                continue;
            };
            let Some(url) = asset.get("browser_download_url").and_then(Value::as_str) else {
                continue;
            };
            let Some(ch) = classify_asset(name) else {
                continue; /* SHA256SUMS.txt / 模型包之类的附件不算发行包 */
            };
            if packages.iter().any(|p| p.channel == ch) {
                continue; /* 同渠道只认第一个 */
            }
            packages.push(Package {
                channel: ch,
                label: ch.default_label().to_string(),
                url: url.to_string(),
                size: asset.get("size").and_then(Value::as_u64).unwrap_or(0),
                /* GitHub Release 资产本身不带摘要; 若发布时把 SHA256SUMS.txt 一起传了,
                 * 下面 attempts_checksums 会补上 (R2.12 预留, 目前留 None 不阻塞下载) */
                sha256: None,
            });
        }
    }
    /* 固定顺序: 完全版在前 (体积档从大到小), 与界面呈现一致 */
    packages.sort_by_key(|p| match p.channel {
        Channel::Full => 0,
        Channel::Basic => 1,
    });
    Ok(Release {
        version,
        notes,
        published,
        packages,
    })
}

/// 按顺序尝试的清单源。
pub fn sources(custom: &str) -> Vec<String> {
    let mut list = Vec::new();
    let custom = custom.trim();
    if !custom.is_empty() {
        list.push(custom.to_string());
    }
    list.push(format!(
        "https://cdn.jsdelivr.net/gh/{REPO}@main/update.json"
    ));
    list.push(format!(
        "https://raw.githubusercontent.com/{REPO}/main/update.json"
    ));
    list.push(format!("https://api.github.com/repos/{REPO}/releases/latest"));
    list
}

/* ═════════════════════ 路径与目录 ═════════════════════ */

pub fn exe_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("."))
}

fn dir_writable(dir: &Path) -> bool {
    if std::fs::create_dir_all(dir).is_err() {
        return false;
    }
    let probe = dir.join(".sorahk_write_probe");
    match std::fs::write(&probe, b"1") {
        Ok(()) => {
            let _ = std::fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

/// 下载落点: 用户配的目录 > exe 同级 `updates\` > `%TEMP%\SorahkDFO_updates`。
/// (exe 放在 Program Files 之类只读位置时自动退到临时目录, 不让下载直接失败)
pub fn resolve_download_dir(configured: &str) -> PathBuf {
    let configured = configured.trim();
    if !configured.is_empty() {
        let p = PathBuf::from(configured);
        if dir_writable(&p) {
            return p;
        }
    }
    let beside = exe_dir().join("updates");
    if dir_writable(&beside) {
        return beside;
    }
    std::env::temp_dir().join("SorahkDFO_updates")
}

/// 百分号解码 (GitHub 资产名里的中文是 %XX 转义, 直接当文件名会是一串码点)。
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = &s[i + 1..i + 3];
            if let Ok(b) = u8::from_str_radix(hex, 16) {
                out.push(b);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

/// 从下载地址取一个干净、可写的文件名。
pub fn file_name_for(pkg: &Package, version: &str) -> String {
    let raw = pkg
        .url
        .split(['?', '#'])
        .next()
        .unwrap_or(&pkg.url)
        .rsplit('/')
        .next()
        .map(percent_decode)
        .unwrap_or_default();
    let mut name: String = raw
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            c if (c as u32) < 0x20 => '_',
            c => c,
        })
        .collect();
    name = name.trim().to_string();
    if name.is_empty() || name == "." || name == ".." {
        name = format!("DfoVibration-Sorahk_v{version}_{}.zip", pkg.channel.key());
    }
    name
}

/* ═════════════════════ 状态容器 ═════════════════════ */

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Phase {
    /// 空闲 (可能已有上次结果)
    #[default]
    Idle,
    Checking,
    Downloading,
    /// 正在校验/落盘完成后由界面读取
    Downloaded,
    Failed,
}

/// 某一时刻的可视快照 (UI 每帧拷一份, 不持有锁)。
#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    pub phase: Phase,
    pub latest: Option<Release>,
    /// 失败原因 (人话, 直接显示)
    pub error: Option<String>,
    pub done: u64,
    pub total: u64,
    pub active: Option<Channel>,
    pub saved: Option<PathBuf>,
    /// 成功的清单源 (诊断用, 界面上藏在悬停提示里)
    pub source: Option<String>,
    pub last_check_unix: i64,
    /// ★R2.12 (优化 10): 下次允许自动检查的时刻 (epoll ms 无关, 用 unix 秒)。
    /// 失败后指数退避, 避免断网/GitHub 抽风时每次启动都反复卡住检查线程。
    pub next_check_unix: i64,
}

struct Inner {
    snap: Snapshot,
    cancel: Option<Arc<AtomicBool>>,
    /// ★R2.12 (优化 10): 连续失败次数 —— 决定退避时长, 成功一次即清零。
    fail_streak: u32,
}

/// 检查失败后的退避时长 (秒): 1min → 5min → 15min → 30min → 1h 封顶。
/// 断网时不再"每次启动都试一遍全套源", 但网络一旦恢复最多一小时就能自动重试。
fn backoff_secs(streak: u32) -> i64 {
    match streak {
        0 | 1 => 60,
        2 => 300,
        3 => 900,
        4 => 1800,
        _ => 3600,
    }
}

/// 检查更新管理器 —— 常驻 GUI 一侧, 状态放 `Arc<Mutex<..>>` 供后台线程回写。
pub struct UpdateManager {
    inner: Arc<Mutex<Inner>>,
}

fn lock(inner: &Arc<Mutex<Inner>>) -> MutexGuard<'_, Inner> {
    match inner.lock() {
        Ok(g) => g,
        /* 后台线程 panic 也只丢一次结果, 不该把整个界面拖死 */
        Err(poisoned) => poisoned.into_inner(),
    }
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

impl UpdateManager {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(Inner {
                snap: Snapshot::default(),
                cancel: None,
                fail_streak: 0,
            })),
        }
    }

    pub fn snapshot(&self) -> Snapshot {
        lock(&self.inner).snap.clone()
    }

    /// ★R2.12 (优化 10): 现在是否**允许**自动检查 —— 同时受"4 小时节流"与
    /// "失败退避"约束。失败后不再每次启动都全套源重试一遍。
    pub fn auto_check_due(&self) -> bool {
        let snap = self.snapshot();
        auto_check_due(snap.last_check_unix) && now_unix() >= snap.next_check_unix
    }

    /// 后台检查。已有检查/下载在跑时直接返回 false。
    pub fn check(&self, custom_manifest: String) -> bool {
        {
            let mut inner = lock(&self.inner);
            if matches!(inner.snap.phase, Phase::Checking | Phase::Downloading) {
                return false;
            }
            inner.snap.phase = Phase::Checking;
            inner.snap.error = None;
        }
        let shared = Arc::clone(&self.inner);
        let spawned = std::thread::Builder::new()
            .name("update-check".into())
            .spawn(move || {
                let mut last_error = String::from("所有清单源都无法访问 (无网络/被拦截?)");
                for url in sources(&custom_manifest) {
                    match http::get_text(&url).and_then(|body| parse_release(&body)) {
                        Ok(rel) => {
                            let mut inner = lock(&shared);
                            inner.snap.phase = Phase::Idle;
                            inner.snap.latest = Some(rel);
                            inner.snap.source = Some(url);
                            inner.snap.error = None;
                            inner.snap.last_check_unix = now_unix();
                            /* ★R2.12 (优化 10): 成功即清零失败计数, 下次按正常 4 小时节流 */
                            inner.fail_streak = 0;
                            inner.snap.next_check_unix = now_unix() + AUTO_CHECK_INTERVAL_SECS;
                            crate::util::crash_log(
                                "update",
                                &format!("检查成功 (源 {} )", inner.snap.source.clone().unwrap_or_default()),
                            );
                            return;
                        }
                        Err(e) => {
                            last_error = format!("{url} → {e}");
                        }
                    }
                }
                let mut inner = lock(&shared);
                inner.snap.phase = Phase::Failed;
                inner.snap.error = Some(last_error.clone());
                /* ★R2.12 (优化 10): 失败按指数退避 (1min→1h), 避免断网时反复卡线程;
                 * 时间戳仍不推进 —— 网络恢复后下次启动会立刻重试。 */
                inner.fail_streak = inner.fail_streak.saturating_add(1);
                inner.snap.next_check_unix = now_unix() + backoff_secs(inner.fail_streak);
                let streak = inner.fail_streak;
                let wait = backoff_secs(streak);
                drop(inner);
                crate::util::crash_log(
                    "update",
                    &format!("检查失败 (第 {streak} 次, {wait}s 后重试): {last_error}"),
                );
            });
        if spawned.is_err() {
            let mut inner = lock(&self.inner);
            inner.snap.phase = Phase::Failed;
            inner.snap.error = Some("无法创建网络线程".to_string());
            return false;
        }
        true
    }

    /// 后台下载某个发行包到 `dir`。
    pub fn download(&self, version: String, pkg: Package, dir: PathBuf) -> bool {
        let cancel = {
            let mut inner = lock(&self.inner);
            if matches!(inner.snap.phase, Phase::Downloading) {
                return false;
            }
            inner.snap.phase = Phase::Downloading;
            inner.snap.error = None;
            inner.snap.saved = None;
            inner.snap.done = 0;
            inner.snap.total = pkg.size;
            inner.snap.active = Some(pkg.channel);
            let cancel = Arc::new(AtomicBool::new(false));
            inner.cancel = Some(Arc::clone(&cancel));
            cancel
        };

        let shared = Arc::clone(&self.inner);
        let dest = dir.join(file_name_for(&pkg, &version));
        let spawned = std::thread::Builder::new()
            .name("update-download".into())
            .spawn(move || {
                /* ★R2.12 (优化 10): 清单声明了 sha256 就强制校验 (没有则照旧下载) */
                let result = http::download_verified(
                    &pkg.url,
                    &dest,
                    pkg.sha256.as_deref(),
                    &cancel,
                    |done, total| {
                        let mut inner = lock(&shared);
                        inner.snap.done = done;
                        if total > 0 {
                            inner.snap.total = total;
                        }
                    },
                );
                let mut inner = lock(&shared);
                inner.cancel = None;
                match result {
                    Ok(bytes) => {
                        inner.snap.phase = Phase::Downloaded;
                        inner.snap.done = bytes;
                        inner.snap.total = bytes;
                        inner.snap.saved = Some(dest.clone());
                        inner.snap.error = None;
                        drop(inner);
                        crate::util::crash_log(
                            "update",
                            &format!("下载完成: {} ({} 字节)", dest.display(), bytes),
                        );
                    }
                    Err(e) => {
                        let cancelled = cancel.load(Ordering::Relaxed);
                        inner.snap.phase = Phase::Failed;
                        inner.snap.error = Some(format!("{e}"));
                        inner.snap.active = None;
                        drop(inner);
                        crate::util::crash_log(
                            "update",
                            &format!("下载失败 (cancelled={cancelled}): {e}"),
                        );
                    }
                }
            });
        if spawned.is_err() {
            let mut inner = lock(&self.inner);
            inner.snap.phase = Phase::Failed;
            inner.snap.error = Some("无法创建下载线程".to_string());
            inner.cancel = None;
            return false;
        }
        true
    }

    /// 取消正在进行的下载 (线程会在下一个数据块处退出)。
    pub fn cancel_download(&self) {
        if let Some(c) = lock(&self.inner).cancel.as_ref() {
            c.store(true, Ordering::Relaxed);
        }
    }

    /// 清掉失败态, 回到空闲 (关闭错误提示时用)。
    pub fn dismiss_error(&self) {
        let mut inner = lock(&self.inner);
        if inner.snap.phase == Phase::Failed {
            inner.snap.phase = Phase::Idle;
            inner.snap.error = None;
        }
    }
}

impl Default for UpdateManager {
    fn default() -> Self {
        Self::new()
    }
}

/// 距上次成功检查是否已超过节流窗口。
pub fn auto_check_due(last_check_unix: i64) -> bool {
    if last_check_unix <= 0 {
        return true;
    }
    now_unix() - last_check_unix >= AUTO_CHECK_INTERVAL_SECS
}
