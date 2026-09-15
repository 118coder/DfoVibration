/* ★★★ v24.19b 手柄隐藏暂时下线 (2026-09-14) ★★★
 *
 * 用户决策 (原话): "感觉不太行, 本次更新的相关功能先暂时注释掉吧, 也就是HidHide相关的。
 * 等未来有空的时候再启用。" 随后澄清: "相关功能和代码全部注释掉, 避免影响主功能。"
 *
 * 本文件**全部内容已整体注释**, 不参与编译 —— 下方保留的是 v24.19a 真机验证 9/9 全 PASS
 * 的完整实现 (IOCTL 契约 / 反向白名单 / 持久黑名单降级 / 崩溃自愈 / 12 条单测)。
 *
 * 未来重新启用步骤 (约 5 处接线):
 *   1. 本文件: 删掉最外层这对 /* */ (其余原样);
 *   2. src/lib.rs 与 src/main.rs: 取消 `pub mod hidhide;` / `mod hidhide;` 的注释;
 *   3. src/config.rs: 取消 hidhide_enabled / hidhide_snapshot / hidhide_learned_paths
 *      三字段与 Default 里三条赋值的注释;
 *   4. src/gui/mod.rs: 取消控制器字段、启动块、`Self` 初始化三处注释;
 *   5. src/gui/whitelist_page.rs: 取消 render_hidhide_card 调用与函数体注释;
 *      src/gui/main_window.rs: 取消 on_exit 恢复块注释。
 *   (Cargo.toml 的三个 windows feature 若已注释也一并恢复。)
 * 建议恢复后重跑 work/_e2e_hidhide.py (真驱动 1.4.181 + 真手柄 9/9 基线)。

/*
//! ★v24.19 手柄隐藏 (HidHide 驱动整合)。
//!
//! 用户规格 (2026-09-14): 部分游戏检测到物理手柄后强制进入原生手柄模式,
//! 导致映射失效 (提高进程优先级无效 —— 优先级只影响 CPU 调度, 不影响设备检测)。
//! 方案: 用 HidHide 过滤驱动做**设备层隐藏**:
//!
//! - 设备可见性: HidHide 挂在 HIDClass / XnaComposite / XboxComposite 三个设备类的
//!   上层过滤驱动位置, 在 IRP_MJ_CREATE 时按"设备黑名单 + 进程反向白名单"裁决。
//! - 作用域: 采用 **inverse 模式** (IOCTL 2055 置反向 + IOCTL 2049 白名单=游戏进程
//!   完整路径) —— 只有列表内的游戏进程被拒绝访问手柄, 其他程序 (含本软件自身、
//!   震动的 XInput 通道) 完全不受影响。
//! - 生命周期: 设备条目走**会话黑名单 IOCTL 2056** (MULTI_SZ 设备实例路径, 属主=
//!   本进程 PID, 仅存内核内存): 本软件退出时驱动按进程回调自动清除; 进程崩溃同理。
//!   inverse / 白名单 / active 是驱动自身的注册表全局状态, 开启前做**快照**存进
//!   Config.toml, 关闭或退出时原样恢复; 崩溃遗留的快照由下次启动自愈恢复。
//!
//! 驱动契约 (与 HidHide 源码 Shared/HidHideIoctlContract.h 逐字一致):
//! 设备类型 32769, 功能号 2048-2057, METHOD_BUFFERED, FILE_READ_DATA。
//! 控制设备 `\\.\HidHide` 以 GENERIC_READ 打开 (SDDL 对所有用户开放 RWX, 无需管理员)。
use std::time::Instant;

use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Devices::DeviceAndDriverInstallation::{
    SetupDiDestroyDeviceInfoList, SetupDiEnumDeviceInfo, SetupDiGetClassDevsW,
    SetupDiGetDeviceInstanceIdW, DIGCF_ALLCLASSES, DIGCF_PRESENT, SP_DEVINFO_DATA,
};
use windows::Win32::Foundation::{CloseHandle, ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND, HANDLE};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
    OPEN_EXISTING,
};
use windows::Win32::System::IO::DeviceIoControl;
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_NATIVE,
    PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};

use crate::config::AppConfig;

// ---------------------------------------------------------------------------
// IOCTL 契约 (黄金值有单测锁定)
// ---------------------------------------------------------------------------

/// HidHide 自定义设备类型 (驱动源码 IoControlDeviceType = 32769)。
const IOCTL_DEVICE_TYPE: u32 = 32769;
/// CTL_CODE 的 Access 位 (驱动用 FILE_READ_DATA = 1)。
const IOCTL_ACCESS_FILE_READ_DATA: u32 = 1;
/// METHOD_BUFFERED = 0。
const METHOD_BUFFERED: u32 = 0;

/// 与 C 宏 CTL_CODE(DeviceType, Function, Method, Access) 等价的常量函数。
const fn ctl_code(function: u32) -> u32 {
    (IOCTL_DEVICE_TYPE << 16)
        | (IOCTL_ACCESS_FILE_READ_DATA << 14)
        | (function << 2)
        | METHOD_BUFFERED
}

/// 2048: 读取进程白名单 (输出 MULTI_SZ 完整映像路径)。
pub const IOCTL_GET_WHITELIST: u32 = ctl_code(2048);
/// 2049: **整体替换**进程白名单 (输入 MULTI_SZ 完整映像路径)。
pub const IOCTL_SET_WHITELIST: u32 = ctl_code(2049);
/// 2050: 读取设备黑名单 (输出 MULTI_SZ 设备实例路径)。
/// (本方案不使用 —— 设备条目走会话黑名单 2056, 不碰持久黑名单; 留档黄金值对账)
#[allow(dead_code)]
pub const IOCTL_GET_BLACKLIST: u32 = ctl_code(2050);
/// 2051: 整体替换设备黑名单 (输入 MULTI_SZ, 条目可带 `!<会话ID>` 限定)。
/// (本方案不使用 —— 设备条目走会话黑名单 2056, 不碰持久黑名单; 留档黄金值对账)
#[allow(dead_code)]
pub const IOCTL_SET_BLACKLIST: u32 = ctl_code(2051);
/// 2052: 读取驱动激活状态 (输出 1 字节 BOOLEAN)。
pub const IOCTL_GET_ACTIVE: u32 = ctl_code(2052);
/// 2053: 设置驱动激活状态 (输入 1 字节 BOOLEAN)。
pub const IOCTL_SET_ACTIVE: u32 = ctl_code(2053);
/// 2054: 读取"白名单取反"状态 (输出 BOOLEAN)。
pub const IOCTL_GET_WLINVERSE: u32 = ctl_code(2054);
/// 2055: 设置"白名单取反" (输入 BOOLEAN): true 时白名单语义反转 ——
/// 列表内的进程**看不到**隐藏设备, 其余进程全部可见。
pub const IOCTL_SET_WLINVERSE: u32 = ctl_code(2055);
/// 2056: 追加**会话黑名单** (输入 MULTI_SZ 设备实例路径)。
/// 条目仅存内核内存, 属主=调用方 PID, 进程退出/崩溃由驱动自动清除。
pub const IOCTL_ADD_SESSION_BLACKLIST: u32 = ctl_code(2056);
/// 2057: 清除**本进程**的全部会话黑名单条目 (无输入输出缓冲)。
pub const IOCTL_CLR_SESSION_BLACKLIST: u32 = ctl_code(2057);

// ---------------------------------------------------------------------------
// 错误
// ---------------------------------------------------------------------------

/// HidHide 操作错误。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HidHideError {
    /// 驱动未安装 (`\\.\HidHide` 不存在)。
    NotInstalled,
    /// 其他 Win32 错误 (附带大致归类)。
    Os(String),
}

impl std::fmt::Display for HidHideError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HidHideError::NotInstalled => write!(f, "未检测到 HidHide 驱动"),
            HidHideError::Os(s) => write!(f, "HidHide 驱动通信失败: {s}"),
        }
    }
}

impl HidHideError {
    fn from_windows(err: &windows::core::Error) -> Self {
        let code = err.code();
        if code == ERROR_FILE_NOT_FOUND.to_hresult() || code == ERROR_PATH_NOT_FOUND.to_hresult() {
            HidHideError::NotInstalled
        } else {
            HidHideError::Os(format!("{} ({:#010x})", err, code.0 as u32))
        }
    }
}

// ---------------------------------------------------------------------------
// MULTI_SZ 编解码 (驱动要求: 双 NUL 结尾, 至少 2 个 WCHAR)
// ---------------------------------------------------------------------------

/// 字符串列表 → MULTI_SZ 宽字符缓冲 (每串 NUL 结尾, 末尾再加一个 NUL)。
/// 空列表输出 `[0, 0]`, 同样满足驱动 "≥2 WCHAR 且双 NUL" 的校验。
pub fn encode_multi_sz(items: &[String]) -> Vec<u16> {
    let mut buf: Vec<u16> = Vec::new();
    for s in items {
        buf.extend(s.encode_utf16());
        buf.push(0);
    }
    buf.push(0);
    if buf.len() < 2 {
        buf.push(0);
    }
    buf
}

/// MULTI_SZ 宽字符缓冲 → 字符串列表 (丢弃尾部空段)。
pub fn decode_multi_sz(buf: &[u16]) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur: Vec<u16> = Vec::new();
    for &w in buf {
        if w == 0 {
            if !cur.is_empty() {
                out.push(String::from_utf16_lossy(&cur));
                cur.clear();
            }
        } else {
            cur.push(w);
        }
    }
    out
}

/// 宽字符缓冲 → 小端字节串 (DeviceIoControl 输入缓冲用)。
fn wide_to_bytes(wide: &[u16]) -> Vec<u8> {
    wide.iter().flat_map(|w| w.to_le_bytes()).collect()
}

// ---------------------------------------------------------------------------
// 设备实例路径扫描
// ---------------------------------------------------------------------------

/// 由 VID/PID 生成实例路径匹配串 (SetupAPI 实例路径形如
/// `HID\VID_045E&PID_028E\8&…` / `USB\VID_045E&PID_028E\…`, 十六进制大写)。
pub fn vid_pid_needle(vid: u16, pid: u16) -> String {
    format!("VID_{vid:04X}&PID_{pid:04X}")
}

/// 收集"当前连接的手柄类 HID 设备"的 (VID, PID) 集合。
/// 手柄判定: 通用桌面页(1) 的 摇杆(4) / 游戏手柄(5) 用法 —— 与 RawInput 采集同一来源。
fn collect_pad_vid_pids() -> Vec<(u16, u16)> {
    let mut pairs: Vec<(u16, u16)> = crate::rawinput::enumerate_hid_devices()
        .iter()
        .filter(|d| d.usage_page == 1 && (d.usage == 4 || d.usage == 5))
        .map(|d| (d.vid, d.pid))
        .collect();
    pairs.sort_unstable();
    pairs.dedup();
    pairs
}

/// 用 SetupAPI 枚举**在场**设备, 返回实例路径包含任一 `VID_xxxx&PID_xxxx` 的全部实例。
///
/// 故意按 VID/PID 扫整个设备树而非只扫 HID 接口: Xbox 系手柄的游戏侧访问走
/// XnaComposite/XboxComposite 栈, 其实例 (`USB\VID_…&PID_…`) 不出现在 RawInput 的
/// HID 接口列表里 —— 全树扫描才能把 HID 兄弟设备与 XInput 栈一并盖住。
pub fn sweep_instances_by_vid_pid(pairs: &[(u16, u16)]) -> Vec<String> {
    if pairs.is_empty() {
        return Vec::new();
    }
    let needles: Vec<String> = pairs.iter().map(|(v, p)| vid_pid_needle(*v, *p)).collect();
    let mut hits: Vec<String> = Vec::new();
    unsafe {
        let Ok(devinfo) = SetupDiGetClassDevsW(
            None,
            PCWSTR::null(),
            None,
            DIGCF_PRESENT | DIGCF_ALLCLASSES,
        ) else {
            return hits;
        };
        let mut index: u32 = 0;
        loop {
            let mut dev = SP_DEVINFO_DATA::default();
            dev.cbSize = std::mem::size_of::<SP_DEVINFO_DATA>() as u32;
            if SetupDiEnumDeviceInfo(devinfo, index, &mut dev).is_err() {
                break; // 枚举完毕
            }
            index += 1;
            // 两段式: 先问长度 (字符数, 含 NUL), 再取字符串
            let mut needed: u32 = 0;
            if SetupDiGetDeviceInstanceIdW(devinfo, &dev, None, Some(&mut needed)).is_ok()
                || needed == 0
            {
                continue;
            }
            let mut buf = vec![0u16; needed as usize];
            if SetupDiGetDeviceInstanceIdW(devinfo, &dev, Some(&mut buf), None).is_err() {
                continue;
            }
            let id = String::from_utf16_lossy(&buf[..buf.len().saturating_sub(1)]);
            let id_upper = id.to_uppercase();
            if needles.iter().any(|n| id_upper.contains(n.as_str())) {
                hits.push(id);
            }
        }
        let _ = SetupDiDestroyDeviceInfoList(devinfo);
    }
    hits.sort();
    hits.dedup();
    hits
}

/// 当前连接手柄的待隐藏实例集合 (VID/PID 全树扫描)。
fn collect_pad_instances() -> Vec<String> {
    sweep_instances_by_vid_pid(&collect_pad_vid_pids())
}

// ---------------------------------------------------------------------------
// 驱动句柄 (薄封装; 不跨线程, GUI 线程专用)
// ---------------------------------------------------------------------------

/// 打开过的 `\\.\HidHide` 控制设备句柄, Drop 自动关闭。
pub struct HidHide {
    handle: HANDLE,
}

impl Drop for HidHide {
    fn drop(&mut self) {
        unsafe { let _ = CloseHandle(self.handle); };
    }
}

impl HidHide {
    /// 打开控制设备。驱动未安装时返回 `NotInstalled`。
    pub fn open() -> Result<Self, HidHideError> {
        let name: Vec<u16> = "\\\\.\\HidHide".encode_utf16().chain([0]).collect();
        unsafe {
            match CreateFileW(
                PCWSTR(name.as_ptr()),
                windows::Win32::Foundation::GENERIC_READ.0,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                None,
                OPEN_EXISTING,
                FILE_ATTRIBUTE_NORMAL,
                None,
            ) {
                Ok(handle) => Ok(HidHide { handle }),
                Err(err) => Err(HidHideError::from_windows(&err)),
            }
        }
    }

    /// 驱动是否已安装 (控制设备能否打开)。
    pub fn driver_present() -> bool {
        Self::open().is_ok()
    }

    /// 仅输入缓冲的 IOCTL。
    fn ioctl_in(&self, code: u32, in_buf: &[u8]) -> Result<(), HidHideError> {
        let mut returned: u32 = 0;
        let r = unsafe {
            DeviceIoControl(
                self.handle,
                code,
                Some(in_buf.as_ptr() as *const _),
                in_buf.len() as u32,
                None,
                0,
                Some(&mut returned),
                None,
            )
        };
        r.map_err(|e| HidHideError::from_windows(&e))
    }

    /// 无缓冲 IOCTL (仅 2057 清会话黑名单)。
    fn ioctl_void(&self, code: u32) -> Result<(), HidHideError> {
        let mut returned: u32 = 0;
        let r = unsafe { DeviceIoControl(self.handle, code, None, 0, None, 0, Some(&mut returned), None) };
        r.map_err(|e| HidHideError::from_windows(&e))
    }

    /// 单字节 BOOLEAN 输出的 IOCTL (GET_ACTIVE / GET_WLINVERSE)。
    fn ioctl_bool_out(&self, code: u32) -> Result<bool, HidHideError> {
        let mut out = [0u8; 1];
        let mut returned: u32 = 0;
        let r = unsafe {
            DeviceIoControl(
                self.handle,
                code,
                None,
                0,
                Some(out.as_mut_ptr() as *mut _),
                out.len() as u32,
                Some(&mut returned),
                None,
            )
        };
        r.map_err(|e| HidHideError::from_windows(&e))?;
        Ok(out[0] != 0)
    }

    /// 单字节 BOOLEAN 输入的 IOCTL (SET_ACTIVE / SET_WLINVERSE)。
    fn ioctl_bool_in(&self, code: u32, v: bool) -> Result<(), HidHideError> {
        self.ioctl_in(code, &[u8::from(v)])
    }

    /// MULTI_SZ 输出的 IOCTL (GET_WHITELIST / GET_BLACKLIST, 两段式取长度)。
    fn ioctl_multi_sz_out(&self, code: u32) -> Result<Vec<String>, HidHideError> {
        let mut needed: u32 = 0;
        let r = unsafe { DeviceIoControl(self.handle, code, None, 0, None, 0, Some(&mut needed), None) };
        if let Err(e) = r {
            return Err(HidHideError::from_windows(&e));
        }
        if needed == 0 {
            return Ok(Vec::new());
        }
        let wchars = needed as usize / 2;
        if wchars == 0 {
            return Ok(Vec::new());
        }
        let mut buf = vec![0u16; wchars];
        let mut returned: u32 = 0;
        let r = unsafe {
            DeviceIoControl(
                self.handle,
                code,
                None,
                0,
                Some(buf.as_mut_ptr() as *mut _),
                buf.len() as u32 * 2,
                Some(&mut returned),
                None,
            )
        };
        r.map_err(|e| HidHideError::from_windows(&e))?;
        Ok(decode_multi_sz(&buf))
    }

    // -- 具体 IOCTL 对应方法 --

    pub fn get_active(&self) -> Result<bool, HidHideError> {
        self.ioctl_bool_out(IOCTL_GET_ACTIVE)
    }

    pub fn set_active(&self, active: bool) -> Result<(), HidHideError> {
        self.ioctl_bool_in(IOCTL_SET_ACTIVE, active)
    }

    pub fn get_inverse(&self) -> Result<bool, HidHideError> {
        self.ioctl_bool_out(IOCTL_GET_WLINVERSE)
    }

    pub fn set_inverse(&self, inverse: bool) -> Result<(), HidHideError> {
        self.ioctl_bool_in(IOCTL_SET_WLINVERSE, inverse)
    }

    pub fn get_whitelist(&self) -> Result<Vec<String>, HidHideError> {
        self.ioctl_multi_sz_out(IOCTL_GET_WHITELIST)
    }

    /// **整体替换**驱动白名单 (注意: 会覆盖其他软件写入的列表, 由快照恢复兜底)。
    pub fn set_whitelist(&self, paths: &[String]) -> Result<(), HidHideError> {
        self.ioctl_in(IOCTL_SET_WHITELIST, &wide_to_bytes(&encode_multi_sz(paths)))
    }

    /// 追加会话黑名单 (IOCTL 2056)。条目属主=本进程 PID, 退出自动清除。
    /// ⚠ 仅驱动源码 master 支持; **官方发布版 1.4.181 没有此功能** (返回 87),
    /// 调用方须先 `session_supported()` 分流, 失败走 `set_blacklist` 持久黑名单路线。
    pub fn add_session_blacklist(&self, instances: &[String]) -> Result<(), HidHideError> {
        self.ioctl_in(IOCTL_ADD_SESSION_BLACKLIST, &wide_to_bytes(&encode_multi_sz(instances)))
    }

    /// 清除本进程的全部会话黑名单条目 (IOCTL 2057)。
    pub fn clear_session_blacklist(&self) -> Result<(), HidHideError> {
        self.ioctl_void(IOCTL_CLR_SESSION_BLACKLIST)
    }

    /// 驱动是否支持会话黑名单 (2056/2057): 用零缓冲 2057 探测。
    /// 支持则返回 Ok (顺带清掉本进程残留条目 —— 此刻本来就没有, 无副作用);
    /// 发布版 1.4.181 不认识该码, 返回 Err (INVALID_PARAMETER)。
    pub fn session_supported(&self) -> bool {
        self.ioctl_void(IOCTL_CLR_SESSION_BLACKLIST).is_ok()
    }

    /// 读取持久设备黑名单 (IOCTL 2050)。
    pub fn get_blacklist(&self) -> Result<Vec<String>, HidHideError> {
        self.ioctl_multi_sz_out(IOCTL_GET_BLACKLIST)
    }

    /// **整体替换**持久设备黑名单 (IOCTL 2051)。发布版驱动的降级路线:
    /// 语义与会话黑名单等价 (配合 inverse 白名单, 只有列表内进程看不到),
    /// 区别是条目在驱动注册表里 —— 退出/崩溃靠我们的快照恢复兜底。
    pub fn set_blacklist(&self, instances: &[String]) -> Result<(), HidHideError> {
        self.ioctl_in(IOCTL_SET_BLACKLIST, &wide_to_bytes(&encode_multi_sz(instances)))
    }
}

/// 持久黑名单路线的合并: 原有条目保持在前, 追加新实例, 忽略大小写去重。
/// (驱动按忽略大小写比较条目; 重复写入虽无害, 但会让 GET 读回越来越长。)
pub fn merge_blacklist(orig: &[String], instances: &[String]) -> Vec<String> {
    let mut out: Vec<String> = orig.to_vec();
    for inst in instances {
        if !out.iter().any(|x| x.eq_ignore_ascii_case(inst)) {
            out.push(inst.clone());
        }
    }
    out
}

// ---------------------------------------------------------------------------
// 驱动状态快照 (崩溃自愈的依据; 序列化进 Config.toml)
// ---------------------------------------------------------------------------

/// 开启隐藏前的驱动全局状态快照 —— 关闭/退出时原样写回。
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct HidHideSnapshot {
    /// 驱动激活位 (Active)。
    pub active: bool,
    /// 白名单取反位 (WhitelistedInverse)。
    pub inverse: bool,
    /// 原白名单 (完整映像路径 MULTI_SZ 解出的列表)。
    pub whitelist: Vec<String>,
    /// ★v24.19a 原持久设备黑名单 (发布版驱动降级路线要在 2051 上做合并/恢复,
    /// 快照必须连它一起存)。serde 默认空: 兼容 v24.19 初版写出的快照。
    #[serde(default)]
    pub blacklist: Vec<String>,
}

// ---------------------------------------------------------------------------
// 控制器: GUI 线程持有的生命周期管理
// ---------------------------------------------------------------------------

/// 扫描节流间隔: 白名单页每帧渲染都会调 `maybe_scan`, 10 秒扫一次足够。
const SCAN_INTERVAL: std::time::Duration = std::time::Duration::from_secs(10);

/// 手柄隐藏控制器 —— 由 `SorahkGui` 持有, 仅 GUI 线程使用。
///
/// `runtime_on` 表示**当前会话**驱动是否已被我们改动 (与 config.hidhide_enabled
/// 的"用户意图"分离: 驱动未装/路径未定位时, 意图可以是开, 运行态是关)。
#[derive(Default)]
pub struct HidHideController {
    runtime_on: bool,
    last_error: Option<String>,
    /// 已定位的游戏完整路径 (当前生效于驱动反向白名单)。
    resolved: Vec<String>,
    /// 尚未定位完整路径的白名单进程名。
    unresolved: Vec<String>,
    /// 当前加入会话黑名单的设备实例数。
    hidden_count: usize,
    /// 最近一次扫描时间 (节流)。
    last_scan: Option<Instant>,
    /// 最近一次扫描学到的路径是否有新增 (供 GUI 落盘, 取走即清)。
    learned_changed: bool,
    /// 驱动是否已安装 (随扫描节流刷新, 不每帧探测)。
    driver_installed: bool,
    /// ★v24.19a 防递归标志: scan_now 里"驱动到位后自动补开"会调 enable,
    /// enable 内部又会 scan_now —— 此标志阻止无限递归。
    auto_retrying: bool,
    /// ★v24.19a 本驱动走哪条设备条目路线: true=会话黑名单 2056 (驱动 master 分支),
    /// false=持久黑名单 2051 (官方发布版 1.4.181 —— 无 2056, 已实测)。
    /// enable 时探测一次。
    use_session_list: bool,
}

impl HidHideController {
    pub fn is_on(&self) -> bool {
        self.runtime_on
    }

    pub fn error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }

    pub fn resolved_count(&self) -> usize {
        self.resolved.len()
    }

    pub fn unresolved_names(&self) -> &[String] {
        &self.unresolved
    }

    pub fn hidden_count(&self) -> usize {
        self.hidden_count
    }

    pub fn take_learned_dirty(&mut self) -> bool {
        std::mem::take(&mut self.learned_changed)
    }

    /// 驱动是否已安装 (上次扫描缓存的值; 扫描随白名单页渲染节流执行)。
    pub fn driver_installed(&self) -> bool {
        self.driver_installed
    }

    // -- 扫描 --

    /// 立即扫描: 驱动在场探测 + 运行中的白名单进程学完整路径 + 手柄实例刷新。
    /// 若运行态为开且实例集合变化, 自动重写驱动的会话黑名单 (清空+重加)。
    fn scan_now(&mut self, config: &mut AppConfig) {
        self.last_scan = Some(Instant::now());
        let was_installed = self.driver_installed;
        self.driver_installed = HidHide::driver_present();
        /* ★v24.19a: 驱动刚装好 → 清掉"未检测到驱动"的陈旧报错。
         * 否则用户装完驱动回到本页, 那行红字永远挂着, 看起来像"还是检测不到"。 */
        if self.driver_installed && !was_installed {
            if matches!(&self.last_error, Some(e) if e.contains("未检测到 HidHide")) {
                self.last_error = None;
            }
        }
        // 1) 学习运行中白名单进程的 NT 映像路径。
        //    ★v24.19a: 旧版本学到的 C:\ 盘符风格条目驱动匹配不上 —— 清掉重学。
        for name in config.process_whitelist.clone() {
            let learned_ok = config
                .hidhide_learned_paths
                .get(&name)
                .is_some_and(|p| is_native_image_path(p));
            if learned_ok {
                continue;
            }
            if let Some(full) = full_path_of_running_process(&name) {
                config.hidhide_learned_paths.insert(name.clone(), full);
                self.learned_changed = true;
            } else if config.hidhide_learned_paths.remove(&name).is_some() {
                // 不在运行且条目是旧格式: 删掉, 等下次运行时重学
                self.learned_changed = true;
            }
        }
        // 2) 重新计算已定位/未定位
        let mut resolved = Vec::new();
        let mut unresolved = Vec::new();
        for name in &config.process_whitelist {
            match config.hidhide_learned_paths.get(name) {
                Some(p) => resolved.push(p.clone()),
                None => unresolved.push(name.clone()),
            }
        }
        self.resolved = resolved;
        self.unresolved = unresolved;
        // 3) 手柄实例 (运行态为开才需要动驱动)
        if self.runtime_on {
            let instances = collect_pad_instances();
            let changed = instances.len() != self.hidden_count;
            if changed {
                if let Ok(hh) = HidHide::open() {
                    if self.use_session_list {
                        // 会话路线: 清掉重加 (2056 条目无法枚举, 只能整体重建)
                        if hh.clear_session_blacklist().is_ok() && !instances.is_empty() {
                            let _ = hh.add_session_blacklist(&instances);
                        }
                    } else {
                        // 持久路线: 原黑名单 (快照里) ∪ 当前实例
                        let orig = config
                            .hidhide_snapshot
                            .as_ref()
                            .map(|s| s.blacklist.clone())
                            .unwrap_or_default();
                        let _ = hh.set_blacklist(&merge_blacklist(&orig, &instances));
                    }
                }
                self.hidden_count = instances.len();
            }
        }
        /* ★v24.19a: 意图开着但运行态没起来 (启动时驱动还没装 / 游戏还没跑过),
         * 而现在驱动已到位 → 自动补开一次, 用户装完驱动回到本页即生效,
         * 不用再点一遍。auto_retrying 防住 enable→scan_now→enable 的递归。 */
        if config.hidhide_enabled
            && !self.runtime_on
            && self.driver_installed
            && !self.auto_retrying
        {
            self.auto_retrying = true;
            let _ = self.enable(config);
            self.auto_retrying = false;
        }
    }

    /// 节流版扫描 —— 白名单页每帧调用。
    pub fn maybe_scan(&mut self, config: &mut AppConfig) {
        if let Some(t) = self.last_scan
            && t.elapsed() < SCAN_INTERVAL
        {
            return;
        }
        self.scan_now(config);
    }

    // -- 开关 --

    /// 开启手柄隐藏。前置: 驱动已装、白名单非空、至少定位到一个游戏完整路径。
    pub fn enable(&mut self, config: &mut AppConfig) -> Result<(), String> {
        self.last_error = None;
        if config.process_whitelist.is_empty() {
            let msg = String::from("进程列表为空 —— 请先在上方添加游戏进程 (如 dnf.exe)");
            self.last_error = Some(msg.clone());
            return Err(msg);
        }
        let hh = HidHide::open().map_err(|e| {
            let msg = match e {
                HidHideError::NotInstalled => {
                    "未检测到 HidHide 驱动 —— 请先安装 HidHide (github.com/nefarius/HidHide)".to_string()
                }
                other => other.to_string(),
            };
            self.last_error = Some(msg.clone());
            msg
        })?;
        // 快照现役驱动状态 (随后修改, 退出恢复)
        let snap = {
            let active = hh.get_active().map_err(fail(self))?;
            let inverse = hh.get_inverse().map_err(fail(self))?;
            let whitelist = hh.get_whitelist().map_err(fail(self))?;
            let blacklist = hh.get_blacklist().map_err(fail(self))?;
            HidHideSnapshot { active, inverse, whitelist, blacklist }
        };
        // ★v24.19a 探测会话黑名单支持: master 分支驱动有 2056/2057,
        // 官方发布版 1.4.181 没有 (实测 87) → 降级到持久黑名单 2051。
        self.use_session_list = hh.session_supported();
        // 解析游戏完整路径 (顺带学习运行中的进程)
        self.scan_now(config);
        if self.resolved.is_empty() {
            let msg = "尚未定位到列表中游戏的完整路径 —— 请先启动一次游戏再开启 (位置会自动记住)"
                .to_string();
            self.last_error = Some(msg.clone());
            return Err(msg);
        }
        let instances = collect_pad_instances();
        if instances.is_empty() {
            let msg = "未检测到已连接的手柄 —— 请接好手柄再开启".to_string();
            self.last_error = Some(msg.clone());
            return Err(msg);
        }
        // 应用: 反向白名单 → 条目 → 激活位 (顺序保证任何中途失败都不会
        // 出现"条目已生效而作用域未就位"的窗口)。
        // 条目路线: 支持则走会话黑名单 (内核内存, 退出自动清);
        // 否则持久黑名单合并写入 (发布版驱动, 退出/崩溃靠快照恢复)。
        let entries = if self.use_session_list {
            hh.add_session_blacklist(&instances)
        } else {
            hh.set_blacklist(&merge_blacklist(&snap.blacklist, &instances))
        };
        let apply = hh
            .set_inverse(true)
            .and_then(|()| hh.set_whitelist(&self.resolved))
            .and_then(|()| entries)
            .and_then(|()| if snap.active { Ok(()) } else { hh.set_active(true) });
        if let Err(e) = apply {
            // 尽力回滚 (失败不阻塞报错)
            let _ = hh.set_whitelist(&snap.whitelist);
            let _ = hh.set_inverse(snap.inverse);
            let _ = hh.set_blacklist(&snap.blacklist);
            let _ = hh.set_active(snap.active);
            let msg = e.to_string();
            self.last_error = Some(msg.clone());
            return Err(msg);
        }
        config.hidhide_snapshot = Some(snap);
        self.hidden_count = instances.len();
        self.runtime_on = true;
        Ok(())
    }

    /// 关闭手柄隐藏: 清会话条目 + 恢复快照 (含持久黑名单)。驱动未装时仅清运行态与快照。
    pub fn disable(&mut self, config: &mut AppConfig) {
        self.last_error = None;
        if let Ok(hh) = HidHide::open() {
            // 先停隐藏 (条目清掉 —— 会话码不存在时返回 87, 忽略), 再恢复驱动全局状态
            let _ = hh.clear_session_blacklist();
            if let Some(snap) = config.hidhide_snapshot.take() {
                let _ = hh.set_whitelist(&snap.whitelist);
                let _ = hh.set_inverse(snap.inverse);
                let _ = hh.set_blacklist(&snap.blacklist);
                let _ = hh.set_active(snap.active);
            }
        } else {
            config.hidhide_snapshot = None;
        }
        self.runtime_on = false;
        self.hidden_count = 0;
    }

    /// 玩家开关 (白名单页状态药丸点击): 开↔关。
    pub fn toggle(&mut self, config: &mut AppConfig) {
        if self.runtime_on {
            self.disable(config);
            config.hidhide_enabled = false;
        } else {
            match self.enable(config) {
                Ok(()) => config.hidhide_enabled = true,
                Err(_) => config.hidhide_enabled = false,
            }
        }
    }

    /// 开机自愈: 上次退出未恢复正常 (崩溃/断电) → 恢复快照并落盘。
    /// 返回给 UI 的提示消息 (无需处理时为 None)。
    pub fn startup_recovery(config: &mut AppConfig) -> Option<String> {
        let snap = config.hidhide_snapshot.take()?;
        match HidHide::open() {
            Ok(hh) => {
                let _ = hh.clear_session_blacklist();
                let _ = hh.set_whitelist(&snap.whitelist);
                let _ = hh.set_inverse(snap.inverse);
                let _ = hh.set_blacklist(&snap.blacklist);
                let _ = hh.set_active(snap.active);
            }
            Err(_) => {
                // 驱动已不在 (卸载/换机): 快照无处恢复, 丢弃即可
            }
        }
        Some("检测到上次未正常退出 —— 已自动恢复 HidHide 驱动的原有设置".into())
    }

    /// 启动时按用户意图恢复运行态 (若驱动可用且能定位游戏)。
    /// 失败不打扰用户, 错误留状态行展示 + 写崩溃日志 (排查"为什么没自动开启")。
    pub fn restore_intent(&mut self, config: &mut AppConfig) {
        if !config.hidhide_enabled {
            return;
        }
        if let Err(e) = self.enable(config) {
            crate::util::crash_log("hidhide-restore", &e);
        }
    }
}

/// 组装"记入 last_error 并返回字符串"的闭包。
fn fail(ctrl: &mut HidHideController) -> impl FnOnce(HidHideError) -> String + '_ {
    move |e: HidHideError| {
        let msg = e.to_string();
        ctrl.last_error = Some(msg.clone());
        msg
    }
}

/// 按进程名找 PID (wchar 全名匹配, 不区分大小写) —— 与 auto_inject 同套路。
fn find_pids_by_name(name: &str) -> Vec<u32> {
    let target: Vec<u16> = name.to_lowercase().encode_utf16().collect();
    let mut pids = Vec::new();
    unsafe {
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else {
            return pids;
        };
        let mut pe = PROCESSENTRY32W::default();
        pe.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        if Process32FirstW(snap, &mut pe).is_ok() {
            loop {
                let exe: Vec<u16> = pe.szExeFile.iter().copied().take_while(|&c| c != 0).collect();
                if exe.len() == target.len() && exe == target {
                    pids.push(pe.th32ProcessID);
                }
                if Process32NextW(snap, &mut pe).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snap);
    }
    pids
}

/// 进程名 → 该进程可执行文件的**NT 映像路径** (\Device\HarddiskVolumeX\..., 未运行返回 None)。
/// ★v24.19a: 必须用 PROCESS_NAME_NATIVE —— HidHide 驱动拿内核映像加载通知里的 NT 路径
/// 与白名单条目做比对 (官方客户端自己存的也是 NT 格式); WIN32 风格的 C:\ 路径永不匹配。
fn full_path_of_running_process(name: &str) -> Option<String> {
    let pid = *find_pids_by_name(name).first()?;
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = vec![0u16; 1024];
        let mut size = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(h, PROCESS_NAME_NATIVE, PWSTR(buf.as_mut_ptr()), &mut size);
        let _ = CloseHandle(h);
        ok.ok()?;
        Some(String::from_utf16_lossy(&buf[..size as usize]))
    }
}

/// 学习表条目是否为驱动可比对的 NT 格式 (\Device\ 开头)。
/// v24.19 初版学的是 C:\ 盘符风格 —— 对驱动无效, 判假以触发清除重学。
pub fn is_native_image_path(p: &str) -> bool {
    p.starts_with(r"\Device\")
}

/// 学习表导出 (测试用)。
#[cfg(test)]
pub(crate) fn test_learned_paths_len(config: &AppConfig) -> usize {
    config.hidhide_learned_paths.len()
}

// ---------------------------------------------------------------------------
// 测试 (纯逻辑: IOCTL 黄金值 / MULTI_SZ / 匹配串 / 快照序列化)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// 黄金值: 与驱动源码 HidHideIoctlContract.h 的 CTL_CODE 逐个对账。
    /// CTL_CODE(32769, fn, METHOD_BUFFERED=0, FILE_READ_DATA=1)
    ///     = (32769<<16) | (1<<14) | (fn<<2) | 0
    #[test]
    fn ctl_codes_match_driver_contract() {
        assert_eq!(IOCTL_GET_WHITELIST, 0x8001_6000);
        assert_eq!(IOCTL_SET_WHITELIST, 0x8001_6004);
        assert_eq!(IOCTL_GET_BLACKLIST, 0x8001_6008);
        assert_eq!(IOCTL_SET_BLACKLIST, 0x8001_600C);
        assert_eq!(IOCTL_GET_ACTIVE, 0x8001_6010);
        assert_eq!(IOCTL_SET_ACTIVE, 0x8001_6014);
        assert_eq!(IOCTL_GET_WLINVERSE, 0x8001_6018);
        assert_eq!(IOCTL_SET_WLINVERSE, 0x8001_601C);
        assert_eq!(IOCTL_ADD_SESSION_BLACKLIST, 0x8001_6020);
        assert_eq!(IOCTL_CLR_SESSION_BLACKLIST, 0x8001_6024);
    }

    /// 编码后的 MULTI_SZ 必须双 NUL 结尾; 空列表也要满足驱动的
    /// "≥2 个 WCHAR 且末尾两个都是 NUL" 校验。
    #[test]
    fn multi_sz_terminators_and_roundtrip() {
        let empty = encode_multi_sz(&[]);
        assert_eq!(empty, vec![0, 0]);

        let one = encode_multi_sz(&["HID\\VID_045E&PID_028E\\8&1".to_string()]);
        assert_eq!(one.last(), Some(&0));
        assert_eq!(one[one.len() - 2], 0);

        let items = vec!["C:\\g\\DNF.exe".to_string(), "C:\\x\\TGP.exe".to_string()];
        let enc = encode_multi_sz(&items);
        assert_eq!(decode_multi_sz(&enc), items);
    }

    /// 解码容忍尾部垃圾 NUL 与空段。
    #[test]
    fn multi_sz_decode_tolerant() {
        assert_eq!(decode_multi_sz(&[0, 0]), Vec::<String>::new());
        assert_eq!(
            decode_multi_sz(&[65, 0, 66, 0, 0, 0]),
            vec!["A".to_string(), "B".to_string()]
        );
    }

    /// VID/PID 匹配串: 大写十六进制补零; 能命中 HID 与 USB(XInput 栈) 实例。
    #[test]
    fn vid_pid_needle_matches_instance_paths() {
        let n = vid_pid_needle(0x045E, 0x28E);
        assert_eq!(n, "VID_045E&PID_028E");
        assert!("HID\\VID_045E&PID_028E\\8&2F6DC50D&0&0000".to_uppercase().contains(&n));
        assert!("USB\\VID_045E&PID_028E\\5&2A5F0E1&0&8B08".to_uppercase().contains(&n));
        assert!(!"HID\\VID_054C&PID_09CC\\X".to_uppercase().contains(&n));
        // 低字节数字也补零到 4 位
        assert_eq!(vid_pid_needle(0x20BC, 0x5159), "VID_20BC&PID_5159");
    }

    /// 快照可进 TOML (Config.toml 持久化 + 崩溃自愈的载体)。
    #[test]
    fn snapshot_serde_roundtrip() {
        let snap = HidHideSnapshot {
            active: true,
            inverse: false,
            whitelist: vec!["C:\\a\\b.exe".into(), "C:\\c d\\e.exe".into()],
            blacklist: vec!["HID\\VID_045E&PID_028E\\8&1".into()],
        };
        let s = toml::to_string(&snap).unwrap();
        let back: HidHideSnapshot = toml::from_str(&s).unwrap();
        assert_eq!(back, snap);
    }

    /// v24.19 初版快照 (没有 blacklist 字段) 必须还能解析 —— 降级路线的字段带 serde 默认。
    #[test]
    fn snapshot_without_blacklist_field_parses() {
        // TOML 单引号字面串: 反斜杠不转义, 一个就是一一个
        let s = "active = true\ninverse = false\nwhitelist = ['C:\\a\\b.exe']\n";
        let snap: HidHideSnapshot = toml::from_str(s).unwrap();
        assert!(snap.active);
        assert_eq!(snap.whitelist, vec!["C:\\a\\b.exe".to_string()]);
        assert!(snap.blacklist.is_empty());
    }

    /// 持久黑名单合并: 原条目保持在前 + 新实例追加 + 忽略大小写去重。
    #[test]
    fn merge_blacklist_unions_and_dedupes() {
        let orig = vec!["HID\\VID_054C&PID_09CC\\X".to_string()];
        let inst = vec![
            "HID\\VID_045E&PID_028E\\8&1".to_string(),
            // 大小写不同的同一条目不得重复
            "hid\\vid_054c&pid_09cc\\x".to_string(),
        ];
        let merged = merge_blacklist(&orig, &inst);
        assert_eq!(merged.len(), 2, "大小写不同的重复条目应合并: {merged:?}");
        assert_eq!(merged[0], orig[0], "原条目保持在前");
        assert_eq!(merged[1], inst[0]);
        // 空原表 = 纯追加
        assert_eq!(merge_blacklist(&[], &inst), inst);
    }

    /// 控制器默认值: 关、无错误、列表空。
    #[test]
    fn controller_defaults_off() {
        let c = HidHideController::default();
        assert!(!c.is_on());
        assert!(c.error().is_none());
        assert_eq!(c.resolved_count(), 0);
        assert_eq!(c.hidden_count(), 0);
        assert!(c.unresolved_names().is_empty());
    }

    /// toggle 在驱动未装时: 运行态保持关, 意图落回关, 错误可读。
    #[test]
    fn toggle_without_driver_stays_off() {
        // 前提: 测试机未安装 HidHide (构建机常态); 若装了驱动则跳过本用例
        if HidHide::driver_present() {
            return; // 驱动在场时此路径不该走, 交由真机手工验收
        }
        let mut config = crate::config::AppConfig::default();
        config.process_whitelist.push("dnf.exe".into());
        let mut c = HidHideController::default();
        c.toggle(&mut config);
        assert!(!c.is_on());
        assert!(!config.hidhide_enabled);
        assert!(c.error().is_some(), "应留下『未检测到驱动』提示");
    }

    /// 学习表是 BTreeMap: 进程名 → 完整路径 (serde 默认空, 兼容旧配置)。
    #[test]
    fn learned_paths_map_default_empty() {
        let config = crate::config::AppConfig::default();
        assert_eq!(test_learned_paths_len(&config), 0);
        assert!(!config.hidhide_enabled);
        assert!(config.hidhide_snapshot.is_none());
    }

    /// ★v24.19a: 驱动按 NT 映像路径匹配 —— 只有 \Device\ 开头才算学到家;
    /// C:\ 盘符风格 (v24.19 初版学习产物) 判非原生, 会触发清除重学。
    #[test]
    fn native_image_path_detection() {
        assert!(is_native_image_path(r"\Device\HarddiskVolume4\Users\x\python.exe"));
        assert!(!is_native_image_path(r"C:\Users\x\python.exe"));
        assert!(!is_native_image_path("python.exe"));
        assert!(!is_native_image_path(""));
    }

    /// 真机诊断 (不进常规套件): `cargo test --release --lib diag_real -- --ignored --nocapture`
    /// 打印应用视角的手柄 VID/PID 与待隐藏实例, 用于排查"enable 被前置校验拒绝"。
    #[test]
    #[ignore = "需要真机驱动与手柄, 仅手动诊断"]
    fn diag_real_machine_pad_instances() {
        println!("driver_present = {}", HidHide::driver_present());
        if let Ok(hh) = HidHide::open() {
            println!("session_supported = {}", hh.session_supported());
        }
        let pairs = collect_pad_vid_pids();
        println!("pad vid_pids = {pairs:04X?}");
        let instances = collect_pad_instances();
        for i in &instances {
            println!("instance: {i}");
        }
        assert!(!instances.is_empty(), "未找到手柄实例 —— enable 的'未检测到手柄'即由此触发");
    }
}


*/
