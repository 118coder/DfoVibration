//! 终末地风格的新外壳（纯 Rust / egui 渲染器）。
//!
//! 这一层只负责「按映射表画界面 + 把点击翻译成命令」，不写业务逻辑：
//! * 页面内容仍调用原有 egui 页面函数，便利性一点没丢；
//! * 常驻动作走 `ui_api::UiBackend::dispatch`，与网页端同一套命令；
//! * 以后换 UI = 换这个渲染器，`ui_api` 与后端都不用动。

pub mod dial;
pub mod palette;
mod backend;
mod shell;

pub use shell::render;

/// 新外壳的路由（与 `ui_api::SCHEMA.pages` 的 id 一一对应）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EfRoute {
    Overview,
    Gamepad,
    Turbo,
    Whitelist,
    Vibration,
    JobPresets,
    Audio,
    Settings,
    Logs,
}

impl EfRoute {
    pub fn id(self) -> &'static str {
        match self {
            EfRoute::Overview => "overview",
            EfRoute::Gamepad => "gamepad",
            EfRoute::Turbo => "turbo",
            EfRoute::Whitelist => "whitelist",
            EfRoute::Vibration => "vibration",
            EfRoute::JobPresets => "jobpresets",
            EfRoute::Audio => "audio",
            EfRoute::Settings => "settings",
            EfRoute::Logs => "logs",
        }
    }

    pub fn title(self) -> &'static str {
        crate::ui_api::SCHEMA
            .page(self.id())
            .map(|p| p.zh)
            .unwrap_or("概括")
    }

    pub fn parse(id: &str) -> Self {
        match id {
            "gamepad" => EfRoute::Gamepad,
            "turbo" => EfRoute::Turbo,
            "whitelist" => EfRoute::Whitelist,
            "vibration" => EfRoute::Vibration,
            "jobpresets" => EfRoute::JobPresets,
            "audio" => EfRoute::Audio,
            "settings" => EfRoute::Settings,
            "logs" => EfRoute::Logs,
            _ => EfRoute::Overview,
        }
    }
}
