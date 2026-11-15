//! 前台进程识别 + HidHide 时代白名单匹配 (v24.29 进程白名单语义) —— 原 state.rs 2888-2995, 2026-09-27 架构重构 B5 归位。

use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};


use crate::util;

use windows::Win32::Foundation::{CloseHandle, MAX_PATH};
use windows::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::PWSTR;

use super::*;

/// ★v24.52 前台进程探测结果。
/// `path` 需要 OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION) 成功;
/// 管理员权限/受保护进程可能拿不到 → 退回 Toolhelp 拿 exe 文件名 (`name`)。
/// 只要 PID 拿得到, 白名单就不会"莫名失效": 先按名匹配, 有路径再按路径匹配。
#[derive(Debug, Clone)]
pub struct ForegroundProbe {
    pub pid: u32,
    pub path: Option<String>,
    pub name: Option<String>,
    /// 拿路径失败的原因 (空 = 正常); 只用于日志/界面提示
    pub note: String,
}

/// ★R2.12 (优化 8) 前台探测缓存条目: 按窗口句柄命中, 不再按固定 50ms 重查。
/// `probe = None` 也是一种结果 (取不到前台窗口), 同样缓存, 避免空转重试。
pub(crate) struct ForegroundCache {
    /// 缓存对应的前台窗口句柄 (0 = 当时没有前台窗口)
    pub(crate) hwnd: isize,
    pub(crate) probe: Option<ForegroundProbe>,
    /// 上次真正探测的时刻 (2s 兜底用)
    pub(crate) at: Instant,
}

impl Default for ForegroundCache {
    fn default() -> Self {
        Self {
            hwnd: -1,
            probe: None,
            at: Instant::now(),
        }
    }
}

impl AppState {
    /// ★v24.52 音频白名单专用: 取前台进程 PID + (路径或文件名)。
    /// 独立于 `is_process_whitelisted` —— 不读连发白名单开关/列表/缓存,
    /// 也不写任何连发状态。
    pub fn foreground_probe() -> Option<ForegroundProbe> {
        unsafe {
            let hwnd = GetForegroundWindow();
            if hwnd.0.is_null() {
                return None;
            }
            let mut pid: u32 = 0;
            GetWindowThreadProcessId(hwnd, Some(&mut pid as *mut u32));
            if pid == 0 {
                return None;
            }
            let mut note = String::new();
            let path = match OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
                Ok(h) if !h.is_invalid() => {
                    let mut buffer = [0u16; MAX_PATH as usize];
                    let mut size = buffer.len() as u32;
                    let res = QueryFullProcessImageNameW(
                        h,
                        PROCESS_NAME_WIN32,
                        PWSTR(buffer.as_mut_ptr()),
                        &mut size,
                    );
                    let _ = CloseHandle(h);
                    if res.is_ok() {
                        Some(String::from_utf16_lossy(&buffer[..size as usize]))
                    } else {
                        note = "QueryFullProcessImageName 失败(高权限/受保护进程)".to_string();
                        None
                    }
                }
                Ok(h) => {
                    let _ = CloseHandle(h);
                    note = "OpenProcess 返回无效句柄".to_string();
                    None
                }
                Err(e) => {
                    note = format!("OpenProcess 失败: {e}");
                    None
                }
            };
            let name = path
                .as_deref()
                .and_then(|p| p.rsplit(['\\', '/']).next().map(|s| s.to_string()))
                .or_else(|| Self::process_name_by_pid(pid));
            Some(ForegroundProbe { pid, path, name, note })
        }
    }

    /// Toolhelp 枚举 PID → exe 文件名 (不需要打开目标进程句柄,
    /// 因此对管理员运行/高完整性进程也能拿到名字)。
    pub fn process_name_by_pid(pid: u32) -> Option<String> {
        use windows::Win32::System::Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
            TH32CS_SNAPPROCESS,
        };
        unsafe {
            let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0).ok()?;
            let mut pe = PROCESSENTRY32W::default();
            pe.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
            let mut found = None;
            if Process32FirstW(snap, &mut pe).is_ok() {
                loop {
                    if pe.th32ProcessID == pid {
                        let len = pe
                            .szExeFile
                            .iter()
                            .position(|&c| c == 0)
                            .unwrap_or(pe.szExeFile.len());
                        found = Some(String::from_utf16_lossy(&pe.szExeFile[..len]));
                        break;
                    }
                    if Process32NextW(snap, &mut pe).is_err() {
                        break;
                    }
                }
            }
            let _ = CloseHandle(snap);
            found
        }
    }

    /// ★v24.52 音频白名单复用连发白名单: 只读快照 (绝不修改连发状态)。
    /// 快照需要独立 Vec (音频线程持有期间不随配置变化) —— 低频路径 (500ms 一次), 克隆无妨。
    pub(crate) fn process_whitelist_snapshot(&self) -> Vec<String> {
        util::read_guard(&self.process_whitelist).as_ref().clone()
    }

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
    /// ★v24.40 白名单匹配 (带模式): simple=true = 简易模式 —— **所有**条目
    /// (含完整路径条目) 都只按文件名尾段比对, 旧版行为, 换目录/换盘不失效;
    /// simple=false = 精细模式 —— 路径条目按完整路径精确匹配 (同名不同版本各归各)。
    pub(super) fn whitelist_entry_matches_in(
        simple: bool,
        entry: &str,
        full_path: Option<&str>,
    ) -> bool {
        let entry = entry.trim();
        if entry.is_empty() {
            return false;
        }
        let Some(path) = full_path else {
            return false;
        };
        if simple {
            // 简易模式: 条目与进程路径都只取文件名尾段比对
            let entry_name = entry.rsplit(['\\', '/']).next().unwrap_or(entry);
            return path
                .rsplit(['\\', '/'])
                .next()
                .is_some_and(|name| name.eq_ignore_ascii_case(entry_name));
        }
        if entry.contains('\\') || entry.contains('/') {
            // 精细模式 · 路径条目: 完整路径精确匹配
            path.eq_ignore_ascii_case(entry)
        } else {
            // 精细模式 · 进程名条目: 与路径尾段文件名比对
            path.rsplit(['\\', '/'])
                .next()
                .is_some_and(|name| name.eq_ignore_ascii_case(entry))
        }
    }

    /// 旧签名委托 (精细模式) —— 既有调用/测试兼容。
    pub(super) fn whitelist_entry_matches(entry: &str, full_path: Option<&str>) -> bool {
        Self::whitelist_entry_matches_in(false, entry, full_path)
    }

    /// 前台窗口句柄 (GetForegroundWindow 是微秒级廉价调用, 用来判断"要不要重新探测")。
    fn foreground_hwnd() -> isize {
        unsafe { GetForegroundWindow().0 as isize }
    }

    /// ★R2.12 (优化 8) 带缓存的前台探测 —— 连发白名单与音频白名单**共用同一份判定**。
    ///
    /// 原来 `is_process_whitelisted` 每 50ms 就 OpenProcess + QueryFullProcessImageName,
    /// 高连发时一小时几万次系统调用; 音频震动侧另有一套 `foreground_probe()` 各探一遍,
    /// 于是出现过「连发放行、震动不放行」的怪事。现在:
    /// - 只有前台窗口变了 (HWND 不同) 才真正探测;
    /// - 同一窗口内有 2s 兜底 (窗口标题/权限变化时兜一下);
    /// - 两份白名单读同一份缓存, 结论必然一致。
    pub(crate) fn foreground_probe_cached(&self) -> Option<ForegroundProbe> {
        const BACKSTOP: Duration = Duration::from_secs(2);
        let hwnd = Self::foreground_hwnd();
        {
            let cache = util::read_guard(&self.cached_process_info);
            if cache.hwnd == hwnd && cache.at.elapsed() < BACKSTOP {
                return cache.probe.clone();
            }
        }
        let probe = Self::foreground_probe();
        *util::write_guard(&self.cached_process_info) = ForegroundCache {
            hwnd,
            probe: probe.clone(),
            at: Instant::now(),
        };
        probe
    }

    /// Check if current foreground process is in whitelist (empty whitelist = all allowed)
    #[inline]
    pub(crate) fn is_process_whitelisted(&self) -> bool {
        // 白名单总开关关闭 = 全部放行 (列表保留, 重开即恢复)
        if !self.whitelist_enabled.load(Ordering::Relaxed) {
            return true;
        }
        // 防中毒 + 零分配: 只 clone Arc 引用计数 (热路径 ~1200 次/秒, 旧实现每次
        // 锁内克隆整个 Vec<String> = 10 次堆分配/次, 且与鼠标移动 worker 的 1kHz
        // 轮询在同一个 Mutex 上互抢 —— 键盘钩子线程偶发等锁)。
        let whitelist = Arc::clone(&util::read_guard(&self.process_whitelist));
        if whitelist.is_empty() {
            return true;
        }

        /* ★R2.12 (优化 8): 走缓存, 不再每次注入都开进程查系统 */
        let Some(info) = self.foreground_probe_cached() else {
            // 拿不到前台窗口 (桌面切换/锁屏瞬间): 放行, 不要把用户按键吞掉
            return true;
        };
        // 先按完整路径比, 拿不到路径 (高权限/受保护进程) 退回文件名比 —— 与音频侧同口径
        let target = info.path.clone().or_else(|| info.name.clone());
        let Some(path) = target else {
            return true;
        };
        let simple = self.whitelist_simple_mode.load(Ordering::Relaxed);
        whitelist
            .iter()
            .any(|p| Self::whitelist_entry_matches_in(simple, p, Some(&path)))
    }
}
