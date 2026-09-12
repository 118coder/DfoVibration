//! 手柄可视化快速映射页。
//!
//! 左侧渲染内嵌的 Xbox 手柄 SVG, 按键区域做成可点击热点;
//! 右侧显示选中槽位的当前映射, 并支持两步快速设置:
//! 1. 捕获物理手柄按键作为触发键;
//! 2. 捕获键盘/鼠标按键作为目标键。
//!
//! 热点坐标标定: SVG viewBox 为 "22 135 545 401" (无文字横版), 槽位坐标 = SVG
//! 元素中心经图层变换后归一化到该 viewBox。修改 `resources/gamepad.svg` 后必须
//! 重新标定 [`SLOTS`] (带文字的原始备份见 docs/gamepad-original-with-text.svg.bak)。

use crate::config::{AppConfig, KeyMapping};
use crate::gui::SorahkGui;
use crate::gui::theme::{self, Theme};
use crate::gui::types::{GpCaptureKind, GpEvent, GpFlow};
use eframe::egui;

/// SVG 渲染倍率 (2x 保证 HiDPI 下不模糊)。
const SVG_RENDER_SCALE: f32 = 2.0;

/// SVG 原始尺寸 (viewBox), 用于计算显示宽高比。
const SVG_SIZE: egui::Vec2 = egui::vec2(545.0, 401.0);

/// 槽位类别, 用于热点配色与右侧图例。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotKind {
    /// 扳机 (LT/RT)
    Trigger,
    /// 肩键 (LB/RB)
    Shoulder,
    /// 十字键
    Dpad,
    /// 摇杆方向
    Stick,
    /// 摇杆按下
    StickClick,
    /// ABXY 面键
    Button,
    /// 系统键 (Back/Start)
    System,
}

impl SlotKind {
    /// 图例文案。
    pub fn legend(self) -> &'static str {
        match self {
            SlotKind::Trigger => "扳机",
            SlotKind::Shoulder => "肩键",
            SlotKind::Dpad => "十字键",
            SlotKind::Stick => "摇杆方向",
            SlotKind::StickClick => "摇杆按下",
            SlotKind::Button => "ABXY 面键",
            SlotKind::System => "系统键",
        }
    }

    /// 热点基础色 (RGB), 明暗主题通用。
    pub fn base_rgb(self) -> (u8, u8, u8) {
        match self {
            SlotKind::Trigger => (235, 146, 52),
            SlotKind::Shoulder => (175, 122, 224),
            SlotKind::Dpad => (72, 170, 235),
            SlotKind::Stick => (80, 200, 130),
            SlotKind::StickClick => (235, 200, 80),
            SlotKind::Button => (124, 140, 248),
            SlotKind::System => (150, 165, 180),
        }
    }
}

/// 手柄上的一个可配置槽位。
#[derive(Debug, Clone, Copy)]
pub struct GamepadSlot {
    pub id: usize,
    pub label: &'static str,
    /// 显示在手柄图热点内的短标签 (方向用箭头, 完整名称见悬停提示)
    pub short: &'static str,
    pub default_trigger: &'static str,
    /// 热点中心在图片中的归一化坐标 (0..1, 0..1)
    pub pos: (f32, f32),
    /// 热点半径 (归一化, 以图片宽度为基准)
    pub radius: f32,
    pub kind: SlotKind,
}

/// 所有可配置手柄槽位。
///
/// 坐标按 `resources/gamepad.svg` (无文字版, viewBox "22 135 545 401") 精确标定:
/// - 扳机/肩键热点骑跨机身顶部肩线 (前视图惯例);
/// - 摇杆/十字键/面键/系统键热点位于机身对应按钮中心。
/// 注意: XInput 层不支持 Guide (Xbox 徽标) 键, 故不提供该槽位。
pub const SLOTS: &[GamepadSlot] = &[
    // ── 扳机 / 肩键 (机身顶部边缘, 前视图惯例: 骑跨肩线) ──
    GamepadSlot { id: 0, label: "LT", short: "LT", default_trigger: "GAMEPAD_045E_LT", pos: (0.2826, 0.0923), radius: 0.038, kind: SlotKind::Trigger },
    GamepadSlot { id: 1, label: "LB", short: "LB", default_trigger: "GAMEPAD_045E_LB", pos: (0.3963, 0.1172), radius: 0.038, kind: SlotKind::Shoulder },
    GamepadSlot { id: 2, label: "RT", short: "RT", default_trigger: "GAMEPAD_045E_RT", pos: (0.7376, 0.0923), radius: 0.038, kind: SlotKind::Trigger },
    GamepadSlot { id: 3, label: "RB", short: "RB", default_trigger: "GAMEPAD_045E_RB", pos: (0.6239, 0.1172), radius: 0.038, kind: SlotKind::Shoulder },
    // ── 十字键 (中心 0.380/0.527, 臂长 0.048x/0.059y) ──
    GamepadSlot { id: 4, label: "十字键·上", short: "↑", default_trigger: "GAMEPAD_045E_DPad_Up", pos: (0.3802, 0.4678), radius: 0.0284, kind: SlotKind::Dpad },
    GamepadSlot { id: 5, label: "十字键·下", short: "↓", default_trigger: "GAMEPAD_045E_DPad_Down", pos: (0.3802, 0.5856), radius: 0.0284, kind: SlotKind::Dpad },
    GamepadSlot { id: 6, label: "十字键·左", short: "←", default_trigger: "GAMEPAD_045E_DPad_Left", pos: (0.3325, 0.5267), radius: 0.0284, kind: SlotKind::Dpad },
    GamepadSlot { id: 7, label: "十字键·右", short: "→", default_trigger: "GAMEPAD_045E_DPad_Right", pos: (0.4279, 0.5267), radius: 0.0284, kind: SlotKind::Dpad },
    // ── 左摇杆 (中心 0.249/0.316) ──
    GamepadSlot { id: 8, label: "左摇杆·上", short: "↑", default_trigger: "GAMEPAD_045E_LS_Up", pos: (0.2492, 0.2611), radius: 0.0251, kind: SlotKind::Stick },
    GamepadSlot { id: 9, label: "左摇杆·下", short: "↓", default_trigger: "GAMEPAD_045E_LS_Down", pos: (0.2492, 0.3709), radius: 0.0251, kind: SlotKind::Stick },
    GamepadSlot { id: 10, label: "左摇杆·左", short: "←", default_trigger: "GAMEPAD_045E_LS_Left", pos: (0.2088, 0.3160), radius: 0.0251, kind: SlotKind::Stick },
    GamepadSlot { id: 11, label: "左摇杆·右", short: "→", default_trigger: "GAMEPAD_045E_LS_Right", pos: (0.2896, 0.3160), radius: 0.0251, kind: SlotKind::Stick },
    // ── 右摇杆 (中心 0.628/0.517) ──
    GamepadSlot { id: 12, label: "右摇杆·上", short: "↑", default_trigger: "GAMEPAD_045E_RS_Up", pos: (0.6283, 0.4696), radius: 0.0251, kind: SlotKind::Stick },
    GamepadSlot { id: 13, label: "右摇杆·下", short: "↓", default_trigger: "GAMEPAD_045E_RS_Down", pos: (0.6283, 0.5644), radius: 0.0251, kind: SlotKind::Stick },
    GamepadSlot { id: 14, label: "右摇杆·左", short: "←", default_trigger: "GAMEPAD_045E_RS_Left", pos: (0.5934, 0.5170), radius: 0.0251, kind: SlotKind::Stick },
    GamepadSlot { id: 15, label: "右摇杆·右", short: "→", default_trigger: "GAMEPAD_045E_RS_Right", pos: (0.6632, 0.5170), radius: 0.0251, kind: SlotKind::Stick },
    // ── ABXY 面键 ──
    GamepadSlot { id: 16, label: "X 键", short: "X", default_trigger: "GAMEPAD_045E_X", pos: (0.6833, 0.3155), radius: 0.036, kind: SlotKind::Button },
    GamepadSlot { id: 17, label: "Y 键", short: "Y", default_trigger: "GAMEPAD_045E_Y", pos: (0.7516, 0.2239), radius: 0.036, kind: SlotKind::Button },
    GamepadSlot { id: 18, label: "B 键", short: "B", default_trigger: "GAMEPAD_045E_B", pos: (0.8220, 0.3155), radius: 0.036, kind: SlotKind::Button },
    GamepadSlot { id: 19, label: "A 键", short: "A", default_trigger: "GAMEPAD_045E_A", pos: (0.7516, 0.4067), radius: 0.036, kind: SlotKind::Button },
    // ── 系统键 ──
    GamepadSlot { id: 20, label: "Back", short: "Back", default_trigger: "GAMEPAD_045E_Back", pos: (0.4284, 0.3145), radius: 0.0257, kind: SlotKind::System },
    GamepadSlot { id: 21, label: "Start", short: "Start", default_trigger: "GAMEPAD_045E_Start", pos: (0.5723, 0.3145), radius: 0.0257, kind: SlotKind::System },
    // ── 摇杆按下 (最后注册, 命中优先于方向) ──
    GamepadSlot { id: 22, label: "左摇杆·按下", short: "", default_trigger: "GAMEPAD_045E_LS_Click", pos: (0.2492, 0.3160), radius: 0.020, kind: SlotKind::StickClick },
    GamepadSlot { id: 23, label: "右摇杆·按下", short: "", default_trigger: "GAMEPAD_045E_RS_Click", pos: (0.6283, 0.5170), radius: 0.020, kind: SlotKind::StickClick },
];

pub fn get_slot(id: usize) -> Option<&'static GamepadSlot> {
    SLOTS.iter().find(|s| s.id == id)
}

/* ★v23.0: 已删除"标准 HID usage 表"(A=1/B=2/…/R3=12) 与"语义轴方向表"。
 * 它们假定第三方手柄遵循微软标准序, 对 20BC:5159 这类手柄会整体错位,
 * 并与原始通道命名并存时造成"按一个键触发两个"。现在一律以**连发同款原始位组合**为准。 */

/// ★v22.4: 引导式一键校准的按键顺序 —— **覆盖全部 24 个热点圈** (用户要求"全部校对过去")。
/// 与 SVG 图的阅读顺序一致 (面键 → 肩键 → 扳机 → 系统键 → 摇杆按下 → 十字/摇杆四方向)。
///
/// 前 12 个是实体按键/扳机 (原始位组合通道校准, 按设备记住);
/// 后 12 个是十字键与左右摇杆的四方向 (固定语义: 用相对基线的方向位判定, 无需厂商数据,
/// 这里是"推动确认" — 推到位即通过, 检测不到会停在该步提示)。
pub const CALIBRATION_ORDER: &[usize] = &[
    // ① 面键 / 肩键 / 扳机 / 系统键 / 摇杆按下 (原始通道校准)
    19, 18, 16, 17, 1, 3, 0, 2, 20, 21, 22, 23,
    // ② 十字键四方向
    4, 5, 6, 7,
    // ③ 左摇杆四方向
    8, 9, 10, 11,
    // ④ 右摇杆四方向
    12, 13, 14, 15,
];

/// ★v22.4: 第 `step` 步要校准的槽位 id。
pub fn calibration_slot(step: usize) -> Option<usize> {
    CALIBRATION_ORDER.get(step).copied()
}

/// 校准进度行用的短名 (避免长中文名被拆行)。
fn calibration_short_label(slot_id: usize) -> &'static str {
    match slot_id {
        22 => "L3",
        23 => "R3",
        4 => "十字↑",
        5 => "十字↓",
        6 => "十字←",
        7 => "十字→",
        8 => "左摇↑",
        9 => "左摇↓",
        10 => "左摇←",
        11 => "左摇→",
        12 => "右摇↑",
        13 => "右摇↓",
        14 => "右摇←",
        15 => "右摇→",
        _ => get_slot(slot_id).map(|s| s.label).unwrap_or("?"),
    }
}

/// 一次物理按压可能被 **两条通道各上报多次**:
/// 原始 HID 通道 → `GenericDevice` (`GAMEPAD_<vid>_<pid>_<tag>_H<usage>`),
/// XInput 通道 → `XInputCombo` (`GAMEPAD_045E_<按钮>`)。
///
/// 校准向导的语义是「按一下 = 记一步」; 若逐条消费, 一次按压会连吞两步、
/// 把 XInput 命名写进下一个槽位 (实机 bug: 按一个键 → A 槽和 B 槽同时被填)。
/// 更麻烦的是**按下摇杆 (L3/R3) 时的物理回弹 / 轻微推动**: 回弹事件 (`RS_Click` 之后弹成
/// `RS_Right`) 迟到几百毫秒才上报, 会覆盖下一个槽位; 摇摆还会让同一次按压里混进多个方向。
///
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
const XI_LS_CLICK: u32 = 0x07;
const XI_RS_CLICK: u32 = 0x08;
const XI_LS_DIRS: [u32; 4] = [0x10, 0x11, 0x12, 0x13];
const XI_RS_DIRS: [u32; 4] = [0x14, 0x15, 0x16, 0x17];

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

/// 触发键的短名 (校准向导里逐条展示用): 去掉设备前缀, 一眼看出串键。
/// 原始命名 `GAMEPAD_20BC_5159_DEV5A27EA46_H680322833` → `H680322833`;
/// XInput 命名 `GAMEPAD_045E_RS_Click` → `045E_RS_Click`。
pub fn short_trigger_name(name: &str) -> String {
    let Some(rest) = name.strip_prefix("GAMEPAD_") else {
        return name.to_string();
    };
    let parts: Vec<&str> = rest.split('_').collect();
    if parts.len() >= 4 && parts[2].starts_with("DEV") {
        parts[3..].join("_")
    } else {
        rest.to_string()
    }
}

/// ★v24.0: 槽位的**系统备注** —— 校准/映射的唯一关联键 (写入 `mapping.note`)。
///
/// 例: 「手柄A键（系统）」「手柄十字键·上（系统）」。
/// 它随映射一起保存在**当前预设**里 → 有线 / USB 接收器 / 蓝牙 各自哈希不同时,
/// 各用各的预设即可, 互不冲突 (用户要求)。
pub fn slot_note(label: &str) -> String {
    format!("手柄{label}（系统）")
}

/// 旧版备注格式 (`手柄·<标签>`) —— 仅供一次性迁移使用。
fn legacy_slot_note(label: &str) -> String {
    format!("手柄·{label}")
}

/// 查找槽位对应的映射 —— **只按系统备注匹配**。
/// (不再回落到 `default_trigger`: 那会把 XInput 命名 (`GAMEPAD_045E_*`) 引进来,
///  与原始通道命名各中一条 = "按一个键触发两个"。)
pub fn find_slot_mapping_index(config: &AppConfig, slot: &GamepadSlot) -> Option<usize> {
    let note = slot_note(slot.label);
    config.mappings.iter().position(|m| m.note == note)
}

/// ★v24.0: 一次性迁移旧备注 (`手柄·X` → `手柄X（系统）`), 触发键/目标键原样保留。
/// 同槽位若已有系统备注条目, 则删掉旧的重复条目。返回是否有改动 (需落盘)。
pub fn migrate_slot_notes(config: &mut AppConfig) -> bool {
    let mut changed = false;
    for slot in SLOTS {
        let new_note = slot_note(slot.label);
        let old_note = legacy_slot_note(slot.label);
        let old_idxs: Vec<usize> = config
            .mappings
            .iter()
            .enumerate()
            .filter(|(_, m)| m.note == old_note)
            .map(|(i, _)| i)
            .collect();
        if old_idxs.is_empty() {
            continue;
        }
        if config.mappings.iter().any(|m| m.note == new_note) {
            /* 已有新备注条目 → 旧的重复条目删掉 */
            for &i in old_idxs.iter().rev() {
                config.mappings.remove(i);
                changed = true;
            }
        } else {
            /* 第一条改名, 其余删掉 */
            let keep = old_idxs[0];
            config.mappings[keep].note = new_note;
            for &i in old_idxs.iter().skip(1).rev() {
                config.mappings.remove(i);
            }
            changed = true;
        }
    }
    changed
}

/// ★v24.0: 触发键是否与当前"原始位组合"匹配 (SVG 按下即亮 / 识别态自动选中)。
/// 只有 RawInput 原始通道的 `GenericDevice` 命名能匹配 —— 与连发映射同一编码。
fn trigger_matches_raw(trigger: &str, vid: u16, pid: u16, pos: u32) -> bool {
    if pos == 0 {
        return false;
    }
    let upper = trigger.to_uppercase();
    if !upper.starts_with(&format!("GAMEPAD_{vid:04X}_{pid:04X}_")) {
        return false;
    }
    matches!(
        crate::state::AppState::input_name_to_device(trigger),
        Some(crate::state::InputDevice::GenericDevice { button_id, .. }) if button_id as u32 == pos
    )
}

/// ★v24.6: 触发键是否与**实时 XInput 输入位图**匹配 (XInput 命名的槽位, 如摇杆方向/按下)。
/// `mask` 的 bit[id] = 该 XInput input id 当前按下 (见 `xinput.rs::inputs_to_bitset`)。
/// 走这条实时位图而不启用捕获模式 —— 捕获模式会抑制正常映射派发 (奔跑/方向失效的根因)。
pub fn trigger_matches_xinput(trigger: &str, vid: u16, mask: u32) -> bool {
    if mask == 0 {
        return false;
    }
    let Some(crate::state::InputDevice::XInputCombo {
        device_type,
        button_ids,
    }) = crate::state::AppState::input_name_to_device(trigger)
    else {
        return false;
    };
    let v = match device_type {
        crate::state::DeviceType::Gamepad(v) | crate::state::DeviceType::Joystick(v) => v,
        _ => return false,
    };
    v == vid
        && !button_ids.is_empty()
        && button_ids
            .iter()
            .all(|id| mask & (1u32 << (id & 31)) != 0)
}

/// ★v22.1: 鼠标虚拟键 (LBUTTON/RBUTTON/MBUTTON/XBUTTON1/2) —— 手柄页键盘捕获必须排除,
/// 否则点 SVG 上的键会被记成"鼠标按键"(用户实测的污染路径)。
pub fn is_mouse_vk(vk: u32) -> bool {
    matches!(vk, 0x01 | 0x02 | 0x04 | 0x05 | 0x06)
}

/// 设置槽位的触发键: 已有映射则更新, 没有则新建 (备注写系统备注)。
/// 返回映射在 config.mappings 中的下标。
pub fn set_slot_trigger(config: &mut AppConfig, slot: &GamepadSlot, trigger: String) -> usize {
    /* ★v21.7d: 触发键先去重+规范排序 (手柄多键组合也适用) */
    let trigger = crate::util::normalize_key_combo(&trigger);
    if let Some(idx) = find_slot_mapping_index(config, slot) {
        config.mappings[idx].trigger_key = trigger;
        config.mappings[idx].note = slot_note(slot.label);
        idx
    } else {
        config.mappings.push(KeyMapping {
            trigger_key: trigger,
            target_keys: Default::default(),
            interval: None,
            event_duration: None,
            /* ★v24.1: 新键位**默认不连发** —— 用户期望"按一下 = 一个动作"。
             * 旧默认 true 配合全局 interval=5ms, 按一下会立刻补发一次 → 游戏端看起来"按一个键出两个"。
             * 需要连发的键可在「更多设置」里单独勾选。 */
            turbo_enabled: false,
            move_speed: 5,
            double_tap_enabled: false,
            double_tap_gap_ms: 50,
            run_enabled: false,
            run_threshold: 80,
            run_recheck: true,
            note: slot_note(slot.label),
        });
        config.mappings.len() - 1
    }
}

/// ★v24.3: 给槽位写触发键, 并**自愈旧数据串位** —— 若该触发键已被别的槽位占用,
/// 先把它从旧槽位移除 (同一个物理键不能属于两个槽位, 否则一个键亮两处 / 触发两条)。
/// 返回被移走的旧槽位标签 (供提示), 没有冲突则为 `None`。
///
/// 为什么是"移动"而不是"拒绝": 实机症状是历史配置把 A 键的位组合记在了 X 槽,
/// 用户重新校对按 A 时被拒绝, 表现为"A 键被识别成了 X 键"。校准时用户正在按的就是答案,
/// 所以以当前槽位为准。
pub fn set_slot_trigger_moving(
    config: &mut AppConfig,
    slot: &GamepadSlot,
    trigger: String,
) -> Option<String> {
    let name = trigger.clone();
    let mut moved_from = None;
    for other in SLOTS {
        if other.id == slot.id {
            continue;
        }
        if let Some(i) = find_slot_mapping_index(config, other)
            && config.mappings[i].trigger_key == name
        {
            config.mappings.remove(i);
            moved_from = Some(other.label.to_string());
            break;
        }
    }
    set_slot_trigger(config, slot, trigger);
    moved_from
}

/// 向槽位添加一个目标键 (不重复)。返回是否发生修改。
/// ★v24.0: 映射必须已由校对创建; 不存在则不改动 (调用方负责提示先校对)。
pub fn add_slot_target(config: &mut AppConfig, slot: &GamepadSlot, target: String) -> bool {
    let Some(idx) = find_slot_mapping_index(config, slot) else {
        return false;
    };
    let len_before = config.mappings[idx].target_keys.len();
    config.mappings[idx].add_target_key(target);
    config.mappings[idx].note = slot_note(slot.label);
    config.mappings[idx].target_keys.len() != len_before
}

/// 清空槽位的目标键。
pub fn clear_slot_targets(config: &mut AppConfig, slot: &GamepadSlot) {
    if let Some(idx) = find_slot_mapping_index(config, slot) {
        config.mappings[idx].clear_target_keys();
        config.mappings[idx].note = slot_note(slot.label);
    }
}

/// 删除槽位对应的映射。
pub fn remove_slot_mapping(config: &mut AppConfig, slot: &GamepadSlot) {
    if let Some(idx) = find_slot_mapping_index(config, slot) {
        config.mappings.remove(idx);
    }
}

/// 获取槽位当前映射的触发键 (未校对 → 空串)。
pub fn slot_trigger_display(config: &AppConfig, slot: &GamepadSlot) -> String {
    if let Some(idx) = find_slot_mapping_index(config, slot) {
        config.mappings[idx].trigger_key.clone()
    } else {
        String::new()
    }
}

/// 获取槽位当前映射的目标键列表。
pub fn slot_targets(config: &AppConfig, slot: &GamepadSlot) -> Vec<String> {
    if let Some(idx) = find_slot_mapping_index(config, slot) {
        config.mappings[idx].target_keys.to_vec()
    } else {
        Vec::new()
    }
}

/// 使用 resvg 将内嵌手柄 SVG 渲染为 egui 纹理 (2x 超采样, HiDPI 清晰)。
pub fn load_gamepad_texture(ctx: &egui::Context, dark: bool) -> Option<egui::TextureHandle> {
    // 双主题变体: 亮色 = resources/gamepad-light.svg (仅 style 颜色不同, 几何一致,
    // 热点坐标共用同一 viewBox)
    let svg_data: &[u8] = if dark {
        include_bytes!("../../resources/gamepad.svg")
    } else {
        include_bytes!("../../resources/gamepad-light.svg")
    };

    let mut opt = resvg::usvg::Options::default();
    opt.fontdb_mut().load_system_fonts();
    let tree = resvg::usvg::Tree::from_data(svg_data, &opt).ok()?;

    let size = tree.size().to_int_size();
    let scale = SVG_RENDER_SCALE;
    let width = (size.width() as f32 * scale) as u32;
    let height = (size.height() as f32 * scale) as u32;
    let mut pixmap = resvg::tiny_skia::Pixmap::new(width, height)?;
    let transform =
        resvg::tiny_skia::Transform::from_scale(scale, scale);
    resvg::render(&tree, transform, &mut pixmap.as_mut());

    let image_size = [pixmap.width() as usize, pixmap.height() as usize];
    let color_image = egui::ColorImage::from_rgba_unmultiplied(image_size, pixmap.data());
    Some(ctx.load_texture(
        if dark { "gamepad_svg_dark" } else { "gamepad_svg_light" },
        color_image,
        egui::TextureOptions::LINEAR,
    ))
}

/// 计算热点在指定显示矩形内的屏幕圆心与半径。
fn hotspot_rect(rect: egui::Rect, slot: &GamepadSlot) -> (egui::Pos2, f32) {
    let center = egui::pos2(
        rect.min.x + slot.pos.0 * rect.width(),
        rect.min.y + slot.pos.1 * rect.height(),
    );
    let radius = slot.radius * rect.width();
    (center, radius)
}

impl SorahkGui {
    /// 手柄可视化快速映射页主体: 左 (大图+图例) / 右 (分步面板) + 底部已配置映射。
    ///
    /// ★v22.0: 交互全部由 [`GpFlow`] 状态机驱动 (唯一真相)。
    /// 输入边沿 (手柄实时按键 / 键盘捕获) 在 `handle_gamepad_flow` 里处理, 本函数只渲染。
    pub(super) fn render_gamepad_page(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        /* ★v24.0: 首次进入手柄页 → 把旧备注 (`手柄·X`) 迁移为系统备注 (`手柄X（系统）`),
         * 触发键/目标键原样保留 (会话内一次)。 */
        if !self.gp_cleanup_done {
            self.gp_cleanup_done = true;
            if migrate_slot_notes(&mut self.config) {
                let _ = self.config.save_to_file("Config.toml");
                if let Err(e) = self.app_state.reload_config(self.config.clone()) {
                    eprintln!("Failed to reload config after slot note migration: {e}");
                }
            }
        }
        // 主题切换时重渲染对应变体 (按 dark 标记缓存)
        if self.gamepad_texture.is_none() || self.gamepad_texture_dark != self.dark_mode {
            self.gamepad_texture = load_gamepad_texture(ctx, self.dark_mode);
            self.gamepad_texture_dark = self.dark_mode;
        }
        let th = Theme::new(self.dark_mode);

        /* ★v22.0: 顶部识别状态条 + 就近提示 + 步骤引导 (一步一步来) */
        self.render_gamepad_status_bar(ui, &th);

        /* ★v24.8 排版重构: 两栏改用**显式宽度**约束。
         * 旧版右栏 `set_min_width(ui.available_width())` 在水平布局里会把整行撑破窗口 →
         * 整页出现横向滚动条 → 底部 chips 的 horizontal_wrapped 失去换行宽度被截断。 */
        th.card(ui, None, |ui| {
            let total_w = ui.available_width();
            let sp = theme::SP_L;
            let left_w = (total_w * 0.5).clamp(320.0, 620.0);
            let right_w = (total_w - left_w - sp).max(260.0);
            ui.horizontal_top(|ui| {
                // 左: 手柄图 + 热点 (横版构图; 宽度显式给定, 高度按原图比例)
                ui.vertical(|ui| {
                    ui.set_min_width(left_w);
                    ui.set_max_width(left_w);
                    let display_h = left_w * (SVG_SIZE.y / SVG_SIZE.x);
                    let clicked_slot =
                        self.render_gamepad_svg(ui, egui::vec2(left_w, display_h), &th);
                    /* 确认/删除等待态不响应改选, 避免打断用户确认 */
                    if let Some(id) = clicked_slot
                        && !self.gp_flow.is_busy()
                    {
                        self.select_slot_autostep(id);
                    }
                });

                ui.add_space(sp);

                // 右: 分步面板 (宽度显式给定 → 内部长文本才会正常换行)
                ui.vertical(|ui| {
                    ui.set_min_width(right_w);
                    ui.set_max_width(right_w);
                    self.render_gamepad_slot_panel(ui, &th);
                });
            });
        });

        self.render_gamepad_overview(ui, &th);
        self.render_mouse_map_dialog(ctx, &th);
    }

    /// ★v21.9: 「鼠标映射」小弹窗 —— 明确选择【滚动】或【点击】, 不再靠鼠标捕获 (避免污染)。
    fn render_mouse_map_dialog(&mut self, ctx: &egui::Context, th: &Theme) {
        if !self.gamepad_mouse_menu {
            return;
        }
        let Some(slot_id) = self.gp_flow.slot() else {
            self.gamepad_mouse_menu = false;
            return;
        };
        let Some(slot) = get_slot(slot_id) else {
            self.gamepad_mouse_menu = false;
            return;
        };
        let mut close = false;
        let mut pick: Option<&'static str> = None;
        egui::Window::new("mouse_map_dialog")
            .id(egui::Id::new("gamepad_mouse_map_dialog"))
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .frame(
                egui::Frame::window(&ctx.style())
                    .fill(th.surface)
                    .corner_radius(egui::CornerRadius::same(14))
                    .stroke(egui::Stroke::new(1.0, th.stroke)),
            )
            .show(ctx, |ui| {
                ui.set_min_width(320.0);
                ui.label(
                    egui::RichText::new(format!("鼠标映射 · {}", slot.label))
                        .size(15.0)
                        .strong()
                        .color(th.title),
                );
                ui.add_space(8.0);
                ui.label(th.weak("滚动"));
                ui.horizontal_wrapped(|ui| {
                    for (label, key) in [
                        ("↑ 上滚", "SCROLL_UP"),
                        ("↓ 下滚", "SCROLL_DOWN"),
                        ("← 左滚", "SCROLL_LEFT"),
                        ("→ 右滚", "SCROLL_RIGHT"),
                    ] {
                        if ui.add(th.secondary_button(label)).clicked() {
                            pick = Some(key);
                        }
                    }
                });
                ui.add_space(10.0);
                ui.label(th.weak("点击"));
                ui.horizontal_wrapped(|ui| {
                    for (label, key) in
                        [("鼠标左键", "LBUTTON"), ("鼠标右键", "RBUTTON"), ("鼠标中键", "MBUTTON")]
                    {
                        if ui.add(th.secondary_button(label)).clicked() {
                            pick = Some(key);
                        }
                    }
                });
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    if ui.add(th.secondary_button("关闭")).clicked() {
                        close = true;
                    }
                    ui.label(th.hint_text("选一个即加入这个键位的映射"));
                });
            });
        if let Some(key) = pick {
            /* ★v24.0: 映射必须已由校对创建 */
            if find_slot_mapping_index(&self.config, slot).is_some() {
                crate::gui::gamepad_mapping::add_slot_target(&mut self.config, slot, key.to_string());
                let _ = self.config.save_to_file("Config.toml");
                if let Err(e) = self.app_state.reload_config(self.config.clone()) {
                    eprintln!("Failed to reload config after mouse map: {e}");
                }
                self.set_gamepad_toast(format!("已设置: {} → {key}", slot.label));
            } else {
                self.set_gamepad_warn("这个键还没校对 — 请先点「一键校对手柄按键」");
            }
            self.gp_flow = GpFlow::Selected { slot: slot.id };
            close = true;
        }
        if close {
            self.gamepad_mouse_menu = false;
        }
    }

    /// ★v22.0: 手柄页顶部状态区 —— 识别状态条 + 就近警告 + 成功提示 + 步骤引导。
    /// 每一步只讲一件事; 识别开关于此处显式可见/可停 (取代旧的"取消即退出识别")。
    fn render_gamepad_status_bar(&mut self, ui: &mut egui::Ui, th: &Theme) {
        /* 提示自动淡出 */
        let now = std::time::Instant::now();
        if let Some(until) = self.gamepad_toast_until
            && now > until
        {
            self.gamepad_toast = None;
            self.gamepad_toast_until = None;
        }
        if let Some(until) = self.gamepad_warn_until
            && now > until
        {
            self.gamepad_warn = None;
            self.gamepad_warn_until = None;
        }

        /* ① 识别状态条: 一眼看到"现在识别的是哪台手柄", 并显式提供停止入口 */
        let identified = self.app_state.live_hid_pad().is_some();
        let device_name = self
            .live_pad()
            .map(|p| p.device_name)
            .or_else(|| self.raw_hid_status.clone());
        th.panel(ui, None, |ui| {
            ui.horizontal(|ui| {
                if identified {
                    let name = device_name.unwrap_or_else(|| "手柄".to_string());
                    /* ★v24.0: 已校对过 (存在系统备注映射) 的设备直接标绿, 方便识别 */
                    let calibrated = crate::gui::gamepad_mapping::SLOTS
                        .iter()
                        .any(|s| find_slot_mapping_index(&self.config, s).is_some());
                    let text = if calibrated {
                        format!("🎮 已校对 · 识别中: {name}")
                    } else {
                        format!("🎮 识别中: {name}")
                    };
                    ui.label(
                        egui::RichText::new(text)
                            .size(13.0)
                            .strong()
                            .color(th.good),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add(th.secondary_button("停止识别"))
                            .on_hover_text("停止读取手柄, 回到初始状态")
                            .clicked()
                        {
                            self.stop_identify();
                        }
                        if !matches!(self.gp_flow, GpFlow::Calibrate { .. })
                            && ui
                                .add(th.primary_button("一键校对手柄按键"))
                                .on_hover_text(
                                    "新手先点这个: 按提示把手柄上全部热点校对一遍 (约 20 秒), 自动记住键位; 中途不能跳过",
                                )
                                .clicked()
                        {
                            self.start_calibration();
                        }
                    });
                } else {
                    ui.label(
                        egui::RichText::new("🎮 未校对 / 未识别")
                            .size(13.0)
                            .strong()
                            .color(th.hint),
                    );
                    ui.label(th.hint_text("新手请先「一键校对手柄按键」; 老手可直接点图上的键"));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        /* 空状态已有一个大按钮; 非空状态才在状态条上补一个 */
                        if self.gp_flow != GpFlow::Idle
                            && ui
                                .add(th.secondary_button("手柄按键快速映射"))
                                .on_hover_text("读取你的手柄, 之后按手柄上的键即可快速设置映射")
                                .clicked()
                        {
                            self.start_identify_first_pad();
                        }
                        if !matches!(self.gp_flow, GpFlow::Calibrate { .. })
                            && ui
                                .add(th.primary_button("一键校对手柄按键"))
                                .on_hover_text(
                                    "新手先点这个: 按提示把手柄上全部热点校对一遍 (约 20 秒), 自动记住键位; 中途不能跳过",
                                )
                                .clicked()
                        {
                            self.start_calibration();
                        }
                    });
                }
            });
            /* 就近警告 (如"还没识别手柄"): 错误提示就放在出错的入口旁边 */
            if let Some(w) = self.gamepad_warn.clone() {
                ui.add_space(2.0);
                ui.label(
                    egui::RichText::new(format!("⚠ {w}"))
                        .size(12.0)
                        .color(th.warn),
                );
            }
            /* ★v24.8: 三步引导并入状态条 (删掉独立「怎么用」卡片 —— 与空状态卡内容重复且拉长页面)。
             * 用 right_to_left + 内嵌 left_to_right: 按钮先占右端, 文本在剩余宽度内换行, 不会互相挤压。 */
            if !self.gamepad_tip_dismissed {
                ui.add_space(theme::SP_XS);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                    if ui
                        .add(th.secondary_button("不再提示"))
                        .on_hover_text("之后不再显示这行引导")
                        .clicked()
                    {
                        self.gamepad_tip_dismissed = true;
                    }
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Min), |ui| {
                        ui.label(
                            egui::RichText::new(
                                "①「一键校对手柄按键」完整校对一次 → ②「手柄按键快速映射」设/改键位 → \
                                 ③ 有遗漏就按同一个手柄键重设",
                            )
                            .size(11.5)
                            .color(th.text_weak),
                        );
                    });
                });
            }
        });
        ui.add_space(theme::SP_XS);

        /* ② 成功提示 (纯成功反馈, 不再挂"退出识别"按钮) */
        if let Some(msg) = self.gamepad_toast.clone() {
            th.panel(ui, None, |ui| {
                ui.label(
                    egui::RichText::new(format!("✓ {msg}"))
                        .size(12.5)
                        .strong()
                        .color(th.good),
                );
            });
            ui.add_space(theme::SP_XS);
        }
        ui.add_space(theme::SP_S);
    }

    /// 已配置手柄映射 chips 总览 (点击可跳选对应槽位)。
    /// ★v21.7d: 只列"已设目标键"的槽位 (真正生效的映射), 避免一堆未设目标的空映射把卡片撑爆;
    /// 已绑定触发键但还没设目标键的数量用一行提示带过。
    fn render_gamepad_overview(&mut self, ui: &mut egui::Ui, th: &Theme) {
        let mut configured: Vec<&GamepadSlot> = Vec::new();
        let mut pending_target = 0usize;
        for s in SLOTS {
            if let Some(i) = find_slot_mapping_index(&self.config, s) {
                if self.config.mappings[i].target_keys.is_empty() {
                    pending_target += 1;
                } else {
                    configured.push(s);
                }
            }
        }

        let title = if configured.is_empty() {
            "我设好的映射".to_string()
        } else {
            format!("我设好的映射 ({})", configured.len())
        };
        let empty = configured.is_empty();

        th.card_with_actions(
            ui,
            Some(&title),
            |ui| {
                if pending_target > 0 {
                    ui.label(th.hint_text(format!(
                        "{pending_target} 个键还没设键盘键 (点手柄图上该键即可设置)"
                    )));
                } else if !configured.is_empty() {
                    ui.label(th.hint_text("点下面任一条可回去修改"));
                }
            },
            |ui| {
                if empty {
                    ui.label(th.hint_text(
                        "还没有可用的映射 — 点左边手柄图上的按键, 再按键盘上要代替的键, 两步即可",
                    ));
                    return;
                }
                /* ★v24.8: **手动按可见宽度分行** —— `horizontal_wrapped` 在父级宽度不受限时不会换行,
                 * 实机表现为 chips 一路向右溢出、被窗口裁掉。这里用 `clip_rect` 的有限宽度自行分行,
                 * 保证超出的 chip 一定落到下一行。 */
                let chips: Vec<(usize, String, String, bool)> = configured
                    .iter()
                    .map(|slot| {
                        let idx = find_slot_mapping_index(&self.config, slot).unwrap();
                        let m = &self.config.mappings[idx];
                        let full = format!("{} → {}", slot.label, m.target_keys.join("+"));
                        let label = theme::truncate_chars(&full, 22);
                        (slot.id, label, full, self.gp_selected_slot() == Some(slot.id))
                    })
                    .collect();

                let clip_w = ui.clip_rect().width();
                let avail_w = ui.available_width();
                let finite = |v: f32| v.is_finite() && v > 0.0;
                let wrap_w = {
                    let cap = if finite(clip_w) { clip_w - 56.0 } else { avail_w };
                    if finite(avail_w) { avail_w.min(cap) } else { cap }
                }
                .max(160.0);

                let font = egui::FontId::proportional(12.0);
                let mut rows: Vec<Vec<usize>> = Vec::new();
                let mut cur: Vec<usize> = Vec::new();
                let mut cur_w = 0.0_f32;
                for (i, c) in chips.iter().enumerate() {
                    /* chip 宽度 ≈ 文本宽 + 左右内边距 (9×2) + 行内间距 (6) */
                    let w = ui
                        .painter()
                        .layout_no_wrap(c.1.clone(), font.clone(), egui::Color32::WHITE)
                        .size()
                        .x
                        + 24.0;
                    if !cur.is_empty() && cur_w + w > wrap_w {
                        rows.push(std::mem::take(&mut cur));
                        cur_w = 0.0;
                    }
                    cur.push(i);
                    cur_w += w;
                }
                if !cur.is_empty() {
                    rows.push(cur);
                }

                for row in rows {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
                        for &i in &row {
                            let (slot_id, label, full, selected) = &chips[i];
                            let (fg, bg) = if *selected {
                                (egui::Color32::WHITE, th.accent)
                            } else {
                                (th.target_fg, th.target_bg)
                            };
                            let resp =
                                th.badge_clickable(ui, label, fg, bg).on_hover_text(full.clone());
                            if resp.clicked() && !self.gp_flow.is_busy() {
                                /* 捕获/确认中不响应跳转, 保证流程不被带偏 */
                                self.gp_flow = GpFlow::Selected { slot: *slot_id };
                            }
                        }
                    });
                }
            },
        );
    }

    /// 绘制手柄图与热点, 返回被点击的槽位 id。
    fn render_gamepad_svg(
        &mut self,
        ui: &mut egui::Ui,
        desired_size: egui::Vec2,
        th: &Theme,
    ) -> Option<usize> {
        let (rect, _) = ui.allocate_exact_size(desired_size, egui::Sense::hover());
        let painter = ui.painter();

        if let Some(texture) = &self.gamepad_texture {
            painter.image(
                texture.id(),
                rect,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
        } else {
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "手柄图片加载失败",
                egui::FontId::proportional(14.0),
                th.hint,
            );
        }

        let mut clicked = None;
        let selected_id = self.gp_selected_slot();
        let live_state = self.app_state.live_hid_state();
        /* ★v22.0: 捕获高亮来自状态机 (AwaitKb/AwaitPad) */
        let capture_slot = match &self.gp_flow {
            GpFlow::AwaitKb { slot, .. } | GpFlow::AwaitPad { slot } => Some(*slot),
            _ => None,
        };
        // 捕获等待态: 各槽位独立雷达动画 (见下方 is_capturing 分支)

        for slot in SLOTS {
            let (center, radius) = hotspot_rect(rect, slot);
            let hit_rect = egui::Rect::from_center_size(
                center,
                egui::vec2(radius * 2.0, radius * 2.0),
            );
            let id = ui.id().with("gamepad_hotspot").with(slot.id);
            let response = ui.interact(hit_rect, id, egui::Sense::click());
            if response.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }

            let is_selected = selected_id == Some(slot.id);
            let is_capturing = matches!(capture_slot, Some(cid) if cid == slot.id);
            let (base_r, base_g, base_b) = slot.kind.base_rgb();
            let slot_col = |a: u8| egui::Color32::from_rgba_unmultiplied(base_r, base_g, base_b, a);
            let configured = find_slot_mapping_index(&self.config, slot).is_some();
            let is_click = matches!(slot.kind, SlotKind::StickClick);

            // 悬停/选中/捕获的放大动画 (150ms 缓动)
            let hot = is_capturing || is_selected || response.hovered();
            let scale = 1.0 + 0.10 * ui.ctx().animate_bool_with_time(
                ui.id().with("gamepad_hotspot_anim").with(slot.id),
                hot,
                0.15,
            );
            let r_eff = radius * scale;

            /* ★v20.2: 摇杆按下 (StickClick) 热点恢复显示 —— 与其余槽位同款
             * 玻璃底+主环+中心点 (09-08 曾改隐形热点, 用户要求加回) */
            if is_capturing {
                /* 捕获态: 雷达扩散环 + 琥珀主环 */
                let phase = (ui.ctx().input(|i| i.time) * 1.6) % 1.0;
                painter.circle_stroke(
                    center,
                    r_eff * (1.05 + 0.40 * phase as f32),
                    egui::Stroke::new(
                        2.0,
                        egui::Color32::from_rgba_unmultiplied(
                            255,
                            190,
                            60,
                            (190.0 * (1.0 - phase as f32)) as u8,
                        ),
                    ),
                );
                painter.circle_filled(center, r_eff, egui::Color32::from_rgba_unmultiplied(255, 190, 60, 60));
                painter.circle_stroke(center, r_eff, egui::Stroke::new(2.2, egui::Color32::from_rgb(255, 200, 80)));
            } else {
                /* ★v21.7b 实体手柄实时按下: 亮色填充 + 白环 (最醒目, 一眼看出按了哪个键) */
                let live = live_state
                    .as_ref()
                    .is_some_and(|l| self.gp_slot_is_live(slot.id, l));
                if live {
                    painter.circle_filled(
                        center,
                        r_eff * 1.45,
                        slot_col(if th.dark { 120 } else { 90 }),
                    );
                    painter.circle_filled(center, r_eff, slot_col(245));
                    painter.circle_stroke(
                        center,
                        r_eff,
                        egui::Stroke::new(2.6, egui::Color32::WHITE),
                    );
                    painter.circle_filled(center, r_eff * 0.15, egui::Color32::WHITE);
                } else {
                /* 悬停/选中: 外柔光 */
                if hot {
                    painter.circle_filled(center, r_eff * 1.5, slot_col(if is_selected { 55 } else { 40 }));
                }
                /* 玻璃底 (深机身增透, 亮机身轻压暗) */
                painter.circle_filled(
                    center,
                    r_eff,
                    if th.dark {
                        egui::Color32::from_black_alpha(64)
                    } else {
                        egui::Color32::from_black_alpha(30)
                    },
                );
                /* 主环: 选中=强调色 / 已配置=类色实线 / 空槽=类色细线 */
                painter.circle_stroke(
                    center,
                    r_eff,
                    if is_selected {
                        egui::Stroke::new(2.6, th.accent)
                    } else if configured {
                        egui::Stroke::new(2.0, slot_col(235))
                    } else {
                        egui::Stroke::new(1.4, slot_col(125))
                    },
                );
                /* 中心点 (单字符槽位与摇杆按下; Back/Start 保留文字空间) */
                if slot.short.chars().count() == 1 || is_click {
                    painter.circle_filled(
                        center,
                        r_eff * 0.15,
                        if is_selected {
                            th.accent
                        } else {
                            slot_col(if configured { 255 } else { 140 })
                        },
                    );
                }
                }
            }

            /* 短标签 (方向箭头/肩键文字) 带微投影; ABXY 面键与 Back/Start 系统键
             * 圈内不绘字 (2026-09-08 用户要求留白); 摇杆按下 short 为空天然不绘 */
            if !slot.short.is_empty()
                && !is_click
                && !matches!(slot.kind, SlotKind::Button | SlotKind::System)
            {
                let font_size = match slot.short.chars().count() {
                    1 => 13.0,
                    2 => 9.5,
                    _ => 8.5,
                };
                let tcol = if is_capturing
                    || is_selected
                    || response.hovered()
                    || live_state
                        .as_ref()
                        .is_some_and(|l| self.gp_slot_is_live(slot.id, l))
                {
                    egui::Color32::WHITE
                } else if configured {
                    slot_col(255)
                } else if th.dark {
                    egui::Color32::from_rgba_unmultiplied(255, 255, 255, 190)
                } else {
                    egui::Color32::from_rgba_unmultiplied(58, 65, 82, 235)
                };
                painter.text(
                    center + egui::vec2(0.0, 1.0),
                    egui::Align2::CENTER_CENTER,
                    slot.short,
                    egui::FontId::proportional(font_size),
                    egui::Color32::from_black_alpha(if th.dark { 130 } else { 60 }),
                );
                painter.text(
                    center,
                    egui::Align2::CENTER_CENTER,
                    slot.short,
                    egui::FontId::proportional(font_size),
                    tcol,
                );
            }

            response.clone().on_hover_text(format!(
                "{} — 点它开始配置 (或直接按手柄上这个键)",
                slot.label
            ));

            if response.clicked() {
                clicked = Some(slot.id);
            }
        }

        clicked
    }

    /// 右栏快捷卡 (空槽态显示): 连发开关 / 映射选择 / 震动开关 / 职业选择。
    /// 控件语义与极简模式一致 (toggle_with_notify / 选中即应用), egui id 用
    /// "gp_" 前缀避免与极简模式冲突; 震动段仅 DFO 玩家可见。
    fn render_quick_connect_card(&mut self, ui: &mut egui::Ui, th: &Theme) {
        use std::sync::atomic::Ordering;
        th.panel(ui, Some("快速连接"), |ui| {
            /* 连发大开关 */
            let running = !self.app_state.is_paused();
            let (ltext, lfg, lbg) = if running {
                ("连发 · 开启中", th.good, th.good_soft)
            } else {
                ("连发 · 已暂停", th.hint, th.faint)
            };
            let turbo_btn = egui::Button::new(
                egui::RichText::new(ltext).size(14.0).strong().color(lfg),
            )
            .fill(lbg)
            .corner_radius(egui::CornerRadius::same(10));
            if ui
                .add_sized([ui.available_width(), 32.0], turbo_btn)
                .clicked()
            {
                self.toggle_with_notify();
            }
            ui.add_space(theme::SP_XS);

            /* 映射选择 (连发预设, 与极简/顶栏同一套切换逻辑) */
            if self.config.presets.is_empty() {
                ui.label(th.hint_text("映射预设: 无 — 到「连发映射修改」页保存一个"));
            } else {
                ui.horizontal(|ui| {
                    ui.label(th.weak("映射选择"));
                    self.render_preset_switch(ui);
                });
            }
            ui.add_space(theme::SP_S);

            /* 震动段 (仅 DFO 玩家) */
            if self.config.dfo_player {
                let vib_on = self
                    .app_state
                    .vibration_enabled
                    .load(Ordering::Relaxed);
                let (vtext, vfg, vbg) = if vib_on {
                    ("震动 · 开启", th.good, th.good_soft)
                } else {
                    ("震动 · 关闭", th.hint, th.faint)
                };
                let vib_btn = egui::Button::new(
                    egui::RichText::new(vtext).size(14.0).strong().color(vfg),
                )
                .fill(vbg)
                .corner_radius(egui::CornerRadius::same(10));
                if ui
                    .add_sized([ui.available_width(), 32.0], vib_btn)
                    .clicked()
                {
                    let v = self
                        .app_state
                        .vibration_enabled
                        .load(Ordering::Relaxed);
                    self.app_state
                        .vibration_enabled
                        .store(!v, Ordering::Relaxed);
                }
                ui.add_space(theme::SP_XS);

                /* 通用 / 全职业 分段开关 (与极简模式共享 minimal_vib_preset_job 状态) */
                let mode_job = self.minimal_vib_preset_job;
                let seg_w = ui.available_width();
                let seg_h = 26.0_f32;
                let (seg_rect, _) =
                    ui.allocate_exact_size(egui::vec2(seg_w, seg_h), egui::Sense::hover());
                let half = seg_w / 2.0;
                let left_rect = egui::Rect::from_x_y_ranges(
                    seg_rect.left()..=seg_rect.center().x,
                    seg_rect.top()..=seg_rect.bottom(),
                );
                let right_rect = egui::Rect::from_x_y_ranges(
                    seg_rect.center().x..=seg_rect.right(),
                    seg_rect.top()..=seg_rect.bottom(),
                );
                let l_resp = ui
                    .interact(left_rect, egui::Id::new("gp_vib_preset_mode").with("l"), egui::Sense::click())
                    .on_hover_cursor(egui::CursorIcon::PointingHand);
                let r_resp = ui
                    .interact(right_rect, egui::Id::new("gp_vib_preset_mode").with("r"), egui::Sense::click())
                    .on_hover_cursor(egui::CursorIcon::PointingHand);
                ui.painter().rect_filled(seg_rect, 8, th.faint);
                let t = ui.ctx().animate_value_with_time(
                    egui::Id::new("gp_vib_preset_mode"),
                    if mode_job { 1.0 } else { 0.0 },
                    0.15,
                );
                let thumb_w = half - 6.0;
                let thumb_x = seg_rect.left() + 3.0 + t * (half - 6.0);
                let thumb = egui::Rect::from_min_size(
                    egui::pos2(thumb_x, seg_rect.top() + 3.0),
                    egui::vec2(thumb_w, seg_h - 6.0),
                );
                ui.painter().rect_filled(thumb, 6, th.accent);
                let unselected = |a: f32| -> egui::Color32 {
                    let c = th.text_weak;
                    egui::Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), a as u8)
                };
                let selected_white = |a: f32| -> egui::Color32 {
                    egui::Color32::from_rgba_unmultiplied(255, 255, 255, a as u8)
                };
                ui.painter().text(
                    left_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "通用预设",
                    egui::FontId::proportional(12.0),
                    if mode_job { unselected(200.0) } else { selected_white(255.0) },
                );
                ui.painter().text(
                    right_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "全职业预设",
                    egui::FontId::proportional(12.0),
                    if mode_job { selected_white(255.0) } else { unselected(200.0) },
                );
                if l_resp.clicked() {
                    self.minimal_vib_preset_job = false;
                    /* ★保险B (2026-09-09, 用户定稿): 切回通用预设段 = 放弃全职业
                     * 预设 —— 自动停用 (关开关+回滚参数+JobVibration.toml applied=false),
                     * 防止职业参数在通用模式下继续静默生效 */
                    if self.vib_job_enabled {
                        self.disable_job_vibration_preset();
                    }
                    let _ = self.config.save_to_file("Config.toml");
                }
                if r_resp.clicked() {
                    self.minimal_vib_preset_job = true;
                    let jobs = crate::job_presets::available_jobs(self.config.vib_legacy_client);
                    let base_sel = self.vib_job_base.min(jobs.len().saturating_sub(1));
                    let class_sel =
                        self.vib_job_class.min(jobs[base_sel].classes.len().saturating_sub(1));
                    self.apply_job_vibration_preset(
                        jobs[base_sel].base_job,
                        &jobs[base_sel].classes[class_sel],
                    );
                    self.vib_job_loaded = Some((base_sel, class_sel));
                    let _ = self.config.save_to_file("Config.toml");
                }
                ui.add_space(theme::SP_XS);

                /* 选择行 (选中即应用); ★v19.3 与上方分段控件拉开间距 (用户: 贴太紧不美观) */
                if !mode_job {
                    ui.add_space(9.0);
                    ui.horizontal(|ui| {
                        ui.label(th.weak("通用预设"));
                        self.ensure_act1_preset_listed();
                        /* ★v19: 用真实下标, 修过滤下标错位 */
                        let entries = crate::config::visible_preset_entries(
                            &self.config.vibration_presets,
                            self.config.vib_legacy_client,
                        );
                        let sel_pos = entries
                            .iter()
                            .position(|(real, _)| *real == self.vib_preset_idx)
                            .unwrap_or(0);
                        if let Some((real, _)) = entries.get(sel_pos) {
                            self.vib_preset_idx = *real;
                        }
                        let selected = entries.get(sel_pos).map(|(_, n)| n.clone()).unwrap_or_default();
                        egui::ComboBox::from_id_salt("gp_vib_general")
                            .selected_text(selected)
                            .width(150.0)
                            .show_ui(ui, |ui| {
                                for (pos, (real, n)) in entries.iter().enumerate() {
                                    if ui.selectable_label(pos == sel_pos, n).clicked() {
                                        self.vib_preset_idx = *real;
                                        self.apply_general_vibration_preset(n);
                                    }
                                }
                            });
                    });
                } else {
                    let jobs = crate::job_presets::available_jobs(self.config.vib_legacy_client);
                    let base_sel = self.vib_job_base.min(jobs.len().saturating_sub(1));
                    let classes = &jobs[base_sel].classes;
                    let class_sel = self.vib_job_class.min(classes.len().saturating_sub(1));
                    /* ★v19.3 与上方分段控件拉开间距 (用户: 贴太紧不美观);
                     * 转职槽仍按原微调上调 1.5px (定块 + 绝对定位子 Ui) */
                    ui.add_space(9.0);
                    ui.horizontal(|ui| {
                        ui.label(th.weak("职业选择"));
                        /* ★v19.6 S1 专属 (用户定稿): "-ACT" 职业名比槽位宽 → 转职槽
                         * 自然排在职业框真实宽度之后, 再右移 30px, 彻底消除左右重叠。
                         * 改为自然流式布局 (不再绝对定位), S4 分支不受影响。 */
                        egui::ComboBox::from_id_salt("gp_vib_job_base")
                            .selected_text(jobs[base_sel].base_job)
                            .width(124.0)
                            .show_ui(ui, |ui| {
                                for (i, j) in jobs.iter().enumerate() {
                                    if ui.selectable_label(i == base_sel, j.base_job).clicked() {
                                        self.vib_job_base = i;
                                        self.vib_job_class = 0;
                                    }
                                }
                            });
                        /* ★v19.7 用户定稿: 转职槽再左移 25px (30 → 5) 并上移 3px
                         * (自然流式预留空间 + 抬高 3px 的矩形内绘制, 行高不变) */
                        ui.add_space(5.0);
                        let combo_h = ui.spacing().interact_size.y;
                        let (class_slot, _) = ui.allocate_exact_size(
                            egui::vec2(96.0, combo_h),
                            egui::Sense::hover(),
                        );
                        ui.allocate_new_ui(
                            egui::UiBuilder::new().max_rect(egui::Rect::from_min_size(
                                egui::pos2(class_slot.min.x, class_slot.min.y - 3.0),
                                egui::vec2(96.0, combo_h),
                            )),
                            |ui| {
                                egui::ComboBox::from_id_salt("gp_vib_job_class")
                                    .selected_text(classes[class_sel].name.clone())
                                    .width(96.0)
                                    .show_ui(ui, |ui| {
                                        for (i, c) in classes.iter().enumerate() {
                                            if ui.selectable_label(i == class_sel, c.name).clicked() {
                                                self.vib_job_class = i;
                                                self.apply_job_vibration_preset(
                                                    jobs[base_sel].base_job,
                                                    c,
                                                );
                                            }
                                        }
                                    });
                            },
                        );
                    });
                }

                /* ★震动模块状态行: 按版本分流 (ACT1=自动注入 / S4+=模块已启用)
                 * ★v19.5 S1 专属: 职业选择行与状态行之间多留一点间距 (S4 不变) */
                ui.add_space(if mode_job { 10.0 } else { theme::SP_XS });
                if self.config.vib_legacy_client {
                    let ready = crate::auto_inject::host_dir_dll();
                    let (itext, ifg, ibg) = if ready {
                        ("ACT1 自动注入 · 已启用 (DLL: 主程序目录)", th.good, th.good_soft)
                    } else {
                        ("ACT1 自动注入 · 已启用 (DLL: 游戏目录)", th.good, th.good_soft)
                    };
                    let inj_btn = egui::Button::new(
                        egui::RichText::new(itext).size(12.5).strong().color(ifg),
                    )
                    .fill(ibg)
                    .corner_radius(egui::CornerRadius::same(10));
                    ui.add_sized([ui.available_width(), 30.0], inj_btn);
                    ui.label(th.hint_text(
                        "把 DfoVibration_OLD.dll 放入游戏目录后开游戏即自动注入; 未检测到 DLL 时不注入",
                    ));
                } else {
                    let s4_btn = egui::Button::new(
                        egui::RichText::new("S4 震动模块自动注入 · 已启用")
                            .size(12.5)
                            .strong()
                            .color(th.good),
                    )
                    .fill(th.good_soft)
                    .corner_radius(egui::CornerRadius::same(10));
                    ui.add_sized([ui.available_width(), 30.0], s4_btn);
                }
            }
        });
    }

    /// ★v22.0: 手柄页输入处理 (每帧调用, 由 `main_window` 在设置弹窗关闭时驱动)。
    ///
    /// 只做三件事: ① 识别态手柄实时按键边沿 → 选中/改选槽位; ② `AwaitKb` 键盘捕获;
    /// ③ `AwaitPad` 的原始报文兜底捕获。**绝不**在此改动识别开关 ——
    /// 修复旧版"识别中途按错一个键就把识别关掉"的结构性问题。
    pub(super) fn handle_gamepad_flow(&mut self, ctx: &egui::Context) {
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
    fn poll_pad_capture(&mut self) -> Option<PadCaptureCandidates> {
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
    fn gp_calibration_step(&mut self, slot_id: usize) {
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
    fn gp_advance_calibration(&mut self) {
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
    fn gp_record_calibration(
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

    /// ★v22.6: 开始「一键校对手柄按键」向导 (覆盖全部热点圈)。
    fn start_calibration(&mut self) {
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
    fn end_calibration(&mut self, msg: &str) {
        self.app_state.set_raw_input_capture_mode(false);
        self.app_state.set_gp_calibrating(false);
        self.gp_pad_capture.reset();
        self.gp_prev_xinput = None;
        self.set_gamepad_toast(msg);
    }

    /// 保存 + 热重载 (校准过程中每个键都落盘, 中途退出不丢已校准的部分)。
    fn gp_save_and_reload(&mut self) {
        let _ = self.config.save_to_file("Config.toml");
        if let Err(e) = self.app_state.reload_config(self.config.clone()) {
            eprintln!("Failed to reload config after gamepad calibration: {e}");
        }
    }

    /// 当前"应该高亮"的槽位: 选中槽位, 或校准向导的当前目标键。
    fn gp_selected_slot(&self) -> Option<usize> {
        self.gp_flow.slot().or_else(|| match self.gp_flow {
            GpFlow::Calibrate { step, .. } => calibration_slot(step),
            _ => None,
        })
    }

    /// 右侧分步面板 —— 按 [`GpFlow`] 分派, 每一步只呈现一件事 ("一步一步来")。
    fn render_gamepad_slot_panel(&mut self, ui: &mut egui::Ui, th: &Theme) {
        match self.gp_flow.clone() {
            GpFlow::Idle => {
                let identified = self.app_state.live_hid_pad().is_some();
                match render_slot_empty_state(ui, th, identified) {
                    EmptyAction::Map => self.start_identify_first_pad(),
                    EmptyAction::Calibrate => self.start_calibration(),
                    EmptyAction::None => {}
                }
                /* 右栏空位宽裕: 快捷卡 (连发/映射/震动/职业) 让玩家开箱即连 */
                self.render_quick_connect_card(ui, th);
            }
            GpFlow::Selected { slot } => self.render_slot_selected(ui, th, slot),
            GpFlow::AwaitKb { slot, keys } => self.render_slot_await_kb(ui, th, slot, keys),
            GpFlow::AwaitPad { slot } => self.render_slot_await_pad(ui, th, slot),
            GpFlow::ConfirmCapture { slot, kind, value } => {
                self.render_slot_confirm(ui, th, slot, kind, value)
            }
            GpFlow::ConfirmDelete { slot } => self.render_slot_confirm_delete(ui, th, slot),
            GpFlow::MapDone { slot } => self.render_slot_map_done(ui, th, slot),
            GpFlow::Calibrate { step, total } => self.render_slot_calibrate(ui, th, step, total),
        }
    }

    /// ★v24.4: 快速映射完成页 —— 三/四步走完后的落点。
    ///
    /// 提示用户可以继续按其他手柄键 (监听态已在 [`Self::gp_confirm_capture`] 里重新打开),
    /// 或用【编辑本映射】回到本键的第 1 步继续调整。
    fn render_slot_map_done(&mut self, ui: &mut egui::Ui, th: &Theme, slot_id: usize) {
        let label = get_slot(slot_id).map(|s| s.label).unwrap_or("?");
        let mut edit_again = false;
        th.panel(ui, None, |ui| {
            ui.add_space(8.0);
            th.badge(ui, "已完成", egui::Color32::WHITE, th.good);
            ui.add_space(8.0);
            ui.label(
                egui::RichText::new(format!("「{label}」已更新"))
                    .size(16.0)
                    .strong()
                    .color(th.good),
            );
            ui.add_space(6.0);
            ui.label(th.hint_text(
                "本次按键已完成修改。您可以继续按下其他手柄按键，为其编辑新的映射键位；\
                 也可以留在当前页面，继续调整此按键的设置。",
            ));
            ui.add_space(10.0);
            if ui.add(th.primary_button("编辑本映射")).clicked() {
                edit_again = true;
            }
            ui.add_space(8.0);
        });
        if edit_again {
            /* 回到本键第 1 步 (Selected 面板: 看现状 / 设键盘键 / 校键) */
            self.gp_quick_listen = false;
            self.app_state.set_raw_input_capture_mode(false);
            self.select_slot(slot_id);
        }
    }

    /// ★v22.4 · 一键校准向导: 按提示逐个按键, 自动记录 (连发同款识别)。
    /// ★v22.5: **必须把全部键按一遍才能完成, 不提供跳过** —— 保证全覆盖、不漏键。
    fn render_slot_calibrate(
        &mut self,
        ui: &mut egui::Ui,
        th: &Theme,
        step: usize,
        total: usize,
    ) {
        let current_slot = calibration_slot(step);
        let current_label = current_slot
            .and_then(get_slot)
            .map(|s| s.label)
            .unwrap_or("?");
        th.panel(ui, None, |ui| {
            ui.horizontal(|ui| {
                th.badge(ui, "一键校对手柄按键", egui::Color32::WHITE, th.accent);
                ui.add_space(6.0);
                ui.label(
                    egui::RichText::new(format!("{}/{}", step + 1, total))
                        .size(15.0)
                        .strong()
                        .color(th.title),
                );
            });
            ui.add_space(10.0);
            ui.label(
                egui::RichText::new(format!("请操作「{current_label}」然后松开"))
                    .size(17.0)
                    .strong()
                    .color(th.accent),
            );
            ui.add_space(6.0);
            /* ★v24.2: 按槽位类型给出更明确的提示 —— 摇杆"按下"与"推方向"要分开, 乱动就是串键的来源 */
            let tip = match current_slot.and_then(get_slot).map(|s| s.kind) {
                Some(SlotKind::StickClick) => {
                    "摇杆按下: 垂直按下去再松开, 不要推动摇杆（推动了会被判为不干净, 让你重按）"
                }
                Some(SlotKind::Stick) => {
                    "摇杆方向: 推到底并保持一下再松开, 一次只推一个方向（斜着推会让你重按）"
                }
                _ => "按键/扳机/十字键: 按一下再松开（别同时碰摇杆）",
            };
            ui.label(th.hint_text(format!(
                "{tip}\n必须把全部热点都操作一遍才能完成。"
            )));
            ui.add_space(8.0);
            /* ★v22.7: 完成进度 —— 已完成项**变绿**、当前项强调色、未完成灰; 一眼识别还差哪些。
             * 用 LayoutJob 实现"单段可换行 + 多颜色" (徽章/多 label 会被面板右缘裁掉)。 */
            let font = egui::FontId::proportional(12.0);
            let mut job = egui::text::LayoutJob::default();
            job.wrap.max_width = ui.available_width();
            for (i, id) in CALIBRATION_ORDER.iter().enumerate() {
                let label = calibration_short_label(*id);
                let (text, color) = if i < step {
                    /* ★v24.2: 已完成项直接显示"记下的触发键简称" —— 串键 / 串进 045E 命名一眼可见,
                     * 不用再跑去连发映射区逐条核对。 */
                    let rec = get_slot(*id)
                        .map(|s| slot_trigger_display(&self.config, s))
                        .filter(|s| !s.is_empty())
                        .map(|s| {
                            let short = short_trigger_name(&s);
                            if short.chars().count() > 22 {
                                format!("={}…", short.chars().take(22).collect::<String>())
                            } else {
                                format!("={short}")
                            }
                        })
                        .unwrap_or_default();
                    (format!("✓{label}{rec}"), th.good)
                } else if i == step {
                    (format!("▶{label}"), th.accent)
                } else {
                    (label.to_string(), th.hint)
                };
                job.append(
                    &text,
                    0.0,
                    egui::TextFormat {
                        font_id: font.clone(),
                        color,
                        ..Default::default()
                    },
                );
                job.append(
                    "   ",
                    0.0,
                    egui::TextFormat {
                        font_id: font.clone(),
                        color: th.hint,
                        ..Default::default()
                    },
                );
            }
            ui.label(job);
            ui.add_space(12.0);
            if ui.add(th.secondary_button("取消校准")).clicked() {
                self.gp_flow = self.gp_flow.transition(GpEvent::Cancel);
                self.end_calibration("已取消校准 (已按过的键仍然保留)");
            }
        });
    }

    /// 第 1 步 · 已选中一个手柄键: 看现状, 给出唯一的下一步。
    fn render_slot_selected(&mut self, ui: &mut egui::Ui, th: &Theme, slot_id: usize) {
        let Some(slot) = get_slot(slot_id) else { return };
        let trigger = slot_trigger_display(&self.config, slot);
        let targets = slot_targets(&self.config, slot);
        let mapped = find_slot_mapping_index(&self.config, slot).is_some();
        let uncalibrated = !mapped;

        th.panel(ui, None, |ui| {
            step_header(ui, th, 1, &slot.label);
            ui.add_space(4.0);
            let (r, g, b) = slot.kind.base_rgb();
            th.badge(
                ui,
                slot.kind.legend(),
                egui::Color32::WHITE,
                egui::Color32::from_rgb(r, g, b),
            );
            ui.add_space(8.0);

            /* 一句话结论 (新手最先看这行) */
            let summary = if targets.is_empty() {
                egui::RichText::new(format!("{} 键  →  还没设键盘键", slot.label))
                    .size(15.0)
                    .strong()
                    .color(th.warn)
            } else {
                egui::RichText::new(format!(
                    "{} 键  →  键盘 {}",
                    slot.label,
                    targets.join(" + ")
                ))
                .size(15.0)
                .strong()
                .color(th.good)
            };
            ui.label(summary);
            ui.add_space(8.0);

            ui.horizontal(|ui| {
                ui.label(th.weak("手柄按键"));
                ui.add_space(4.0);
                if uncalibrated {
                    th.badge(ui, "未校对", th.warn, th.faint)
                        .on_hover_text("请先点右上「一键校对手柄按键」把手柄键位校一遍");
                } else {
                    th.trigger_badge(ui, &trigger);
                }
            });
            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                ui.label(th.weak("键盘按键"));
                ui.add_space(4.0);
                if targets.is_empty() {
                    ui.label(th.hint_text("还没设 — 点下面的按钮后按键盘"));
                } else {
                    for t in &targets {
                        th.target_badge(ui, t);
                    }
                }
            });

            ui.add_space(14.0);
            ui.separator();
            ui.add_space(12.0);

            /* 第 2 步入场: 只有一个主按钮, 不并列多个入口 (未校对 → 置灰) */
            let main_label = if targets.is_empty() {
                "第 ② 步: 按键盘键 (设置要代替的键)"
            } else {
                "修改键盘键"
            };
            let main_hint = if uncalibrated {
                "这个键还没校对 — 请先点右上「一键校对手柄按键」"
            } else {
                "按一下键盘上要代替的键 → 再点「确认」; 想设组合键可在确认页点「＋ 再加一个键」"
            };
            if ui
                .add_enabled(
                    !uncalibrated,
                    th.primary_button(main_label)
                        .min_size(egui::vec2(280.0, 36.0)),
                )
                .on_hover_text(main_hint)
                .clicked()
            {
                self.begin_set_kb(slot_id);
            }
            ui.add_space(8.0);
            ui.horizontal_wrapped(|ui| {
                if ui
                    .add(th.secondary_button("校对手柄键位"))
                    .on_hover_text("按一个手柄物理键, 指定这个槽位是哪个键 (只取一个键)")
                    .clicked()
                {
                    self.begin_set_pad(slot_id);
                }
                if ui
                    .add(th.secondary_button("鼠标映射…"))
                    .on_hover_text("把手柄这个键映射成鼠标: 滚动 或 点击")
                    .clicked()
                {
                    self.gamepad_mouse_menu = true;
                }
            });

            /* 更多设置 (连发 / 奔跑) —— 就地展开, 不再跳去连发页 */
            if mapped {
                ui.add_space(10.0);
                self.render_slot_advanced(ui, th, slot);
            }

            ui.add_space(12.0);
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(!targets.is_empty(), th.secondary_button("清空键盘按键"))
                    .clicked()
                {
                    clear_slot_targets(&mut self.config, slot);
                    let _ = self.config.save_to_file("Config.toml");
                    if let Err(e) = self.app_state.reload_config(self.config.clone()) {
                        eprintln!("Failed to reload config after clear targets: {}", e);
                    }
                }
                if ui
                    .add_enabled(mapped, th.danger_button("删除该映射"))
                    .on_hover_text("删除需要二次确认")
                    .clicked()
                {
                    self.gp_flow = self.gp_flow.transition(GpEvent::RequestDelete);
                }
            });
        });
    }

    /// 第 2 步 · 等键盘按键 (可累加组合)。
    fn render_slot_await_kb(
        &mut self,
        ui: &mut egui::Ui,
        th: &Theme,
        slot_id: usize,
        keys: Vec<String>,
    ) {
        let Some(slot) = get_slot(slot_id) else { return };
        th.panel(ui, None, |ui| {
            step_header(
                ui,
                th,
                2,
                &format!("{}: 按键盘上你想让它代替的那个键", slot.label),
            );
            ui.add_space(8.0);
            if !keys.is_empty() {
                ui.horizontal_wrapped(|ui| {
                    ui.label(th.weak("已捕获"));
                    for k in &keys {
                        th.target_badge(ui, k);
                    }
                });
                ui.add_space(6.0);
            }
            ui.label(th.hint_text(
                "按下后先出现「确认」按钮, 确认后才写入。想设组合键: 在确认页点「＋ 再加一个键」。\n\
                 本步只认键盘键 —— 想换手柄键请先点「取消」回到上一步。",
            ));
            ui.add_space(10.0);
            if ui.add(th.secondary_button("✕ 取消")).clicked() {
                self.cancel_gp();
            }
        });
    }

    /// 第 2 步 · 等手柄物理键 (校对该槽位是哪个键)。
    fn render_slot_await_pad(&mut self, ui: &mut egui::Ui, th: &Theme, slot_id: usize) {
        let Some(slot) = get_slot(slot_id) else { return };
        let identified = self.app_state.live_hid_pad().is_some();
        th.panel(ui, None, |ui| {
            step_header(ui, th, 2, &format!("{}: 请按手柄上的一个键", slot.label));
            ui.add_space(8.0);
            ui.label(th.hint_text(
                "按一下手柄上的这个键**再松开** (扳机也照扣一次), 然后点「确认」。\n\
                 程序会记住这个槽位对应你手柄上的哪个键 (与连发映射同一套识别)。",
            ));
            if !identified {
                ui.add_space(4.0);
                ui.label(th.hint_text(
                    "按了没反应? 第三方手柄请先在设备管理里激活该手柄。",
                ));
            }
            ui.add_space(10.0);
            if ui.add(th.secondary_button("✕ 取消")).clicked() {
                self.cancel_gp();
            }
        });
    }

    /// 第 3 步 · 已捕获, 等确认。
    fn render_slot_confirm(
        &mut self,
        ui: &mut egui::Ui,
        th: &Theme,
        slot_id: usize,
        kind: GpCaptureKind,
        value: String,
    ) {
        if get_slot(slot_id).is_none() {
            return;
        }
        th.panel(ui, None, |ui| {
            step_header(ui, th, 3, "确认");
            ui.add_space(8.0);
            ui.label(
                egui::RichText::new(format!("已捕获: {value}"))
                    .size(16.0)
                    .strong()
                    .color(th.accent),
            );
            ui.add_space(4.0);
            ui.label(th.hint_text(match kind {
                GpCaptureKind::Trigger => "点「确认」把该手柄键记为这个槽位",
                GpCaptureKind::Target => "点「确认」把它记成这个键位的键盘映射",
            }));
            ui.add_space(10.0);
            ui.horizontal_wrapped(|ui| {
                if ui
                    .add(th.primary_button("✓ 确认").min_size(egui::vec2(120.0, 34.0)))
                    .clicked()
                {
                    self.gp_confirm_capture();
                }
                if kind == GpCaptureKind::Target
                    && ui
                        .add(
                            th.secondary_button("＋ 再加一个键")
                                .min_size(egui::vec2(130.0, 34.0)),
                        )
                        .on_hover_text("继续按键盘, 组成组合键 (如 CTRL+F6)")
                        .clicked()
                {
                    self.gp_add_another_key();
                }
                if ui
                    .add(th.secondary_button("✕ 取消").min_size(egui::vec2(90.0, 34.0)))
                    .clicked()
                {
                    self.cancel_gp();
                }
            });
        });
    }

    /// 删除二次确认。
    fn render_slot_confirm_delete(&mut self, ui: &mut egui::Ui, th: &Theme, slot_id: usize) {
        let Some(slot) = get_slot(slot_id) else { return };
        th.panel(ui, None, |ui| {
            ui.label(
                egui::RichText::new(format!("确定删除「{}」的映射?", slot.label))
                    .size(15.0)
                    .strong()
                    .color(th.warn),
            );
            ui.add_space(4.0);
            ui.label(th.hint_text("删除后需要重新设置才能恢复。"));
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                if ui.add(th.danger_button("删除")).clicked() {
                    self.gp_confirm_delete();
                }
                if ui.add(th.secondary_button("取消")).clicked() {
                    self.cancel_gp();
                }
            });
        });
    }

    /// ★v24.0: 该槽位此刻是否被按下 —— 直接看**它自己那条映射的触发键**与当前原始位组合是否相等。
    /// (与连发映射同一套判定; 未校对的槽位不亮。)
    fn gp_slot_is_live(&self, slot_id: usize, live: &crate::state::LiveHidState) -> bool {
        let Some(slot) = get_slot(slot_id) else {
            return false;
        };
        match find_slot_mapping_index(&self.config, slot) {
            Some(i) => trigger_matches_raw(
                &self.config.mappings[i].trigger_key,
                live.vid,
                live.pid,
                live.raw_position,
            ),
            None => false,
        }
    }

    /// ★v24.0: 这一帧新按下的槽位 (识别态自动选中用)。
    fn gp_newly_pressed_slot(
        &self,
        prev: &crate::state::LiveHidState,
        now: &crate::state::LiveHidState,
    ) -> Option<usize> {
        if now.raw_position == 0 || now.raw_position == prev.raw_position {
            return None;
        }
        SLOTS
            .iter()
            .find(|s| self.gp_slot_is_live(s.id, now))
            .map(|s| s.id)
    }

    /// ★v24.6: 该槽位此刻是否被 XInput 输入位图按下 (XInput 命名槽位, 如摇杆方向/按下)。
    fn gp_slot_is_live_xinput(&self, slot_id: usize, vid: u16, mask: u32) -> bool {
        let Some(slot) = get_slot(slot_id) else {
            return false;
        };
        match find_slot_mapping_index(&self.config, slot) {
            Some(i) => trigger_matches_xinput(&self.config.mappings[i].trigger_key, vid, mask),
            None => false,
        }
    }

    /// ★v24.6: 这一帧新按下的槽位 (XInput live 位图版) —— 要求该槽位在上一帧还没被按下,
    /// 避免松手/位图收缩时把已按下的槽位重新选中。
    fn gp_newly_pressed_slot_xinput(
        &self,
        vid: u16,
        mask: u32,
        prev: Option<(u16, u32)>,
    ) -> Option<usize> {
        SLOTS
            .iter()
            .find(|s| {
                self.gp_slot_is_live_xinput(s.id, vid, mask)
                    && !prev.is_some_and(|(pv, pm)| {
                        pv == vid && self.gp_slot_is_live_xinput(s.id, vid, pm)
                    })
            })
            .map(|s| s.id)
    }

    /// ★v22.0: 当前正在"实时识别"的手柄 (从已枚举列表里按 vid:pid 找)。
    fn live_pad(&self) -> Option<crate::rawinput::RawHidGamepad> {
        let (v, p) = self.app_state.live_hid_pad()?;
        self.raw_hid_pads
            .iter()
            .find(|d| d.vid == v && d.pid == p)
            .cloned()
    }

    /// ★v24.0: 槽位映射由"一键校对"创建 (触发键 = 捕获到的原始命名)。
    /// 手柄页不再自行臆造触发键 —— 未校对就是没映射。
    /// 就近警告 (橙色, 数秒后消失)。
    fn set_gamepad_warn(&mut self, msg: impl Into<String>) {
        self.gamepad_warn = Some(msg.into());
        self.gamepad_warn_until =
            Some(std::time::Instant::now() + std::time::Duration::from_secs(6));
    }

    /// 成功提示 (绿色, 数秒后消失)。
    fn set_gamepad_toast(&mut self, msg: impl Into<String>) {
        self.gamepad_toast = Some(msg.into());
        self.gamepad_toast_until =
            Some(std::time::Instant::now() + std::time::Duration::from_secs(4));
    }

    /// 选中槽位 (不自动进入捕获)。
    fn select_slot(&mut self, slot_id: usize) {
        self.gp_flow = self.gp_flow.transition(GpEvent::SelectSlot(slot_id));
    }

    /// 选中槽位; 若该键还没设键盘键 → 自动进入第 2 步 (识别态按手柄键的两步走)。
    fn select_slot_autostep(&mut self, slot_id: usize) {
        self.select_slot(slot_id);
        let Some(slot) = get_slot(slot_id) else { return };
        let has_target = find_slot_mapping_index(&self.config, slot)
            .map(|i| !self.config.mappings[i].target_keys.is_empty())
            .unwrap_or(false);
        if !has_target {
            self.begin_set_kb(slot_id);
        }
    }

    /// 进入第 2 步 (设键盘键)。
    ///
    /// ★v24.0: 手柄页只是连发映射的可视化 UI —— **映射必须已由校对创建**。
    /// 未校对的槽位不给设键盘键 (先校对, 才有触发键)。
    fn begin_set_kb(&mut self, slot_id: usize) {
        let Some(slot) = get_slot(slot_id) else { return };
        if find_slot_mapping_index(&self.config, slot).is_none() {
            self.set_gamepad_warn("这个键还没校对 — 请先点右上「一键校对手柄按键」");
            return;
        }
        self.gp_flow = self
            .gp_flow
            .transition(GpEvent::SelectSlot(slot_id))
            .transition(GpEvent::BeginSetKb);
        /* ★v24.4: 进入键盘捕获 → 退出"按手柄键继续映射"的监听态 (捕获通道也关掉) */
        self.gp_quick_listen = false;
        self.app_state.set_raw_input_capture_mode(false);
        self.gp_pad_capture.reset();
        self.capture_pressed_keys.clear();
        self.capture_initial_pressed = Self::poll_all_pressed_keys();
        self.just_captured_input = true;
    }

    /// 进入"校对手柄键位"。
    ///
    /// ★v22.3: 改走**连发同款原始报文捕获通道** (与是否识别过手柄无关) ——
    /// 语义 usage 通道对非标准手柄的 LT/RT 根本没有事件, 而原始位组合通道能精准抓到。
    /// 也不预先建映射 (触发键要等抓到真实按键名才知道)。
    fn begin_set_pad(&mut self, slot_id: usize) {
        if get_slot(slot_id).is_none() {
            return;
        }
        self.gp_flow = self
            .gp_flow
            .transition(GpEvent::SelectSlot(slot_id))
            .transition(GpEvent::BeginSetPad);
        self.capture_pressed_keys.clear();
        self.just_captured_input = true;
        self.gp_pad_capture.reset();
        self.gp_quick_listen = false;
        self.app_state.set_raw_input_capture_mode(true);
    }

    /// 取消: **只回退一层**并关掉捕获通道 —— 保留手柄识别 (旧版"取消即退出识别"已废弃)。
    fn cancel_gp(&mut self) {
        self.gp_flow = self.gp_flow.transition(GpEvent::Cancel);
        self.capture_pressed_keys.clear();
        self.just_captured_input = false;
        self.gp_pad_capture.reset();
        self.gp_quick_listen = false;
        self.app_state.set_raw_input_capture_mode(false);
    }

    /// 停止识别 → 回初始状态 (现在唯一的"退出识别"入口)。
    fn stop_identify(&mut self) {
        self.app_state.set_live_hid_pad(None);
        self.app_state.set_raw_input_capture_mode(false);
        self.gp_flow = self.gp_flow.transition(GpEvent::Reset);
        self.capture_pressed_keys.clear();
        self.just_captured_input = false;
        self.gp_pad_capture.reset();
        self.gp_quick_listen = false;
        self.gp_prev_xinput = None;
        self.gamepad_toast = None;
        self.gamepad_toast_until = None;
    }

    /// 确认写入捕获到的触发键/目标键。
    fn gp_confirm_capture(&mut self) {
        let GpFlow::ConfirmCapture {
            slot: slot_id,
            kind,
            value,
        } = self.gp_flow.clone()
        else {
            return;
        };
        let Some(slot) = get_slot(slot_id) else { return };
        match kind {
            GpCaptureKind::Trigger => {
                /* ★v24.0: 校准 = 直接写这条映射的触发键 (备注已是系统备注)。
                 * 与连发映射完全一致, 不再有独立的校准表。 */
                set_slot_trigger(&mut self.config, slot, value.clone());
                self.set_gamepad_toast(format!("已校对: {} 键", slot.label));
            }
            GpCaptureKind::Target => {
                if find_slot_mapping_index(&self.config, slot).is_none() {
                    self.set_gamepad_warn("这个键还没校对 — 请先点「一键校对手柄按键」");
                    self.gp_flow = self.gp_flow.transition(GpEvent::Cancel);
                    return;
                }
                add_slot_target(&mut self.config, slot, value.clone());
                self.set_gamepad_toast(format!("已设置: {} → {}", slot.label, value));
            }
        }
        let _ = self.config.save_to_file("Config.toml");
        if let Err(e) = self.app_state.reload_config(self.config.clone()) {
            eprintln!("Failed to reload config after capture confirm: {e}");
        }
        self.gp_flow = self.gp_flow.transition(GpEvent::ConfirmCapture);
        /* ★v24.4: 完成页允许"直接按其他手柄键继续映射"。
         * ★v24.6: 用 live 位图 (identify 状态已开) 自动选中, **不再启用捕获模式** ——
         * 捕获模式会抑制正常映射派发, 导致进游戏后奔跑/方向失效。 */
        if matches!(self.gp_flow, GpFlow::MapDone { .. }) {
            self.gp_pad_capture.reset();
            self.gp_quick_listen = true;
        }
    }

    /// 确认页「再加一个键」→ 回第 2 步继续捕获 (组成组合键)。
    fn gp_add_another_key(&mut self) {
        self.gp_flow = self.gp_flow.transition(GpEvent::AddAnotherKey);
        self.capture_pressed_keys.clear();
        self.capture_initial_pressed = Self::poll_all_pressed_keys();
        self.just_captured_input = true;
    }

    /// 删除二次确认 → 真删。
    fn gp_confirm_delete(&mut self) {
        let GpFlow::ConfirmDelete { slot: slot_id } = self.gp_flow.clone() else {
            return;
        };
        let Some(slot) = get_slot(slot_id) else { return };
        remove_slot_mapping(&mut self.config, slot);
        let _ = self.config.save_to_file("Config.toml");
        if let Err(e) = self.app_state.reload_config(self.config.clone()) {
            eprintln!("Failed to reload config after remove mapping: {e}");
        }
        self.set_gamepad_toast(format!("已删除: {}", slot.label));
        self.gp_flow = self.gp_flow.transition(GpEvent::ConfirmDelete);
    }

    /// 内联「更多设置」(连发 / 简易奔跑 / 重推奔跑) —— 不再跳去连发页。
    fn render_slot_advanced(
        &mut self,
        ui: &mut egui::Ui,
        _th: &Theme,
        slot: &'static GamepadSlot,
    ) {
        let Some(idx) = find_slot_mapping_index(&self.config, slot) else {
            return;
        };
        let (mut turbo, mut dtap, mut run, mut run_thr) = {
            let m = &self.config.mappings[idx];
            (
                m.turbo_enabled,
                m.double_tap_enabled,
                m.run_enabled,
                m.run_threshold,
            )
        };
        egui::CollapsingHeader::new("更多设置 (连发 / 奔跑)")
            .id_salt(("gp_slot_advanced", slot.id))
            .show(ui, |ui| {
                let mut changed = false;
                ui.horizontal_wrapped(|ui| {
                    changed |= ui
                        .checkbox(&mut turbo, "⚡ 连发")
                        .on_hover_text("勾选 = 此键触发连发; 不勾选 = 单发")
                        .changed();
                    ui.add_enabled_ui(!run, |ui| {
                        changed |= ui
                            .checkbox(&mut dtap, "简易奔跑")
                            .on_hover_text(if run {
                                "与「重推奔跑」互斥"
                            } else {
                                "按一次自动补一次敲击 (DNF 简易双击跑)"
                            })
                            .changed();
                    });
                    ui.add_enabled_ui(!dtap, |ui| {
                        changed |= ui
                            .checkbox(&mut run, "🏃 重推奔跑")
                            .on_hover_text(if dtap {
                                "与「简易奔跑」互斥"
                            } else {
                                "摇杆轻推=走, 推过阈值=自动补一次 (游戏判定双击→奔跑)"
                            })
                            .changed();
                    });
                });
                if run {
                    changed |= ui
                        .add(egui::Slider::new(&mut run_thr, 10..=100).text("重推阈值"))
                        .changed();
                }
                if changed {
                    let m = &mut self.config.mappings[idx];
                    m.turbo_enabled = turbo;
                    m.double_tap_enabled = dtap && !run;
                    m.run_enabled = run;
                    m.run_threshold = run_thr;
                    let _ = self.config.save_to_file("Config.toml");
                    if let Err(e) = self.app_state.reload_config(self.config.clone()) {
                        eprintln!("Failed to reload config after slot advanced edit: {e}");
                    }
                }
            });
    }

    /// ★v22.0/★v22.6: 「手柄按键快速映射」→ 枚举并读取选中的手柄 (原「开始识别手柄」)。
    fn start_identify_first_pad(&mut self) {
        if self.raw_hid_pads.is_empty() {
            self.raw_hid_pads = crate::rawinput::enumerate_raw_hid_gamepads();
            self.raw_hid_pads_fetched = Some(std::time::Instant::now());
        }
        if self.raw_hid_pads.is_empty() {
            self.set_gamepad_warn("没找到第三方手柄 — 请确认已连接 (官方 Xbox 手柄无需识别)");
            return;
        }
        if self.raw_hid_selected >= self.raw_hid_pads.len() {
            self.raw_hid_selected = 0;
        }
        let pad = self.raw_hid_pads[self.raw_hid_selected].clone();
        self.app_state.set_live_hid_pad(Some((pad.vid, pad.pid)));
        self.raw_hid_status = Some(pad.device_name.clone());
        /* ★v24.2: 进入"监听"态 —— 按手柄上任意已校对的键, 直接选中该槽位并开始设键盘键。
         * ★v24.6: 只开 live 状态匹配 (raw + XInput 位图), **不启用捕获模式**, 不干扰正常映射派发。 */
        self.gp_quick_listen = true;
        self.set_gamepad_toast(format!(
            "已开始识别: {} — 按手柄上已校对的键, 会直接跳到它的键盘键设置",
            pad.device_name
        ));
    }
}

/// ★v22.0: 步骤标记 —— 「第 N 步」小徽章 + 一句话标题 (一步一步来)。
fn step_header(ui: &mut egui::Ui, th: &Theme, step: u8, title: &str) {
    ui.horizontal(|ui| {
        th.badge(ui, &format!("第 {step} 步"), egui::Color32::WHITE, th.accent);
        ui.add_space(6.0);
        ui.label(
            egui::RichText::new(title)
                .size(15.0)
                .strong()
                .color(th.title),
        );
    });
}

/// 空状态矢量图标 (52px Gamepad)。`interactive` = 可点击 (accent 色 + 悬停放大 + 手型光标)。
/// 返回是否被点击 (非交互态恒 false)。
fn render_slot_empty_icon(ui: &mut egui::Ui, th: &Theme, interactive: bool) -> bool {
    let sense = if interactive {
        egui::Sense::click()
    } else {
        egui::Sense::hover()
    };
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(64.0, 64.0), sense);
    if interactive && resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let t = ui.ctx().animate_bool_with_time(
        ui.id().with("empty_icon_hover"),
        interactive && resp.hovered(),
        0.15,
    );
    let scale = 1.0 + 0.12 * t;
    let glyph_rect = egui::Rect::from_center_size(rect.center(), rect.size() * scale);
    let color = if interactive && resp.hovered() {
        th.accent
    } else {
        th.accent_soft
    };
    crate::gui::widgets::Icon::Gamepad.paint(ui.painter(), glyph_rect, color);
    interactive && resp.clicked()
}

/// ★v22.6: 空状态里用户点了哪个入口。
#[derive(PartialEq, Eq)]
enum EmptyAction {
    None,
    /// 「手柄按键快速映射」(读取手柄 → 按手柄键快速设映射)
    Map,
    /// 「一键校对手柄按键」
    Calibrate,
}

/// 未选中槽位时的引导空状态。
/// 未识别 → 主推「一键校对手柄按键」+「手柄按键快速映射」两个入口;
/// 已识别 → 提示直接点图上的键。
fn render_slot_empty_state(ui: &mut egui::Ui, th: &Theme, identified: bool) -> EmptyAction {
    let mut action = EmptyAction::None;
    th.panel(ui, None, |ui| {
        ui.add_space(12.0);
        ui.vertical_centered(|ui| {
            ui.set_min_width(ui.available_width());
            if render_slot_empty_icon(ui, th, !identified) {
                action = EmptyAction::Map;
            }
            ui.add_space(6.0);
            if identified {
                ui.label(
                    egui::RichText::new("点左边手柄图上的按键, 或直接按手柄上的键")
                        .size(15.0)
                        .strong()
                        .color(th.title),
                );
                ui.add_space(8.0);
                ui.label(th.hint_text("选中一个键后, 右栏会告诉你下一步怎么做"));
            } else {
                /* ① 新手第一步: 主推「一键校对手柄按键」(实心强调色, 最显眼) */
                if ui
                    .add(
                        egui::Button::new(
                            egui::RichText::new("一键校对手柄按键")
                                .size(16.0)
                                .strong()
                                .color(egui::Color32::WHITE),
                        )
                        .fill(th.accent)
                        .corner_radius(egui::CornerRadius::same(theme::RADIUS_CTRL))
                        .min_size(egui::vec2(200.0, 38.0)),
                    )
                    .on_hover_text("按提示把手柄上全部热点校对一遍, 自动记住键位")
                    .clicked()
                {
                    action = EmptyAction::Calibrate;
                }
                ui.add_space(8.0);
                /* ② 第二步: 读手柄 → 按手柄键快速设映射 */
                if ui
                    .add(
                        egui::Button::new(
                            egui::RichText::new("手柄按键快速映射")
                                .size(15.0)
                                .strong()
                                .color(th.accent),
                        )
                        .fill(egui::Color32::TRANSPARENT)
                        .stroke(egui::Stroke::new(1.2, th.accent_soft))
                        .corner_radius(egui::CornerRadius::same(theme::RADIUS_CTRL))
                        .min_size(egui::vec2(200.0, 36.0)),
                    )
                    .on_hover_text("读取你的手柄; 之后按手柄上的键即可快速设置映射")
                    .clicked()
                {
                    action = EmptyAction::Map;
                }
                ui.add_space(8.0);
                /* ★v24.8: 步骤说明已并入顶部状态条 —— 这里只留一句结论, 避免重复把卡片撑高 */
                ui.label(th.hint_text("先完整校对一次, 之后按手柄上的键即可快速设键位"));
            }
            ui.add_space(12.0);
        });
    });
    action
}

#[cfg(test)]
mod live_highlight_tests {
    use super::*;
    use crate::state::LiveHidState;

    /// 构造一个"原始位组合"实时状态 (v23.0 起: 高亮只看 raw_position)。
    fn live_raw(pos: u32) -> LiveHidState {
        LiveHidState {
            vid: 0x20BC,
            pid: 0x5159,
            raw_position: pos,
            ..Default::default()
        }
    }

    #[test]
    fn slot_note_is_system_format() {
        assert_eq!(slot_note("A 键"), "手柄A 键（系统）");
        assert_eq!(slot_note("十字键·上"), "手柄十字键·上（系统）");
    }

    /// 只按系统备注关联映射 —— **不能**按 default_trigger / 旧备注串味 (那是双触发根源)。
    #[test]
    fn find_slot_mapping_matches_only_system_note() {
        use crate::config::KeyMapping;
        let mk = |trigger: &str, note: &str| KeyMapping {
            trigger_key: trigger.to_string(),
            target_keys: Default::default(),
            interval: None,
            event_duration: None,
            turbo_enabled: true,
            move_speed: 5,
            double_tap_enabled: false,
            double_tap_gap_ms: 50,
            run_enabled: false,
            run_threshold: 80,
            run_recheck: true,
            note: note.to_string(),
        };
        let slot = get_slot(19).unwrap(); // A 键
        let mut cfg = AppConfig::default();
        // 只有旧 XInput 默认名 + 旧备注 → 不认
        cfg.mappings.push(mk("GAMEPAD_045E_A", "手柄·A 键"));
        assert_eq!(find_slot_mapping_index(&cfg, slot), None);
        // 有系统备注 → 认
        cfg.mappings.push(mk("GAMEPAD_20BC_5159_X_B1.2", "手柄A 键（系统）"));
        let expect = cfg.mappings.len() - 1;
        assert_eq!(find_slot_mapping_index(&cfg, slot), Some(expect));
    }

    /// ★v24.0: 旧备注一次性迁移为系统备注 (触发键/目标键保留); 重复条目去重。
    #[test]
    fn migrate_slot_notes_renames_and_dedupes() {
        use crate::config::KeyMapping;
        let mk = |trigger: &str, note: &str| KeyMapping {
            trigger_key: trigger.to_string(),
            target_keys: vec!["Q".to_string()].into(),
            interval: None,
            event_duration: None,
            turbo_enabled: true,
            move_speed: 5,
            double_tap_enabled: false,
            double_tap_gap_ms: 50,
            run_enabled: false,
            run_threshold: 80,
            run_recheck: true,
            note: note.to_string(),
        };
        let mut cfg = AppConfig::default();
        cfg.mappings.push(mk("GAMEPAD_20BC_5159_X_B1.2", "手柄·A 键"));
        cfg.mappings.push(mk("GAMEPAD_20BC_5159_X_B1.2", "手柄·A 键")); // 重复
        assert!(migrate_slot_notes(&mut cfg));
        let a: Vec<&KeyMapping> = cfg
            .mappings
            .iter()
            .filter(|m| m.note == "手柄A 键（系统）")
            .collect();
        assert_eq!(a.len(), 1, "同槽位只保留一条");
        assert_eq!(a[0].trigger_key, "GAMEPAD_20BC_5159_X_B1.2", "触发键保留");
        assert_eq!(a[0].target_keys.len(), 1, "目标键保留");
        // 幂等
        assert!(!migrate_slot_notes(&mut cfg));
    }

    /// 高亮判定: 必须"同设备前缀 + 同位组合"才匹配; 0 位置(未按)不匹配。
    #[test]
    fn trigger_matches_raw_requires_same_device_and_position() {
        let name = "GAMEPAD_20BC_5159_DEVDEADBEEF_B1.7";
        assert!(trigger_matches_raw(name, 0x20BC, 0x5159, 0x0001_0007));
        // 位置不同
        assert!(!trigger_matches_raw(name, 0x20BC, 0x5159, 0x0002_0007));
        // 未按下
        assert!(!trigger_matches_raw(name, 0x20BC, 0x5159, 0));
        // 设备不同
        assert!(!trigger_matches_raw(name, 0x045E, 0x028E, 0x0001_0007));
        // 完全不是原始命名
        assert!(!trigger_matches_raw("GAMEPAD_045E_A", 0x20BC, 0x5159, 0x0001_0007));
    }

    #[test]
    fn calibration_order_covers_all_hotspots_without_duplicates() {
        assert_eq!(CALIBRATION_ORDER.len(), SLOTS.len());
        assert_eq!(calibration_slot(0), Some(19)); // A 起
        assert_eq!(calibration_slot(23), Some(15)); // 右摇杆·右 止
        assert_eq!(calibration_slot(CALIBRATION_ORDER.len()), None);
        let mut seen = std::collections::HashSet::new();
        for id in CALIBRATION_ORDER {
            assert!(get_slot(*id).is_some(), "无效槽位 {id}");
            assert!(seen.insert(*id), "重复槽位 {id}");
        }
        for slot in SLOTS {
            assert!(seen.contains(&slot.id), "漏了槽位 {} ({})", slot.id, slot.label);
        }
    }

    #[test]
    fn mouse_vks_are_filtered() {
        for vk in [0x01u32, 0x02, 0x04, 0x05, 0x06] {
            assert!(is_mouse_vk(vk));
        }
        assert!(!is_mouse_vk(0x41)); // A
        assert!(!is_mouse_vk(0x20)); // SPACE
    }
}

/// ★v24.2: 按压窗口合并 + 按槽位挑选 —— 校准向导回归测试。
#[cfg(test)]
mod pad_capture_reconcile_tests {
    use super::*;
    use crate::state::{DeviceType, InputDevice};
    use std::time::{Duration, Instant};

    /// 原始 HID 通道事件 (第三方手柄, 走 RawInput)。
    fn raw(pos: u64) -> InputDevice {
        InputDevice::GenericDevice {
            device_type: DeviceType::Gamepad(0x20BC),
            button_id: (1u64 << 32) | pos,
        }
    }

    /// XInput 通道事件 (命名恒为 `GAMEPAD_045E_*`)。
    fn xinput(ids: &[u32]) -> InputDevice {
        InputDevice::XInputCombo {
            device_type: DeviceType::Gamepad(0x045E),
            button_ids: ids.to_vec(),
        }
    }

    fn xinput1(id: u32) -> InputDevice {
        xinput(&[id])
    }

    /// 模拟校准向导: 按时间轴喂入事件, 用 [`pick_calibration_device`] 挑输入。
    /// 返回 (每步写入的 (槽位标签, 触发键), 被拒绝时的提示)。
    fn run_wizard(
        events: &[(InputDevice, Duration)],
        hold: Duration,
    ) -> (Vec<(String, String)>, Vec<String>) {
        let mut rec = PadCaptureReconciler::new(hold);
        let mut cfg = AppConfig::default();
        let mut step = 0usize;
        let mut out = Vec::new();
        let mut errs = Vec::new();
        let base = Instant::now();
        let mut handle = |cands: PadCaptureCandidates,
                          cfg: &mut AppConfig,
                          step: &mut usize,
                          out: &mut Vec<(String, String)>,
                          errs: &mut Vec<String>| {
            let Some(slot_id) = calibration_slot(*step) else {
                return;
            };
            let Some(slot) = get_slot(slot_id) else {
                return;
            };
            match pick_calibration_device(slot, &cands) {
                Ok(dev) => {
                    let name = dev.to_string();
                    set_slot_trigger(cfg, slot, name.clone());
                    out.push((slot.label.to_string(), name));
                    *step += 1;
                }
                Err(e) => errs.push(e),
            }
        };
        for (dev, offset) in events {
            let now = base + *offset;
            if let Some(c) = rec.feed(dev.clone(), now) {
                handle(c, &mut cfg, &mut step, &mut out, &mut errs);
                continue;
            }
            if let Some(c) = rec.tick(now) {
                handle(c, &mut cfg, &mut step, &mut out, &mut errs);
            }
        }
        // 收尾: 让仍挂起的窗口结算 (正常 UI 每帧都会 tick)。
        let end = base + hold * 2 + Duration::from_secs(1);
        if let Some(c) = rec.tick(end) {
            handle(c, &mut cfg, &mut step, &mut out, &mut errs);
        }
        (out, errs)
    }

    /// 实机 bug: 按一个键, A 槽 (原始命名) + B 槽 (`GAMEPAD_045E_A`) 同时被填。
    #[test]
    fn one_press_with_dual_channel_writes_one_slot() {
        let events = vec![
            (raw(0x0001_0007), Duration::from_millis(0)),
            (xinput1(0x0B), Duration::from_millis(6)),
        ];
        let (steps, errs) = run_wizard(&events, Duration::from_millis(PAD_CAPTURE_HOLD_MS));
        assert_eq!(steps.len(), 1, "一次按压只能记一步, 实际: {steps:?} {errs:?}");
        assert_eq!(steps[0].0, "A 键", "应记入当前步 (A 键), 实际: {steps:?}");
        assert!(
            steps[0].1.starts_with("GAMEPAD_20BC_"),
            "按钮槽位应优先原始命名, 实际: {}",
            steps[0].1
        );
    }

    /// XInput 事件先到时也要折叠成一步, 并仍优先原始命名。
    #[test]
    fn xinput_first_still_prefers_raw_name() {
        let events = vec![
            (xinput1(0x0B), Duration::from_millis(0)),
            (raw(0x0001_0007), Duration::from_millis(8)),
        ];
        let (steps, errs) = run_wizard(&events, Duration::from_millis(PAD_CAPTURE_HOLD_MS));
        assert_eq!(steps.len(), 1, "反序到达也要折叠成一步: {steps:?} {errs:?}");
        assert!(steps[0].1.starts_with("GAMEPAD_20BC_"), "实际: {}", steps[0].1);
    }

    /// 官方 Xbox 手柄被 RawInput 忽略 → 只会收到 XInput 事件, 不能丢。
    #[test]
    fn lone_xinput_event_commits_after_hold() {
        let events = vec![(xinput1(0x0B), Duration::from_millis(0))];
        let (steps, errs) = run_wizard(&events, Duration::from_millis(PAD_CAPTURE_HOLD_MS));
        assert_eq!(steps.len(), 1, "单通道事件不能丢: {steps:?} {errs:?}");
        assert_eq!(steps[0].1, "GAMEPAD_045E_A");
    }

    /// 两次按压间隔超过窗口 → 正常记两步 (窗口不能无限大)。
    #[test]
    fn two_presses_beyond_window_are_recorded_separately() {
        let events = vec![
            (raw(0x0001_0007), Duration::from_millis(0)),
            (raw(0x0002_0007), Duration::from_millis(900)),
        ];
        let (steps, errs) = run_wizard(&events, Duration::from_millis(PAD_CAPTURE_HOLD_MS));
        assert_eq!(steps.len(), 2, "超出窗口的两次按压应各记一步: {steps:?} {errs:?}");
        assert_ne!(steps[0].1, steps[1].1);
    }

    /// 窗口按用户实测定为 300ms。
    #[test]
    fn default_window_is_300ms() {
        assert_eq!(PAD_CAPTURE_HOLD_MS, 300);
        let hold = Duration::from_millis(PAD_CAPTURE_HOLD_MS);
        let base = Instant::now();
        // 迟到 200ms 的同一次按压 → 仍在窗口内, 合并
        let mut rec = PadCaptureReconciler::new(hold);
        assert!(rec.feed(raw(0x0001_0007), base).is_none());
        assert!(rec.feed(xinput1(0x0B), base + Duration::from_millis(200)).is_none());
        assert!(rec.tick(base + hold + Duration::from_millis(1)).is_some());
        // 迟到 400ms → 超出窗口, 上一次先结算
        let mut rec = PadCaptureReconciler::new(hold);
        assert!(rec.feed(raw(0x0001_0007), base).is_none());
        assert!(
            rec.feed(xinput1(0x0B), base + Duration::from_millis(400)).is_some(),
            "超出窗口应先结算上一次"
        );
    }

    /// 实机症状: 右摇杆按下后回弹 (`RS_Click` → `RS_Right`) 都在同一次按压里,
    /// 合并后按"摇杆按下"槽位过滤 → 只留 click, 回弹被丢掉。
    #[test]
    fn stick_click_rebound_is_filtered_to_the_click() {
        let hold = Duration::from_millis(PAD_CAPTURE_HOLD_MS);
        let base = Instant::now();
        let mut rec = PadCaptureReconciler::new(hold);
        assert!(rec.feed(xinput1(0x08), base).is_none()); // RS_Click
        assert!(rec.feed(xinput1(0x14), base + Duration::from_millis(200)).is_none()); // RS_Right 回弹
        let c = rec.tick(base + hold + Duration::from_millis(1)).expect("窗口应结算");
        let slot = get_slot(23).unwrap(); // 右摇杆·按下
        let got = pick_calibration_device(slot, &c).expect("回弹应被过滤, 只留 click");
        assert_eq!(got.to_string(), "GAMEPAD_045E_RS_Click");
    }

    /// 斜推摇杆 (两个方向) → 判为不干净, 让用户重按 (不能把斜推记成一个方向)。
    #[test]
    fn diagonal_stick_push_is_rejected() {
        let slot = get_slot(10).unwrap(); // 左摇杆·左
        let mut c = PadCaptureCandidates::default();
        c.xinput = Some(xinput(&[0x11, 0x13])); // LS_Left + LS_Down
        assert!(pick_calibration_device(slot, &c).is_err(), "斜推应被判为不干净");

        let slot_click = get_slot(22).unwrap(); // 左摇杆·按下
        let mut c2 = PadCaptureCandidates::default();
        c2.xinput = Some(xinput(&[0x14])); // 只有方向、没有 click
        assert!(pick_calibration_device(slot_click, &c2).is_err(), "缺 click 应重按");
    }

    /// 摇杆槽位优先 XInput (摇杆语义只有 XInput 可靠), 按钮槽位优先原始命名。
    #[test]
    fn stick_slots_prefer_xinput_button_slots_prefer_raw() {
        let stick = get_slot(10).unwrap(); // 左摇杆·左
        let mut c = PadCaptureCandidates::default();
        c.raw = Some(raw(0x0002_0007));
        c.xinput = Some(xinput(&[0x11]));
        assert_eq!(
            pick_calibration_device(stick, &c).unwrap().to_string(),
            "GAMEPAD_045E_LS_Left"
        );

        let button = get_slot(19).unwrap(); // A 键
        let mut c2 = PadCaptureCandidates::default();
        c2.raw = Some(raw(0x0001_0007));
        assert!(pick_calibration_device(button, &c2).unwrap().to_string().starts_with("GAMEPAD_20BC_"));

        let mut c3 = PadCaptureCandidates::default();
        c3.xinput = Some(xinput(&[0x0B, 0x0C])); // A+B 同时按
        assert!(pick_calibration_device(button, &c3).is_err(), "多键应被拒绝");
    }

    /// 旧数据串位自愈: 该键已被别的槽位占用时, 移到当前槽位而不是拒绝
    /// (实机症状: A 键的位组合被旧配置记在 X 槽 → 重新校对按 A 被拦下)。
    #[test]
    fn set_slot_trigger_moves_conflicting_key() {
        let mut cfg = AppConfig::default();
        let a = get_slot(19).unwrap();
        let x = get_slot(16).unwrap();
        set_slot_trigger(&mut cfg, x, "GAMEPAD_20BC_5159_DEVAA_H123".to_string());
        let moved =
            set_slot_trigger_moving(&mut cfg, a, "GAMEPAD_20BC_5159_DEVAA_H123".to_string());
        assert_eq!(moved.as_deref(), Some("X 键"), "应报告从哪个槽位移走");
        assert!(find_slot_mapping_index(&cfg, a).is_some(), "当前槽位应写入");
        assert!(find_slot_mapping_index(&cfg, x).is_none(), "旧槽位应被清掉");
        assert_eq!(
            slot_trigger_display(&cfg, a),
            "GAMEPAD_20BC_5159_DEVAA_H123"
        );
    }

    /// ★v24.6: XInput 实时位图匹配 (快速映射识别 XInput 命名槽位, 不依赖捕获模式)。
    #[test]
    fn xinput_live_match() {
        let ls_left = 1u32 << 0x11;
        assert!(trigger_matches_xinput("GAMEPAD_045E_LS_Left", 0x045E, ls_left));
        assert!(!trigger_matches_xinput("GAMEPAD_045E_LS_Left", 0x045E, 1u32 << 0x10));
        assert!(!trigger_matches_xinput("GAMEPAD_045E_LS_Left", 0x20BC, ls_left));
        assert!(!trigger_matches_xinput("GAMEPAD_20BC_5159_DEVAA_H1", 0x045E, ls_left));
        assert!(!trigger_matches_xinput("GAMEPAD_045E_LS_Left", 0x045E, 0));
        // 组合键: 必须所有 id 都在位图里
        let ab = (1u32 << 0x0B) | (1u32 << 0x0C);
        assert!(trigger_matches_xinput("GAMEPAD_045E_A+B", 0x045E, ab));
        assert!(!trigger_matches_xinput("GAMEPAD_045E_A+B", 0x045E, 1u32 << 0x0B));
    }

    /// 触发键短名: 原始命名去掉设备前缀, XInput 命名去掉 GAMEPAD_。
    #[test]
    fn short_trigger_strips_device_prefix() {
        assert_eq!(
            short_trigger_name("GAMEPAD_20BC_5159_DEV5A27EA46_H680322833"),
            "H680322833"
        );
        assert_eq!(short_trigger_name("GAMEPAD_045E_RS_Click"), "045E_RS_Click");
    }
}
