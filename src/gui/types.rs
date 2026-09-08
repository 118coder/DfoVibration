//! GUI type definitions.

/// 主窗口页面 (顶部选项卡)。
///
/// 新增页面: 在此添加变体并加入 [`Page::TABS`], 选项卡栏与页面分发自动生效。
/// 顺序即选项卡顺序, 手柄映射放第一位 (用户最常用)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    /// 🎮 手柄可视化快速映射
    Gamepad,
    /// 连发映射 (键盘/鼠标触发 → 目标键)
    Turbo,
    /// 白名单 (进程过滤管理)
    Whitelist,
    /// 通用型震动设定
    Vibration,
    /// 全职业预设 (震动)
    JobPresets,
}

impl Page {
    /// 选项卡从左到右的固定顺序。
    pub const TABS: &'static [Page] = &[Page::Gamepad, Page::Turbo, Page::Whitelist, Page::Vibration, Page::JobPresets];

    /// 导航文案 (左侧选项卡; 2026-09-08 用户定稿: 除手柄映射外带「修改」后缀)。
    pub fn label(self) -> &'static str {
        match self {
            Page::Gamepad => "手柄映射",
            Page::Turbo => "连发映射修改",
            Page::Whitelist => "白名单修改",
            Page::Vibration => "通用震动修改",
            Page::JobPresets => "全职业预设修改",
        }
    }

    /// 页面大标题 (内容页头部, 不带「修改」后缀)。
    pub fn title(self) -> &'static str {
        match self {
            Page::Gamepad => "手柄映射",
            Page::Turbo => "连发映射",
            Page::Whitelist => "白名单",
            Page::Vibration => "通用震动",
            Page::JobPresets => "全职业预设",
        }
    }

    /// 导航矢量图标。
    pub fn icon(self) -> crate::gui::widgets::Icon {
        match self {
            Page::Gamepad => crate::gui::widgets::Icon::Gamepad,
            Page::Turbo => crate::gui::widgets::Icon::Bolt,
            Page::Whitelist => crate::gui::widgets::Icon::Shield,
            Page::Vibration => crate::gui::widgets::Icon::Wave,
            Page::JobPresets => crate::gui::widgets::Icon::Wand,
        }
    }

    /// 选项卡下方的页面级提示语。
    pub fn hint(self) -> &'static str {
        match self {
            Page::Gamepad => "点击手柄按键设置映射, 修改后自动保存并立即生效",
            Page::Turbo => "连发映射与全局配置, 由切换键控制启停",
            Page::Whitelist => "只允许列表内进程使用连发, 防止误触其他程序",
            Page::Vibration => "震动参数实时生效, 进图自动驱动",
            Page::JobPresets => "按职业加载震动预设, 滑块修改自动保存到 JobVibration.toml",
        }
    }
}

/// Key capture mode for keyboard input handling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyCaptureMode {
    None,
    ToggleKey,
    MappingTrigger(usize),
    MappingTarget(usize),
    NewMappingTrigger,
    NewMappingTarget,
    /// 手柄可视化页: 捕获“物理手柄按键”作为某个槽位的触发键
    QuickGamepadTrigger(usize),
    /// 手柄可视化页: 捕获“键盘/鼠标按键”作为某个槽位的目标键
    QuickGamepadTarget(usize),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_key_capture_mode_pattern_matching() {
        // Test extracting index from MappingTrigger variant
        let mode = KeyCaptureMode::MappingTrigger(7);

        match mode {
            KeyCaptureMode::MappingTrigger(idx) => {
                assert_eq!(idx, 7);
            }
            _ => panic!("Pattern matching failed"),
        }

        // Test extracting index from MappingTarget variant
        let mode = KeyCaptureMode::MappingTarget(42);

        match mode {
            KeyCaptureMode::MappingTarget(idx) => {
                assert_eq!(idx, 42);
            }
            _ => panic!("Pattern matching failed"),
        }
    }

    #[test]
    fn test_key_capture_mode_discriminant() {
        // Test that variants with different indices are distinguished
        let trigger_0 = KeyCaptureMode::MappingTrigger(0);
        let trigger_1 = KeyCaptureMode::MappingTrigger(1);
        let target_0 = KeyCaptureMode::MappingTarget(0);

        assert_ne!(
            trigger_0, trigger_1,
            "Different indices should not be equal"
        );
        assert_ne!(
            trigger_0, target_0,
            "Different variants should not be equal"
        );
    }
}
