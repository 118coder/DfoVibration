//! 手柄槽位模型: SlotKind 类别 / GamepadSlot 24 槽位表(SLOTS) / 校准步序 —— 原 gamepad_mapping.rs 24-184, 2026-09-27 架构重构 C3 归位。


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
pub(super) fn calibration_short_label(slot_id: usize) -> &'static str {
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
