v20.9.1 (2026-09-10)
====================
修复:

- 连发映射键帽配色定稿为「描边式」: 中性键帽底 + 1.3px 彩色描边 + 彩色字
  (紫=手柄 / 橙=鼠标 / 键盘=完全原样式); v20.9 色块填充风格经用户反馈否决后重做,
  编辑面板目标标签底色同步减淡

v20.9 (2026-09-10)
==================
功能增强:

- 连发映射列表与编辑面板键帽按设备类型上色, 一眼分清手柄/鼠标/键盘来源
- 新增按键设备类型识别 utils::key_kind (GAMEPAD 前缀=手柄, MOUSE_*/SCROLL_*/鼠标按键=鼠标)
- 设计系统新增 gamepad/mouse 键帽语义色 (明暗两套)

v20.8 (2026-09-10)
==================
修复:

- 鼠标「移动方向」「滚动方向」选择弹窗塌缩成 ~24px 小圆点的老 bug (modal_window
  构建器点状塌缩, 交接记录第 17 条冻结案结案): 两弹窗改回内联 egui::Window 样板,
  构建器标记禁用 (零调用者)
- 由此修复的下游问题: 方向永远无法选中 → 映射列表恒显示「未设置目标」

文档:

- HANDOFF 新增第 40/41/42 条; 第 42 条 = GitHub 上交流程 (GitHub Desktop + D 盘 clone,
  口令「测试通过，上交到仓库」触发)

0.4.0
=====
Feature enhancements:

- Add multiple target keys support for simultaneous key presses
- Add XInput API integration for Xbox controller support
- Add Raw Input API integration for HID devices
- Add HID device activation system with interactive calibration dialog
- Add mouse movement support with eight-directional control
- Add mouse scroll support with configurable speed
- Add tray icon internationalization support with dynamic language switching
- Add performance optimizations for input processing
  - Multi-tier caching for device information
  - SIMD acceleration for data comparison when available
  - Inline optimization for frequently called functions

UI Improvements:

- Add Device Manager dialog for device configuration and testing
- Add mouse direction and scroll selection dialogs
- Add target type selector in settings dialog
- Add HID device activation dialog with real-time feedback
- Add internationalization support for tray icon menus and notifications

Configuration:

- Change `target_key` to `target_keys` array for multi-target support
- Add capture mode configuration for XInput devices
- Add device baseline persistence for HID devices
- Add per-device API preference settings
- Add movement and scroll speed configuration

0.3.0
=====
Feature enhancements:

- Add multi-language support (English, 简体中文, 繁體中文, 日本語)
- Add language selector in settings dialog with real-time preview
- Add key combination support for triggers and targets (e.g., LALT+A, RCTRL+RSHIFT+S)
- Add mouse button support (Left, Right, Middle, X1, X2)
- Add per-mapping turbo mode toggle with Windows native repeat support
- Add lock-free concurrency with scc containers for improved performance
- Add multi-layer caching for process whitelist, turbo state, and mapping info
- Add combo key reverse index for O(1) lookup optimization
- Add enhanced key capture with comprehensive keyboard support
  - Support for F1-F24, numpad, lock keys, system keys, and OEM punctuation
  - Left/right modifier distinction (LCTRL/RCTRL, LALT/RALT, LSHIFT/RSHIFT)
  - Initial state filtering to prevent false positives
- Add input validation for duplicate trigger keys and process names
- Add comprehensive test suite with TESTING.md guide
- Add `turbo_enabled` configuration field (defaults to true)
- Update timing parameter minimums (input_timeout: 2ms, event_duration: 2ms)

UI Improvements:

- Add turbo toggle button with visual state indication (⚡/○)
- Add localized hover tooltips for turbo toggle in all languages
- Add turbo status display in main window mappings table
- Increase settings window width to 720px for better layout

0.2.0
=====
Feature enhancements:

* Add GUI with anime-style design
* Add interactive settings dialog with real-time configuration editing
* Add configurable light/dark theme support with persistent storage
* Add multi-threaded worker pool with load-balanced event dispatching
* Add process whitelist for application-specific turbo-fire control
* Add Windows Toast notification system with fallback support
* Add About dialog with project information
* Add embedded application icon

UI Improvements:
- Replace tray icon display to use custom icon

0.1.1
=====
Feature enhancements:

* Tray icon support
