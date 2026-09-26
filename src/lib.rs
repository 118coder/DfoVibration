//! Core modules for the Sorahk auto key press application.
//!
//! This library exposes internal modules for testing purposes.
//! It is not intended for external use as a library.

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
pub mod util;
pub mod vibration;
pub mod xinput;

// Re-export types for test modules
pub use config::{AppConfig, KeyMapping};
pub use i18n::{CachedTranslations, Language};
