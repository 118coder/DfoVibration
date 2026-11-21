//! ★G 轮混合 UI 架构 (契约 v1.4 §11): egui 主线程 + mod 窗线程。
//!
//! 窗口拓扑:
//! ```text
//! main 线程: eframe/egui 窗 (经典原生 UI) —— 默认唯一窗口
//!   └─ egui「外观 MOD」页 应用 MOD → Hybrid::apply(mod_id):
//!       ① egui 窗隐藏 (ViewportCommand::Visible(false), 进程与后端照常跑)
//!       ② spawn「mod 窗线程」: 自建 tao EventLoop + wry WebView
//!          (run_mod_session → shell::run_session), 加载 MOD 入口页
//!   └─ mod 窗内「⤶ 停用」/关窗/shell.apply classic → 会话退出 →
//!       ③ finish_session + Win32 直连还原 egui (spike 实测: egui 隐藏期
//!          update 停摆, ViewportCommand 不可依赖, 必须走 FindWindowW+ShowWindow)
//! ```
//!
//! 线程模型 / 状态机:
//! * [`Hybrid`] (Arc) = 两线程共享的唯一状态: `active: Option<ActiveMod>` +
//!   主线程→mod 窗线程 的指令通道 (NavMsg) + 报告暂存 (StopReport)。
//! * 应用时已有 mod 窗 = `NavMsg` 换 URL (不重开线程); 停用 = `NavMsg::Stop`。
//! * mod 窗线程带 catch_unwind 看护: 崩溃 → 自动停用 MOD + Win32 还原 egui +
//!   托盘通知条 (任务单指定语义; 不做 spawn_supervised 式重启 —— 窗口重启会
//!   叠加第二个 WebView2 会话, 停用是唯一安全出口)。
//!
//! 托盘路由: mod 活跃期 pump (ModWindow 角色) 消费 ShowWindow → 还原 mod 窗;
//! 非活跃期 egui 消费 → 还原 egui (见 shell/pump.rs 角色注释)。
//!
//! 退出语义: egui 关闭 = 全进程退出 (main 里 join mod 窗线程); mod 窗关闭 =
//! 仅停用 MOD; 托盘退出菜单 = backend.quit() 全退 (两个 UI 各自的
//! should_exit 轮询都会触发收尾)。

use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex, MutexGuard};

use tao::event_loop::EventLoopProxy;

use crate::backend::Backend;
use crate::shell::mods::ModMeta;

use super::{ShellEv, ShellShared, SessionCfg, CLASSIC_URL, ENDFIELD_URL, EGUI_WINDOW_TITLE, HYBRID_WINDOW_CLASS};

/// mod 窗会话结束原因 (StopReport.reason)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopReason {
    /// 用户关窗 / 会话正常退出 (停用 MOD, 还原 egui)。
    Closed,
    /// 会话线程 panic / 会话初始化失败 (停用 MOD + 还原 egui + 通知条)。
    Crashed,
    /// 全进程退出中 (backend.should_exit) —— 不还原 egui, 不通知。
    AppExiting,
}

/// ★H 轮 (契约 v1.5 §11.6): mod 窗内发起的「目标界面模式」。
/// 停用 MOD → 还原 egui → egui 帧头消费并照抄标题栏按钮路径
/// (set_minimal_mode / set_classic_mode) 切换 —— egui 运行时切换**不走**
/// UiCommand::SetMode (那只落盘 config, 不动 GUI 本地模式标记)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiMode {
    /// 完整界面 (清极简/经典标记; egui 已在完整态则无操作)。
    Full,
    /// 极简模式小窗。
    Minimal,
    /// 经典模式 (900×600 三页签)。
    Classic,
}

/// IPC shell.stop{mode} 字符串 → [`UiMode`] (可单测; 大小写不敏感, 未知值兜底 Full)。
pub fn parse_exit_mode(s: &str) -> UiMode {
    match s.trim().to_ascii_lowercase().as_str() {
        "minimal" => UiMode::Minimal,
        "classic" => UiMode::Classic,
        _ => UiMode::Full,
    }
}

/// mod 窗线程 → egui 主线程 的停用报告 (egui 帧头轮询 [`Hybrid::poll_stopped`])。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StopReport {
    /// 结束会话的 mod id。
    pub id: String,
    pub reason: StopReason,
}

/// [`Hybrid::apply`] 的高层语义结果 (egui 页面据此决定是否隐藏自己)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApplyOutcome {
    /// 已拉起新 mod 窗线程 (egui 应随即将自己隐藏到托盘)。
    WindowSpawned,
    /// 已有 mod 窗, 已下发换 URL / 回宿主注入 指令 (不重开线程)。
    Navigated,
}

/// 主线程 → mod 窗线程 指令 (nav 转发线程泵进事件循环)。
#[derive(Debug, Clone)]
pub enum NavMsg {
    /// shell 型换 MOD: 导航到入口 URL (不重开窗)。
    Shell { id: String, url: String },
    /// overlay 型换 MOD: 回宿主 index + hello 后注入资源。
    Overlay { id: String, resource: String },
    /// 停用 MOD: 关 mod 窗 → egui 还原。
    Stop,
}

/// 活跃 mod 会话 (Hybrid 内部)。
#[derive(Debug, Clone)]
struct ActiveMod {
    id: String,
    /// "shell" | "overlay"。
    kind: String,
}

/// 混合 UI 状态机 (egui 主线程 ↔ mod 窗线程 共享; `Arc<Hybrid>`)。
pub struct Hybrid {
    inner: Mutex<HybridInner>,
    /// mod 窗线程 JoinHandle (egui 退出后 join, 任务单退出语义)。
    join_handle: Mutex<Option<std::thread::JoinHandle<()>>>,
}

#[derive(Default)]
struct HybridInner {
    /// 当前活跃 mod (None = 原生 egui 界面)。
    active: Option<ActiveMod>,
    /// 指令通道 (mod 窗活跃时 Some; nav 转发线程持有 Receiver)。
    nav_tx: Option<std::sync::mpsc::Sender<NavMsg>>,
    /// 待消费的停用报告 (egui 每帧轮询取走)。
    pending_report: Option<StopReport>,
    /// egui 还原待办 (mod 窗结束且非全进程退出时置位; egui 帧头取走,
    /// 与 Win32 直连还原互为双保险)。
    restore_pending: bool,
    /// ★H 轮 (契约 v1.5 §11.6): mod 窗内 shell.stop{mode} 带来的目标界面模式
    /// (停用还原后由 egui 帧头消费应用; None = 纯停用, 不动模式)。
    pending_mode: Option<UiMode>,
}

impl Default for Hybrid {
    fn default() -> Self {
        Self::new()
    }
}

impl Hybrid {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(HybridInner::default()),
            join_handle: Mutex::new(None),
        }
    }

    /// 防中毒锁获取 (照 util::lock_guard 口径: 锁被 panic 弄脏仍继续运行)。
    fn lock(&self) -> MutexGuard<'_, HybridInner> {
        self.inner.lock().unwrap_or_else(|p| p.into_inner())
    }

    // ── 状态查询 (egui 页面/托盘路由) ────────────────────────────────

    /// 是否有 mod 窗活跃 (托盘 ShowWindow 路由 / egui 侧事件消费门控的判据)。
    pub fn is_active(&self) -> bool {
        self.lock().active.is_some()
    }

    /// 当前活跃 mod id (None = 原生 egui)。
    pub fn active_id(&self) -> Option<String> {
        self.lock().active.as_ref().map(|m| m.id.clone())
    }

    // ── 应用 / 停用 (egui「外观 MOD」页入口) ─────────────────────────

    /// 应用一个 MOD (egui「外观 MOD」页「应用」按钮)。
    ///
    /// * 不兼容包 → Err (不可应用);
    /// * 已有 mod 窗 → 换 URL / 回宿主注入 (契约 §11: 换 URL 不重开线程);
    /// * 无窗 → 建会话通道 + spawn 看护线程 (catch_unwind; 崩溃自动停用 +
    ///   Win32 还原 egui + 通知条)。egui 的隐藏由调用方在 `Ok(WindowSpawned)`
    ///   后用 `ViewportCommand::Visible(false)` 完成。
    pub fn apply(self: &Arc<Self>, backend: &Backend, meta: &ModMeta) -> Result<ApplyOutcome, String> {
        if !meta.compatible {
            return Err(format!(
                "MOD「{}」引擎不兼容 (声明 {}), 只可查看不可应用",
                meta.name, meta.engine
            ));
        }
        if self.is_active() {
            // 已有 mod 窗: 换 URL 不重开线程 (契约 §11)
            let nav_tx = {
                let inner = self.lock();
                inner.nav_tx.clone()
            };
            let Some(nav_tx) = nav_tx else {
                return Err("mod 窗会话通道已失效, 请先停用再应用".into());
            };
            let msg = if meta.kind == "overlay" {
                let resource = overlay_resource_url(meta)
                    .ok_or_else(|| "overlay 包缺 entry (注入资源), 无法应用".to_string())?;
                NavMsg::Overlay { id: meta.id.clone(), resource }
            } else {
                NavMsg::Shell { id: meta.id.clone(), url: entry_url_for(meta) }
            };
            nav_tx
                .send(msg)
                .map_err(|_| "mod 窗已关闭, 请重试应用".to_string())?;
            // 状态机同步 (egui 徽章/托盘路由)
            {
                let mut inner = self.lock();
                if let Some(a) = inner.active.as_mut() {
                    a.id = meta.id.clone();
                    a.kind = meta.kind.clone();
                }
            }
            return Ok(ApplyOutcome::Navigated);
        }

        // 首窗: 建会话指令通道 + spawn 看护线程
        let (nav_tx, nav_rx) = std::sync::mpsc::channel::<NavMsg>();
        {
            let mut inner = self.lock();
            if inner.active.is_some() {
                return Err("已有 mod 窗在运行 (并发应用被拒绝)".into());
            }
            inner.active = Some(ActiveMod { id: meta.id.clone(), kind: meta.kind.clone() });
            inner.nav_tx = Some(nav_tx);
            inner.pending_report = None;
            inner.restore_pending = false;
        }

        let me = self.clone();
        let backend_for_session = backend.clone();
        let backend_for_check = backend.clone();
        let meta2 = meta.clone();
        let handle = std::thread::Builder::new()
            .name("mod-window".into())
            .spawn(move || {
                let id = meta2.id.clone();
                // catch_unwind 看护 (任务单: 崩了 → 自动停用 + egui 还原 + 通知条;
                // 不做重启 —— 窗口重启会叠加第二个 WebView2 会话, 停用是唯一安全出口)
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    run_mod_session(backend_for_session, &meta2, nav_rx, me.clone())
                }));
                let (reason, err) = match result {
                    Ok(Ok(())) => {
                        // 会话正常返回: 区分全进程退出与单纯关窗
                        if backend_for_check.should_exit() {
                            (StopReason::AppExiting, None)
                        } else {
                            (StopReason::Closed, None)
                        }
                    }
                    Ok(Err(e)) => (StopReason::Crashed, Some(e.to_string())),
                    Err(_) => (StopReason::Crashed, Some("mod 窗线程 panic".to_string())),
                };
                me.finish_session(&id, reason);
                if reason == StopReason::Crashed {
                    if let Some(e) = err {
                        crate::util::crash_log("MOD_WINDOW", &format!("{id}: {e}"));
                    }
                    backend_for_check.notify_warning("外观 MOD 已停止 (异常退出), 已还原原生界面");
                }
                // 全进程退出中不还原 (egui 自己在关); 其余情况 Win32 直连还原
                // (spike: egui 隐藏期 update 停摆, ViewportCommand 不可依赖)。
                if reason != StopReason::AppExiting {
                    win32_show_egui();
                }
            })
            .map_err(|e| format!("mod 窗线程创建失败: {e}"))?;
        *self.join_handle.lock().unwrap_or_else(|p| p.into_inner()) = Some(handle);
        Ok(ApplyOutcome::WindowSpawned)
    }

    /// 「停用」按钮 (egui 侧): 下发 Stop 指令关 mod 窗。
    /// 会话结束后 finish_session 完成状态回滚与还原。
    pub fn request_stop(&self) -> bool {
        let tx = self.lock().nav_tx.clone();
        match tx {
            Some(tx) => tx.send(NavMsg::Stop).is_ok(),
            None => false,
        }
    }

    /// mod 窗内页面自行 shell.apply 切换 MOD → 同步状态机当前 id
    /// (egui 状态徽章/托盘路由读它; mod 窗线程侧入口)。
    pub fn update_active_id(&self, id: &str) {
        let mut inner = self.lock();
        if let Some(a) = inner.active.as_mut() {
            a.id = id.to_string();
        }
    }

    // ── ★H 轮: 停用+切模式 (契约 v1.5 §11.6) ────────────────────────

    /// mod 窗内「完整/极简/经典模式」→ 停用会话时带上目标模式 (shell.stop IPC 入口)。
    /// egui 帧头在还原后经 [`Hybrid::take_exit_mode`] 消费。
    pub fn set_exit_mode(&self, mode: UiMode) {
        self.lock().pending_mode = Some(mode);
    }

    /// 取走待应用的界面模式 (true = 停用会话曾携带目标模式; 只返回一次)。
    pub fn take_exit_mode(&self) -> Option<UiMode> {
        self.lock().pending_mode.take()
    }

    // ── 会话生命周期 ────────────────────────────────────────────────

    /// mod 窗会话结束 → 回滚状态机 + 挂还原待办 + 存报告 (mod 窗线程收尾调用)。
    /// `AppExiting` 不挂还原 (egui 正在全进程退出)。
    pub fn finish_session(&self, id: &str, reason: StopReason) {
        let mut inner = self.lock();
        inner.active = None;
        inner.nav_tx = None;
        if reason != StopReason::AppExiting {
            inner.restore_pending = true;
        }
        inner.pending_report = Some(StopReport { id: id.to_string(), reason });
    }

    // ── egui 主线程轮询 (每帧) ──────────────────────────────────────

    /// 取走停用报告 (有则 Some)。
    pub fn poll_stopped(&self) -> Option<StopReport> {
        self.lock().pending_report.take()
    }

    /// 取走还原待办 (true 只返回一次)。
    pub fn take_restore(&self) -> bool {
        let mut inner = self.lock();
        std::mem::take(&mut inner.restore_pending)
    }

    /// egui 退出后 join mod 窗线程 (任务单退出语义: egui 关闭 = 全进程退出, 线程收编)。
    pub fn join_mod_window(&self) {
        let handle = self.join_handle.lock().unwrap_or_else(|p| p.into_inner()).take();
        if let Some(h) = handle {
            if !h.is_finished() {
                // 泵 50ms 一拍即见 should_exit → QuitRequested, 会话退出在毫秒级
                let _ = h.join();
            }
        }
    }
}

/// 跑一个 mod 窗会话 (mod 窗线程内; 阻塞到会话结束)。
/// 会话差异全部在 [`SessionCfg`] (入口 URL / 默认尺寸 / overlay 注入 / 唯一窗口类名 / nav 通道)。
fn run_mod_session(
    backend: Backend,
    meta: &ModMeta,
    nav_rx: Receiver<NavMsg>,
    hybrid: Arc<Hybrid>,
) -> anyhow::Result<()> {
    let (entry_url, overlay_inject) = match meta.kind.as_str() {
        "overlay" => (CLASSIC_URL.to_string(), overlay_resource_url(meta)),
        _ => (entry_url_for(meta), None),
    };
    let cfg = SessionCfg {
        initial_url: entry_url,
        current: meta.id.clone(),
        // ★H 轮 (契约 v1.5 §11.5): mod 窗默认尺寸按包分流 —— endfield/extmod =
        // 1920×1080 设计稿基准 (修「mod 模式字太小」), overlay (宿主页) = 1180×760。
        initial_size: super::session_size_for(meta),
        overlay_inject,
        smoke: false,
        hybrid: Some(hybrid),
        nav_rx: Some(nav_rx),
        window_classname: Some(HYBRID_WINDOW_CLASS),
    };
    super::run_session(backend, cfg)
}

/// nav 指令 → 转发动作 (纯函数, 可单测; [`spawn_nav_forwarder`] 消费)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum NavAction {
    /// 先更新会话共享态 (current/overlay_inject), 再导航到 url。
    Navigate { current: String, overlay_inject: Option<String>, url: String },
    /// 关 mod 窗 (停用 MOD)。
    Stop,
}

/// NavMsg 的转发规则 (契约 §11):
/// * Shell → 换 current、清 overlay 注入、导航入口 URL;
/// * Overlay → 换 current、挂 overlay 注入资源、导航回宿主 index;
/// * Stop → 关窗。
pub(crate) fn nav_msg_action(msg: &NavMsg) -> NavAction {
    match msg {
        NavMsg::Shell { id, url } => NavAction::Navigate {
            current: id.clone(),
            overlay_inject: None,
            url: url.clone(),
        },
        NavMsg::Overlay { id, resource } => NavAction::Navigate {
            current: id.clone(),
            overlay_inject: Some(resource.clone()),
            url: CLASSIC_URL.to_string(),
        },
        NavMsg::Stop => NavAction::Stop,
    }
}

/// 主线程指令 → mod 窗事件循环 的转发线程 (run_session 在混合模式下拉起)。
/// recv 阻塞等待 (send 即唤醒); nav_tx 全部 drop (会话结束) → 通道关闭 → 线程退出。
pub(crate) fn spawn_nav_forwarder(
    shared: Arc<Mutex<ShellShared>>,
    proxy: EventLoopProxy<ShellEv>,
    nav_rx: Receiver<NavMsg>,
) {
    let _ = std::thread::Builder::new()
        .name("mod-window-nav".into())
        .spawn(move || {
            for msg in nav_rx {
                match nav_msg_action(&msg) {
                    NavAction::Navigate { current, overlay_inject, url } => {
                        if let Ok(mut s) = shared.lock() {
                            s.current = current;
                            s.overlay_inject = overlay_inject;
                        }
                        let _ = proxy.send_event(ShellEv::Navigate(url));
                    }
                    NavAction::Stop => {
                        let _ = proxy.send_event(ShellEv::StopWindow);
                        return; // 会话即将结束, 转发线程收工
                    }
                }
            }
        });
}

// ═══════════════════════════════════════════════════════════════════════
// MOD 入口 URL / 启动模式 / Win32 还原
// ═══════════════════════════════════════════════════════════════════════

/// ModMeta → mod 窗入口 URL (契约 §11):
/// 内置 shell 型 = `sorahk://mod/endfield/endfield.html`;
/// 外部 shell 型 = `sorahk://extmod/<id>/<entry>`;
/// overlay 型 = 宿主 index (资源走 hello 后注入)。
pub fn entry_url_for(meta: &ModMeta) -> String {
    if meta.kind == "overlay" {
        return CLASSIC_URL.to_string();
    }
    if meta.builtin {
        ENDFIELD_URL.to_string()
    } else {
        let entry = meta.entry.clone().unwrap_or_else(|| "index.html".into());
        format!("sorahk://extmod/{}/{}", meta.id, entry)
    }
}

/// overlay 型 MOD 的注入资源 URL (None = 非 overlay 包 / 缺 entry)。
pub fn overlay_resource_url(meta: &ModMeta) -> Option<String> {
    if meta.kind != "overlay" {
        return None;
    }
    let entry = meta.entry.clone()?;
    Some(format!("sorahk://extmod/{}/{}", meta.id, entry))
}

/// 启动模式 (契约 v1.4 §8 翻转: 默认 egui 原生界面)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartupMode {
    /// 默认: egui 原生界面 + 「外观 MOD」页 (混合模式在页内按需拉起 mod 窗)。
    NativeEgui,
    /// 旧纯 web 壳模式 (--shell / SORAHK_SHELL=1): 行为与 G 轮前一致。
    WebShell,
    /// --shell-smoke 冒烟通道 (不变; 在一切钩子/托盘前分流)。
    ShellSmoke,
}

/// 解析启动模式。`argv` = 去掉 argv[0] 的参数; `sorahk_shell` = SORAHK_SHELL 环境变量。
/// 优先级: --shell-smoke > --shell > SORAHK_SHELL=1/true > 默认 egui。
/// 兼容性: `--gui` 与 `SORAHK_SHELL=0` 保留接受, 语义 = 默认 (egui)。
pub fn startup_mode(argv: &[String], sorahk_shell: Option<&str>) -> StartupMode {
    if argv.iter().any(|a| a == "--shell-smoke") {
        return StartupMode::ShellSmoke;
    }
    if argv.iter().any(|a| a == "--shell") {
        return StartupMode::WebShell;
    }
    if let Some(v) = sorahk_shell {
        if v == "1" || v.eq_ignore_ascii_case("true") {
            return StartupMode::WebShell;
        }
    }
    StartupMode::NativeEgui
}

/// 解析 `--open-mod <id>` / `--open-mod=<id>` 启动参数 (G 轮补充):
/// 启动后自动应用指定外观 MOD (egui 帧循环跑稳后走与「应用」按钮**完全相同**的
/// `Hybrid::apply` 路径) —— 既是混合链路的自动化验收通道, 也是正当用户功能
/// (带参直达 MOD 界面)。缺值/空值 → None (stderr 提示); 多次出现取最后一个。
/// 仅 NativeEgui 分支消费 (WebShell/Smoke 忽略 —— 纯壳的 MOD 切换走页内 shell.apply)。
pub fn parse_open_mod(argv: &[String]) -> Option<String> {
    let mut out: Option<String> = None;
    let mut i = 0;
    while i < argv.len() {
        let a = &argv[i];
        if let Some(rest) = a.strip_prefix("--open-mod=") {
            let v = rest.trim();
            if !v.is_empty() {
                out = Some(v.to_string());
            } else {
                eprintln!("[sorahk] --open-mod= 缺少非空 id, 参数被忽略");
            }
        } else if a == "--open-mod" {
            match argv.get(i + 1) {
                Some(v) if !v.trim().is_empty() && !v.starts_with("--") => {
                    out = Some(v.trim().to_string());
                    i += 1; // 消费值
                }
                _ => eprintln!("[sorahk] --open-mod 缺少 <id> 参数 (用法: --open-mod <id>), 参数被忽略"),
            }
        }
        i += 1;
    }
    out
}

/// Win32 直连还原 egui 主窗 (托盘 restore_main_window 同款路径; mod 窗线程收尾用)。
/// egui 隐藏后 update 停摆 (spike 实测), ViewportCommand 不可依赖 —— 必须绕过事件
/// 循环直接 ShowWindow; 窗口重新可见后 eframe 帧循环自行恢复。
/// ★H 轮 (契约 v1.5 §11.6): Win32 直连调整 egui 主窗几何 (逻辑尺寸; 可带位置)。
/// 场景 = 「停用 MOD + 切模式」: mod 线程 Win32 还原 egui 后, egui 侧
/// ViewportCommand::InnerSize 实测在「隐藏→还原」当帧不生效 (经典模式窗口维持
/// 还原前尺寸), 与 [`win32_show_egui`] 同款直连机制按目标模式直接设定窗口几何。
/// egui 侧的模式标记/渲染已由帧头应用, 这里只兜几何。
pub fn win32_resize_egui(logical_w: f64, logical_h: f64, pos: Option<(f64, f64)>) {
    use windows::Win32::Foundation::{RECT, HWND};
    use windows::Win32::UI::HiDpi::GetDpiForWindow;
    use windows::Win32::UI::WindowsAndMessaging::{
        AdjustWindowRectEx, FindWindowW, GetWindowLongW, SetWindowPos, GWL_EXSTYLE, GWL_STYLE,
        SWP_NOZORDER, SWP_NOMOVE, WINDOW_EX_STYLE, WINDOW_STYLE,
    };
    unsafe {
        let title: Vec<u16> = format!("{EGUI_WINDOW_TITLE} ").encode_utf16().collect();
        let result = FindWindowW(windows::core::PCWSTR::null(), windows::core::PCWSTR(title.as_ptr()));
        if let Ok(hwnd) = result {
            if hwnd.is_invalid() {
                return;
            }
            let dpi = GetDpiForWindow(hwnd) as f64; // 96 = 100%
            let w = (logical_w * dpi / 96.0).round() as i32;
            let h = (logical_h * dpi / 96.0).round() as i32;
            // 客户区逻辑尺寸 → 外框物理尺寸 (按窗口样式补边框)
            let mut rect = RECT { left: 0, top: 0, right: w, bottom: h };
            let style = WINDOW_STYLE(GetWindowLongW(hwnd, GWL_STYLE) as u32);
            let ex = WINDOW_EX_STYLE(GetWindowLongW(hwnd, GWL_EXSTYLE) as u32);
            let _ = AdjustWindowRectEx(&mut rect, style, false, ex);
            let ow = rect.right - rect.left;
            let oh = rect.bottom - rect.top;
            let (x, y, flags) = match pos {
                Some((px, py)) => (
                    (px * dpi / 96.0).round() as i32,
                    (py * dpi / 96.0).round() as i32,
                    SWP_NOZORDER,
                ),
                None => (0, 0, SWP_NOZORDER | SWP_NOMOVE),
            };
            let _ = SetWindowPos(hwnd, None, x, y, ow, oh, flags);
        }
    }
}

pub fn win32_show_egui() {
    use windows::Win32::UI::WindowsAndMessaging::{
        FindWindowW, SetForegroundWindow, ShowWindowAsync, SW_RESTORE,
    };
    unsafe {
        let title: Vec<u16> = format!("{EGUI_WINDOW_TITLE}\0").encode_utf16().collect();
        let result = FindWindowW(
            windows::core::PCWSTR::null(),
            windows::core::PCWSTR(title.as_ptr()),
        );
        if let Ok(hwnd) = result {
            if !hwnd.is_invalid() {
                let _ = ShowWindowAsync(hwnd, SW_RESTORE);
                let _ = SetForegroundWindow(hwnd);
            } else {
                crate::util::crash_log("HYBRID_RESTORE", "FindWindowW 返回无效句柄 (egui 主窗尚未创建?)");
            }
        } else {
            crate::util::crash_log("HYBRID_RESTORE", "FindWindowW 未找到 egui 主窗 (标题不匹配? 请回报此日志)");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// G 轮: Hybrid 状态机 (首窗 → 指令通道 → 停用报告 → 还原待办 → 再应用)。
    /// 纯状态测试, 不 spawn 真窗 (窗口线程路径由 spike + smoke 覆盖)。
    #[test]
    fn hybrid_state_machine() {
        let h = Hybrid::new();
        assert!(!h.is_active());
        assert_eq!(h.active_id(), None);

        // 模拟 apply 的会话建立 (绕开线程 spawn: 直接操纵内部通道)
        let (nav_tx, nav_rx) = std::sync::mpsc::channel::<NavMsg>();
        {
            let mut inner = h.lock();
            inner.active = Some(ActiveMod { id: "endfield".into(), kind: "shell".into() });
            inner.nav_tx = Some(nav_tx);
        }
        assert!(h.is_active());
        assert_eq!(h.active_id().as_deref(), Some("endfield"));

        // 停用指令送达
        assert!(h.request_stop());
        let dropped_rx = nav_rx;
        match dropped_rx.recv_timeout(Duration::from_millis(100)) {
            Ok(NavMsg::Stop) => {}
            other => panic!("应收到 Stop 指令, 实际 {other:?}"),
        }

        // 会话结束 → 状态回滚 + 还原待办 + 报告
        h.finish_session("endfield", StopReason::Closed);
        assert!(!h.is_active());
        assert!(h.take_restore(), "Closed 必须挂还原待办");
        assert!(!h.take_restore(), "还原待办只消费一次");
        let report = h.poll_stopped().expect("必须有待报告");
        assert_eq!(report.id, "endfield");
        assert_eq!(report.reason, StopReason::Closed);
        assert!(h.poll_stopped().is_none(), "报告只消费一次");

        // AppExiting: 不挂还原 (egui 正在全进程退出)
        h.finish_session("x", StopReason::AppExiting);
        assert!(!h.take_restore());
        assert_eq!(h.poll_stopped().map(|r| r.reason), Some(StopReason::AppExiting));
    }

    /// G 轮: nav 转发线程的指令语义 (Shell 换 URL / Overlay 回宿主注入 / Stop 关窗)。
    /// 转发规则抽成 [`nav_msg_action`] 纯函数; 真实转发线程由 smoke/手动点验覆盖。
    #[test]
    fn nav_forwarder_semantics() {
        // Shell 型: 换 current、清 overlay 注入、导航入口 URL
        assert_eq!(
            nav_msg_action(&NavMsg::Shell { id: "endfield".into(), url: ENDFIELD_URL.into() }),
            NavAction::Navigate {
                current: "endfield".into(),
                overlay_inject: None,
                url: ENDFIELD_URL.into(),
            }
        );
        // Overlay 型: 换 current、挂注入资源、导航回宿主 index (契约 §11)
        assert_eq!(
            nav_msg_action(&NavMsg::Overlay {
                id: "oldtheme".into(),
                resource: "sorahk://extmod/oldtheme/theme.css".into(),
            }),
            NavAction::Navigate {
                current: "oldtheme".into(),
                overlay_inject: Some("sorahk://extmod/oldtheme/theme.css".into()),
                url: CLASSIC_URL.into(),
            }
        );
        // Stop = 关窗指令 (非导航)
        assert_eq!(nav_msg_action(&NavMsg::Stop), NavAction::Stop);
    }

    /// G 轮补充: `--open-mod` 参数解析 (空格/等号两种形态 + 缺值/空值/未知旗标防护)。
    #[test]
    fn parse_open_mod_forms() {
        let argv = |items: &[&str]| -> Vec<String> { items.iter().map(|s| s.to_string()).collect() };
        // 空格形态
        assert_eq!(
            parse_open_mod(&argv(&["--open-mod", "endfield"])),
            Some("endfield".to_string())
        );
        // 等号形态
        assert_eq!(
            parse_open_mod(&argv(&["--open-mod=endfield"])),
            Some("endfield".to_string())
        );
        // 多次出现取最后一个
        assert_eq!(
            parse_open_mod(&argv(&["--open-mod=a", "--open-mod", "b"])),
            Some("b".to_string())
        );
        // 缺值 / 空值 / 下一参为旗标 → None (stderr 提示)
        assert_eq!(parse_open_mod(&argv(&["--open-mod"])), None);
        assert_eq!(parse_open_mod(&argv(&["--open-mod="])), None);
        assert_eq!(parse_open_mod(&argv(&["--open-mod", "--shell-smoke"])), None);
        // 无参数
        assert_eq!(parse_open_mod(&argv(&[])), None);
        assert_eq!(parse_open_mod(&argv(&["--gui"])), None);
    }

    /// ★H 轮 (契约 v1.5 §11.6): shell.stop{mode} → set_exit_mode 暂存 →
    /// egui 帧头 take_exit_mode 消费 (只取一次; 新会话不残留旧值)。
    #[test]
    fn exit_mode_pending_plumbing() {
        let h = Hybrid::new();
        assert_eq!(h.take_exit_mode(), None, "初始无待应用模式");
        h.set_exit_mode(UiMode::Minimal);
        assert_eq!(h.take_exit_mode(), Some(UiMode::Minimal));
        assert_eq!(h.take_exit_mode(), None, "只消费一次");
        h.set_exit_mode(UiMode::Classic);
        h.set_exit_mode(UiMode::Full);
        assert_eq!(h.take_exit_mode(), Some(UiMode::Full), "多次设置取最后值");
        assert_eq!(h.take_exit_mode(), None);
        // parse_exit_mode 词表 (IPC mode 字符串)
        assert_eq!(parse_exit_mode("minimal"), UiMode::Minimal);
        assert_eq!(parse_exit_mode("classic"), UiMode::Classic);
        assert_eq!(parse_exit_mode("full"), UiMode::Full);
        assert_eq!(parse_exit_mode("bogus"), UiMode::Full);
    }
}
