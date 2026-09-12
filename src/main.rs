// Hide console window in release mode
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod config;
mod gui;
mod hid_layout;
mod i18n;
mod input_manager;
mod input_ownership;
mod job_presets;
mod keyboard;
mod auto_inject;
mod mouse;
mod rawinput;
mod safety;
mod signal;
mod state;
mod tray;
mod util;
mod vibration;
mod xinput;

use std::sync::Arc;
use std::thread;

use anyhow::Result;
use config::AppConfig;
use gui::{SorahkGui, show_error};
use input_manager::InputManager;
use keyboard::KeyboardHook;
use mouse::MouseHook;
use state::AppState;
use tray::TrayIcon;

/// 全局 panic hook: 任何线程的未捕获 panic 先写崩溃日志(SorahkDFO_crash.log),
/// 再按 panic = "unwind" 策略解栈 —— 进程绝不被单个 panic 直接杀掉。
fn setup_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        let loc = info
            .location()
            .map(|l| format!(" @ {}:{}:{}", l.file(), l.line(), l.column()))
            .unwrap_or_default();
        util::crash_log("PANIC", &format!("{}{loc}", info));
    }));
}

fn main() -> Result<()> {
    setup_panic_hook();

    signal::set_control_ctrl_handler()?;

    // 紧急保险键: Ctrl+Alt+Shift+A 强制退出。
    // 独立线程注册全局热键, 即使注入/连发失控导致键盘鼠标几乎无法操作,
    // 此热键仍能生效并立即终止进程(注册失败时静默跳过, 不影响正常运行)。
    safety::start_emergency_hotkey();

    // Load config or create default if not exists
    let config = match AppConfig::load_or_create("Config.toml") {
        Ok(cfg) => cfg,
        Err(e) => {
            let error_msg = format!("Failed to load configuration: {}", e);
            return show_error(&error_msg);
        }
    };

    let app_state = Arc::new(match AppState::new(config.clone()) {
        Ok(state) => state,
        Err(e) => {
            let error_msg = format!("Failed to initialize application state: {}", e);
            return show_error(&error_msg);
        }
    });

    // Start keyboard hook in a separate thread BEFORE GUI
    // Create hook INSIDE the thread to ensure proper message loop
    let keyboard_state = app_state.clone();
    util::spawn_supervised("keyboard_hook", move || {
        let _ = match KeyboardHook::new(keyboard_state.clone()) {
            Ok(hook) => hook.run_message_loop(),
            Err(e) => {
                util::crash_log("KEYBOARD_HOOK_INIT", &e.to_string());
                Err(e)
            }
        };
    });

    // Start mouse hook in a separate thread
    let mouse_state = app_state.clone();
    util::spawn_supervised("mouse_hook", move || {
        let _ = match MouseHook::new(mouse_state.clone()) {
            Ok(hook) => hook.run_message_loop(),
            Err(e) => {
                util::crash_log("MOUSE_HOOK_INIT", &e.to_string());
                Err(e)
            }
        };
    });

    // Start input manager
    let _input_manager = match InputManager::new(
        app_state.clone(),
        config.hid_baselines.clone(),
        config.device_api_preferences.clone(),
    ) {
        Ok(manager) => manager,
        Err(e) => {
            let error_msg = format!("Failed to initialize input manager: {}", e);
            return show_error(&error_msg);
        }
    };

    // Start vibration engine (reads DFO battle events via shared memory)
    vibration::run(app_state.clone());

    // ★S1 ACT1 老方案: 免 Loader 自动注入线程 (路线关闭时线程待机不注入;
    // 检测到 DNF.exe 且 DLL 就位才注入, 普通连发玩家零影响)
    auto_inject::run(app_state.clone());

    // Give hooks and input managers time to initialize
    thread::sleep(std::time::Duration::from_millis(200));

    // Start tray icon if enabled
    if app_state.show_tray_icon() {
        let tray_state = app_state.clone();
        util::spawn_supervised("tray", move || {
            match TrayIcon::new(tray_state.should_exit.clone()) {
                Ok(mut tray) => {
                    let language = tray_state.language();
                    let translations = crate::i18n::CachedTranslations::new(language);
                    let msg = translations.tray_notification_launched().to_string();
                    let _ = tray.show_info(&msg);
                    let _ = tray.run_message_loop();
                }
                Err(e) => {
                    util::crash_log("TRAY_INIT", &e.to_string());
                }
            }
        });
    }

    let gui_result = SorahkGui::run(app_state.clone(), config);

    gui_result
}
