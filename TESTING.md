# Testing Guide

This document describes the test suite for Sorahk.

## Quick Start

```bash
# Run all tests
cargo test

# Run specific module
cargo test config::tests
cargo test state::tests

# Run combo key related tests
cargo test combo

# Run key conversion tests
cargo test key_name_to_vk
cargo test vk_to_scancode

# Run with output
cargo test -- --nocapture

# Run in release mode
cargo test --release
```

Windows batch file:
```batch
run_tests.bat
```

## Test Structure

### Unit Tests

Located within each module using `#[cfg(test)]`:

```
src/
├── config.rs      # Configuration management
├── state.rs       # State and input mapping logic
├── i18n.rs        # Internationalization
├── keyboard.rs    # Worker pool and event handling
├── mouse.rs       # Mouse input handling
├── rawinput.rs    # HID device input processing
├── xinput.rs      # XInput controller handling
├── tray.rs        # System tray utilities
├── signal.rs      # Signal handling
└── gui/
    ├── utils.rs   # GUI utility functions
    └── types.rs   # GUI type definitions
```

### Integration Tests

Located in `tests/` directory:

```
tests/
├── integration_tests.rs    # Cross-module tests
├── headless_ui_tests.rs    # 无头 UI 渲染回归 (像素级)
└── example_test_guide.rs   # Testing patterns reference
```

## 无头 UI 渲染 (Headless UI)

整套界面可以在**没有窗口、没有 GPU、没有显示设备**的环境下渲染成 PNG。
调整排版时不必再"启动 exe → 挪窗口 → PrintWindow 抓帧"。

### 为什么能这样

`SorahkGui::update_ctx(ctx)` 是 UI 的完整驱动逻辑, 签名里**没有** `eframe::Frame`。
`eframe::App::update` 只是转调它, 无头路径 (`src/gui/headless.rs`) 也是转调它 ——
所以截图与真机跑的是同一段渲染代码, 不存在"截图版和真机版不一致"。

光栅化是自实现的 CPU 扫描线填充 (`src/gui/headless.rs` 的 `Canvas`), 颜色在
**gamma 空间**按预乘 alpha 混合, 与 `egui_glow` 的 `fragment.glsl` 一致
(改成线性空间会让文字出现色边/发灰)。

### 导出一整套图

```bash
# 默认: work/headless_shots/, 1180x760, 深色, 5 页面 + 分段滚动 + 3 个弹窗
sorahk.exe --headless-shots

# 指定目录 / 尺寸 / 2x 高清 / 明暗都要
sorahk.exe --headless-shots=D:\shots --headless-size=1600x1000 --headless-ppp=2 --headless-themes=dark,light

# 只要页面, 不要弹窗和滚动
sorahk.exe --headless-shots --headless-no-dialogs --headless-scroll=0
```

| 参数 | 说明 |
|---|---|
| `--headless-shots[=目录]` | 触发无头导出 (默认 `work/headless_shots`) |
| `--headless-size=WxH` | 逻辑尺寸 (points), 默认 `1180x760` |
| `--headless-ppp=N` | 物理像素/逻辑点, 默认 `1.0`; `2` = 2x 高清 |
| `--headless-themes=dark,light` | 要导出哪些主题 |
| `--headless-scroll=N` | 长页面 (震动/职业预设) 分段滚动段数, `0` = 不导 |
| `--headless-no-dialogs` | 不导弹窗 |

导出时**自动到底就停**: 滚不动时不再产出重复图。

### 在测试里用

```rust
use sorahk::gui::headless::{HeadlessGui, HeadlessOptions};
use sorahk::gui::types::Page;

let mut app = HeadlessGui::new(config, HeadlessOptions::default())?;
app.gui.skip_first_run_for_shot();   // 截图不该被首启向导遮挡
app.gui.set_page(Page::Vibration);
let out = app.warmup(3);             // 至少 3 帧: 首帧没有上一帧的尺寸/动画状态
let shot = app.headless.shot(&out, "probe")?;
```

`tests/headless_ui_tests.rs` 会验证: 五个页面都画出真实内容 (色数/亮度)、
明暗主题互为反相、PNG 合法、以及无头路径仍与 `update_ctx` 同源。
**UI 排版改坏了 (某页变空白/整页偏色), 这里会直接失败。**

### 边界

* `Primitive::Callback` 不渲染 (GPU 专用扩展点, 本项目未使用)。
* 弹窗需要先 `set_dialog(...)`: 弹窗状态由调用方控制, 无头下没有"点按钮"这一步。
* 手柄/键盘页的 SVG 走 `prewarm_textures()` 同步灌入 (真机是后台线程)。

## Test Coverage

| Module | Primary Focus |
|--------|---------------|
| **config.rs** | Configuration loading, saving, validation, error handling, TOML serialization, multiple target keys management (add, remove, clear, set operations) |
| **state.rs** | Key conversion (VK/scancode for all key types: standard, numpad, system, lock, OEM, mouse), input device mappings (keyboard, mouse, HID devices), combo key parsing (including numpad and OEM keys), device type parsing (gamepad, joystick), mouse scroll direction parsing, output action handling (keyboard, mouse buttons, mouse movement, mouse scroll, multiple actions), state management, thread safety, atomic operations, lock-free concurrent data structures, batch INPUT event processing, extended scancode bitmap detection |
| **i18n.rs** | Multi-language translations, formatting functions, translation completeness |
| **keyboard.rs** | Worker pool creation, worker distribution stability, mapping cache retrieval |
| **mouse.rs** | Mouse button handling, message parsing, event processing |
| **rawinput.rs** | FNV-1a hash algorithm, device ID generation, VID/PID parsing, button counting, HID baseline management, combo key capture logic |
| **xinput.rs** | VID/PID hash generation, button state detection, analog stick direction mapping, trigger state detection, input combination hashing, deadzone filtering, combo mask building, bitset matching, layered index matching, AVX2 SIMD batch matching (compile-time), extended scancode detection |
| **gui/utils.rs** | Key string conversion, icon loading |
| **gui/types.rs** | KeyCaptureMode enum |
| **tray.rs** | XML escaping for notifications, utility functions, constants |
| **signal.rs** | Console control event constants, type wrappers |
| **Integration** | Cross-module interactions, configuration persistence, concurrent operations |

Run `cargo test -- --list` to see all available test functions.

## What is Tested

- Configuration management and validation
- Multiple target keys functionality (add, remove, clear, display)
- Key name to VK code conversion
- VK code to scancode mapping
- Combo key parsing and validation
- Mouse button name parsing and event handling
- Mouse movement target validation and direction parsing
- Multiple simultaneous actions
- HID device input parsing and device type identification
- Device ID generation and vendor/product ID handling
- FNV-1a hash algorithm correctness
- Generic device button ID format parsing
- HID device baseline establishment and persistence
- HID combo key capture logic (frame selection, button counting)
- Device ID parsing and validation (VID/PID/Serial format)
- XInput combo mask building and bitset matching
- GUI utility functions and type definitions
- Multi-language translation system
- Worker pool and event distribution
- Device-based load balancing and hashing
- Thread safety and atomic operations
- Error handling and edge cases

## What is Not Tested

Due to Windows API requirements, the following are not covered by automated tests:

- Windows API calls (SetWindowsHookExA, Shell_NotifyIconW, SendInput, XInputGetState, XInputGetCapabilities, etc.)
- Actual keyboard and mouse hook installation
- System tray icon display
- Toast notification display
- Physical key press and mouse click simulation
- GUI rendering and user interactions
- Combo key event handling with real keyboard input
- HID device activation dialog interactions
- Raw Input API device enumeration and data reception
- XInput API device polling and state retrieval
- Physical Xbox controller connection and disconnection

**Note:** Internal logic is tested without requiring Windows API interaction. Tests verify key conversion, combo parsing, data structure operations, button state detection, and hash generation, but avoid triggering actual input or API calls to prevent system interference.

## Writing New Tests

### Example Structure

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_descriptive_name() {
        // Arrange: Set up test data
        let config = create_test_config();

        // Act: Perform operation
        let result = process_config(config);

        // Assert: Verify result
        assert!(result.is_ok());
    }
}
```

### Conventions

- **Naming**: Use descriptive names that clearly indicate what is being tested (e.g., `test_config_save_and_load`, `test_key_name_to_vk_letters`)
- **Isolation**: Use unique temporary files with timestamps, always clean up resources
- **Focus**: Test one specific behavior per function
- **Helpers**: Extract common setup code into helper functions

### Example Test Helpers

```rust
fn get_test_file_path(name: &str) -> PathBuf {
    let mut path = std::env::temp_dir();
    path.push(format!("test_{}_{}.toml", name, timestamp()));
    path
}

fn cleanup_test_file(path: &PathBuf) {
    let _ = std::fs::remove_file(path);
}
```

## Platform Requirements

Tests require a Windows environment due to platform-specific APIs used in the project.

Potential issues:
- File access errors may occur if antivirus software interferes with temporary files
- Dependencies must be built before running tests (`cargo build`)
- Initial test runs may take longer due to compilation

## Before Committing

Run tests and lints to verify changes:

```bash
cargo test && cargo clippy
```

## Additional Resources

- [Rust Testing Documentation](https://doc.rust-lang.org/book/ch11-00-testing.html)
- [Cargo Test Documentation](https://doc.rust-lang.org/cargo/commands/cargo-test.html)
- [Testing Patterns Reference](tests/example_test_guide.rs) - Code examples and test patterns
