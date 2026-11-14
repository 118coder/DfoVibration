//! 本地 Web 控制台：Rust 后端 + 浏览器前端。
//!
//! 定位：把原来那套 egui 界面换成网页，进程仍然是一个普通的 Rust 程序 —— 钩子、
//! 连发 worker、震动引擎、托盘全部留在原生侧，网页只负责「看状态 + 改配置」。
//!
//! 安全边界（本地界面最容易踩的两个坑）：
//! * 只监听 `127.0.0.1` 的**随机**端口，不对外网开放；
//! * 启动时生成一次性口令，写进 index.html 的 `<meta>`，所有 `/api/*` 请求都要带；
//!   没有口令的页面拿不到这个 meta，等于没有接口。
//!
//! 单文件分发：前端资源全部 `include_str!` 进 exe（见 `assets`），不要求安装目录
//! 旁边有一份 `webui/`。开发时设 `SORAHK_WEBUI_DIR` 可以直接改、刷新即见。

mod assets;
mod http;

use std::io::Write;
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::config::AppConfig;
use crate::state::AppState;
use crate::update::{Phase, UpdateManager};

/// 配置文件基路径（与 GUI 保持一致，都是工作目录下的 Config.toml）。
const CONFIG_PATH: &str = "Config.toml";
/// 状态推送间隔：500ms 够顺滑，也不会让浏览器一直重绘。
const STREAM_MS: u64 = 500;
/// 一次最多回多少行日志。
const LOG_LINES_MAX: usize = 500;

/// 是否走 Web 控制台。
///
/// 优先级：`--gui`（临时切回原版窗口）> `--web` > 专用 exe 文件名含 `WebUI`
/// > 环境变量 `SORAHK_WEBUI=1` > 配置 `web_ui`。
/// 专用包双击即可进新版控制台，不必记参数；老用户仍靠配置开关决定启动方式。
pub fn requested(argv: &[String]) -> bool {
    if argv.iter().any(|a| a == "--gui") {
        return false;
    }
    if argv.iter().any(|a| a == "--web") {
        return true;
    }
    let dedicated_exe = std::env::current_exe()
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_lowercase()))
        .is_some_and(|name| name.contains("webui"));
    if dedicated_exe {
        return true;
    }
    matches!(
        std::env::var("SORAHK_WEBUI").ok().as_deref(),
        Some("1") | Some("true") | Some("yes")
    )
}

pub struct WebUi {
    pub addr: std::net::SocketAddr,
    token: String,
    inner: Arc<Inner>,
}

struct Inner {
    state: Arc<AppState>,
    /// 当前配置的唯一真身。GUI 模式下这份由 egui 持有；Web 模式下由这里持有。
    config: RwLock<AppConfig>,
    update: UpdateManager,
    started: Instant,
    /// 服务线程自己用的停机标志（进程退出走 state.should_exit）。
    stop: AtomicBool,
    /// 保存动作的串行化：滑块连发时避免两条 patch 同时写盘。
    saving: Mutex<()>,
}

impl WebUi {
    /// 起服务；返回句柄后调用方决定什么时候退出。
    pub fn start(state: Arc<AppState>, config: AppConfig) -> std::io::Result<Self> {
        let listener = TcpListener::bind(("127.0.0.1", 0))?;
        let addr = listener.local_addr()?;
        let token = make_token();
        let inner = Arc::new(Inner {
            state,
            config: RwLock::new(config),
            update: UpdateManager::new(),
            started: Instant::now(),
            stop: AtomicBool::new(false),
            saving: Mutex::new(()),
        });
        let stop = inner.clone();

        let ui = Self {
            addr,
            token: token.clone(),
            inner: inner.clone(),
        };

        // 每个连接一个线程：SSE 会长期占住一条，不能堵住 accept。
        std::thread::Builder::new()
            .name("webui-http".into())
            .spawn(move || {
                for stream in listener.incoming() {
                    if stop.stop.load(Ordering::Relaxed) {
                        break;
                    }
                    let Ok(stream) = stream else { continue };
                    let inner = inner.clone();
                    let token = token.clone();
                    let ui = Handler { inner, token };
                    std::thread::Builder::new()
                        .name("webui-conn".into())
                        .spawn(move || ui.handle(stream))
                        .ok();
                }
            })?;

        Ok(ui)
    }

    pub fn url(&self) -> String {
        format!("http://{}/", self.addr)
    }

    /// 让浏览器把控制台打开。失败不影响服务本身（用户可手动复制地址）。
    pub fn open_browser(&self) {
        let url = self.url();
        #[cfg(windows)]
        {
            let _ = std::process::Command::new("cmd")
                .args(["/C", "start", "", &url])
                .spawn();
        }
        #[cfg(not(windows))]
        {
            let _ = std::process::Command::new("xdg-open").arg(&url).spawn();
        }
    }

    /// 阻塞到进程该退出为止（托盘退出、网页点退出、Ctrl+C 都落到 should_exit）。
    pub fn wait(&self) {
        while !self.inner.state.should_exit() && !self.inner.stop.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(200));
        }
    }

    pub fn shutdown(&self) {
        self.inner.stop.store(true, Ordering::Relaxed);
    }
}

struct Handler {
    inner: Arc<Inner>,
    token: String,
}

impl Handler {
    fn handle(&self, mut stream: TcpStream) {
        stream.set_nodelay(true).ok();
        let Some(req) = http::read_request(&mut stream) else {
            return;
        };
        // 事件流要一直写着走，单独处理。
        if req.path == "/api/events" {
            if !self.authorized(&req) {
                let _ = http::write_response(&mut stream, &http::Response::error(401, "口令不对"));
                return;
            }
            self.stream_events(stream);
            return;
        }
        let res = self.route(&req);
        let _ = http::write_response(&mut stream, &res);
    }

    fn authorized(&self, req: &http::Request) -> bool {
        // 常量时间比较没意义（本地回环 + 随机端口 + 每启动一次换口令），
        // 但至少要求长度一致，别让空口令误过。
        match req.token() {
            Some(t) => t.len() == self.token.len() && t.as_bytes() == self.token.as_bytes(),
            None => false,
        }
    }

    fn route(&self, req: &http::Request) -> http::Response {
        let path = req.path.as_str();

        // ---------- 页面与静态资源（不需要口令，本来就没什么秘密） ----------
        if path == "/" || path == "/index.html" {
            return match assets::index_html(&self.token) {
                Some(html) => http::Response::new(
                    200,
                    "text/html; charset=utf-8",
                    html.into_bytes(),
                ),
                None => http::Response::error(500, "前端资源缺失"),
            };
        }
        if let Some(rest) = path.strip_prefix("/assets/") {
            let key = format!("/assets/{rest}");
            return match assets::load(&key) {
                Some((body, mime)) => http::Response::new(200, mime, body),
                None => http::Response::error(404, "没有这个文件"),
            };
        }

        // ---------- 接口 ----------
        if !path.starts_with("/api/") {
            return http::Response::error(404, "没有这个地址");
        }
        if !self.authorized(req) {
            return http::Response::error(401, "口令不对，请从控制台页面里操作");
        }

        match (req.method.as_str(), path) {
            ("GET", "/api/bootstrap") => {
                let cfg = read(&self.inner.config);
                http::Response::json(&json!({
                    "ok": true,
                    "version": crate::update::APP_VERSION,
                    "status": self.status_json(&cfg),
                    "config": serde_json::to_value(&*cfg).unwrap_or(Value::Null),
                    "vibration": serde_json::to_value(&cfg.vibration).unwrap_or(Value::Null),
                    "logs": read_logs(80),
                }))
            }
            ("GET", "/api/state") => {
                let cfg = read(&self.inner.config);
                http::Response::json(&json!({ "ok": true, "status": self.status_json(&cfg) }))
            }
            ("GET", "/api/logs") => {
                let n = req
                    .query("n")
                    .and_then(|v| v.parse::<usize>().ok())
                    .unwrap_or(200)
                    .min(LOG_LINES_MAX);
                http::Response::json(&json!({ "ok": true, "lines": read_logs(n) }))
            }
            ("POST", "/api/action") => match body_json(req) {
                Ok(v) => self.action(v.get("act").and_then(Value::as_str).unwrap_or("")),
                Err(res) => res,
            },
            ("POST", "/api/config") => match body_json(req) {
                Ok(v) => {
                    let patch = v.get("patch").cloned().unwrap_or(Value::Null);
                    self.patch_config(patch)
                }
                Err(res) => res,
            },
            ("POST", "/api/mappings") => match body_json(req) {
                Ok(v) => {
                    let list = v.get("mappings").cloned().unwrap_or(Value::Null);
                    self.patch_config(json!({ "mappings": list }))
                }
                Err(res) => res,
            },
            ("POST", "/api/vibration") => match body_json(req) {
                Ok(v) => {
                    let patch = v.get("patch").cloned().unwrap_or(Value::Null);
                    self.patch_vibration(patch)
                }
                Err(res) => res,
            },
            ("GET", _) | ("POST", _) => http::Response::error(404, "没有这个接口"),
            _ => http::Response::error(405, "方法不对"),
        }
    }

    fn action(&self, act: &str) -> http::Response {
        let state = &self.inner.state;
        match act {
            "toggle-pause" => {
                let paused = state.toggle_paused();
                http::Response::json(&json!({ "ok": true, "paused": paused }))
            }
            "quit" => {
                state.exit();
                http::Response::json(&json!({ "ok": true, "message": "正在退出" }))
            }
            "check-update" => {
                let started = self.inner.update.check(String::new());
                http::Response::json(&json!({
                    "ok": true,
                    "message": if started { "正在检查更新…" } else { "已经在检查了" },
                }))
            }
            "test-pulse" => {
                // 极简模式的触感反馈：让震动线程按测试通道震一小下。
                let until = crate::vibration::now_ms_u64() + 140;
                state
                    .vibration_test_until
                    .store(until, std::sync::atomic::Ordering::Relaxed);
                http::Response::json(&json!({ "ok": true }))
            }
            "test-vibration" => {
                // 试震 1 秒：让玩家不进游戏也能确认手柄那条线是通的。
                let until = crate::vibration::now_ms_u64() + 1000;
                state
                    .vibration_test_until
                    .store(until, std::sync::atomic::Ordering::Relaxed);
                http::Response::json(&json!({ "ok": true }))
            }
            "test-rank" => {
                state
                    .vibration_rank_test
                    .store(true, std::sync::atomic::Ordering::Relaxed);
                http::Response::json(&json!({ "ok": true }))
            }
            "audio-self-check" => {
                state.audio_hap.request_self_check();
                http::Response::json(&json!({ "ok": true }))
            }
            "audio-calibrate" => {
                state.audio_hap.request_calibration();
                http::Response::json(&json!({ "ok": true, "message": "保持安静 3 秒" }))
            }
            "audio-calib-dismiss" => {
                state
                    .audio_hap
                    .calib_result
                    .store(0, std::sync::atomic::Ordering::Relaxed);
                http::Response::json(&json!({ "ok": true }))
            }
            "reset-devices" => {
                crate::input_manager::request_xinput_reset();
                http::Response::json(&json!({ "ok": true, "message": "已请求重连手柄" }))
            }
            "refresh-devices" => {
                // 设备清单在 status 里每 500ms 推一次，这里只需给界面一个确认。
                http::Response::json(&json!({ "ok": true }))
            }
            "restart-threads" => {
                let names = crate::util::stopped_threads();
                let restarted = names
                    .iter()
                    .filter(|n| crate::util::restart_thread(*n))
                    .count();
                http::Response::json(&json!({ "ok": true, "restarted": restarted }))
            }
            "save" => {
                let cfg = read(&self.inner.config);
                match cfg.save_to_file(CONFIG_PATH) {
                    Ok(()) => http::Response::json(&json!({
                        "ok": true,
                        "message": "已写入 Config.toml",
                    })),
                    Err(e) => http::Response::error(500, &format!("写入失败：{e}")),
                }
            }
            other => http::Response::error(400, &format!("不认识的动作：{other}")),
        }
    }

    /// 改主配置：JSON 深合并 → 反序列化 → 落盘 → 运行时热重载。
    ///
    /// 走「合并 + 整份反序列化」而不是逐字段手写，是为了以后加字段不用改接口；
    /// `vibration` 是 `#[serde(skip)]`，反序列化后会被默认值覆盖，所以这里显式还回去。
    fn patch_config(&self, patch: Value) -> http::Response {
        if !patch.is_object() {
            return http::Response::error(400, "patch 必须是对象");
        }
        let _guard = lock(&self.inner.saving);
        let mut cfg = write(&self.inner.config);

        let mut base = match serde_json::to_value(&*cfg) {
            Ok(v) => v,
            Err(e) => return http::Response::error(500, &format!("序列化失败：{e}")),
        };
        merge(&mut base, &patch);
        let mut next: AppConfig = match serde_json::from_value(base) {
            Ok(v) => v,
            Err(e) => return http::Response::error(400, &format!("字段对不上：{e}")),
        };
        next.vibration = cfg.vibration.clone();
        next.vibration_presets = cfg.vibration_presets.clone();

        if let Err(e) = next.save_to_file(CONFIG_PATH) {
            return http::Response::error(500, &format!("写入 Config.toml 失败：{e}"));
        }
        if let Err(e) = self.inner.state.reload_config(next.clone()) {
            return http::Response::error(500, &format!("热重载失败：{e}"));
        }
        *cfg = next;
        http::Response::json(&json!({
            "ok": true,
            "config": serde_json::to_value(&*cfg).unwrap_or(Value::Null),
            "vibration": serde_json::to_value(&cfg.vibration).unwrap_or(Value::Null),
        }))
    }

    fn patch_vibration(&self, patch: Value) -> http::Response {
        if !patch.is_object() {
            return http::Response::error(400, "patch 必须是对象");
        }
        let _guard = lock(&self.inner.saving);
        let mut cfg = write(&self.inner.config);

        let mut base = match serde_json::to_value(&cfg.vibration) {
            Ok(v) => v,
            Err(e) => return http::Response::error(500, &format!("序列化失败：{e}")),
        };
        merge(&mut base, &patch);
        let next: crate::config::VibrationConfig = match serde_json::from_value(base) {
            Ok(v) => v,
            Err(e) => return http::Response::error(400, &format!("字段对不上：{e}")),
        };
        cfg.vibration = next;

        // 只写震动设定文件；Config.toml 里的其它内容不该因为拖个滑块被动一遍。
        let vib_path = AppConfig::vibration_path_for(CONFIG_PATH);
        if let Err(e) = cfg.save_vibration_to_file(vib_path) {
            return http::Response::error(500, &format!("写入震动设定失败：{e}"));
        }
        if let Err(e) = self.inner.state.reload_config(cfg.clone()) {
            return http::Response::error(500, &format!("热重载失败：{e}"));
        }
        http::Response::json(&json!({
            "ok": true,
            "vibration": serde_json::to_value(&cfg.vibration).unwrap_or(Value::Null),
        }))
    }

    /// 状态快照：网页每 500ms 收一次，字段都做成了「拿不到就是 null/0」。
    fn status_json(&self, cfg: &AppConfig) -> Value {
        let state = &self.inner.state;
        let hid = state.live_hid_state();
        let xinput = state.live_xinput_state();
        let (ml, mr) = state.audio_hap.output();
        let snap = self.inner.update.snapshot();

        let source = match (&hid, xinput) {
            (Some(h), _) => format!("{:04X}:{:04X} · RawInput", h.vid, h.pid),
            (None, Some((vid, _))) => format!("{vid:04X} · XInput"),
            _ => String::new(),
        };

        let counts = json!({
            "mappings": cfg.mappings.len(),
            "turbo": cfg.mappings.iter().filter(|m| m.turbo_enabled).count(),
            "combos": cfg
                .mappings
                .iter()
                .filter(|m| m.trigger_key.contains('+'))
                .count(),
            "presets": cfg.presets.len(),
            "whitelist": cfg.process_whitelist.len(),
            "macros": cfg.universal_macros.len(),
        });

        let input = json!({
            "source": source,
            "hid": hid.map(|h| json!({
                "present": true,
                "vid": h.vid,
                "pid": h.pid,
                "buttons": h.buttons,
                "dpad": h.dpad,
                "ls": h.ls,
                "rs": h.rs,
                "lt": h.lt,
                "rt": h.rt,
                "raw": h.raw_position,
            })).unwrap_or_else(|| json!({ "present": false })),
        });

        json!({
            "version": crate::update::APP_VERSION,
            "uptime": self.inner.started.elapsed().as_secs(),
            "paused": state.is_paused(),
            "workers": state.get_actual_worker_count(),
            "counts": counts,
            "vibration": {
                "enabled": cfg.vibration.enabled,
                "legacy": cfg.vib_legacy_client,
                "master": cfg.vibration.master_gain,
            },
            "audio": {
                "enabled": cfg.audio_haptics.enabled,
                "preset": cfg.audio_haptics.preset,
            },
            "motor": {
                "l": f32::from(ml) / 65535.0,
                "r": f32::from(mr) / 65535.0,
            },
            "input": input,
            "update": {
                "phase": phase_name(snap.phase),
                "latest": snap.latest.as_ref().map(|r| r.version.clone()),
                "error": snap.error,
            },
            // 校准建议门槛（0 = 没有建议）与停掉的后台线程，供网页里的提示条使用。
            "audio_calib": state
                .audio_hap
                .calib_result
                .load(std::sync::atomic::Ordering::Relaxed),
            "stopped_threads": crate::util::stopped_threads(),
        })
    }

    /// SSE：推状态，客户端一断写就失败，线程随之退出。
    fn stream_events(&self, mut stream: TcpStream) {
        if http::start_sse(&mut stream).is_err() {
            return;
        }
        let state = &self.inner.state;
        let mut tick: u64 = 0;
        while !state.should_exit() && !self.stream_stopped() {
            let cfg = read(&self.inner.config);
            let payload = json!({
                "status": self.status_json(&cfg),
                "vibration": serde_json::to_value(&cfg.vibration).unwrap_or(Value::Null),
            });
            let frame = format!("event: status\ndata: {payload}\n\n");
            if stream.write_all(frame.as_bytes()).is_err() || stream.flush().is_err() {
                break;
            }
            tick += 1;
            // 每 20 次（10 秒）补一个心跳注释，防止休眠回来的连接被中间层判死。
            if tick % 20 == 0 && stream.write_all(b": ping\n\n").is_err() {
                break;
            }
            std::thread::sleep(Duration::from_millis(STREAM_MS));
        }
    }

    /// SSE 线程检查的是「进程退出」，句柄那边的 stop 不在作用域里，
    /// 这里用状态里的同一份退出标志即可（走 `should_exit`）。
    fn stream_stopped(&self) -> bool {
        self.inner.stop.load(Ordering::Relaxed)
    }
}

/* ---------------------------------------------------------------- 小工具 */

fn read(cfg: &RwLock<AppConfig>) -> std::sync::RwLockReadGuard<'_, AppConfig> {
    cfg.read().unwrap_or_else(|e| e.into_inner())
}

fn write(cfg: &RwLock<AppConfig>) -> std::sync::RwLockWriteGuard<'_, AppConfig> {
    cfg.write().unwrap_or_else(|e| e.into_inner())
}

fn lock(m: &Mutex<()>) -> std::sync::MutexGuard<'_, ()> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn body_json(req: &http::Request) -> Result<Value, http::Response> {
    if req.body.is_empty() {
        return Ok(Value::Null);
    }
    serde_json::from_slice(&req.body)
        .map_err(|e| http::Response::error(400, &format!("JSON 解析失败：{e}")))
}

/// 深合并：对象逐键递归，其余（数组/标量/null）直接覆盖。
fn merge(base: &mut Value, patch: &Value) {
    match (base, patch) {
        (Value::Object(dst), Value::Object(src)) => {
            for (k, v) in src {
                match dst.get_mut(k) {
                    Some(slot) => merge(slot, v),
                    None => {
                        dst.insert(k.clone(), v.clone());
                    }
                }
            }
        }
        (dst, src) => *dst = src.clone(),
    }
}

fn phase_name(p: Phase) -> &'static str {
    match p {
        Phase::Idle => "Idle",
        Phase::Checking => "Checking",
        Phase::Downloading => "Downloading",
        Phase::Downloaded => "Downloaded",
        Phase::Failed => "Failed",
    }
}

fn make_token() -> String {
    // 进程内计数器 + 时间 + 地址，避免同一毫秒里两次启动撞出同一个口令。
    static SEQ: AtomicU64 = AtomicU64::new(0);
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0)
        .hash(&mut h);
    SEQ.fetch_add(1, Ordering::Relaxed).hash(&mut h);
    (&h as *const _ as usize).hash(&mut h);
    format!("{:016x}", h.finish())
}

/// 读运行日志的最后 `n` 行。
///
/// 日志在 exe 旁边（`vibration::vib_log_at` 的约定），一行形如
/// `[1730000000000] [INFO] ...`；时间戳交给前端按本机时区渲染，后端不猜时区。
fn read_logs(n: usize) -> Vec<Value> {
    let path = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("SorahkDFO_vib.log")))
        .unwrap_or_else(|| std::path::PathBuf::from("SorahkDFO_vib.log"));
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    lines
        .iter()
        .rev()
        .take(n)
        .rev()
        .map(|line| {
            let (ms, msg) = match line.strip_prefix('[') {
                Some(rest) => match rest.split_once(']') {
                    Some((ts, tail)) => (
                        ts.trim().parse::<u64>().ok(),
                        tail.trim_start().to_string(),
                    ),
                    None => (None, line.to_string()),
                },
                None => (None, line.to_string()),
            };
            json!({ "ms": ms, "m": msg })
        })
        .collect()
}
