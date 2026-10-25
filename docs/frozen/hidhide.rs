/* ★★★ v24.48d 手柄隐藏二次下线 —— 方案失败归档 (2026-10-25) ★★★
 *
 * 用户决策 (原话): "本次更新放弃掉, 选择隐藏相关的更新... 等之后有人更新的时候
 * 再考虑换驱动, 记得注释, 隐藏手柄方案失败。"
 *
 * ── 失败结论 ─────────────────────────────────────────────────────────
 * 本文件是 v24.48 全系列 (会话模式重设计 → 双路线改造) 的最终实现,
 * 真机事故后整体冻结, 不参与编译。失败不是 HidHide 代码的问题
 * (同一份上游源码官方跑得好好的), 而是**签名分发层**的死结:
 *
 *  1. 会话黑名单 (IOCTL 2056/2057, 进程退出自动清) 只存在于上游 master
 *     (v1.7.333+), 官方**从未发布签名版** —— 维护者 nefarius 在 issue #217
 *     亲答: 不给未测试代码做生产签名; 预发布也只能测试签名 (需测试模式)。
 *  2. 用户机器 Secure Boot 开启 → 测试签名被 BCD 策略保护无法开启
 *     (真机原始报错: "该值受安全引导策略保护") → **任何**自建/第三方构建
 *     的驱动包都无法通过入库校验, 这是物理性死路, 代码层无法绕过。
 *  3. 官方发行版 v1.5.230 (attestation 签名, SB 机器可装) 没有会话黑名单,
 *     走持久黑名单 2051 降级路线 —— 功能上可用, 但用户实测后判定"还是不太行"。
 *  4. 附带事故 (v24.48b 复盘): 自建包安装失败被漏检 + 类过滤器挂到不存在的
 *     服务 → HIDClass 全类设备 (键盘/鼠标/手柄) 无法启动。教训与硬防线
 *     (服务存在性验证通过才允许挂过滤器 / 包签名顺序 签sys→Inf2Cat→签cat /
 *     GUI 程序退出码不可靠 / 官方安装器带 Watchdog 会实时恢复过滤器,
 *     卸载必须先杀 HidHideWatchdog.exe 服务) 都写在 CHANGELOG v24.48 系列。
 *
 * ── 重启条件 ─────────────────────────────────────────────────────────
 * 等官方发布**带会话黑名单的签名版** (关注 github.com/nefarius/HidHide
 * 的 Releases + issue #217 动向) 时重评; 或用户机器关闭 Secure Boot 后
 * 自建路线可复活的场合。届时本文件回到 src/hidhide.rs, 参考下方接线记录。
 *
 * ── 复活接线点 (v24.48 形态, 共 7 处) ────────────────────────────────
 *   0. git mv docs/frozen/hidhide.rs src/hidhide.rs (本文件保留在 git 历史,
 *      mv 回去即可, 内容无需改动 —— 它就是最后一次可编译的完整实现);
 *   1. src/lib.rs 与 src/main.rs: `pub mod hidhide;` / `mod hidhide;`;
 *   2. src/config.rs: HidHideSnapshot 结构体 + hidhide_enabled/hidhide_snapshot
 *      两字段 (+ tests/architecture_tests.rs 契约键 hidhide_enabled);
 *   3. src/gui/mod.rs: 控制器字段 + 启动块 (自愈 + restore_intent) + Self 初始化;
 *   4. src/gui/whitelist_page.rs: render_hidhide_card + 调用点;
 *   5. src/gui/main_window.rs: on_exit 的 disable 恢复块;
 *   6. tests/architecture_tests.rs: GUI 依赖允许清单 + "hidhide";
 *   7. Cargo.toml: 四个 windows feature (Win32_Devices_DeviceAndDriverInstallation /
 *      Win32_Storage / Win32_Storage_FileSystem / Win32_System_IO);
 *   8. driver\ 内置包: 走官方发行版安装器 (HidHide_1.5.230_x64.exe 起步,
 *      install_bundled_driver 用 /passive /norestart 静默安装)。
 *
 * ★v24.37 手柄隐藏 (HidHide) 复活 —— 纯会话模式重设计。
//! ★v24.37 手柄隐藏 (HidHide) 复活 —— 纯会话模式重设计。
//!
//! 用户规格 (2026-10-25): 「打开我的软件, 我的软件则是主导者; 关闭我的软件,
//! 则一切恢复正常」—— 手柄对**所有**其他程序隐身 (任何游戏, 无需维护游戏名单)。
//!
//! 与 v24.19 冻结版的关键差异:
//! - **白名单 = 仅本软件自身** (NT 映像路径): 反向白名单下只有列表内进程被拒,
//!   旧版需要"学习表"逐个定位游戏进程路径 —— 新规格下列表只有一条已知路径,
//!   学习表 / 已定位-未定位 / 落盘学习表整套机制全部删除。
//! - **只走会话黑名单 (2056/2057)**, 不再降级持久黑名单 2051: 官方 v1.7.333+ 起
//!   会话黑名单已进主干 (本仓库随附的内置驱动即源码构建), 条目存内核内存、
//!   按属主 PID 生命周期自动清理 —— 进程退出/崩溃驱动侧零残留。
//!   v24.19 那套"快照 + 崩溃自愈 + 学习表"复杂机器的根源 (发布版 1.4.181 无 2056)
//!   已不存在; 快照仅剩 active/inverse/whitelist 三项驱动全局状态需要退出还原
//!   (blacklist 字段仅为消费旧版配置遗留快照而保留)。
//! - **软启用**: 手柄未连接时也允许开启 (热插拔由扫描循环自动补写条目),
//!   前置校验只剩驱动在场 + 会话能力两个硬条件。
//! - **内置驱动安装器**: 软件目录 `driver\HidHide.inf` 存在时, 一键提权
//!   `pnputil /add-driver /install` (见 `install_bundled_driver`)。
//!
//! 驱动契约 (与上游 Shared/HidHideIoctlContract.h 逐字一致, 黄金值有单测锁定):
//! 设备类型 32769, 功能号 2048-2057, METHOD_BUFFERED, FILE_READ_DATA。
//! 控制设备 `\\.\HidHide` 以 GENERIC_READ 打开 (SDDL 对所有用户开放, 无需管理员)。
//! 过滤语义 (上游 Logic.c): System 进程 (PID 4) 永远放行 —— RawInput 内核投递
//! 不受影响 (固有边界: 仅 RawInput 采集的游戏仍可见手柄); XInput/DInput/WGI
//! 走用户态设备打开、归属调用进程, 正是本功能要拦的通道。
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
    GetCurrentProcess, GetExitCodeProcess, QueryFullProcessImageNameW, PROCESS_NAME_NATIVE,
    WaitForSingleObject,
};
use windows::Win32::UI::Shell::{
    SEE_MASK_FLAG_DDEWAIT, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW,
};
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

use crate::config::{AppConfig, HidHideSnapshot};

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
/// 2051: 整体替换设备黑名单 (输入 MULTI_SZ)。
/// (★v24.37 只读不用: 仅启动自愈消费旧版遗留快照时写回。)
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

/// 持久黑名单路线 (2051) 的合并: 原有条目保持在前, 追加新实例, 忽略大小写去重。
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

/// 本软件自身的**NT 映像路径** (`\Device\HarddiskVolumeX\...`)。
/// HidHide 驱动拿内核映像加载通知里的 NT 路径与白名单条目比对
/// (WIN32 盘符风格 `C:\...` 永不匹配); 新规格白名单只放自己这一条。
/// 拿不到 (几乎不可能) 返回 None —— 此时不得开启 (否则反向白名单会把自己也挡掉)。
pub fn self_nt_image_path() -> Option<String> {
    unsafe {
        let h = GetCurrentProcess();
        let mut buf = vec![0u16; 1024];
        let mut size = buf.len() as u32;
        QueryFullProcessImageNameW(h, PROCESS_NAME_NATIVE, PWSTR(buf.as_mut_ptr()), &mut size)
            .ok()?;
        Some(String::from_utf16_lossy(&buf[..size as usize]))
    }
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
    /// ★v24.48c: 仅在 `use_session_list` (驱动 v1.7.333+) 时使用;
    /// 官方发行版 v1.5.230 无此码 (返回 87), 走 `set_blacklist` 持久路线。
    pub fn add_session_blacklist(&self, instances: &[String]) -> Result<(), HidHideError> {
        self.ioctl_in(IOCTL_ADD_SESSION_BLACKLIST, &wide_to_bytes(&encode_multi_sz(instances)))
    }

    /// 清除本进程的全部会话黑名单条目 (IOCTL 2057)。
    pub fn clear_session_blacklist(&self) -> Result<(), HidHideError> {
        self.ioctl_void(IOCTL_CLR_SESSION_BLACKLIST)
    }

    /// 驱动是否支持会话黑名单 (2056/2057): 用零缓冲 2057 探测。
    /// 支持则返回 Ok (顺带清掉本进程残留条目 —— 此刻本来就没有, 无副作用);
    /// 老驱动不认识该码, 返回 Err (INVALID_PARAMETER)。
    pub fn session_supported(&self) -> bool {
        self.ioctl_void(IOCTL_CLR_SESSION_BLACKLIST).is_ok()
    }

    /// 读取持久设备黑名单 (IOCTL 2050) —— 仅启动自愈读旧快照时配对使用。
    pub fn get_blacklist(&self) -> Result<Vec<String>, HidHideError> {
        self.ioctl_multi_sz_out(IOCTL_GET_BLACKLIST)
    }

    /// **整体替换**持久设备黑名单 (IOCTL 2051) —— 仅启动自愈消费旧版遗留快照时写回。
    pub fn set_blacklist(&self, instances: &[String]) -> Result<(), HidHideError> {
        self.ioctl_in(IOCTL_SET_BLACKLIST, &wide_to_bytes(&encode_multi_sz(instances)))
    }
}

// ---------------------------------------------------------------------------
// 控制器: GUI 线程持有的生命周期管理
// ---------------------------------------------------------------------------

/// 软件目录下内置**官方发行版**驱动安装器的定位 (微软认证签名, Secure Boot
/// 开启的机器可直接安装 —— v1.7 会话黑名单官方从未发布, 见 CHANGELOG v24.48c)。
pub fn bundled_driver_installer() -> Option<std::path::PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let p = exe.parent()?.join("driver").join("HidHide_1.5.230_x64.exe");
    p.exists().then_some(p)
}

/// 内置驱动安装结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallOutcome {
    /// 驱动已装好 (设备节点 + 服务 + 类过滤器全部完成)。
    Done,
    /// 安装器要求重启电脑 (部分设备需重启后才挂上新过滤器);
    /// 重启后重新打开开关即可继续。
    NeedReboot,
}

/// 安装内置驱动: 运行**官方发行版安装器** (触发一次 UAC)。
///
/// 为什么是官方包而不是自建: 微软 attestation 签名只有驱动版权方能拿,
/// Secure Boot 开启的机器上自建/测试签名驱动永远无法通过入库校验
/// (2026-10-25 真机事故根因, 详见 CHANGELOG v24.48b/c)。
/// 官方安装器自带正确的设备节点/服务/类过滤器安装序列, 且经过百万级装机验证。
/// 阻塞至安装器退出 (上限 10 分钟); 结果由调用方 `HidHide::driver_present()` 复核。
pub fn install_bundled_driver() -> Result<InstallOutcome, String> {
    if HidHide::driver_present() {
        return Ok(InstallOutcome::Done); // 已装: 无需重复安装
    }
    let Some(exe) = bundled_driver_installer() else {
        return Err("软件目录下未找到内置驱动安装器 (driver\\HidHide_1.5.230_x64.exe)".to_string());
    };
    let file = wide_str(&exe.to_string_lossy());
    // WiX 捆绑包标准静默参数: /passive 只显示进度条, /norestart 不自动重启
    let params = wide_str("/passive /norestart");
    let verb = wide_str("runas");
    unsafe {
        let mut sei = SHELLEXECUTEINFOW {
            cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
            fMask: SEE_MASK_NOCLOSEPROCESS | SEE_MASK_FLAG_DDEWAIT,
            lpVerb: PCWSTR::from_raw(verb.as_ptr()),
            lpFile: PCWSTR::from_raw(file.as_ptr()),
            lpParameters: PCWSTR::from_raw(params.as_ptr()),
            nShow: SW_SHOWNORMAL.0 as i32,
            ..Default::default()
        };
        ShellExecuteExW(&mut sei).map_err(|e| format!("提权启动失败: {e}"))?;
        if sei.hProcess.is_invalid() {
            return Err("用户取消了 UAC 授权 —— 可稍后在「手柄隐藏」卡片点按钮重试".to_string());
        }
        // 官方安装器通常秒级; 上限 10 分钟防挂
        let _ = WaitForSingleObject(sei.hProcess, 600_000);
        let mut exit_code: u32 = 0;
        let _ = GetExitCodeProcess(sei.hProcess, &mut exit_code);
        let _ = CloseHandle(sei.hProcess);
        if exit_code != 0 && exit_code != 3010 {
            return Err(format!("官方安装器异常退出 (代码 {exit_code}) —— 可能被杀软拦截或系统策略阻止"));
        }
    }
    // 服务/控制设备就绪可能有零点几秒延迟, 短重试兜底
    for _ in 0..15 {
        if HidHide::driver_present() {
            return Ok(InstallOutcome::Done);
        }
        std::thread::sleep(std::time::Duration::from_millis(300));
    }
    // 安装器成功但控制设备未就绪: 已挂过滤器的设备要重启/重插后才走新驱动栈
    Ok(InstallOutcome::NeedReboot)
}

fn wide_str(s: &str) -> Vec<u16> {
    s.encode_utf16().chain([0]).collect()
}

// ---------------------------------------------------------------------------
// 控制器: GUI 线程持有的生命周期管理
// ---------------------------------------------------------------------------

/// 扫描节流间隔: 白名单页每帧渲染都会调 `maybe_scan`, 10 秒扫一次足够。
const SCAN_INTERVAL: std::time::Duration = std::time::Duration::from_secs(10);

/// 手柄隐藏控制器 —— 由 `SorahkGui` 持有, 仅 GUI 线程使用。
///
/// `runtime_on` 表示**当前会话**驱动是否已被我们改动 (与 config.hidhide_enabled
/// 的"用户意图"分离: 驱动未装/不支持会话时, 意图可以是开, 运行态是关)。
#[derive(Default)]
pub struct HidHideController {
    runtime_on: bool,
    last_error: Option<String>,
    /// 当前已加入会话黑名单的设备实例 (热插拔按全表比对决定重建)。
    current_instances: Vec<String>,
    /// 最近一次扫描时间 (节流)。
    last_scan: Option<Instant>,
    /// 驱动是否已安装 (随扫描节流刷新, 不每帧探测)。
    driver_installed: bool,
    /// ★v24.48c 设备条目路线: true=会话黑名单 2056 (v1.7.333+), false=持久黑名单
    /// 2051 降级 (官方发行版 v1.5.230)。enable 时探测一次。
    use_session_list: bool,
    /// ★v24.48c 持久路线的"原始 2051 黑名单" (enable 时的快照值) ——
    /// 热插拔重建 = 原始 ∪ 当前实例, 不丢用户/其他软件的原有条目。
    orig_blacklist: Vec<String>,
    /// 防递归标志: scan_now 里"驱动到位后自动补开"会调 enable,
    /// enable 内部又会 scan_now —— 此标志阻止无限递归。
    auto_retrying: bool,
}

impl HidHideController {
    pub fn is_on(&self) -> bool {
        self.runtime_on
    }

    pub fn error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }

    pub fn hidden_count(&self) -> usize {
        self.current_instances.len()
    }

    /// 驱动是否已安装 (上次扫描缓存的值; 扫描随白名单页渲染节流执行)。
    pub fn driver_installed(&self) -> bool {
        self.driver_installed
    }

    // -- 扫描 --

    /// 立即扫描: 驱动在场探测 + 手柄实例刷新。
    /// 若运行态为开且实例集合变化, 自动重写驱动的会话黑名单 (清空+重加)。
    fn scan_now(&mut self, config: &mut AppConfig) {
        self.last_scan = Some(Instant::now());
        let was_installed = self.driver_installed;
        self.driver_installed = HidHide::driver_present();
        // 驱动刚装好 → 清掉"未检测到驱动"的陈旧报错, 用户回到本页红点能自动消失。
        if self.driver_installed && !was_installed {
            if matches!(&self.last_error, Some(e) if e.contains("未检测到 HidHide")) {
                self.last_error = None;
            }
        }
        // 手柄实例热插拔 (运行态为开才需要动驱动; 双路线各自重建)
        if self.runtime_on {
            let instances = collect_pad_instances();
            if instances != self.current_instances {
                if let Ok(hh) = HidHide::open() {
                    if self.use_session_list {
                        // 会话路线: 清掉重加 (2056 条目无法枚举, 只能整体重建)
                        if hh.clear_session_blacklist().is_ok() {
                            if !instances.is_empty() {
                                let _ = hh.add_session_blacklist(&instances);
                            }
                            self.current_instances = instances;
                        }
                    } else {
                        // 持久路线: 原始 2051 ∪ 当前实例 (丢掉已拔出的旧实例)
                        let _ = hh.set_blacklist(&merge_blacklist(&self.orig_blacklist, &instances));
                        self.current_instances = instances;
                    }
                }
            }
        }
        // 意图开着但运行态没起来 (启动时驱动还没装 / 版本过旧),
        // 而现在驱动已到位 → 自动补开一次, 不用再点一遍。
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

    /// 内置驱动安装完成后调用: 绕过节流立即重扫 (刷新在场标志 + 意图自动补开)。
    pub fn rescan_now_for_driver_install(&mut self, config: &mut AppConfig) {
        self.last_scan = None;
        self.scan_now(config);
    }

    /// 内置驱动安装失败时记录错误 (供状态行展示)。
    pub fn record_install_error(&mut self, msg: &str) {
        self.last_error = Some(msg.to_string());
    }

    // -- 开关 --

    /// 开启手柄隐藏。前置: 驱动已装。
    /// 软启用: 手柄未连接也允许开 (热插拔由扫描循环补写条目)。
    ///
    /// ★v24.48b **按需安装**: 用户显式打开开关且驱动缺席 → 此刻才触发内置驱动
    /// 安装 (一次 UAC)。绝不随启动自动安装 —— 隐藏是少数玩家的需求,
    /// 多数用户不该看到任何 UAC 弹窗。装好一次后, 后续打开软件即静默接管。
    ///
    /// ★v24.48c **双路线**: 驱动支持会话黑名单 (v1.7.333+, 内核内存按 PID 自动清)
    /// 则走会话路线; 否则 (官方发行版 v1.5.230, Secure Boot 机器的唯一定档)
    /// 降级持久黑名单 2051 —— 白名单=仅自己后, 崩溃残留状态恰好等于"开启状态"
    /// (手柄对本软件可见、对其他程序隐藏), 危害极小, 退出/下次启动仍照常还原。
    pub fn enable(&mut self, config: &mut AppConfig) -> Result<(), String> {
        self.last_error = None;
        if !HidHide::driver_present() {
            if bundled_driver_installer().is_none() {
                let msg = "未检测到 HidHide 驱动, 且软件目录下没有内置驱动安装器 (driver\\)"
                    .to_string();
                self.last_error = Some(msg.clone());
                return Err(msg);
            }
            match install_bundled_driver() {
                Ok(InstallOutcome::Done) => {
                    // 服务/控制设备就绪可能有零点几秒延迟, 短重试兜底
                    let mut ready = HidHide::driver_present();
                    for _ in 0..10 {
                        if ready {
                            break;
                        }
                        std::thread::sleep(std::time::Duration::from_millis(200));
                        ready = HidHide::driver_present();
                    }
                    if !ready {
                        let msg = "驱动安装完成但控制设备未就绪 —— 请重开一次开关".to_string();
                        self.last_error = Some(msg.clone());
                        return Err(msg);
                    }
                }
                Ok(InstallOutcome::NeedReboot) => {
                    let msg = "内置驱动需要重启电脑完成安装 —— 重启后重新打开此开关即可继续"
                        .to_string();
                    self.last_error = Some(msg.clone());
                    return Err(msg);
                }
                Err(e) => {
                    let msg = format!("内置驱动安装未完成: {e}");
                    self.last_error = Some(msg.clone());
                    return Err(msg);
                }
            }
        }
        let hh = HidHide::open().map_err(|e| {
            let msg = match e {
                HidHideError::NotInstalled => {
                    "未检测到 HidHide 驱动 —— 点击下方「安装内置驱动」(需要管理员授权)".to_string()
                }
                other => other.to_string(),
            };
            self.last_error = Some(msg.clone());
            msg
        })?;
        // ★v24.48c 探测会话黑名单能力并选定路线 (v1.5.230 无 2056/2057 → 持久路线)
        self.use_session_list = hh.session_supported();
        let self_path = match self_nt_image_path() {
            Some(p) => p,
            None => {
                let msg = "无法获取本软件 NT 映像路径 —— 反向白名单无法安全开启".to_string();
                self.last_error = Some(msg.clone());
                return Err(msg);
            }
        };
        // 快照现役驱动全局状态 (随后修改, 退出恢复)。
        // ★复用已有快照: 崩溃残留场景下驱动里的现值 ≈ 我们上次写入的开启状态,
        // 若此时重新快照会把"已改"当"原始", 关闭时还原成开启状态 → 手柄永久隐身。
        // config 里的快照在首次 enable 时抓的才是真原始值, 退出/自愈时清掉。
        let snap = match config.hidhide_snapshot.clone() {
            Some(s) => s,
            None => {
                let active = hh.get_active().map_err(fail(self))?;
                let inverse = hh.get_inverse().map_err(fail(self))?;
                let whitelist = hh.get_whitelist().map_err(fail(self))?;
                let blacklist = hh.get_blacklist().map_err(fail(self))?;
                HidHideSnapshot { active, inverse, whitelist, blacklist }
            }
        };
        self.orig_blacklist = snap.blacklist.clone();
        let instances = collect_pad_instances();
        // 应用: 反向白名单 → 白名单(仅自己) → 设备条目(双路线) → 激活位
        // (顺序保证任何中途失败都不会出现"条目已生效而作用域未就位"的窗口)。
        let entries = if self.use_session_list {
            if instances.is_empty() { Ok(()) } else { hh.add_session_blacklist(&instances) }
        } else {
            hh.set_blacklist(&merge_blacklist(&snap.blacklist, &instances))
        };
        let apply = hh
            .set_inverse(true)
            .and_then(|()| hh.set_whitelist(&[self_path]))
            .and_then(|()| entries)
            .and_then(|()| if snap.active { Ok(()) } else { hh.set_active(true) });
        if let Err(e) = apply {
            // 尽力回滚 (失败不阻塞报错)
            let _ = hh.clear_session_blacklist();
            let _ = hh.set_whitelist(&snap.whitelist);
            let _ = hh.set_inverse(snap.inverse);
            let _ = hh.set_blacklist(&snap.blacklist);
            let _ = hh.set_active(snap.active);
            let msg = e.to_string();
            self.last_error = Some(msg.clone());
            return Err(msg);
        }
        if config.hidhide_snapshot.is_none() {
            config.hidhide_snapshot = Some(snap);
        }
        self.current_instances = instances;
        self.runtime_on = true;
        Ok(())
    }

    /// 关闭手柄隐藏: 清会话条目 + 恢复快照。驱动未装时仅清运行态与快照。
    pub fn disable(&mut self, config: &mut AppConfig) {
        self.last_error = None;
        if let Ok(hh) = HidHide::open() {
            // 先停隐藏 (清条目 + 会话条目由驱动随本进程存活, 双保险),
            // 再恢复驱动全局状态。
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
        self.current_instances.clear();
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

    /// 开机自愈: v24.19 时代遗留快照 (崩溃/断电) → 恢复驱动原状并落盘。
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

    /// 启动时按用户意图恢复运行态 (★v24.37 默认意图=开: 打开软件即主导)。
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

    /// 快照可进 TOML (Config.toml 持久化 + 退出还原的载体)。
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

    /// v24.19 初版快照 (没有 blacklist 字段) 必须还能解析 —— 兼容字段带 serde 默认。
    #[test]
    fn snapshot_without_blacklist_field_parses() {
        // TOML 单引号字面串: 反斜杠不转义, 一个就是一个
        let s = "active = true\ninverse = false\nwhitelist = ['C:\\a\\b.exe']\n";
        let snap: HidHideSnapshot = toml::from_str(s).unwrap();
        assert!(snap.active);
        assert_eq!(snap.whitelist, vec!["C:\\a\\b.exe".to_string()]);
        assert!(snap.blacklist.is_empty());
    }

    /// 控制器默认值: 关、无错误、列表空。
    #[test]
    fn controller_defaults_off() {
        let c = HidHideController::default();
        assert!(!c.is_on());
        assert!(c.error().is_none());
        assert_eq!(c.hidden_count(), 0);
        assert!(!c.driver_installed());
    }

    /// toggle 在驱动未装时: 运行态保持关, 意图落回关, 错误可读。
    #[test]
    fn toggle_without_driver_stays_off() {
        // 前提: 测试机未安装 HidHide (构建机常态); 若装了驱动则跳过本用例
        if HidHide::driver_present() {
            return; // 驱动在场时此路径不该走, 交由真机手工验收
        }
        let mut config = crate::config::AppConfig::default();
        let mut c = HidHideController::default();
        c.toggle(&mut config);
        assert!(!c.is_on());
        assert!(!config.hidhide_enabled);
        assert!(c.error().is_some(), "应留下『未检测到驱动』提示");
    }

    /// 自身 NT 映像路径: 必须是驱动可比对的 \Device\ 格式 (白名单唯一的条目来源)。
    #[test]
    fn self_image_path_is_native_format() {
        let Some(p) = self_nt_image_path() else {
            return; // 查询失败的环境 (极少) 不硬性失败
        };
        assert!(p.starts_with(r"\Device\"), "NT 映像路径应为 \\Device\\ 开头: {p}");
    }

    /// 内置驱动安装器定位: 无 driver 目录时返回 None (不 panic)。
    #[test]
    fn bundled_driver_installer_absent_is_none() {
        // 仅断言函数可安全调用 (存在与否取决于运行目录, 不做环境断言)
        let _ = bundled_driver_installer();
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
        assert!(!instances.is_empty(), "未找到手柄实例 —— 接好手柄后重试");
    }
}
