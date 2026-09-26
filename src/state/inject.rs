//! SendInput 系统注入 (simulate_action/press/release + collect_*_inputs) —— 原 state.rs 2247-2888, 2026-09-27 架构重构 B6 归位。
//! 含注入安全保险 (injection_guard_ok / cooldown) 调用方语义: 保险本体仍在 mod.rs。

use std::sync::atomic::Ordering;

use smallvec::SmallVec;

use crate::util::unlikely;

use windows::Win32::UI::Input::KeyboardAndMouse::*;

use super::*;

impl AppState {
    #[inline]
    pub fn simulate_action(&self, action: OutputAction, duration: u64) {
        // 注入前实时校验: ① 前台进程白名单(空=全部启用, 所有输入源统一生效,
        // 修复手柄路径绕过白名单 + 切窗后旧事件继续注入的问题) ② 注入频率保险
        if unlikely(!self.is_process_whitelisted() || is_ime_composing() || !injection_guard_ok()) {
            return;
        }
        unsafe {
            match action {
                OutputAction::KeyboardKey(scancode) => {
                    let mut press_flags = KEYEVENTF_SCANCODE;
                    if Self::is_extended_scancode(scancode) {
                        press_flags |= KEYEVENTF_EXTENDEDKEY;
                    }

                    // Press the key
                    let mut input = INPUT {
                        r#type: INPUT_KEYBOARD,
                        Anonymous: INPUT_0 {
                            ki: KEYBDINPUT {
                                wVk: VIRTUAL_KEY(0),
                                wScan: scancode,
                                dwFlags: press_flags,
                                time: 0,
                                dwExtraInfo: SIMULATED_EVENT_MARKER,
                            },
                        },
                    };

                    SendInput(&[input], std::mem::size_of::<INPUT>() as i32);
                    std::thread::sleep(std::time::Duration::from_millis(duration));

                    // Release the key
                    input.Anonymous.ki.dwFlags = press_flags | KEYEVENTF_KEYUP;
                    SendInput(&[input], std::mem::size_of::<INPUT>() as i32);
                }
                OutputAction::MouseButton(button) => {
                    use windows::Win32::UI::Input::KeyboardAndMouse::*;

                    let (down_flag, up_flag) = match button {
                        MouseButton::Left => (MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP),
                        MouseButton::Right => (MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP),
                        MouseButton::Middle => (MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP),
                        MouseButton::X1 => (MOUSEEVENTF_XDOWN, MOUSEEVENTF_XUP),
                        MouseButton::X2 => (MOUSEEVENTF_XDOWN, MOUSEEVENTF_XUP),
                    };

                    let mouse_data = match button {
                        MouseButton::X1 => 1,
                        MouseButton::X2 => 2,
                        _ => 0,
                    };

                    // Press the button
                    let mut input = INPUT {
                        r#type: INPUT_MOUSE,
                        Anonymous: INPUT_0 {
                            mi: MOUSEINPUT {
                                dx: 0,
                                dy: 0,
                                mouseData: mouse_data,
                                dwFlags: down_flag,
                                time: 0,
                                dwExtraInfo: SIMULATED_EVENT_MARKER,
                            },
                        },
                    };

                    SendInput(&[input], std::mem::size_of::<INPUT>() as i32);
                    std::thread::sleep(std::time::Duration::from_millis(duration));

                    // Release the button
                    input.Anonymous.mi.dwFlags = up_flag;
                    SendInput(&[input], std::mem::size_of::<INPUT>() as i32);
                }
                OutputAction::MouseMove(direction, speed) => {
                    use windows::Win32::UI::Input::KeyboardAndMouse::*;

                    let (dx, dy) = match direction {
                        MouseMoveDirection::Up => (0, -speed),
                        MouseMoveDirection::Down => (0, speed),
                        MouseMoveDirection::Left => (-speed, 0),
                        MouseMoveDirection::Right => (speed, 0),
                        MouseMoveDirection::UpLeft => (-speed, -speed),
                        MouseMoveDirection::UpRight => (speed, -speed),
                        MouseMoveDirection::DownLeft => (-speed, speed),
                        MouseMoveDirection::DownRight => (speed, speed),
                    };

                    let input = INPUT {
                        r#type: INPUT_MOUSE,
                        Anonymous: INPUT_0 {
                            mi: MOUSEINPUT {
                                dx,
                                dy,
                                mouseData: 0,
                                dwFlags: MOUSEEVENTF_MOVE,
                                time: 0,
                                dwExtraInfo: SIMULATED_EVENT_MARKER,
                            },
                        },
                    };

                    SendInput(&[input], std::mem::size_of::<INPUT>() as i32);
                }
                OutputAction::MouseScroll(direction, speed) => {
                    use windows::Win32::UI::Input::KeyboardAndMouse::*;

                    // Direct speed control without multiplier
                    let wheel_delta = match direction {
                        MouseScrollDirection::Up => speed,
                        MouseScrollDirection::Down => -speed,
                    };

                    let input = INPUT {
                        r#type: INPUT_MOUSE,
                        Anonymous: INPUT_0 {
                            mi: MOUSEINPUT {
                                dx: 0,
                                dy: 0,
                                mouseData: wheel_delta as u32,
                                dwFlags: MOUSEEVENTF_WHEEL,
                                time: 0,
                                dwExtraInfo: SIMULATED_EVENT_MARKER,
                            },
                        },
                    };

                    SendInput(&[input], std::mem::size_of::<INPUT>() as i32);
                }
                OutputAction::KeyCombo(scancodes) => {
                    // Press all keys in sequence (modifiers first, then main key)
                    for &scancode in scancodes.iter() {
                        let mut flags = KEYEVENTF_SCANCODE;
                        if Self::is_extended_scancode(scancode) {
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
                        // Short delay between keys for better compatibility
                        std::thread::sleep(std::time::Duration::from_millis(1));
                    }

                    // Hold duration
                    std::thread::sleep(std::time::Duration::from_millis(duration));

                    // Release all keys in reverse order (main key first, then modifiers)
                    for &scancode in scancodes.iter().rev() {
                        let mut flags = KEYEVENTF_SCANCODE | KEYEVENTF_KEYUP;
                        if Self::is_extended_scancode(scancode) {
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
                        std::thread::sleep(std::time::Duration::from_millis(1));
                    }
                }
                OutputAction::MultipleActions(actions) => {
                    /* ★v24.27 模拟手速: combo_key_gap_ms > 0 时**逐键顺序执行** ——
                     * 每键 按下 → 保持 duration → 弹起 → 间隔 gap; ↓→→Z / ↓+Z+Z 这类
                     * 序列指令靠它 (重复键 = 先后两次实按)。
                     * gap = 0 → 旧版整组同按 (一次 KEYDOWN 批量 + 保持 + 弹起批量)。 */
                    let gap = self.combo_key_gap_ms.load(Ordering::Relaxed);
                    if gap > 0 {
                        for a in actions.iter() {
                            let one = SmallVec::from_vec(vec![a.clone()]);
                            let mut press_inputs: SmallVec<[INPUT; 8]> = SmallVec::new();
                            self.collect_press_inputs(&one, &mut press_inputs);
                            if !press_inputs.is_empty() {
                                SendInput(&press_inputs, std::mem::size_of::<INPUT>() as i32);
                            }
                            std::thread::sleep(std::time::Duration::from_millis(duration));
                            let mut release_inputs: SmallVec<[INPUT; 8]> = SmallVec::new();
                            self.collect_release_inputs(&one, &mut release_inputs);
                            if !release_inputs.is_empty() {
                                SendInput(&release_inputs, std::mem::size_of::<INPUT>() as i32);
                            }
                            std::thread::sleep(std::time::Duration::from_millis(gap));
                        }
                    } else {
                        let mut press_inputs: SmallVec<[INPUT; 8]> = SmallVec::new();
                        self.collect_press_inputs(&actions, &mut press_inputs);
                        if !press_inputs.is_empty() {
                            SendInput(&press_inputs, std::mem::size_of::<INPUT>() as i32);
                        }

                        // Hold duration
                        std::thread::sleep(std::time::Duration::from_millis(duration));

                        let mut release_inputs: SmallVec<[INPUT; 8]> = SmallVec::new();
                        self.collect_release_inputs(&actions, &mut release_inputs);
                        if !release_inputs.is_empty() {
                            SendInput(&release_inputs, std::mem::size_of::<INPUT>() as i32);
                        }
                    }
                }
                OutputAction::SequenceControl(_) => {
                    // ★v24.31: 控制键由 worker 拦截, 永不到达模拟层
                }
            }
        }
    }

    /// Checks if a scancode requires KEYEVENTF_EXTENDEDKEY flag
    #[inline(always)]
    pub(super) fn is_extended_scancode(scancode: u16) -> bool {
        const EXTENDED_KEYS_BITMAP: u128 = (1u128 << 0x1D)
            | (1u128 << 0x38)
            | (1u128 << 0x47)
            | (1u128 << 0x48)
            | (1u128 << 0x49)
            | (1u128 << 0x4B)
            | (1u128 << 0x4D)
            | (1u128 << 0x4F)
            | (1u128 << 0x50)
            | (1u128 << 0x51)
            | (1u128 << 0x52)
            | (1u128 << 0x53)
            | (1u128 << 0x5B)
            | (1u128 << 0x5C);

        scancode < 128 && (EXTENDED_KEYS_BITMAP & (1u128 << scancode)) != 0
    }

    /// Simulates only the press event for an action
    #[inline(always)]
    pub fn simulate_press(&self, action: &OutputAction) {
        if unlikely(!self.is_process_whitelisted() || is_ime_composing() || !injection_guard_ok()) {
            return;
        }
        unsafe {
            match action {
                OutputAction::KeyboardKey(scancode) => {
                    let mut flags = KEYEVENTF_SCANCODE;
                    if Self::is_extended_scancode(*scancode) {
                        flags |= KEYEVENTF_EXTENDEDKEY;
                    }

                    let input = INPUT {
                        r#type: INPUT_KEYBOARD,
                        Anonymous: INPUT_0 {
                            ki: KEYBDINPUT {
                                wVk: VIRTUAL_KEY(0),
                                wScan: *scancode,
                                dwFlags: flags,
                                time: 0,
                                dwExtraInfo: SIMULATED_EVENT_MARKER,
                            },
                        },
                    };
                    SendInput(&[input], std::mem::size_of::<INPUT>() as i32);
                }
                OutputAction::MouseButton(button) => {
                    use windows::Win32::UI::Input::KeyboardAndMouse::*;

                    let down_flag = match button {
                        MouseButton::Left => MOUSEEVENTF_LEFTDOWN,
                        MouseButton::Right => MOUSEEVENTF_RIGHTDOWN,
                        MouseButton::Middle => MOUSEEVENTF_MIDDLEDOWN,
                        MouseButton::X1 | MouseButton::X2 => MOUSEEVENTF_XDOWN,
                    };

                    let mouse_data = match button {
                        MouseButton::X1 => 1,
                        MouseButton::X2 => 2,
                        _ => 0,
                    };

                    let input = INPUT {
                        r#type: INPUT_MOUSE,
                        Anonymous: INPUT_0 {
                            mi: MOUSEINPUT {
                                dx: 0,
                                dy: 0,
                                mouseData: mouse_data,
                                dwFlags: down_flag,
                                time: 0,
                                dwExtraInfo: SIMULATED_EVENT_MARKER,
                            },
                        },
                    };
                    SendInput(&[input], std::mem::size_of::<INPUT>() as i32);
                }
                OutputAction::MouseMove(_, _) => {
                    // Mouse movement doesn't have press/release states
                }
                OutputAction::MouseScroll(_, _) => {
                    // Mouse scroll doesn't have press/release states
                }
                OutputAction::KeyCombo(scancodes) => {
                    for &scancode in scancodes.iter() {
                        let mut flags = KEYEVENTF_SCANCODE;
                        if Self::is_extended_scancode(scancode) {
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
                        std::thread::sleep(std::time::Duration::from_millis(1));
                    }
                }
                OutputAction::MultipleActions(actions) => {
                    // ★v24.26 按住模式不拆组: 重复键的第二次 KEYDOWN/KEYUP 被
                    // 系统忽略 (无害), 按住 ↓+Z+Z = 按住 ↓+Z, 符合"按住期间"语义。
                    // Collect and send all press events in a single call
                    let mut inputs: SmallVec<[INPUT; 8]> = SmallVec::new();
                    self.collect_press_inputs(actions, &mut inputs);
                    if !inputs.is_empty() {
                        SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
                    }
                }
                OutputAction::SequenceControl(_) => {
                    // ★v24.31: 控制键由 worker 拦截, 永不到达模拟层
                }
            }
        }
    }

    /// Simulates only the release event for an action
    /// (弹起不设保险: 只拦按下, 弹起永远放行, 避免按键卡死)
    #[inline(always)]
    pub fn simulate_release(&self, action: &OutputAction) {
        unsafe {
            match action {
                OutputAction::KeyboardKey(scancode) => {
                    let mut flags = KEYEVENTF_SCANCODE | KEYEVENTF_KEYUP;
                    if Self::is_extended_scancode(*scancode) {
                        flags |= KEYEVENTF_EXTENDEDKEY;
                    }

                    let input = INPUT {
                        r#type: INPUT_KEYBOARD,
                        Anonymous: INPUT_0 {
                            ki: KEYBDINPUT {
                                wVk: VIRTUAL_KEY(0),
                                wScan: *scancode,
                                dwFlags: flags,
                                time: 0,
                                dwExtraInfo: SIMULATED_EVENT_MARKER,
                            },
                        },
                    };
                    SendInput(&[input], std::mem::size_of::<INPUT>() as i32);
                }
                OutputAction::MouseButton(button) => {
                    use windows::Win32::UI::Input::KeyboardAndMouse::*;

                    let up_flag = match button {
                        MouseButton::Left => MOUSEEVENTF_LEFTUP,
                        MouseButton::Right => MOUSEEVENTF_RIGHTUP,
                        MouseButton::Middle => MOUSEEVENTF_MIDDLEUP,
                        MouseButton::X1 | MouseButton::X2 => MOUSEEVENTF_XUP,
                    };

                    let mouse_data = match button {
                        MouseButton::X1 => 1,
                        MouseButton::X2 => 2,
                        _ => 0,
                    };

                    let input = INPUT {
                        r#type: INPUT_MOUSE,
                        Anonymous: INPUT_0 {
                            mi: MOUSEINPUT {
                                dx: 0,
                                dy: 0,
                                mouseData: mouse_data,
                                dwFlags: up_flag,
                                time: 0,
                                dwExtraInfo: SIMULATED_EVENT_MARKER,
                            },
                        },
                    };
                    SendInput(&[input], std::mem::size_of::<INPUT>() as i32);
                }
                OutputAction::MouseMove(_, _) => {
                    // Mouse movement doesn't have press/release states
                }
                OutputAction::MouseScroll(_, _) => {
                    // Mouse scroll doesn't have press/release states
                }
                OutputAction::KeyCombo(scancodes) => {
                    for &scancode in scancodes.iter().rev() {
                        let mut flags = KEYEVENTF_SCANCODE | KEYEVENTF_KEYUP;
                        if Self::is_extended_scancode(scancode) {
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
                        std::thread::sleep(std::time::Duration::from_millis(1));
                    }
                }
                OutputAction::MultipleActions(actions) => {
                    // Collect and send all release events in a single call
                    let mut inputs: SmallVec<[INPUT; 8]> = SmallVec::new();
                    self.collect_release_inputs(actions, &mut inputs);
                    if !inputs.is_empty() {
                        SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
                    }
                }
                OutputAction::SequenceControl(_) => {
                    // ★v24.31: 控制键由 worker 拦截, 永不到达模拟层
                }
            }
        }
    }

    /// Collects press INPUT events from actions into a buffer
    #[inline(always)]
    pub(super) fn collect_press_inputs(
        &self,
        actions: &SmallVec<[OutputAction; 4]>,
        inputs: &mut SmallVec<[INPUT; 8]>,
    ) {
        for action in actions.iter() {
            match action {
                OutputAction::KeyboardKey(scancode) => {
                    let mut flags = KEYEVENTF_SCANCODE;
                    if Self::is_extended_scancode(*scancode) {
                        flags |= KEYEVENTF_EXTENDEDKEY;
                    }

                    inputs.push(INPUT {
                        r#type: INPUT_KEYBOARD,
                        Anonymous: INPUT_0 {
                            ki: KEYBDINPUT {
                                wVk: VIRTUAL_KEY(0),
                                wScan: *scancode,
                                dwFlags: flags,
                                time: 0,
                                dwExtraInfo: SIMULATED_EVENT_MARKER,
                            },
                        },
                    });
                }
                OutputAction::MouseButton(button) => {
                    use windows::Win32::UI::Input::KeyboardAndMouse::*;

                    let down_flag = match button {
                        MouseButton::Left => MOUSEEVENTF_LEFTDOWN,
                        MouseButton::Right => MOUSEEVENTF_RIGHTDOWN,
                        MouseButton::Middle => MOUSEEVENTF_MIDDLEDOWN,
                        MouseButton::X1 | MouseButton::X2 => MOUSEEVENTF_XDOWN,
                    };

                    let mouse_data = match button {
                        MouseButton::X1 => 1,
                        MouseButton::X2 => 2,
                        _ => 0,
                    };

                    inputs.push(INPUT {
                        r#type: INPUT_MOUSE,
                        Anonymous: INPUT_0 {
                            mi: MOUSEINPUT {
                                dx: 0,
                                dy: 0,
                                mouseData: mouse_data,
                                dwFlags: down_flag,
                                time: 0,
                                dwExtraInfo: SIMULATED_EVENT_MARKER,
                            },
                        },
                    });
                }
                OutputAction::KeyCombo(scancodes) => {
                    for &scancode in scancodes.iter() {
                        let mut flags = KEYEVENTF_SCANCODE;
                        if Self::is_extended_scancode(scancode) {
                            flags |= KEYEVENTF_EXTENDEDKEY;
                        }

                        inputs.push(INPUT {
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
                        });
                    }
                }
                OutputAction::MultipleActions(nested_actions) => {
                    // Recursively collect nested actions
                    self.collect_press_inputs(nested_actions, inputs);
                }
                OutputAction::MouseMove(_, _) | OutputAction::MouseScroll(_, _) => {
                    // Skip actions without press state
                }
                OutputAction::SequenceControl(_) => {
                    // ★v24.31: 控制键由 worker 拦截, 永不到达模拟层
                }
            }
        }
    }

    /// Collects release INPUT events from actions into a buffer
    #[inline(always)]
    pub(super) fn collect_release_inputs(
        &self,
        actions: &SmallVec<[OutputAction; 4]>,
        inputs: &mut SmallVec<[INPUT; 8]>,
    ) {
        for action in actions.iter() {
            match action {
                OutputAction::KeyboardKey(scancode) => {
                    let mut flags = KEYEVENTF_SCANCODE | KEYEVENTF_KEYUP;
                    if Self::is_extended_scancode(*scancode) {
                        flags |= KEYEVENTF_EXTENDEDKEY;
                    }

                    inputs.push(INPUT {
                        r#type: INPUT_KEYBOARD,
                        Anonymous: INPUT_0 {
                            ki: KEYBDINPUT {
                                wVk: VIRTUAL_KEY(0),
                                wScan: *scancode,
                                dwFlags: flags,
                                time: 0,
                                dwExtraInfo: SIMULATED_EVENT_MARKER,
                            },
                        },
                    });
                }
                OutputAction::MouseButton(button) => {
                    use windows::Win32::UI::Input::KeyboardAndMouse::*;

                    let up_flag = match button {
                        MouseButton::Left => MOUSEEVENTF_LEFTUP,
                        MouseButton::Right => MOUSEEVENTF_RIGHTUP,
                        MouseButton::Middle => MOUSEEVENTF_MIDDLEUP,
                        MouseButton::X1 | MouseButton::X2 => MOUSEEVENTF_XUP,
                    };

                    let mouse_data = match button {
                        MouseButton::X1 => 1,
                        MouseButton::X2 => 2,
                        _ => 0,
                    };

                    inputs.push(INPUT {
                        r#type: INPUT_MOUSE,
                        Anonymous: INPUT_0 {
                            mi: MOUSEINPUT {
                                dx: 0,
                                dy: 0,
                                mouseData: mouse_data,
                                dwFlags: up_flag,
                                time: 0,
                                dwExtraInfo: SIMULATED_EVENT_MARKER,
                            },
                        },
                    });
                }
                OutputAction::KeyCombo(scancodes) => {
                    for &scancode in scancodes.iter().rev() {
                        let mut flags = KEYEVENTF_SCANCODE | KEYEVENTF_KEYUP;
                        if Self::is_extended_scancode(scancode) {
                            flags |= KEYEVENTF_EXTENDEDKEY;
                        }

                        inputs.push(INPUT {
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
                        });
                    }
                }
                OutputAction::MultipleActions(nested_actions) => {
                    // Recursively collect nested actions
                    self.collect_release_inputs(nested_actions, inputs);
                }
                OutputAction::MouseMove(_, _) | OutputAction::MouseScroll(_, _) => {
                    // Skip actions without release state
                }
                OutputAction::SequenceControl(_) => {
                    // ★v24.31: 控制键由 worker 拦截, 永不到达模拟层
                }
            }
        }
    }
}
