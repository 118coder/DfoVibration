//! ★R2.14 前后端分离契约层 (ui_api)。
//!
//! 本模块是 UI 与后端之间**唯一的类型契约**：
//! * [`UiSnapshot`] —— 后端 → UI 的运行态快照 (每帧/每 tick 聚合一次，UI 只读)；
//! * [`UiEvent`]    —— 后端 → UI 的异步事件 (托盘请求/预设切换/捕获结果…)，
//!   经 [`crate::backend::BackendService::poll_events`] 取走；
//! * [`UiCommand`]  —— UI → 后端的命令 (带 id 字符串，为将来的 HTTP/WS 适配器保留
//!   与归档 webui 相同的 action 命名)；
//! * [`UiReply`]    —— 命令执行结果。
//!
//! 设计约束 (调研结论，见交接 §13)：
//! 1. **serde-first**：契约类型全部 `Serialize + Deserialize`，将来加 HTTP/SSE
//!    传输适配器时零改契约；`UiSnapshot` 里嵌入的 state 类型已在 `state/types.rs`
//!    补了 derive (纯属性添加，无行为变化)。
//! 2. **配置不进快照**：配置读取走 `Backend::with_config` (短借只读)，
//!    写入走 `Backend::update_config(level, …)` 三档提交 (Save/SaveVib/Reload)。
//!    快照只承载运行态 (引擎原子量聚合)，避免整份 AppConfig 每帧克隆。
//! 3. **热路径零接触**：快照只读既有 pub 原子量，命令只走既有写入口
//!    (`apply_vibration_config` / `reload_config` / `AudioHapState::apply_config`)。

use serde::{Deserialize, Serialize};

use crate::config::KeyMapping;
use crate::i18n::Language;
use crate::state::{InputDevice, LiveHidState, PresetSwitchBinding};

// ═══════════════════════════════════════════════════════════════════════
// UiSnapshot —— 运行态快照 (后端聚合，UI 只读)
// ═══════════════════════════════════════════════════════════════════════

/// 每帧一次的运行态聚合快照。字段全部来自引擎原子量 / 后端服务，
/// UI 渲染**只允许**从这里与 `with_config` 取数据。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UiSnapshot {
    pub version: String,
    pub core: CoreSnap,
    pub input: InputSnap,
    pub vibration: VibSnap,
    pub audio: AudioSnap,
    pub update: UpdateSnap,
    pub counts: CountsSnap,
}

/// 核心/全局运行态。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CoreSnap {
    pub paused: bool,
    pub language: Language,
    /// 配置代次 (每次成功 reload +1；排查缓存一致性用)
    pub config_epoch: u64,
    pub workers_configured: usize,
    pub workers_actual: usize,
    /// 看护线程放弃重启后登记的名字 (设备管理页提示用)
    pub stopped_threads: Vec<String>,
    /// 注入保险统计 (命中数, 透传数, 白名单外拦截?)
    pub injection_guard_hits: u64,
    pub injection_guard_passed: u64,
    pub injection_guard_whitelisted: bool,
    pub gp_calibrating: bool,
    /// 序列宏全局暂停
    pub sequence_paused: bool,
    /// 序列宏录制进行中
    pub key_record_active: bool,
    /// 震动 S1/S4 路线 (true = S1 ACT 老方案)
    pub vib_legacy_client: bool,
}

/// 输入/设备实时态。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct InputSnap {
    /// 正在实时识别的第三方手柄 (vid,pid)
    pub live_hid_pad: Option<(u16, u16)>,
    /// 该手柄最新实时状态
    pub live_hid: Option<LiveHidState>,
    /// XInput 实时输入位图 (vid, 按钮位图)
    pub live_xinput: Option<(u16, u32)>,
    /// RawInput 捕获模式进行中
    pub raw_capture_active: bool,
    pub raw_capture_mode: crate::state::CaptureMode,
    pub xinput_capture_mode: crate::config::XInputCaptureMode,
    /// 配置里存在语义 HID 触发键 (热路径开关状态)
    pub has_semantic_hid: bool,
}

/// 震动引擎实时态 (S1/S4 共用；全部来自 AppState pub 原子)。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct VibSnap {
    pub enabled: bool,
    pub connected: bool,
    pub font_hits: bool,
    pub advanced_enabled: bool,
    pub independent_test: bool,
    /// S1 输出引擎模式 (0 经典 / 1 S4 纯 / 2 S4+不丢)
    pub legacy_output_mode: u32,
    pub events_received: u64,
    pub rank_events: u64,
    pub rank_last: u32,
    /// 运行参数镜像 (60 槽，与引擎读数同源)
    pub params: Vec<u32>,
    /// 基础项 L/R 权重 (26 项 × [L,R])
    pub item_lr: Vec<u32>,
    /// 评分事件 L/R 权重 (15 × 2)
    pub rank_lr: Vec<u32>,
    /// 评分细分强度 (15 项)
    pub rank_type_gain: Vec<u32>,
    /// 评分细分累计触发次数 (15 项)
    pub rank_type_events: Vec<u64>,
    pub rank_level_gain: u32,
    pub rank_duration: u32,
    /// 实时输出电平 (0-65535)
    pub out_l: u32,
    pub out_r: u32,
    pub rank_out: Vec<u32>,
    pub item_out: Vec<u32>,
    /// 测试震动截止时刻 (now_ms 口径；0 = 无)
    pub test_until_ms: u64,
    /// 快照时刻 (now_ms 口径；UI 用它判测试是否进行中)
    pub now_ms: u64,
    /// 职业算法 id 与参数 [4]
    pub algo_id: u32,
    pub algo_ap: [u32; 4],
}

/// 音频震动实时态。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AudioSnap {
    pub enabled: bool,
    /// 引擎状态名 (audio_status_name 口径)
    pub status: String,
    /// 合流后马达输出 (0-65535)
    pub motor_l: u16,
    pub motor_r: u16,
    /// 频带平滑 RMS (5 频带)
    pub bands: [f32; 5],
    pub band_peak: [f32; 5],
    /// 引擎目标输出 (0..1)
    pub level_l: f32,
    pub level_r: f32,
    /// 内容判定模式 (mapping::MODE_*: 0 游戏 1 BGM 2 人声 3 安静)
    pub mode: u8,
    pub noise_floor: f32,
    pub transient: f32,
    pub threshold_eff: f32,
    /// 白名单可观测性
    pub wl_entries: usize,
    pub wl_source: String,
    pub wl_foreground: String,
    pub wl_matched: bool,
    pub wl_degraded: bool,
    pub wl_error: String,
    /// 马达自检进行中
    pub self_check_active: bool,
    /// 屏蔽校准结果 (request_calibration 期间引擎写入；0 = 无)
    pub calib_result: u32,
}

/// 检查更新快照 (serde 友好镜像；进程内另可取原始
/// [`crate::update::Snapshot`]，见 Backend::update_snapshot)。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UpdateSnap {
    pub phase: String,
    pub latest_version: Option<String>,
    pub latest_notes: Option<String>,
    pub error: Option<String>,
    pub done: u64,
    pub total: u64,
    pub active_channel: Option<String>,
    pub saved_path: Option<String>,
    pub source: Option<String>,
    pub last_check_unix: i64,
    pub next_check_unix: i64,
}

/// 配置驱动的计数聚合 (UI 徽标/速览用)。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CountsSnap {
    pub mappings: usize,
    pub turbo: usize,
    pub combos: usize,
    pub presets: usize,
    pub whitelist: usize,
    pub macros: usize,
}

// ═══════════════════════════════════════════════════════════════════════
// UiEvent —— 后端 → UI 异步事件
// ═══════════════════════════════════════════════════════════════════════

/// 后端攒着、UI 每帧 `poll_events()` 取走的一次性事件。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum UiEvent {
    /// 托盘双击/菜单 → 显示主窗口
    ShowWindow,
    /// 托盘菜单 → 关于对话框
    ShowAbout,
    /// 输入线程触发的预设切换 (按名；UI 查表真正切换)
    PresetSwitch(String),
    /// RawInput 捕获模式抓到一帧设备输入
    RawCaptured(InputDevice),
    /// HID 激活流程回传的原始报文 (device_handle, data)
    HidActivationData { handle: isize, data: Vec<u8> },
}

// ═══════════════════════════════════════════════════════════════════════
// UiCommand —— UI → 后端命令
// ═══════════════════════════════════════════════════════════════════════

/// UI → 后端的离散命令。带参命令的载荷一律复用 config/state 类型，
/// 保证「契约即持久化模型」，不另造镜像结构。
///
/// id 字符串沿用归档 webui 的 action 命名 (小写连字符)，无参命令可由
/// [`UiCommand::from_action`] 反解，为将来 HTTP `/api/action` 免费铺路。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum UiCommand {
    // ── 全局 ──
    TogglePause,
    SetPaused(bool),
    /// Ctrl+S：全部落盘
    SaveAll,
    Quit,
    SetLanguage(Language),
    SetDarkMode(bool),
    SetAlwaysOnTop(bool),
    SetTrayEnabled(bool),
    SetNotificationsEnabled(bool),
    /// 切完整/极简/纯连发 (None = 不动该项)
    SetMode {
        minimal: Option<bool>,
        classic: Option<bool>,
    },
    ShowGuide,

    // ── 震动 ──
    SetVibrationEnabled(bool),
    /// test-pulse：1 秒轻震 (极简模式提示音配合)
    TestPulse,
    TestVibration,
    TestRank,
    /// 切换 S1 老方案 / S4 新方案 (两份路线文件互换)
    SwitchVibEdition { legacy: bool },

    // ── 音频震动 ──
    AudioSelfCheck,
    AudioCalibrate,
    AudioCalibDismiss,

    // ── 设备/线程 ──
    RefreshDevices,
    ResetDevices,
    /// 重启看护线程 (参数 = util 看护名单里的线程名，如 "audio_engine")
    RestartThreads(String),

    // ── 更新 ──
    CheckUpdate,
    CancelDownload,
    DismissUpdateError,

    // ── 映射预设 ──
    ApplyMappingPreset { name: String },
    SaveMappingPreset { name: String },
    DeleteMappingPreset { name: String },
    /// 整条映射的新增/替换 (index = Some 时按位替换)
    UpsertMapping {
        index: Option<usize>,
        mapping: KeyMapping,
    },
    DeleteMapping(usize),
}

impl UiCommand {
    /// 无参命令的 action id (与归档 webui 对齐，便于将来 HTTP 直用)。
    pub fn id(&self) -> &'static str {
        match self {
            UiCommand::TogglePause => "toggle-pause",
            UiCommand::SaveAll => "save",
            UiCommand::Quit => "quit",
            UiCommand::TestPulse => "test-pulse",
            UiCommand::TestVibration => "test-vibration",
            UiCommand::TestRank => "test-rank",
            UiCommand::AudioSelfCheck => "audio-self-check",
            UiCommand::AudioCalibrate => "audio-calibrate",
            UiCommand::AudioCalibDismiss => "audio-calib-dismiss",
            UiCommand::ResetDevices => "reset-devices",
            UiCommand::RefreshDevices => "refresh-devices",
            UiCommand::CheckUpdate => "check-update",
            UiCommand::ShowGuide => "show-guide",
            _ => "",
        }
    }

    /// 由 action 字符串反解无参命令 (带参命令返回 None)。
    pub fn from_action(act: &str) -> Option<UiCommand> {
        let cmd = match act {
            "toggle-pause" => UiCommand::TogglePause,
            "save" => UiCommand::SaveAll,
            "quit" => UiCommand::Quit,
            "test-pulse" => UiCommand::TestPulse,
            "test-vibration" => UiCommand::TestVibration,
            "test-rank" => UiCommand::TestRank,
            "audio-self-check" => UiCommand::AudioSelfCheck,
            "audio-calibrate" => UiCommand::AudioCalibrate,
            "audio-calib-dismiss" => UiCommand::AudioCalibDismiss,
            "reset-devices" => UiCommand::ResetDevices,
            "refresh-devices" => UiCommand::RefreshDevices,
            "check-update" => UiCommand::CheckUpdate,
            "show-guide" => UiCommand::ShowGuide,
            _ => return None,
        };
        Some(cmd)
    }
}

/// 命令执行结果。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UiReply {
    pub ok: bool,
    pub message: Option<String>,
}

impl UiReply {
    pub fn ok() -> Self {
        Self {
            ok: true,
            message: None,
        }
    }
    /// 带提示的成功 (如 toggle 后的新状态)。
    pub fn ok_with(msg: impl Into<String>) -> Self {
        Self {
            ok: true,
            message: Some(msg.into()),
        }
    }
    pub fn err(msg: impl Into<String>) -> Self {
        Self {
            ok: false,
            message: Some(msg.into()),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 提交档位 —— 配置写入的三档语义 (与 GUI 既有行为一一对应)
// ═══════════════════════════════════════════════════════════════════════

/// [`crate::backend::BackendService::update_config`] 的提交档位。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Commit {
    /// 只改内存 (高频滑杆路径配合 `mark_*_dirty` 去抖落盘)
    Memory,
    /// 立即写 Config.toml (+ 路线震动文件，save_to_file 既有语义)
    Save,
    /// 立即只写路线震动文件 (Vibration.toml / Vibration-ACT.toml)
    SaveVib,
    /// 立即落盘 + 两阶段热重载 + 运行态重挂 (预设切换键/语言等)
    Reload,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_roundtrip_and_ids() {
        let cmds = vec![
            UiCommand::TogglePause,
            UiCommand::SaveAll,
            UiCommand::Quit,
            UiCommand::TestPulse,
            UiCommand::TestVibration,
            UiCommand::TestRank,
            UiCommand::AudioSelfCheck,
            UiCommand::AudioCalibrate,
            UiCommand::AudioCalibDismiss,
            UiCommand::ResetDevices,
            UiCommand::RefreshDevices,
            UiCommand::CheckUpdate,
            UiCommand::ShowGuide,
        ];
        for c in &cmds {
            let id = c.id();
            assert!(!id.is_empty(), "{c:?} 缺 id");
            assert!(
                matches!(UiCommand::from_action(id), Some(ref x) if std::mem::discriminant(x) == std::mem::discriminant(c)),
                "{id} 反解失败"
            );
            let json = serde_json::to_string(c).unwrap();
            let back: UiCommand = serde_json::from_str(&json).unwrap();
            assert!(std::mem::discriminant(&back) == std::mem::discriminant(c));
        }
    }

    #[test]
    fn snapshot_serde_roundtrip() {
        let mut s = UiSnapshot::default();
        s.core.paused = true;
        s.vibration.params = vec![1, 2, 3];
        s.input.live_hid = Some(LiveHidState {
            vid: 0x045e,
            pid: 0x028e,
            buttons: 0b1010,
            dpad: 1,
            ls: 2,
            rs: 3,
            lt: true,
            rt: false,
            raw_position: 0xdead,
        });
        let json = serde_json::to_string(&s).unwrap();
        let back: UiSnapshot = serde_json::from_str(&json).unwrap();
        assert!(back.core.paused);
        assert_eq!(back.vibration.params, vec![1, 2, 3]);
        assert_eq!(back.input.live_hid.unwrap().vid, 0x045e);
    }

    #[test]
    fn event_serde_roundtrip() {
        for ev in [
            UiEvent::ShowWindow,
            UiEvent::ShowAbout,
            UiEvent::PresetSwitch("A".into()),
            UiEvent::HidActivationData {
                handle: 7,
                data: vec![1, 2],
            },
        ] {
            let json = serde_json::to_string(&ev).unwrap();
            let back: UiEvent = serde_json::from_str(&json).unwrap();
            assert!(std::mem::discriminant(&back) == std::mem::discriminant(&ev));
        }
    }
}
