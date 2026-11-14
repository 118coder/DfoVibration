//! Core modules for the Sorahk auto key press application.
//!
//! This library exposes internal modules for testing purposes.
//! It is not intended for external use as a library.

pub mod audio_haptics;
pub mod config;
pub mod gui;
pub mod hid_layout;
/* ★v24.19b 手柄隐藏暂时下线 (用户决策 2026-09-14: 全部注释, 避免影响主功能)。
 * ★2026-09-27 重构 G: 实现已整体移出编译目录 → docs/frozen/hidhide.rs。
 * 未来重启: 先把该文件移回 src/hidhide.rs, 再取消下行与 main.rs 的注释,
 * 并还原 config.rs / gui 三处接线 (详见 docs/frozen/hidhide.rs 文件头)。 */
// pub mod hidhide;
pub mod i18n;
pub mod input_manager;
pub mod input_ownership;
pub mod job_presets;
pub mod auto_inject;
pub mod rawinput;
pub mod sequence;
pub mod state;
/// ★v24.55 检查更新 (WinHTTP + 多源清单, 不强制、不自动替换 exe)
pub mod update;
pub mod util;
pub mod vibration;
/// 本地 Web 控制台 (Rust 后端 + 浏览器前端)。
pub mod webui;
pub mod xinput;
/// UI 无关的应用契约层（API + 映射模式）；egui 与网页端共用。
pub mod ui_api;

// Re-export types for test modules
pub use config::{AppConfig, KeyMapping};
pub use i18n::{CachedTranslations, Language};
