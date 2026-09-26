//! 键盘/鼠标钩子事件入口 (handle_key_event / find_matching_combo / handle_mouse_event) —— 原 state.rs 2995-3231, 2026-09-27 架构重构 B4 归位。

use super::*;

impl AppState {

    #[allow(non_snake_case)]
    #[inline]
    pub fn handle_key_event(&self, message: u32, vk_code: u32) -> bool {
        let mut should_block = false;

        if matches!(message, WM_KEYDOWN | WM_SYSKEYDOWN) {
            let _ = self.pressed_keys.insert_sync(vk_code);

            let kb_vk = self.switch_key_cache.keyboard_vk.load(Ordering::Relaxed);

            if kb_vk != 0 && vk_code == kb_vk {
                self.handle_switch_key_toggle();
                return true;
            }

            if kb_vk == 0
                && let Some(device) = util::read_guard(&self.switch_key_cache.full_device).as_ref()
                && let InputDevice::KeyCombo(keys) = device
                && keys.contains(&vk_code)
            {
                let mut all_pressed = true;
                for k in keys.iter() {
                    if !self.pressed_keys.contains_sync(k) {
                        all_pressed = false;
                        break;
                    }
                }

                if all_pressed {
                    self.handle_switch_key_toggle();
                    return true;
                }
            }
        }

        if matches!(message, WM_KEYUP | WM_SYSKEYUP) {
            let _ = self.pressed_keys.remove_sync(&vk_code);
        }

        let is_up = matches!(message, WM_KEYUP | WM_SYSKEYUP);
        // 冷却期/输入法打字中/白名单外/暂停: 按下事件全部"透传" —— 不拦截也不映射。
        // 抬起事件不能跟着被吞: Released 到不了 worker, 注入的键会永久悬空 (卡键)。
        // 因此抬起始终进入下方派发, 但保持透传 (不拦截真实抬起)。
        let gated = unlikely(
            self.is_paused()
                || !self.is_process_whitelisted()
                || is_ime_composing()
                || injection_cooldown_active()
        );
        if gated && !is_up {
            return should_block;
        }

        match message {
            WM_KEYDOWN | WM_SYSKEYDOWN => {
                // Check if this key is in an active combo
                if unlikely(self.is_in_active_combo(vk_code)) {
                    // Check if this is a main key of a combo with turbo disabled
                    let is_main_key_no_turbo = self.is_main_key_in_active_combo_no_turbo(vk_code);

                    if !is_main_key_no_turbo {
                        // Block repeat events for modifiers and turbo-enabled combo main keys
                        return true;
                    }
                }

                let mut pressed_snapshot: std::collections::HashSet<u32> =
                    std::collections::HashSet::with_capacity(16);
                self.pressed_keys.iter_sync(|key| {
                    pressed_snapshot.insert(*key);
                    true
                });
                let matched_device = self
                    .find_matching_combo(&pressed_snapshot, vk_code)
                    .or_else(|| {
                        let device = InputDevice::Keyboard(vk_code);
                        if self.get_input_mapping(&device).is_some() {
                            Some(device)
                        } else {
                            None
                        }
                    });

                if let Some(device) = matched_device {
                    let already_active = self.is_combo_active(&device);

                    if already_active {
                        let allow_repeat = !self.is_turbo_enabled(&device);

                        if !allow_repeat {
                            return true;
                        }
                    }

                    if let InputDevice::KeyCombo(_) = &device {
                        let mut modifiers: SmallVec<[u32; 8]> = SmallVec::new();
                        self.pressed_keys.iter_sync(|key| {
                            if *key != vk_code && self.is_modifier_key(*key) {
                                modifiers.push(*key);
                            }
                            true
                        });

                        self.release_modifiers_once(&modifiers);
                        self.add_active_combo(device.clone(), modifiers.clone());
                    }

                    if let Some(pool) = self.worker_pool.get() {
                        pool.dispatch(InputEvent::Pressed(device));
                        should_block = true;
                    }
                }
            }

            WM_KEYUP | WM_SYSKEYUP => {
                // gated (暂停/白名单外/IME/冷却) 时的抬起: 只补发 Released 解卡,
                // 不拦截真实抬起 —— 按下当时是透传的, 游戏侧已经收到了按下
                let transparent = gated;
                let removed_combos = self.cleanup_released_combos();

                if !removed_combos.is_empty()
                    && let Some(pool) = self.worker_pool.get()
                {
                    for combo in removed_combos {
                        pool.dispatch(InputEvent::Released(combo));
                    }
                    if !transparent {
                        should_block = true;
                    }
                }

                let device = self.find_device_for_release(vk_code);
                if let Some(dev) = device
                    && let Some(pool) = self.worker_pool.get()
                {
                    pool.dispatch(InputEvent::Released(dev));
                    if !transparent {
                        should_block = true;
                    }
                }
            }

            _ => {}
        }

        should_block
    }

    /// Find a matching key combo from currently pressed keys
    /// Supports multiple combos simultaneously (e.g., LALT+1, LALT+2, LALT+3 all active)
    #[inline]
    pub(super) fn find_matching_combo(
        &self,
        pressed_keys: &std::collections::HashSet<u32>,
        main_key: u32,
    ) -> Option<InputDevice> {
        // Fast path: use lock-free read
        self.cached_combo_index
            .read_sync(&main_key, |_, combos| {
                // Iterate through potential combos
                for device in combos {
                    if let InputDevice::KeyCombo(combo_keys) = device {
                        // Check if all combo keys are pressed
                        let all_pressed = combo_keys.iter().all(|&k| pressed_keys.contains(&k));
                        if likely(all_pressed) {
                            return Some(device.clone());
                        }
                    }
                }
                None
            })
            .flatten()
    }

    /// Find a combo key that contains the released key
    /// This is called when a key is released to check if it was part of an active combo
    #[allow(non_snake_case)]
    pub fn handle_mouse_event(&self, message: u32, mouse_data: u32) -> bool {
        let mut should_block = false;

        // Parse mouse button from message
        let button_opt = match message {
            WM_LBUTTONDOWN | WM_LBUTTONUP => Some(MouseButton::Left),
            WM_RBUTTONDOWN | WM_RBUTTONUP => Some(MouseButton::Right),
            WM_MBUTTONDOWN | WM_MBUTTONUP => Some(MouseButton::Middle),
            WM_XBUTTONDOWN | WM_XBUTTONUP => {
                // Extract X button identifier from high word of mouseData
                // XBUTTON1 = 1, XBUTTON2 = 2
                let x_button = (mouse_data >> 16) & 0xFFFF;
                match x_button {
                    1 => Some(MouseButton::X1),
                    2 => Some(MouseButton::X2),
                    _ => None, // Unknown X button
                }
            }
            _ => None,
        };

        if let Some(button) = button_opt {
            let device = InputDevice::Mouse(button);
            let is_up = matches!(
                message,
                WM_LBUTTONUP | WM_RBUTTONUP | WM_MBUTTONUP | WM_XBUTTONUP
            );
            // 与键盘路径一致: 暂停/白名单外/注入冷却期间的按下事件透传
            // (旧实现漏了冷却检查 → 冷却期点击被拦截又未注入 = 凭空丢失);
            // 抬起事件始终派发, 否则注入的鼠标键悬空卡键。
            let gated = self.is_paused()
                || !self.is_process_whitelisted()
                || injection_cooldown_active();
            if (!gated || is_up)
                && self.get_input_mapping(&device).is_some()
                && let Some(pool) = self.worker_pool.get()
            {
                match message {
                    WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN | WM_XBUTTONDOWN => {
                        pool.dispatch(InputEvent::Pressed(device));
                        // Block the original down event since we'll simulate it
                        should_block = true;
                    }
                    WM_LBUTTONUP | WM_RBUTTONUP | WM_MBUTTONUP | WM_XBUTTONUP => {
                        pool.dispatch(InputEvent::Released(device));
                        // Don't block the release event - let it pass through to the system
                        // so that context menus and other UI elements can respond properly
                        should_block = false;
                    }
                    _ => {}
                }
            }
        }

        should_block
    }
}
