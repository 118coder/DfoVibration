//! 手柄捕获对账器 PadCaptureReconciler (按压窗口合并) + 校准设备挑选 —— 原 186-376, C3 归位。

use super::*;
/// 一次物理按压可能被 **两条通道各上报多次**:
/// 原始 HID 通道 → `GenericDevice` (`GAMEPAD_<vid>_<pid>_<tag>_H<usage>`),
/// XInput 通道 → `XInputCombo` (`GAMEPAD_045E_<按钮>`)。
///
/// 校准向导的语义是「按一下 = 记一步」; 若逐条消费, 一次按压会连吞两步、
/// 把 XInput 命名写进下一个槽位 (实机 bug: 按一个键 → A 槽和 B 槽同时被填)。
/// 更麻烦的是**按下摇杆 (L3/R3) 时的物理回弹 / 轻微推动**: 回弹事件 (`RS_Click` 之后弹成
/// `RS_Right`) 迟到几百毫秒才上报, 会覆盖下一个槽位; 摇摆还会让同一次按压里混进多个方向。

/// 因此本合并器采用**按压窗口**: 从窗口内第一条事件起 `hold` 毫秒内的**所有**事件都算同一次按压,
/// 按通道各留一条候选交给调用方挑选 (见 [`pick_calibration_device`])。窗口到期后关闭, 下一条重新开窗。
#[derive(Debug, Clone)]
pub struct PadCaptureReconciler {
    /// 当前按压窗口: (开窗时刻, 两条通道各自的候选)
    window: Option<(std::time::Instant, PadCaptureCandidates)>,
    hold: std::time::Duration,
}

/// 一次按压窗口内收集到的候选: 原始 HID 通道 / XInput 通道各至多一条。
#[derive(Debug, Clone, Default)]
pub struct PadCaptureCandidates {
    pub raw: Option<crate::state::InputDevice>,
    pub xinput: Option<crate::state::InputDevice>,
}

impl PadCaptureCandidates {
    fn push(&mut self, device: crate::state::InputDevice) {
        use crate::state::InputDevice;
        match device {
            InputDevice::GenericDevice { .. } => self.raw = Some(device),
            InputDevice::XInputCombo {
                device_type,
                button_ids,
            } => {
                /* 同一次按压内 XInput 可能分几帧上报 (按下 → 回弹): 合并 id 集合, 交给
                 * [`pick_calibration_device`] 按槽位过滤。若直接覆盖, 回弹会把真正的按键顶掉。 */
                let mut merged = match self.xinput.take() {
                    Some(InputDevice::XInputCombo {
                        button_ids: old, ..
                    }) => old,
                    _ => Vec::new(),
                };
                for id in button_ids {
                    if !merged.contains(&id) {
                        merged.push(id);
                    }
                }
                self.xinput = Some(InputDevice::XInputCombo {
                    device_type,
                    button_ids: merged,
                });
            }
            /* 键盘/鼠标事件不该出现在手柄捕获通道里, 忽略 */
            _ => {}
        }
    }
}

/// 按压窗口长度 (ms)。用户实测定值: 600ms 太钝 (每步都要等), 200ms 又兜不住摇杆回弹。
pub const PAD_CAPTURE_HOLD_MS: u64 = 300;

impl Default for PadCaptureReconciler {
    fn default() -> Self {
        Self::new(std::time::Duration::from_millis(PAD_CAPTURE_HOLD_MS))
    }
}

impl PadCaptureReconciler {
    pub fn new(hold: std::time::Duration) -> Self {
        Self {
            window: None,
            hold,
        }
    }

    /// 清空窗口 (开始/结束一次捕获时必须调用)。
    pub fn reset(&mut self) {
        self.window = None;
    }

    /// 喂入一条捕获事件; 返回 `Some` = 这次按压的窗口已结算, 交给调用方挑选。
    ///
    /// 窗口内的其它事件 (另一条通道 / 回弹) 不返回、只并入候选; 窗口到期由 [`Self::tick`]
    /// 或本条事件触发结算 (开窗即到期时不依赖 tick 帧率)。
    pub fn feed(
        &mut self,
        device: crate::state::InputDevice,
        now: std::time::Instant,
    ) -> Option<PadCaptureCandidates> {
        /* 上一条按压的窗口已到期 → 先结算它, 再为新事件开窗 */
        if let Some((start, _)) = self.window
            && now.duration_since(start) >= self.hold
        {
            let done = self.window.take().map(|(_, c)| c);
            let mut c = PadCaptureCandidates::default();
            c.push(device);
            self.window = Some((now, c));
            return done;
        }
        match self.window.as_mut() {
            None => {
                let mut c = PadCaptureCandidates::default();
                c.push(device);
                self.window = Some((now, c));
                None
            }
            Some((_, c)) => {
                c.push(device);
                None
            }
        }
    }

    /// 每帧调用: 窗口到期 → 产出这次按压的候选。
    pub fn tick(&mut self, now: std::time::Instant) -> Option<PadCaptureCandidates> {
        if let Some((start, _)) = self.window
            && now.duration_since(start) >= self.hold
        {
            return self.window.take().map(|(_, c)| c);
        }
        None
    }
}

/// XInput 输入 id (与 `xinput.rs::input_id_to_name` 的编号一致)。
pub(super) const XI_LS_CLICK: u32 = 0x07;
pub(super) const XI_RS_CLICK: u32 = 0x08;
pub(super) const XI_LS_DIRS: [u32; 4] = [0x10, 0x11, 0x12, 0x13];
pub(super) const XI_RS_DIRS: [u32; 4] = [0x14, 0x15, 0x16, 0x17];

/// 校准单步: 从一次按压的候选里挑出该槽位应记录的输入。
///
/// **摇杆类槽位 (方向 / 摇杆按下) 优先 XInput 通道** —— 摇杆的「方向 + 回弹」只有 XInput 有确定语义,
/// 原始位组合哈希会把「按下摇杆时轻轻推动」混进同一个名字里。
/// 此外按槽位类型**过滤** XInput 的多个 id (只留该槽位该有的那一个), 过滤后不是恰好一个就报错让用户重按。
/// 非摇杆槽位原始命名优先 (与设计一致)。
///
/// 返回 `Err(提示)` = 这次输入不干净, 调用方应提示用户重按、**不要**推进步骤。
pub fn pick_calibration_device(
    slot: &GamepadSlot,
    cands: &PadCaptureCandidates,
) -> Result<crate::state::InputDevice, String> {
    use crate::state::InputDevice;
    let is_stick = matches!(slot.kind, SlotKind::Stick | SlotKind::StickClick);
    if is_stick {
        if let Some(InputDevice::XInputCombo {
            device_type,
            button_ids,
        }) = cands.xinput.clone()
        {
            /* 该槽位只认这一类 id: 摇杆按下认 click, 方向认对应摇杆的四个方向 */
            let expected: &[u32] = match slot.id {
                22 => &[XI_LS_CLICK],
                23 => &[XI_RS_CLICK],
                8..=11 => &XI_LS_DIRS,
                12..=15 => &XI_RS_DIRS,
                _ => &[],
            };
            let kept: Vec<u32> = button_ids
                .iter()
                .copied()
                .filter(|id| expected.contains(id))
                .collect();
            if kept.len() == 1 {
                return Ok(InputDevice::XInputCombo {
                    device_type,
                    button_ids: kept,
                });
            }
            return Err(match slot.kind {
                SlotKind::StickClick => "按摇杆时别推动 — 请垂直按下再松开".to_string(),
                _ if kept.len() > 1 => "摇杆推直一点 — 一次只推一个方向".to_string(),
                _ => "没识别到摇杆方向 — 请推到底再松开".to_string(),
            });
        }
        /* 非 XInput 手柄: 退回原始命名 (内容无法校验, 但总比卡住好) */
        if let Some(raw) = cands.raw.clone() {
            return Ok(raw);
        }
        return Err("没等到摇杆输入 — 请操作一下再松开".to_string());
    }
    /* 非摇杆槽位: 原始命名优先; 只有 XInput 时要求恰好一个键 */
    if let Some(raw) = cands.raw.clone() {
        return Ok(raw);
    }
    if let Some(InputDevice::XInputCombo {
        device_type,
        button_ids,
    }) = cands.xinput.clone()
    {
        if button_ids.len() == 1 {
            return Ok(InputDevice::XInputCombo {
                device_type,
                button_ids,
            });
        }
        return Err("同时按了多个键 — 请只按一个键".to_string());
    }
    Err("没等到手柄输入 — 请按一下再松开".to_string())
}
