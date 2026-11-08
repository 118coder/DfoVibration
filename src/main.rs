// Hide console window in release mode
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod audio_haptics;
mod config;
mod gui;
mod hid_layout;
/* ★v24.19b 手柄隐藏暂时下线 —— 与 lib.rs 同步 (实现已移至 docs/frozen/hidhide.rs)。 */
// mod hidhide;
mod i18n;
mod input_manager;
mod input_ownership;
mod job_presets;
mod keyboard;
mod auto_inject;
mod mouse;
mod rawinput;
mod safety;
mod sequence;
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

/// `--headless-shots` 的实现: 读配置 → 批量渲染导出 → 打印清单后退出。
fn run_headless_shots(plan: &gui::headless_shots::ShotPlan) -> Result<()> {
    // 截图不需要真实用户配置里的路径记忆等瞬态; 缺文件时用默认值即可
    let config = AppConfig::load_or_create("Config.toml").unwrap_or_default();

    println!(
        "[headless] 输出目录: {} · 尺寸 {}x{} @{}x · 主题 {:?}",
        plan.opts.out_dir.display(),
        plan.opts.width,
        plan.opts.height,
        plan.opts.pixels_per_point,
        plan.themes
    );

    let shots = gui::headless_shots::run(plan, &config)?;
    for s in &shots {
        println!(
            "[headless] {} -> {} ({}x{})",
            s.name,
            s.path.display(),
            s.width,
            s.height
        );
    }
    println!("[headless] 完成, 共 {} 张", shots.len());
    Ok(())
}

fn main() -> Result<()> {
    setup_panic_hook();

    /* 无头截图模式: 在任何钩子/引擎/托盘起线程之前分流。
     * 这条路径完全不碰输入钩子、震动引擎与托盘, 只读配置 + 渲染 PNG,
     * 所以可在 CI / 无桌面的远程机器上跑。 */
    let argv: Vec<String> = std::env::args().skip(1).collect();
    if let Some(plan) = gui::headless_shots::ShotPlan::from_args(&argv) {
        return run_headless_shots(&plan);
    }

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

    // ★v24.50 音频震动引擎 (独立可选, 默认关; 关闭态空转零副作用)
    audio_haptics::spawn(app_state.clone());

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
