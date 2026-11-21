//! ★双 UI 壳 —— tao 窗口 + wry WebView2 界面壳 (契约 docs/双UI桥契约_v1.md)。
//!
//! 入口形态 (契约 §8, G 轮 v1.4 翻转):
//! * 默认 (无参) = **egui 原生界面** (gui/); 用户在 egui「外观 MOD」页应用 MOD →
//!   本模块在**第二线程**拉起 web 窗 (tao+wry) 接管界面、egui 隐入托盘
//!   (混合模式, 见 [`hybrid`] 与契约 §11);
//! * `--shell` / `SORAHK_SHELL=1` = 旧纯 web 壳模式 (开发/测试逃生门, 行为不变);
//! * `--shell-smoke` = 冒烟通道 (不变)。
//!
//! 会话模型 (G 轮重构): 一个 web 界面会话 = [`run_session`] —— tao EventLoop +
//! wry WebView + 快照泵 + IPC 分派, 自洽跑在一个线程上; 纯 shell 模式与混合模式的
//! mod 窗共用同一实现, 差异只在 [`SessionCfg`] (混合句柄 / 入口 URL / 窗口类名)。
//!
//! 传输 (契约 §2):
//! * Rust→JS: `evaluate_script("window.__hostPush(<json>)")` (事件驱动, 经泵/代理走
//!   事件循环线程 —— WebView 非 Clone, evaluate 不能跨线程, spike 实测);
//! * JS→Rust: `with_ipc_handler` (`window.wp.postMessage(str)`, init 脚本里
//!   `window.wp = window.ipc` 别名), 请求-应答带自增 id。
//!
//! 消息 schema 见契约 §3; cfg.write 白名单见 [`cfg_paths`] (附录 §A);
//! MOD 系统见 [`mods`]; 资源嵌入见 [`assets_table`]。

pub mod assets_table;
pub mod cfg_paths;
pub mod dvmod;
pub mod hybrid;
pub mod mods;
pub mod pump;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::{json, Value};
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy, EventLoopWindowTarget};
use tao::platform::run_return::EventLoopExtRunReturn;
use tao::platform::windows::{
    EventLoopBuilderExtWindows, WindowBuilderExtWindows, WindowExtWindows,
};
use tao::window::{ResizeDirection, WindowBuilder};
use windows::Win32::UI::Input::KeyboardAndMouse::ReleaseCapture;
use wry::WebViewBuilder;

use crate::backend::{Backend, BackendService};
use crate::ui_api::Commit;

/// 宿主经典壳入口 (原版 1:1 复刻)。
pub const CLASSIC_URL: &str = "sorahk://host/index.html";
/// 内置终末地包入口。
pub const ENDFIELD_URL: &str = "sorahk://mod/endfield/endfield.html";
/// 宿主页默认窗口尺寸 (契约 §2-§8: 1180×760, 与 egui 像素对齐)。
pub const WIN_SIZE: (f64, f64) = (1180.0, 760.0);
/// ★J 轮 (契约 v1.7 §11.5, 推翻 H 轮 1920×1080 决策): mod 窗默认尺寸 = **1180×760** ——
/// 与原生 egui 默认窗完全同尺寸 (用户要求「以原生 UI 窗口大小为基准重新设计」)。
/// endfield 前端设计画布同步改为 1180×760 (fitStage 宽度定标 s=1 一比一, 字号/密度
/// 按 1180 宽直接设计, 字体清晰可读); H 轮的「大窗字大」诉求由更大窗口等比放大保留。
/// 外部 shell 型 extmod 包同口径; 宿主页 (classic / overlay) 同为 1180×760
/// (分流逻辑保留 —— 按包语义仍在, 数值当前统一)。
/// 按显示器工作区 clamp (别超屏, 等比收缩, 见 [`clamp_size_to_work_area`])。
pub const MOD_WIN_SIZE: (f64, f64) = (1180.0, 760.0);
/// 最小窗口尺寸 (960×620; ★J 轮语义复核: endfield 1180 基准窗沿用同一 min ——
/// fitStage 宽度定标在窗口小于设计稿时仍等比缩小 (innerWidth/1180),
/// 更小的窗只是字更小, 无破版无黑框, min 不需要按包分流)。
const WIN_MIN: (f64, f64) = (960.0, 620.0);
/// 关闭确认无应答强退时限 (JS 弹退出确认并回执 win.close-opened 则撤销; 无回执兜底直退)。
/// 仅纯 shell 模式使用 (混合模式 mod 窗关闭 = 停用 MOD 还原 egui, 无需确认弹窗)。
const CLOSE_TIMEOUT_MS: u64 = 3000;
/// 关闭兜底线程轮询间隔 (close_watchdog_should_quit; 250ms × 12 = 3s)。
const CLOSE_POLL_MS: u64 = 250;

/// hello 契约字段 (v1.1 无边框窗口契约): 页面自绘标题栏是唯一标题栏。
pub const FRAMELESS: bool = true;
/// hello 契约字段 (v1.3): 拖动模式改默认 **"js"** 全量 JS 化 —— nc 在 WebView2
/// 真机/合成输入下拖动与边缘缩放体验不稳定 (用户实测痛点②), 改由页面显式热区
/// (bridge.js installWindowChrome 双壳通用安装器):
/// 标题栏 mousedown → win.drag、双击 → win.maximize、四边+四角 8px 热区 →
/// win.resize-begin{direction} (tao drag_resize_window 八方向, 事件循环线程调用)。
pub const DRAG_MODE: &str = "js";

/// 混合模式 mod 窗的 **唯一** Win32 窗口类名 (G 轮 spike 实测: winit (eframe 后端) 与
/// tao 的默认类名同为 "Window Class", 同进程双事件循环时 tao 建窗会命中 winit 已注册的
/// 类 → 消息进 winit 的 wndproc → 空指针秒崩 (0xC0000005/0xC000041D)。必须唯一化)。
pub const HYBRID_WINDOW_CLASS: &str = "DfoVibration ModWindow";

/// egui 主窗标题 (gui/mod.rs with_title 同源; mod 窗线程 Win32 还原 egui 时按它查找)。
pub const EGUI_WINDOW_TITLE: &str = "DfoVibration-V3 连发与映射工具";

/// 事件循环用户事件 (泵/IPC 线程 → 主线程; evaluate 只在主线程做)。
#[derive(Debug)]
pub enum ShellEv {
    /// 向 JS 推一条 `window.__hostPush(<json>)` (json 已序列化)。
    Push(String),
    /// IPC 原始报文 (worker 线程收到后转发)。
    Ipc(String),
    /// 导航 (shell.apply / 降级 fallback / hybrid 换 MOD)。
    Navigate(String),
    /// 托盘「显示主窗口」→ 还原聚焦 (JS 侧同时收到 ShowWindow evt)。
    WinShow,
    /// smoke: JS 桥回执 (`bridge.ready`) 到达, 真桥握手完成。
    SmokeOk,
    /// smoke: bridge.ready 超时 → 注入诊断探针 (经 IPC 回传页面状态)。
    SmokeTimeout,
    /// 请求优雅退出 (cmd quit / 托盘退出 / 关闭超时)。
    QuitRequested,
    /// ★G 轮混合模式: 关闭 mod 窗 (= 停用 MOD) —— 只退出本会话事件循环,
    /// **不**触发 backend.quit (进程由 egui 托管)。
    StopWindow,
    /// ★G 轮混合模式: overlay 型 MOD 的资源注入 (hello 后 evaluate `<link>/<script>`)。
    InjectScript(String),
}

/// 壳会话共享态 (泵 ↔ 事件循环)。
pub struct ShellShared {
    /// 当前壳: "classic" | "endfield" | 外部包 id。
    pub current: String,
    /// ★H 轮 (契约 v1.5): 是否混合模式 mod 窗 (hello 帧的 `hybrid` 字段由此上报,
    /// 前端据此决定「典/简/完整模式」按钮走 shell.stop{mode} 真切换还是演示 toast;
    /// 纯 shell 模式 / 浏览器演示 = false, 行为不变)。
    pub hybrid_mode: bool,
    /// 可应用清单 (内置 + 外部扫描)。
    pub mods: Vec<mods::ModMeta>,
    /// exe 目录 (外部包根 = exe_dir/mods; None = 无法定位)。
    pub ext_dir: Option<PathBuf>,
    /// 上次全量快照帧的 FNV-1a 哈希 (None = 下一拍推全量)。
    /// hello/换页时清零 → 新页面必收一帧全量快照 (契约 §3.1 v1.3)。
    pub last_snap: Option<u64>,
    /// ★v1.3 差量快照: 六域 (core/input/vibration/audio/update/counts) 各自的
    /// FNV-1a 哈希 (泵按域检测变化, 只推变化域 patch); hello/换页时随 last_snap
    /// 一起清零。域序 = [`pump::SNAP_DOMAINS`]。
    pub domain_hashes: [u64; 6],
    /// ★G 轮混合模式: overlay 型 MOD 待注入资源 (overlay 应用/切换时挂载, hello
    /// 推送后由泵/IPC 兜底路径取走并转 `ShellEv::InjectScript`; 消费即清零 ——
    /// 同页重 hello 不重复注入, 导航换页会重新挂载, 页面重建后注入不丢)。
    pub overlay_inject: Option<String>,
}

impl ShellShared {
    /// hello/换页: 清快照去重 → 泵下一拍必推一帧全量 (新页面缓存从零建立)。
    pub fn reset_snap_dedup(&mut self) {
        self.last_snap = None;
        self.domain_hashes = [0u64; 6];
    }
}

/// IPC 处理后的控制流指令。
enum Flow {
    Continue,
    /// 全进程退出 (cmd quit / 托盘退出 / 关闭超时): backend.quit + 事件循环退出。
    Quit,
    /// ★混合模式: 仅退出本会话 (停用 MOD / mod 窗被关) —— 不碰 backend。
    StopWindow,
}

/// web 界面会话配置 (纯 shell 模式与混合模式 mod 窗共用 [`run_session`])。
pub struct SessionCfg {
    /// 会话入口 URL (classic / mod / extmod)。
    pub initial_url: String,
    /// `ShellShared.current` 初值 (hello 帧的 shell/modId 由此推导)。
    pub current: String,
    /// ★H 轮 (契约 v1.5 §11.5) / ★J 轮改值 (v1.7): 会话默认窗口逻辑尺寸 (跟随入口包:
    /// host=1180×760 / endfield+extmod=1180×760 —— 与原生 egui 默认窗同尺寸;
    /// 建窗后按显示器工作区 clamp)。页内换包导航时同样按 [`nav_target_size`]
    /// 跟随 (未最大化才调整)。
    pub initial_size: (f64, f64),
    /// overlay 型 MOD 的待注入资源 URL (None = 无)。
    pub overlay_inject: Option<String>,
    /// smoke 通道标志 (握手判据/诊断流程)。
    pub smoke: bool,
    /// 混合模式句柄 (None = 纯 shell 模式; 关窗即全进程退出, 行为与 G 轮前一致)。
    pub hybrid: Option<Arc<hybrid::Hybrid>>,
    /// 混合模式: 主线程 → 本会话 的指令接收端 (导航/停用; 由泵外转发线程泵给事件循环)。
    pub nav_rx: Option<std::sync::mpsc::Receiver<hybrid::NavMsg>>,
    /// 窗口类名 (混合模式必须唯一 —— spike 结论; None = tao 默认 "Window Class")。
    pub window_classname: Option<&'static str>,
}

/// 默认入口 (纯 web 壳模式, `--shell` / `SORAHK_SHELL=1`): 装配窗口 + WebView + 泵,
/// 阻塞到退出。返回即进程应退出。行为与 G 轮前完全一致 (契约 §8)。
pub fn run(backend: Backend) -> anyhow::Result<()> {
    run_session(
        backend,
        SessionCfg {
            initial_url: CLASSIC_URL.to_string(),
            current: "classic".into(),
            initial_size: WIN_SIZE,
            overlay_inject: None,
            smoke: false,
            hybrid: None,
            nav_rx: None,
            window_classname: None,
        },
    )
}

/// `--shell-smoke` 入口 (契约 §8): 不起钩子/震动/音频/注入/托盘 (main.rs 分流保证),
/// 只装配 BackendService + 本壳, 15 秒内收到 JS 回执 `bridge.ready` → 打印 OK 行 → 退出。
pub fn run_smoke(backend: Backend) -> anyhow::Result<()> {
    run_session(
        backend,
        SessionCfg {
            initial_url: CLASSIC_URL.to_string(),
            current: "classic".into(),
            initial_size: WIN_SIZE,
            overlay_inject: None,
            smoke: true,
            hybrid: None,
            nav_rx: None,
            window_classname: None,
        },
    )
}

/// web 界面会话 (G 轮重构自原 run_impl): tao EventLoop + wry WebView + 泵 + IPC,
/// 阻塞到会话结束 (纯 shell: 关窗=进程退出; 混合: 关窗=停用 MOD 还原 egui)。
fn run_session(backend: Backend, cfg: SessionCfg) -> anyhow::Result<()> {
    let event_loop = EventLoopBuilder::<ShellEv>::with_user_event()
        .with_any_thread(true) // ★混合模式: mod 窗线程建第二事件循环 (tao 守卫要求; 主线程建无害)
        .build();
    let proxy = event_loop.create_proxy();

    let mut builder = WindowBuilder::new()
        .with_title("DfoVibration")
        .with_inner_size(tao::dpi::LogicalSize::new(cfg.initial_size.0, cfg.initial_size.1))
        .with_min_inner_size(tao::dpi::LogicalSize::new(WIN_MIN.0, WIN_MIN.1))
        .with_resizable(true)
        // 契约 v1.1 无边框窗口: 页面顶栏自带 ─□✕, 系统边框必须摘掉
        // (tao 保留 WS_SIZEBOX/WS_MAXIMIZEBOX → 边缘缩放/最大化/Win+贴边仍原生可用)。
        .with_decorations(false);
    // ★混合模式 (spike 结论): eframe 的 winit 与 tao 默认窗口类名同为 "Window Class",
    // 同进程双事件循环时 tao 建窗命中 winit 已注册类 → 消息进错 wndproc → 秒崩。
    // mod 窗必须用唯一类名。
    if let Some(cn) = cfg.window_classname {
        builder = builder.with_window_classname(cn);
    }
    let window = builder
        .build(&event_loop)
        .map_err(|e| anyhow::anyhow!("tao 窗口创建失败: {e}"))?;

    // ★H 轮 (契约 v1.5 §11.5): 默认尺寸按显示器工作区 clamp —— 超屏默认尺寸
    // (如外置包自定义更大画布) 在 2K/低分辨率屏 (或 >100% DPI) 上会超屏,
    // 等比收缩到工作区内 (别超屏); J 轮 1180×760 常规屏不触发, 兜底保留。
    if let Some(clamped) = clamp_size_to_work_area(&window, cfg.initial_size) {
        window.set_inner_size(tao::dpi::LogicalSize::new(clamped.0, clamped.1));
    }

    // MOD 清单: 内置 + 外部扫描 (启动时一次; mods.list 时重扫)
    let ext_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()));
    let mods_root = ext_dir.as_ref().map(|d| mods::mods_root_for_exe_dir(d));
    let mut all_mods = mods::builtin_mods();
    if let Some(root) = &mods_root {
        all_mods.extend(mods::scan_external(root));
    }
    let shared = Arc::new(Mutex::new(ShellShared {
        current: cfg.current.clone(),
        hybrid_mode: cfg.hybrid.is_some(),
        mods: all_mods,
        ext_dir,
        last_snap: None,
        domain_hashes: [0u64; 6],
        overlay_inject: cfg.overlay_inject.clone(),
    }));
    let mods_count = shared.lock().unwrap_or_else(|p| p.into_inner()).mods.len();

    // 混合模式: 主线程指令 (导航/停用) → 事件循环 的转发线程 (recv 阻塞唤醒, 通道
    // 关闭即线程退出)。必须在泵启动前接好, 否则 apply-while-active 的导航会丢。
    if cfg.hybrid.is_some() && let Some(nav_rx) = cfg.nav_rx {
        hybrid::spawn_nav_forwarder(shared.clone(), proxy.clone(), nav_rx);
    }

    // WebView (协议 handler 持 mods 根目录副本; IPC 走代理回主线程)
    let hello_pending = Arc::new(AtomicBool::new(true));
    let hello_sent = Arc::new(AtomicBool::new(false));
    let quit_flag = Arc::new(AtomicBool::new(false));
    // smoke 判据: JS 桥回执 (bridge.ready) 是否到达 (真桥 JS→Rust 全链路)。
    let bridge_ready = Arc::new(AtomicBool::new(false));
    // smoke 诊断: 超时后等页面探针回包。
    let diag_pending = Arc::new(AtomicBool::new(false));
    // smoke 诊断: 收到的 IPC 报文计数 (0 = WebView2 IPC 通道本身没通)。
    let ipc_count = Arc::new(AtomicUsize::new(0));
    // 退出确认弹窗回执 (契约 v1.2 §3.2 win.close-opened): 前端弹窗打开瞬间置位 →
    // 兜底强退线程轮询到后直接结束, 永不强退 (用户在弹窗里自由决策)。
    // 仅纯 shell 模式使用 (混合模式关窗 = 停用 MOD, 无确认弹窗)。
    let close_opened = Arc::new(AtomicBool::new(false));
    // ★G+2 混合模式: 我们主动发起的「回宿主」导航放行标记 (overlay 应用 → 宿主
    // index + 注入)。webview 自发起的回宿主导航 (⤶ 停用链接等) 无此标记 → 拦截停用。
    // 初值 = 入口页本身是宿主页 (overlay 会话首次加载, NavigationStarting 同样触发)。
    let self_nav = Arc::new(AtomicBool::new(is_back_to_host_nav(&cfg.initial_url)));

    let mut wv_builder = WebViewBuilder::new()
        .with_url(&cfg.initial_url)
        .with_custom_protocol("sorahk".into(), {
            let mods_root = mods_root.clone();
            move |_id, request| {
                let uri = request.uri().to_string();
                let r = assets_table::serve(&uri, mods_root.as_deref());
                if std::env::var("SORAHK_SHELL_DEBUG").is_ok() {
                    eprintln!("[shell-debug] asset: {uri} -> {}", r.status());
                }
                r
            }
        })
        .with_ipc_handler({
            let proxy = proxy.clone();
            let ipc_count = ipc_count.clone();
            move |request| {
                ipc_count.fetch_add(1, Ordering::Relaxed);
                let _ = proxy.send_event(ShellEv::Ipc(request.body().clone()));
            }
        });
    if cfg.hybrid.is_some() {
        // ★G+2 修复 (实机终验缺口, 契约 §11): mod 会话内 webview 自发起的「回宿主」
        // 导航 (⤶ 停用 MOD 链接 / location.href / 任何回宿主导航) = 停用 MOD ——
        // 取消页内导航 (SetCancel) + 关 mod 窗还原 egui。此前该路径漏接: <a href>
        // 不走 ShellEv::Navigate / shell.apply IPC, 宿主页被导航进 mod 窗滞留。
        // 纯 shell 模式不装 (关窗=进程退出, 页内切 classic 是旧行为)。
        wv_builder = wv_builder.with_navigation_handler({
            let proxy = proxy.clone();
            let self_nav = self_nav.clone();
            move |url| {
                let (allow, stop) = mod_nav_decision(&url, &self_nav);
                if stop {
                    let _ = proxy.send_event(ShellEv::StopWindow);
                }
                allow
            }
        });
    }
    let webview = wv_builder
        .with_initialization_script(init_script())
        .build(&window)
        .map_err(|e| anyhow::anyhow!("WebView2 创建失败: {e}"))?;

    if std::env::var("SORAHK_SHELL_DEVTOOLS").is_ok() {
        webview.open_devtools();
    }

    // 快照泵线程 (50ms 一拍; 混合模式 mod 窗 = ModWindow 角色)
    pump::Pump {
        backend: backend.clone(),
        proxy: proxy.clone(),
        shared: shared.clone(),
        hello_pending: hello_pending.clone(),
        hello_sent: hello_sent.clone(),
        quit: quit_flag.clone(),
        hid_waiting: Vec::new(),
        quit_reported: false,
        role: if cfg.hybrid.is_some() {
            pump::PumpRole::ModWindow
        } else {
            pump::PumpRole::Standalone
        },
    }
    .spawn();

    // smoke 硬超时 (判据 = JS 回执 bridge.ready; 15 秒未到 → 走诊断流程后非 0 退出)
    if cfg.smoke {
        let done = bridge_ready.clone();
        let hello_sent = hello_sent.clone();
        let ipc_count = ipc_count.clone();
        let proxy = proxy.clone();
        std::thread::spawn(move || {
            for _ in 0..60 {
                std::thread::sleep(std::time::Duration::from_millis(250));
                if done.load(Ordering::Relaxed) {
                    return;
                }
            }
            println!("[shell-smoke] FAIL: bridge.ready 超时 (15s), JS→Rust 桥链路未打通");
            println!(
                "[shell-smoke] diag hello_pushed={} ipc_msgs_received={}",
                hello_sent.load(Ordering::Relaxed),
                ipc_count.load(Ordering::Relaxed)
            );
            let _ = proxy.send_event(ShellEv::SmokeTimeout);
        });
    }

    // 主事件循环体 (纯 shell = run: Exit 即进程退出, 旧行为; 混合 mod 窗 = run_return:
    // Exit 后返回控制权 —— mod 窗关闭绝不 process::exit, 进程由 egui 托管)
    let backend_for_loop = backend.clone();
    let is_hybrid = cfg.hybrid.is_some();
    let event_handler =
        move |event: Event<'_, ShellEv>,
              _target: &EventLoopWindowTarget<ShellEv>,
              control_flow: &mut ControlFlow| {
        // 常规: 等事件 (泵每 50ms 会喂一帧)
        *control_flow = ControlFlow::Wait;
        match event {
            Event::UserEvent(ShellEv::Push(msg)) => {
                // ★v1.2 传输修复: bridge.js 的 __hostPush 期望「JSON 字符串」
                // (内部 JSON.parse)。这里必须把载荷包成 JS 字符串字面量再内插,
                // 否则实参是对象字面量, JSON.parse(对象) 抛错被 catch 静默吞掉
                // —— 整条 Rust→JS 通道 (snap/hello/evt/reply) 全部静默失效。
                // serde_json::to_string(&String) = 带转义的 JS 字符串字面量。
                let arg = serde_json::to_string(&msg).unwrap_or_else(|_| "\"\"".into());
                let js = format!("window.__hostPush({arg});");
                if let Err(e) = webview.evaluate_script(&js) {
                    // 页面未就绪时 evaluate 可能失败 —— 丢弃该帧 (泵下拍重推快照)
                    let _ = e;
                }
            }
            Event::UserEvent(ShellEv::Ipc(body)) => {
                // smoke 诊断探针回包 (bridge.ready 超时后注入的页面状态) → 打印后非 0 退出
                if diag_pending.load(Ordering::Relaxed) && body.contains("__smoke_diag") {
                    if let Ok(v) = serde_json::from_str::<Value>(&body) {
                        if v.get("t").and_then(Value::as_str) == Some("__smoke_diag") {
                            diag_pending.store(false, Ordering::Relaxed);
                            println!(
                                "[shell-smoke] diag title={:?} readyState={:?} url={:?}",
                                v.get("title").and_then(Value::as_str).unwrap_or(""),
                                v.get("ready").and_then(Value::as_str).unwrap_or(""),
                                v.get("url").and_then(Value::as_str).unwrap_or("")
                            );
                            println!(
                                "[shell-smoke] diag __SORAHK_BRIDGE__={} window.wp={} window.ipc={} __hostPush={} SorahkHost={}",
                                v.get("bridgeFlag").and_then(Value::as_bool).unwrap_or(false),
                                v.get("wp").and_then(Value::as_str).unwrap_or("?"),
                                v.get("ipc").and_then(Value::as_str).unwrap_or("?"),
                                v.get("hostPush").and_then(Value::as_str).unwrap_or("?"),
                                v.get("bridgeJs").and_then(Value::as_bool).unwrap_or(false)
                            );
                            println!(
                                "[shell-smoke] diag scripts={}",
                                v.get("scripts").and_then(Value::as_str).unwrap_or("")
                            );
                            std::process::exit(1);
                        }
                    }
                }
                match handle_ipc(
                    &backend_for_loop,
                    &shared,
                    &proxy,
                    &window,
                    &webview,
                    &hello_pending,
                    &hello_sent,
                    cfg.smoke,
                    &bridge_ready,
                    &close_opened,
                    cfg.hybrid.as_deref(),
                    &body,
                ) {
                    Flow::Quit => {
                        backend_for_loop.quit();
                        quit_flag.store(true, Ordering::Relaxed);
                        *control_flow = ControlFlow::Exit;
                    }
                    Flow::StopWindow => {
                        // 混合模式: 只关本窗 (停用 MOD), 后端会话与进程不动
                        quit_flag.store(true, Ordering::Relaxed);
                        *control_flow = ControlFlow::Exit;
                    }
                    Flow::Continue => {}
                }
            }
            Event::UserEvent(ShellEv::Navigate(url)) => {
                // ★G+2: 我们主动发起的回宿主导航 (overlay 应用 → 宿主 index + 注入)
                // 标记放行一次 —— 否则导航拦截器会把它误判为 webview 自发起的停用。
                if is_hybrid && is_back_to_host_nav(&url) {
                    self_nav.store(true, Ordering::Relaxed);
                }
                // ★H 轮 (契约 v1.5 §11.5): 窗口默认尺寸跟随当前加载的包 —— 页内换包
                // (endfield↔宿主/extmod) 时同步调整默认尺寸 (fitStage 基准变化)。
                // 最大化时不打扰用户几何; 用户手动改过尺寸仍会被重置为该包默认 ——
                // 换包本身就是明确的布局切换, 与重新开窗语义一致。
                let want = nav_target_size(&url);
                if !window.is_maximized() && want != cfg.initial_size {
                    let clamped = clamp_size_to_work_area(&window, want).unwrap_or(want);
                    window.set_inner_size(tao::dpi::LogicalSize::new(clamped.0, clamped.1));
                }
                // ★K 轮修复 (示例包全链路实测暴露): wry 0.57.3 只在**初始建窗**时把
                // 自定义协议 URL 改写成 http://sorahk.<host>/… (lib.rs Navigation 段),
                // 运行中 load_url 不做改写 → WebView2 对不认识的原生 scheme 导航静默
                // 失败 —— 页内换包 (shell.apply endfield/外部包 → Navigate) 从 G 轮起
                // 一直没真正导航成功 (此前验收只覆盖 --open-mod 初始加载与关窗停用
                // 路径)。这里统一补上 wry 的改写规则 (见 nav_url_for_load), 导航到
                // http://sorahk.* 形态 → WebView2 过滤器拦截 → wry 还原喂协议 handler。
                let nav_url = nav_url_for_load(&url);
                shell_debug(&format!("navigate: {url} -> {nav_url}"));
                let _ = webview.load_url(&nav_url);
                hello_pending.store(true, Ordering::Relaxed);
                hello_sent.store(false, Ordering::Relaxed);
            }
            Event::UserEvent(ShellEv::InjectScript(js)) => {
                // 混合模式: overlay 型 MOD 资源注入 (hello 后执行; 失败静默,
                // 重 hello 会重注入 —— overlay_inject 在 hello 推送路径已被取走)。
                let r = webview.evaluate_script(&js);
                shell_debug(&format!(
                    "inject: len={} ok={} head={}",
                    js.len(),
                    r.is_ok(),
                    &js.chars().take(40).collect::<String>()
                ));
            }
            Event::UserEvent(ShellEv::WinShow) => {
                // 托盘「显示主窗口」/ win.hide 之后恢复: 先还原最小化、
                // 恢复显示 (win.hide 隐藏的窗口 set_focus 不生效) 再聚焦。
                window.set_minimized(false);
                window.set_visible(true);
                window.set_focus();
            }
            Event::UserEvent(ShellEv::SmokeOk) => {
                println!(
                    "[shell-smoke] OK {} mods={} bridge=ready",
                    crate::update::APP_VERSION, mods_count
                );
                quit_flag.store(true, Ordering::Relaxed);
                *control_flow = ControlFlow::Exit;
            }
            Event::UserEvent(ShellEv::SmokeTimeout) => {
                // 注入页面探针: title/load 状态/桥对象存在性经 IPC 回传 (IPC 全断则 2.5s 兜底非 0 退出)
                diag_pending.store(true, Ordering::Relaxed);
                let _ = webview.evaluate_script(smoke_diag_script());
                let diag = diag_pending.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_millis(2500));
                    if diag.load(Ordering::Relaxed) {
                        println!("[shell-smoke] diag 页面探针无回包 (JS→Rust IPC 完全不可达)");
                        std::process::exit(1);
                    }
                });
            }
            Event::UserEvent(ShellEv::QuitRequested) => {
                backend_for_loop.quit();
                quit_flag.store(true, Ordering::Relaxed);
                *control_flow = ControlFlow::Exit;
            }
            Event::UserEvent(ShellEv::StopWindow) => {
                // 混合模式: 主线程请求停用 (egui「停用」按钮 → nav 转发线程)
                quit_flag.store(true, Ordering::Relaxed);
                *control_flow = ControlFlow::Exit;
            }
            Event::WindowEvent { event: WindowEvent::CloseRequested, .. } => {
                if cfg.hybrid.is_some() {
                    // ★混合模式: mod 窗关闭 = 停用 MOD 还原 egui (契约 §11)。
                    // 无需退出确认弹窗 (后端会话不受影响, 低风险操作),
                    // 直接退出会话; 会话收尾由 hybrid 线程包装层完成
                    // (finish_session + Win32 还原 egui)。
                    quit_flag.store(true, Ordering::Relaxed);
                    *control_flow = ControlFlow::Exit;
                } else {
                    // 契约 v1.2 §3.2 win.close-opened (修 3 秒强杀 bug):
                    // 推 ShellCloseRequested → 前端弹退出确认弹窗并立即回执
                    // win.close-opened → 兜底线程轮询到后直接结束 (永不强退, 用户在
                    // 弹窗里自由决策, 唯一出口 = 弹窗按钮: 退出=cmd quit / 托盘=win.hide
                    // / 取消=留在界面); CLOSE_TIMEOUT_MS 内没回执 (页面 JS 已死) →
                    // 维持强退兜底。每次关闭请求都是一轮新周期: 先清回执再推事件,
                    // 取消后再次关闭会重新走完整流程 (弹窗可反复打开)。
                    close_opened.store(false, Ordering::Relaxed);
                    // ★v1.2 传输修复: 与 Push 路径同构 —— 先序列化出 JSON 文本,
                    // 再把「文本作为字符串」二次序列化成带转义的 JS 字符串字面量。
                    // 直接序列化 json! 对象得到的是裸 JSON (对象字面量实参),
                    // bridge.js 的 JSON.parse(对象) 会抛错被静默吞掉。
                    let close_payload =
                        serde_json::json!({"t":"evt","ev":"ShellCloseRequested"}).to_string();
                    let close_arg = serde_json::to_string(&close_payload)
                        .unwrap_or_else(|_| "\"{}\"".into());
                    let _ = webview.evaluate_script(&format!(
                        "window.__hostPush({close_arg});"
                    ));
                    let flag = close_opened.clone();
                    let p = proxy.clone();
                    std::thread::spawn(move || {
                        let will_quit =
                            close_watchdog_should_quit(&flag, CLOSE_POLL_MS, CLOSE_TIMEOUT_MS);
                        if will_quit {
                            let _ = p.send_event(ShellEv::QuitRequested);
                        }
                    });
                }
            }
            Event::WindowEvent { event: WindowEvent::Destroyed, .. } => {
                quit_flag.store(true, Ordering::Relaxed);
                *control_flow = ControlFlow::Exit;
            }
            Event::LoopDestroyed => {
                quit_flag.store(true, Ordering::Relaxed);
                // 纯 shell 模式: 关窗 = 全进程退出 (既有语义)。
                // 混合模式: mod 窗关闭 ≠ 进程退出, 后端由 egui 托管, 不碰。
                if cfg.hybrid.is_none() {
                    backend_for_loop.quit();
                }
            }
            _ => {}
        }
    };

    if is_hybrid {
        // 混合模式: run_return —— ControlFlow::Exit 只退出本会话循环 (run_return 返回
        // 控制权; tao 的 run 在 Exit 后直接 process::exit, 混合模式禁用)。
        let mut el = event_loop;
        let _exit_code = el.run_return(event_handler);
    } else {
        // 纯 shell 模式: 旧行为 —— run 永不返回, Exit 即全进程退出。
        event_loop.run(event_handler);
    }
    Ok(())
}

/// 注入初始化脚本 (契约 §2: `__SORAHK_BRIDGE__` + `window.wp` + `__SHELL__`)。
fn init_script() -> String {
    format!(
        "window.__SORAHK_BRIDGE__ = true; \
         window.wp = window.ipc; \
         window.__SHELL__ = {{version: {:?}, shell: \"classic\"}};",
        crate::update::APP_VERSION
    )
}

/// smoke 诊断探针脚本 (bridge.ready 超时后注入; 经 IPC 把页面状态回传给宿主)。
fn smoke_diag_script() -> &'static str {
    r#"(function(){var d={t:"__smoke_diag",title:String(document.title),ready:String(document.readyState),url:String(location.href),bridgeFlag:!!window.__SORAHK_BRIDGE__,wp:typeof window.wp,ipc:typeof window.ipc,hostPush:typeof window.__hostPush,bridgeJs:!!window.SorahkHost,scripts:(function(){var a=[];var s=document.scripts;for(var i=0;i<s.length;i++){a.push(s[i].src||"inline")}return a.join("|")})()};try{window.wp.postMessage(JSON.stringify(d))}catch(e){}})()"#
}

/// 关闭兜底判定 (契约 v1.2 §3.2 win.close-opened, 兜底线程体; 可单测)。
/// 每 poll_ms 轮询一次 close_opened: 累计 timeout_ms 内置位 → false (弹窗已打开,
/// **永不强退**, 用户决策唯一出口是弹窗按钮); 超时仍未置位 → true (维持强退兜底,
/// 页面 JS 已死时的保命通道)。
fn close_watchdog_should_quit(close_opened: &AtomicBool, poll_ms: u64, timeout_ms: u64) -> bool {
    let polls = timeout_ms / poll_ms.max(1);
    for _ in 0..polls {
        std::thread::sleep(std::time::Duration::from_millis(poll_ms));
        if close_opened.load(Ordering::Relaxed) {
            return false;
        }
    }
    true
}

/// overlay 型 MOD 的资源注入脚本 (G 轮混合模式; 契约 §5 overlay 语义的壳侧实现)。
/// `.js` → `<script src>`, 其余按 CSS `<link rel=stylesheet>` 注入到 `<head>`;
/// 自执行 IIFE, 失败静默 (资源 404 由 sorahk://extmod 协议侧回 404, 不阻塞页面)。
pub fn overlay_inject_script(resource_url: &str) -> String {
    if resource_url.ends_with(".js") {
        format!(
            "(function(){{var s=document.createElement('script');s.src=\"{url}\";document.head.appendChild(s);}})()",
            url = resource_url
        )
    } else {
        format!(
            "(function(){{var l=document.createElement('link');l.rel='stylesheet';l.href=\"{url}\";document.head.appendChild(l);}})()",
            url = resource_url
        )
    }
}

/// 「回宿主」导航判定 (G+2 修复, 契约 §11; 可单测)。mod 会话内命中 = 停用 MOD:
/// * 绝对宿主: `sorahk://host/index.html` / 裸根 `sorahk://host/` (query/fragment 忽略);
/// * 内置终末地的相对别名: `sorahk://mod/endfield/index.html` —— 嵌入表把 endfield
///   包内 index.html 别名到宿主页 (endfield.html:59 停用链接 `href="index.html"`
///   相对解析的落点), 语义同为「回到原版」—— 实机缺口: 该链接曾把宿主页导航进
///   mod 窗而不还原 egui。
/// 外部包包内相对导航 (`sorahk://extmod/<id>/…`) 与 mod 页自身 (endfield.html)
/// 不算停用 —— 那是包内页面切换。
///
/// ★形态鲁棒 (wry custom_protocol_workaround.rs: WebView2 端把自定义 scheme 字符串
/// 改写为 `http(s)://sorahk.<authority>/…`, CDP 实测一致; NavigationStarting 回调
/// 拿到的 URI 就是改写形态) —— 先还原成 `sorahk://` 再匹配。
pub(crate) fn is_back_to_host_nav(url: &str) -> bool {
    let normalized = if let Some(rest) = url
        .strip_prefix("http://sorahk.")
        .or_else(|| url.strip_prefix("https://sorahk."))
    {
        format!("sorahk://{rest}")
    } else {
        url.to_string()
    };
    let base = normalized.split(['?', '#']).next().unwrap_or(&normalized);
    base == CLASSIC_URL
        || base == "sorahk://host/"
        || base == "sorahk://mod/endfield/index.html"
}

/// mod 会话导航拦截决策 (G+2 修复; 可单测)。返回 (放行导航, 触发停用):
/// * 非「回宿主」→ 放行 (mod 页自身 / 外部包包内 / 任意外链);
/// * 回宿主且**非**我们主动发起 (webview 自发起 = ⤶ 停用链接 / location.href) →
///   取消导航 + 停用 MOD (关 mod 窗 + Win32 还原 egui);
/// * 我们主动发起 (overlay 应用 → 宿主 index + 注入, `self_nav` 标记) → 放行一次
///   (标志消费, 下一次回宿主导航仍停用)。
fn mod_nav_decision(url: &str, self_nav: &AtomicBool) -> (bool, bool) {
    if !is_back_to_host_nav(url) {
        return (true, false);
    }
    if self_nav.swap(false, Ordering::Relaxed) {
        return (true, false);
    }
    (false, true)
}

// ═══════════════════════════════════════════════════════════════════════
// IPC 分派 (契约 §3.2)
// ═══════════════════════════════════════════════════════════════════════

/// 回一条 reply (Promise resolve 用)。
fn reply(proxy: &EventLoopProxy<ShellEv>, id: Value, ok: bool, message: Option<String>, data: Option<Value>) {
    let msg = json!({
        "t": "reply",
        "id": id,
        "ok": ok,
        "message": message,
        "data": data,
    });
    let _ = proxy.send_event(ShellEv::Push(msg.to_string()));
}

/// hello 推送 + overlay 注入跟随 (IPC 首包兜底 / bridge.ready 两条路径共用)。
/// hello = 新页面首帧: 清快照去重; 若挂了 overlay 待注入资源, 随后立即 evaluate 注入。
fn push_hello_and_overlay(
    shared: &Arc<Mutex<ShellShared>>,
    proxy: &EventLoopProxy<ShellEv>,
) {
    let _ = proxy.send_event(ShellEv::Push(pump::hello_json(shared).to_string()));
    let inject = shared
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .overlay_inject
        .take();
    if let Some(res) = inject {
        let _ = proxy.send_event(ShellEv::InjectScript(overlay_inject_script(&res)));
    }
}

/// 处理一条 IPC 报文 (已在事件循环线程)。
/// `window` 同线程可用 (WebView 非 Clone, 窗口操作不跨线程 —— A 轮 spike 结论)。
/// `hybrid` = Some 时走混合模式 shell.apply/stop 语义 (契约 §11)。
#[allow(clippy::too_many_arguments)]
fn handle_ipc(
    backend: &Backend,
    shared: &Arc<Mutex<ShellShared>>,
    proxy: &EventLoopProxy<ShellEv>,
    window: &tao::window::Window,
    _webview: &wry::WebView,
    hello_pending: &Arc<AtomicBool>,
    hello_sent: &Arc<AtomicBool>,
    smoke: bool,
    bridge_ready: &Arc<AtomicBool>,
    close_opened: &Arc<AtomicBool>,
    hybrid: Option<&hybrid::Hybrid>,
    body: &str,
) -> Flow {
    // 页面加载后第一帧兜底: JS 首个 IPC 到达而 hello 还没轮到泵推 → 先补 hello
    // (hello_json 内部会清快照去重, 新页面必收一帧全量 snap)
    if !hello_sent.swap(true, Ordering::Relaxed) {
        hello_pending.store(false, Ordering::Relaxed);
        push_hello_and_overlay(shared, proxy);
    }

    let Ok(v) = serde_json::from_str::<Value>(body) else {
        reply(proxy, Value::Null, false, Some("IPC 报文不是合法 JSON".into()), None);
        return Flow::Continue;
    };
    let t = v.get("t").and_then(Value::as_str).unwrap_or("").to_string();
    let id = v.get("id").cloned().unwrap_or(Value::Null);
    shell_debug(&format!("ipc: t={t} body={}", &body.chars().take(600).collect::<String>()));

    match t.as_str() {
        // ── bridge.ready (B 侧约定 ③): 页面脚本就绪, 补发 hello (幂等) ──
        // (hello_json 内部清快照去重 → 重推后保一帧全量 snap)
        "bridge.ready" => {
            push_hello_and_overlay(shared, proxy);
            hello_sent.store(true, Ordering::Relaxed);
            hello_pending.store(false, Ordering::Relaxed);
            // smoke 判据 (A 轮交接 §7.2 → 交付轮升级): JS 桥回执到达才算握手成功
            bridge_ready.store(true, Ordering::Relaxed);
            // ★H 轮临时诊断 (命中测试排查; SORAHK_SHELL_DEBUG=1 才启用): 2s 后向页面
            // 注入探针, 回报镶边安装态/舞台缩放态/视口/关键点 elementFromPoint 命中元素。
            if std::env::var("SORAHK_SHELL_DEBUG").is_ok() {
                let p = proxy.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_millis(2000));
                    const PROBE: &str = concat!(
    "window.wp && (function(){try{",
    "var d={t:'__hitprobe',",
    "chrome:window.__SORAHK_CHROME__||'',",
    "zones:document.querySelectorAll('[data-resize-dir]').length,",
    "zoom:(document.getElementById('stage-root')||{style:{}}).style.zoom||'',",
    "tf:(document.getElementById('stage-root')||{style:{}}).style.transform||'',",
    "iw:innerWidth,ih:innerHeight,",
    "p1:(function(){var e=document.elementFromPoint(231,9.6);return e?e.tagName+'.'+(e.className||''):'null'})(),",
    "p2:(function(){var e=document.elementFromPoint(231,30);return e?e.tagName+'.'+(e.className||''):'null'})(),",
    "p3:(function(){var e=document.elementFromPoint(231,55);return e?e.tagName+'.'+(e.className||''):'null'})(),",
    "pv:(function(){var i=document.querySelector('#mod-cards-dyn img');return i?(i.src.slice(-20)+' nw='+i.naturalWidth+' ok='+(i.complete&&i.naturalWidth>0)):'no-img'})(),",
    "iq:(function(){var r=document.createElement('img');r.src='sorahk://mod/endfield/preview.png';r.onload=function(){try{window.wp.postMessage(JSON.stringify({t:'__imgabs',ok:true,nw:r.naturalWidth}))}catch(e){}};r.onerror=function(){try{window.wp.postMessage(JSON.stringify({t:'__imgabs',ok:false}))}catch(e){}};document.body.appendChild(r);return 'go'})(),",
    "rq:(function(){var r2=document.createElement('img');r2.src='./preview.png';r2.onload=function(){try{window.wp.postMessage(JSON.stringify({t:'__imgrel',ok:true,nw:r2.naturalWidth}))}catch(e){}};r2.onerror=function(){try{window.wp.postMessage(JSON.stringify({t:'__imgrel',ok:false}))}catch(e){}};document.body.appendChild(r2);return 'go'})()",
    "};",
    "window.wp.postMessage(JSON.stringify(d));",
    "}catch(e){try{window.wp.postMessage(JSON.stringify({t:'__hitprobe_err',msg:String(e&&e.message||e)}))}catch(e2){}}})()"
                    );
                    let _ = p.send_event(ShellEv::InjectScript(PROBE.to_string()));
                });
            }
            // ★H 轮临时自动化 (模式切换验收; SORAHK_DEBUG_CLICK_MODE=minimal|classic|full
            // 时 8s 后程序化触发 endfield 顶栏模式 radio 的 change → shell.stop{mode} 全链路):
            if let Ok(m) = std::env::var("SORAHK_DEBUG_CLICK_MODE") {
                let p = proxy.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_millis(8000));
                    let js = format!(
                        concat!(
                            "(function(){{var v='{m}';",
                            "var r=document.querySelector('input[name=\"mode\"][value=\"'+v+'\"]')||document.querySelector('input[name=\"mode2\"][value=\"'+v+'\"]');",
                            "if(r){{r.checked=true;r.dispatchEvent(new Event('change',{{bubbles:true}}));}}}})()"
                        ),
                        m = m
                    );
                    let _ = p.send_event(ShellEv::InjectScript(js));
                });
            }
            if smoke {
                let _ = proxy.send_event(ShellEv::SmokeOk);
            }
        }

        // ── cmd (契约 §3.2: act 词表 → UiCommand) ────────────────────────
        "cmd" => {
            let act = v.get("act").and_then(Value::as_str).unwrap_or("").to_string();
            let norm = act.to_ascii_lowercase().replace('_', "-");
            match norm.as_str() {
                "quit" => return Flow::Quit,
                // shell 级扩展 (§7: HID 激活应答; 附录 §A 登记)
                "hid-activate" => {
                    let handle = v.pointer("/params/handle").and_then(Value::as_i64).unwrap_or(0) as isize;
                    let data: Vec<u8> = v
                        .pointer("/params/data")
                        .and_then(Value::as_array)
                        .map(|a| a.iter().filter_map(|x| x.as_u64().map(|n| n as u8)).collect())
                        .unwrap_or_default();
                    BackendService::activate_hid_device(handle, data);
                    reply(proxy, id, true, None, None);
                }
                "hid-cancel" => {
                    backend.clear_activating_device();
                    reply(proxy, id, true, None, None);
                }
                _ => {
                    if let Some(cmd) = build_command(&norm, v.get("params")) {
                        let r = backend.dispatch(cmd);
                        reply(proxy, id, r.ok, r.message, None);
                    } else {
                        reply(
                            proxy,
                            id,
                            false,
                            Some(format!("未知 act: {act} (词表见 MOD开发文档.md §3.2)")),
                            None,
                        );
                    }
                }
            }
        }

        // ── cfg.get: 全量配置 JSON (skip 字段 vibration/vibration_presets 手工补全) ──
        "cfg.get" => {
            let data = backend.with_config(|c| {
                let mut node = serde_json::to_value(c).unwrap_or(Value::Null);
                if let Some(obj) = node.as_object_mut() {
                    obj.insert(
                        "vibration".into(),
                        serde_json::to_value(&c.vibration).unwrap_or(Value::Null),
                    );
                    obj.insert(
                        "vibration_presets".into(),
                        serde_json::to_value(&c.vibration_presets).unwrap_or(Value::Null),
                    );
                }
                node
            });
            reply(proxy, id, true, None, Some(data));
        }

        // ── cfg.write (typed mutation 白名单; 契约 §4) ───────────────────
        "cfg.write" => handle_cfg_write(backend, proxy, id, &v),

        // ── cfg.paths: 白名单表动态发现 ─────────────────────────────────
        "cfg.paths" => {
            reply(proxy, id, true, None, Some(cfg_paths::paths_table()));
        }

        // ── mods.list: 重扫外部包 + 内置 ────────────────────────────────
        "mods.list" => {
            let list = {
                let mut s = shared.lock().unwrap_or_else(|p| p.into_inner());
                let mut all = mods::builtin_mods();
                if let Some(root) = s.ext_dir.as_ref().map(|d| mods::mods_root_for_exe_dir(d)) {
                    all.extend(mods::scan_external(&root));
                }
                s.mods = all.clone();
                all
            };
            reply(proxy, id, true, None, Some(json!(list)));
        }

        // ── ★K 轮 mods.import-dialog: 导入 .dvmod 单文件格式包 (契约 v1.8 §5) ──
        // 原生文件对话框在独立线程开 (模态循环不能卡事件循环), 文件字节不过 IPC ——
        // 对话框线程直接读盘/校验/安装, 完成后经 proxy 回 reply (send_event 线程安全)。
        // reply data = {"cancelled":true} | {"cancelled":false,"report":InstallReport}。
        "mods.import-dialog" => {
            let p = proxy.clone();
            let root = shared
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .ext_dir
                .as_ref()
                .map(|d| mods::mods_root_for_exe_dir(d));
            std::thread::spawn(move || {
                let Some(root) = root else {
                    reply(&p, id, false, Some("无法定位 exe 目录, .dvmod 导入不可用".into()), None);
                    return;
                };
                let picked = rfd::FileDialog::new()
                    .set_title("选择 .dvmod 外观 MOD 包")
                    .add_filter("DfoVibration MOD 包", &[dvmod::DVMOD_EXT])
                    .pick_file();
                let Some(path) = picked else {
                    reply(&p, id, true, None, Some(json!({ "cancelled": true })));
                    return;
                };
                match dvmod::install_from_file(&path, &root) {
                    Ok(report) => {
                        shell_debug(&format!(
                            "mods.import-dialog: id={} replaced={}",
                            report.id, report.replaced
                        ));
                        reply(
                            &p,
                            id,
                            true,
                            None,
                            Some(json!({ "cancelled": false, "report": report })),
                        );
                    }
                    Err(e) => reply(&p, id, false, Some(e), None),
                }
            });
        }

        // ── shell.apply / shell.stop (契约 §5; 混合模式语义见 §11) ──────
        "shell.apply" => {
            let mod_id = v.get("modId").and_then(Value::as_str).unwrap_or("classic").to_string();
            let f = match hybrid {
                Some(h) => hybrid_shell_apply(h, shared, proxy, id, &mod_id),
                None => shell_apply(shared, proxy, id, &mod_id),
            };
            // 混合模式 shell.apply classic = 停用 MOD → 关窗 (Flow::StopWindow)
            if !matches!(f, Flow::Continue) {
                return f;
            }
        }
        "shell.stop" => {
            // ★H 轮 (契约 v1.5 §11.6): 可带 mode ("full"|"minimal"|"classic", 缺省
            // full) —— mod 窗内「完整/极简/经典模式」与宿主页「典/简」入口: 停用 MOD
            // → mod 窗线程收尾 Win32 还原 egui → egui 帧头消费 pending 模式并
            // set_minimal/classic_mode (egui 运行时切换照抄标题栏按钮路径)。
            // 纯 shell 模式 (无 egui 可还原) 忽略 mode, 保持旧行为 (页内回宿主)。
            if let Some(h) = hybrid {
                let mode = v
                    .get("mode")
                    .and_then(Value::as_str)
                    .map(hybrid::parse_exit_mode)
                    .unwrap_or(hybrid::UiMode::Full);
                h.set_exit_mode(mode);
                shell_debug(&format!("shell.stop: mode={mode:?} (停用+切换)"));
                reply(
                    proxy,
                    id,
                    true,
                    None,
                    Some(json!({"type": "stop", "action": "close-window", "mode": exit_mode_tag(mode)})),
                );
                return Flow::StopWindow;
            }
            shell_apply(shared, proxy, id, "classic");
        }

        // ── 捕获流 (契约 §7; backend/capture.rs 既有入口) ───────────────
        "capture.start" => {
            let kind = v.get("kind").and_then(Value::as_str).unwrap_or("");
            match kind {
                "trigger" | "target" => {
                    backend.set_pad_capture(true);
                    reply(proxy, id, true, Some(format!("capture:{kind} 已进入 (Esc 取消)")), None);
                }
                other => {
                    reply(proxy, id, false, Some(format!("未知捕获类型: {other}")), None);
                }
            }
        }
        "capture.cancel" => {
            backend.set_pad_capture(false);
            backend.set_live_hid_pad(None);
            reply(proxy, id, true, None, None);
        }

        // ── win.* (契约 §3.2 + v1.1 无边框窗口契约) ─────────────────────
        // 页面自绘顶栏 ─□✕ → 窗口控制; 全部经事件循环线程 (窗口操作不跨线程)。
        "win.minimize" => {
            window.set_minimized(true);
            reply(proxy, id, true, None, None);
        }
        "win.maximize" => {
            let maxed = window.is_maximized();
            window.set_maximized(!maxed);
            reply(proxy, id, true, None, Some(json!({ "maximized": !maxed })));
        }
        "win.hide" => {
            // 隐藏到托盘 (退出确认弹窗「最小化到托盘」); 托盘线程不动。
            // 混合模式: mod 窗隐藏后由 egui 侧/pump 托盘 ShowWindow → WinShow 恢复。
            window.set_visible(false);
            reply(proxy, id, true, None, None);
        }
        "win.close-opened" => {
            // 契约 v1.2 §3.2: 退出确认弹窗已打开的回执 → 撤销强退兜底。
            // 置位后兜底线程轮询到即结束 (永不强退); 弹窗因任何原因关闭不发
            // 额外消息 (计时器已撤销, 无需恢复)。回 reply 仅为了让前端
            // Promise 落定 (不挂 pending)。
            close_opened.store(true, Ordering::Relaxed);
            reply(proxy, id, true, None, None);
        }
        "win.drag" => {
            // 契约: 无需 id、不回 reply (高频)。nc 模式下页面用 CSS
            // app-region: drag 原生拖动, 此消息是 js 模式/兜底通道。
            // ★H 轮修复 (用户实测: mod 窗标题栏真鼠标拖不动): mousedown 时 WebView2
            // 子窗口 (Chrome_RenderWidgetHostHWND) 持有鼠标捕获, HTCAPTION 模态移动
            // 循环期间捕获仍在 → 循环误判「按键已松」立即退出, 窗口纹丝不动。
            // 先 ReleaseCapture() (捕获是线程级的 —— webview 子窗与 tao 主窗同属事件
            // 循环线程, 直接释放) 再 drag_window; tao 0.37.3 内部也有一份
            // ReleaseCapture (双保险, 幂等)。Tauri 同款问题同款修法。
            let cap_released = unsafe { ReleaseCapture().is_ok() };
            let r = window.drag_window();
            shell_debug(&format!(
                "win.drag: capture_released={cap_released} drag_window={}",
                r.is_ok()
            ));
        }
        "win.resize-begin" => {
            // 边缘缩放 JS 兜底 (tao 0.37.3 drag_resize_window; 方向参数)。
            // ★H 轮: 热区放大到 12px (bridge.js DIRS, 用户实测 8px 太挑剔)。
            let dir = v.get("direction").and_then(Value::as_str).unwrap_or("");
            shell_debug(&format!("win.resize-begin: dir={dir}"));
            match parse_resize_direction(dir) {
                Some(d) => {
                    let r = window.drag_resize_window(d);
                    reply(
                        proxy,
                        id,
                        r.is_ok(),
                        r.err().map(|e| format!("drag_resize_window: {e}")),
                        None,
                    );
                }
                None => {
                    reply(
                        proxy,
                        id,
                        false,
                        Some(format!("未知缩放方向: {dir} (n/s/e/w/ne/nw/se/sw)")),
                        None,
                    );
                }
            }
        }

        other => {
            reply(proxy, id, false, Some(format!("未知消息类型 t={other}")), None);
        }
    }
    Flow::Continue
}

/// 带参命令: (kebab act, 变体名, 是否元组变体)。
/// B 侧约定 (交接 v12 §3): act = serde 变体名 (兼容 kebab 别名); 元组变体
/// params = {"value": …}; 结构变体 params = serde 字段对象。
const PARAM_COMMANDS: &[(&str, &str, bool)] = &[
    ("set-vibration-enabled", "SetVibrationEnabled", true),
    ("set-language", "SetLanguage", true),
    ("set-dark-mode", "SetDarkMode", true),
    ("set-always-on-top", "SetAlwaysOnTop", true),
    ("set-tray-enabled", "SetTrayEnabled", true),
    ("set-notifications-enabled", "SetNotificationsEnabled", true),
    ("restart-threads", "RestartThreads", true),
    ("delete-mapping", "DeleteMapping", true),
    ("set-paused", "SetPaused", true),
    ("set-mode", "SetMode", false),
    ("switch-vib-edition", "SwitchVibEdition", false),
    ("apply-mapping-preset", "ApplyMappingPreset", false),
    ("save-mapping-preset", "SaveMappingPreset", false),
    ("delete-mapping-preset", "DeleteMappingPreset", false),
    ("upsert-mapping", "UpsertMapping", false),
];

/// act + params → 带参 UiCommand (serde 反序列化; 外标签枚举 = {"Variant": params})。
fn build_command(act: &str, params: Option<&Value>) -> Option<crate::ui_api::UiCommand> {
    // ① 无参命令走 from_action (小写连字符词表)
    if let Some(cmd) = crate::ui_api::UiCommand::from_action(act) {
        return Some(cmd);
    }
    // ② 带参命令: 接受 serde 变体名或 kebab 别名
    let entry = PARAM_COMMANDS.iter().find(|(k, v, _)| {
        *v == act || *k == act.to_ascii_lowercase().replace('_', "-").as_str()
    })?;
    let &(kebab, variant, is_newtype) = entry;
    let _ = kebab;
    let raw = params.cloned().unwrap_or(Value::Null);
    let inner = if is_newtype {
        match raw {
            // 元组变体 {value: X} → X (B 侧约定); 兼容单字段对象直接给值 / 标量直给
            Value::Object(o) if o.contains_key("value") => o.get("value").cloned().unwrap_or(Value::Null),
            Value::Object(o) if o.len() == 1 => o.values().next().cloned().unwrap_or(Value::Null),
            other => other,
        }
    } else {
        raw
    };
    let payload = json!({ variant: inner });
    serde_json::from_value(payload).ok()
}

/// win.resize-begin 的方向参数 → tao [`ResizeDirection`]。
/// 接受缩写 n/s/e/w/ne/nw/se/sw 与全称/别名 (大小写不敏感, `_`/`-` 忽略)。
fn parse_resize_direction(s: &str) -> Option<ResizeDirection> {
    let norm = s.trim().to_ascii_lowercase().replace(['_', '-'], "");
    match norm.as_str() {
        "n" | "north" | "top" => Some(ResizeDirection::North),
        "s" | "south" | "bottom" => Some(ResizeDirection::South),
        "e" | "east" | "right" => Some(ResizeDirection::East),
        "w" | "west" | "left" => Some(ResizeDirection::West),
        "ne" | "northeast" | "topright" => Some(ResizeDirection::NorthEast),
        "nw" | "northwest" | "topleft" => Some(ResizeDirection::NorthWest),
        "se" | "southeast" | "bottomright" => Some(ResizeDirection::SouthEast),
        "sw" | "southwest" | "bottomleft" => Some(ResizeDirection::SouthWest),
        _ => None,
    }
}

// ═══════════════════════════════════════════════════════════════════════
// ★H 轮: 窗口尺寸按包分流 + 工作区 clamp + 诊断日志 (契约 v1.5 §11.5)
// ═══════════════════════════════════════════════════════════════════════

/// 会话入口包 → mod 窗默认逻辑尺寸 (可单测; ★J 轮两族统一 1180×760, 分流语义保留):
/// * 宿主页 (纯壳 classic / overlay 型回宿主) = 1180×760 (与 egui 像素对齐);
/// * shell 型包 (内置 endfield / 外部 extmod) = 1180×760 (与原生 egui 默认窗同尺寸;
///   endfield 设计画布 = 1180×760, s=1 一比一呈现)。
pub fn session_size_for(meta: &mods::ModMeta) -> (f64, f64) {
    if meta.kind == "overlay" {
        WIN_SIZE
    } else {
        MOD_WIN_SIZE
    }
}

/// 运行中导航 URL → WebView2 实际可加载形态 (★K 轮修复; 可单测):
/// `sorahk://X/…` → `http://sorahk.X/…` —— wry 0.57.3 只在初始建窗时做该改写
/// (lib.rs Navigation 段), 运行中 `load_url` 原样 Navigate → 原生 scheme 静默失败;
/// 改写成 workaround 形态后命中 WebView2 过滤器 (`http://sorahk.*`), wry 在
/// WebResourceRequested 里还原成 sorahk:// 喂协议 handler。其余 URL 原样放行。
pub(crate) fn nav_url_for_load(url: &str) -> String {
    if let Some(rest) = url.strip_prefix("sorahk://") {
        format!("http://sorahk.{rest}")
    } else {
        url.to_string()
    }
}

/// 导航目标 URL → 该包的默认窗口尺寸 (页内换包跟随; 可单测)。
/// `sorahk://mod/<id>/…` 与 `sorahk://extmod/<id>/…` = shell 型包 → 1180×760 (J 轮);
/// 其余 (sorahk://host/…) = 宿主页 → 1180×760。容错 wry workaround 改写形态
/// (`http(s)://sorahk.<authority>/…`)。
fn nav_target_size(url: &str) -> (f64, f64) {
    let normalized = if let Some(rest) = url
        .strip_prefix("http://sorahk.")
        .or_else(|| url.strip_prefix("https://sorahk."))
    {
        format!("sorahk://{rest}")
    } else {
        url.to_string()
    };
    let base = normalized.split(['?', '#']).next().unwrap_or(&normalized);
    if base.starts_with("sorahk://mod/") || base.starts_with("sorahk://extmod/") {
        MOD_WIN_SIZE
    } else {
        WIN_SIZE
    }
}

/// 等比收缩到可用矩形内 (纯函数, 可单测): want 超出 avail 时按两方向的最小比例
/// 缩小 (保持宽高比, fitStage 不破版); 未超出则原样返回 (只缩不放)。
fn fit_size_within(want: (f64, f64), avail: (f64, f64)) -> (f64, f64) {
    if want.0 <= avail.0 && want.1 <= avail.1 {
        return want;
    }
    let k = (avail.0 / want.0).min(avail.1 / want.1);
    (want.0 * k, want.1 * k)
}

/// 按窗口所在显示器工作区 clamp 逻辑尺寸 (默认值防超屏兜底; J 轮 1180×760 常规屏
/// 不触发, 保留给未来按包差异化的更大默认值):
/// MonitorFromWindow(NEAREST) → GetMonitorInfoW rcWork (物理像素) → 除以
/// scale_factor 得可用逻辑尺寸 → [`fit_size_within`]。无需调整 → None。
fn clamp_size_to_work_area(window: &tao::window::Window, logical: (f64, f64)) -> Option<(f64, f64)> {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST,
    };
    let scale = window.scale_factor();
    let mut mi = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    // tao WindowExtWindows::hwnd() 返回 isize → 包成 windows-rs HWND
    let hmon = unsafe { MonitorFromWindow(HWND(window.hwnd() as _), MONITOR_DEFAULTTONEAREST) };
    if unsafe { GetMonitorInfoW(hmon, &mut mi) }.as_bool() {
        let avail_w = (mi.rcWork.right - mi.rcWork.left) as f64 / scale;
        let avail_h = (mi.rcWork.bottom - mi.rcWork.top) as f64 / scale;
        let clamped = fit_size_within(logical, (avail_w, avail_h));
        if (clamped.0 - logical.0).abs() > f64::EPSILON
            || (clamped.1 - logical.1).abs() > f64::EPSILON
        {
            shell_debug(&format!(
                "clamp_size_to_work_area: {logical:?} → {clamped:?} (work {avail_w:.0}x{avail_h:.0} @{scale:.2}x)"
            ));
            return Some(clamped);
        }
    }
    None
}

/// shell 诊断日志 (SORAHK_SHELL_DEBUG=1 时输出 stderr; 默认静默零开销)。
/// H 轮拖动排查用: win.drag / win.resize-begin / shell.stop / clamp 的落地轨迹。
fn shell_debug(msg: &str) {
    if std::env::var("SORAHK_SHELL_DEBUG").is_ok() {
        eprintln!("[shell-debug] {msg}");
    }
}

/// UiMode → IPC reply 里的 tag 字符串 (与前端 shell.stop{mode} 入参词表一致)。
fn exit_mode_tag(mode: hybrid::UiMode) -> &'static str {
    match mode {
        hybrid::UiMode::Full => "full",
        hybrid::UiMode::Minimal => "minimal",
        hybrid::UiMode::Classic => "classic",
    }
}

/// cfg.write: 校验 → 按族走 backend 单一写路径 → 按档位落盘 → reply。
fn handle_cfg_write(
    backend: &Backend,
    proxy: &EventLoopProxy<ShellEv>,
    id: Value,
    v: &Value,
) {
    let msg_commit = v
        .get("commit")
        .and_then(Value::as_str)
        .and_then(cfg_paths::CommitTag::parse);
    let muts: Vec<cfg_paths::Mutation> = v
        .get("mutations")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|m| {
                    let path = m.get("path")?.as_str()?.to_string();
                    Some(cfg_paths::Mutation { path, value: m.get("value").cloned().unwrap_or(Value::Null) })
                })
                .collect()
        })
        .unwrap_or_default();

    let f = match cfg_paths::validate_split(&muts) {
        Ok(f) => f,
        Err(errs) => {
            reply(proxy, id, false, Some(errs.join("; ")), None);
            return;
        }
    };

    let mut errors: Vec<String> = Vec::new();

    // 震动族: config 写 + 引擎镜像 + 去抖落盘 (update_vibration_config 单一写路径)
    if !f.vib.is_empty() {
        let r = backend.update_vibration_config(|vib| cfg_paths::apply_vib(vib, &f.vib));
        match r {
            Ok(()) => {
                if matches!(
                    msg_commit,
                    Some(cfg_paths::CommitTag::Save)
                        | Some(cfg_paths::CommitTag::Vib)
                        | Some(cfg_paths::CommitTag::Reload)
                ) {
                    if let Err(e) = backend.update_config(Commit::SaveVib, |_| {}) {
                        errors.push(format!("vibration 落盘: {e}"));
                    }
                }
            }
            Err(list) => errors.extend(list),
        }
    }

    // 音频族: config 写 + apply_config 镜像 + 去抖落盘 (update_audio_config 单一写路径)
    if !f.audio.is_empty() {
        let r = backend.update_audio_config(|a| cfg_paths::apply_audio(a, &f.audio));
        match r {
            Ok(()) => {
                if matches!(
                    msg_commit,
                    Some(cfg_paths::CommitTag::Save)
                        | Some(cfg_paths::CommitTag::ConfigOnly)
                        | Some(cfg_paths::CommitTag::Reload)
                ) {
                    if let Err(e) = backend.update_config(Commit::SaveConfigOnly, |_| {}) {
                        errors.push(format!("audio 落盘: {e}"));
                    }
                }
            }
            Err(list) => errors.extend(list),
        }
    }

    // 更新族: 只写 Config.toml
    if !f.update.is_empty() {
        let level = commit_to_commit(msg_commit, Commit::SaveConfigOnly);
        let r = backend.update_config(level, |c| cfg_paths::apply_update(&mut c.update, &f.update));
        if let Err(e) = r {
            errors.push(format!("update: {e}"));
        }
    }

    // 设置族: 默认 SaveConfigOnly (契约 §4); 钉死 Reload 条目整批 Reload
    if !f.top.is_empty() {
        let level = cfg_paths::top_commit(f.top_force_reload, msg_commit);
        let r = backend.update_config(level, |c| cfg_paths::apply_top(c, &f.top));
        if let Err(e) = r {
            errors.push(format!("settings: {e}"));
        }
    }

    // 特殊路径: 专用后端入口 (附录 §A)
    for m in &f.special {
        let value = m.value.as_bool().unwrap_or(false);
        match m.path.as_str() {
            "dfo_player" => {
                if let Err(e) = backend.set_dfo_player(value) {
                    errors.push(format!("dfo_player: {e}"));
                }
            }
            "vib_legacy_client" => match backend.switch_vib_edition(value) {
                Ok(_) => {}
                Err(e) => errors.push(format!("vib_legacy_client: {e}")),
            },
            other => errors.push(format!("{other}: 特殊路径无处理分支")),
        }
    }

    if errors.is_empty() {
        reply(proxy, id, true, None, None);
    } else {
        reply(proxy, id, false, Some(errors.join("; ")), None);
    }
}

/// 消息 commit → Commit (族内缺省档位兜底)。
fn commit_to_commit(tag: Option<cfg_paths::CommitTag>, default: Commit) -> Commit {
    match tag {
        Some(cfg_paths::CommitTag::Memory) => Commit::Memory,
        Some(cfg_paths::CommitTag::Save) => Commit::Save,
        Some(cfg_paths::CommitTag::Vib) => default, // 对非震动族无意义, 回族默认
        Some(cfg_paths::CommitTag::Reload) => Commit::Reload,
        Some(cfg_paths::CommitTag::ConfigOnly) | None => default,
    }
}

/// shell.apply / shell.stop 共用 (纯 shell 模式): 解析 modId → 导航 (shell 型) /
/// 回资源 URL (overlay 型)。返回 Flow::Continue (纯 shell 模式应用不改变会话存续)。
fn shell_apply(
    shared: &Arc<Mutex<ShellShared>>,
    proxy: &EventLoopProxy<ShellEv>,
    id: Value,
    mod_id: &str,
) -> Flow {
    // classic = 宿主本体, 永远可用
    if mod_id == "classic" {
        shared.lock().unwrap_or_else(|p| p.into_inner()).current = "classic".into();
        let _ = proxy.send_event(ShellEv::Navigate(CLASSIC_URL.into()));
        reply(
            proxy,
            id,
            true,
            None,
            Some(json!({"type": "classic", "url": CLASSIC_URL})),
        );
        return Flow::Continue;
    }
    let s = shared.lock().unwrap_or_else(|p| p.into_inner());
    let Some(meta) = s.mods.iter().find(|m| m.id == mod_id).cloned() else {
        drop(s);
        reply(proxy, id, false, Some(format!("MOD 不存在: {mod_id}")), None);
        return Flow::Continue;
    };
    if !meta.compatible {
        let engine = meta.engine.clone();
        drop(s);
        reply(
            proxy,
            id,
            false,
            Some(format!("MOD 引擎不兼容 (声明 {engine}), 只可查看不可应用")),
            None,
        );
        return Flow::Continue;
    }
    if meta.kind == "overlay" {
        // overlay 型: 停留宿主, JS 注入资源 (契约 §5)
        let entry = meta.entry.clone().unwrap_or_default();
        let url = format!("sorahk://extmod/{}/{}", meta.id, entry);
        drop(s);
        shared.lock().unwrap_or_else(|p| p.into_inner()).current = meta.id.clone();
        reply(
            proxy,
            id,
            true,
            None,
            Some(json!({"type": "overlay", "resources": [url]})),
        );
    } else {
        // shell 型: 整页导航 (内置 endfield 走内嵌表; 外部走 extmod 服务)
        let url = if meta.builtin {
            ENDFIELD_URL.to_string()
        } else {
            let entry = meta.entry.clone().unwrap_or_else(|| "index.html".into());
            format!("sorahk://extmod/{}/{}", meta.id, entry)
        };
        drop(s);
        shared.lock().unwrap_or_else(|p| p.into_inner()).current = meta.id.clone();
        let _ = proxy.send_event(ShellEv::Navigate(url.clone()));
        reply(proxy, id, true, None, Some(json!({"type": "shell", "url": url})));
    }
    Flow::Continue
}

/// shell.apply (混合模式, 契约 §11): mod 窗内换 URL 不重开窗。
/// * "classic" → 停用 MOD = 关 mod 窗还原 egui (Flow::StopWindow);
/// * shell 型 → 导航到包入口;
/// * overlay 型 → 回宿主 index + hello 后注入资源 (ShellShared.overlay_inject)。
fn hybrid_shell_apply(
    hybrid: &hybrid::Hybrid,
    shared: &Arc<Mutex<ShellShared>>,
    proxy: &EventLoopProxy<ShellEv>,
    id: Value,
    mod_id: &str,
) -> Flow {
    if mod_id == "classic" {
        reply(
            proxy,
            id,
            true,
            None,
            Some(json!({"type": "classic", "action": "close-window"})),
        );
        return Flow::StopWindow;
    }
    let s = shared.lock().unwrap_or_else(|p| p.into_inner());
    let Some(meta) = s.mods.iter().find(|m| m.id == mod_id).cloned() else {
        drop(s);
        reply(proxy, id, false, Some(format!("MOD 不存在: {mod_id}")), None);
        return Flow::Continue;
    };
    if !meta.compatible {
        let engine = meta.engine.clone();
        drop(s);
        reply(
            proxy,
            id,
            false,
            Some(format!("MOD 引擎不兼容 (声明 {engine}), 只可查看不可应用")),
            None,
        );
        return Flow::Continue;
    }
    // 更新混合状态机的当前 mod (egui 状态徽章/托盘路由都读它) + 会话共享态
    // (hello 帧的 shell/modId 由此推导; 两条更新 = mod 窗内切换路径)
    hybrid.update_active_id(&meta.id);
    if meta.kind == "overlay" {
        // overlay 型: 回宿主页 + hello 后注入 (契约 §5/§11)
        let entry = meta.entry.clone().unwrap_or_default();
        let res = format!("sorahk://extmod/{}/{}", meta.id, entry);
        drop(s);
        let mut s2 = shared.lock().unwrap_or_else(|p| p.into_inner());
        s2.current = meta.id.clone();
        s2.overlay_inject = Some(res.clone());
        drop(s2);
        let _ = proxy.send_event(ShellEv::Navigate(CLASSIC_URL.into()));
        reply(
            proxy,
            id,
            true,
            None,
            Some(json!({"type": "overlay", "url": CLASSIC_URL, "resources": [res]})),
        );
    } else {
        let url = if meta.builtin {
            ENDFIELD_URL.to_string()
        } else {
            let entry = meta.entry.clone().unwrap_or_else(|| "index.html".into());
            format!("sorahk://extmod/{}/{}", meta.id, entry)
        };
        drop(s);
        let mut s2 = shared.lock().unwrap_or_else(|p| p.into_inner());
        s2.current = meta.id.clone();
        s2.overlay_inject = None;
        drop(s2);
        let _ = proxy.send_event(ShellEv::Navigate(url.clone()));
        reply(proxy, id, true, None, Some(json!({"type": "shell", "url": url})));
    }
    Flow::Continue
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui_api::UiCommand;

    /// B 侧传输约定 (交接 v12 §3): 带参 act = serde 变体名; 元组变体 params = {"value":…};
    /// 结构变体 params = serde 字段对象; 无参命令走 kebab 词表。
    #[test]
    fn param_command_forms() {
        // 变体名 + {value} 元组
        let c = build_command("SetVibrationEnabled", Some(&json!({"value": true}))).unwrap();
        assert!(matches!(c, UiCommand::SetVibrationEnabled(true)));
        // kebab 别名等价
        let c = build_command("set-dark-mode", Some(&json!({"value": true}))).unwrap();
        assert!(matches!(c, UiCommand::SetDarkMode(true)));
        // 新类型单字段对象 (B: SetLanguage{language})
        let c = build_command("SetLanguage", Some(&json!({"language": "English"}))).unwrap();
        assert!(matches!(c, UiCommand::SetLanguage(crate::i18n::Language::English)));
        // 结构变体 serde 字段对象
        let c = build_command("SetMode", Some(&json!({"minimal": true, "classic": false}))).unwrap();
        assert!(matches!(c, UiCommand::SetMode { minimal: Some(true), classic: Some(false) }));
        let c = build_command("SwitchVibEdition", Some(&json!({"legacy": true}))).unwrap();
        assert!(matches!(c, UiCommand::SwitchVibEdition { legacy: true }));
        let c = build_command("ApplyMappingPreset", Some(&json!({"name": "A"}))).unwrap();
        assert!(matches!(c, UiCommand::ApplyMappingPreset { .. }));
        let c = build_command(
            "UpsertMapping",
            Some(&json!({"index": null, "mapping": {"trigger_key": "A"}})),
        )
        .unwrap();
        assert!(matches!(c, UiCommand::UpsertMapping { index: None, .. }));
        // 元组索引 {value: 3}
        let c = build_command("DeleteMapping", Some(&json!({"value": 3}))).unwrap();
        assert!(matches!(c, UiCommand::DeleteMapping(3)));
        // 无参词表
        let c = build_command("toggle-pause", None).unwrap();
        assert!(matches!(c, UiCommand::TogglePause));
        // 未知 act
        assert!(build_command("no-such-act", None).is_none());
    }

    /// win.resize-begin 方向参数解析 (契约 v1.1; 8 方向 + 别名 + 非法值)。
    #[test]
    fn resize_direction_parsing() {
        for (s, want) in [
            ("n", ResizeDirection::North),
            ("s", ResizeDirection::South),
            ("e", ResizeDirection::East),
            ("w", ResizeDirection::West),
            ("ne", ResizeDirection::NorthEast),
            ("nw", ResizeDirection::NorthWest),
            ("se", ResizeDirection::SouthEast),
            ("sw", ResizeDirection::SouthWest),
            // 全称 / 别名 / 大小写 / 分隔符
            ("north", ResizeDirection::North),
            ("NorthEast", ResizeDirection::NorthEast),
            ("north_east", ResizeDirection::NorthEast),
            ("north-east", ResizeDirection::NorthEast),
            ("top-left", ResizeDirection::NorthWest),
            ("bottom_right", ResizeDirection::SouthEast),
        ] {
            assert_eq!(parse_resize_direction(s), Some(want), "input: {s}");
        }
        assert!(parse_resize_direction("").is_none());
        assert!(parse_resize_direction("x").is_none());
        assert!(parse_resize_direction("ns").is_none());
    }

    /// hello 首帧契约字段 (v1.1/v1.3/v1.5): frameless + dragMode + hybrid 必须如实上报;
    /// v1.3 起拖动/缩放全量 js 化 (nc 真机体验不稳, 见 DRAG_MODE 注释);
    /// v1.5 起 hybrid 标记混合模式 mod 窗 (前端模式按钮真切换的门控)。
    #[test]
    fn hello_contract_frameless_fields() {
        let shared = std::sync::Mutex::new(ShellShared {
            current: "classic".into(),
            hybrid_mode: false,
            mods: vec![],
            ext_dir: None,
            last_snap: None,
            domain_hashes: [0u64; 6],
            overlay_inject: None,
        });
        let h = pump::hello_json(&shared);
        assert_eq!(h.get("t").and_then(Value::as_str), Some("hello"));
        assert_eq!(h.get("frameless").and_then(Value::as_bool), Some(true));
        assert_eq!(h.get("dragMode").and_then(Value::as_str), Some("js"));
        assert_eq!(h.get("hybrid").and_then(Value::as_bool), Some(false));
        // 既有字段不回归
        assert_eq!(h.get("shell").and_then(Value::as_str), Some("classic"));
        // hello = 新页面首帧 → 清快照去重 (差量推送重置为全量)
        assert!(shared.lock().unwrap_or_else(|p| p.into_inner()).last_snap.is_none());
        assert_eq!(shared.lock().unwrap_or_else(|p| p.into_inner()).domain_hashes, [0u64; 6]);

        // 混合 mod 窗: hybrid=true 如实上报
        let hybrid_shared = std::sync::Mutex::new(ShellShared {
            current: "endfield".into(),
            hybrid_mode: true,
            mods: vec![],
            ext_dir: None,
            last_snap: None,
            domain_hashes: [0u64; 6],
            overlay_inject: None,
        });
        let h2 = pump::hello_json(&hybrid_shared);
        assert_eq!(h2.get("hybrid").and_then(Value::as_bool), Some(true));
        assert_eq!(h2.get("shell").and_then(Value::as_str), Some("mod"));
    }

    /// ★H 轮 (契约 v1.5 §11.5) 分流语义 + ★J 轮 (v1.7) 改值: 会话/导航目标尺寸按包分流 ——
    /// shell 型包 (内置 endfield / 外部 extmod) = 1180×760 (与原生 egui 默认窗同尺寸);
    /// 宿主页 (classic / overlay) = 1180×760 (egui 像素对齐)。两族数值当前统一,
    /// 分流函数保留 (未来按包差异化只改常量)。
    #[test]
    fn session_and_nav_size_routing() {
        // 内置 endfield (shell 型) → 1180×760
        let mut m = mods::builtin_mods().remove(0);
        assert_eq!(session_size_for(&m), MOD_WIN_SIZE);
        assert_eq!(MOD_WIN_SIZE, (1180.0, 760.0), "J 轮: 与原生 egui 默认窗同尺寸");
        assert_eq!(WIN_SIZE, (1180.0, 760.0));
        // 外部 shell 包 → 1180×760
        m.builtin = false;
        m.id = "mytheme".into();
        assert_eq!(session_size_for(&m), MOD_WIN_SIZE);
        // overlay 包 (宿主 index) → 1180×760
        m.kind = "overlay".into();
        assert_eq!(session_size_for(&m), WIN_SIZE);
        // 导航 URL 分流 (含 wry workaround 改写形态 + query/fragment)
        assert_eq!(nav_target_size(ENDFIELD_URL), MOD_WIN_SIZE);
        assert_eq!(nav_target_size("sorahk://extmod/mytheme/index.html"), MOD_WIN_SIZE);
        assert_eq!(nav_target_size("http://sorahk.mod/endfield/endfield.html"), MOD_WIN_SIZE);
        // wry workaround 改写形态保留完整 authority (sorahk.extmod, 同 is_back_to_host_nav)
        assert_eq!(nav_target_size("http://sorahk.extmod/mytheme/index.html#x"), MOD_WIN_SIZE);
        assert_eq!(nav_target_size(CLASSIC_URL), WIN_SIZE);
        assert_eq!(nav_target_size("sorahk://host/"), WIN_SIZE);
        assert_eq!(nav_target_size("http://sorahk.host/index.html?v=2"), WIN_SIZE);
    }

    /// ★K 轮: 运行中导航 URL 改写 (wry workaround 补齐) —— sorahk:// → http://sorahk.
    #[test]
    fn nav_url_for_load_workaround() {
        assert_eq!(
            nav_url_for_load("sorahk://host/index.html"),
            "http://sorahk.host/index.html"
        );
        assert_eq!(
            nav_url_for_load("sorahk://mod/endfield/endfield.html"),
            "http://sorahk.mod/endfield/endfield.html"
        );
        assert_eq!(
            nav_url_for_load("sorahk://extmod/example_theme/index.html"),
            "http://sorahk.extmod/example_theme/index.html"
        );
        // 非 sorahk scheme 原样 (外链/已改写形态)
        assert_eq!(
            nav_url_for_load("https://example.com/x"),
            "https://example.com/x"
        );
        assert_eq!(
            nav_url_for_load("http://sorahk.host/index.html"),
            "http://sorahk.host/index.html"
        );
    }

    /// ★H 轮: 等比收缩纯函数 —— 超屏按最小比例缩 (保持宽高比), 未超原样, 只缩不放。
    #[test]
    fn fit_size_within_math() {
        // 未超出 → 原样
        assert_eq!(fit_size_within((1920.0, 1080.0), (2560.0, 1400.0)), (1920.0, 1080.0));
        // 高超屏 (2K @125%: 工作区约 2048×1051 逻辑) → 等比缩
        let (w, h) = fit_size_within((1920.0, 1080.0), (2048.0, 1051.0));
        assert!((w - 1920.0 * 1051.0 / 1080.0).abs() < 0.01);
        assert!((h - 1051.0).abs() < 0.01);
        // 宽超屏 → 宽约束
        let (w, h) = fit_size_within((1920.0, 1080.0), (1600.0, 1200.0));
        assert!((w - 1600.0).abs() < 0.01 && (h - 900.0).abs() < 0.01);
        // 只缩不放
        assert_eq!(fit_size_within((1180.0, 760.0), (900.0, 700.0)).0, 900.0 * 1180.0 / 1180.0);
        let (w, h) = fit_size_within((1180.0, 760.0), (900.0, 700.0));
        assert!((h - 760.0 * 900.0 / 1180.0).abs() < 0.01);
        assert!(w <= 900.0 && h <= 700.0);
    }

    /// ★H 轮: shell.stop{mode} 参数解析 (大小写不敏感, 未知/缺省兜底 Full)。
    #[test]
    fn exit_mode_parsing() {
        assert_eq!(hybrid::parse_exit_mode("minimal"), hybrid::UiMode::Minimal);
        assert_eq!(hybrid::parse_exit_mode("Classic"), hybrid::UiMode::Classic);
        assert_eq!(hybrid::parse_exit_mode("full"), hybrid::UiMode::Full);
        assert_eq!(hybrid::parse_exit_mode(""), hybrid::UiMode::Full);
        assert_eq!(hybrid::parse_exit_mode("nonsense"), hybrid::UiMode::Full);
        // exit_mode_tag 与词表互逆 (reply data.mode 与前端入参一致)
        for (tag, mode) in [
            ("full", hybrid::UiMode::Full),
            ("minimal", hybrid::UiMode::Minimal),
            ("classic", hybrid::UiMode::Classic),
        ] {
            assert_eq!(exit_mode_tag(mode), tag);
            assert_eq!(hybrid::parse_exit_mode(tag), mode);
        }
    }

    /// 关闭兜底判定 (契约 v1.2 win.close-opened): 回执先到 → 永不强退;
    /// 全程无回执 → 超时强退; 窗口中途置位 (慢回执) → 轮询到仍不强退。
    #[test]
    fn close_watchdog_respects_close_opened() {
        // 回执先到 (弹窗已打开) → 不强退
        assert!(!close_watchdog_should_quit(&AtomicBool::new(true), 5, 20));
        // 全程无回执 (页面 JS 已死) → 超时判强退
        assert!(close_watchdog_should_quit(&AtomicBool::new(false), 5, 30));
        // 慢回执: 10ms 后置位, 50ms 窗口 → 轮询到 → 不强退 (修 3 秒强杀 bug 的核心语义)
        let late = Arc::new(AtomicBool::new(false));
        let late2 = late.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(10));
            late2.store(true, Ordering::Relaxed);
        });
        assert!(!close_watchdog_should_quit(&late, 5, 50));
    }

    /// G 轮: overlay 注入脚本形态 (.js → script; 其余 → link stylesheet)。
    #[test]
    fn overlay_inject_script_shapes() {
        let css = overlay_inject_script("sorahk://extmod/oldtheme/theme.css");
        assert!(css.contains("createElement('link')"), "css 必须用 link: {css}");
        assert!(css.contains("rel='stylesheet'"), "link 必须带 stylesheet: {css}");
        assert!(css.contains("sorahk://extmod/oldtheme/theme.css"));
        let js = overlay_inject_script("sorahk://extmod/oldtheme/boot.js");
        assert!(js.contains("createElement('script')"), "js 必须用 script: {js}");
        assert!(!js.contains("createElement('link')"));
        // 自执行 IIFE (evaluate 后立即生效)
        assert!(css.starts_with("(function(){"));
        assert!(js.starts_with("(function(){"));
    }

    /// G 轮: 启动模式解析 (契约 v1.4 §8 翻转: 默认 egui; --shell / SORAHK_SHELL=1 逃生门)。
    #[test]
    fn startup_mode_resolution() {
        use hybrid::StartupMode;
        let argv = |items: &[&str]| -> Vec<String> { items.iter().map(|s| s.to_string()).collect() };
        // 默认 (无参) → egui 原生
        assert_eq!(hybrid::startup_mode(&argv(&[]), None), StartupMode::NativeEgui);
        // --gui 保留, 等同默认
        assert_eq!(hybrid::startup_mode(&argv(&["--gui"]), None), StartupMode::NativeEgui);
        // 旧逃生门 SORAHK_SHELL=0: 默认已是 egui → 仍 NativeEgui (不再有翻转语义)
        assert_eq!(hybrid::startup_mode(&argv(&[]), Some("0")), StartupMode::NativeEgui);
        assert_eq!(hybrid::startup_mode(&argv(&[]), Some("false")), StartupMode::NativeEgui);
        assert_eq!(hybrid::startup_mode(&argv(&[]), Some("")), StartupMode::NativeEgui);
        // 新逃生门 SORAHK_SHELL=1/true → 纯 web 壳
        assert_eq!(hybrid::startup_mode(&argv(&[]), Some("1")), StartupMode::WebShell);
        assert_eq!(hybrid::startup_mode(&argv(&[]), Some("TRUE")), StartupMode::WebShell);
        assert_eq!(hybrid::startup_mode(&argv(&[]), Some("true")), StartupMode::WebShell);
        // --shell 旗标 → 纯 web 壳 (优先于环境变量)
        assert_eq!(hybrid::startup_mode(&argv(&["--shell"]), Some("0")), StartupMode::WebShell);
        // smoke 最高优先 (不起钩子的分流在装配前)
        assert_eq!(hybrid::startup_mode(&argv(&["--shell-smoke"]), Some("1")), StartupMode::ShellSmoke);
        assert_eq!(hybrid::startup_mode(&argv(&["--shell-smoke"]), None), StartupMode::ShellSmoke);
        assert_eq!(hybrid::startup_mode(&argv(&["--shell", "--gui"]), None), StartupMode::WebShell);
    }

    /// G+2 修复: mod 会话导航拦截 —— webview 自发起的「回宿主」导航 (⤶ 停用链接 /
    /// shell.apply classic / 任何回宿主导航) → 取消导航 + 停用 MOD; 我们主动发起的
    /// (overlay 应用, self_nav 标记) → 放行一次; 其余导航 (包内/资源) 放行。
    #[test]
    fn mod_session_nav_intercept() {
        // ⤶ 停用链接的相对解析落点 (endfield.html:59 href="index.html" →
        // sorahk://mod/endfield/index.html, 嵌入表别名到宿主页) → 停用
        let stop = AtomicBool::new(false);
        assert_eq!(
            mod_nav_decision("sorahk://mod/endfield/index.html", &stop),
            (false, true)
        );
        // 绝对宿主 (含裸根 / query / fragment)
        assert_eq!(mod_nav_decision(CLASSIC_URL, &stop), (false, true));
        assert_eq!(mod_nav_decision("sorahk://host/", &stop), (false, true));
        assert_eq!(
            mod_nav_decision("sorahk://host/index.html?x=1#a", &stop),
            (false, true)
        );
        // 我们主动发起 (overlay 应用): 放行一次, 标志消费后下一次仍停用
        let self_nav = AtomicBool::new(true);
        assert_eq!(mod_nav_decision(CLASSIC_URL, &self_nav), (true, false));
        assert!(!self_nav.load(Ordering::Relaxed), "放行标志必须被消费");
        assert_eq!(mod_nav_decision(CLASSIC_URL, &self_nav), (false, true));
        // 非回宿主导航一律放行 (mod 页自身 / 外部包包内相对 / 任意外链)
        assert_eq!(
            mod_nav_decision("sorahk://mod/endfield/endfield.html", &stop),
            (true, false)
        );
        assert_eq!(
            mod_nav_decision("sorahk://extmod/mytheme/index.html", &stop),
            (true, false)
        );
        assert_eq!(mod_nav_decision("https://example.com/x", &stop), (true, false));
        // 判定器形态覆盖 (资源类路径不算回宿主; ★WebView2 改写形态必须同样命中)
        assert!(is_back_to_host_nav("sorahk://host/index.html"));
        assert!(is_back_to_host_nav("sorahk://mod/endfield/index.html#frag"));
        assert!(!is_back_to_host_nav("sorahk://mod/endfield/endfield.html"));
        assert!(!is_back_to_host_nav("sorahk://extmod/a/index.html"));
        assert!(!is_back_to_host_nav("sorahk://host/assets/replica.css"));
        assert!(!is_back_to_host_nav(""));
        // wry workaround 改写形态 (NavigationStarting 回调实际收到的 URI)
        assert!(is_back_to_host_nav("http://sorahk.host/index.html"));
        assert!(is_back_to_host_nav("http://sorahk.mod/endfield/index.html"));
        assert!(is_back_to_host_nav(
            "http://sorahk.mod/endfield/index.html#x"
        ));
        assert!(!is_back_to_host_nav(
            "http://sorahk.mod/endfield/endfield.html"
        ));
        assert!(!is_back_to_host_nav("http://sorahk.ext/a/index.html"));
        // 同页 fragment 导航 (endfield 页顶栏锚点, 不得误判停用)
        assert_eq!(
            mod_nav_decision(
                "http://sorahk.mod/endfield/endfield.html#overview",
                &stop
            ),
            (true, false)
        );
    }

    /// G 轮: ModMeta → 会话入口 URL (混合模式 apply/换 MOD 共用)。
    #[test]
    fn mod_entry_url_shapes() {
        let mut m = mods::builtin_mods().remove(0);
        assert_eq!(hybrid::entry_url_for(&m), ENDFIELD_URL);
        // 外部 shell 包
        m.builtin = false;
        m.id = "mytheme".into();
        m.entry = Some("index.html".into());
        assert_eq!(
            hybrid::entry_url_for(&m),
            "sorahk://extmod/mytheme/index.html"
        );
        // overlay 包: 入口页 = 宿主 index, 资源 = extmod entry
        m.kind = "overlay".into();
        m.entry = Some("theme.css".into());
        assert_eq!(hybrid::entry_url_for(&m), CLASSIC_URL);
        assert_eq!(
            hybrid::overlay_resource_url(&m),
            Some("sorahk://extmod/mytheme/theme.css".to_string())
        );
    }
}
