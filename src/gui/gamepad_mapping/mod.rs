//! 手柄可视化快速映射页 (原 gui/gamepad_mapping.rs 3178 行, 2026-09-27 架构重构 C3 目录化)。
//!
//! 左侧渲染内嵌的 Xbox 手柄 SVG, 按键区域做成可点击热点; 右侧显示选中槽位的当前映射,
//! 支持两步快速设置 (捕获手柄键 → 捕获键盘/鼠标键)。
//! 热点坐标标定: SVG viewBox "22 135 545 401" (无文字横版); 修改 resources/gamepad.svg
//! 后必须重新标定 model.rs 的 SLOTS。
//!
//! 拆分: model(槽位模型) / capture(捕获对账器) / helpers(配置读写) / svg(光栅化) /
//!       page(页面主体) / quick_connect(右栏快捷卡) / flow(GpFlow 状态机) / panel(槽位面板) / tests。
//! 顶层 pub 项经 pub use 再导出, crate::gui::gamepad_mapping::X 路径保持不变。

mod capture;
mod flow;
mod helpers;
mod model;
mod panel;
mod page;
mod quick_connect;
mod svg;
#[cfg(test)]
mod tests;

pub use capture::*;
pub use helpers::*;
pub use model::*;
pub use svg::*;
