//! auto_inject — 免 Loader 自动挂载 DfoVibration_OLD.dll
//!
//! 设计目标(用户约束):
//!   * 文件名必须是 DfoVibration_OLD.dll(本名), 不借用 version.dll 等假名;
//!   * 不修改游戏的任何原始 DLL / EXE;
//!   * 不需要手动运行 DfoVibration_OLD_Loader.exe;
//!   * 游戏(DNF.exe)一启动, 采集 DLL 自动进入游戏进程 -> 事件流直达宿主。
//!
//! 原理: 本宿主是 64 位进程, 游戏是 32 位进程 -> 跨位远程注入:
//!   1. CreateToolhelp32Snapshot 轮询找 DNF.exe 的 PID;
//!   2. OpenProcess 打开目标;
//!   3. EnumProcessModulesEx(LIST_MODULES_32BIT) 枚举目标进程的 32 位模块,
//!      逐个 GetModuleBaseNameW 找 kernel32.dll 的 32 位基址;
//!   4. ReadProcessMemory 读该 kernel32 的 PE 导出表, 解析出 LoadLibraryA
//!      (32 位入口地址 —— 不能用本进程 GetProcAddress, 那是 64 位地址);
//!   5. VirtualAllocEx 在目标进程写 DLL 全路径;
//!   6. CreateRemoteThread(起始地址 = 目标进程内 32 位 LoadLibraryA, 参数 = 路径)
//!      -> DfoVibration_OLD.dll 的 DllMain(ATTACH) 自动启动 worker/collector
//!      (建共享内存 Local\DfoVibrationShm + 装 hook) -> 宿主照常读事件。
//!   7. 注入成功后记录 PID; 游戏重启(旧 PID 消失)后自动重注入。
//!
//! 线程周期 1500ms, 与宿主常驻生命周期一致; 失败静默重试(游戏可能还在启动中)。

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

/// 采集 DLL 的文件名(用户要求: 坚持本名, 不借名)
const COLLECT_DLL_NAME: &str = "DfoVibration_OLD.dll";
const POLL_MS: u64 = 1500;

/// 启动自动注入服务线程(随宿主常驻)。
pub fn run(state: Arc<AppState>) {
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

/// 状态: 上次已注入的 PID(游戏重启后归零重注入)
static LAST_INJECTED_PID: std::sync::atomic::AtomicU32 =
    std::sync::atomic::AtomicU32::new(0);

fn step_once(_state: &Arc<AppState>) {
    let Some(pid) = find_pid_by_name("DNF.exe") else {
        LAST_INJECTED_PID.store(0, std::sync::atomic::Ordering::Relaxed);
        return;
    };
    let last = LAST_INJECTED_PID.load(std::sync::atomic::Ordering::Relaxed);
    if pid == last {
        return; // 已经注入过这个进程实例
    }

    let Ok(dll_path) = locate_collect_dll() else {
        return; // 采集 DLL 不在已知位置, 静默等待(可能还没部署)
    };

    // SAFETY: inject_process 只做 Win32 远程注入; 参数来自本进程上下文
    match unsafe { inject_process(pid, &dll_path) } {
        Ok(()) => {
            LAST_INJECTED_PID.store(pid, std::sync::atomic::Ordering::Relaxed);
            eprintln!("[auto_inject] DNF.exe pid={} injected {}", pid, dll_path);
        }
        Err(e) => {
            // 游戏进程可能刚创建保护未就绪, 下一轮重试
            eprintln!("[auto_inject] inject failed pid={}: {}", pid, e);
        }
    }
}

/// 按进程名找 PID(wchar 全名匹配, 不区分大小写)。
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

/// 定位采集 DLL: 宿主 exe 同目录优先, 其次游戏(DNF.exe)同目录。
fn locate_collect_dll() -> Result<String, String> {
    let exe_self = std::env::current_exe()
        .map_err(|e| format!("current_exe: {}", e))?;
    let host_dir = exe_self.parent().unwrap_or(std::path::Path::new("."));
    let p1 = host_dir.join(COLLECT_DLL_NAME);
    if p1.exists() {
        return Ok(p1.to_string_lossy().into_owned());
    }
    // 游戏目录: 找 DNF.exe 所在目录
    if let Some(pid) = find_pid_by_name("DNF.exe") {
        if let Ok(path) = game_path_from_pid(pid) {
            if let Some(dir) = path.parent() {
                let p2 = dir.join(COLLECT_DLL_NAME);
                if p2.exists() {
                    return Ok(p2.to_string_lossy().into_owned());
                }
            }
        }
    }
    Err(format!("{} not found next to host or game", COLLECT_DLL_NAME))
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
    let h_target = OpenProcess(PROCESS_ALL_ACCESS, false, pid)
        .map_err(|e| format!("OpenProcess: {}", e))?;

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
                .map(|d| d.join(COLLECT_DLL_NAME).exists())
        })
        .unwrap_or(false)
}
