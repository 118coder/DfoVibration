//! ★R2.14 捕获会话领域 (BackendService impl)。
//!
//! 键盘/手柄/录制三类捕获的**传输层**归后端；VK→显示名等格式化留在 UI 层
//! (纯表现逻辑)。实现全部是对既有通道/状态的薄封装，行为与旧 GUI 一致。

use std::collections::HashSet;

use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;

use super::BackendService;

impl BackendService {
    // ── 键盘捕获 (旧 settings_dialog::poll_all_pressed_keys 原样移入) ────

    /// 轮询当前所有按下的键 (VK 集合)。UI 以帧率调用，自行做边缘检测。
    #[inline]
    pub fn poll_pressed_keys(&self) -> HashSet<u32> {
        let mut pressed_keys = HashSet::new();

        unsafe {
            // Modifiers
            let modifiers = [
                0xA0, 0xA1, // LSHIFT, RSHIFT
                0xA2, 0xA3, // LCTRL, RCTRL
                0xA4, 0xA5, // LALT, RALT
                0x5B, 0x5C, // LWIN, RWIN
            ];
            for &vk in &modifiers {
                if GetAsyncKeyState(vk as i32) < 0 {
                    pressed_keys.insert(vk);
                }
            }

            // A-Z (0x41-0x5A)
            for vk in 0x41..=0x5A {
                if GetAsyncKeyState(vk) < 0 {
                    pressed_keys.insert(vk as u32);
                }
            }

            // 0-9 (0x30-0x39)
            for vk in 0x30..=0x39 {
                if GetAsyncKeyState(vk) < 0 {
                    pressed_keys.insert(vk as u32);
                }
            }

            // Numpad 0-9 (0x60-0x69)
            for vk in 0x60..=0x69 {
                if GetAsyncKeyState(vk) < 0 {
                    pressed_keys.insert(vk as u32);
                }
            }

            // F1-F24 (0x70-0x87)
            for vk in 0x70..=0x87 {
                if GetAsyncKeyState(vk) < 0 {
                    pressed_keys.insert(vk as u32);
                }
            }

            // Navigation and editing keys
            let navigation = [
                0x20, // SPACE
                0x0D, // ENTER
                0x09, // TAB
                0x1B, // ESC
                0x08, // BACKSPACE
                0x2E, // DELETE
                0x2D, // INSERT
                0x24, // HOME
                0x23, // END
                0x21, // PAGEUP
                0x22, // PAGEDOWN
                0x26, // UP
                0x28, // DOWN
                0x25, // LEFT
                0x27, // RIGHT
            ];
            for &vk in &navigation {
                if GetAsyncKeyState(vk as i32) < 0 {
                    pressed_keys.insert(vk);
                }
            }

            // Lock and special keys
            let lock_keys = [
                0x14, // CAPSLOCK
                0x90, // NUMLOCK
                0x91, // SCROLL LOCK
                0x13, // PAUSE
                0x2C, // PRINT SCREEN
            ];
            for &vk in &lock_keys {
                if GetAsyncKeyState(vk as i32) < 0 {
                    pressed_keys.insert(vk);
                }
            }

            // Numpad operators
            let numpad_ops = [
                0x6A, // MULTIPLY
                0x6B, // ADD
                0x6C, // SEPARATOR
                0x6D, // SUBTRACT
                0x6E, // DECIMAL
                0x6F, // DIVIDE
            ];
            for &vk in &numpad_ops {
                if GetAsyncKeyState(vk as i32) < 0 {
                    pressed_keys.insert(vk);
                }
            }

            // OEM keys (punctuation and symbols)
            let oem_keys = [
                0xBA, // OEM_1 (;:)
                0xBB, // OEM_PLUS (=+)
                0xBC, // OEM_COMMA (,<)
                0xBD, // OEM_MINUS (-_)
                0xBE, // OEM_PERIOD (.>)
                0xBF, // OEM_2 (/?)
                0xC0, // OEM_3 (`~)
                0xDB, // OEM_4 ([{)
                0xDC, // OEM_5 (\|)
                0xDD, // OEM_6 (]})
                0xDE, // OEM_7 ('")
                0xDF, // OEM_8
                0xE2, // OEM_102 (<>)
            ];
            for &vk in &oem_keys {
                if GetAsyncKeyState(vk as i32) < 0 {
                    pressed_keys.insert(vk);
                }
            }

            // Mouse buttons
            let mouse_buttons = [
                0x01, // LBUTTON
                0x02, // RBUTTON
                0x04, // MBUTTON
                0x05, // XBUTTON1
                0x06, // XBUTTON2
            ];
            for &vk in &mouse_buttons {
                if GetAsyncKeyState(vk as i32) < 0 {
                    pressed_keys.insert(vk);
                }
            }
        }

        pressed_keys
    }

    // ── 手柄捕获 (RawInput 捕获模式；结果走 poll_events → RawCaptured) ───

    /// 进入/退出手柄捕获模式 (进入时后端清缓存/清通道，旧 set_raw_input_capture_mode 语义)。
    pub fn set_pad_capture(&self, on: bool) {
        self.app.set_raw_input_capture_mode(on);
    }

    /// 取走一帧捕获结果 (None = 本帧没有)。与 `poll_events()` 的 RawCaptured 二选一用。
    pub fn poll_pad_capture(&self) -> Option<crate::state::InputDevice> {
        self.app.try_recv_raw_input_capture()
    }

    /// 捕获策略透传 (组合键捕获需要"输入数最多"的帧选择)。
    pub fn set_capture_prefers_max_inputs(&self, on: bool) {
        self.app
            .capture_prefers_max_inputs
            .store(on, std::sync::atomic::Ordering::Relaxed);
    }

    /// 清某设备的基线 (捕获前调用，旧 device_manager/flow 语义)。
    pub fn clear_device_baseline(&self, vid: u16, pid: u16) {
        self.app.clear_device_baseline(vid, pid);
    }

    // ── 实时态细粒度透传 (帧级原子读; 聚合版见 snapshot().input) ────────

    /// 正在实时识别的第三方手柄 (vid, pid)。
    pub fn live_hid_pad(&self) -> Option<(u16, u16)> {
        self.app.live_hid_pad()
    }

    /// 该手柄最新实时状态 (SVG 按下即亮 / 识别态自动选中)。
    pub fn live_hid_state(&self) -> Option<crate::state::LiveHidState> {
        self.app.live_hid_state()
    }

    /// XInput 实时输入位图 (vid, 按钮位图)。
    pub fn live_xinput_state(&self) -> Option<(u16, u32)> {
        self.app.live_xinput_state()
    }

    /// 设置/停止实时识别目标 (手柄页「停止识别」与校准记录后挂载)。
    pub fn set_live_hid_pad(&self, pad: Option<(u16, u16)>) {
        self.app.set_live_hid_pad(pad);
    }

    // ── 手柄校准抑制标志 (旧 flow.rs 直调 AppState 的两处) ───────────────

    /// 校准向导进行中标志 (进行中输入线程不派发任何映射)。
    pub fn is_gp_calibrating(&self) -> bool {
        self.app.is_gp_calibrating()
    }

    /// 设置校准向导进行中标志 (与 GpFlow::Calibrate 状态机每帧同步)。
    pub fn set_gp_calibrating(&self, on: bool) {
        self.app.set_gp_calibrating(on);
    }

    // ── 序列录制状态查询 (渲染「● 录制中…」用; start/stop 见上) ─────────

    /// 序列宏录制是否进行中。
    pub fn key_record_active(&self) -> bool {
        self.app
            .key_record_active
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    /// 停止录制并产出序列文本, 附带诊断: IME 吞键计数 (vk == 0xE5)。
    /// 旧 edit.rs 停止按钮用它区分「没按键盘」与「被中文输入法接管」两种失败提示。
    /// 返回 `None` = 录制状态丢失 (没在录制 / 状态已取走);
    /// `Some((序列文本, ime_hits))`, 内层文本 None = 录到了但没有效按键。
    pub fn stop_key_record_with_diagnostics(&self) -> Option<(Option<String>, usize)> {
        let events = self.app.stop_key_record()?;
        let ime_hits = events.iter().filter(|e| e.vk == 0xE5).count();
        let text =
            crate::sequence::recorded_to_sequence(&events, &crate::sequence::vk_to_seq_name);
        Some((if text.is_empty() { None } else { Some(text) }, ime_hits))
    }

    // ── 序列宏录制 ──────────────────────────────────────────────────────

    /// 开始录制 (后端负责拉起轮询线程，旧 edit.rs 的两步合一；已在录则幂等)。
    pub fn start_key_record(&self) {
        /* 已在录制中：不重复拉起轮询线程 (旧 GUI 重复点击会叠线程)。 */
        if self
            .app
            .key_record_active
            .load(std::sync::atomic::Ordering::Relaxed)
        {
            return;
        }
        self.app.start_key_record();
        let st = self.app.clone();
        /* 线程名沿用旧实现 (util 看护名单外的辅助线程)。 */
        let _ = std::thread::Builder::new()
            .name("key-record-poller".into())
            .spawn(move || crate::sequence::run_key_record_poller(st));
    }

    /// 停止录制并直接产出序列文本 (空文本 = 没录到有效按键)。
    pub fn stop_key_record(&self) -> Option<String> {
        let events = self.app.stop_key_record()?;
        let text = crate::sequence::recorded_to_sequence(&events, &crate::sequence::vk_to_seq_name);
        if text.is_empty() {
            None
        } else {
            Some(text)
        }
    }

    // ── HID 激活引导 (第三方手柄原始报文通道) ───────────────────────────

    /// 取走某设备句柄的原始报文 (旧 main_window.rs:124-137 消费语义)。
    pub fn try_hid_activation_data(&self, device_handle: isize) -> Option<Vec<u8>> {
        self.app.try_recv_hid_activation_data(device_handle)
    }

    pub fn is_device_activating(&self, device_handle: isize) -> bool {
        self.app.is_device_activating(device_handle)
    }

    pub fn clear_activating_device(&self) {
        self.app.clear_activating_device();
    }
}
