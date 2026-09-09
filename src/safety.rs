//! 紧急保险: 全局热键 Ctrl+Alt+Shift+A 强制退出进程。
//!
//! 独立线程 + RegisterHotKey + 消息循环: 即使主线程/注入线程被连发风暴或
//! 循环事件拖住(系统键盘鼠标几乎无法动弹), 该线程仍能独立收到 WM_HOTKEY,
//! 立即调用 ExitProcess 终止整个进程, 让系统输入恢复可控。
//!
//! 为什么用 RegisterHotKey 而不是按键钩子:
//! - 钩子回调运行在被钩线程的上下文里, 主循环被拖住时同样无法执行;
//! - RegisterHotKey 由系统键盘驱动直接识别组合键, 与软件自身状态无关,
//!   且不会被软件自己 SendInput 注入的键盘事件触发(避免自触发), 
//!   只对用户的真实物理按键生效。

use std::thread;

use windows::Win32::System::Threading::ExitProcess;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, RegisterHotKey,
};
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GetMessageW, MSG, PM_NOREMOVE, PeekMessageW, TranslateMessage, WM_HOTKEY,
};

/// 启动紧急保险热键线程(注册失败时静默跳过, 不影响正常运行)。
pub fn start_emergency_hotkey() {
    let _ = thread::Builder::new()
        .name("emergency_hotkey".to_string())
        .spawn(emergency_hotkey_thread);
}

fn emergency_hotkey_thread() {
    const HOTKEY_ID: i32 = 0x5A5A; // 专用热键 ID
    const HOTKEY_VK: u16 = 0x41;   // 'A'

    unsafe {
        // 先创建本线程的消息队列(RegisterHotKey 的前置要求)
        let mut msg = MSG::default();
        let _ = PeekMessageW(&mut msg, None, 0, 0, PM_NOREMOVE);

        // 注册 Ctrl+Alt+Shift+A(MOD_NOREPEAT: 按住期间不重复触发)
        let result = RegisterHotKey(
            None,
            HOTKEY_ID,
            MOD_CONTROL | MOD_ALT | MOD_SHIFT | MOD_NOREPEAT,
            HOTKEY_VK as u32,
        );
        if result.is_err() {
            // 注册失败(被其它程序占用等)则不启用, 程序照常运行
            return;
        }

        // 消息循环: 收到 WM_HOTKEY 即强制退出
        loop {
            let ret = GetMessageW(&mut msg, None, 0, 0);
            if ret.0 == 0 || ret.0 == -1 {
                break; // WM_QUIT 或错误
            }

            if msg.message == WM_HOTKEY && (msg.wParam.0 as i32) == HOTKEY_ID {
                // 紧急退出: 不执行任何析构, 立即终止整个进程, 释放系统输入
                ExitProcess(0);
            }

            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    // 仅当线程被要求退出(WM_QUIT)时走到这里, 不影响进程主体
}