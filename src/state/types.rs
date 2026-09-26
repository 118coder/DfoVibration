//! 类型字典: 设备/事件/映射/序列/HID 实况等跨模块共享类型 —— 原 state.rs 131-619, 2026-09-27 架构重构 B9 归位。
//! 经 mod.rs 的 pub use types::* 再导出, crate::state::X 路径保持不变。

use std::convert::Infallible;
use std::str::FromStr;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::time::Instant;

use smallvec::SmallVec;



use super::*;

/// HID device activation request information.
#[derive(Debug, Clone)]
pub struct HidActivationRequest {
    pub device_handle: isize,
    pub device_name: String,
    pub vid: u16,
    pub pid: u16,
    pub usage_page: u16,
    pub usage: u16,
}

/// Trait for dispatching input events to worker threads.
pub trait EventDispatcher: Send + Sync {
    fn dispatch(&self, event: InputEvent);
    /// Clear internal caches (called when configuration is reloaded)
    fn clear_cache(&self);
}

/// Marker value to identify simulated keyboard events.
pub const SIMULATED_EVENT_MARKER: usize = 0x4659;


/// Notification event types for user feedback.
#[allow(unused)]
#[derive(Debug, Clone)]
pub enum NotificationEvent {
    Info(String),
    Warning(String),
    Error(String),
}

/// Input device type for unified input handling.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum InputDevice {
    /// Keyboard input with virtual key code
    Keyboard(u32),
    /// Mouse button input
    Mouse(MouseButton),
    /// Key combination input (modifier keys + main key)
    /// Format: [modifier1, modifier2, ..., main_key]
    /// The last element is always the main key, others are modifiers
    KeyCombo(Vec<u32>),
    /// XInput gamepad combo
    /// Format: (device_type, button_ids)
    XInputCombo {
        device_type: DeviceType,
        button_ids: Vec<u32>,
    },
    /// Generic device input for gamepads, joysticks, and other HID devices
    /// Format: (device_type, button_id)
    GenericDevice {
        device_type: DeviceType,
        button_id: u64,
    },
}

impl std::fmt::Display for InputDevice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InputDevice::XInputCombo {
                device_type,
                button_ids,
            } => {
                let vid = match device_type {
                    DeviceType::Gamepad(vid) => *vid,
                    DeviceType::Joystick(vid) => *vid,
                    DeviceType::HidDevice { .. } => 0,
                };

                let prefix = match device_type {
                    DeviceType::Gamepad(_) => "GAMEPAD",
                    DeviceType::Joystick(_) => "JOYSTICK",
                    _ => "XINPUT",
                };

                write!(f, "{}_{:04X}_", prefix, vid)?;

                for (i, &input_id) in button_ids.iter().enumerate() {
                    if i > 0 {
                        write!(f, "+")?;
                    }
                    write!(
                        f,
                        "{}",
                        crate::xinput::XInputHandler::input_id_to_name(input_id)
                    )?;
                }

                Ok(())
            }
            InputDevice::GenericDevice {
                device_type,
                button_id,
            } => {
                // HID device format: [32-bit stable_device_id][32-bit position]
                let stable_device_id = (button_id >> 32) as u32;
                let position = (button_id & 0xFFFFFFFF) as u32;

                let display_info_opt =
                    crate::rawinput::get_device_display_info(stable_device_id as u64);

                let display_info = if let Some(info) = display_info_opt {
                    info
                } else {
                    let vid = match device_type {
                        DeviceType::Gamepad(vid) => *vid,
                        DeviceType::Joystick(vid) => *vid,
                        DeviceType::HidDevice { .. } => 0,
                    };
                    crate::rawinput::DeviceDisplayInfo {
                        vendor_id: vid,
                        product_id: 0,
                        serial_number: None,
                    }
                };

                let prefix = match device_type {
                    DeviceType::Gamepad(_) => "GAMEPAD",
                    DeviceType::Joystick(_) => "JOYSTICK",
                    DeviceType::HidDevice { usage_page, .. } => {
                        return if let Some(ref serial) = display_info.serial_number {
                            write!(
                                f,
                                "HID_{:04X}_{:04X}_{:04X}_{}",
                                usage_page, display_info.vendor_id, display_info.product_id, serial
                            )
                        } else {
                            write!(
                                f,
                                "HID_{:04X}_{:04X}_{:04X}_DEV{:08X}",
                                usage_page,
                                display_info.vendor_id,
                                display_info.product_id,
                                stable_device_id
                            )
                        };
                    }
                };

                // Format with VID/PID/Serial or VID/PID/DEV
                let dev_tag = crate::hid_layout::device_tag(
                    display_info.serial_number.as_deref(),
                    stable_device_id,
                );
                /* ★v21.7 语义按键/轴方向: HidP 路线 (第三方 HID 手柄一键标准布局) */
                if let Some(usage) = crate::hid_layout::semantic_button_usage(position) {
                    return write!(
                        f,
                        "{}_{:04X}_{:04X}_{}_H{}",
                        prefix, display_info.vendor_id, display_info.product_id, dev_tag, usage
                    );
                }
                if let Some((axis_usage, dir)) = crate::hid_layout::semantic_axis_decode(position) {
                    let d = match dir {
                        0 => 'L',
                        1 => 'R',
                        2 => 'U',
                        _ => 'D',
                    };
                    return write!(
                        f,
                        "{}_{:04X}_{:04X}_{}_A{}{}",
                        prefix, display_info.vendor_id, display_info.product_id, dev_tag, axis_usage, d
                    );
                }
                if let Some(ref serial) = display_info.serial_number {
                    // Has serial number: format with serial
                    if position & 0x80000000 != 0 {
                        // Byte-level
                        let byte_idx = position & 0x7FFFFFFF;
                        write!(
                            f,
                            "{}_{:04X}_{:04X}_{}_B{}",
                            prefix,
                            display_info.vendor_id,
                            display_info.product_id,
                            serial,
                            byte_idx
                        )
                    } else {
                        // Bit-level
                        let byte_idx = (position >> 16) as u16;
                        let bit_idx = (position & 0xFFFF) as u16;
                        write!(
                            f,
                            "{}_{:04X}_{:04X}_{}_B{}.{}",
                            prefix,
                            display_info.vendor_id,
                            display_info.product_id,
                            serial,
                            byte_idx,
                            bit_idx
                        )
                    }
                } else {
                    // No serial number: format with DEV prefix
                    if position & 0x80000000 != 0 {
                        // Byte-level
                        let byte_idx = position & 0x7FFFFFFF;
                        write!(
                            f,
                            "{}_{:04X}_{:04X}_DEV{:08X}_B{}",
                            prefix,
                            display_info.vendor_id,
                            display_info.product_id,
                            stable_device_id,
                            byte_idx
                        )
                    } else {
                        // Bit-level
                        let byte_idx = (position >> 16) as u16;
                        let bit_idx = (position & 0xFFFF) as u16;
                        write!(
                            f,
                            "{}_{:04X}_{:04X}_DEV{:08X}_B{}.{}",
                            prefix,
                            display_info.vendor_id,
                            display_info.product_id,
                            stable_device_id,
                            byte_idx,
                            bit_idx
                        )
                    }
                }
            }
            _ => write!(f, "{:?}", self),
        }
    }
}

/// Device type classification for generic input devices.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DeviceType {
    /// HID gamepad (Xbox, PlayStation, etc.)
    Gamepad(u16),
    /// Joystick device
    Joystick(u16),
    /// Custom HID device with usage page and usage
    HidDevice { usage_page: u16, usage: u16 },
}

/// HID input capture mode strategy.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize, Default,
)]
pub enum CaptureMode {
    /// Captures the most sustained input pattern (default)
    #[default]
    MostSustained,
    /// Adaptive scoring based on encoding type detection
    AdaptiveIntelligent,
    /// Selects pattern with most changed bits
    MaxChangedBits,
    /// Selects pattern with most set bits
    MaxSetBits,
    /// Selects the last stable frame
    LastStable,
    /// For Hat Switch devices (prioritizes numeric value)
    HatSwitchOptimized,
    /// For analog devices (prioritizes deviation magnitude)
    AnalogOptimized,
}

impl FromStr for CaptureMode {
    type Err = Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "MostSustained" => Ok(Self::MostSustained),
            "AdaptiveIntelligent" => Ok(Self::AdaptiveIntelligent),
            "MaxChangedBits" => Ok(Self::MaxChangedBits),
            "MaxSetBits" => Ok(Self::MaxSetBits),
            "LastStable" => Ok(Self::LastStable),
            "HatSwitchOptimized" => Ok(Self::HatSwitchOptimized),
            "AnalogOptimized" => Ok(Self::AnalogOptimized),
            _ => Ok(Self::default()),
        }
    }
}

impl CaptureMode {
    pub fn all_modes() -> &'static [CaptureMode] {
        &[
            CaptureMode::MostSustained,
            CaptureMode::AdaptiveIntelligent,
            CaptureMode::MaxChangedBits,
            CaptureMode::MaxSetBits,
            CaptureMode::LastStable,
            CaptureMode::HatSwitchOptimized,
            CaptureMode::AnalogOptimized,
        ]
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::MostSustained => "MostSustained",
            Self::AdaptiveIntelligent => "AdaptiveIntelligent",
            Self::MaxChangedBits => "MaxChangedBits",
            Self::MaxSetBits => "MaxSetBits",
            Self::LastStable => "LastStable",
            Self::HatSwitchOptimized => "HatSwitchOptimized",
            Self::AnalogOptimized => "AnalogOptimized",
        }
    }
}

/// Mouse button types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
    X1,
    X2,
}

/// Unified input event type for keyboard and mouse.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputEvent {
    Pressed(InputDevice),
    Released(InputDevice),
    /// ★v21.0 重推奔跑: 轴值从轻推区跳入重推区时派发 (仅勾选【奔跑】的映射)。
    /// 语义 = "模拟松开再按下" 的第二次敲击 → 游戏判定双击 → 奔跑。
    /// 处理方 (keyboard.rs) 负责补足首击时长 → 松开 → 停双击间隔 → 再按住。
    RunTap(InputDevice),
}

/// Mouse movement direction
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MouseMoveDirection {
    Up,
    Down,
    Left,
    Right,
    UpLeft,
    UpRight,
    DownLeft,
    DownRight,
}

/// Mouse scroll direction
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MouseScrollDirection {
    Up,
    Down,
}

/// Output action type for input mapping.
#[derive(Debug, Clone, PartialEq)]
pub enum OutputAction {
    /// Keyboard key output with scancode
    KeyboardKey(u16),
    /// Mouse button output
    MouseButton(MouseButton),
    /// Mouse movement output (direction, speed in pixels per move)
    MouseMove(MouseMoveDirection, i32),
    /// Mouse scroll output (direction, wheel delta)
    MouseScroll(MouseScrollDirection, i32),
    /// Key combination output (modifier scancodes + main key scancode)
    /// Format: [modifier1_scancode, modifier2_scancode, ..., main_key_scancode]
    /// Using Arc to avoid cloning on every key repeat
    KeyCombo(Arc<[u16]>),
    /// Multiple simultaneous actions for handling combined inputs
    /// Uses SmallVec with inline capacity of 4 to reduce allocations
    MultipleActions(Arc<SmallVec<[OutputAction; 4]>>),
    /// ★v24.31 序列控制键 (Toggle/Pause/Continue) —— worker 拦截, 永不被模拟
    SequenceControl(SequenceCtl),
}

/// ★v24.31 序列控制类型 (特殊映射键: KeySequenceToggle / KeySequencePause / KeySequenceContinue)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SequenceCtl {
    Toggle,
    Pause,
    Continue,
}

/// ★v24.31 序列的一步 (已解析成动作; actions 空 = 纯等待步)
#[derive(Debug, Clone)]
pub struct ResolvedStep {
    pub actions: SmallVec<[OutputAction; 2]>,
    pub hold_ms: u64,
}

/// ★v24.31 一条正在执行的序列 (按设备去重; 停止由控制/UI 置位)
pub struct SequenceRun {
    pub stop: std::sync::atomic::AtomicBool,
}

/// ★v24.31 按键录制状态 (钩子线程写, GUI 读走)
pub(super) struct KeyRecordState {
    pub(super) start: Instant,
    pub(super) events: Vec<crate::sequence::RecordedKey>,
}

/// Configuration for a single input mapping.
#[derive(Debug, Clone)]
pub struct InputMappingInfo {
    /// Target output action
    pub target_action: OutputAction,
    /// Repeat interval in milliseconds
    pub interval: u64,
    /// Event duration in milliseconds (not used for MouseMove)
    pub event_duration: u64,
    /// Enable turbo mode (auto-repeat)
    pub turbo_enabled: bool,
    /// Enable double-tap simulation on first press (e.g. DNF run)
    pub double_tap_enabled: bool,
    /// Gap between the two simulated taps in milliseconds
    pub double_tap_gap_ms: u64,
    /// ★v21.0 重推奔跑 (勾选后 turbo/double_tap 被压制, 改走"按住+重推补敲"语义)
    pub run_enabled: bool,
    /// 重推阈值: 满量程 32768 的百分比 (50-95)
    pub run_threshold: u8,
    /// ★v21.1 重推再检测: true = 重推时模拟完整双击序列 (松开→敲→松开→再按住)
    pub run_recheck: bool,
    /// ★v24.31 锁定 Lock (引擎侧已压制 奔跑/简易奔跑 的组合)
    pub lock_enabled: bool,
    /// ★v24.31 抬起映射: 触发键抬起时额外发送的动作 (None = 旧行为)
    pub release_action: Option<OutputAction>,
    /// ★v24.31 序列宏 (Some = 整条映射按序列执行; 连发/锁定/双击/奔跑全部停用)
    pub sequence: Option<Arc<[ResolvedStep]>>,
    /// ★v24.31 序列控制键 (Some = 本条是 KeySequenceToggle/Pause/Continue, 不注入)
    pub sequence_ctl: Option<SequenceCtl>,
}

/// ★v21.7b 第三方手柄实时状态 —— 供手柄映射页 SVG 热点"按下即亮"。
///
/// 由 RawInput 语义通道 (HidP) 每帧发布; GUI 只读。方向位用位掩码:
/// `ls`/`rs`: bit0=左 bit1=右 bit2=上 bit3=下; `dpad`: 1..8 顺时针从"上"起, 0=中。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LiveHidState {
    pub vid: u16,
    pub pid: u16,
    /// 标准按钮 usage 位图 (usage 1..31 → bit[usage])。
    pub buttons: u32,
    pub dpad: u8,
    pub ls: u8,
    pub rs: u8,
    /// ★v22.1: 左右扳机 (模拟轴 Z/Rz 超阈值) —— 非标准手柄的 LT/RT 多为轴而非按钮
    pub lt: bool,
    pub rt: bool,
    /// ★v22.3: 当前"原始报文位组合"的哈希低 32 位 (0 = 无按键)。
    /// 与连发映射同一套编码 —— 非标准手柄的按键/扳机用它识别最可靠 (白名单通道外)。
    pub raw_position: u32,
}

/// ★v21.7: 预设切换键绑定 —— GUI 在配置变化时写入, 输入线程 (XInput/RawInput) 每帧检测。
///
/// 键盘切换键仍由 GUI 侧 `GetAsyncKeyState` 轮询 (见 gui/main_window.rs), 这里只承载
/// **手柄**类绑定 (XInput 组合 / 原始 HID 设备), 因为它们的状态只有输入线程实时掌握。
#[derive(Debug, Clone)]
pub struct PresetSwitchBinding {
    /// 目标预设名 (切换时按名查找)
    pub preset_name: String,
    /// 触发设备 (XInputCombo 或 GenericDevice; 键盘绑定不入此表)
    pub device: InputDevice,
}

/// Cache for switch key detection with lock-free fast paths
pub struct SwitchKeyCache {    pub keyboard_vk: AtomicU32,
    pub xinput_button_mask: AtomicU32,
    pub xinput_device_hash: AtomicU32,
    pub generic_button_id: AtomicU64,
    pub full_device: RwLock<Option<InputDevice>>,
}

impl SwitchKeyCache {
    #[inline(always)]
    pub(super) const fn new() -> Self {
        Self {
            keyboard_vk: AtomicU32::new(0),
            xinput_button_mask: AtomicU32::new(0),
            xinput_device_hash: AtomicU32::new(0),
            generic_button_id: AtomicU64::new(0),
            full_device: RwLock::new(None),
        }
    }

    #[inline(always)]
    pub(super) fn clear(&self) {
        self.keyboard_vk.store(0, Ordering::Relaxed);
        self.xinput_button_mask.store(0, Ordering::Relaxed);
        self.xinput_device_hash.store(0, Ordering::Relaxed);
        self.generic_button_id.store(0, Ordering::Relaxed);
        *util::write_guard(&self.full_device) = None;
    }
}
