//! auto_inject — 免 Loader 自动挂载采集 DLL (★v24.23 智能识别版, 支持多客户端)
//!
//! 设计目标(用户约束):
//!   * 文件名坚持本名 (DfoVibration_OLD.dll / DfoVibration.dll), 不借用 version.dll 等假名;
//!   * 不修改游戏的任何原始 DLL / EXE;
//!   * 不需要手动运行 Loader; 宿主默认普通权限 (90CN 客户端自提权 → 见使用说明右键管理员);
//!   * 客户端一启动, **正确版本**的采集 DLL 自动进入游戏进程 -> 事件流直达宿主。
//!
//! ★v24.23 智能识别 (本版核心): 同名 DLL 多客户端共用 (OLD.dll 被 ACT1/4/5/40JP 共用,
//! DfoVibration.dll 被 90US/90CN 共用), 不改名怎么分辨? —— **各版构建在字节里自证身份**
//! (内嵌 "target=…" 日志串: ACT1/ACT4/ACT5/40JP/90CN, 实测每版命中 1~3 次):
//!   1. 客户端身份: 进程名直判 (ARAD.exe→40JP, DNFACT4·5→ACT4/5); DNF.exe 一名多客 →
//!      借客户端目录判 (us_extend_dll→90US 不注入; 有新一代 DLL→90CN; 目录 OLD 的
//!      内嵌标签→该标签);
//!   2. 候选 = 实际存在的 DLL (客户端目录两代 + 宿主目录 OLD 兜底) 逐一读标签;
//!   3. 身份匹配者注入; 全部无标签 → 按部署位置注入 (日志提示未验证);
//!      **身份冲突 → 拒绝注入** (错版 DLL hook 错地址, 轻则无信号重则崩游戏;
//!      有冲突也不退回无标签候选 —— 宁可不注, 不可注错)。
//! ★v24.22 部署真值优先: 注入跟客户端目录实际部署走, 路线只在两代都在时定优先级;
//!   (v24.20 的路线门控把"路线设错"变成"完全不注入" —— 90CN/ACT4 双双未连接的根因)。
//! ★v24.20a 宿主默认普通权限 (highestAvailable manifest 已撤; SORAHK_MANIFEST=1 可恢复)。
//!
//! 原理 (注入机制与初版一致): 本宿主 64 位, 游戏 32 位 -> 跨位远程注入:
//!   轮询找客户端 PID -> OpenProcess -> 枚举 32 位模块找 kernel32 基址 -> 读导出表
//!   解析 LoadLibraryA (32 位地址) -> VirtualAllocEx 写路径 -> CreateRemoteThread
//!   -> DLL DllMain(ATTACH) 自启 worker/collector (建 Local\DfoVibrationShm + 装 hook)。
//!   注入成功记录 PID; 客户端重启自动重注入; 失败静默重试 (1500ms)。
//!   测试开关: 环境变量 SORAHK_NO_AUTO_INJECT=1 时本线程空转。
//!   日志: 同步写 SorahkDFO_vib.log (eprintln 在无控制台 GUI 里全部丢失)。


#![allow(unsafe_op_in_unsafe_fn)]

use std::ffi::OsString;
use std::os::windows::ffi::OsStrExt;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use windows::Win32::Foundation::{CloseHandle, HANDLE, HMODULE, MAX_PATH};
use windows::Win32::System::Diagnostics::Debug::{ReadProcessMemory, WriteProcessMemory};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
    TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Memory::{
    VirtualAllocEx, VirtualFreeEx, MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_READWRITE,
};
use windows::Win32::System::ProcessStatus::{
    EnumProcessModulesEx, GetModuleBaseNameW, LIST_MODULES_32BIT,
};
use windows::Win32::System::Threading::{OpenProcess, PROCESS_ALL_ACCESS};

use crate::state::AppState;

const POLL_MS: u64 = 1500;

/// 客户端进程名表 (★v24.23: 不绑定路线/DLL 名 —— 身份与 DLL 选择见 `pick_dll_to_inject`)
struct Client {
    process: &'static str,
}

const CLIENTS: &[Client] = &[
    Client { process: "DNF.exe" },      // ACT1 / ACT4·5 双名之一 / 90US(自挂载) / 90CN
    Client { process: "DNFACT4.exe" },  // ACT4 备用名
    Client { process: "DNF ACT4.exe" }, // ACT4 实测包名
    Client { process: "DNFACT5.exe" },  // ACT5 备用名
    Client { process: "DNF ACT5.exe" }, // ACT5 备用名
    Client { process: "ARAD.exe" },     // 40JP
];

/// 客户端目录出现该文件 = 90US 挂载形态, 宿主绝不注入 (自挂载已含采集 DLL)。
const US_EXTEND_MARKER: &str = "us_extend_dll\\DfoVibration.dll";

/// 已知客户端身份标签 (各 profile 构建在 "target=…" 日志串里自证身份; 旧版构建无标签)。
const PROFILE_TAGS: &[&str] = &["ACT1", "ACT4", "ACT5", "40JP", "90CN", "90US"];

/// 字节串计数 (Vec<u8> 没有现成的 matches)。
fn count_subst(hay: &[u8], needle: &[u8]) -> usize {
    if needle.is_empty() || hay.len() < needle.len() {
        return 0;
    }
    hay.windows(needle.len()).filter(|w| *w == needle).count()
}

/// 读 DLL 字节, 辨认内嵌的客户端身份标签 (命中次数最多者; 无命中 = None)。
fn dll_profile_tag(path: &std::path::Path) -> Option<&'static str> {
    let data = std::fs::read(path).ok()?;
    let mut best: Option<(&'static str, usize)> = None;
    for t in PROFILE_TAGS {
        let n = count_subst(&data, t.as_bytes());
        if n > 0 && best.map_or(true, |(_, bn)| n > bn) {
            best = Some((t, n));
        }
    }
    best.map(|(t, _)| t)
}

/// 进程名可直接确定的客户端身份 (DNF.exe 一名多客 —— 返回 None, 交给目录内容/DLL 标签)。
fn client_kind_from_process(process: &str) -> Option<&'static str> {
    match process {
        "ARAD.exe" => Some("40JP"),
        "DNFACT4.exe" | "DNF ACT4.exe" => Some("ACT4"),
        "DNFACT5.exe" | "DNF ACT5.exe" => Some("ACT5"),
        _ => None,
    }
}

/// 一个候选采集 DLL (实际存在的才进列表)。
struct DllCand {
    name: &'static str,
    from_host: bool,
    /// 内嵌身份标签; None = 无标签 (旧构建/无法读取)
    tag: Option<&'static str>,
}

/// ★v24.24 注入决策 (纯函数, 可单测) —— 智能识别版, 语义见模块头注释。
/// 路线 (S1/S4) 只决定算法与预设档案, **不决定注入哪个 DLL** —— 可任意组合
/// "S4 客户端 + S1 预设"; 返回 `(dll 文件名, 是否取自宿主目录)`; None = 不注入。
fn pick_dll_to_inject(
    cands: &[DllCand],
    us_extend: bool,
    process_kind: Option<&str>,
    dir_has_new: bool,
    dir_old_tag: Option<&'static str>,
    route_legacy: bool,
) -> Option<(&'static str, bool)> {
    if us_extend || cands.is_empty() {
        return None;
    }
    // 客户端身份: 进程名直判最权威; DNF.exe 一名多客时借**单代**部署判 ——
    // 两代并存 = 部署歧义, 不拿任何一代的标签当身份 (否则误放入的 OLD.dll 会否决
    // 正确的新一代 DLL), 交给路线优先级排序。
    let kind = match process_kind {
        Some(k) => Some(k),
        None => match (dir_old_tag, dir_has_new) {
            (Some(t), false) => Some(t),
            (None, true) => Some("90CN"),
            _ => None,
        },
    };

    // 候选排序: 与路线同代优先 (稳定排序), 宿主目录兜底永远在最后
    let rank = |c: &DllCand| -> usize {
        let is_old = c.name.ends_with("_OLD.dll");
        (if c.from_host { 2 } else { 0 }) + usize::from(is_old != route_legacy)
    };
    let mut ordered: Vec<&DllCand> = cands.iter().collect();
    ordered.sort_by_key(|c| rank(c));

    /* ★v24.24 选择规则 (路线对 DLL 的影响归零):
     * ① 身份匹配者直接注入; ② 目录内无标签者 (部署真值, 日志提示未验证);
     * ③ 全场无冲突时才按排序取首 (含 kind=None 的平局 —— 此时路线顺序只是
     *    用户自己声明的口味, 且仅在"同目录两代无标签 DLL 并存"的退化场景生效);
     * ④ 有明确冲突 → 拒绝 (错版 hook 错地址; 也不许退回无标签候选 —— 宁可不注不可注错) */
    let mut dir_untagged: Option<(&'static str, bool)> = None;
    let mut any_untagged: Option<(&'static str, bool)> = None;
    let mut saw_conflict = false;
    let mut kindless_tags: Vec<&'static str> = Vec::new();
    for c in ordered {
        match (c.tag, kind) {
            (Some(t), Some(k)) if t == k => return Some((c.name, c.from_host)),
            (Some(_), Some(_)) => saw_conflict = true,
            (Some(t), None) => {
                // 身份不明 (DNF.exe 两代并存): 标签无处比对 —— 记下来, 互相矛盾就拒绝
                if !kindless_tags.contains(&t) {
                    kindless_tags.push(t);
                }
                if any_untagged.is_none() {
                    any_untagged = Some((c.name, c.from_host));
                }
            }
            _ => {
                if !c.from_host && dir_untagged.is_none() {
                    dir_untagged = Some((c.name, c.from_host));
                }
                if any_untagged.is_none() {
                    any_untagged = Some((c.name, c.from_host));
                }
            }
        }
    }
    if let Some(d) = dir_untagged {
        return Some(d); // 目录内无标签 = 部署真值优先 (宿主候选的冲突不连坐)
    }
    if saw_conflict {
        return None; // 身份明确的冲突: 宁可不注不可注错
    }
    if kindless_tags.len() > 1 {
        return None; // 两代并存且标签互相矛盾: 无法判定, 拒绝 (日志会指引只留一个)
    }
    any_untagged // 平局: 路线排序首 (唯一标签或全无标签)
}

/// ★v24.22 日志落盘: eprintln 在无控制台的 windows-subsystem GUI 里全部丢失,
/// 同步写 SorahkDFO_vib.log。
fn ilog(msg: &str) {
    eprintln!("[auto_inject] {}", msg);
    crate::vibration::vib_log(&format!("[auto_inject] {}", msg));
}

/// 启动自动注入服务线程(随宿主常驻)。
pub fn run(state: Arc<AppState>) {
    /* 测试开关: 自动化验证时防止真注入 */
    if std::env::var("SORAHK_NO_AUTO_INJECT").as_deref() == Ok("1") {
        ilog("SORAHK_NO_AUTO_INJECT=1 -> 本线程空转");
        return;
    }
    thread::Builder::new()
        .name("auto_inject".to_string())
        .spawn(move || loop {
            if state.should_exit.load(std::sync::atomic::Ordering::Relaxed) {
                break;
            }
            step_once(&state);
            thread::sleep(Duration::from_millis(POLL_MS));
        })
        .ok();
}

/// 状态: 已注入的客户端 PID 集合 (客户端退出/重启后按本轮扫描结果裁剪重注入)
static INJECTED_PIDS: std::sync::Mutex<Vec<u32>> = std::sync::Mutex::new(Vec::new());

/// ★v24.25 已打过【警告】的客户端 PID (身份冲突拒绝等) —— 每实例只警告一次,
/// 防止拒绝-重试循环以 ~40 行/分钟的速度刷 vib.log; 客户端退场后随 present 裁剪。
static WARNED_PIDS: std::sync::Mutex<Vec<u32>> = std::sync::Mutex::new(Vec::new());

/// ★v24.25 上一次注入失败的去重键 (pid, 错误摘要) —— 同因失败只记一条,
/// 错误变化 (如权限问题→路径问题) 或换客户端才再记。
static LAST_FAIL: std::sync::Mutex<Option<(u32, String)>> = std::sync::Mutex::new(None);

fn step_once(state: &Arc<AppState>) {
    let legacy = state
        .vib_legacy_client
        .load(std::sync::atomic::Ordering::Relaxed);
    let Ok(exe_self) = std::env::current_exe() else {
        return;
    };
    let host_dir = exe_self.parent().unwrap_or(std::path::Path::new("."));
    let host_dir_has_old = host_dir.join("DfoVibration_OLD.dll").exists();

    /* 本轮在场的匹配客户端 PID (无论注入成败 —— 用于裁剪退场条目) */
    let mut present: Vec<u32> = Vec::new();
    /* 本轮**成功**注入的 PID (失败的不记, 下一轮自动重试) */
    let mut ok_new: Vec<u32> = Vec::new();

    for c in CLIENTS {
        let Some(pid) = find_pid_by_name(c.process) else {
            continue;
        };
        present.push(pid);
        if INJECTED_PIDS
            .lock()
            .map(|v| v.contains(&pid))
            .unwrap_or(false)
        {
            continue; // 已经注入过这个进程实例
        }
        let client_dir = game_path_from_pid(pid)
            .ok()
            .and_then(|p| p.parent().map(|d| d.to_path_buf()));
        let dir = client_dir.as_deref();
        let us_extend = dir
            .map(|d| d.join(US_EXTEND_MARKER).exists())
            .unwrap_or(false);

        /* ★v24.23 候选 = 实际存在的 DLL 各自带字节身份标签 (宿主目录只兜底 OLD.dll:
         * 新一代文件名被 90US/90CN 共用, 宿主目录里的同名文件可能是别的客户端的) */
        let mut cands: Vec<DllCand> = Vec::new();
        for (name, from_host) in [
            ("DfoVibration_OLD.dll", false),
            ("DfoVibration.dll", false),
            ("DfoVibration_OLD.dll", true),
        ] {
            let p = if from_host {
                host_dir.join(name)
            } else {
                match dir {
                    Some(d) => d.join(name),
                    None => continue,
                }
            };
            if p.exists() {
                let tag = dll_profile_tag(&p);
                cands.push(DllCand { name, from_host, tag });
            }
        }
        let dir_has_new = dir
            .map(|d| d.join("DfoVibration.dll").exists())
            .unwrap_or(false);
        let dir_old_tag = dir
            .map(|d| d.join("DfoVibration_OLD.dll"))
            .filter(|p| p.exists())
            .and_then(|p| dll_profile_tag(&p));

        let Some((dll_name, from_host)) = pick_dll_to_inject(
            &cands,
            us_extend,
            client_kind_from_process(c.process),
            dir_has_new,
            dir_old_tag,
            legacy,
        ) else {
            /* 有候选但被拒 = 身份冲突 (自挂载/无部署时候选为空或 us_extend)。
             * ★v24.25 警告每实例只发一次 —— 拒绝的客户端每轮都会重试到这里,
             * 不节流会以 ~40 行/分钟刷 vib.log。 */
            if !us_extend && !cands.is_empty() {
                let already = WARNED_PIDS
                    .lock()
                    .map(|v| v.contains(&pid))
                    .unwrap_or(true);
                if !already {
                    let tags: Vec<String> = cands
                        .iter()
                        .map(|c| match c.tag {
                            Some(t) => format!("{}={}", c.name, t),
                            None => format!("{}=<无标签>", c.name),
                        })
                        .collect();
                    ilog(&format!(
                        "【警告】{}: 候选 DLL 身份与客户端不符, 已拒绝注入 (候选: {}) —— 请部署正确版本的采集 DLL",
                        c.process, tags.join(", ")
                    ));
                    if let Ok(mut v) = WARNED_PIDS.lock() {
                        v.push(pid);
                    }
                }
            }
            continue;
        };
        let dll_path = if from_host {
            host_dir.join(dll_name)
        } else {
            match dir {
                Some(d) => d.join(dll_name),
                None => continue,
            }
        };
        let dll_path = dll_path.to_string_lossy().into_owned();

        // SAFETY: inject_process 只做 Win32 远程注入; 参数来自本进程上下文
        match unsafe { inject_process(pid, &dll_path) } {
            Ok(()) => {
                ok_new.push(pid);
                if let Ok(mut v) = LAST_FAIL.lock() {
                    if v.as_ref().map(|(p, _)| *p == pid).unwrap_or(false) {
                        *v = None;
                    }
                }
                let gen_label = if dll_name.ends_with("_OLD.dll") {
                    "老一代/ACT"
                } else {
                    "新一代/S4+"
                };
                ilog(&format!(
                    "{} pid={} injected {} (部署代次={})",
                    c.process, pid, dll_path, gen_label
                ));
                let chosen_untagged = cands
                    .iter()
                    .find(|c| c.name == dll_name && c.from_host == from_host)
                    .map(|c| c.tag.is_none())
                    .unwrap_or(false);
                if chosen_untagged {
                    ilog("【提示】该 DLL 未嵌身份标签, 无法验证客户端版本 —— 已按部署位置注入");
                }
                if dll_name.ends_with("_OLD.dll") != legacy {
                    ilog("【提示】注入的 DLL 代次与当前震动路线不一致 —— 震动会通, 但算法语义/参数档案可能不匹配; 建议在问号界面或「设置 → 客户端版本」切到对应路线 (ACT 系=S1 老版本, 90CN/90US=S4+ 新版本)");
                }
            }
            Err(e) => {
                // 客户端可能刚创建保护未就绪 / 权限不足, 下一轮重试。
                // ★v24.25 同因失败只记一条 (节流), 错误变化或换客户端才再记。
                let fresh = LAST_FAIL
                    .lock()
                    .map(|v| v.as_ref() != Some(&(pid, e.clone())))
                    .unwrap_or(true);
                if fresh {
                    ilog(&format!("inject failed pid={}: {} (将持续重试, 本条只记一次)", pid, e));
                    if let Ok(mut v) = LAST_FAIL.lock() {
                        *v = Some((pid, e));
                    }
                }
            }
        }
    }

    /* 裁剪退场 + 记录新成功 (失败的下一轮重试); 警告集合同步裁剪 */
    if let Ok(mut v) = INJECTED_PIDS.lock() {
        v.retain(|p| present.contains(p));
        for p in ok_new {
            if !v.contains(&p) {
                v.push(p);
            }
        }
    }
    if let Ok(mut v) = WARNED_PIDS.lock() {
        v.retain(|p| present.contains(p));
    }
}

fn find_pid_by_name(name: &str) -> Option<u32> {
    let target: Vec<u16> = OsString::from(name).encode_wide().collect();
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0).ok()?;
        let mut pe = PROCESSENTRY32W::default();
        pe.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut found = None;
        if Process32FirstW(snap, &mut pe).is_ok() {
            loop {
                if pe.szExeFile[..target.len()] == target[..] {
                    found = Some(pe.th32ProcessID);
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

/// 从进程 PID 拿可执行文件全路径(Win32 QueryFullProcessImageNameW 风格)。
fn game_path_from_pid(pid: u32) -> Result<std::path::PathBuf, String> {
    unsafe {
        let h = OpenProcess(
            windows::Win32::System::Threading::PROCESS_QUERY_LIMITED_INFORMATION,
            false,
            pid,
        )
        .map_err(|e| format!("OpenProcess: {}", e))?;
        let mut buf = vec![0u16; MAX_PATH as usize + 1];
        let mut size = buf.len() as u32;
        let ok = windows::Win32::System::Threading::QueryFullProcessImageNameW(
            h,
            windows::Win32::System::Threading::PROCESS_NAME_WIN32,
            windows::core::PWSTR(buf.as_mut_ptr()),
            &mut size,
        );
        let _ = CloseHandle(h);
        if ok.is_ok() {
            let s = String::from_utf16_lossy(&buf[..size as usize]);
            Ok(std::path::PathBuf::from(s))
        } else {
            Err("QueryFullProcessImageNameW".into())
        }
    }
}

/// 跨位注入: 把 dll_path 注入 pid 进程。
///
/// 64 -> 32 位注入的关键: 目标进程内 LoadLibraryA 的地址必须来自
/// 目标进程自身的 32 位 kernel32, 从导出表手工解析。
pub unsafe fn inject_process(pid: u32, dll_path: &str) -> Result<(), String> {
    // 1. 打开目标
    let h_target = OpenProcess(PROCESS_ALL_ACCESS, false, pid).map_err(|e| {
        if e.code() == windows::Win32::Foundation::E_ACCESSDENIED {
            "OpenProcess: 客户端以管理员运行中 —— 请右键【以管理员身份运行】本软件 (90CN 必读, 见使用说明)".to_string()
        } else {
            format!("OpenProcess: {}", e)
        }
    })?;

    // 2. 枚举目标进程的 32 位模块, 找 kernel32.dll 基址
    let k32_base: u64 = {
        let mut needed: u32 = 0;
        EnumProcessModulesEx(h_target, std::ptr::null_mut(), 0, &mut needed, LIST_MODULES_32BIT)
            .map_err(|e| format!("EnumProcessModulesEx size: {}", e))?;
        if needed == 0 {
            let _ = CloseHandle(h_target);
            return Err("no 32-bit modules".into());
        }
        let count = (needed / std::mem::size_of::<HMODULE>() as u32) as usize;
        let mut mods = vec![HMODULE::default(); count];
        EnumProcessModulesEx(
            h_target,
            mods.as_mut_ptr(),
            needed,
            &mut needed,
            LIST_MODULES_32BIT,
        )
        .map_err(|e| format!("EnumProcessModulesEx: {}", e))?;

        let mut base: u64 = 0;
        let mut name = [0u16; 260];
        for &m in &mods {
            let len = GetModuleBaseNameW(h_target, Some(m), &mut name);
            if len == 0 {
                continue;
            }
            let s = String::from_utf16_lossy(&name[..len as usize]);
            if s.eq_ignore_ascii_case("kernel32.dll") {
                base = m.0 as u64 & 0xFFFF_FFFF; // 32 位基址
                break;
            }
        }
        if base == 0 {
            let _ = CloseHandle(h_target);
            return Err("kernel32.dll not found among 32-bit modules".into());
        }
        base
    };

    // 3. 解析目标 kernel32 导出表 -> LoadLibraryA 的 32 位地址
    let load_library_a = parse_export_addr(h_target, k32_base, "LoadLibraryA")?;

    // 4. 写 DLL 路径到目标进程
    let mut path_bytes: Vec<u8> = dll_path.as_bytes().to_vec();
    path_bytes.push(0); // ANSI NUL
    let remote = VirtualAllocEx(
        h_target,
        None,
        path_bytes.len(),
        MEM_COMMIT | MEM_RESERVE,
        PAGE_READWRITE,
    );
    if remote.is_null() {
        let _ = CloseHandle(h_target);
        return Err("VirtualAllocEx failed".into());
    }
    let written = WriteProcessMemory(
        h_target,
        remote,
        path_bytes.as_ptr().cast(),
        path_bytes.len(),
        None,
    );
    if written.is_err() {
        let _ = VirtualFreeEx(h_target, remote, 0, MEM_RELEASE);
        let _ = CloseHandle(h_target);
        return Err("WriteProcessMemory failed".into());
    }

    // 5. 远程线程: 入口 = 目标进程内 32 位 LoadLibraryA
    //    LPTHREAD_START_ROUTINE = Option<unsafe extern "system" fn(*mut c_void) -> u32>
    let start_routine: unsafe extern "system" fn(*mut core::ffi::c_void) -> u32 =
        std::mem::transmute(load_library_a as usize);
    let tid = windows::Win32::System::Threading::CreateRemoteThread(
        h_target,
        None,
        0,
        Some(start_routine),
        Some(remote),
        0,
        None,
    );
    match tid {
        Ok(h_thread) => {
            // 等待注入完成(加载可能触发 DllMain 里的线程创建)
            windows::Win32::System::Threading::WaitForSingleObject(
                h_thread,
                10000,
            );
            let _ = CloseHandle(h_thread);
            // 清理远端缓冲(LoadLibraryA 已复制路径到内核)
            let _ = VirtualFreeEx(h_target, remote, 0, MEM_RELEASE);
            let _ = CloseHandle(h_target);
            Ok(())
        }
        Err(e) => {
            let _ = VirtualFreeEx(h_target, remote, 0, MEM_RELEASE);
            let _ = CloseHandle(h_target);
            Err(format!("CreateRemoteThread: {}", e))
        }
    }
}

/// 手动解析目标进程内 kernel32 的导出表, 返回指定名字的函数地址(32 位)。
unsafe fn parse_export_addr(
    h: HANDLE,
    module_base: u64,
    wanted: &str,
) -> Result<u64, String> {
    read_pe_export_addr(h, module_base, wanted).ok_or_else(|| {
        format!("export {} not found in kernel32 at {:#x}", wanted, module_base)
    })
}

/// 读目标进程 PE 导出表, 找函数 RVA -> 绝对地址。
unsafe fn read_pe_export_addr(h: HANDLE, base: u64, wanted: &str) -> Option<u64> {
    // DOS header: e_lfanew @ 0x3C
    let mut dos = [0u8; 0x40];
    read_mem(h, base, &mut dos)?;
    let e_lfanew = u32::from_le_bytes([dos[0x3C], dos[0x3D], dos[0x3E], dos[0x3F]]) as u64;

    // NT headers (32 位): Signature + FileHeader(20B) + OptionalHeader32
    // 导出目录 DataDirectory[0] 在 OptionalHeader32 偏移 0x60 (RVA) / 0x64 (Size)
    let nt = base + e_lfanew;
    let mut nt_region = [0u8; 0x88]; // 覆盖到 DataDirectory[0]
    read_mem(h, nt, &mut nt_region)?;
    let magic = u16::from_le_bytes([nt_region[0x18], nt_region[0x19]]);
    if magic != 0x010B {
        return None; // 不是 32 位 PE(预期, 游戏是 x86)
    }
    let export_rva = u32::from_le_bytes([
        nt_region[0x78],
        nt_region[0x79],
        nt_region[0x7A],
        nt_region[0x7B],
    ]) as u64;
    if export_rva == 0 {
        return None;
    }

    // IMAGE_EXPORT_DIRECTORY:
    //   NumberOfNames   @ 0x18
    //   AddressOfNames  @ 0x20
    //   AddressOfNameOrdinals @ 0x24
    //   AddressOfFunctions @ 0x1C
    let exp = base + export_rva;
    let mut exp_hdr = [0u8; 0x28];
    read_mem(h, exp, &mut exp_hdr)?;
    let number_of_names = u32::from_le_bytes([exp_hdr[0x18], exp_hdr[0x19], exp_hdr[0x1A], exp_hdr[0x1B]]);
    let addr_of_names = u32::from_le_bytes([exp_hdr[0x20], exp_hdr[0x21], exp_hdr[0x22], exp_hdr[0x23]]) as u64;
    let addr_of_ordinals = u32::from_le_bytes([exp_hdr[0x24], exp_hdr[0x25], exp_hdr[0x26], exp_hdr[0x27]]) as u64;
    let addr_of_functions = u32::from_le_bytes([exp_hdr[0x1C], exp_hdr[0x1D], exp_hdr[0x1E], exp_hdr[0x1F]]) as u64;

    for i in 0..number_of_names.min(4096) {
        // 名字 RVA 表
        let mut name_rva_u32 = [0u8; 4];
        read_mem_exact(h, base + addr_of_names + (i as u64) * 4, &mut name_rva_u32)?;
        let name_rva = u32::from_le_bytes(name_rva_u32) as u64;
        // 读名字字符串
        let mut name_buf = [0u8; 64];
        let got = read_mem_exact(h, base + name_rva, &mut name_buf)?;
        let mut name_len = 0;
        while name_len < got && name_buf[name_len] != 0 {
            name_len += 1;
        }
        if name_len == wanted.len()
            && name_buf[..name_len].eq_ignore_ascii_case(wanted.as_bytes())
        {
            // ordinal 表 -> 函数 RVA 表
            let mut ord_u32 = [0u8; 2];
            read_mem_exact(h, base + addr_of_ordinals + (i as u64) * 2, &mut ord_u32)?;
            let ordinal = u16::from_le_bytes(ord_u32) as u64;
            let mut fn_rva_u32 = [0u8; 4];
            read_mem_exact(h, base + addr_of_functions + ordinal * 4, &mut fn_rva_u32)?;
            let fn_rva = u32::from_le_bytes(fn_rva_u32) as u64;
            return Some(base + fn_rva);
        }
    }
    None
}

/// 读目标进程内存(自动适配长度), 返回读取字节数。
unsafe fn read_mem(h: HANDLE, addr: u64, buf: &mut [u8]) -> Option<usize> {
    let mut read: usize = 0;
    ReadProcessMemory(
        h,
        addr as *const core::ffi::c_void,
        buf.as_mut_ptr().cast(),
        buf.len(),
        Some(&mut read),
    )
    .ok()?;
    Some(read)
}

/// 读目标进程内存, 要求读满(按调用方意图)。
unsafe fn read_mem_exact(h: HANDLE, addr: u64, buf: &mut [u8]) -> Option<usize> {
    let n = read_mem(h, addr, buf)?;
    if n < buf.len() {
        // 页面边界处可能读少(名字字符串读 64B 时常见), 只保证非零
        if n == 0 {
            return None;
        }
    }
    Some(n)
}
/// UI 状态: 主程序目录是否已放置采集 DLL (未放置时自动改用游戏目录的)
pub fn host_dir_dll() -> bool {
    std::env::current_exe()
        .ok()
        .and_then(|p| {
            p.parent()
                .map(|d| d.join("DfoVibration_OLD.dll").exists())
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod decision_tests {
    use super::{DllCand, pick_dll_to_inject};

    fn cands(list: &[(&'static str, bool, Option<&'static str>)]) -> Vec<DllCand> {
        list.iter()
            .map(|(name, from_host, tag)| DllCand { name, from_host: *from_host, tag: *tag })
            .collect()
    }

    const OLD: &str = "DfoVibration_OLD.dll";
    const NEW: &str = "DfoVibration.dll";

    /// S-A 本机 90CN: 目录只有新一代 DLL, 路线 S1 (真实配置实测) —— 依然注入
    #[test]
    fn sa_90cn_new_dll_with_legacy_route_still_injects() {
        let c = cands(&[(NEW, false, Some("90CN"))]);
        assert_eq!(
            pick_dll_to_inject(&c, false, None, true, None, true),
            Some((NEW, false))
        );
    }

    /// S-B 远端 ACT4: 目录 OLD 标签 ACT4, 路线默认 S4+ —— 身份匹配, 注入
    #[test]
    fn sb_act4_old_dll_with_s4_route_still_injects() {
        let c = cands(&[(OLD, false, Some("ACT4"))]);
        assert_eq!(
            pick_dll_to_inject(&c, false, Some("ACT4"), false, None, false),
            Some((OLD, false))
        );
    }

    /// S-C 90US 自挂载: 永不注入
    #[test]
    fn sc_90us_self_mount_never_injects() {
        let c = cands(&[(OLD, false, Some("ACT1")), (NEW, false, Some("90US"))]);
        assert_eq!(pick_dll_to_inject(&c, true, None, true, None, true), None);
    }

    /// S-D ACT1 便携部署: 只有宿主目录 OLD (无标签) —— 兜底注入
    #[test]
    fn sd_act1_portable_host_dir_fallback() {
        let c = cands(&[(OLD, true, None)]);
        assert_eq!(
            pick_dll_to_inject(&c, false, None, false, None, true),
            Some((OLD, true))
        );
    }

    /// S-E ★v24.24 两代都在且各有标签:
    ///  - 客户端身份明确 (ACT4 进程): 身份匹配压过路线 —— 换路线也是注 ACT4 版 (解绑)
    ///  - 客户端身份不明 (DNF.exe): 部署歧义 → 拒绝 + 日志指引, 路线不裁决
    #[test]
    fn se_both_deployed_tagged() {
        let c = cands(&[(OLD, false, Some("ACT4")), (NEW, false, Some("90CN"))]);
        assert_eq!(
            pick_dll_to_inject(&c, false, Some("ACT4"), true, None, false),
            Some((OLD, false)),
            "身份匹配优先, 与路线无关"
        );
        assert_eq!(
            pick_dll_to_inject(&c, false, Some("ACT4"), true, None, true),
            Some((OLD, false))
        );
        // dir_old_tag=Some(ACT4) 与现实一致 (目录里那颗 OLD 的标签会传进来)
        assert_eq!(
            pick_dll_to_inject(&c, false, None, true, Some("ACT4"), true),
            None,
            "身份不明时路线不裁决"
        );
        assert_eq!(
            pick_dll_to_inject(&c, false, None, true, Some("ACT4"), false),
            None
        );
    }

    /// S-E2 退化平局: 同目录两代 DLL 都无标签 (无任何身份信息) —— 按路线排序取首
    /// (这是路线影响注入的唯一残留场景: 用户自己的口味当平局票)
    #[test]
    fn se2_both_untagged_route_tiebreak() {
        let c = cands(&[(OLD, false, None), (NEW, false, None)]);
        assert_eq!(
            pick_dll_to_inject(&c, false, None, true, None, true),
            Some((OLD, false))
        );
        assert_eq!(
            pick_dll_to_inject(&c, false, None, true, None, false),
            Some((NEW, false))
        );
    }

    /// S-E3 目录新一代无标签 + 宿主老代有标签: 目录部署真值胜出 (宿主冲突不连坐)
    #[test]
    fn se3_dir_untagged_beats_host_conflict() {
        let c = cands(&[(NEW, false, None), (OLD, true, Some("ACT1"))]);
        assert_eq!(
            pick_dll_to_inject(&c, false, None, true, None, false),
            Some((NEW, false))
        );
    }

    /// S-F 什么都没部署
    #[test]
    fn sf_nothing_deployed_no_injection() {
        assert_eq!(pick_dll_to_inject(&[], false, None, false, None, true), None);
    }

    /// T-A ★智能识别: ARAD(40JP) + 目录 DLL 标签 40JP → 匹配注入 (名字相同无需改名)
    #[test]
    fn ta_arad_tagged_40jp_matches() {
        let c = cands(&[(OLD, false, Some("40JP"))]);
        assert_eq!(
            pick_dll_to_inject(&c, false, Some("40JP"), false, None, true),
            Some((OLD, false))
        );
    }

    /// T-B ★身份冲突拒绝: ARAD(40JP) 但目录 DLL 是 ACT4 版 → 拒绝注入
    #[test]
    fn tb_profile_conflict_refuses_injection() {
        let c = cands(&[(OLD, false, Some("ACT4"))]);
        assert_eq!(
            pick_dll_to_inject(&c, false, Some("40JP"), false, None, true),
            None,
            "错版 DLL 会 hook 错地址, 必须拒绝"
        );
    }

    /// T-C ★无标签放行: ARAD + 目录 DLL 无标签 (旧构建) → 按部署注入 (日志提示未验证)
    #[test]
    fn tc_untagged_dll_falls_back_to_deployment() {
        let c = cands(&[(OLD, false, None)]);
        assert_eq!(
            pick_dll_to_inject(&c, false, Some("40JP"), false, None, true),
            Some((OLD, false))
        );
    }

    /// T-D ★DNF.exe 一名多客: 借目录 OLD 标签判身份 (ACT4) → 匹配注入
    #[test]
    fn td_dnf_exe_kind_from_dir_tag() {
        let c = cands(&[(OLD, false, Some("ACT4"))]);
        assert_eq!(
            pick_dll_to_inject(&c, false, None, false, Some("ACT4"), true),
            Some((OLD, false))
        );
    }

    /// T-E ★DNF.exe + 目录新一代标 90CN → 判 90CN, 匹配注入
    #[test]
    fn te_dnf_exe_new_dll_tagged_90cn() {
        let c = cands(&[(NEW, false, Some("90CN"))]);
        assert_eq!(
            pick_dll_to_inject(&c, false, None, true, None, false),
            Some((NEW, false))
        );
    }

    /// T-F ★90US 版误放进 90CN 目录: 身份冲突 → 拒绝
    #[test]
    fn tf_90us_dll_in_90cn_dir_refused() {
        let c = cands(&[(NEW, false, Some("90US"))]);
        assert_eq!(
            pick_dll_to_inject(&c, false, None, true, None, false),
            None,
            "90US 的 hook 地址在 90CN 里全错, 必须拒绝"
        );
    }

    /// T-G ★冲突 + 无标签兜底并存: 有冲突就不许退回无标签候选 (宁可不注, 不可注错)
    #[test]
    fn tg_conflict_blocks_untagged_fallback() {
        let c = cands(&[(OLD, false, Some("ACT1")), (OLD, true, None)]);
        assert_eq!(
            pick_dll_to_inject(&c, false, Some("40JP"), false, None, true),
            None
        );
    }
}
