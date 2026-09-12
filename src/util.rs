//! Common utility functions.
//!
//! Provides branch prediction hints and hash functions used across modules.

/// Marker function for cold code paths.
///
/// Used with branch prediction hints to inform the compiler about infrequently executed paths.
#[inline(always)]
#[cold]
pub fn cold() {}

/// Branch prediction hint for conditions expected to be false.
///
/// Helps the compiler optimize for the more common case where the condition is false.
///
/// # Example
/// ```
/// use sorahk::util::unlikely;
///
/// fn process_data(value: i32) -> Result<i32, &'static str> {
///     if unlikely(value < 0) {
///         return Err("negative value");
///     }
///     Ok(value * 2)
/// }
///
/// assert_eq!(process_data(5).unwrap(), 10);
/// assert!(process_data(-1).is_err());
/// ```
#[inline(always)]
pub fn unlikely(b: bool) -> bool {
    if b {
        cold()
    }
    b
}

/// Branch prediction hint for conditions expected to be true.
///
/// Helps the compiler optimize for the more common case where the condition is true.
///
/// # Example
/// ```
/// use sorahk::util::likely;
///
/// fn validate_input(value: i32) -> Option<i32> {
///     if likely(value >= 0 && value <= 100) {
///         return Some(value);
///     }
///     None
/// }
///
/// assert_eq!(validate_input(50), Some(50));
/// assert_eq!(validate_input(150), None);
/// ```
#[inline(always)]
pub fn likely(b: bool) -> bool {
    if !b {
        cold()
    }
    b
}

/// FNV-1a 32-bit hash constants.
pub mod fnv32 {
    /// Offset basis for FNV-1a 32-bit hash.
    pub const OFFSET_BASIS: u32 = 0x811c9dc5;
    /// Prime multiplier for FNV-1a 32-bit hash.
    pub const PRIME: u32 = 0x01000193;
}

/// FNV-1a 64-bit hash constants.
pub mod fnv64 {
    /// Offset basis for FNV-1a 64-bit hash.
    pub const OFFSET_BASIS: u64 = 0xcbf29ce484222325;
    /// Prime multiplier for FNV-1a 64-bit hash.
    pub const PRIME: u64 = 0x100000001b3;
}

/// Computes FNV-1a 32-bit hash.
///
/// Non-cryptographic hash function suitable for hash tables and checksums.
///
/// # Arguments
/// * `hash` - Current hash state
/// * `value` - Value to incorporate into hash
///
/// # Returns
/// Updated hash state
#[inline(always)]
pub fn fnv1a_hash_u32(mut hash: u32, value: u32) -> u32 {
    hash ^= value;
    hash.wrapping_mul(fnv32::PRIME)
}

/// Computes FNV-1a 64-bit hash.
///
/// Non-cryptographic hash function suitable for hash tables and checksums.
///
/// # Arguments
/// * `hash` - Current hash state
/// * `value` - Value to incorporate into hash
///
/// # Returns
/// Updated hash state
#[inline(always)]
pub fn fnv1a_hash_u64(mut hash: u64, value: u64) -> u64 {
    hash ^= value;
    hash.wrapping_mul(fnv64::PRIME)
}

/// Computes FNV-1a 64-bit hash for a byte sequence.
///
/// Processes each byte in the input using the FNV-1a algorithm.
///
/// # Arguments
/// * `hash` - Initial hash state
/// * `bytes` - Input bytes to hash
///
/// # Returns
/// Final hash state
#[inline(always)]
pub fn fnv1a_hash_bytes(mut hash: u64, bytes: &[u8]) -> u64 {
    for &byte in bytes {
        hash = fnv1a_hash_u64(hash, byte as u64);
    }
    hash
}

// ─── 绝对稳定化基础设施: 崩溃日志 + 受监督线程 ─────────────────────────────
// panic 策略为 unwind(见 Cargo.toml [profile.release] panic = "unwind"):
// 任何线程 panic 都不再杀死整个进程, 由这里记录日志并让线程安全退出/重启。

use std::any::Any;
use std::io::Write;

/// 崩溃日志文件路径: 与可执行文件同目录的 SorahkDFO_crash.log
pub fn crash_log_path() -> std::path::PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("SorahkDFO_crash.log")
}

/// 追加一条异常日志(每次打开-追加-关闭, 不持有文件句柄, 可跨线程安全调用)。
pub fn crash_log(context: &str, detail: &str) {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let thread = std::thread::current().name().unwrap_or("<unnamed>").to_string();
    let line = format!("[{ts}] [thread:{thread}] [{context}] {detail}\n");
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(crash_log_path())
    {
        let _ = f.write_all(line.as_bytes());
    }
    #[cfg(debug_assertions)]
    eprint!("{line}");
}

/// 把 panic 载荷转成可读字符串(绝不 panic)。
pub fn panic_payload_str(payload: &(dyn Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        s.to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "non-string panic payload".to_string()
    }
}

/// 受监督线程: panic 被捕获(日志由 main 里的全局 panic hook 统一记录),
/// 线程安全退出; 若闭包以 panic 结束则自动重启(最多 MAX 次), 进程永不死。
///
/// 闭包正常返回(Ok)表示该线程职责正常结束, 不重启。
pub fn spawn_supervised<F>(name: &'static str, mut f: F)
where
    F: FnMut() + Send + 'static,
{
    const MAX_RESTARTS: u32 = 10;
    let _ = std::thread::Builder::new()
        .name(name.to_string())
        .spawn(move || {
            let mut restarts: u32 = 0;
            loop {
                let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(&mut f));
                if r.is_ok() {
                    break; // 正常结束, 不再重启
                }
                restarts += 1;
                if restarts > MAX_RESTARTS {
                    crash_log(
                        "SUPERVISOR_GIVE_UP",
                        &format!("{name} 连续 panic {MAX_RESTARTS} 次, 放弃重启(进程保持存活)"),
                    );
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(300));
            }
        });
}

/// 防中毒锁获取: 锁被其它线程 panic 弄脏时仍取出内部数据继续运行(降级而非 panic)。
pub fn lock_guard<T>(m: &std::sync::Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    match m.lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    }
}

/// 防中毒读锁获取: 同上(RwLock)。
pub fn read_guard<T>(rw: &std::sync::RwLock<T>) -> std::sync::RwLockReadGuard<'_, T> {
    match rw.read() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    }
}

/// 防中毒写锁获取: 同上(RwLock)。
pub fn write_guard<T>(rw: &std::sync::RwLock<T>) -> std::sync::RwLockWriteGuard<'_, T> {
    match rw.write() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    }
}

/* ═══════════ ★v21.7d 组合键规范化 (去重 + 规范排序) ═══════════
 *
 * 用户要求: 同一按键不得重复 (上+上+空格 不允许); 顺序必须规范 (下+上+空格 → 上+下+空格)。
 * 规则: 修饰键 → 方向键 (上/下/左/右) → 手柄按键 → 手柄轴方向 → XInput 按键 → 其余按名。
 * 手柄多键 `GAMEPAD_<VID>_A+B` 会展开成带前缀的独立标签, 规范化后再合并回去。
 */

/// 组合键字符串 → 独立按键标签 (展开同一手柄的多键写法)。
pub fn key_combo_parts(combo: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut carry: Option<String> = None;
    for raw in combo.split('+') {
        let p = raw.trim();
        if p.is_empty() {
            continue;
        }
        let up = p.to_uppercase();
        let is_pad = up.starts_with("GAMEPAD_")
            || up.starts_with("JOYSTICK_")
            || up.starts_with("HID_");
        if is_pad {
            if let Some(idx) = p.rfind('_') {
                carry = Some(p[..idx].to_string());
            }
            out.push(p.to_string());
        } else if let Some(c) = &carry {
            out.push(format!("{c}_{p}"));
        } else {
            out.push(p.to_string());
        }
    }
    out
}

/// 标签列表 → 组合键字符串 (同一手柄多键合并回 `GAMEPAD_<VID>_A+B`)。
/// **保持输入顺序** (输入若已规范排序, 输出即规范序); 同手柄的按键就地并入同一组。
pub fn compact_key_combo(parts: &[String]) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut pad_pos: Vec<(String, usize)> = Vec::new();
    for p in parts {
        let up = p.to_uppercase();
        let is_pad =
            up.starts_with("GAMEPAD_") || up.starts_with("JOYSTICK_") || up.starts_with("HID_");
        if is_pad && let Some(idx) = p.rfind('_') {
            let (pref, btn) = (&p[..idx], &p[idx + 1..]);
            if let Some((_, oi)) = pad_pos.iter().find(|(x, _)| x.eq_ignore_ascii_case(pref)) {
                out[*oi].push('+');
                out[*oi].push_str(btn);
            } else {
                pad_pos.push((pref.to_string(), out.len()));
                out.push(format!("{pref}_{btn}"));
            }
            continue;
        }
        out.push(p.clone());
    }
    out.join("+")
}

/// 单个标签的规范排序键 (tier, 序号, 原名)。
fn combo_part_rank(part: &str) -> (u8, u32, String) {
    let u = part.to_uppercase();
    let m = match u.as_str() {
        "CTRL" | "LCTRL" | "RCTRL" => 0,
        "SHIFT" | "LSHIFT" | "RSHIFT" => 1,
        "ALT" | "LALT" | "RALT" => 2,
        "WIN" | "LWIN" | "RWIN" => 3,
        _ => 255,
    };
    if m != 255 {
        return (0, m, u);
    }
    let d = match u.as_str() {
        "UP" => 0,
        "DOWN" => 1,
        "LEFT" => 2,
        "RIGHT" => 3,
        _ => 255,
    };
    if d != 255 {
        return (1, d, u);
    }
    /* 语义手柄按键 ..._H<usage> */
    if let Some(pos) = u.rfind("_H")
        && let Ok(n) = u[pos + 2..].parse::<u32>()
    {
        return (2, n, u);
    }
    /* 语义手柄轴方向 ..._A<usage><L|R|U|D> */
    if let Some(pos) = u.rfind("_A") {
        let rest = &u[pos + 2..];
        if rest.len() >= 2 {
            let dir = rest.chars().next_back().unwrap_or('?');
            if let Ok(usage) = rest[..rest.len() - 1].parse::<u32>() {
                let dord = match dir {
                    'U' => 0,
                    'D' => 1,
                    'L' => 2,
                    'R' => 3,
                    _ => 9,
                };
                return (3, usage * 10 + dord, u);
            }
        }
    }
    /* XInput 名 ..._<BUTTON> */
    if let Some(pos) = u.rfind('_') {
        let rank = match &u[pos + 1..] {
            "A" => 0,
            "B" => 1,
            "X" => 2,
            "Y" => 3,
            "LB" => 4,
            "RB" => 5,
            "LT" => 6,
            "RT" => 7,
            "BACK" => 8,
            "START" => 9,
            "LS_CLICK" => 10,
            "RS_CLICK" => 11,
            "DPAD_UP" => 12,
            "DPAD_DOWN" => 13,
            "DPAD_LEFT" => 14,
            "DPAD_RIGHT" => 15,
            _ => 255,
        };
        if rank != 255 {
            return (4, rank, u);
        }
    }
    (9, 0, u)
}

/// 去重 (忽略大小写) + 规范排序。
pub fn normalize_combo_parts(parts: &[String]) -> Vec<String> {
    let mut seen: Vec<String> = Vec::new();
    for p in parts {
        let p = p.trim();
        if p.is_empty() {
            continue;
        }
        if !seen.iter().any(|e| e.eq_ignore_ascii_case(p)) {
            seen.push(p.to_string());
        }
    }
    seen.sort_by_key(|p| combo_part_rank(p));
    seen
}

/// 组合键字符串 → 去重 + 规范排序后的组合键字符串 (用户要求的"严格顺序+不重复")。
pub fn normalize_key_combo(combo: &str) -> String {
    compact_key_combo(&normalize_combo_parts(&key_combo_parts(combo)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_likely_unlikely() {
        assert!(likely(true));
        assert!(!likely(false));
        assert!(unlikely(true));
        assert!(!unlikely(false));
    }

    #[test]
    fn test_fnv1a_hash_u32() {
        let hash = fnv32::OFFSET_BASIS;
        let result = fnv1a_hash_u32(hash, 42);
        assert_ne!(result, hash);

        // Verify determinism
        let hash1 = fnv1a_hash_u32(fnv32::OFFSET_BASIS, 42);
        let hash2 = fnv1a_hash_u32(fnv32::OFFSET_BASIS, 42);
        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_fnv1a_hash_u64() {
        let hash = fnv64::OFFSET_BASIS;
        let result = fnv1a_hash_u64(hash, 42);
        assert_ne!(result, hash);

        // Verify determinism
        let hash1 = fnv1a_hash_u64(fnv64::OFFSET_BASIS, 42);
        let hash2 = fnv1a_hash_u64(fnv64::OFFSET_BASIS, 42);
        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_fnv1a_hash_bytes() {
        let hash = fnv64::OFFSET_BASIS;
        let data = b"test data";
        let result = fnv1a_hash_bytes(hash, data);
        assert_ne!(result, hash);

        // Verify determinism
        let hash1 = fnv1a_hash_bytes(fnv64::OFFSET_BASIS, data);
        let hash2 = fnv1a_hash_bytes(fnv64::OFFSET_BASIS, data);
        assert_eq!(hash1, hash2);

        // Verify different inputs produce different outputs
        let hash3 = fnv1a_hash_bytes(fnv64::OFFSET_BASIS, b"other data");
        assert_ne!(hash1, hash3);
    }

    /* ── ★v21.7d 组合键规范化 (用户实测要求) ── */

    #[test]
    fn combo_dedupes_identical_keys() {
        // 上+上+空格 → 上+空格 (重复的上被丢弃)
        assert_eq!(normalize_key_combo("UP+UP+SPACE"), "UP+SPACE");
        assert_eq!(normalize_key_combo("UP+up+SPACE"), "UP+SPACE");
    }

    #[test]
    fn combo_sorts_directions_and_modifiers() {
        // 下+上+空格 → 上+下+空格 (方向键按 上/下/左/右 规范序)
        assert_eq!(normalize_key_combo("DOWN+UP+SPACE"), "UP+DOWN+SPACE");
        // 修饰键最前
        assert_eq!(normalize_key_combo("F6+CTRL"), "CTRL+F6");
        assert_eq!(normalize_key_combo("RIGHT+LEFT+ALT"), "ALT+LEFT+RIGHT");
    }

    #[test]
    fn combo_expands_and_recompacts_gamepad_multi_button() {
        let parts = key_combo_parts("GAMEPAD_045E_A+B");
        assert_eq!(parts, vec!["GAMEPAD_045E_A", "GAMEPAD_045E_B"]);
        assert_eq!(compact_key_combo(&parts), "GAMEPAD_045E_A+B");
        // 手柄按键在键盘键之前 (tier 2/4 < 9)
        assert_eq!(
            normalize_key_combo("SPACE+GAMEPAD_045E_A"),
            "GAMEPAD_045E_A+SPACE"
        );
    }

    #[test]
    fn combo_normalization_is_idempotent_and_safe() {
        for s in ["UP+DOWN+SPACE", "CTRL+F6", "GAMEPAD_045E_A+B", "SPACE", ""] {
            let once = normalize_key_combo(s);
            assert_eq!(normalize_key_combo(&once), once, "幂等: {s}");
        }
        assert_eq!(normalize_key_combo(""), "");
        assert_eq!(normalize_key_combo("   "), "");
    }
}
