//! UI 无关的应用契约层（API + 映射模式）。
//!
//! 这一层**不依赖 egui、不依赖浏览器**：只定义「界面能读到什么」「界面能发什么命令」
//! 「界面有哪些页面和动作」。任何渲染器（egui / 网页 / 以后的新 UI）都只跟这层打交道，
//! 换 UI 时后端一行不用改。
//!
//! * `UiSnapshot`：后端推给界面的只读状态。
//! * `UiCommand`：界面发给后端的动作；两套渲染器共用同一套 `id()`，网页端 `/api/action`
//!   的 act 字符串就是它。
//! * `UiSchema`：声明式映射表（页面 / 快捷动作 / 模式）。增删页面或改按钮只改这张表。
//! * `UiBackend`：渲染器调用的统一入口，由 egui 与 webui 各自实现（配置持有方式不同）。

use serde::Serialize;

/// 界面模式。属于 UI 状态，但放进契约层，方便任何渲染器一致切换。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UiMode {
    Full,
    Minimal,
    TurboOnly,
}

impl UiMode {
    pub const ALL: &'static [UiMode] = &[UiMode::Full, UiMode::Minimal, UiMode::TurboOnly];

    pub fn id(self) -> &'static str {
        match self {
            UiMode::Full => "full",
            UiMode::Minimal => "minimal",
            UiMode::TurboOnly => "turbo",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            UiMode::Full => "完整",
            UiMode::Minimal => "极简",
            UiMode::TurboOnly => "纯连发",
        }
    }

    pub fn command(self) -> UiCommand {
        match self {
            UiMode::Full => UiCommand::ModeFull,
            UiMode::Minimal => UiCommand::ModeMinimal,
            UiMode::TurboOnly => UiCommand::ModeTurboOnly,
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|m| m.id() == s)
    }
}

/// 界面 → 后端的所有动作。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UiCommand {
    TogglePause,
    Quit,
    Save,
    CheckUpdate,
    TestPulse,
    TestVibration,
    TestRank,
    AudioSelfCheck,
    AudioCalibrate,
    AudioCalibDismiss,
    ResetDevices,
    RefreshDevices,
    RestartThreads,
    ToggleTheme,
    ShowGuide,
    ModeFull,
    ModeMinimal,
    ModeTurboOnly,
}

impl UiCommand {
    pub const ALL: &'static [UiCommand] = &[
        UiCommand::TogglePause,
        UiCommand::Quit,
        UiCommand::Save,
        UiCommand::CheckUpdate,
        UiCommand::TestPulse,
        UiCommand::TestVibration,
        UiCommand::TestRank,
        UiCommand::AudioSelfCheck,
        UiCommand::AudioCalibrate,
        UiCommand::AudioCalibDismiss,
        UiCommand::ResetDevices,
        UiCommand::RefreshDevices,
        UiCommand::RestartThreads,
        UiCommand::ToggleTheme,
        UiCommand::ShowGuide,
        UiCommand::ModeFull,
        UiCommand::ModeMinimal,
        UiCommand::ModeTurboOnly,
    ];

    /// 与网页端 `/api/action` 的 `act` 字符串完全一致。
    pub fn id(self) -> &'static str {
        match self {
            UiCommand::TogglePause => "toggle-pause",
            UiCommand::Quit => "quit",
            UiCommand::Save => "save",
            UiCommand::CheckUpdate => "check-update",
            UiCommand::TestPulse => "test-pulse",
            UiCommand::TestVibration => "test-vibration",
            UiCommand::TestRank => "test-rank",
            UiCommand::AudioSelfCheck => "audio-self-check",
            UiCommand::AudioCalibrate => "audio-calibrate",
            UiCommand::AudioCalibDismiss => "audio-calib-dismiss",
            UiCommand::ResetDevices => "reset-devices",
            UiCommand::RefreshDevices => "refresh-devices",
            UiCommand::RestartThreads => "restart-threads",
            UiCommand::ToggleTheme => "toggle-theme",
            UiCommand::ShowGuide => "show-guide",
            UiCommand::ModeFull => "mode-full",
            UiCommand::ModeMinimal => "mode-minimal",
            UiCommand::ModeTurboOnly => "mode-turbo",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            UiCommand::TogglePause => "暂停 / 恢复",
            UiCommand::Quit => "退出",
            UiCommand::Save => "保存",
            UiCommand::CheckUpdate => "检查更新",
            UiCommand::TestPulse => "轻触反馈",
            UiCommand::TestVibration => "试震",
            UiCommand::TestRank => "档位自检",
            UiCommand::AudioSelfCheck => "音频自检",
            UiCommand::AudioCalibrate => "音频校准",
            UiCommand::AudioCalibDismiss => "关闭校准结果",
            UiCommand::ResetDevices => "重连手柄",
            UiCommand::RefreshDevices => "刷新设备",
            UiCommand::RestartThreads => "重启线程",
            UiCommand::ToggleTheme => "深浅主题",
            UiCommand::ShowGuide => "开局引导",
            UiCommand::ModeFull => "完整模式",
            UiCommand::ModeMinimal => "极简模式",
            UiCommand::ModeTurboOnly => "纯连发模式",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|c| c.id() == s)
    }

    pub fn mode(self) -> Option<UiMode> {
        match self {
            UiCommand::ModeFull => Some(UiMode::Full),
            UiCommand::ModeMinimal => Some(UiMode::Minimal),
            UiCommand::ModeTurboOnly => Some(UiMode::TurboOnly),
            _ => None,
        }
    }
}

/// 映射表里的一个页面。渲染器按 `id` 决定画什么内容，按 `zh/en/glyph` 画导航。
#[derive(Debug, Clone, Copy)]
pub struct UiPage {
    pub id: &'static str,
    pub zh: &'static str,
    pub en: &'static str,
    pub glyph: &'static str,
    pub hint: &'static str,
}

/// 映射表里的一个常驻快捷动作。
#[derive(Debug, Clone, Copy)]
pub struct UiAction {
    pub cmd: UiCommand,
    pub label: &'static str,
}

/// 声明式映射表：以后加页面、改按钮、加模式只改这里。
#[derive(Debug, Clone, Copy)]
pub struct UiSchema {
    pub pages: &'static [UiPage],
    pub actions: &'static [UiAction],
    pub modes: &'static [UiMode],
}

impl UiSchema {
    pub fn page(&self, id: &str) -> Option<UiPage> {
        self.pages.iter().copied().find(|p| p.id == id)
    }
}

pub static SCHEMA: UiSchema = UiSchema {
    pages: &[
        UiPage { id: "overview", zh: "概括", en: "OVERVIEW", glyph: "◈", hint: "运行态与分区总览" },
        UiPage { id: "gamepad", zh: "手柄", en: "GAMEPAD", glyph: "◎", hint: "可视化快速映射" },
        UiPage { id: "turbo", zh: "连发", en: "TURBO", glyph: "⟳", hint: "键盘 / 鼠标触发映射" },
        UiPage { id: "whitelist", zh: "白名单", en: "WHITELIST", glyph: "▤", hint: "进程过滤" },
        UiPage { id: "vibration", zh: "震动", en: "HAPTICS", glyph: "≈", hint: "通用震动调校" },
        UiPage { id: "jobpresets", zh: "全职业", en: "JOBS", glyph: "⌘", hint: "全职业预设" },
        UiPage { id: "audio", zh: "音频", en: "AUDIO", glyph: "♫", hint: "音频震动" },
        UiPage { id: "settings", zh: "设置", en: "SETTINGS", glyph: "⚙", hint: "常用开关与自检" },
        UiPage { id: "logs", zh: "日志", en: "LOGS", glyph: "≡", hint: "运行日志" },
    ],
    actions: &[
        UiAction { cmd: UiCommand::TogglePause, label: "暂停" },
        UiAction { cmd: UiCommand::TestVibration, label: "试震" },
        UiAction { cmd: UiCommand::AudioSelfCheck, label: "音频" },
        UiAction { cmd: UiCommand::Save, label: "保存" },
        UiAction { cmd: UiCommand::ShowGuide, label: "引导" },
    ],
    modes: UiMode::ALL,
};

/// 后端推给界面的只读状态快照。两套渲染器读同一份字段。
#[derive(Debug, Clone, Default, Serialize)]
pub struct UiSnapshot {
    pub version: String,
    pub paused: bool,
    pub worker_count: u32,
    pub mappings: usize,
    pub turbo: usize,
    pub presets: usize,
    pub whitelist: usize,
    pub vibration_enabled: bool,
    pub audio_enabled: bool,
}

/// 命令执行结果。
#[derive(Debug, Clone, Serialize)]
pub struct UiReply {
    pub ok: bool,
    pub message: String,
}

impl UiReply {
    pub fn ok(message: impl Into<String>) -> Self {
        Self { ok: true, message: message.into() }
    }

    pub fn fail(message: impl Into<String>) -> Self {
        Self { ok: false, message: message.into() }
    }
}

/// 所有渲染器共用的后端入口。
pub trait UiBackend {
    fn snapshot(&self) -> UiSnapshot;
    fn dispatch(&mut self, cmd: UiCommand) -> UiReply;
    fn schema(&self) -> &'static UiSchema {
        &SCHEMA
    }
}
