//! ★双 UI 壳 —— 快照泵线程 (契约 §1/§3.1)。
//!
//! 50ms 一拍:
//! * `Backend::snapshot()` 聚合 → 快照推送 (契约 §3.1 **v1.3 差量推送**):
//!   首帧/hello 后/换页 = 全量 `{"t":"snap","full":true,"data":{…六域}}`;
//!   此后对六域 (core/input/vibration/audio/update/counts) 分别 FNV-1a 哈希
//!   (哈希输入 = 该域实际要发的序列化文本, 哈希/载荷天然一致),
//!   只把变化的域装进 patch = `{"t":"snap","patch":{"vibration":{…}}}`
//!   —— WebView2 下消除「大 JSON 每帧全量重解析」卡顿源; 全部未变不推。
//! * 异步事件合并转发 (契约 §3.1 evt): `poll_events()` 排空 + 既有按通道透传
//!   (托盘 ShowWindow/ShowAbout、手柄预设切换、RawInput 捕获结果、HID 激活报文)
//!   —— 后者今天还没接 UiEvent 队列 (架构说明 §3.3), 泵把它们统一搬进 evt 通道;
//! * hello 首帧 (页面加载/换壳后推一次);
//! * 外部 MOD 目录失效 → 自动回 classic (契约 §5 不允许无主题可用)。
//!
//! smoke 判据说明: `--shell-smoke` 的握手判据 = JS 回执 `bridge.ready`
//! (在 mod.rs handle_ipc 分支上报 SmokeOk), 泵不再单方上报 —— 宿主推了 hello
//! 不代表 JS→Rust 桥通了, 交付轮以真桥回执为准。
//!
//! 线程模型: WebView 非 Clone 且 evaluate 必须在事件循环线程 (spike 实测),
//! 泵线程只做聚合与序列化, 经 `EventLoopProxy` 转发 JSON 给事件循环执行。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::json;
use tao::event_loop::EventLoopProxy;

use crate::backend::Backend;

use super::{ShellEv, ShellShared};

/// 泵节拍。★M2 轮: 50ms → 100ms（20Hz → 10Hz）——真 wry 桥下 audio 频谱/motor 实时域
/// 每拍必变，20Hz 全速灌入 + 前端 DOM 写入是持续卡顿主因；10Hz 视觉无感（人眼频谱条
/// 20Hz vs 10Hz 分辨不出），推送压力与跨进程 evaluate 次数直接砍半。
const TICK: Duration = Duration::from_millis(100);
/// HID 激活报文等待上限 (拍数; 50 × 100ms = 5s, 保持原 5s 超时不变)。
const HID_MAX_WAIT_TICKS: u32 = 50;

/// 快照六域名 (契约 §3.1; 顺序 = `UiSnapshot` serde 字段序)。
/// 差量推送按域检测变化: 改一个域只发该域 patch (见 `domain_hashes` 测试)。
const SNAP_DOMAINS: [&str; 6] = ["core", "input", "vibration", "audio", "update", "counts"];

/// 泵角色 (G 轮混合模式 §11):
/// * [`PumpRole::Standalone`] = 纯 shell 模式 (--shell/smoke), 行为与 G 轮前一致;
/// * [`PumpRole::ModWindow`] = 混合模式的 mod 窗: ① 消费托盘 ShowWindow 还原本窗
///   (egui 隐藏期 update 停摆 —— spike 实测 —— 不能依赖 egui 转发); ② **不**消费
///   ShowAbout (留给 egui, mod 停用还原后照常弹)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PumpRole {
    Standalone,
    ModWindow,
}

/// 泵上下文。
pub struct Pump {
    pub backend: Backend,
    pub proxy: EventLoopProxy<ShellEv>,
    pub shared: Arc<Mutex<ShellShared>>,
    /// 换页/换壳后由事件循环置位; 泵推完 hello 清零。
    pub hello_pending: Arc<AtomicBool>,
    /// 当前页 hello 是否已推 (IPC 首包兜底判据 + smoke 诊断输出)。
    pub hello_sent: Arc<AtomicBool>,
    /// 事件循环退出时置位, 泵自行结束。
    pub quit: Arc<AtomicBool>,
    /// HID 报文等待表 (handle → 已等待拍数)。
    pub hid_waiting: Vec<(isize, u32)>,
    /// should_exit 只报一次。
    pub quit_reported: bool,
    /// 泵角色 (G 轮混合模式; Standalone = 旧行为)。
    pub role: PumpRole,
}

impl Pump {
    /// 拉起泵线程 (supervised 看护之外的后台线程; 泵挂掉只丢快照, 不致命)。
    pub fn spawn(self) {
        let _ = std::thread::Builder::new()
            .name("shell-snapshot-pump".into())
            .spawn(move || self.run());
    }

    fn run(mut self) {
        while !self.quit.load(Ordering::Relaxed) {
            std::thread::sleep(TICK);
            self.tick();
        }
    }

    fn push(&self, msg: serde_json::Value) {
        let _ = self.proxy.send_event(ShellEv::Push(msg.to_string()));
    }

    /// 推送已序列化好的报文 (快照去重路径: 避免二次序列化)。
    fn push_str(&self, raw: &str) {
        let _ = self.proxy.send_event(ShellEv::Push(raw.to_string()));
    }

    fn tick(&mut self) {
        // ① 托盘/主循环退出标志 (GUI 旧通道: should_exit → shell 优雅退出)
        if !self.quit_reported && self.backend.should_exit() {
            self.quit_reported = true;
            let _ = self.proxy.send_event(ShellEv::QuitRequested);
        }

        // ② hello 首帧 (页面加载后第一帧; 换壳/首次加载都会置位)
        if self.hello_pending.swap(false, Ordering::Relaxed) {
            self.push_hello();
            self.hello_sent.store(true, Ordering::Relaxed);
            // G 轮混合模式: overlay 型 MOD 的待注入资源随 hello 落地 (取走即清零;
            // 挂载时机 = overlay 应用/切换, 导航换页重新挂载 —— 页面重建后注入不丢,
            // 同页重 hello 不重复注入)
            let inject = self
                .shared
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .overlay_inject
                .take();
            if let Some(res) = inject {
                let _ = self
                    .proxy
                    .send_event(ShellEv::InjectScript(super::overlay_inject_script(&res)));
            }
        }

        // ③ 异步事件合并转发
        for ev in self.collect_events() {
            self.push(json!({ "t": "evt", "ev": ev }));
        }

        // ④ 外部 MOD 目录失效 → 回 classic (契约 §5 fallback)
        self.check_mod_fallback();

        // ⑤ 运行态快照推送 (契约 §3.1 v1.3 差量): 六域各自序列化一次
        //    (每域字符串同时用于 FNV 哈希与 envelope 组装, 不做二次序列化),
        //    last_snap 为空 (首帧/hello/换页后) = 全量; 否则只推变化域 patch。
        //    hello/换页清零 last_snap + domain_hashes (ShellShared) → 新页面必收一帧全量。
        let snap = self.backend.snapshot();
        let parts: [String; 6] = [
            to_json_str(&snap.core),
            to_json_str(&snap.input),
            to_json_str(&snap.vibration),
            to_json_str(&snap.audio),
            to_json_str(&snap.update),
            to_json_str(&snap.counts),
        ];
        let version_json = to_json_str(&snap.version);
        // ★M 轮卡顿修复: vibration 域的 now_ms 每拍必变（且前端根本不消费它），
        //   若参与差量哈希会导致该域永远判「变化」→ 每 50ms 强制全量 patch 一帧（≈40KB/s
        //   跨进程 evaluate + 前端全表 DOM 写入）。哈希时剔除 now_ms，发送仍用完整 parts
        //   （契约的 full 帧字段不变）——差量去重恢复正常，静止时六域全未变 = 不推。
        let mut cur = domain_hashes(&parts);
        cur[2] = fnv1a64(strip_now_ms(&parts[2]).as_bytes());
        let envelope = {
            let mut s = self.shared.lock().unwrap_or_else(|p| p.into_inner());
            if s.last_snap.is_none() {
                // 首帧/hello 后/换页 = 全量 (full:true; JS 侧整体替换缓存)
                let env = full_envelope(&parts, &version_json);
                s.last_snap = Some(fnv1a64(env.as_bytes()));
                s.domain_hashes = cur;
                Some(env)
            } else if let Some(env) = patch_envelope(&changed_mask(&s.domain_hashes, &cur), &parts) {
                // 有域变化 = 差量 patch (只装变化的域)
                s.domain_hashes = cur;
                Some(env)
            } else {
                None // 六域全未变 → 不推 evaluate
            }
        };
        if let Some(env) = envelope {
            self.push_str(&env);
        }
    }

    fn push_hello(&self) {
        let (shell, mod_id, mods, hybrid_mode) = {
            let mut s = self.shared.lock().unwrap_or_else(|p| p.into_inner());
            s.reset_snap_dedup(); // hello = 新页面首帧 → 清快照去重 (下拍必全量)
            let shell = if s.current == "classic" { "classic" } else { "mod" };
            let mod_id = if s.current == "classic" { None } else { Some(s.current.clone()) };
            (shell, mod_id, s.mods.clone(), s.hybrid_mode)
        };
        self.push(json!({
            "t": "hello",
            "version": crate::update::APP_VERSION,
            "shell": shell,
            "modId": mod_id,
            "mods": mods,
            "frameless": super::FRAMELESS,
            "dragMode": super::DRAG_MODE,
            "hybrid": hybrid_mode,
        }));
    }

    /// UiEvent 队列 + 既有按通道透传 (架构说明 §3.3) 统一成 evt 推送列表。
    /// 角色差异 (G 轮混合模式 §11): ModWindow 消费托盘 ShowWindow 还原本窗
    /// (egui 隐藏期停摆, 托盘还原不能等 egui); ShowAbout 不消费 —— 留给 egui,
    /// mod 停用还原后照常弹 (请求不丢)。
    fn collect_events(&mut self) -> Vec<crate::ui_api::UiEvent> {
        use crate::ui_api::UiEvent;
        let mut out: Vec<UiEvent> = self.backend.poll_events();

        // 托盘请求 (今天走 AppState 标志位, 未进 UiEvent 队列)
        if self.backend.take_show_window_request() {
            out.push(UiEvent::ShowWindow);
            // 托盘双击还原窗口: JS 侧渲染事件之外, 壳窗口本身要还原聚焦
            let _ = self.proxy.send_event(ShellEv::WinShow);
        }
        if self.role == PumpRole::Standalone && self.backend.take_show_about_request() {
            out.push(UiEvent::ShowAbout);
        }
        // 输入线程检出的预设切换 (FIFO)
        for name in self.backend.take_pending_preset_switches() {
            out.push(UiEvent::PresetSwitch(name));
        }
        // RawInput 捕获结果 (poll_pad_capture 与 poll_events 的 RawCaptured 二选一用)
        while let Some(device) = self.backend.poll_pad_capture() {
            out.push(UiEvent::RawCaptured(device));
        }
        // HID 激活请求 → 等到原始报文 (或超时) 后推 HidActivationData
        for req in self.backend.poll_hid_activation_requests() {
            self.hid_waiting.push((req.device_handle, 0));
        }
        let mut done: Vec<isize> = Vec::new();
        for (handle, waited) in self.hid_waiting.iter_mut() {
            if let Some(data) = self.backend.try_hid_activation_data(*handle) {
                out.push(UiEvent::HidActivationData { handle: *handle, data });
                done.push(*handle);
            } else {
                *waited += 1;
                if *waited >= HID_MAX_WAIT_TICKS {
                    out.push(UiEvent::HidActivationData { handle: *handle, data: Vec::new() });
                    done.push(*handle);
                }
            }
        }
        self.hid_waiting.retain(|(h, _)| !done.contains(h));
        out
    }

    /// 当前 shell 型外部包目录被删 → 回 classic (契约 §5 fallback):
    /// * 纯 shell 模式 = 页内导航 + ShellModFallback 事件 (旧行为不变);
    /// * 混合模式 mod 窗 (★G+2 修复, 与「⤶ 停用链接」同缺口家族) = **停用 MOD** ——
    ///   关 mod 窗还原 egui (egui 原生界面即"回 classic"的落点); 若仍页内导航回宿主,
    ///   会复现「宿主页滞留 mod 窗」缺口 (导航拦截器会拦, 但语义上包失效就该停用)。
    fn check_mod_fallback(&mut self) {
        let gone = {
            let s = self.shared.lock().unwrap_or_else(|p| p.into_inner());
            if s.current == "classic" || s.current == "endfield" {
                return; // 内置包不删, 无需检查
            }
            // 外部包: 清单里没了 / 目录或 entry 文件消失 = 失效
            let root = s.ext_dir.as_ref().map(|d| crate::shell::mods::mods_root_for_exe_dir(d));
            let entry = s
                .mods
                .iter()
                .find(|m| m.id == s.current)
                .and_then(|m| m.entry.clone());
            !match (&root, entry) {
                (Some(root), Some(entry)) => root.join(&s.current).join(entry).is_file(),
                _ => false,
            }
        };
        if gone {
            {
                let mut s = self.shared.lock().unwrap_or_else(|p| p.into_inner());
                s.current = "classic".to_string();
            }
            if self.role == PumpRole::ModWindow {
                self.backend.notify_info("外观 MOD 目录已失效, 已自动还原原生界面");
                let _ = self.proxy.send_event(ShellEv::StopWindow);
            } else {
                let _ = self.proxy.send_event(ShellEv::Navigate(super::CLASSIC_URL.into()));
                self.push(json!({ "t": "evt", "ev": "ShellModFallback" }));
                self.hello_pending.store(true, Ordering::Relaxed);
            }
        }
    }
}

/// 供 mod.rs 复用的 hello 消息构造 (IPC 首包兜底时用)。
pub fn hello_json(shared: &Mutex<ShellShared>) -> serde_json::Value {
    let mut s = shared.lock().unwrap_or_else(|p| p.into_inner());
    s.reset_snap_dedup(); // hello = 新页面首帧 → 清快照去重 (下拍必全量)
    let shell = if s.current == "classic" { "classic" } else { "mod" };
    let mod_id = if s.current == "classic" { None } else { Some(s.current.clone()) };
    json!({
        "t": "hello",
        "version": crate::update::APP_VERSION,
        "shell": shell,
        "modId": mod_id,
        "mods": s.mods,
        "frameless": super::FRAMELESS,
        "dragMode": super::DRAG_MODE,
        // ★H 轮 (契约 v1.5): 混合模式 mod 窗 = true —— 前端「完整/极简/经典模式」
        // 与宿主页「典/简」按钮据此走 shell.stop{mode} 真切换 (否则保持演示 toast)。
        "hybrid": s.hybrid_mode,
    })
}

/// serde 序列化兜底 (失败回 null 字面量; 单域序列化不该失败, 保底不 panic)。
fn to_json_str<T: serde::Serialize>(v: &T) -> String {
    serde_json::to_string(v).unwrap_or_else(|_| "null".into())
}

/// 六域 FNV 哈希 (输入 = 各域实际要发的序列化 JSON 文本)。
/// 域间独立: 改一个域只变该域哈希 → patch 只装该域 (见测试)。
/// ★M 轮: 从 vibration 域 JSON 文本里剔除 `now_ms` 键，供差量哈希使用。
/// 只影响「该域是否变化」的判定，不改动实际发送的文本（full/patch 载荷仍带 now_ms，契约不变）。
/// 解析失败时原样返回（安全降级：最坏退回旧行为，不会崩/不会丢帧）。
fn strip_now_ms(vib_json: &str) -> String {
    match serde_json::from_str::<serde_json::Value>(vib_json) {
        Ok(mut v) => {
            if let Some(obj) = v.as_object_mut() {
                obj.remove("now_ms");
            }
            v.to_string()
        }
        Err(_) => vib_json.to_string(),
    }
}

fn domain_hashes(parts: &[String; 6]) -> [u64; 6] {
    let mut out = [0u64; 6];
    for (i, p) in parts.iter().enumerate() {
        out[i] = fnv1a64(p.as_bytes());
    }
    out
}

/// 前后两轮域哈希比对 → 变化掩码 (true = 该域变化, 需进 patch)。
fn changed_mask(prev: &[u64; 6], cur: &[u64; 6]) -> [bool; 6] {
    let mut mask = [false; 6];
    for i in 0..6 {
        mask[i] = prev[i] != cur[i];
    }
    mask
}

/// patch envelope 组装: `{"t":"snap","patch":{<变化的域>:<该域序列化文本>}}`。
/// 全部未变 → None (不推)。键序照 SNAP_DOMAINS (确定性, 利于测试与 diff)。
fn patch_envelope(mask: &[bool; 6], parts: &[String; 6]) -> Option<String> {
    let mut body = String::new();
    for i in 0..6 {
        if !mask[i] {
            continue;
        }
        if !body.is_empty() {
            body.push(',');
        }
        body.push('"');
        body.push_str(SNAP_DOMAINS[i]);
        body.push_str("\":");
        body.push_str(&parts[i]);
    }
    if body.is_empty() {
        return None;
    }
    Some(format!("{{\"t\":\"snap\",\"patch\":{{{body}}}}}"))
}

/// full envelope 组装: `{"t":"snap","full":true,"data":{version,…六域}}`。
/// 键序与 `UiSnapshot` serde 字段序一致 (JS 侧按域对象消费, 顺序仅利于 diff 可读)。
fn full_envelope(parts: &[String; 6], version_json: &str) -> String {
    let mut s = String::with_capacity(512);
    s.push_str("{\"t\":\"snap\",\"full\":true,\"data\":{\"version\":");
    s.push_str(version_json);
    for (i, name) in SNAP_DOMAINS.iter().enumerate() {
        s.push_str(",\"");
        s.push_str(name);
        s.push_str("\":");
        s.push_str(&parts[i]);
    }
    s.push_str("}}");
    s
}

/// FNV-1a 64bit (无依赖确定性哈希; 只用于「内容是否变化」相等性判断,
/// 不做防碰撞安全用途)。
fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

#[cfg(test)]
mod tests {
    use super::{changed_mask, domain_hashes, fnv1a64, full_envelope, patch_envelope, SNAP_DOMAINS};
    use serde_json::Value;

    /// 快照去重哈希: 确定性 + 可区分不同内容。
    #[test]
    fn fnv1a64_stable_and_discriminating() {
        let a = br#"{"t":"snap","data":{"x":1}}"#;
        let b = br#"{"t":"snap","data":{"x":2}}"#;
        assert_eq!(fnv1a64(a), fnv1a64(a), "同内容必须同哈希");
        assert_ne!(fnv1a64(a), fnv1a64(b), "不同内容必须不同哈希");
        assert_ne!(fnv1a64(b""), fnv1a64(a));
    }

    /// 造一套六域序列化文本 (各域内容互不相同, 贴近真实快照形态)。
    fn sample_parts() -> [String; 6] {
        [
            r#"{"paused":false,"config_epoch":1}"#.into(),
            r#"{"live_hid_pad":null,"raw_capture_active":false}"#.into(),
            r#"{"enabled":true,"out_l":0,"out_r":0,"params":[1,2,3]}"#.into(),
            r#"{"enabled":false,"motor_l":0,"bands":[0,0,0,0,0]}"#.into(),
            r#"{"phase":"idle","done":0}"#.into(),
            r#"{"mappings":3,"turbo":0,"whitelist":1}"#.into(),
        ]
    }

    /// v1.3 六域哈希独立性: 改一个域只变该域哈希 → 该域独占 patch。
    #[test]
    fn domain_hashes_independent() {
        let base = domain_hashes(&sample_parts());
        // 六域内容互不相同 → 哈希两两不同 (防「域序列化相同导致哈希串域」)
        for i in 0..6 {
            for j in (i + 1)..6 {
                assert_ne!(base[i], base[j], "域 {}/{} 样本内容相同则测试失效", SNAP_DOMAINS[i], SNAP_DOMAINS[j]);
            }
        }
        // 只改 vibration 域
        let mut parts = sample_parts();
        parts[2] = r#"{"enabled":true,"out_l":43690,"out_r":21845,"params":[1,2,3]}"#.into();
        let cur = domain_hashes(&parts);
        let mask = changed_mask(&base, &cur);
        for i in 0..6 {
            assert_eq!(mask[i], SNAP_DOMAINS[i] == "vibration", "域 {} 掩码错", SNAP_DOMAINS[i]);
        }
        // 同内容 → 掩码全 false
        let again = domain_hashes(&sample_parts());
        assert!(changed_mask(&base, &again).iter().all(|c| !c));
    }

    /// v1.3 patch envelope 结构: 只装变化的域、t=snap、无 data/full 键; 全未变 → None。
    #[test]
    fn patch_envelope_structure() {
        let base = domain_hashes(&sample_parts());
        let mut parts = sample_parts();
        parts[2] = r#"{"enabled":true,"out_l":43690,"out_r":21845,"params":[1,2,3]}"#.into();
        parts[5] = r#"{"mappings":9,"turbo":0,"whitelist":1}"#.into();
        let cur = domain_hashes(&parts);
        let mask = changed_mask(&base, &cur);
        let env = patch_envelope(&mask, &parts).expect("有变化必须产出 patch");
        let v: Value = serde_json::from_str(&env).expect("patch envelope 必须是合法 JSON");
        assert_eq!(v.get("t").and_then(Value::as_str), Some("snap"));
        let patch = v.get("patch").and_then(Value::as_object).expect("patch 必须是对象");
        assert_eq!(patch.len(), 2, "只装变化的域");
        assert!(patch.contains_key("vibration"));
        assert!(patch.contains_key("counts"));
        assert!(v.get("data").is_none() && v.get("full").is_none(), "patch 帧不得夹带全量");
        assert_eq!(
            patch.get("vibration").and_then(Value::as_object).map(|o| o.get("out_l").cloned()),
            Some(Some(Value::from(43690)))
        );
        // 全部未变 → None (不推 evaluate)
        assert!(patch_envelope(&changed_mask(&cur, &cur), &parts).is_none());
    }

    /// ★M 轮: now_ms 抖动不得触发 vibration 域 patch（静止时零推送）。
    #[test]
    fn now_ms_jitter_does_not_trigger_patch() {
        let base_vib = r#"{"enabled":true,"out_l":100,"out_r":100,"now_ms":1000}"#;
        let jitter_vib = r#"{"enabled":true,"out_l":100,"out_r":100,"now_ms":1050}"#;
        assert_eq!(super::strip_now_ms(base_vib), super::strip_now_ms(jitter_vib), "now_ms 必须被剔除");
        // 真实参数变化仍要触发
        let real_change = r#"{"enabled":true,"out_l":200,"out_r":100,"now_ms":1050}"#;
        assert_ne!(super::strip_now_ms(base_vib), super::strip_now_ms(real_change), "真实参数变化必须保留");
        // 非法 JSON 安全降级为原样（不崩）
        assert_eq!(super::strip_now_ms("not json"), "not json");
    }

    /// v1.3 full envelope 结构: full:true + data 六域齐备 (含 version)。
    #[test]
    fn full_envelope_structure() {
        let parts = sample_parts();
        let env = full_envelope(&parts, "\"R2.14\"");
        let v: Value = serde_json::from_str(&env).expect("full envelope 必须是合法 JSON");
        assert_eq!(v.get("t").and_then(Value::as_str), Some("snap"));
        assert_eq!(v.get("full").and_then(Value::as_bool), Some(true));
        let data = v.get("data").and_then(Value::as_object).expect("data 必须是对象");
        assert_eq!(data.get("version").and_then(Value::as_str), Some("R2.14"));
        for name in SNAP_DOMAINS {
            assert!(data.contains_key(name), "全量帧缺域 {name}");
        }
        assert_eq!(data.len(), 7, "version + 六域");
    }
}
