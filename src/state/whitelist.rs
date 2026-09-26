//! 前台进程识别 + HidHide 时代白名单匹配 (v24.29 进程白名单语义) —— 原 state.rs 2888-2995, 2026-09-27 架构重构 B5 归位。

use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};


use crate::util::likely;
use crate::util;

use windows::Win32::Foundation::MAX_PATH;
use windows::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::PWSTR;

use super::*;

impl AppState {
/// Get the process name of the foreground window
    pub(super) fn get_foreground_process_name() -> Option<String> {
        unsafe {
            let hwnd = GetForegroundWindow();
            if hwnd.0.is_null() {
                return None;
            }

            let mut process_id: u32 = 0;
            GetWindowThreadProcessId(hwnd, Some(&mut process_id as *mut u32));
            if process_id == 0 {
                return None;
            }

            let process_handle =
                match OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, process_id) {
                    Ok(handle) => handle,
                    Err(_) => return None,
                };

            let mut buffer = [0u16; MAX_PATH as usize];
            let mut size = buffer.len() as u32;

            match QueryFullProcessImageNameW(
                process_handle,
                PROCESS_NAME_WIN32,
                PWSTR(buffer.as_mut_ptr()),
                &mut size,
            ) {
                Ok(_) => {
                    // ★返回完整映像路径 (不再截成文件名): 白名单路径条目按完整路径匹配,
                    // 纯进程名条目在 whitelist_entry_matches 里从路径提取文件名比对。
                    Some(String::from_utf16_lossy(&buffer[..size as usize]))
                }
                Err(_) => None,
            }
        }
    }

    /// ★白名单条目匹配 (忽略大小写)。条目两种形态:
    /// - 纯进程名 (如 `dnf.exe`): 匹配任意路径的同名进程 (老语义, 兼容旧配置);
    /// - 完整路径 (含 `\` 或 `/`, 如 `E:\games\A\DFO.exe`): 只匹配该路径的进程 ——
    ///   用于区分**同名不同版本**的 exe (A 版/B 版 DFO.exe 各登记一条, 互不干扰)。
    pub(super) fn whitelist_entry_matches(entry: &str, full_path: Option<&str>) -> bool {
        let entry = entry.trim();
        if entry.is_empty() {
            return false;
        }
        let Some(path) = full_path else {
            return false;
        };
        if entry.contains('\\') || entry.contains('/') {
            // 路径条目: 完整路径精确匹配
            path.eq_ignore_ascii_case(entry)
        } else {
            // 进程名条目: 与路径尾段文件名比对
            path.rsplit(['\\', '/'])
                .next()
                .is_some_and(|name| name.eq_ignore_ascii_case(entry))
        }
    }

    /// Check if current foreground process is in whitelist (empty whitelist = all allowed)
    #[inline]
    pub(crate) fn is_process_whitelisted(&self) -> bool {
        // 白名单总开关关闭 = 全部放行 (列表保留, 重开即恢复)
        if !self.whitelist_enabled.load(Ordering::Relaxed) {
            return true;
        }
        // 防中毒: 锁内只做轻量 clone, 绝不持锁调用 WinAPI(OpenProcess 等)
        let whitelist = util::lock_guard(&self.process_whitelist).clone();
        if whitelist.is_empty() {
            return true;
        }

        const CACHE_DURATION_MS: u64 = 50;
        let now = Instant::now();

        let process_path = {
            let cache = util::read_guard(&self.cached_process_info);
            let (cached_path, cached_time) = &*cache;

            if likely(now.duration_since(*cached_time) < Duration::from_millis(CACHE_DURATION_MS)) {
                // Cache hit: return cached path without write lock
                cached_path.clone()
            } else {
                // Cache miss: need refresh
                drop(cache);
                let new_path = Self::get_foreground_process_name();
                *util::write_guard(&self.cached_process_info) = (new_path.clone(), now);

                new_path
            }
        };

        // Check if process is in whitelist (任一条目命中即放行)
        if let Some(path) = process_path {
            whitelist
                .iter()
                .any(|p| Self::whitelist_entry_matches(p, Some(&path)))
        } else {
            // If we can't get process path, allow by default
            true
        }
    }
}
