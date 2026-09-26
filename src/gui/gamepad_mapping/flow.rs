//! GpFlow 状态机输入边沿处理 (识别态边沿/键盘捕获/报文兜底, v22.0-v22.7) —— 原 1583-1812, C3 归位。

use crate::gui::SorahkGui;
use crate::gui::types::{GpEvent, GpFlow};
use eframe::egui;
use super::*;

impl SorahkGui {
    /// ★v22.0: 手柄页输入处理 (每帧调用, 由 `main_window` 在设置弹窗关闭时驱动)。
    ///
    /// 只做三件事: ① 识别态手柄实时按键边沿 → 选中/改选槽位; ② `AwaitKb` 键盘捕获;
    /// ③ `AwaitPad` 的原始报文兜底捕获。**绝不**在此改动识别开关 ——
    /// 修复旧版"识别中途按错一个键就把识别关掉"的结构性问题。
    pub(in crate::gui) fn handle_gamepad_flow(&mut self, ctx: &egui::Context) {
        /* ★v22.7: 校准抑制标志与状态机保持同步 (任何异常路径都不会漏关) ——
         * 校准期间输入线程不派发任何映射, 但实时状态照常发布。 */
        let calibrating = matches!(self.gp_flow, GpFlow::Calibrate { .. });
        if self.app_state.is_gp_calibrating() != calibrating {
            self.app_state.set_gp_calibrating(calibrating);
        }
        /* ★v22.0: 只在手柄页生效 —— 旧版切到别的页面后仍会吃键盘/手柄输入 (bug)。
         * 离开页面时结束进行中的捕获 (回退一层并关通道), 避免捕获卡在半路。 */
        if self.active_page != crate::gui::types::Page::Gamepad {
            if self.gp_flow.is_capturing() {
                self.cancel_gp();
            }
            if self.gp_quick_listen {
                self.gp_quick_listen = false;
            }
            /* 离开页面清掉实时边沿缓存, 回来时不会拿旧位图误判"新按下" */
            self.gp_prev_xinput = None;
            return;
        }
        /* ① 识别态: 手柄实时按键边沿 */
        if self.app_state.live_hid_pad().is_some() {
            if let Some(now) = self.app_state.live_hid_state() {
                let prev = self.gamepad_live_prev.clone();
                if matches!(self.gp_flow, GpFlow::AwaitPad { .. }) {
                    /* ★v22.3 校键改走"连发同款原始通道" (见下面 ③, 按下后松开才完成),
                     * 这里不再用语义 usage 抓取 —— 非标准手柄的 LT/RT 在语义通道里根本没有事件。 */
                } else if matches!(self.gp_flow, GpFlow::AwaitKb { .. }) {
                    /* ★v22.1 流程锁: 等键盘键期间按任何手柄键都**不改选** ——
                     * 否则用户会把 A 键的键盘键错记到 B 槽位。必须按键盘键或点「取消」。 */
                } else if matches!(
                    self.gp_flow,
                    GpFlow::ConfirmCapture { .. }
                        | GpFlow::ConfirmDelete { .. }
                        | GpFlow::Calibrate { .. }
                ) {
                    /* 等确认/删除/校准中都不动 —— 校准期间按手柄键不能触发"自动选中+弹键盘映射" */
                } else if ctx.input(|i| i.focused)
                    && let Some(id) = self.gp_newly_pressed_slot(&prev, &now)
                {
                    /* ★v24.6: 仅在窗口聚焦时自动选中 —— 否则工具开着去玩游戏时按手柄键
                     * 会误进"设键盘键"捕获态, 把游戏里的按键悄悄记成映射 (潜在串键/劫持)。 */
                    self.select_slot_autostep(id);
                }
                self.gamepad_live_prev = now;
            }
        } else {
            self.gamepad_live_prev = crate::state::LiveHidState::default();
        }

        /* ①b XInput 实时输入 → 按下新键选中槽位 (XInput 命名的槽位, 如摇杆方向/按下)。
         * ★v24.6: 走 live 位图而**不启用捕获模式** —— 捕获模式会跳过正常 XInput 派发,
         * 导致奔跑阈值不注册 / 方向不派发 (实机回归: 进游戏后奔跑失效)。 */
        {
            let now_x = self.app_state.live_xinput_state();
            let prev_x = self.gp_prev_xinput;
            if self.gp_quick_listen
                && ctx.input(|i| i.focused)
                && !self.gp_flow.is_busy()
                && let Some((vid, mask)) = now_x
                && let Some(id) = self.gp_newly_pressed_slot_xinput(vid, mask, prev_x)
            {
                self.gp_quick_listen = false;
                self.select_slot_autostep(id);
            }
            self.gp_prev_xinput = now_x;
        }

        /* ② AwaitKb: 键盘捕获 (松开判定) —— ★v22.1 排除鼠标键, 避免点热区被记成鼠标 */
        if matches!(self.gp_flow, GpFlow::AwaitKb { .. }) {
            self.capture_pressed_keys.retain(|vk| !is_mouse_vk(*vk));
            let current: std::collections::HashSet<u32> = Self::poll_all_pressed_keys()
                .into_iter()
                .filter(|vk| !is_mouse_vk(*vk))
                .collect();
            current
                .iter()
                .filter(|&vk| !self.capture_initial_pressed.contains(vk))
                .for_each(|&vk| {
                    self.capture_pressed_keys.insert(vk);
                });
            let any_released = self
                .capture_pressed_keys
                .iter()
                .any(|vk| !current.contains(vk));
            if any_released
                && let Some(name) = Self::format_captured_keys(&self.capture_pressed_keys)
            {
                self.capture_pressed_keys.clear();
                self.gp_flow = self.gp_flow.transition(GpEvent::KbCaptured(name));
            }
        }

        /* ③ AwaitPad: 连发同款"原始报文捕获"通道 —— 按下后**松开**才完成, 任何物理键/扳机都能抓到 */
        if let GpFlow::AwaitPad { slot } = self.gp_flow
            && let Some(cands) = self.poll_pad_capture()
        {
            match get_slot(slot).map(|s| pick_calibration_device(s, &cands)) {
                Some(Ok(device)) => {
                    self.app_state.set_raw_input_capture_mode(false);
                    self.gp_flow = self
                        .gp_flow
                        .transition(GpEvent::PadCaptured(device.to_string()));
                }
                Some(Err(msg)) => self.set_gamepad_warn(msg),
                None => {}
            }
        }

        /* ④ 一键校准向导: 覆盖全部热点圈 —— 实体键走连发同款原始通道, 方向类优先语义、认不出则原始学习 */
        if let GpFlow::Calibrate { step, .. } = self.gp_flow
            && let Some(slot_id) = calibration_slot(step)
        {
            self.gp_calibration_step(slot_id);
        }
    }

    /// ★v24.2: 从捕获通道取一次"已定案的按压"候选。
    ///
    /// 同一次物理按压会被原始 HID / XInput 两条通道各上报, 摇杆按下 (L3/R3) 还有回弹事件;
    /// 直接逐条消费会让校准向导一次按压连吞两步 (实机: 按一个键 → A 槽拿原始命名、B 槽拿
    /// `GAMEPAD_045E_A`), 或让回弹覆盖下一个键。交给 [`PadCaptureReconciler`] 按 `hold` 窗口
    /// (300ms) 合并, 两条通道各留一条候选, 由 [`pick_calibration_device`] 按槽位挑选。
    /// 返回 `None` = 窗口还没到期。
    pub(super) fn poll_pad_capture(&mut self) -> Option<PadCaptureCandidates> {
        let now = std::time::Instant::now();
        while let Some(device) = self.app_state.try_recv_raw_input_capture() {
            if let Some(done) = self.gp_pad_capture.feed(device, now) {
                return Some(done);
            }
        }
        self.gp_pad_capture.tick(now)
    }

    /// ★v23.0: 校准向导的单步处理 —— **全部 24 个热点统一走"连发同款原始通道"**。
    ///
    /// ★v24.2: 挑输入时按槽位类型过滤 (见 [`pick_calibration_device`]): 摇杆槽位优先 XInput,
    /// 且只留该槽位该有的那个输入 —— 摇杆回弹 / 轻微推动混进来的多余 id 被丢弃或直接判为不干净,
    /// 提示用户重按而不是把脏名字记进槽位。
    pub(super) fn gp_calibration_step(&mut self, slot_id: usize) {
        let Some(cands) = self.poll_pad_capture() else {
            return;
        };
        let Some(slot) = get_slot(slot_id) else {
            return;
        };
        match pick_calibration_device(slot, &cands) {
            Ok(device) => {
                if self.gp_record_calibration(slot_id, device) {
                    self.gp_advance_calibration();
                }
            }
            Err(msg) => self.set_gamepad_warn(msg),
        }
    }

    /// 校准向导推进一步; 走完则收尾。
    pub(super) fn gp_advance_calibration(&mut self) {
        self.gp_flow = self.gp_flow.transition(GpEvent::CalibrationStepDone);
        if self.gp_flow == GpFlow::Idle {
            self.end_calibration(&format!(
                "一键校准完成 — {} 个热点全部校对过了",
                CALIBRATION_ORDER.len()
            ));
        }
    }

    /// ★v24.0: 记录一步校对 —— **就是写一条普通映射** (触发键 = 捕获到的原始命名,
    /// 备注 = 系统备注)。与连发映射页完全同构, 没有任何独立校准表。
    ///
    /// ★v24.3: 触发键若已被**别的槽位**占着, 说明历史数据串位 (旧版本 bug 把键记错槽)。
    /// 此时**把该键移到当前槽位** (清掉旧槽位) 而不是拦下 —— 实机症状: 旧配置把 A 键的位组合
    /// 记在了 X 槽, 用户重新校对按 A 时被"这个键已经记给 X 键"拦住, 看起来就是"A 被识别成 X"。
    /// 返回是否算作完成 (false 仅用于槽位不存在等异常)。
    pub(super) fn gp_record_calibration(
        &mut self,
        slot_id: usize,
        device: crate::state::InputDevice,
    ) -> bool {
        let Some(slot) = get_slot(slot_id) else {
            return true;
        };
        let name = device.to_string();
        /* 写触发键; 旧数据串位 (该键已被别的槽位占用) 会被自动移到当前槽位 */
        if let Some(prev_label) = set_slot_trigger_moving(&mut self.config, slot, name) {
            self.set_gamepad_toast(format!(
                "该键原属「{prev_label}」(旧数据串位), 已改记到「{}」",
                slot.label
            ));
        }
        /* 顺便把这只手柄设为"实时识别目标" → SVG 按下即亮 */
        if let crate::state::InputDevice::GenericDevice { button_id, .. } = device {
            let stable = (button_id >> 32) as u32;
            if let Some(info) = crate::rawinput::get_device_display_info(stable as u64) {
                self.app_state
                    .set_live_hid_pad(Some((info.vendor_id, info.product_id)));
            }
        }
        self.gp_save_and_reload();
        true
    }

    /// ★v22.6: 开始「快速校对手柄按键」向导 (覆盖全部热点圈)。
    pub(super) fn start_calibration(&mut self) {
        self.capture_pressed_keys.clear();
        self.gp_pad_capture.reset();
        self.gp_quick_listen = false;
        self.gp_prev_xinput = None;
        self.gp_flow = self
            .gp_flow
            .transition(GpEvent::StartCalibration {
                total: CALIBRATION_ORDER.len(),
            });
        self.app_state.set_raw_input_capture_mode(true);
    }

    /// 结束校准向导 (完成/取消): 关通道 + 关抑制 + 提示。
    pub(super) fn end_calibration(&mut self, msg: &str) {
        self.app_state.set_raw_input_capture_mode(false);
        self.app_state.set_gp_calibrating(false);
        self.gp_pad_capture.reset();
        self.gp_prev_xinput = None;
        self.set_gamepad_toast(msg);
    }

}
