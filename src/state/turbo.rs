//! 连发引擎状态机: 按键录制 (v24.31 序列宏) + 组合键激活/修饰键释放 —— 原 state.rs 1962-2247, 2026-09-27 架构重构 B7 归位。

use std::sync::atomic::Ordering;
use std::time::Instant;

use smallvec::SmallVec;


use windows::Win32::UI::Input::KeyboardAndMouse::*;

use super::*;

impl AppState {
    /// ★v24.31 开始录制按键 (序列宏录制)
    pub fn start_key_record(&self) {
        if let Ok(mut r) = self.key_record.lock() {
            *r = Some(KeyRecordState {
                start: Instant::now(),
                events: Vec::new(),
            });
        }
        self.key_record_active.store(true, Ordering::Relaxed);
        crate::vibration::vib_log("[record] start (poller)");
    }

    /// ★v24.31 停止录制并取走事件
    pub fn stop_key_record(&self) -> Option<Vec<crate::sequence::RecordedKey>> {
        self.key_record_active.store(false, Ordering::Relaxed);
        let mut r = self.key_record.lock().ok()?;
        let state = r.take()?;
        /* ★v24.31 诊断探针 */
        crate::vibration::vib_log(&format!(
            "[record] stop events={}",
            state.events.len()
        ));
        Some(state.events)
    }

    /// ★v24.31 轮询通道的录制入口 (键盘按键状态沿; 过滤 vk=0 的空事件)
    pub fn record_key_state(&self, is_down: bool, vk_code: u32) {
        if vk_code == 0 {
            return;
        }
        self.record_key_event(is_down, vk_code);
    }

    /// ★v24.31 钩子线程调用: 记录一次按键按下/抬起 (录制中才有效)
    pub(super) fn record_key_event(&self, is_down: bool, vk_code: u32) {
        if !self.key_record_active.load(Ordering::Relaxed) {
            return;
        }
        if let Ok(mut r) = self.key_record.lock() {
            let Some(rec) = r.as_mut() else { return };
            /* ★v24.31 诊断探针: 每次录制只记第一条按下 (证明钩子→录制链路通) */
            if is_down && rec.events.is_empty() {
                crate::vibration::vib_log(&format!(
                    "[record] first key vk=0x{vk_code:X} (钩子→录制链路通)"
                ));
            }
            let t = rec.start.elapsed().as_millis() as u64;
            if is_down {
                rec.events.push(crate::sequence::RecordedKey {
                    vk: vk_code,
                    down_ms: t,
                    up_ms: None,
                });
            } else if let Some(e) = rec
                .events
                .iter_mut()
                .rev()
                .find(|e| e.vk == vk_code && e.up_ms.is_none())
            {
                e.up_ms = Some(t);
            }
        }
    }

    /// Gets all XInputCombo button_ids for a specific device type
    /// Used for subset matching in runtime (cached for performance)
    #[inline(always)]
    pub fn get_xinput_combos_for_device(&self, device_type: &DeviceType) -> Vec<Vec<u32>> {
        self.cached_xinput_combos
            .get_sync(device_type)
            .map(|v| v.get().clone())
            .unwrap_or_default()
    }

    /// Check turbo_enabled state from cache (hot path)
    #[inline(always)]
    pub(super) fn is_turbo_enabled(&self, device: &InputDevice) -> bool {
        match device {
            InputDevice::Keyboard(vk) if *vk < 256 => {
                self.cached_turbo_keyboard[*vk as usize].load(Ordering::Relaxed)
            }
            _ => self
                .cached_turbo_other
                .read_sync(device, |_, v| *v)
                .unwrap_or(true),
        }
    }

    #[inline]
    pub fn handle_switch_key_toggle(&self) {
        let was_paused = self.toggle_paused();
        self.active_combo_triggers.clear_sync();

        if let Some(sender) = self.notification_sender.lock().ok().as_mut().and_then(|s| s.as_ref()) {
            let msg = if was_paused {
                "Sorahk activating".to_string()
            } else {
                "Sorahk paused".to_string()
            };
            let _ = sender.send(NotificationEvent::Info(msg));
        }
    }

    /// Check if a key is a modifier key
    #[inline]
    pub(super) fn is_modifier_key(&self, vk: u32) -> bool {
        matches!(
            vk,
            0x10 | 0xA0 | 0xA1 |  // SHIFT, LSHIFT, RSHIFT
            0x11 | 0xA2 | 0xA3 |  // CTRL, LCTRL, RCTRL
            0x12 | 0xA4 | 0xA5 |  // ALT, LALT, RALT
            0x5B | 0x5C // LWIN, RWIN
        )
    }

    /// Check if a key is part of any active combo (should be blocked)
    /// Returns true if the key should be intercepted
    #[inline]
    pub(super) fn is_in_active_combo(&self, vk_code: u32) -> bool {
        let mut found = false;
        self.active_combo_triggers.iter_sync(|combo_device, _| {
            if found {
                return false;
            }
            if let InputDevice::KeyCombo(keys) = combo_device
                && keys.contains(&vk_code)
            {
                found = true;
            }
            true
        });
        found
    }

    /// Check if this key is the main key (last key) of an active combo with turbo disabled
    /// Used to allow Windows repeat behavior for turbo-disabled combos
    #[inline]
    pub(super) fn is_main_key_in_active_combo_no_turbo(&self, vk_code: u32) -> bool {
        let mut result = false;
        self.active_combo_triggers.iter_sync(|combo_device, _| {
            if result {
                return false;
            }
            if let InputDevice::KeyCombo(keys) = combo_device {
                // Check if this is the main key (last key in combo)
                if let Some(&last_key) = keys.last()
                    && last_key == vk_code
                    && !self.is_turbo_enabled(combo_device)
                {
                    result = true;
                }
            }
            true
        });
        result
    }

    /// Add a combo to active triggers
    pub(super) fn add_active_combo(&self, combo: InputDevice, modifiers: SmallVec<[u32; 8]>) {
        let _ = self.active_combo_triggers.insert_sync(combo, modifiers);
    }

    /// Check if a specific combo is active
    #[inline(always)]
    pub(super) fn is_combo_active(&self, combo: &InputDevice) -> bool {
        self.active_combo_triggers.contains_sync(combo)
    }

    /// Release modifiers once (send KEYUP events using scancodes)
    /// Skips modifiers already suppressed by other active combos
    pub(super) fn release_modifiers_once(&self, modifiers: &SmallVec<[u32; 8]>) {
        if modifiers.is_empty() {
            return;
        }

        // Check which modifiers are already suppressed by active combos
        let mut already_suppressed: SmallVec<[u32; 8]> = SmallVec::new();
        self.active_combo_triggers.iter_sync(|_, modifiers| {
            already_suppressed.extend_from_slice(modifiers);
            true
        });

        unsafe {
            for &vk in modifiers {
                // Skip if this modifier is already suppressed by another active combo
                if already_suppressed.contains(&vk) {
                    continue;
                }

                let (scancode, is_extended) = match vk {
                    0x10 | 0xA0 => (0x2A, false), // SHIFT, LSHIFT
                    0xA1 => (0x36, false),        // RSHIFT
                    0x11 | 0xA2 => (0x1D, false), // CTRL, LCTRL
                    0xA3 => (0x1D, true),         // RCTRL (extended)
                    0x12 | 0xA4 => (0x38, false), // ALT, LALT
                    0xA5 => (0x38, true),         // RALT (extended)
                    0x5B => (0x5B, true),         // LWIN (extended)
                    0x5C => (0x5C, true),         // RWIN (extended)
                    _ => continue,
                };

                let mut flags = KEYEVENTF_SCANCODE | KEYEVENTF_KEYUP;
                if is_extended {
                    flags |= KEYEVENTF_EXTENDEDKEY;
                }

                let input = INPUT {
                    r#type: INPUT_KEYBOARD,
                    Anonymous: INPUT_0 {
                        ki: KEYBDINPUT {
                            wVk: VIRTUAL_KEY(0),
                            wScan: scancode,
                            dwFlags: flags,
                            time: 0,
                            dwExtraInfo: SIMULATED_EVENT_MARKER,
                        },
                    },
                };
                SendInput(&[input], std::mem::size_of::<INPUT>() as i32);
            }

            // Small delay to ensure modifier release is processed
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }

    /// Remove combos that no longer have all their keys pressed
    /// Returns vec of combos that were removed
    pub(super) fn cleanup_released_combos(&self) -> SmallVec<[InputDevice; 4]> {
        let mut pressed: SmallVec<[u32; 16]> = SmallVec::new();
        self.pressed_keys.iter_sync(|&k| {
            pressed.push(k);
            true
        });

        let mut to_remove: SmallVec<[InputDevice; 4]> = SmallVec::new();

        self.active_combo_triggers.iter_sync(|combo_device, _| {
            if let InputDevice::KeyCombo(keys) = combo_device {
                // Fast linear scan — optimal for small N
                if !keys.iter().all(|&k| pressed.contains(&k)) {
                    to_remove.push(combo_device.clone());
                }
            }
            true
        });

        let mut removed = SmallVec::new();
        for combo in to_remove {
            if self.active_combo_triggers.remove_sync(&combo).is_some() {
                removed.push(combo);
            }
        }

        removed
    }

    /// Find device for release event (single key or combo from active triggers)
    pub(super) fn find_device_for_release(&self, vk_code: u32) -> Option<InputDevice> {
        // Check if it's part of any active combo
        let mut result = None;
        self.active_combo_triggers.iter_sync(|combo_device, _| {
            if result.is_some() {
                return false;
            }
            if let InputDevice::KeyCombo(keys) = combo_device
                && keys.contains(&vk_code)
            {
                result = Some(combo_device.clone());
            }
            true
        });
        if result.is_some() {
            return result;
        }

        // Otherwise, check for single key mapping
        let device = InputDevice::Keyboard(vk_code);
        if self.get_input_mapping(&device).is_some() {
            Some(device)
        } else {
            None
        }
    }
}
