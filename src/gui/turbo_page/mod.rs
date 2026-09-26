//! 连发映射页 (原 gui/turbo_page.rs 2410 行, 2026-09-27 架构重构 C4 目录化)。
//! 拆分: page(入口+hero+映射列表+全局参数) / presets(预设管理+切换键捕获) /
//!       edit(行内编辑+编辑期捕获) / macros(通用宏卡) / tests。
//! 方法 pub(in crate::gui)/pub(super) 与拆分前可见语义等价。

mod edit;
mod macros;
mod page;
mod presets;
#[cfg(test)]
mod tests;
