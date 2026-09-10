//! GUI module for application interface components.
//!
//! This module provides the graphical user interface using the `egui` framework,
//! including the main window, dialogs, and utility functions.

mod about_dialog;
mod device_info;
pub mod device_manager_dialog;
mod error_dialog;
mod fonts;
mod gamepad_mapping;
mod hid_activation_dialog;
mod guide;
mod main_window;
mod minimal;
mod turbo_page;
mod vibration_page;
mod whitelist_page;
mod mouse_direction_dialog;
mod mouse_scroll_dialog;
mod settings_dialog;
mod classic_mode;
mod theme;
mod types;
mod widgets;
pub(crate) mod utils;

use crate::config::AppConfig;
use crate::gui::theme::Theme;
use crate::gui::types::{KeyCaptureMode, Page};
use crate::i18n::CachedTranslations;
use crate::state::AppState;
use eframe::egui;
use smallvec::SmallVec;
use std::sync::Arc;

pub use error_dialog::show_error;

/// Pre-parsed switch key configuration.
#[derive(Clone, Debug)]
enum ParsedSwitchKey {
    /// No switch key configured
    None,
    /// Single key
    Single(u32),
    /// Key combination with modifiers
    Combo {
        /// Modifier flags: bit0=ctrl, bit1=shift, bit2=alt
        modifiers: u8,
        /// Main keys (VK codes)
        keys: SmallVec<[u32; 4]>,
    },
}

/// ★v20.3: 手柄快速捕获待确认项 (升级自 `(usize, bool, String)` 元组)。
/// 确认条上可直接勾选【连发】【1×双击】, 与连发映射区的同名开关写同一字段。
#[derive(Debug, Clone, PartialEq)]
pub struct QuickGamepadPending {
    /// 手柄槽位 id (SLOTS 下标)
    pub slot_id: usize,
    /// true = 本次捕获的是槽位触发键, false = 目标键
    pub is_trigger: bool,
    /// 捕获到的输入名 (如 "GAMEPAD_045E_A" / "F" / "LBUTTON")
    pub input: String,
    /// 确认时写入该槽位映射的 连发 开关 (新建默认 true = 沿用旧行为)
    pub turbo: bool,
    /// 确认时写入该槽位映射的 1×双击 开关 (默认 false)
    pub double_tap: bool,
    /// ★v21.0 确认时写入该槽位映射的 奔跑 开关 (默认 false; 勾选后连发/双击失效)
    pub run: bool,
}

/// Main GUI application structure.
///
/// Manages the application window state, dialogs, and user interactions.
pub struct SorahkGui {
    /// Shared application state
    app_state: Arc<AppState>,
    /// Application configuration
    config: AppConfig,
    /// Cached translations for rendering
    translations: CachedTranslations,
    /// 当前激活的页面 (顶部选项卡, 见 types::Page)
    pub active_page: Page,
    /// 手柄 SVG 渲染后的纹理
    pub gamepad_texture: Option<egui::TextureHandle>,
    /// gamepad_texture 对应的主题 (false=亮色变体, true=深色变体)
    pub gamepad_texture_dark: bool,
    /// 当前选中的手柄槽位
    pub gamepad_selected_slot: Option<usize>,
    pub vib_preset_idx: usize,
    pub vib_preset_name: String,
    /// 全职业预设 (v21): 基础职业/转职选择索引
    pub vib_job_base: usize,
    pub vib_job_class: usize,
    /// 已应用的职业 (基础职业, 转职)
    pub vib_job_active: Option<(String, String)>,
    /// 已加载预览的职业索引 (用于检测切换, 刷新专属设置)
    pub vib_job_loaded: Option<(usize, usize)>,
    /// 全职业预设应用开关 (开=应用, 关=不应用; 存 JobVibration.toml applied)
    pub vib_job_enabled: bool,
    /// 上次保存的职业参数快照 (用于检测滑块修改后自动落盘)
    pub vib_job_snapshot: Vec<u32>,
    /// 全职业预设页: 高级调校允许开关 (默认开, 独立于全局 advanced_enabled)
    pub vib_job_adv_on: bool,
    /// ★v20: 高级调校内的「专家参数」显示开关 (会话内瞬态, 默认关) ——
    /// 关时只显示各算法的高层旋钮, 低频微调收进专家模式, 减少参数噪音
    pub vib_show_expert: bool,
    /// 导出提示 (显示最近一次导出文件名)
    pub vib_export_msg: Option<String>,
    /// 导入文件选择索引 (通用震动设定页 / 全职业预设页共用布局)
    pub vib_import_sel: usize,
    pub job_import_sel: usize,
    /// 导入提示
    pub vib_import_msg: Option<String>,
    /// Close confirmation dialog visibility
    show_close_dialog: bool,
    /// Settings dialog visibility
    show_settings_dialog: bool,
    /// About dialog visibility
    show_about_dialog: bool,
    /// 使用说明浮窗 (标题栏「?」触发; 首次运行由 guide_seen 驱动)
    pub show_guide: bool,
    /// 首次运行 DFO 询问已回答 (会话内瞬态, 不持久化)
    dfo_ask_answered: bool,
    /// 标题栏「?」触发的首次流程重跑 (旁路 guide_seen, 让玩家重选 DFO/版本;
    /// 会话内瞬态, 使用说明「开始使用」后清除)
    pub first_run_rerun: bool,
    /// 首次运行 · 第 1.5 弹「客户端版本」询问窗 (仅 DFO 玩家; 选完写入
    /// config.vib_legacy_client / vib_edition_asked 并进使用说明)
    pub show_edition_ask: bool,
    /// ★v19: 向导弹窗串接延迟帧数 —— 新弹窗晚一帧渲染, 防"同帧点击穿透"
    /// (一次点击被刚弹出的新窗口按钮再吃一遍, 导致版本询问被瞬间跳过)
    pub modal_defer: u8,
    /// 白名单页: 添加输入草稿 + 错误提示 (会话内瞬态)
    new_whitelist_name: String,
    whitelist_error: Option<String>,
    /// 手柄快速捕获待确认项。
    /// 捕获不再立即写映射, 需用户在槽位面板点「确认应用」(防误操作)。
    /// ★v20.3: 升级为结构体 —— 确认条上可直接勾选【连发】【1×双击】(与连发映射区同字段)。
    pub quick_gamepad_pending: Option<QuickGamepadPending>,
    /// ★v20.3: 连发页「新增映射」置顶后滚动到编辑面板 (一次性标志)
    pub scroll_to_edit_row: bool,
    /// ★v20.3: 经典模式页当前子页签 (0=连发映射 1=通用型震动设定 2=全职业预设)
    pub classic_tab: usize,
    /// ★v20.4: 经典模式窗口 (与极简模式同级, 820×600 复刻老宿主; config.classic_mode 默认 true)
    pub classic_mode: bool,
    /// 经典模式窗口矩形记忆 (运行时)
    pub classic_window_rect: Option<(egui::Pos2, egui::Vec2)>,
    /// 经典模式位置锁存帧数 (与 InnerSize 竞争时校正)
    pub classic_pos_latch: u8,
    /// ★v20.3: 预设切换键的逐键按压状态 (与 config.presets 下标一一对应, 边缘检测用)
    pub preset_switch_key_states: Vec<bool>,
    /// ★v20.3: 预设切换键编辑 — 目标预设下标 / 键名输入 / 冲突错误
    pub preset_key_target: usize,
    pub preset_key_input: String,
    pub preset_key_error: Option<String>,
    /// Device manager dialog visibility
    show_device_manager: bool,
    /// Device manager dialog
    device_manager_dialog: Option<device_manager_dialog::DeviceManagerDialog>,
    /// HID device activation dialog
    hid_activation_dialog: Option<hid_activation_dialog::HidActivationDialog>,
    /// HID activation dialog creation time (for 10ms debounce)
    hid_activation_creation_time: Option<std::time::Instant>,
    /// Mouse direction selection dialog
    mouse_direction_dialog: Option<mouse_direction_dialog::MouseDirectionDialog>,
    /// Mouse scroll selection dialog
    mouse_scroll_dialog: Option<mouse_scroll_dialog::MouseScrollDialog>,
    /// Index of mapping being edited for mouse direction (None for new mapping)
    mouse_direction_mapping_idx: Option<usize>,
    /// Index of mapping being edited for mouse scroll (None for new mapping)
    mouse_scroll_mapping_idx: Option<usize>,
    /// Whether to minimize to tray on close
    minimize_on_close: bool,
    /// Current theme mode
    dark_mode: bool,
    /// Temporary config during settings edit
    temp_config: Option<AppConfig>,
    /// New mapping trigger key input
    new_mapping_trigger: String,
    /// New mapping target key input (single key for capture)
    new_mapping_target: String,
    /// New mapping target keys (multiple keys)
    new_mapping_target_keys: Vec<String>,
    /// New mapping interval input
    new_mapping_interval: String,
    /// New mapping duration input
    new_mapping_duration: String,
    /// New mapping turbo enabled state
    new_mapping_turbo: bool,
    /// New mapping double-tap enabled state (v22.2: DNF run on first press)
    new_mapping_double_tap: bool,
    /// New mapping move speed input
    new_mapping_move_speed: String,
    /// New mapping note input
    new_mapping_note: String,
    /// New process name input
    new_process_name: String,
    /// Current key capture state
    key_capture_mode: KeyCaptureMode,
    /// Flag to prevent re-entering capture mode immediately after capturing
    just_captured_input: bool,
    /// Keys currently pressed during capture (VK codes)
    capture_pressed_keys: std::collections::HashSet<u32>,
    /// Keys that were pressed when capture mode started (noise baseline)
    capture_initial_pressed: std::collections::HashSet<u32>,
    /// Pre-parsed switch key configuration
    parsed_switch_key: ParsedSwitchKey,
    /// VK key states for switch key detection (16 bits using modulo mapping)
    last_vk_state: u16,
    /// Close dialog highlight expiration time
    dialog_highlight_until: Option<std::time::Instant>,
    /// Pause state before entering settings
    was_paused_before_settings: Option<bool>,
    /// Error message for duplicate mapping
    duplicate_mapping_error: Option<String>,
    /// Error message for duplicate process
    duplicate_process_error: Option<String>,
    /// Preset save name input visibility
    show_preset_name_input: bool,
    /// Preset save name input text
    preset_name_input: String,
    /// Preset rename target name (old name to replace)
    preset_rename_target: String,
    /// Preset rename input text
    preset_rename_input: String,
    /// 极简模式: 只保留 连发/震动 两个总开关
    pub minimal_mode: bool,
    /// 顶栏预设下拉的实测高度 (跨帧记忆, 用于等高包裹使其垂直居中)
    pub preset_combo_h: Option<f32>,
    /// 完整模式窗口矩形记忆 (逻辑 pos+size, 退出极简时精确还原)
    pub normal_window_rect: Option<(egui::Pos2, egui::Vec2)>,
    /// 极简模式小窗矩形记忆 (逻辑 pos+size, 再进极简时各回各位)
    pub minimal_window_rect: Option<(egui::Pos2, egui::Vec2)>,
    /// 极简位置还原锁存帧数 (进入极简后前 12 帧持续校正位置, 抵御与 InnerSize 的竞争)
    pub minimal_pos_latch: u8,
    /// 极简全职业两级 combo 块的实测宽度 (跨帧记忆, 用于精确居中)
    pub minimal_job_block_w: f32,
    /// 极简模式震动预设来源: false=通用预设 / true=全职业预设 (互斥)
    pub minimal_vib_preset_job: bool,
    /// ★60 槽滑块自动落盘脏标 (v16.2): 拖动即镜像进 config.vibration 并记时刻,
    /// 主循环静默 1 秒后统一写 Vibration.toml (去抖, 防逐帧写盘)。仅非职业模式。
    pub vib_params_dirty_since: Option<std::time::Instant>,
    /// 连发映射页预设管理卡状态
    pub page_preset_name_input: String,
    pub page_preset_rename_input: String,
    pub page_preset_rename_show: bool,
    pub page_preset_delete_arm: bool,
    /// 进入极简前的窗口尺寸 (退出时恢复)
    normal_window_size: Option<egui::Vec2>,
    /// 进入极简前窗口是否最大化 (退出时恢复)
    normal_was_maximized: bool,
    /// 连发页内联编辑: 正在编辑的映射下标
    pub edit_mapping_idx: Option<usize>,
    /// 进入编辑时的映射快照 (取消时还原)
    pub edit_mapping_snapshot: Option<crate::config::KeyMapping>,
    /// 该映射是否为本次新增 (取消时删除)
    pub edit_mapping_is_new: bool,
    /// 上一帧窗口聚焦状态 (用于检测后台→前台跳变, 驱动白帧 workaround)
    was_focused: bool,
    /// Cached dark theme style (visuals + type scale + spacing)
    cached_dark_style: egui::Style,
    /// Cached light theme style
    cached_light_style: egui::Style,
}

impl SorahkGui {
    /// Creates a new GUI instance with the given state and configuration.
    pub fn new(app_state: Arc<AppState>, config: AppConfig) -> Self {
        let dark_mode = config.dark_mode;
        let translations = CachedTranslations::new(config.language);
        let cached_dark_style = Self::create_dark_style();
        let cached_light_style = Self::create_light_style();
        let parsed_switch_key = Self::parse_switch_key(&config.switch_key);
        let minimal_mode = config.minimal_mode;
        /* 经典模式默认开, 但显式设置的极简模式优先 (双标记同真时极简赢) */
        let classic_mode = config.classic_mode && !config.minimal_mode;
        let classic_mode = config.classic_mode;
        let minimal_vib_preset_job = config.minimal_vib_preset_job;

        Self {
            app_state,
            config,
            translations,
            active_page: Page::Gamepad,
            gamepad_texture: None,
            gamepad_texture_dark: false,
            gamepad_selected_slot: None,
            vib_preset_idx: 0,
            vib_preset_name: String::new(),
            vib_job_base: 0,
            vib_job_class: 0,
            vib_job_active: None,
            vib_job_loaded: None,
            vib_job_enabled: false,
            vib_job_snapshot: Vec::new(),
            vib_job_adv_on: true,
            vib_show_expert: false,
            vib_export_msg: None,
            vib_import_sel: 0,
            job_import_sel: 0,
            vib_import_msg: None,
            show_close_dialog: false,
            show_settings_dialog: false,
            show_about_dialog: false,
            show_guide: false,
            dfo_ask_answered: false,
            first_run_rerun: false,
            show_edition_ask: false,
            modal_defer: 0,
            new_whitelist_name: String::new(),
            whitelist_error: None,
            quick_gamepad_pending: None,
            scroll_to_edit_row: false,
            classic_tab: 0,
            classic_mode,
            classic_window_rect: None,
            classic_pos_latch: 0,
            preset_switch_key_states: Vec::new(),
            preset_key_target: 0,
            preset_key_input: String::new(),
            preset_key_error: None,
            show_device_manager: false,
            device_manager_dialog: None,
            hid_activation_dialog: None,
            hid_activation_creation_time: None,
            mouse_direction_dialog: None,
            mouse_scroll_dialog: None,
            mouse_direction_mapping_idx: None,
            mouse_scroll_mapping_idx: None,
            minimize_on_close: true,
            dialog_highlight_until: None,
            dark_mode,
            temp_config: None,
            new_mapping_trigger: String::new(),
            new_mapping_target: String::new(),
            new_mapping_target_keys: Vec::new(),
            new_mapping_interval: String::new(),
            new_mapping_duration: String::new(),
            new_mapping_turbo: true,
            new_mapping_double_tap: false,
            new_mapping_move_speed: "5".to_string(),
            new_mapping_note: String::new(),
            new_process_name: String::new(),
            key_capture_mode: KeyCaptureMode::None,
            just_captured_input: false,
            capture_pressed_keys: std::collections::HashSet::new(),
            capture_initial_pressed: std::collections::HashSet::new(),
            parsed_switch_key,
            last_vk_state: 0,
            minimal_mode,
            preset_combo_h: None,
            normal_window_rect: None,
            minimal_window_rect: None,
            minimal_pos_latch: 0,
            minimal_vib_preset_job,
            minimal_job_block_w: 0.0,
            vib_params_dirty_since: None,
            page_preset_name_input: String::new(),
            page_preset_rename_input: String::new(),
            page_preset_rename_show: false,
            page_preset_delete_arm: false,
            normal_window_size: None,
            normal_was_maximized: false,
            edit_mapping_idx: None,
            edit_mapping_snapshot: None,
            edit_mapping_is_new: false,
            was_focused: false,
            was_paused_before_settings: None,
            duplicate_mapping_error: None,
            duplicate_process_error: None,
            show_preset_name_input: false,
            preset_name_input: String::new(),
            preset_rename_target: String::new(),
            preset_rename_input: String::new(),
            cached_dark_style,
            cached_light_style,
        }
    }

    /// Updates the cached translations for the given language.
    fn update_translations(&mut self, language: crate::i18n::Language) {
        self.translations = CachedTranslations::new(language);
    }

    /// Parse switch key configuration during initialization.
    fn parse_switch_key(switch_key: &str) -> ParsedSwitchKey {
        use crate::gui::utils::string_to_vk;

        if switch_key.is_empty() {
            return ParsedSwitchKey::None;
        }

        if !switch_key.contains('+') {
            if let Some(vk) = string_to_vk(switch_key) {
                return ParsedSwitchKey::Single(vk);
            }
            return ParsedSwitchKey::None;
        }

        // Combo key - parse modifiers and keys
        let parts: Vec<&str> = switch_key.split('+').collect();
        let mut modifiers = 0u8;
        let mut keys = SmallVec::new();

        for part in parts {
            let p = part.trim().to_uppercase();
            match p.as_str() {
                "LCTRL" | "RCTRL" | "CTRL" => modifiers |= 0b001,
                "LSHIFT" | "RSHIFT" | "SHIFT" => modifiers |= 0b010,
                "LALT" | "RALT" | "ALT" => modifiers |= 0b100,
                _ => {
                    if let Some(vk) = string_to_vk(part.trim()) {
                        keys.push(vk);
                    }
                }
            }
        }

        ParsedSwitchKey::Combo { modifiers, keys }
    }

    /// Creates dark theme style.
    ///
    /// 视觉/字号/间距统一由 [`theme`] 模块定义, 此处仅做缓存。
    fn create_dark_style() -> egui::Style {
        theme::build_style(true)
    }

    /// Creates light theme style.
    fn create_light_style() -> egui::Style {
        theme::build_style(false)
    }

    /// 当前主题 (渲染函数内的取色入口)。
    pub(crate) fn theme(&self) -> Theme {
        Theme::new(self.dark_mode)
    }

    /// Launches the GUI application.
    ///
    /// # Errors
    ///
    /// Returns an error if the GUI framework fails to initialize or run.
    pub fn run(app_state: Arc<AppState>, config: AppConfig) -> anyhow::Result<()> {
        let icon = crate::gui::utils::create_icon();

        let mut viewport = egui::ViewportBuilder::default()
            .with_inner_size([1180.0, 760.0])
            .with_min_inner_size([980.0, 680.0])
            .with_resizable(true)
            .with_title("DfoVibration-V3 连发与映射工具")
            .with_icon(icon)
            .with_taskbar(true)
            .with_visible(true)
            .with_decorations(false); // 自绘标题栏 (无系统边框)

        if config.always_on_top {
            viewport = viewport.with_always_on_top();
        }

        let options = eframe::NativeOptions {
            viewport,
            ..Default::default()
        };

        let language = config.language;

        eframe::run_native(
            "Sorahk",
            options,
            Box::new(move |cc| {
                fonts::load_fonts(&cc.egui_ctx, language);

                // 后台保活线程: 窗口长时间被其他窗口遮挡时, eframe 的重绘循环
                // 可能在取消遮挡后停滞 (表现为激活后白屏, 需点击一次才恢复,
                // 且 update() 内部的任何修复都无效——它根本没被调用)。
                // 线程安全的 request_repaint 每 500ms 唤醒一次渲染循环,
                // 代价为零 (循环本就以 10fps 常驻重绘)。
                let keepalive_ctx = cc.egui_ctx.clone();
                std::thread::Builder::new()
                    .name("ui-keepalive".into())
                    .spawn(move || loop {
                        keepalive_ctx.request_repaint();
                        std::thread::sleep(std::time::Duration::from_millis(500));
                    })
                    .expect("spawn ui-keepalive thread");

                Ok(Box::new(SorahkGui::new(app_state, config)))
            }),
        )
        .map_err(|e| anyhow::anyhow!("Failed to run GUI: {}", e))
    }
}
