// Hide console window in release mode
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod audio_haptics;
/// ★R2.14 前后端分离: 后端服务门面 (与 lib.rs 同步声明)
mod backend;
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
/// ★双 UI 壳 (契约 docs/双UI桥契约_v1.md): 内嵌 WebView2 界面壳 (与 lib.rs 同步声明)
mod shell;
mod signal;
mod state;
mod tray;
/// ★v24.55 检查更新 (与 lib.rs 同步声明)
mod update;
/// ★R2.14 前后端分离: UI↔后端 契约层 (与 lib.rs 同步声明)
mod ui_api;
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

    /* ★K 轮 --install-mod <路径.dvmod>: .dvmod 单文件格式包导入 CLI 通道 (契约 v1.8;
     * 自动化验收用)。不起钩子/震动/音频/注入/托盘/窗口, 逐包安装打印结果后退出,
     * 任一失败 → 非 0 退出码。 */
    let install_mods = shell::dvmod::parse_install_mod_args(&argv);
    if !install_mods.is_empty() {
        return shell::dvmod::run_install_mod(&install_mods);
    }

    /* ★双 UI 壳 --shell-smoke (契约 §8): 不起钩子/震动/音频/注入/托盘, 只装配
     * BackendService + shell 窗口, hello 握手 → 打印 [shell-smoke] OK → 退出。
     * 无人工干预的端到端验收通道, 必须在一切系统钩子之前分流。 */
    if shell::hybrid::startup_mode(&argv, std::env::var("SORAHK_SHELL").ok().as_deref())
        == shell::hybrid::StartupMode::ShellSmoke
    {
        let smoke_config = AppConfig::load_or_create("Config.toml").unwrap_or_default();
        let smoke_state = Arc::new(AppState::new(smoke_config.clone()).map_err(|e| {
            anyhow::anyhow!("shell-smoke AppState 初始化失败: {e}")
        })?);
        let smoke_backend = Arc::new(backend::BackendService::new(
            smoke_state,
            smoke_config,
            "Config.toml",
        ));
        return shell::run_smoke(smoke_backend);
    }

    signal::set_control_ctrl_handler()?;

    // 紧急保险键: Ctrl+Alt+Shift+A 强制退出。
    // 独立线程注册全局热键, 即使注入/连发失控导致键盘鼠标几乎无法操作,
    // 此热键仍能生效并立即终止进程(注册失败时静默跳过, 不影响正常运行)。
    safety::start_emergency_hotkey();

    // Load config or create default if not exists
    let mut config = match AppConfig::load_or_create("Config.toml") {
        Ok(cfg) => cfg,
        Err(e) => {
            let error_msg = format!("Failed to load configuration: {}", e);
            return show_error(&error_msg);
        }
    };

    /* ★R2.14: 「仅用连发」(dfo_player=false) → 震动关闭并落盘 (原 SorahkGui::new 的启动修正
     * 移到装配处、AppState 构造前 —— 保证运行态镜像的就是修正后的配置)。 */
    if !config.dfo_player && config.vibration.enabled {
        config.vibration.enabled = false;
        let _ = config.save_vibration_to_file(AppConfig::vibration_path_for("Config.toml"));
    }

    /* ★R2.12: 手改 Config.toml / 导入配置都绕过了 GUI 建键时的组合键校验,
     * 启动后补一次只读体检。有问题只写运行日志, 绝不改动或丢弃用户配置。 */
    for issue in crate::gui::gamepad_mapping::validate_all_chords(&config) {
        crate::vibration::vib_log(&format!("[chord] config-check | {issue}"));
    }

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

    /* ★R2.14 前后端分离: 后端服务门面 —— 配置权威/保存调度/命令分派全归它。
     * AppState/钩子/引擎的接线保持不变 (app_state 照旧共享)。 */
    let backend =
        std::sync::Arc::new(backend::BackendService::new(app_state.clone(), config, "Config.toml"));

    /* ★G 轮混合 UI 分流 (契约 v1.4 §8/§11, 默认翻转):
     * 默认 (无参) → egui 原生界面 (混合模式入口, 「外观 MOD」页按需拉 mod 窗);
     * --shell / SORAHK_SHELL=1 → 旧纯 web 壳 (开发/测试逃生门, 行为不变);
     * --gui / SORAHK_SHELL=0 → 保留接受, 语义 = 默认 (egui)。
     * --open-mod <id> → 启动自动应用外观 MOD (egui 路径; WebShell/Smoke 忽略)。
     * 既有装配一行未改。 */
    let open_mod = shell::hybrid::parse_open_mod(&argv);
    let gui_result = match shell::hybrid::startup_mode(
        &argv,
        std::env::var("SORAHK_SHELL").ok().as_deref(),
    ) {
        shell::hybrid::StartupMode::WebShell => {
            if open_mod.is_some() {
                crate::backend::BackendService::log(
                    "[hybrid] --open-mod 在 --shell 纯壳模式下被忽略 (MOD 切换走页内 shell.apply)",
                );
            }
            shell::run(backend)
        }
        // NativeEgui (含 --gui / SORAHK_SHELL=0 兼容): egui + 混合状态机
        shell::hybrid::StartupMode::NativeEgui => {
            let hybrid = Arc::new(shell::hybrid::Hybrid::new());
            if let Some(id) = &open_mod {
                crate::backend::BackendService::log(&format!(
                    "[hybrid] 启动参数 --open-mod {id} (帧循环跑稳后自动应用)"
                ));
            }
            let r = SorahkGui::run(backend.clone(), hybrid.clone(), open_mod);
            /* 退出语义 (契约 §11): egui 关闭 = 全进程退出 —— mod 窗线程收编
             * (pump 50ms 内见 should_exit → 会话退出; 这里 join 兜底收尾)。 */
            hybrid.join_mod_window();
            r
        }
        // ShellSmoke 已在钩子装配前分流, 正常不会走到 (兜底: 同 smoke)
        shell::hybrid::StartupMode::ShellSmoke => {
            anyhow::bail!("--shell-smoke 应在系统钩子装配前分流 (main.rs 前置块), 不应走到此处")
        }
    };

    gui_result
}
