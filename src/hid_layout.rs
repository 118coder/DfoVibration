//! ★v21.7 方案 A: HID 报告描述符解析 + 通用手柄布局约定 (自主实现, 零新依赖)。
//!
//! 目的: 让第三方 HID 手柄 (只能通过 RawInput 拿到原始报文的那类) 也能被"翻译"成
//! 标准布局 —— A/B/X/Y/LB/RB/LT/RT/Back/Start/L3/R3/十字键 + 左右摇杆, 从而支持
//! 「一键生成完整标准布局映射」, 用户不必再逐个按键手动捕获。
//!
//! 本模块是**纯函数**: 输入 = HID 报告描述符字节, 输出 = [`HidLayout`] (按钮位偏移 +
//! 轴位偏移/量程)。不含任何 Windows 调用, 因此可离线单测 (喂样本描述符断言布局)。
//! 运行时的描述符字节获取与 HidP_* 交叉校验见 `rawinput.rs` 的 `hid_layout` 集成点。
//!
//! 依据: HID 1.11 规范 §6.2.2 (Item 编码) + §6.2.2.7/6.2.2.8 (Global/Local) +
//! Windows 游戏手柄通用布局惯例 (DirectInput 语义):
//!   Button1=A(下) 2=B(右) 3=X(左) 4=Y(上); 5=LB 6=RB 7=LT 8=RT;
//!   9=Back 10=Start 11=L3 12=R3; X/Y=左摇杆 Rx/Ry=右摇杆 Z/Rz=扳机 Hat=十字键。

use std::collections::BTreeMap;

/// HID Usage Page: Generic Desktop Controls。
pub const USAGE_PAGE_GENERIC: u16 = 0x01;
/// HID Usage Page: Button。
pub const USAGE_PAGE_BUTTON: u16 = 0x09;

/// Generic Desktop 轴 usage。
pub const USAGE_X: u16 = 0x30;
pub const USAGE_Y: u16 = 0x31;
pub const USAGE_Z: u16 = 0x32;
pub const USAGE_RX: u16 = 0x33;
pub const USAGE_RY: u16 = 0x34;
pub const USAGE_RZ: u16 = 0x35;
pub const USAGE_SLIDER: u16 = 0x36;
pub const USAGE_DIAL: u16 = 0x37;
pub const USAGE_WHEEL: u16 = 0x38;
pub const USAGE_HAT_SWITCH: u16 = 0x39;

/// 解析报文的起始偏移: 原始 HID 报文前若干字节常为轴 (持续变化), 现有 bit-hash
/// 检测从第 5 字节起算 (见 rawinput.rs `SKIP_BYTES`)。生成触发键名时必须用同一约定,
/// 否则生成的映射在运行时永远匹配不上。
pub const RAW_REPORT_SKIP_BYTES: usize = 5;

/// 轴的标准语义 (用于布局展示与方向判定)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HidAxisKind {
    X,
    Y,
    Z,
    Rx,
    Ry,
    Rz,
    Slider,
    Dial,
    Wheel,
    HatSwitch,
    Other,
}

impl HidAxisKind {
    /// Generic Desktop usage → 轴语义。
    pub fn from_usage(usage: u16) -> Option<Self> {
        Some(match usage {
            USAGE_X => Self::X,
            USAGE_Y => Self::Y,
            USAGE_Z => Self::Z,
            USAGE_RX => Self::Rx,
            USAGE_RY => Self::Ry,
            USAGE_RZ => Self::Rz,
            USAGE_SLIDER => Self::Slider,
            USAGE_DIAL => Self::Dial,
            USAGE_WHEEL => Self::Wheel,
            USAGE_HAT_SWITCH => Self::HatSwitch,
            _ => return None,
        })
    }

    /// 展示名 (标准布局约定里的轴位置)。
    pub fn label(self) -> &'static str {
        match self {
            Self::X => "左摇杆·X",
            Self::Y => "左摇杆·Y",
            Self::Z => "扳机/滑条·Z",
            Self::Rx => "右摇杆·X",
            Self::Ry => "右摇杆·Y",
            Self::Rz => "扳机/滑条·Rz",
            Self::Slider => "滑条",
            Self::Dial => "旋钮",
            Self::Wheel => "滚轮",
            Self::HatSwitch => "十字键",
            Self::Other => "其他轴",
        }
    }

    /// 是否为左右摇杆的两轴之一 (三区奔跑关注的轴)。
    pub fn is_stick_axis(self) -> bool {
        matches!(self, Self::X | Self::Y | Self::Rx | Self::Ry)
    }
}

/// 一个按钮 (Usage Page 0x09) 在报文中的位置。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HidButton {
    /// 按钮序号 (Usage, 1-based)。标准约定 1=A 2=B 3=X 4=Y …
    pub usage: u32,
    /// 位偏移 —— **相对报告负载 (不含 Report ID 字节)**。
    pub bit_offset: u32,
    /// 所属 Report ID (0 = 无 Report ID)。
    pub report_id: u8,
}

/// 一个轴 (Generic Desktop 0x01) 在报文中的位置与量程。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HidAxis {
    pub usage: u16,
    pub kind: HidAxisKind,
    /// 位偏移 —— 相对报告负载 (不含 Report ID 字节)。
    pub bit_offset: u32,
    pub bit_size: u8,
    pub logical_min: i32,
    pub logical_max: i32,
    pub report_id: u8,
}

/// 解析错误。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HidParseError {
    /// 描述符为空。
    Empty,
    /// 解析途中越界或字段数异常。
    Truncated,
    /// report_count × report_size 过大 (疑似损坏描述符)。
    UnreasonableFieldCount,
}

impl std::fmt::Display for HidParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => write!(f, "描述符为空"),
            Self::Truncated => write!(f, "描述符被截断/格式非法"),
            Self::UnreasonableFieldCount => write!(f, "描述符字段数异常"),
        }
    }
}

/// 解析结果: 将 HID 报告描述符翻译成"设备能力"。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HidLayout {
    /// 是否使用 Report ID (若有, 每个报文首字节即 ID)。
    pub uses_report_ids: bool,
    /// 主输入报告的总位数 (取 Report ID 0/首个出现的报告)。
    pub input_report_bits: u32,
    /// 按钮, 按 usage 升序。
    pub buttons: Vec<HidButton>,
    /// 轴, 按位偏移升序。
    pub axes: Vec<HidAxis>,
}

impl HidLayout {
    /// 依据标准布局约定给出按钮语义名 (A/B/X/Y/LB/RB/LT/RT/Back/Start/L3/R3/Guide)。
    pub fn standard_button_name(&self, usage: u32) -> Option<&'static str> {
        standard_button_name(usage)
    }

    /// 按钮在**原始报文**中的 (字节下标, 位下标) —— 计入 Report ID 前缀。
    pub fn raw_byte_bit(&self, bit_offset: u32) -> (usize, u32) {
        let prefix_bits = if self.uses_report_ids { 8 } else { 0 };
        let abs = prefix_bits + bit_offset;
        ((abs / 8) as usize, abs % 8)
    }

    /// 按钮是否落在现有 bit-hash 检测区间内 (前 `RAW_REPORT_SKIP_BYTES` 字节被跳过),
    /// 只有 true 的按钮才能生成"运行时能命中"的映射。
    pub fn button_is_detectable(&self, b: &HidButton) -> bool {
        let (byte, _) = self.raw_byte_bit(b.bit_offset);
        byte >= RAW_REPORT_SKIP_BYTES
    }

    /// 主报告字节长度 (向上取整, 含 Report ID 字节)。
    pub fn report_bytes(&self) -> usize {
        let payload_bits = self.input_report_bits as usize;
        let payload = payload_bits.div_ceil(8);
        payload + if self.uses_report_ids { 1 } else { 0 }
    }

    /// 描述符里是否含左摇杆 (X+Y) —— 用于判断能否支持三区奔跑。
    pub fn has_left_stick(&self) -> bool {
        self.axes.iter().any(|a| a.kind == HidAxisKind::X)
            && self.axes.iter().any(|a| a.kind == HidAxisKind::Y)
    }
}

/// 标准布局约定: 按钮 Usage → 语义名。
pub fn standard_button_name(usage: u32) -> Option<&'static str> {
    Some(match usage {
        1 => "A",
        2 => "B",
        3 => "X",
        4 => "Y",
        5 => "LB",
        6 => "RB",
        7 => "LT",
        8 => "RT",
        9 => "Back",
        10 => "Start",
        11 => "L3",
        12 => "R3",
        13 => "Guide",
        _ => return None,
    })
}

/// ★v22.1: 模拟扳机超阈值判定。
///
/// 有**空闲基线**时用"与基线的偏移 > 半量程"——单极扳机 (静止≈0) 与居中轴 (静止=中点)
/// 都正确, 不会出现"居中轴静止就判按下"的假阳性。无基线时回落"原始值 > 半量程"。
/// (不做环形回绕: 扳机是单极轴, 满量程 255 就是按到底, 不是"离 0 一步"。)
/// `bits` = 轴位宽 (0 → 无此轴, false)。
pub fn trigger_pressed(raw: Option<u32>, baseline: Option<u32>, bits: u8) -> bool {
    let Some(v) = raw else { return false };
    let bits = bits.min(31);
    if bits == 0 {
        return false;
    }
    let max = (1u32 << bits) - 1;
    let half = max / 2;
    match baseline {
        Some(b) => (v as i64 - b as i64).unsigned_abs() > half as u64,
        None => v > half,
    }
}

/// 解析器内部全局状态 (Global items), 支持 Push/Pop。
#[derive(Debug, Clone, Copy)]
struct GlobalState {
    usage_page: u16,
    logical_min: i32,
    logical_max: i32,
    report_size: u32,
    report_count: u32,
    report_id: u8,
}

impl Default for GlobalState {
    fn default() -> Self {
        Self {
            usage_page: 0,
            logical_min: 0,
            logical_max: 0,
            report_size: 0,
            report_count: 0,
            report_id: 0,
        }
    }
}

/// 解析 HID 报告描述符字节 → [`HidLayout`]。
///
/// 只关心 Input (主项 0x8) 中非恒定的按钮与 Generic Desktop 轴; Output/Feature/
/// Collection 结构被跳过 (但仍参与局部状态清理, 保证 Usage 归属正确)。
pub fn parse_report_descriptor(data: &[u8]) -> Result<HidLayout, HidParseError> {
    if data.is_empty() {
        return Err(HidParseError::Empty);
    }

    let mut layout = HidLayout::default();
    let mut g = GlobalState::default();
    let mut stack: Vec<GlobalState> = Vec::new();

    // 局部项 (每个 Main item 后清空)
    let mut usages: Vec<u32> = Vec::new();
    let mut usage_min: Option<u32> = None;
    let mut usage_max: Option<u32> = None;

    // 每个 Report ID 各自的位游标 (位偏移不含 Report ID 字节)
    let mut cursors: BTreeMap<u8, u32> = BTreeMap::new();

    let mut i = 0usize;
    while i < data.len() {
        let prefix = data[i];
        i += 1;

        // Long item: 0xFE, size, tag, data...
        if prefix == 0xFE {
            if i + 2 > data.len() {
                return Err(HidParseError::Truncated);
            }
            let size = data[i] as usize;
            i += 2; // size + tag
            i = i.saturating_add(size);
            continue;
        }

        let size = match prefix & 0x03 {
            0 => 0usize,
            1 => 1,
            2 => 2,
            _ => 4,
        };
        let item_type = (prefix >> 2) & 0x03;
        let tag = (prefix >> 4) & 0x0F;

        if i + size > data.len() {
            return Err(HidParseError::Truncated);
        }
        let mut raw: u32 = 0;
        for k in 0..size {
            raw |= (data[i + k] as u32) << (8 * k);
        }
        i += size;
        let signed = sign_extend(raw, size);

        match item_type {
            // ── Main items ──
            0 => {
                match tag {
                    0x8 => {
                        // Input
                        let constant = (raw & 0x01) != 0; // bit0 = Constant
                        let total = g
                            .report_count
                            .checked_mul(g.report_size.max(1))
                            .ok_or(HidParseError::UnreasonableFieldCount)?;
                        if total > 8 * 4096 {
                            return Err(HidParseError::UnreasonableFieldCount);
                        }
                        let cursor = cursors.get(&g.report_id).copied().unwrap_or(0);
                        let mut field_cursor = cursor;

                        for f in 0..g.report_count {
                            let field_usage = field_usage_for(f, &usages, usage_min, usage_max);
                            if !constant {
                                match g.usage_page {
                                    USAGE_PAGE_BUTTON => {
                                        layout.buttons.push(HidButton {
                                            usage: field_usage,
                                            bit_offset: field_cursor,
                                            report_id: g.report_id,
                                        });
                                    }
                                    USAGE_PAGE_GENERIC => {
                                        if let Some(kind) =
                                            HidAxisKind::from_usage(field_usage as u16)
                                        {
                                            layout.axes.push(HidAxis {
                                                usage: field_usage as u16,
                                                kind,
                                                bit_offset: field_cursor,
                                                bit_size: g.report_size.min(32) as u8,
                                                logical_min: g.logical_min,
                                                logical_max: g.logical_max,
                                                report_id: g.report_id,
                                            });
                                        }
                                    }
                                    _ => {}
                                }
                            }
                            field_cursor += g.report_size;
                        }
                        cursors.insert(g.report_id, field_cursor);

                        if g.report_id == 0 || cursors.len() == 1 {
                            layout.input_report_bits = layout.input_report_bits.max(field_cursor);
                        }
                    }
                    0xA => { /* Collection: 结构信息, 忽略 */ }
                    0xC => { /* EndCollection */ }
                    _ => { /* Output(0x9)/Feature(0xB): 与输入布局无关 */ }
                }
                // 每个 Main item 后清局部状态
                usages.clear();
                usage_min = None;
                usage_max = None;
            }

            // ── Global items ──
            1 => match tag {
                0x0 => g.usage_page = raw as u16,
                0x1 => g.logical_min = signed,
                0x2 => g.logical_max = signed,
                0x7 => g.report_size = raw,
                0x8 => {
                    g.report_id = raw as u8;
                    layout.uses_report_ids = true;
                }
                0x9 => g.report_count = raw,
                0xA => stack.push(g),
                0xB => {
                    if let Some(prev) = stack.pop() {
                        g = prev;
                    }
                }
                _ => {}
            },

            // ── Local items ──
            2 => match tag {
                0x0 => usages.push(raw & 0xFFFF),
                0x1 => usage_min = Some(raw & 0xFFFF),
                0x2 => usage_max = Some(raw & 0xFFFF),
                _ => {}
            },

            _ => {}
        }
    }

    layout.buttons.sort_by_key(|b| b.usage);
    layout.buttons.dedup_by_key(|b| (b.report_id, b.bit_offset));
    layout.axes.sort_by_key(|a| a.bit_offset);
    Ok(layout)
}

/// 第 `field` 个字段使用的 Usage: 优先 UsageMin+field 区间, 否则按下标取 Usage 列表
/// (列表不足时重复最后一项 —— 单个 Usage + ReportCount>1 的常见写法)。
fn field_usage_for(
    field: u32,
    usages: &[u32],
    usage_min: Option<u32>,
    usage_max: Option<u32>,
) -> u32 {
    if let Some(min) = usage_min {
        let max = usage_max.unwrap_or(min);
        let v = min.saturating_add(field);
        return if v <= max { v } else { max };
    }
    if usages.is_empty() {
        return 0;
    }
    usages
        .get(field as usize)
        .copied()
        .unwrap_or_else(|| *usages.last().unwrap())
}

/// 按字节数把无符号原始值符号扩展为 i32 (HID Logical Minimum 可为负)。
fn sign_extend(raw: u32, size: usize) -> i32 {
    match size {
        0 => 0,
        1 => raw as u8 as i8 as i32,
        2 => raw as u16 as i16 as i32,
        4 => raw as i32,
        _ => raw as i32,
    }
}

/// ★v21.7: 计算原始 HID 按键在运行时会被 hash 成的 button_id —— 与 rawinput.rs
/// `hash_bit_positions` 完全一致 (FNV-1a 32 位, 依次喂 byte_idx / bit_idx, 最后
/// 拼上设备指纹高 32 位)。生成的映射名必须能被 `state::AppState::input_name_to_device`
/// 解析回同一 button_id, 否则映射永不可命中。
pub fn raw_hid_button_id(stable_device_id: u32, byte_idx: usize, bit_idx: u32) -> u64 {
    let mut hash = crate::util::fnv32::OFFSET_BASIS;
    hash = crate::util::fnv1a_hash_u32(hash, byte_idx as u32);
    hash = crate::util::fnv1a_hash_u32(hash, bit_idx);
    ((stable_device_id as u64) << 32) | (hash as u64)
}

/// 前缀 (GAMEPAD / JOYSTICK / HID)。
pub fn device_prefix(kind: &crate::state::DeviceType) -> &'static str {
    match kind {
        crate::state::DeviceType::Gamepad(_) => "GAMEPAD",
        crate::state::DeviceType::Joystick(_) => "JOYSTICK",
        crate::state::DeviceType::HidDevice { .. } => "GAMEPAD",
    }
}

/// 依据设备信息 + button_id 位置, 生成与手动捕获格式一致的触发键名。
/// 格式与 `state::InputDevice::Display` 对 GenericDevice 的写法保持一致,
/// 保证 `input_name_to_device` 能无损解析回同一 button_id。
///
/// * `serial` = 真实序列号 (非 Windows 实例 ID) 时用序列号, 否则用 `DEV{:08X}`。
pub fn format_raw_trigger_name(
    prefix: &str,
    vid: u16,
    pid: u16,
    serial: Option<&str>,
    stable_device_id_low32: u32,
    position: u32,
) -> String {
    let serial_real = serial
        .map(|s| !s.contains('&') && s.len() > 4)
        .unwrap_or(false);
    let dev = match (serial_real, serial) {
        (true, Some(s)) => s.to_string(),
        _ => format!("DEV{:08X}", stable_device_id_low32),
    };

    /* 位置编码复用 Display 的双分支: 高位=1 → 字节级; 否则 → "B<byte>.<bit>" */
    if position & 0x8000_0000 != 0 {
        format!(
            "{}_{:04X}_{:04X}_{}_B{}",
            prefix,
            vid,
            pid,
            dev,
            position & 0x7FFF_FFFF
        )
    } else {
        format!(
            "{}_{:04X}_{:04X}_{}_B{}.{}",
            prefix,
            vid,
            pid,
            dev,
            position >> 16,
            position & 0xFFFF
        )
    }
}

/// 兜底: 稳定设备 ID 的低 32 位 (与 button_id 高 32 位一致)。
pub fn stable_id_low32(stable_device_id: u64) -> u32 {
    stable_device_id as u32
}

/* ───────── ★v21.7 方案A(运行时主线): HidP 语义按键编码 ─────────
 *
 * 本机实测: 目标第三方手柄 (20BC:5159) 不支持 IOCTL_HID_GET_REPORT_DESCRIPTOR
 * (DeviceIoControl 恒返回 ERROR_INVALID_FUNCTION), 但 Windows 自带 HidP_* 能正常
 * 解析并读取按键语义 (HidP_GetUsages)。因此运行时走"语义"路线:
 *   按键触发键名 = GAMEPAD_<VID>_<PID>_<SER|DEV%08X>_H<usage>  (usage 即 HID 按钮号)
 * button_id 用 0x4000_0000 标记位 + usage 编码, 与 bit-hash (任意 32 位) / 字节级
 * (0x8000_0000|x) 两个既有命名空间区分 (bit30=1 且 bit31=0 → 二者都不占用)。
 */

/// 语义按键在 button_id 低 32 位里的标记 (bit31=0, bit30=1)。
pub const SEMANTIC_BUTTON_MARKER: u32 = 0x4000_0000;
/// 语义轴方向的标记 (bit31=0, bit30=0, bit29=1)。
pub const SEMANTIC_AXIS_MARKER: u32 = 0x2000_0000;

/// HID 按钮 usage → button_id 低 32 位。
pub fn semantic_button_position(usage: u32) -> u32 {
    SEMANTIC_BUTTON_MARKER | (usage & 0x3FFF_FFFF)
}

/// button_id 低 32 位 → HID 按钮 usage (非语义按键返回 None)。
pub fn semantic_button_usage(position: u32) -> Option<u32> {
    if position & 0xC000_0000 == SEMANTIC_BUTTON_MARKER {
        Some(position & 0x3FFF_FFFF)
    } else {
        None
    }
}

/// 语义轴方向: `direction` 0=左 1=右 2=上 3=下。
pub fn semantic_axis_position(axis_usage: u16, direction: u8) -> u32 {
    SEMANTIC_AXIS_MARKER | ((axis_usage as u32) << 8) | (direction as u32 & 0xFF)
}

/// button_id 低 32 位 → (轴 usage, 方向) (非语义轴返回 None)。
pub fn semantic_axis_decode(position: u32) -> Option<(u16, u8)> {
    if position & 0xE000_0000 == SEMANTIC_AXIS_MARKER {
        Some((((position >> 8) & 0xFFFF) as u16, (position & 0xFF) as u8))
    } else {
        None
    }
}

/// 语义按键触发键名 (与手动捕获同前缀/同设备段, 后缀用 `H<usage>`)。
pub fn format_semantic_button_name(
    prefix: &str,
    vid: u16,
    pid: u16,
    serial: Option<&str>,
    stable_device_id_low32: u32,
    usage: u32,
) -> String {
    format!(
        "{}_{:04X}_{:04X}_{}_H{}",
        prefix,
        vid,
        pid,
        device_tag(serial, stable_device_id_low32),
        usage
    )
}

/// 语义轴方向触发键名 (后缀 `A<usage>_<dir>`; dir: L/R/U/D)。
pub fn format_semantic_axis_name(
    prefix: &str,
    vid: u16,
    pid: u16,
    serial: Option<&str>,
    stable_device_id_low32: u32,
    axis_usage: u16,
    direction: u8,
) -> String {
    let dir = match direction {
        0 => 'L',
        1 => 'R',
        2 => 'U',
        _ => 'D',
    };
    format!(
        "{}_{:04X}_{:04X}_{}_A{}{}",
        prefix,
        vid,
        pid,
        device_tag(serial, stable_device_id_low32),
        axis_usage,
        dir
    )
}

/// 设备段: 真实序列号优先, 否则 `DEV{:08X}` (与 Display / 手动捕获一致)。
pub fn device_tag(serial: Option<&str>, stable_device_id_low32: u32) -> String {
    match serial {
        Some(s) if !s.contains('&') && s.len() > 4 => s.to_string(),
        _ => format!("DEV{:08X}", stable_device_id_low32),
    }
}

/// 轴方向语义名 (UI 展示与生成共用)。
pub fn axis_direction_label(direction: u8) -> &'static str {
    match direction {
        0 => "左",
        1 => "右",
        2 => "上",
        _ => "下",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 标准 12 键 + 4 轴手柄 (无 Report ID): 12 按钮各 1 bit, 4 bit 填充, 4 轴各 8 bit。
    fn descriptor_standard_gamepad() -> Vec<u8> {
        vec![
            0x05, 0x01, // Usage Page (Generic Desktop)
            0x09, 0x05, // Usage (Gamepad)
            0xA1, 0x01, // Collection (Application)
            0x15, 0x00, // Logical Min 0
            0x25, 0x01, // Logical Max 1
            0x75, 0x01, // Report Size 1
            0x95, 0x0C, // Report Count 12
            0x05, 0x09, // Usage Page (Button)
            0x19, 0x01, // Usage Min 1
            0x29, 0x0C, // Usage Max 12
            0x81, 0x02, // Input (Data,Var,Abs)
            0x75, 0x01, // Report Size 1
            0x95, 0x04, // Report Count 4
            0x81, 0x03, // Input (Const) 填充
            0x05, 0x01, // Usage Page (Generic Desktop)
            0x09, 0x30, // Usage X
            0x09, 0x31, // Usage Y
            0x09, 0x33, // Usage Rx
            0x09, 0x34, // Usage Ry
            0x15, 0x81, // Logical Min -127
            0x25, 0x7F, // Logical Max 127
            0x75, 0x08, // Report Size 8
            0x95, 0x04, // Report Count 4
            0x81, 0x02, // Input (Data,Var,Abs)
            0xC0, // End Collection
        ]
    }

    #[test]
    fn parses_standard_gamepad_buttons_and_axes() {
        let layout = parse_report_descriptor(&descriptor_standard_gamepad()).unwrap();
        assert!(!layout.uses_report_ids);
        assert_eq!(layout.buttons.len(), 12, "12 个按钮");
        for (i, b) in layout.buttons.iter().enumerate() {
            assert_eq!(b.usage, i as u32 + 1);
            assert_eq!(b.bit_offset, i as u32, "按钮 {i} 位偏移");
        }
        assert_eq!(layout.axes.len(), 4);
        assert_eq!(layout.axes[0].kind, HidAxisKind::X);
        assert_eq!(layout.axes[0].bit_offset, 16);
        assert_eq!(layout.axes[1].kind, HidAxisKind::Y);
        assert_eq!(layout.axes[1].bit_offset, 24);
        assert_eq!(layout.axes[2].kind, HidAxisKind::Rx);
        assert_eq!(layout.axes[2].bit_offset, 32);
        assert_eq!(layout.axes[3].kind, HidAxisKind::Ry);
        assert_eq!(layout.axes[3].bit_offset, 40);
        assert_eq!(layout.axes[0].logical_min, -127);
        assert_eq!(layout.axes[0].logical_max, 127);
        assert_eq!(layout.input_report_bits, 48);
        assert_eq!(layout.report_bytes(), 6);
        assert!(layout.has_left_stick());
    }

    #[test]
    fn trigger_threshold_uses_baseline_delta() {
        // 单极扳机 (静止≈0): 半量程以上算按下
        assert!(!trigger_pressed(Some(0), Some(0), 8));
        assert!(!trigger_pressed(Some(127), Some(0), 8));
        assert!(trigger_pressed(Some(128), Some(0), 8));
        assert!(trigger_pressed(Some(255), Some(0), 8));
        // 双极/居中轴 (静止=中点): 静止不按, 推到远端才算 —— 这是旧实现的假阳性来源
        assert!(!trigger_pressed(Some(128), Some(128), 8));
        assert!(!trigger_pressed(Some(100), Some(128), 8));
        assert!(trigger_pressed(Some(0), Some(128), 8));
        // 10 位轴
        assert!(trigger_pressed(Some(900), Some(0), 10));
        assert!(!trigger_pressed(Some(400), Some(0), 10));
        // 无基线 → 回落半量程; 无轴 → false
        assert!(trigger_pressed(Some(255), None, 8));
        assert!(!trigger_pressed(Some(255), Some(0), 0));
        assert!(!trigger_pressed(None, Some(0), 8));
    }

    #[test]
    fn standard_button_names_follow_convention() {
        let layout = parse_report_descriptor(&descriptor_standard_gamepad()).unwrap();
        let names: Vec<_> = layout
            .buttons
            .iter()
            .map(|b| layout.standard_button_name(b.usage))
            .collect();
        assert_eq!(names[0], Some("A"));
        assert_eq!(names[1], Some("B"));
        assert_eq!(names[2], Some("X"));
        assert_eq!(names[3], Some("Y"));
        assert_eq!(names[4], Some("LB"));
        assert_eq!(names[5], Some("RB"));
        assert_eq!(names[6], Some("LT"));
        assert_eq!(names[7], Some("RT"));
        assert_eq!(names[8], Some("Back"));
        assert_eq!(names[9], Some("Start"));
        assert_eq!(names[10], Some("L3"));
        assert_eq!(names[11], Some("R3"));
    }

    #[test]
    fn report_id_shifts_raw_byte_offset_by_one() {
        // 在按钮 Input 前插入 Report ID = 1
        let mut d = vec![
            0x05, 0x01, 0x09, 0x05, 0xA1, 0x01, 0x85, 0x01, // Report ID 1
            0x15, 0x00, 0x25, 0x01, 0x75, 0x01, 0x95, 0x0C, 0x05, 0x09, 0x19, 0x01, 0x29, 0x0C,
            0x81, 0x02, 0x75, 0x01, 0x95, 0x04, 0x81, 0x03, 0x05, 0x01, 0x09, 0x30, 0x09, 0x31,
            0x15, 0x81, 0x25, 0x7F, 0x75, 0x08, 0x95, 0x02, 0x81, 0x02, 0xC0,
        ];
        let layout = parse_report_descriptor(&d).unwrap();
        assert!(layout.uses_report_ids);
        assert_eq!(layout.buttons.len(), 12);
        assert_eq!(layout.buttons[0].report_id, 1);
        // 无 Report ID 时按钮 1 在字节 0; 有则整体后移 1 字节
        assert_eq!(layout.raw_byte_bit(layout.buttons[0].bit_offset), (1, 0));
        assert_eq!(layout.raw_byte_bit(layout.buttons[11].bit_offset), (2, 3));
        // 按钮在字节 1..2 —— 前 5 字节被跳过 → 不可检测 (诚实标注, 不生成假映射)
        assert!(!layout.button_is_detectable(&layout.buttons[0]));
        d.clear(); // 避免 unused_mut 警告
    }

    #[test]
    fn parses_hat_switch_and_z_axis() {
        let d = vec![
            0x05, 0x01, 0x09, 0x05, 0xA1, 0x01, 0x05, 0x01, // Usage Page Generic
            0x09, 0x30, // X
            0x15, 0x00, 0x26, 0xFF, 0x03, // Logical 0..1023
            0x75, 0x10, 0x95, 0x01, 0x81, 0x02, // Input 16-bit
            0x09, 0x39, // Hat switch
            0x15, 0x00, 0x25, 0x07, // Logical 0..7
            0x75, 0x04, 0x95, 0x01, 0x81, 0x42, // Input (Data,Var,Abs,Null state)
            0xC0,
        ];
        let layout = parse_report_descriptor(&d).unwrap();
        assert_eq!(layout.axes.len(), 2);
        assert_eq!(layout.axes[0].kind, HidAxisKind::X);
        assert_eq!(layout.axes[0].bit_size, 16);
        assert_eq!(layout.axes[0].logical_max, 1023);
        assert_eq!(layout.axes[1].kind, HidAxisKind::HatSwitch);
        assert_eq!(layout.axes[1].bit_offset, 16);
        assert_eq!(layout.axes[1].bit_size, 4);
    }

    #[test]
    fn push_pop_and_empty_descriptor() {
        assert_eq!(
            parse_report_descriptor(&[]),
            Err(HidParseError::Empty)
        );
        // Push 一个 usage_page=0x09, 再 Pop 回 0x01, 后续轴应被识别
        let d = vec![
            0x05, 0x09, 0xA4, // Usage Page Button + Push
            0x05, 0x01, 0xB4, // Usage Page Generic + Pop (回 0x09!)
            0x09, 0x30, 0x15, 0x00, 0x25, 0x7F, 0x75, 0x08, 0x95, 0x01, 0x81, 0x02,
        ];
        let layout = parse_report_descriptor(&d).unwrap();
        // Pop 后 usage_page 回到 0x09 (Button) → 0x30 被当成按钮而非 X 轴
        assert!(layout.axes.is_empty());
        assert_eq!(layout.buttons.len(), 1);
        assert_eq!(layout.buttons[0].usage, 0x30);
    }

    #[test]
    fn raw_trigger_name_round_trips_through_parser() {
        // 生成的名字必须能被 state::AppState::input_name_to_device 解析回同一 button_id
        let stable = 0xDEAD_BEEFu64;
        let low = stable_id_low32(stable);
        let (byte, bit) = (6usize, 2u32);
        let position = (raw_hid_button_id(low, byte, bit) & 0xFFFF_FFFF) as u32;
        let name = format_raw_trigger_name("GAMEPAD", 0x20BC, 0x5158, None, low, position);

        let device = crate::state::AppState::input_name_to_device(&name)
            .expect("生成的名字必须可解析");
        match device {
            crate::state::InputDevice::GenericDevice { button_id, .. } => {
                assert_eq!(button_id, raw_hid_button_id(low, byte, bit));
            }
            other => panic!("期望 GenericDevice, 得到 {other:?}"),
        }
    }

    #[test]
    fn raw_hid_button_id_matches_rawinput_hash_convention() {
        // 与 rawinput.rs `hash_bit_positions` 的算法一致: FNV32(byte) then FNV32(bit)
        let mut h = crate::util::fnv32::OFFSET_BASIS;
        h = crate::util::fnv1a_hash_u32(h, 6);
        h = crate::util::fnv1a_hash_u32(h, 2);
        assert_eq!(raw_hid_button_id(0x1234_5678, 6, 2), (0x1234_5678u64 << 32) | h as u64);
    }

    #[test]
    fn semantic_markers_do_not_collide_with_other_namespaces() {
        let bp = semantic_button_position(1);
        assert_eq!(semantic_button_usage(bp), Some(1));
        assert!(semantic_axis_decode(bp).is_none());

        let ap = semantic_axis_position(0x30, 2);
        assert_eq!(semantic_axis_decode(ap), Some((0x30, 2)));
        assert!(semantic_button_usage(ap).is_none());

        // 标记位: 按键 bit30=1/bit31=0; 轴 bit29=1/bit31,30=0
        assert_eq!(bp & 0xC000_0000, 0x4000_0000);
        assert_eq!(ap & 0xE000_0000, 0x2000_0000);
        // 字节级命名空间 (0x8000_0000|x, bit31=1) 与二者都不重叠
        assert_eq!(bp & 0x8000_0000, 0);
        assert_eq!(ap & 0x8000_0000, 0);
    }

    #[test]
    fn semantic_axis_name_round_trips_through_parser() {
        let name = format_semantic_axis_name("GAMEPAD", 0x20BC, 0x5159, None, 0x1234_5678, 0x30, 0);
        let dev = crate::state::AppState::input_name_to_device(&name).unwrap();
        match dev {
            crate::state::InputDevice::GenericDevice { button_id, .. } => {
                assert_eq!((button_id >> 32) as u32, 0x1234_5678);
                let pos = (button_id & 0xFFFF_FFFF) as u32;
                assert_eq!(semantic_axis_decode(pos), Some((0x30, 0)));
            }
            other => panic!("期望 GenericDevice, 得到 {other:?}"),
        }
    }
}
