//! 通用震动页 + 全职业预设页 (原 gui/vibration_page.rs 3577 行, 2026-09-27 架构重构 C1 目录化)。
//! 入口: page::render_vibration_full_page —— job_mode=false 渲染通用震动页, true 渲染全职业预设页。
//! 各文件方法 pub(in crate::gui): 与拆分前 pub(super)(=gui 内可见) 语义等价。

mod export;
mod hints;
mod page;
mod presets;
mod sections;
