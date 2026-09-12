//! Raw Input API integration for HID devices.
//!
//! Provides support for gamepads, joysticks, and other HID controllers through
//! the Windows Raw Input API. Implements single-shot triggering where each
//! input event generates one output action.
//!
//! ## Cache Architecture
//!
//! Uses a three-tier caching system for device information:
//!
//! 1. **Thread-local cache** (`LAST_DEVICE_CACHE`)
//!    - Stores the most recently accessed device per thread
//!    - Caches most recently accessed device per thread
//!    - Invalidated on device removal events
//!
//! 2. **Global device cache** (`device_cache`)
//!    - Lock-free concurrent HashMap for all connected devices
//!    - Shared across all threads
//!    - Invalidated on device removal events
//!
//! 3. **Windows API** (`GetRawInputDeviceInfoW`)
//!    - Authoritative source for device information
//!    - Only queried on cache misses or after invalidation
//!
//! ## Cache Management
//!
//! Device caches are automatically cleaned up when devices are disconnected.
//! The `WM_INPUT_DEVICE_CHANGE` removal event triggers cleanup of all associated
//! cache entries, including device info, HID states, and capture states.

use smallvec::SmallVec;
use std::cell::UnsafeCell;
use std::sync::{Arc, OnceLock, atomic::Ordering};
use std::time::Instant;
use windows::Win32::Foundation::{GetLastError, HANDLE, HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::PCWSTR;

use crate::state::{AppState, DeviceType, InputDevice, InputEvent};
use crate::util::{
    fnv1a_hash_bytes, fnv1a_hash_u32, fnv1a_hash_u64, fnv32, fnv64, likely, unlikely,
};

/// Number of buffers in the thread-local pool.
const BUFFER_POOL_SIZE: usize = 8;
/// Maximum buffer size before falling back to heap allocation.
const MAX_BUFFER_SIZE: usize = 256;

/// HID usage page for generic desktop controls.
const HID_USAGE_PAGE_GENERIC: u16 = 0x01;
/// HID usage page for game controls (many gamepads/joysticks/wheels report here).
const HID_USAGE_PAGE_GAME_CTRL: u16 = 0x05;
/// HID usage ID for 3D game controller devices (Game Controls page).
const HID_USAGE_3D_GAME: u16 = 0x01;
/// HID usage ID for gamepad devices.
const HID_USAGE_GAMEPAD: u16 = 0x05;
/// HID usage ID for joystick devices.
const HID_USAGE_JOYSTICK: u16 = 0x04;
/// HID usage ID for multi-axis controllers.
const HID_USAGE_MULTI_AXIS: u16 = 0x08;

/// Minimum valid HID report size in bytes.
/// (一些国产手柄报文只有 8 字节, 10 会误杀, 放宽到 8 以兼容)
const MIN_HID_DATA_SIZE: usize = 8;

/// Skip first bytes of HID report during processing.
const SKIP_BYTES: usize = 5;

/// Maximum capture frames stored per device.
const DEVICE_CAPTURE_FRAMES: usize = 32;

/// Size of each frame buffer in bytes.
const FRAME_BUFFER_SIZE: usize = 256;

thread_local! {
    /// Thread-local buffer pool for Raw Input data.
    static BUFFER_POOL: UnsafeCell<RingBufferPool> = const { UnsafeCell::new(RingBufferPool::new()) };
    /// Thread-local cache for most recently accessed device.
    static LAST_DEVICE_CACHE: UnsafeCell<Option<(isize, CachedDeviceInfo)>> = const { UnsafeCell::new(None) };
    /// Thread-local HID activation data pool.
    static HID_DATA_POOL: UnsafeCell<HidDataPool> = const { UnsafeCell::new(HidDataPool::new()) };
}

/// Ring buffer pool for reusing Raw Input data buffers.
struct RingBufferPool {
    buffers: [Vec<u8>; BUFFER_POOL_SIZE],
    current: usize,
}

impl RingBufferPool {
    const fn new() -> Self {
        const EMPTY_VEC: Vec<u8> = Vec::new();
        Self {
            buffers: [EMPTY_VEC; BUFFER_POOL_SIZE],
            current: 0,
        }
    }

    /// Returns the next available buffer, expanding if necessary.
    #[inline]
    fn get_buffer(&mut self, size: usize) -> &mut Vec<u8> {
        let idx = self.current;
        self.current = (self.current + 1) % BUFFER_POOL_SIZE;

        let buffer = &mut self.buffers[idx];
        buffer.clear();
        if buffer.capacity() < size {
            buffer.reserve(size.saturating_sub(buffer.capacity()));
        }
        buffer
    }
}

/// HID data memory pool for activation process.
const HID_DATA_POOL_SIZE: usize = 4; // Small pool for activation (typically 1-2 devices)

struct HidDataPool {
    buffers: [Vec<u8>; HID_DATA_POOL_SIZE],
    current: usize,
}

impl HidDataPool {
    const fn new() -> Self {
        const EMPTY_VEC: Vec<u8> = Vec::new();
        Self {
            buffers: [EMPTY_VEC; HID_DATA_POOL_SIZE],
            current: 0,
        }
    }

    /// Returns pooled Vec<u8> with data copied.
    #[inline]
    fn copy_to_vec(&mut self, data: &[u8]) -> Vec<u8> {
        let idx = self.current;
        self.current = (self.current + 1) % HID_DATA_POOL_SIZE;

        let buffer = &mut self.buffers[idx];
        buffer.clear();
        if buffer.capacity() < data.len() {
            buffer.reserve(data.len().saturating_sub(buffer.capacity()));
        }
        buffer.extend_from_slice(data);
        buffer.clone()
    }
}

/// Global Raw Input handler instance.
static RAW_INPUT_HANDLER: OnceLock<RawInputHandler> = OnceLock::new();

/// Global cache for device display information.
static DEVICE_DISPLAY_INFO: OnceLock<scc::HashMap<u64, DeviceDisplayInfo>> = OnceLock::new();

/// Display information for HID devices.
#[derive(Debug, Clone)]
pub struct DeviceDisplayInfo {
    pub vendor_id: u16,
    pub product_id: u16,
    pub serial_number: Option<String>,
}

/// Cached device information
#[derive(Debug, Clone)]
struct CachedDeviceInfo {
    device_type: DeviceType,
    vendor_id: u16,
    product_id: u16,
    usage_page: u16,
    usage: u16,
    serial_number: Option<String>,
}

/// Capture state for a single device during GUI button capture.
/// Tracks frame timestamps to calculate true sustained duration.
#[derive(Debug, Clone, Copy)]
struct DeviceCaptureState {
    /// Pre-allocated inline storage for captured frames (32 frames × 256 bytes)
    frames: [FrameRecord; DEVICE_CAPTURE_FRAMES],
    /// Number of frames currently stored
    frame_count: u8,
}

#[derive(Debug, Clone, Copy)]
struct FrameRecord {
    /// Frame data buffer
    data: [u8; FRAME_BUFFER_SIZE],
    /// Actual frame length
    len: u16,
    /// Timestamp when each frame was received (in milliseconds since epoch)
    timestamp: u64,
}

impl FrameRecord {
    #[inline(always)]
    fn new() -> Self {
        Self {
            data: [0; FRAME_BUFFER_SIZE],
            len: 0,
            timestamp: 0,
        }
    }
}

impl DeviceCaptureState {
    #[inline(always)]
    fn new() -> Self {
        Self {
            frames: [FrameRecord::new(); DEVICE_CAPTURE_FRAMES],
            frame_count: 0,
        }
    }

    #[inline(always)]
    fn with_frame(data: &[u8], timestamp_ms: u64) -> Self {
        let mut state = Self::new();
        let len = data.len().min(FRAME_BUFFER_SIZE);
        let idx = 0;

        let frame_record = &mut state.frames[idx];
        frame_record.data[..len].copy_from_slice(&data[..len]);
        frame_record.len = len as u16;
        frame_record.timestamp = timestamp_ms;
        state.frame_count = 1;
        state
    }

    /// Adds a captured frame with timestamp.
    #[inline(always)]
    fn add_frame(&mut self, data: &[u8], timestamp_ms: u64) {
        if unlikely(self.frame_count >= DEVICE_CAPTURE_FRAMES as u8) {
            return;
        }

        let len = data.len().min(FRAME_BUFFER_SIZE);
        let idx = self.frame_count as usize;
        let data = &data[..len];

        if idx > 0 {
            let last_idx = idx - 1;
            let last_frame_record = &mut self.frames[last_idx];
            let last_frame = &last_frame_record.data[..last_frame_record.len as usize];
            if Self::is_equal_fast(last_frame, data) {
                return;
            }
        }

        let frame_record = &mut self.frames[idx];
        frame_record.data[..len].copy_from_slice(data);
        frame_record.len = len as u16;
        frame_record.timestamp = timestamp_ms;
        self.frame_count += 1;
    }

    /// Returns the frame with the longest sustained duration.
    ///
    /// `now`: current timestamp in milliseconds.
    /// Duration of a stable segment is measured from its first frame's timestamp to:
    ///   - the timestamp of the first different frame that follows, OR
    ///   - `now` if it is the final segment.
    #[inline(always)]
    fn get_most_sustained_frame(&self, now: u64) -> Option<&[u8]> {
        if unlikely(self.frame_count == 0) {
            return None;
        }

        if self.frame_count == 1 {
            let len = self.frames[0].len as usize;
            return Some(&self.frames[0].data[..len]);
        }

        let mut best_idx = 0usize;
        let mut max_duration = 0u64;

        let mut i = 0;
        while i < self.frame_count as usize {
            let seg_start_time = self.frames[i].timestamp;
            let seg_frame = &self.frames[i].data[..self.frames[i].len as usize];

            // Extend segment as far as frames are equal
            let mut j = i;
            while j + 1 < self.frame_count as usize {
                let next_frame = &self.frames[j + 1].data[..self.frames[j + 1].len as usize];
                if Self::is_equal_fast(seg_frame, next_frame) {
                    j += 1;
                } else {
                    break;
                }
            }

            // Compute segment duration
            let seg_end_time = if j + 1 < self.frame_count as usize {
                // Next different frame exists → segment ends at its timestamp
                self.frames[j + 1].timestamp
            } else {
                // Last segment → ends at current time
                now
            };

            let duration = seg_end_time.saturating_sub(seg_start_time);
            if duration > max_duration {
                max_duration = duration;
                best_idx = i; // representative: first frame of the longest segment
            }

            // Jump to next distinct segment
            i = j + 1;
        }

        let len = self.frames[best_idx].len as usize;
        Some(&self.frames[best_idx].data[..len])
    }

    /// Fast equality check for byte slices with AVX2 optimization.
    #[inline(always)]
    fn is_equal_fast(a: &[u8], b: &[u8]) -> bool {
        if unlikely(a.len() != b.len()) {
            return false;
        }

        #[cfg(all(target_arch = "x86_64", target_feature = "avx2"))]
        {
            return Self::is_equal_avx2(a, b);
        }

        #[cfg(not(all(target_arch = "x86_64", target_feature = "avx2")))]
        {
            a == b
        }
    }

    #[cfg(all(target_arch = "x86_64", target_feature = "avx2"))]
    #[inline(always)]
    fn is_equal_avx2(a: &[u8], b: &[u8]) -> bool {
        use std::arch::x86_64::*;

        let len = a.len();
        let mut offset = 0;

        unsafe {
            // Process 32-byte chunks with AVX2
            while offset + 32 <= len {
                let va = _mm256_loadu_si256(a.as_ptr().add(offset) as *const __m256i);
                let vb = _mm256_loadu_si256(b.as_ptr().add(offset) as *const __m256i);
                let cmp = _mm256_cmpeq_epi8(va, vb);
                let mask = _mm256_movemask_epi8(cmp);

                if mask != -1 {
                    return false;
                }
                offset += 32;
            }
        }

        // Process remaining bytes
        a[offset..].iter().zip(&b[offset..]).all(|(x, y)| x == y)
    }

    /// Selects the best frame based on the specified capture mode.
    fn get_best_frame(&self, baseline: &[u8], mode: crate::state::CaptureMode) -> Option<&[u8]> {
        use crate::state::CaptureMode;

        if unlikely(self.frame_count == 0) {
            return None;
        }

        match mode {
            CaptureMode::MostSustained => self.get_most_sustained_frame(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_millis() as u64,
            ),
            CaptureMode::AdaptiveIntelligent => self.get_adaptive_intelligent_frame(baseline),
            CaptureMode::MaxChangedBits => self.get_max_changed_bits_frame(baseline),
            CaptureMode::MaxSetBits => self.get_max_set_bits_frame(),
            CaptureMode::LastStable => self.get_last_stable_frame(baseline),
            CaptureMode::HatSwitchOptimized => self.get_hat_switch_optimized_frame(baseline),
            CaptureMode::AnalogOptimized => self.get_analog_optimized_frame(baseline),
        }
    }

    /// Max Changed Bits: Selects frame with most bits changed from baseline.
    #[inline(always)]
    fn get_max_changed_bits_frame(&self, baseline: &[u8]) -> Option<&[u8]> {
        if self.frame_count == 0 {
            return None;
        }

        let mut max_changed = 0u32;
        let mut best_idx = 0usize;

        for i in 0..self.frame_count as usize {
            let frame_record = &self.frames[i];
            let len = frame_record.len as usize;
            let frame = &frame_record.data[..len];

            let changed = Self::count_changed_bits(frame, baseline);
            if changed > max_changed {
                max_changed = changed;
                best_idx = i;
            }
        }

        let len = self.frames[best_idx].len as usize;
        Some(&self.frames[best_idx].data[..len])
    }

    /// Max Set Bits: Selects frame with most bits set to 1.
    #[inline(always)]
    fn get_max_set_bits_frame(&self) -> Option<&[u8]> {
        if self.frame_count == 0 {
            return None;
        }

        let mut max_set = 0u32;
        let mut best_idx = 0usize;

        for i in 0..self.frame_count as usize {
            let frame_record = &self.frames[i];
            let len = frame_record.len as usize;
            let frame = &frame_record.data[..len];

            let set_count: u32 = frame[SKIP_BYTES..].iter().map(|b| b.count_ones()).sum();

            if set_count > max_set {
                max_set = set_count;
                best_idx = i;
            }
        }

        let len = self.frames[best_idx].len as usize;
        Some(&self.frames[best_idx].data[..len])
    }

    /// Last Stable: Finds last frame that's significantly different from baseline.
    #[inline(always)]
    fn get_last_stable_frame(&self, baseline: &[u8]) -> Option<&[u8]> {
        if self.frame_count == 0 {
            return None;
        }

        for i in (0..self.frame_count as usize).rev() {
            let frame_record = &self.frames[i];
            let len = frame_record.len as usize;
            let frame = &frame_record.data[..len];

            let changed = Self::count_changed_bits(frame, baseline);
            if changed > 0 {
                return Some(frame);
            }
        }

        let idx = (self.frame_count - 1) as usize;
        let len = self.frames[idx].len as usize;
        Some(&self.frames[idx].data[..len])
    }

    /// Hat Switch Optimized: Prioritizes numeric deviation over bit count.
    #[inline(always)]
    fn get_hat_switch_optimized_frame(&self, baseline: &[u8]) -> Option<&[u8]> {
        if self.frame_count == 0 {
            return None;
        }

        let mut max_score = 0u32;
        let mut best_idx = 0usize;

        for i in 0..self.frame_count as usize {
            let frame_record = &self.frames[i];
            let len = frame_record.len as usize;
            let frame = &frame_record.data[..len];

            let mut score = 0u32;
            for byte_idx in SKIP_BYTES..len.min(baseline.len()) {
                let val = frame[byte_idx];
                let base = baseline[byte_idx];
                if val != base {
                    let numeric_diff = val.abs_diff(base) as u32;
                    score += numeric_diff * 100;
                }
            }

            if score > max_score {
                max_score = score;
                best_idx = i;
            }
        }

        let len = self.frames[best_idx].len as usize;
        Some(&self.frames[best_idx].data[..len])
    }

    /// Analog Optimized: Prioritizes magnitude of deviation.
    #[inline(always)]
    fn get_analog_optimized_frame(&self, baseline: &[u8]) -> Option<&[u8]> {
        if self.frame_count == 0 {
            return None;
        }

        let mut max_deviation = 0u32;
        let mut best_idx = 0usize;

        for i in 0..self.frame_count as usize {
            let frame_record = &self.frames[i];
            let len = frame_record.len as usize;
            let frame = &frame_record.data[..len];

            let mut deviation = 0u32;
            for byte_idx in SKIP_BYTES..len.min(baseline.len()) {
                let val = frame[byte_idx];
                let base = baseline[byte_idx];
                deviation += val.abs_diff(base) as u32;
            }

            if deviation > max_deviation {
                max_deviation = deviation;
                best_idx = i;
            }
        }

        let len = self.frames[best_idx].len as usize;
        Some(&self.frames[best_idx].data[..len])
    }

    /// Adaptive Intelligent: Uses smart scoring based on encoding detection.
    #[inline(always)]
    fn get_adaptive_intelligent_frame(&self, baseline: &[u8]) -> Option<&[u8]> {
        if self.frame_count == 0 {
            return None;
        }
        let mut max_score = 0u32;
        let mut best_idx = 0usize;

        for i in 0..self.frame_count as usize {
            let frame_record = &self.frames[i];
            let len = frame_record.len as usize;
            let frame = &frame_record.data[..len];

            let mut score = 0u32;
            for byte_idx in SKIP_BYTES..len.min(baseline.len()) {
                let val = frame[byte_idx];
                let base = baseline[byte_idx];
                if val != base {
                    let numeric_diff = val.abs_diff(base) as u32;
                    let hamming_dist = (val ^ base).count_ones();

                    // Adaptive weighting based on change pattern
                    if numeric_diff <= 16 && hamming_dist >= 2 {
                        // Likely bitmask: prioritize Hamming distance
                        score += hamming_dist * 150;
                    } else if numeric_diff > 32 {
                        // Likely analog: prioritize numeric diff
                        score += numeric_diff * 100;
                    } else {
                        // Mixed: use both
                        score += numeric_diff * 80 + hamming_dist * 80;
                    }
                }
            }

            if score > max_score {
                max_score = score;
                best_idx = i;
            }
        }

        let len = self.frames[best_idx].len as usize;
        Some(&self.frames[best_idx].data[..len])
    }

    /// Helper: count changed bits between data and baseline.
    #[inline(always)]
    fn count_changed_bits(data: &[u8], baseline: &[u8]) -> u32 {
        // 绝对稳定: baseline 可能来自 Config.toml(用户可写)/异常设备, 长度不足
        // SKIP_BYTES 时切片会 panic(在 FFI 回调内 panic 是未定义行为) —— 直接视为无变化。
        if unlikely(data.len() <= SKIP_BYTES || baseline.len() <= SKIP_BYTES) {
            return 0;
        }
        data[SKIP_BYTES..]
            .iter()
            .zip(&baseline[SKIP_BYTES..])
            .map(|(d, b)| (d ^ b).count_ones())
            .sum()
    }
}

/// HID device state for button change detection.
#[derive(Debug, Clone)]
struct DeviceHidState {
    /// Whether baseline is established (hot field, placed first)
    baseline_ready: bool,
    /// Baseline HID data (idle state with no buttons pressed)
    baseline_data: Vec<u8>,
    /// Last received HID data for change detection
    last_data: Vec<u8>,
    /// Last update timestamp
    last_update: Instant,
    /// Last generated button_id (for proper release tracking)
    last_button_id: Option<u64>,
}

impl DeviceHidState {
    #[inline]
    fn with_baseline(baseline: Vec<u8>) -> Self {
        Self {
            baseline_ready: true,
            baseline_data: baseline.clone(),
            last_data: baseline,
            last_update: Instant::now(),
            last_button_id: None,
        }
    }
}

/// ★v21.7 方案A: 语义翻译状态 (每设备) —— preparsed 数据 + 上一帧按钮 + 轴规格。
struct SemanticDeviceState {
    /// `RIDI_PREPARSEDDATA` 拷贝 (HidP_* 直接吃这块内存; 保持存活即可, 无需设备句柄)。
    preparsed: Vec<u8>,
    /// 上一帧按下的 HidP usage 列表 (边缘检测用)。
    last_usages: SmallVec<[u16; 16]>,
    /// page 0x01 轴规格: (usage, 位宽)。用于判定摇杆方向 (与空闲基线比较)。
    axis_specs: Vec<(u16, u8)>,
    /// 上一帧方向位: (十字键4位, 左摇杆4位, 右摇杆4位) —— 边缘检测用。
    prev_dirs: (u8, u8, u8),
    /// ★v22.1: 上一帧扳机态 (LT=Z 轴超阈值, RT=Rz 轴超阈值)。
    prev_triggers: (bool, bool),
}

/// Handler for Raw Input API messages from HID devices.
pub struct RawInputHandler {
    state: Arc<AppState>,
    /// Lock-free cache for device information.
    device_cache: scc::HashMap<isize, CachedDeviceInfo>,
    /// Capture state tracking during GUI button capture mode (lock-free).
    capture_states: scc::HashMap<isize, DeviceCaptureState>,
    /// Device HID state tracking for button change detection (lock-free).
    device_states: scc::HashMap<isize, DeviceHidState>,
    /// Config baselines keyed by stable device ID (hash of VID:PID:Serial).
    config_baselines: scc::HashMap<u64, Vec<u8>>,
    /// ★v21.7 方案A: 语义翻译状态 (存在语义触发键或正在实时识别时建立)。
    semantic_states: scc::HashMap<isize, SemanticDeviceState>,
    /// Device ownership manager.
    ownership: crate::input_ownership::DeviceOwnership,
}

impl RawInputHandler {
    /// Removes cached data for a specific device.
    ///
    /// Called when a device is disconnected to free associated resources.
    fn remove_device_caches(&self, handle_key: isize) {
        if let Some(device_info) = self.device_cache.read_sync(&handle_key, |_, v| v.clone()) {
            let vid_pid = (device_info.vendor_id, device_info.product_id);
            if let Some(owner) = self.ownership.get_owner(vid_pid)
                && matches!(owner, crate::input_ownership::InputSource::RawInput(_))
            {
                self.ownership.release_device(vid_pid);
            }
        }

        self.device_cache.remove_sync(&handle_key);
        self.device_states.remove_sync(&handle_key);
        self.capture_states.remove_sync(&handle_key);
        self.semantic_states.remove_sync(&handle_key);

        // Clear thread-local cache if it references this device
        LAST_DEVICE_CACHE.with(|cache| unsafe {
            if let Some((cached_handle, _)) = *cache.get()
                && cached_handle == handle_key
            {
                *cache.get() = None;
            }
        });
    }

    /// Resets all HID device states to baseline (idle state).
    /// Called when entering capture mode to ensure clean state detection.
    #[inline]
    pub fn reset_device_states_to_baseline(&self) {
        self.device_states.retain_sync(|_handle, state| {
            if state.baseline_ready {
                // 自适应拷贝: 报文长度漂移时不再 panic(基线 15B vs 运行 45B
                // 曾触发 copy_from_slice panic, 每帧靠 FFI 兜底丢输入)
                if state.last_data.len() == state.baseline_data.len() {
                    state.last_data.copy_from_slice(&state.baseline_data);
                } else {
                    state.last_data.clear();
                    state.last_data.extend_from_slice(&state.baseline_data);
                }
            }
            true // Keep all entries
        });
    }
}

/// Window class name for the Raw Input message-only window.
const RAWINPUT_WINDOW_CLASS: &str = "SorahkRawInputWindow";

/// Handle to the Raw Input processing thread.
pub struct RawInputThread {
    _handle: std::thread::JoinHandle<()>,
}

impl RawInputHandler {
    /// Creates a new Raw Input handler and registers HID devices.
    fn new(
        hwnd: HWND,
        state: Arc<AppState>,
        hid_baselines: Vec<crate::config::HidDeviceBaseline>,
        ownership: crate::input_ownership::DeviceOwnership,
    ) -> anyhow::Result<Self> {
        Self::register_devices(hwnd)?;

        // Load baselines into lock-free HashMap
        let config_baselines = scc::HashMap::new();
        for baseline in hid_baselines {
            // Parse device_id to extract VID, PID, Serial and compute hash
            if let Some((vid, pid, serial)) = Self::parse_device_id(&baseline.device_id) {
                let stable_id = if let Some(ref serial) = serial {
                    const SERIAL_FORBIDDEN_CHARS: [char; 3] = ['&', ':', '.'];
                    let is_real_serial = serial.len() > 4
                        && !serial.chars().any(|c| SERIAL_FORBIDDEN_CHARS.contains(&c));

                    if is_real_serial {
                        Self::hash_vid_pid_serial(vid, pid, serial)
                    } else {
                        Self::hash_vid_pid(vid, pid)
                    }
                } else {
                    Self::hash_vid_pid(vid, pid)
                };
                let _ = config_baselines.insert_sync(stable_id, baseline.baseline_data);
            }
        }

        Ok(Self {
            state,
            device_cache: scc::HashMap::new(),
            capture_states: scc::HashMap::new(),
            device_states: scc::HashMap::new(),
            config_baselines,
            semantic_states: scc::HashMap::new(),
            ownership,
        })
    }

    /// Starts the Raw Input handler in a dedicated thread.
    pub fn start_thread(
        state: Arc<AppState>,
        hid_baselines: Vec<crate::config::HidDeviceBaseline>,
        ownership: crate::input_ownership::DeviceOwnership,
    ) -> RawInputThread {
        let handle = std::thread::Builder::new()
            .name("rawinput_thread".to_string())
            .spawn(move || {
                if let Err(e) = Self::run_message_loop(state, hid_baselines, ownership) {
                    eprintln!("Raw Input thread error: {}", e);
                }
            })
            .unwrap_or_else(|e| {
                // 绝对稳定: RawInput 线程创建失败不 panic(否则 abort 杀进程),
                // 记日志后仍可继续运行(键盘/鼠标/XInput 通道不受影响)。
                crate::util::crash_log("SPAWN_RAWINPUT", &e.to_string());
                std::thread::spawn(|| {}) // 占位句柄, 线程永不启动, 仅保结构完整
            });

        RawInputThread { _handle: handle }
    }

    /// Runs the Windows message loop for Raw Input processing.
    fn run_message_loop(
        state: Arc<AppState>,
        hid_baselines: Vec<crate::config::HidDeviceBaseline>,
        ownership: crate::input_ownership::DeviceOwnership,
    ) -> anyhow::Result<()> {
        unsafe {
            let class_name = Self::to_wstring(RAWINPUT_WINDOW_CLASS);
            let h_instance = GetModuleHandleW(None)?;

            let wc = WNDCLASSW {
                lpfnWndProc: Some(Self::window_proc),
                hInstance: HINSTANCE(h_instance.0),
                lpszClassName: PCWSTR(class_name.as_ptr()),
                ..Default::default()
            };

            if RegisterClassW(&wc) == 0 {
                let last_error = GetLastError();
                if last_error.0 != 1410 {
                    return Err(anyhow::anyhow!(
                        "Failed to register window class: {:?}",
                        last_error
                    ));
                }
            }

            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                PCWSTR(class_name.as_ptr()),
                windows::core::w!("Sorahk Raw Input Window"),
                WINDOW_STYLE(0),
                0,
                0,
                0,
                0,
                Some(HWND_MESSAGE),
                None,
                Some(HINSTANCE(h_instance.0)),
                None,
            )?;

            let handler = Self::new(hwnd, state, hid_baselines, ownership)?;
            let _ = RAW_INPUT_HANDLER.set(handler);

            let mut msg = MSG::default();
            loop {
                let result = GetMessageW(&mut msg, None, 0, 0);

                if result.0 == 0 || result.0 == -1 {
                    break;
                }

                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }

            let _ = DestroyWindow(hwnd);
            UnregisterClassW(PCWSTR(class_name.as_ptr()), Some(HINSTANCE(h_instance.0)))?;
        }

        Ok(())
    }

    /// Window procedure for Raw Input messages
    #[allow(non_snake_case)]
    unsafe extern "system" fn window_proc(
        hwnd: HWND,
        msg: u32,
        w_param: WPARAM,
        l_param: LPARAM,
    ) -> LRESULT {
        const WM_INPUT_DEVICE_CHANGE: u32 = 0x00FE;
        const GIDC_ARRIVAL: usize = 1;
        const GIDC_REMOVAL: usize = 2;

        // 绝对稳定: FFI 回调(被 Windows 直接调用)内绝不 unwind ——
        // panic 跨 FFI 边界是未定义行为(panic=unwind 配置下会崩),
        // 这里把整个处理包进 catch_unwind, panic 时返回默认窗口处理。
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            match msg {
                WM_INPUT => unsafe {
                    if let Some(handler) = RAW_INPUT_HANDLER.get() {
                        handler.handle_raw_input(l_param);
                    }
                    DefWindowProcW(hwnd, msg, w_param, l_param)
                },
                WM_INPUT_DEVICE_CHANGE => unsafe {
                    match w_param.0 {
                        GIDC_REMOVAL => {
                            // Device disconnected - remove its caches
                            // l_param contains the device handle as isize
                            let handle_key = l_param.0;

                            if let Some(handler) = RAW_INPUT_HANDLER.get() {
                                handler.remove_device_caches(handle_key);
                            }
                        }
                        GIDC_ARRIVAL => {
                            // Device connected - caches will populate on first input
                        }
                        _ => {}
                    }
                    DefWindowProcW(hwnd, msg, w_param, l_param)
                },
                WM_CLOSE | WM_DESTROY => unsafe {
                    PostQuitMessage(0);
                    LRESULT(0)
                },
                _ => unsafe { DefWindowProcW(hwnd, msg, w_param, l_param) },
            }
        }));
        match result {
            Ok(lr) => lr,
            Err(_) => {
                crate::util::crash_log(
                    "FFI_WINDOW_PROC_PANIC",
                    "RawInput 窗口过程异常, 已兜底返回默认处理(进程保持存活)",
                );
                unsafe { DefWindowProcW(hwnd, msg, w_param, l_param) }
            }
        }
    }

    /// Converts a string to null-terminated UTF-16 for Windows APIs.
    fn to_wstring(s: &str) -> Vec<u16> {
        use std::os::windows::ffi::OsStrExt;
        std::ffi::OsStr::new(s)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    /// Registers HID device types with the Raw Input API.
    fn register_devices(hwnd: HWND) -> anyhow::Result<()> {
        unsafe {
            let devices = [
                // Generic Desktop 页常用手柄用法
                RAWINPUTDEVICE {
                    usUsagePage: 0x01,
                    usUsage: 0x05, // Game Pad
                    dwFlags: RIDEV_INPUTSINK | RIDEV_DEVNOTIFY,
                    hwndTarget: hwnd,
                },
                RAWINPUTDEVICE {
                    usUsagePage: 0x01,
                    usUsage: 0x04, // Joystick
                    dwFlags: RIDEV_INPUTSINK | RIDEV_DEVNOTIFY,
                    hwndTarget: hwnd,
                },
                RAWINPUTDEVICE {
                    usUsagePage: 0x01,
                    usUsage: 0x08, // Multi-axis Controller
                    dwFlags: RIDEV_INPUTSINK | RIDEV_DEVNOTIFY,
                    hwndTarget: hwnd,
                },
                // Game Controls 页(0x05): 部分国产手柄/方向盘/飞行摇杆在此上报
                RAWINPUTDEVICE {
                    usUsagePage: 0x05,
                    usUsage: 0x01, // 3D Game Controller
                    dwFlags: RIDEV_INPUTSINK | RIDEV_DEVNOTIFY,
                    hwndTarget: hwnd,
                },
                RAWINPUTDEVICE {
                    usUsagePage: 0x05,
                    usUsage: 0x04, // Joystick (Game Controls)
                    dwFlags: RIDEV_INPUTSINK | RIDEV_DEVNOTIFY,
                    hwndTarget: hwnd,
                },
                RAWINPUTDEVICE {
                    usUsagePage: 0x05,
                    usUsage: 0x05, // Game Pad (Game Controls)
                    dwFlags: RIDEV_INPUTSINK | RIDEV_DEVNOTIFY,
                    hwndTarget: hwnd,
                },
            ];

            match RegisterRawInputDevices(&devices, std::mem::size_of::<RAWINPUTDEVICE>() as u32) {
                Ok(_) => Ok(()),
                Err(e) => Err(e.into()),
            }
        }
    }

    /// Processes a WM_INPUT message from the Windows message loop.
    #[inline]
    pub fn handle_raw_input(&self, l_param: LPARAM) -> bool {
        unsafe {
            let mut size = 0u32;

            let result = GetRawInputData(
                HRAWINPUT(l_param.0 as _),
                RID_INPUT,
                None,
                &mut size,
                std::mem::size_of::<RAWINPUTHEADER>() as u32,
            );

            if unlikely(result != 0) {
                return false;
            }

            // Size check for common buffer optimization
            if unlikely(size as usize > MAX_BUFFER_SIZE) {
                // Fallback for large buffers
                let mut buffer = vec![0u8; size as usize];
                let result = GetRawInputData(
                    HRAWINPUT(l_param.0 as _),
                    RID_INPUT,
                    Some(buffer.as_mut_ptr() as _),
                    &mut size,
                    std::mem::size_of::<RAWINPUTHEADER>() as u32,
                );

                if unlikely(result as u32 != size) {
                    return false;
                }

                let raw = &*(buffer.as_ptr() as *const RAWINPUT);
                return self.process_hid_input_fast(raw, buffer.len());
            }

            // Use thread-local buffer pool for typical sizes
            BUFFER_POOL.with(|pool| {
                let pool = &mut *pool.get();
                let buffer = pool.get_buffer(size as usize);
                buffer.resize(size as usize, 0);

                let result = GetRawInputData(
                    HRAWINPUT(l_param.0 as _),
                    RID_INPUT,
                    Some(buffer.as_mut_ptr() as _),
                    &mut size,
                    std::mem::size_of::<RAWINPUTHEADER>() as u32,
                );

                if unlikely(result as u32 != size) {
                    return false;
                }

                let raw = &*(buffer.as_ptr() as *const RAWINPUT);
                self.process_hid_input_fast(raw, buffer.len())
            })
        }
    }

    /// Processes HID input data with performance optimizations.
    #[inline(always)]
    fn process_hid_input_fast(&self, raw: &RAWINPUT, buffer_len: usize) -> bool {
        unsafe {
            let hid = &raw.data.hid;

            // Early size validation using const
            let raw_data_size = hid.dwSizeHid as usize;
            let raw_data_count = hid.dwCount as usize;

            if unlikely(raw_data_size < MIN_HID_DATA_SIZE || raw_data_count == 0) {
                return false;
            }

            let device_handle = raw.header.hDevice;
            let handle_key = device_handle.0 as isize;

            // Thread-local cache lookup for most recent device
            let device_info = LAST_DEVICE_CACHE.with(|cache| {
                let cache_ptr = cache.get();
                if let Some((cached_handle, ref cached_info)) = *cache_ptr
                    && likely(cached_handle == handle_key)
                {
                    return Some(cached_info.clone());
                }

                // Fast path: check global device info cache
                if let Some(info) = self.device_cache.get_sync(&handle_key) {
                    let info_clone = info.get().clone();
                    // Update thread-local cache
                    *cache_ptr = Some((handle_key, info_clone.clone()));
                    return Some(info_clone);
                }

                // Slow path: fetch and cache device info
                if let Some(info) = self.get_device_info(device_handle) {
                    // Update both caches
                    *cache_ptr = Some((handle_key, info.clone()));
                    Some(info)
                } else {
                    None
                }
            });

            let device_info = match device_info {
                Some(info) => info,
                None => return false,
            };

            // Fast filter check: 支持 Generic Desktop(0x01)与 Game Controls(0x05)两页,
            // 以最大程度兼容国产手柄/摇杆/方向盘等不同 HID 上报用法
            let usage = device_info.usage;
            let is_supported_usage = match device_info.usage_page {
                HID_USAGE_PAGE_GENERIC => {
                    usage == HID_USAGE_GAMEPAD
                        || usage == HID_USAGE_JOYSTICK
                        || usage == HID_USAGE_MULTI_AXIS
                }
                HID_USAGE_PAGE_GAME_CTRL => {
                    usage == HID_USAGE_3D_GAME
                        || usage == HID_USAGE_JOYSTICK
                        || usage == HID_USAGE_GAMEPAD
                }
                _ => false,
            };
            if unlikely(!is_supported_usage) {
                return false;
            }

            let vid_pid = (device_info.vendor_id, device_info.product_id);
            if self.ownership.is_claimed_by_higher_priority(
                vid_pid,
                &crate::input_ownership::InputSource::RawInput(handle_key),
            ) {
                return false;
            }

            let is_capturing = self.state.is_raw_input_capture_active();

            let data_ptr = hid.bRawData.as_ptr();
            // 绝对稳定: bRawData 是 RAWINPUT 内的可变长字段, 设备/驱动可能上报异常元数据
            // (dwSizeHid*dwCount 乘积远大于实际缓冲) —— 构建切片前必须与真实缓冲容量比对,
            // 超界则丢弃该帧(防 OOB 读导致崩溃)。
            let b_raw_offset =
                std::mem::size_of::<windows::Win32::UI::Input::RAWINPUTHEADER>() + 8;
            let max_avail = buffer_len.saturating_sub(b_raw_offset);
            let claimed = raw_data_size.saturating_mul(raw_data_count);
            if unlikely(claimed > max_avail) {
                return false;
            }
            let data_slice = std::slice::from_raw_parts(data_ptr, claimed);

            // === CAPTURE MODE ===
            if unlikely(is_capturing) {
                return self.handle_capture_mode(handle_key, device_info, data_slice);
            }

            // === NORMAL MODE - Detect button changes and dispatch events ===

            // Generate device identifier (needed for both activation and normal processing)
            let stable_device_id = Self::generate_stable_device_id(&device_info);

            Self::update_device_display_info(stable_device_id, &device_info);

            // Check if device is activated (has baseline)
            // This check MUST be before paused check to allow activation even when paused
            let has_baseline = if let Some(baseline_ready) = self
                .device_states
                .read_sync(&handle_key, |_, state| state.baseline_ready)
            {
                baseline_ready
            } else {
                // New device detected - try to load baseline from config
                if let Some(baseline) = self
                    .config_baselines
                    .read_sync(&stable_device_id, |_, v| v.clone())
                {
                    // Found baseline in config - load it
                    let _ = self
                        .device_states
                        .insert_sync(handle_key, DeviceHidState::with_baseline(baseline));
                    true
                } else {
                    false
                }
            };

            if unlikely(!has_baseline) {
                /* 官方 Xbox 手柄(VID 045E + 官方 PID 清单): RawInput 完全忽略,
                 * 由 XInput 独占处理(先连后开也稳定, 无激活弹窗)。
                 * 注意: 只按官方 PID 清单判定——不少国产手柄会冒用 045E VID,
                 * 但它们不是 XInput 设备, 必须走 RawInput 激活流程才能被识别。 */
                if device_info.vendor_id == 0x045E
                    && device_info.usage_page == 0x01
                    && crate::xinput::is_genuine_xbox_pid(device_info.product_id)
                {
                    let _ = self.device_states.insert_sync(
                        handle_key,
                        DeviceHidState::with_baseline(data_slice.to_vec()),
                    );
                    return false;
                }

                /* 若 XInput 当前已连接手柄: 该 RawInput 设备通常就是同一只手柄的 HID 接口
                 * (物理 VID:PID 与 XInput 身份 045E:xxxx 不一致, 无法直接比对所有权)。
                 * 跳过激活弹窗, 交由 XInput 独占处理 —— 避免国产 X360 兼容手柄
                 * 在\"明明 Xbox 模式可用\"时弹出永远无法成功激活的对话框。 */
                if crate::xinput::any_connected_static() {
                    let _ = self.device_states.insert_sync(
                        handle_key,
                        DeviceHidState::with_baseline(data_slice.to_vec()),
                    );
                    return false;
                }
                // Device not activated - handle activation regardless of paused state
                if likely(self.state.is_device_activating(handle_key)) {
                    // Send HID data to activation dialog
                    let pooled_data =
                        HID_DATA_POOL.with(|pool| (*pool.get()).copy_to_vec(data_slice));
                    self.state.send_hid_activation_data(handle_key, pooled_data);
                    return false;
                } else {
                    // Request activation for first time
                    self.request_device_activation(handle_key, &device_info);
                    return false;
                }
            }

            // Detect button changes using baseline comparison
            let mut changes = self.detect_hid_changes(
                handle_key,
                data_slice,
                stable_device_id,
                device_info.device_type,
            );

            /* ★v22.4: 发布原始位组合哈希 (与连发同编码) —— 即使语义通道没在跑,
             * 已校准的槽位也能点亮。仅对识别目标手柄生效 (内部已过滤)。 */
            if self.state.live_hid_pad().is_some() {
                let pos = self
                    .device_states
                    .read_sync(&handle_key, |_, s| s.last_button_id)
                    .flatten()
                    .map(|id| id as u32)
                    .unwrap_or(0);
                self.state.publish_live_raw(
                    device_info.vendor_id,
                    device_info.product_id,
                    pos,
                );
            }

            /* ★v22.7: 校准向导进行中 → 只保留实时状态 (上面已发布), 不响应切换键/预设切换,
             * 也不派发任何映射 —— 否则校对按键会被注入游戏或被别的功能抢走。 */
            let calibrating = self.state.is_gp_calibrating();

            // Check switch key first (before paused check)
            if likely(!changes.is_empty()) && !calibrating {
                let switch_button_id = self
                    .state
                    .switch_key_cache
                    .generic_button_id
                    .load(Ordering::Relaxed);

                // Check for switch key toggle
                if unlikely(switch_button_id != 0) {
                    for (button_id, is_pressed) in &changes {
                        if *button_id == switch_button_id && *is_pressed {
                            self.state.handle_switch_key_toggle();
                            return true;
                        }
                    }
                }

                /* ★v21.7 预设切换键 (原始 HID 手柄): button_id 高 32 位即设备指纹,
                 * 精确比对即可定位"同一只设备的同一个按键组合"。命中即请求切换
                 * (GUI 线程执行落盘+热重载); 同时把该按键从映射派发里剔除, 保证
                 * 切换键优先 —— 即使用户把它同时绑成了普通映射也不会双触发。 */
                let mut switched_ids: SmallVec<[u64; 4]> = SmallVec::new();
                if unlikely(!self.state.preset_switch_bindings_is_empty()) {
                    let bindings = self.state.preset_switch_bindings_snapshot();
                    for b in &bindings {
                        if let InputDevice::GenericDevice { button_id, .. } = &b.device {
                            for (changed_id, is_pressed) in &changes {
                                if *is_pressed && *changed_id == *button_id {
                                    self.state.request_preset_switch(&b.preset_name);
                                    switched_ids.push(*changed_id);
                                }
                            }
                        }
                    }
                }
                if !switched_ids.is_empty() {
                    changes.retain(|(id, _)| !switched_ids.contains(id));
                }
            }

            // Fast paused check (only for activated devices)
            if unlikely(self.state.is_paused()) {
                return false;
            }

            /* ★v21.7 方案A: 语义翻译 —— 配置含语义 HID 触发键, 或 GUI 正在实时识别某手柄
             * (SVG 按下即亮) 时启用。两条通道命名空间独立, 各匹配各的映射, 互不干扰。 */
            if self.state.has_semantic_hid_mappings() || self.state.live_hid_pad().is_some() {
                self.dispatch_semantic_inputs(
                    device_handle,
                    handle_key,
                    &device_info,
                    stable_device_id,
                    data_slice,
                );
            }

            // Dispatch events for each button change
            if likely(!changes.is_empty())
                && !calibrating
                && let Some(pool) = self.state.get_worker_pool()
            {
                for (button_id, is_pressed) in changes {
                    let device = InputDevice::GenericDevice {
                        device_type: device_info.device_type,
                        button_id,
                    };

                    // Only dispatch if mapping exists
                    if likely(self.state.get_input_mapping(&device).is_some()) {
                        let event = if is_pressed {
                            InputEvent::Pressed(device)
                        } else {
                            InputEvent::Released(device)
                        };
                        pool.dispatch(event);
                    }
                }
                true
            } else {
                false
            }
        }
    }

    /// Captures HID button input at bit level.
    /// Finds the first changed bit in the busiest frame and returns its button_id.
    #[inline(always)]
    fn handle_capture_mode(
        &self,
        handle_key: isize,
        device_info: CachedDeviceInfo,
        current_data: &[u8],
    ) -> bool {
        // Check if device baseline is established
        let has_baseline = self
            .device_states
            .read_sync(&handle_key, |_, state| state.baseline_ready)
            .unwrap_or(false);

        if unlikely(!has_baseline) {
            // Device not activated - handle activation regardless of capture mode
            if likely(self.state.is_device_activating(handle_key)) {
                // Send HID data to activation dialog
                let pooled_data =
                    unsafe { HID_DATA_POOL.with(|pool| (*pool.get()).copy_to_vec(current_data)) };
                self.state.send_hid_activation_data(handle_key, pooled_data);
                return false;
            } else {
                // Request activation for first time
                self.request_device_activation(handle_key, &device_info);
                return false;
            }
        }

        let stable_device_id = Self::generate_stable_device_id(&device_info);
        Self::update_device_display_info(stable_device_id, &device_info);

        // Compare current data with baseline
        let is_baseline = if let Some(baseline) = self
            .device_states
            .read_sync(&handle_key, |_, state| state.baseline_data.clone())
        {
            DeviceCaptureState::is_equal_fast(current_data, &baseline)
        } else {
            false
        };

        if unlikely(is_baseline) {
            // All buttons released - finalize capture if we have frames
            let has_frames = self
                .capture_states
                .read_sync(&handle_key, |_, state| state.frame_count > 0)
                .unwrap_or(false);

            if likely(has_frames) {
                // Get best frame based on capture mode
                let capture_mode = self.state.get_rawinput_capture_mode();

                if let Some((best_data, baseline_data)) = self
                    .device_states
                    .read_sync(&handle_key, |_, state| state.baseline_data.clone())
                    .and_then(|baseline| {
                        self.capture_states
                            .read_sync(&handle_key, |_, state| {
                                state
                                    .get_best_frame(&baseline, capture_mode)
                                    .map(|slice| slice.to_vec())
                            })
                            .flatten()
                            .map(|best| (best, baseline))
                    })
                {
                    // Hash all changed bit positions to uniquely identify this input pattern
                    let button_id = Self::hash_changed_bit_pattern(
                        &best_data,
                        &baseline_data,
                        stable_device_id,
                    );

                    let device = InputDevice::GenericDevice {
                        device_type: device_info.device_type,
                        button_id,
                    };

                    let _ = self.state.get_raw_input_capture_sender().send(device);
                    self.capture_states.remove_sync(&handle_key);

                    return true;
                }
            }
            return false;
        }

        // Not baseline - add frame with timestamp
        let timestamp_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;

        let updated = self
            .capture_states
            .update_sync(&handle_key, |_, capture_state| {
                capture_state.add_frame(current_data, timestamp_ms);
            })
            .is_some();

        if unlikely(!updated) {
            // Create new state with first frame
            let _ = self.capture_states.insert_sync(
                handle_key,
                DeviceCaptureState::with_frame(current_data, timestamp_ms),
            );
        }

        false
    }

    /// Hashes all changed bit positions to create a unique button_id for this input pattern.
    /// Returns button_id in format: (device_id << 32) | hash(changed_bit_positions)
    ///
    /// Uses FNV-1a to hash the positions of all changed bits. This ensures:
    /// - Different input patterns get unique IDs (e.g., joystick UP vs RIGHT vs UP+RIGHT)
    /// - Same input pattern always gets the same ID (deterministic)
    /// - Extremely fast with minimal collisions
    #[inline(always)]
    fn hash_changed_bit_pattern(data: &[u8], baseline: &[u8], stable_device_id: u64) -> u64 {
        let min_len = data.len().min(baseline.len());
        let mut hash = fnv32::OFFSET_BASIS;

        // Hash each changed bit position
        for byte_idx in SKIP_BYTES..min_len {
            let data_byte = data[byte_idx];
            let baseline_byte = baseline[byte_idx];
            let mut diff = data_byte ^ baseline_byte;

            if diff != 0 {
                // Process each changed bit in this byte
                while diff != 0 {
                    let bit_idx = diff.trailing_zeros();

                    // Hash the position (byte_idx, bit_idx) using FNV-1a
                    hash = fnv1a_hash_u32(hash, byte_idx as u32);
                    hash = fnv1a_hash_u32(hash, bit_idx);

                    // Clear the lowest set bit (BLSR instruction)
                    diff &= diff - 1;
                }
            }
        }

        (stable_device_id << 32) | (hash as u64)
    }

    /// Extract serial number from device path
    fn extract_serial_from_path(path: &str) -> Option<String> {
        let parts: Vec<&str> = path.split('#').collect();
        if parts.len() >= 3 {
            let serial = parts[2].trim();
            if !serial.is_empty() && serial.chars().all(|c| c.is_alphanumeric() || c == '_') {
                return Some(serial.to_string());
            }
        }
        None
    }

    /// Get device serial number from device handle
    fn get_device_serial_number(device_handle: HANDLE) -> Option<String> {
        unsafe {
            let mut size = 0u32;
            let result =
                GetRawInputDeviceInfoW(Some(device_handle), RIDI_DEVICENAME, None, &mut size);

            if result != 0 || size == 0 {
                return None;
            }

            let mut path_buf = vec![0u16; size as usize];
            let result = GetRawInputDeviceInfoW(
                Some(device_handle),
                RIDI_DEVICENAME,
                Some(path_buf.as_mut_ptr() as _),
                &mut size,
            );

            if result == u32::MAX {
                return None;
            }

            let path = String::from_utf16_lossy(&path_buf);
            Self::extract_serial_from_path(&path)
        }
    }

    /// Get device info for a given device handle
    #[inline]
    fn get_device_info(&self, device_handle: HANDLE) -> Option<CachedDeviceInfo> {
        let handle_key = device_handle.0 as isize;

        // Fast path: try async read first (lock-free)
        if let Some(device_info) = self.device_cache.read_sync(&handle_key, |_, v| v.clone()) {
            return Some(device_info);
        }

        unsafe {
            let mut size = 0u32;
            let result =
                GetRawInputDeviceInfoW(Some(device_handle), RIDI_DEVICEINFO, None, &mut size);

            if result != 0 {
                return None;
            }

            let mut buffer = vec![0u8; size as usize];
            let result = GetRawInputDeviceInfoW(
                Some(device_handle),
                RIDI_DEVICEINFO,
                Some(buffer.as_mut_ptr() as _),
                &mut size,
            );

            if result == u32::MAX {
                return None;
            }

            let device_info = &*(buffer.as_ptr() as *const RID_DEVICE_INFO);

            let cached_info = match device_info.dwType {
                t if t == RIM_TYPEHID => {
                    let hid_info = &device_info.Anonymous.hid;
                    let usage_page = hid_info.usUsagePage;
                    let usage = hid_info.usUsage;
                    let vendor_id = hid_info.dwVendorId as u16;
                    let product_id = hid_info.dwProductId as u16;

                    let device_type = match (usage_page, usage) {
                        (0x01, 0x05) | (0x05, 0x05) => DeviceType::Gamepad(vendor_id),
                        (0x01, 0x04) | (0x05, 0x04) | (0x05, 0x01) => {
                            DeviceType::Joystick(vendor_id)
                        }
                        (0x01, 0x08) => DeviceType::Gamepad(vendor_id),
                        _ => DeviceType::HidDevice { usage_page, usage },
                    };

                    let serial_number = Self::get_device_serial_number(device_handle);

                    CachedDeviceInfo {
                        device_type,
                        vendor_id,
                        product_id,
                        serial_number,
                        usage_page,
                        usage,
                    }
                }
                _ => return None,
            };

            let _ = self
                .device_cache
                .upsert_sync(handle_key, cached_info.clone());

            Some(cached_info)
        }
    }

    /// Generate stable device ID for consistent identification.
    ///
    /// Uses a hybrid strategy:
    /// - If device has a valid serial number (not Windows instance ID), use VID+PID+Serial
    /// - Otherwise, use only VID+PID to support device reconnection
    #[inline(always)]
    fn generate_stable_device_id(device_info: &CachedDeviceInfo) -> u64 {
        if let Some(ref serial) = device_info.serial_number {
            // Check if this is a real serial number or Windows instance ID
            // Windows instance IDs contain '&' (e.g., "6&2c5b8c5d&0&0000")
            // Real serial numbers are typically longer and don't contain '&'
            let is_real_serial = !serial.contains('&') && serial.len() > 4;

            if is_real_serial {
                return Self::hash_vid_pid_serial(
                    device_info.vendor_id,
                    device_info.product_id,
                    serial,
                );
            }
        }

        // No serial or unreliable serial - use only VID+PID
        Self::hash_vid_pid(device_info.vendor_id, device_info.product_id)
    }

    /// Computes FNV-1a hash for device identification using VID, PID, and serial number.
    #[inline(always)]
    fn hash_vid_pid_serial(vendor_id: u16, product_id: u16, serial: &str) -> u64 {
        let mut hash = fnv64::OFFSET_BASIS;
        hash = fnv1a_hash_u64(hash, vendor_id as u64);
        hash = fnv1a_hash_u64(hash, product_id as u64);
        hash = fnv1a_hash_bytes(hash, serial.as_bytes());
        hash
    }

    /// Computes FNV-1a hash for device identification using VID and PID only.
    #[inline(always)]
    fn hash_vid_pid(vendor_id: u16, product_id: u16) -> u64 {
        let mut hash = fnv64::OFFSET_BASIS;
        hash = fnv1a_hash_u64(hash, vendor_id as u64);
        hash = fnv1a_hash_u64(hash, product_id as u64);
        hash
    }

    /// Parses device_id string (format: "VID:PID" or "VID:PID:Serial") into components.
    #[inline]
    fn parse_device_id(device_id: &str) -> Option<(u16, u16, Option<String>)> {
        let parts: Vec<&str> = device_id.split(':').collect();
        if parts.len() < 2 {
            return None;
        }

        let vid = u16::from_str_radix(parts[0], 16).ok()?;
        let pid = u16::from_str_radix(parts[1], 16).ok()?;
        let serial = if parts.len() >= 3 {
            Some(parts[2].to_string())
        } else {
            None
        };

        Some((vid, pid, serial))
    }

    /// Request device activation from GUI.
    #[inline(never)]
    #[cold]
    fn request_device_activation(&self, handle_key: isize, device_info: &CachedDeviceInfo) {
        let device_name = format!(
            "{} ({:04X}:{:04X})",
            match device_info.usage {
                HID_USAGE_GAMEPAD => "Gamepad",
                HID_USAGE_JOYSTICK => "Joystick",
                HID_USAGE_MULTI_AXIS => "Controller",
                _ => "HID Device",
            },
            device_info.vendor_id,
            device_info.product_id
        );

        let request = crate::state::HidActivationRequest {
            device_handle: handle_key,
            device_name,
            vid: device_info.vendor_id,
            pid: device_info.product_id,
            usage_page: device_info.usage_page,
            usage: device_info.usage,
        };

        self.state.request_hid_activation(request);
    }

    /// Detects HID button state changes at bit level using optimized bit operations.
    /// Returns list of (button_id, is_pressed) for detected changes.
    ///
    /// Strategy:
    /// 1. Collect all currently active bits (relative to baseline)
    /// 2. Compare with previously active bits to find press/release events
    /// 3. For each event, generate both combo and individual button_ids
    ///    - Combo: for multi-button combos or unique patterns
    ///    - Individual: for simultaneous independent keys
    #[inline]
    fn detect_hid_changes(
        &self,
        handle_key: isize,
        current_data: &[u8],
        stable_device_id: u64,
        _device_type: crate::state::DeviceType,
    ) -> SmallVec<[(u64, bool); 8]> {
        let mut changes = SmallVec::new();

        if let Some(result_changes) = self.device_states.update_sync(&handle_key, |_, state| {
            let mut temp_changes: SmallVec<[(u64, bool); 8]> = SmallVec::new();

            if unlikely(current_data.len() <= SKIP_BYTES) {
                return temp_changes;
            }

            let min_len = current_data
                .len()
                .min(state.last_data.len())
                .min(state.baseline_data.len());

            // Collect all currently active bit positions (relative to baseline)
            let mut curr_active: SmallVec<[(usize, u32); 8]> = SmallVec::new();
            let mut prev_active: SmallVec<[(usize, u32); 8]> = SmallVec::new();

            #[allow(clippy::needless_range_loop)]
            for byte_idx in SKIP_BYTES..min_len {
                let curr_byte = current_data[byte_idx];
                let prev_byte = state.last_data[byte_idx];
                let baseline_byte = state.baseline_data[byte_idx];

                // Collect currently active bits
                let mut curr_diff = curr_byte ^ baseline_byte;
                while curr_diff != 0 {
                    let bit_idx = curr_diff.trailing_zeros();
                    curr_active.push((byte_idx, bit_idx));
                    curr_diff &= curr_diff - 1;
                }

                // Collect previously active bits
                let mut prev_diff = prev_byte ^ baseline_byte;
                while prev_diff != 0 {
                    let bit_idx = prev_diff.trailing_zeros();
                    prev_active.push((byte_idx, bit_idx));
                    prev_diff &= prev_diff - 1;
                }
            }

            // 基线长度自适应学习: 报文长度与基线不一致时会限制检测区间(只能覆盖
            // 公共前缀), 落在多余字节上的按键(组合键/高位键)因此可能永远检测不到。
            // 若当前帧相对现有基线无任何按键活动(空闲形态), 直接以当前帧为新基线,
            // 之后检测区间覆盖完整报文长度。
            if current_data.len() != state.baseline_data.len() && curr_active.is_empty() {
                state.baseline_data.clear();
                state.baseline_data.extend_from_slice(current_data);
            }

            // Detect changes in active button combination
            if curr_active != prev_active {
                // If previous combination existed and is different, release it
                if !prev_active.is_empty()
                    && let Some(last_id) = state.last_button_id
                {
                    temp_changes.push((last_id, false));
                }

                // If new combination exists, press it
                if !curr_active.is_empty() {
                    Self::generate_button_events(
                        &curr_active,
                        stable_device_id,
                        true,
                        &mut temp_changes,
                    );

                    // Save the new button_id for next release
                    if let Some(&(new_button_id, _)) = temp_changes.last() {
                        state.last_button_id = Some(new_button_id);
                    }
                } else {
                    // All released
                    state.last_button_id = None;
                }
            }

            // Update state
            // 自适应拷贝: 报文长度漂移时重建历史帧与当前帧严格对齐
            // (旧实现直接 copy_from_slice 会在长度不一致时 panic ——
            //  曾导致 rawinput_thread 每帧 FFI 兜底、输入间歇性丢失)
            if state.last_data.len() == current_data.len() {
                state.last_data.copy_from_slice(current_data);
            } else {
                state.last_data.clear();
                state.last_data.extend_from_slice(current_data);
            }
            state.last_update = Instant::now();

            temp_changes
        }) {
            changes = result_changes;
        }

        changes
    }

    /// Generates button events for a set of bit positions.
    /// Only creates a single combo button_id from all positions together.
    #[inline(always)]
    fn generate_button_events(
        positions: &[(usize, u32)],
        stable_device_id: u64,
        is_pressed: bool,
        events: &mut SmallVec<[(u64, bool); 8]>,
    ) {
        // Generate single combo button_id (hash all positions together)
        let combo_id = Self::hash_bit_positions(positions, stable_device_id);
        events.push((combo_id, is_pressed));
    }

    /// Hashes a set of bit positions using FNV-1a for consistent button_id generation.
    /// Extremely fast with minimal collisions.
    #[inline(always)]
    fn hash_bit_positions(positions: &[(usize, u32)], stable_device_id: u64) -> u64 {
        let mut hash = fnv32::OFFSET_BASIS;
        for &(byte_idx, bit_idx) in positions {
            hash = fnv1a_hash_u32(hash, byte_idx as u32);
            hash = fnv1a_hash_u32(hash, bit_idx);
        }

        (stable_device_id << 32) | (hash as u64)
    }

    /// Updates the global device display information cache.
    #[inline(always)]
    fn update_device_display_info(stable_device_id: u64, device_info: &CachedDeviceInfo) {
        let display_info = DeviceDisplayInfo {
            vendor_id: device_info.vendor_id,
            product_id: device_info.product_id,
            serial_number: device_info.serial_number.clone(),
        };

        let cache = DEVICE_DISPLAY_INFO.get_or_init(scc::HashMap::new);
        // Only use lower 32 bits to match button_id format (device_id is stored in high 32 bits of button_id)
        let _ = cache.upsert_sync(stable_device_id & 0xFFFFFFFF, display_info);
    }

    /// ★v21.7 方案A: 用 `HidP_GetUsages` 把当前原始报文翻译成**语义按键**事件并派发。
    ///
    /// 只在配置里存在语义触发键时才调用 (调用方已判 `has_semantic_hid_mappings`), 因此
    /// 普通用户零开销。语义 button_id 与既有 bit-hash / 手动捕获命名空间不重叠, 两套映射
    /// 可共存: 每个事件只在存在对应映射时才派发, 不会互相干扰或双触发。
    fn dispatch_semantic_inputs(
        &self,
        device_handle: HANDLE,
        handle_key: isize,
        device_info: &CachedDeviceInfo,
        stable_device_id: u64,
        data: &[u8],
    ) {
        use windows::Win32::Devices::HumanInterfaceDevice as hid;

        /* 首次见到该设备: 取 preparsed 数据并缓存 + 轴规格 */
        if self.semantic_states.read_sync(&handle_key, |_, _| ()).is_none() {
            let Some(mut blob) = read_preparsed_blob(device_handle) else {
                return;
            };
            let axis_specs = axis_specs_from_preparsed(&mut blob);
            let _ = self.semantic_states.insert_sync(
                handle_key,
                SemanticDeviceState {
                    preparsed: blob,
                    last_usages: SmallVec::new(),
                    axis_specs,
                    prev_dirs: (0, 0, 0),
                    prev_triggers: (false, false),
                },
            );
        }

        /* HidP_GetUsages 需要 &mut 报文 —— 拷贝一份小的栈缓冲 (报文 ≤128 字节足够) */
        let mut report = [0u8; 128];
        let rlen = data.len().min(report.len());
        report[..rlen].copy_from_slice(&data[..rlen]);
        /* 空闲基线 (用于摇杆方向判定; 缺失则本帧不判方向) */
        let baseline: Vec<u8> = self
            .device_states
            .read_sync(&handle_key, |_, s| s.baseline_data.clone())
            .unwrap_or_default();

        let mut changes: SmallVec<[(u32, bool); 24]> = SmallVec::new();
        let mut live = crate::state::LiveHidState {
            vid: device_info.vendor_id,
            pid: device_info.product_id,
            ..Default::default()
        };
        let _ = self.semantic_states.update_sync(&handle_key, |_, st| {
            let mut button_mask: u32 = 0;
            let now: SmallVec<[u16; 16]> = unsafe {
                let pp = hid::PHIDP_PREPARSED_DATA(st.preparsed.as_mut_ptr() as isize);
                let mut list = [0u16; 32];
                let mut len = list.len() as u32;
                let status = hid::HidP_GetUsages(
                    hid::HidP_Input,
                    0x09, // Button usage page
                    None,
                    list.as_mut_ptr(),
                    &mut len,
                    pp,
                    &mut report[..rlen],
                );
                /* ⚠ 全松开时 HidP_GetUsages 返回 USAGE_NOT_FOUND —— 这是"空集"而非错误;
                 * 若当错误提前 return, last_usages 永不更新 → 注入键永久悬空 (卡键)。 */
                if status == hid::HIDP_STATUS_USAGE_NOT_FOUND {
                    SmallVec::new()
                } else if status.0 < 0 {
                    return;
                } else {
                    list[..(len as usize).min(list.len())]
                        .iter()
                        .copied()
                        .collect()
                }
            };
            for &u in &now {
                if (u as u32) < 32 {
                    button_mask |= 1u32 << u;
                }
                if !st.last_usages.contains(&u) {
                    changes.push((crate::hid_layout::semantic_button_position(u as u32), true));
                }
            }
            for &u in &st.last_usages {
                if !now.contains(&u) {
                    changes.push((crate::hid_layout::semantic_button_position(u as u32), false));
                }
            }
            st.last_usages = now;
            live.buttons = button_mask;

            /* 十字键 (Hat switch): 绝对方向值 1..8 (顺时针自"上"), 0/失败 = 中 */
            let specs = st.axis_specs.clone();
            unsafe {
                let pp = hid::PHIDP_PREPARSED_DATA(st.preparsed.as_mut_ptr() as isize);
                let mut hat_v: u32 = 0;
                let hat_ok = hid::HidP_GetUsageValue(
                    hid::HidP_Input,
                    0x01,
                    None,
                    crate::hid_layout::USAGE_HAT_SWITCH,
                    &mut hat_v,
                    pp,
                    &report[..rlen],
                );
                if hat_ok.0 >= 0 && (1..=8).contains(&hat_v) {
                    live.dpad = hat_v as u8;
                }

                let read_axis = |usage: u16, buf: &[u8]| -> Option<u32> {
                    let mut v: u32 = 0;
                    let stt = hid::HidP_GetUsageValue(
                        hid::HidP_Input,
                        0x01,
                        None,
                        usage,
                        &mut v,
                        pp,
                        buf,
                    );
                    (stt.0 >= 0).then_some(v)
                };
                let base: &[u8] = if baseline.len() == rlen { &baseline } else { &[] };
                let mut ls_bits = 0u8;
                let mut rs_bits = 0u8;
                let mut ls_size = 16u8;
                let mut rs_size = 16u8;
                for (usage, size) in &specs {
                    match *usage {
                        crate::hid_layout::USAGE_X => ls_size = *size,
                        crate::hid_layout::USAGE_RX => rs_size = *size,
                        _ => {}
                    }
                }
                let x = read_axis(crate::hid_layout::USAGE_X, &report[..rlen]);
                let y = read_axis(crate::hid_layout::USAGE_Y, &report[..rlen]);
                let rx = read_axis(crate::hid_layout::USAGE_RX, &report[..rlen]);
                let ry = read_axis(crate::hid_layout::USAGE_RY, &report[..rlen]);
                if !base.is_empty() {
                    let bx = read_axis(crate::hid_layout::USAGE_X, base);
                    let by = read_axis(crate::hid_layout::USAGE_Y, base);
                    let brx = read_axis(crate::hid_layout::USAGE_RX, base);
                    let bry = read_axis(crate::hid_layout::USAGE_RY, base);
                    ls_bits = stick_dirs_from_delta(x, bx, y, by, ls_size);
                    rs_bits = stick_dirs_from_delta(rx, brx, ry, bry, rs_size);
                }
                live.ls = ls_bits;
                live.rs = rs_bits;

                /* ★v22.1 扳机 (Z/Rz 模拟轴) → 超阈值按压事件。
                 * 非标准第三方手柄的 LT/RT 常是模拟轴而非按钮; 这里统一转成
                 * `semantic_axis_position(Z/Rz, 2)` 的 Pressed/Released, 与方向键同通道,
                 * 因此 LT/RT 既能点亮也能映射 (触发名 `..._A50U` / `..._A53U`)。 */
                let z_bits = specs
                    .iter()
                    .find(|(u, _)| *u == crate::hid_layout::USAGE_Z)
                    .map(|(_, s)| *s);
                let rz_bits = specs
                    .iter()
                    .find(|(u, _)| *u == crate::hid_layout::USAGE_RZ)
                    .map(|(_, s)| *s);
                let lt_now = z_bits.is_some_and(|b| {
                    let base = if base.is_empty() {
                        None
                    } else {
                        read_axis(crate::hid_layout::USAGE_Z, base)
                    };
                    crate::hid_layout::trigger_pressed(
                        read_axis(crate::hid_layout::USAGE_Z, &report[..rlen]),
                        base,
                        b,
                    )
                });
                let rt_now = rz_bits.is_some_and(|b| {
                    let base = if base.is_empty() {
                        None
                    } else {
                        read_axis(crate::hid_layout::USAGE_RZ, base)
                    };
                    crate::hid_layout::trigger_pressed(
                        read_axis(crate::hid_layout::USAGE_RZ, &report[..rlen]),
                        base,
                        b,
                    )
                });
                if lt_now != st.prev_triggers.0 {
                    changes.push((
                        crate::hid_layout::semantic_axis_position(
                            crate::hid_layout::USAGE_Z,
                            2,
                        ),
                        lt_now,
                    ));
                }
                if rt_now != st.prev_triggers.1 {
                    changes.push((
                        crate::hid_layout::semantic_axis_position(
                            crate::hid_layout::USAGE_RZ,
                            2,
                        ),
                        rt_now,
                    ));
                }
                st.prev_triggers = (lt_now, rt_now);
                live.lt = lt_now;
                live.rt = rt_now;
            }

            /* ★v21.7b 方向键 → 语义轴事件 (与按钮同通道; 简单方向映射即可用) */
            let now_dirs = (dpad_bits_from_hat(live.dpad), live.ls, live.rs);
            for (usage, dir, pressed) in axis_dir_transitions(st.prev_dirs, now_dirs) {
                changes.push((
                    crate::hid_layout::semantic_axis_position(usage, dir),
                    pressed,
                ));
            }
            st.prev_dirs = now_dirs;
        });

        /* ★v22.3: 发布"原始位组合哈希" —— 与连发映射同一编码, 供 SVG 高亮识别非标准手柄按键。
         * `detect_hid_changes` 已在同一帧先跑完, last_button_id 就是当前按下的组合 (松开为 None)。 */
        live.raw_position = self
            .device_states
            .read_sync(&handle_key, |_, s| s.last_button_id)
            .flatten()
            .map(|id| id as u32)
            .unwrap_or(0);

        /* 发布实时状态 (无论有无映射: SVG 热点"按下即亮"依赖它) */
        self.state.publish_live_hid(live);

        if changes.is_empty() {
            return;
        }
        /* ★v22.7: 校准向导进行中 → 实时状态已发布, 但语义映射一律不派发 (防校对键被注入游戏) */
        if self.state.is_gp_calibrating() {
            return;
        }
        let Some(pool) = self.state.get_worker_pool() else {
            return;
        };
        for (position, pressed) in changes {
            let button_id = (stable_device_id << 32) | position as u64;
            let device = InputDevice::GenericDevice {
                device_type: device_info.device_type,
                button_id,
            };
            if self.state.get_input_mapping(&device).is_some() {
                let event = if pressed {
                    InputEvent::Pressed(device)
                } else {
                    InputEvent::Released(device)
                };
                pool.dispatch(event);
            }
        }
    }
}

/// Activates a HID device with the given baseline data.
///
/// Updates runtime device state and baseline cache for reconnection support.
/// The caller is responsible for persisting baseline data to configuration file.
#[inline]
pub fn activate_hid_device(device_handle: isize, baseline_data: Vec<u8>) {
    if let Some(handler) = RAW_INPUT_HANDLER.get() {
        let _ = handler.device_states.insert_sync(
            device_handle,
            DeviceHidState::with_baseline(baseline_data.clone()),
        );

        if let Some(device_info) = handler
            .device_cache
            .read_sync(&device_handle, |_, v| v.clone())
        {
            let stable_device_id = RawInputHandler::generate_stable_device_id(&device_info);
            let _ = handler
                .config_baselines
                .insert_sync(stable_device_id, baseline_data);
        }
    }
}

/// Resets all HID device states to baseline and clears capture states.
pub fn reset_hid_device_states() {
    if let Some(handler) = RAW_INPUT_HANDLER.get() {
        handler.reset_device_states_to_baseline();
        handler.capture_states.retain_sync(|_, _| false);
    }
}

/// Gets device info for a device handle.
pub fn get_device_info_for_handle(device_handle: isize) -> Option<(u16, u16, Option<String>)> {
    if let Some(handler) = RAW_INPUT_HANDLER.get()
        && let Some(device_info) = handler
            .device_cache
            .read_sync(&device_handle, |_, v| v.clone())
    {
        return Some((
            device_info.vendor_id,
            device_info.product_id,
            device_info.serial_number,
        ));
    }
    None
}

/// Retrieves cached display information for a device.
pub fn get_device_display_info(stable_device_id: u64) -> Option<DeviceDisplayInfo> {
    let cache = DEVICE_DISPLAY_INFO.get()?;
    cache
        .get_sync(&stable_device_id)
        .map(|entry| entry.get().clone())
}

/// Registers device display information.
///
/// Used for devices not detected through Raw Input (e.g., XInput).
pub fn register_device_display_info(stable_device_id: u64, info: DeviceDisplayInfo) {
    if let Some(cache) = DEVICE_DISPLAY_INFO.get() {
        let _ = cache.upsert_sync(stable_device_id, info);
    }
}

/// Clears the device display information cache.
pub fn clear_device_display_info_cache() {
    if let Some(cache) = DEVICE_DISPLAY_INFO.get() {
        cache.clear_sync();
    }
}

/// Clears baseline data for a device identified by VID:PID.
///
/// Removes activation data from runtime state and configuration cache.
#[inline]
pub fn clear_device_baseline(vid: u16, pid: u16) {
    if let Some(handler) = RAW_INPUT_HANDLER.get() {
        let stable_id = RawInputHandler::hash_vid_pid(vid, pid);
        handler.config_baselines.remove_sync(&stable_id);

        let mut handles_to_remove = smallvec::SmallVec::<[isize; 4]>::new();

        handler.device_cache.retain_sync(|&handle, info| {
            if info.vendor_id == vid && info.product_id == pid {
                handles_to_remove.push(handle);
            }
            true
        });

        for handle in handles_to_remove {
            handler.device_states.remove_sync(&handle);
            handler.capture_states.remove_sync(&handle);
        }
    }

    // Release device ownership to allow re-claiming
    crate::input_manager::release_device_ownership((vid, pid));
}

/// Enumerates all HID devices currently connected.
pub fn enumerate_hid_devices() -> Vec<crate::gui::device_manager_dialog::HidDeviceInfo> {
    use windows::Win32::UI::Input::*;

    let mut devices = Vec::new();

    unsafe {
        let mut device_count = 0u32;
        if GetRawInputDeviceList(
            None,
            &mut device_count,
            std::mem::size_of::<RAWINPUTDEVICELIST>() as u32,
        ) == u32::MAX
        {
            return devices;
        }

        if device_count == 0 {
            return devices;
        }

        let mut device_list = vec![RAWINPUTDEVICELIST::default(); device_count as usize];
        let result = GetRawInputDeviceList(
            Some(device_list.as_mut_ptr()),
            &mut device_count,
            std::mem::size_of::<RAWINPUTDEVICELIST>() as u32,
        );

        if result == u32::MAX {
            return devices;
        }

        for device in device_list.iter().take(device_count as usize) {
            if device.dwType != RIM_TYPEHID {
                continue;
            }

            let handle = device.hDevice;

            // Get device info
            let mut size = 0u32;
            if GetRawInputDeviceInfoW(Some(handle), RIDI_DEVICEINFO, None, &mut size) != 0 {
                continue;
            }

            let mut buffer = vec![0u8; size as usize];
            if GetRawInputDeviceInfoW(
                Some(handle),
                RIDI_DEVICEINFO,
                Some(buffer.as_mut_ptr() as *mut _),
                &mut size,
            ) == u32::MAX
            {
                continue;
            }

            let device_info = &*(buffer.as_ptr() as *const RID_DEVICE_INFO);
            if device_info.dwType != RIM_TYPEHID {
                continue;
            }

            let hid = device_info.Anonymous.hid;

            // Get device name
            let mut name_size = 0u32;
            if GetRawInputDeviceInfoW(Some(handle), RIDI_DEVICENAME, None, &mut name_size) != 0 {
                continue;
            }

            let mut name_buffer = vec![0u16; name_size as usize];
            if GetRawInputDeviceInfoW(
                Some(handle),
                RIDI_DEVICENAME,
                Some(name_buffer.as_mut_ptr() as *mut _),
                &mut name_size,
            ) == u32::MAX
            {
                continue;
            }

            let device_path = String::from_utf16_lossy(&name_buffer);
            let device_name = extract_device_name(&device_path);

            // Parse VID/PID from device path
            let (vid, pid) =
                if let Some((v, p)) = RawInputHandler::parse_vid_pid_from_path(&device_path) {
                    (v, p)
                } else {
                    continue;
                };

            devices.push(crate::gui::device_manager_dialog::HidDeviceInfo {
                vid,
                pid,
                device_name,
                usage_page: hid.usUsagePage,
                usage: hid.usUsage,
            });
        }
    }

    devices
}

impl RawInputHandler {
    /// Parses VID and PID from a device path.
    /// Device path format: \\?\HID#VID_045E&PID_028E#...
    fn parse_vid_pid_from_path(path: &str) -> Option<(u16, u16)> {
        let upper = path.to_uppercase();

        let vid_pos = upper.find("VID_")?;
        let pid_pos = upper.find("PID_")?;

        let vid_str = &upper[vid_pos + 4..].split(&['&', '#'][..]).next()?;
        let pid_str = &upper[pid_pos + 4..].split(&['&', '#'][..]).next()?;

        let vid = u16::from_str_radix(vid_str, 16).ok()?;
        let pid = u16::from_str_radix(pid_str, 16).ok()?;

        Some((vid, pid))
    }
}

/// Extracts a human-readable device name from the device path.
fn extract_device_name(path: &str) -> String {
    if let Some(name_part) = path.split('#').nth(1) {
        name_part.replace('_', " ").to_string()
    } else {
        "Unknown HID Device".to_string()
    }
}

/* ═══════════ ★v21.7 方案A: 第三方 HID 手柄 → 标准布局 (HidP 语义路线) ═══════════
 *
 * 第三方 HID 手柄只能经 RawInput 拿到原始报文, button_id 无语义, 用户必须逐个按键
 * 手动捕获。本功能把它"翻译"成标准布局, 支持一键生成整套映射。
 *
 * ⚠ 路线选择 (本机实测, 2026-09-11):
 *   目标设备 20BC:5159 (A1 手柄) **不支持** IOCTL_HID_GET_REPORT_DESCRIPTOR ——
 *   CreateFile 成功但 DeviceIoControl 恒返回 ERROR_INVALID_FUNCTION(1), 两种访问权限
 *   都试过。因此放弃"读描述符字节 + 自己算 bit 偏移"的方案, 改用 Windows 自带
 *   `HidP_*` 语义 API:
 *     · GetRawInputDeviceInfoW(RIDI_PREPARSEDDATA) 拿 preparsed 数据 (无需保持句柄)
 *     · HidP_GetButtonCaps  → 按钮 usage 列表 (+ 标准布局名)
 *     · HidP_GetValueCaps   → 轴 usage / 量程 (能力展示)
 *     · HidP_GetUsages      → 从每帧原始报文直接读出"当前按下的按钮 usage"(语义)
 *   好处: 由 Windows 解析描述符 → 天然处理 Report ID / bit 打包 / 厂商怪癖, 且可现场
 *   验证。触发键名 = `GAMEPAD_<VID>_<PID>_<SER|DEV%08X>_H<usage>` (见 hid_layout)。
 */

/// 一个原始 HID 手柄的能力 (HidP 语义, 用于 UI 展示与一键生成)。
#[derive(Debug, Clone)]
pub struct RawHidGamepad {
    pub vid: u16,
    pub pid: u16,
    pub device_name: String,
    pub serial: Option<String>,
    /// 稳定设备指纹低 32 位 (button_id 高 32 位; 触发键名里 DEV 段的来源)。
    pub stable_id_low32: u32,
    /// 按钮: (HID usage, 标准布局名 Option)
    pub buttons: Vec<(u32, Option<&'static str>)>,
    /// 轴展示名 (左摇杆·X / 十字键 …)
    pub axes: Vec<String>,
    /// 轴的 HID usage (page 0x01), 用于方向槽位绑定 (X/Y/Rx/Ry/Hat)
    pub axis_usages: Vec<u16>,
    /// 输入报文字节长度 (HidP caps)
    pub report_bytes: u16,
    /// preparsed 数据是否成功解析
    pub preparsed_ok: bool,
}

/// 生成的一条标准布局映射 (展示名, 触发键名)。
#[derive(Debug, Clone)]
pub struct StandardLayoutEntry {
    pub std_name: String,
    pub trigger_name: String,
}

/// 取设备的 preparsed 数据拷贝 (`RIDI_PREPARSEDDATA`; 无需 CreateFile/保持句柄)。
fn read_preparsed_blob(handle: HANDLE) -> Option<Vec<u8>> {
    unsafe {
        let mut size = 0u32;
        let probe = GetRawInputDeviceInfoW(Some(handle), RIDI_PREPARSEDDATA, None, &mut size);
        /* 注意: 首次查询返回 0 且 size 被填入 (与 RIDI_DEVICEINFO 的语义不同) */
        if probe != 0 || size == 0 || size > 64 * 1024 {
            return None;
        }
        let mut blob = vec![0u8; size as usize];
        let got = GetRawInputDeviceInfoW(
            Some(handle),
            RIDI_PREPARSEDDATA,
            Some(blob.as_mut_ptr() as *mut _),
            &mut size,
        );
        if got == u32::MAX {
            return None;
        }
        blob.truncate(size as usize);
        Some(blob)
    }
}

/// 摇杆方向判定的死区: 相对轴位宽的 30%。
const STICK_DIR_THRESHOLD_RATIO: i64 = 3; // /10

/// 提取 page 0x01 轴规格 (usage, 位宽) —— 仅 X/Y/Rx/Ry/Hat。
fn axis_specs_from_preparsed(blob: &mut [u8]) -> Vec<(u16, u8)> {
    use windows::Win32::Devices::HumanInterfaceDevice as hid;
    let mut specs = Vec::new();
    unsafe {
        let pp = hid::PHIDP_PREPARSED_DATA(blob.as_mut_ptr() as isize);
        let mut caps = hid::HIDP_CAPS::default();
        if hid::HidP_GetCaps(pp, &mut caps).0 < 0 {
            return specs;
        }
        let nv = caps.NumberInputValueCaps as usize;
        if nv == 0 {
            return specs;
        }
        let mut arr = vec![hid::HIDP_VALUE_CAPS::default(); nv];
        let mut len = nv as u16;
        if hid::HidP_GetValueCaps(hid::HidP_Input, arr.as_mut_ptr(), &mut len, pp).0 < 0 {
            return specs;
        }
        for v in arr.iter().take(len as usize) {
            if v.UsagePage != 0x01 {
                continue;
            }
            let usage = if v.IsRange {
                v.Anonymous.Range.UsageMin
            } else {
                v.Anonymous.NotRange.Usage
            };
            if matches!(
                usage,
                crate::hid_layout::USAGE_X
                    | crate::hid_layout::USAGE_Y
                    | crate::hid_layout::USAGE_RX
                    | crate::hid_layout::USAGE_RY
                    | crate::hid_layout::USAGE_Z
                    | crate::hid_layout::USAGE_RZ
                    | crate::hid_layout::USAGE_HAT_SWITCH
            ) {
                specs.push((usage, v.BitSize.min(32) as u8));
            }
        }
    }
    specs
}

/// 由两根轴的**当前原始值与空闲基线之差**判定四方向位 (bit0=左 bit1=右 bit2=上 bit3=下)。
///
/// 用"与基线之差"而非逻辑量程, 好处: 不需要知道轴的符号约定/量程上下限, 对手柄断电前后
/// 也不会误判。`bit_size` 决定死区 (30% 半量程)。纯函数, 可单测。
fn stick_dirs_from_delta(
    x_cur: Option<u32>,
    x_base: Option<u32>,
    y_cur: Option<u32>,
    y_base: Option<u32>,
    bit_size: u8,
) -> u8 {
    let thr = (1i64 << (bit_size.saturating_sub(1)).min(30)) * STICK_DIR_THRESHOLD_RATIO / 10;
    let mut bits = 0u8;
    if let (Some(c), Some(b)) = (x_cur, x_base) {
        let d = c as i64 - b as i64;
        if d <= -thr {
            bits |= 0b0001; // 左
        } else if d >= thr {
            bits |= 0b0010; // 右
        }
    }
    if let (Some(c), Some(b)) = (y_cur, y_base) {
        let d = c as i64 - b as i64;
        /* HID 惯例: Y 正值朝下 → 基线之上 (d<0) = 上 */
        if d <= -thr {
            bits |= 0b0100; // 上
        } else if d >= thr {
            bits |= 0b1000; // 下
        }
    }
    bits
}

/// hat 值 (1..8, 顺时针自"上"; 0=中) → 4 位掩码 (bit0=上 bit1=右 bit2=下 bit3=左)。
fn dpad_bits_from_hat(hat: u8) -> u8 {
    match hat {
        1 => 0b0001,
        2 => 0b0011,
        3 => 0b0010,
        4 => 0b0110,
        5 => 0b0100,
        6 => 0b1100,
        7 => 0b1000,
        8 => 0b1001,
        _ => 0,
    }
}

/// 方向位变化 → 需要派发的 (轴 usage, 方向, 是否按下) 列表。
/// `prev`/`now` = (十字键4位, 左摇杆4位, 右摇杆4位); 摇杆位序 bit0=左 bit1=右 bit2=上 bit3=下。
/// 纯函数, 可单测。
fn axis_dir_transitions(
    prev: (u8, u8, u8),
    now: (u8, u8, u8),
) -> SmallVec<[(u16, u8, bool); 8]> {
    use crate::hid_layout::{USAGE_HAT_SWITCH, USAGE_RX, USAGE_RY, USAGE_X, USAGE_Y};
    let mut out = SmallVec::new();
    /* 左摇杆: 左右→X, 上下→Y */
    for (bit, usage, dir) in [
        (0b0001u8, USAGE_X, 0u8),
        (0b0010, USAGE_X, 1),
        (0b0100, USAGE_Y, 2),
        (0b1000, USAGE_Y, 3),
    ] {
        let was = prev.1 & bit != 0;
        let is = now.1 & bit != 0;
        if was != is {
            out.push((usage, dir, is));
        }
    }
    /* 右摇杆: 左右→Rx, 上下→Ry */
    for (bit, usage, dir) in [
        (0b0001u8, USAGE_RX, 0u8),
        (0b0010, USAGE_RX, 1),
        (0b0100, USAGE_RY, 2),
        (0b1000, USAGE_RY, 3),
    ] {
        let was = prev.2 & bit != 0;
        let is = now.2 & bit != 0;
        if was != is {
            out.push((usage, dir, is));
        }
    }
    /* 十字键: bit0=上(U) bit1=右(R) bit2=下(D) bit3=左(L) */
    for (bit, dir) in [(0b0001u8, 2u8), (0b0010, 1), (0b0100, 3), (0b1000, 0)] {
        let was = prev.0 & bit != 0;
        let is = now.0 & bit != 0;
        if was != is {
            out.push((USAGE_HAT_SWITCH, dir, is));
        }
    }
    out
}

/// 从 preparsed 数据提取 (按钮 usage+标准名, 轴展示名, 报文字节长度)。
fn capability_from_preparsed(blob: &mut [u8]) -> Option<(Vec<(u32, Option<&'static str>)>, Vec<String>, u16)> {
    use windows::Win32::Devices::HumanInterfaceDevice as hid;
    unsafe {
        let pp = hid::PHIDP_PREPARSED_DATA(blob.as_mut_ptr() as isize);
        let mut caps = hid::HIDP_CAPS::default();
        if hid::HidP_GetCaps(pp, &mut caps).0 < 0 {
            return None;
        }

        let mut buttons: Vec<(u32, Option<&'static str>)> = Vec::new();
        let n = caps.NumberInputButtonCaps as usize;
        if n > 0 {
            let mut arr = vec![hid::HIDP_BUTTON_CAPS::default(); n];
            let mut len = n as u16;
            if hid::HidP_GetButtonCaps(hid::HidP_Input, arr.as_mut_ptr(), &mut len, pp).0 >= 0 {
                for b in arr.iter().take(len as usize) {
                    if b.UsagePage != 0x09 {
                        continue;
                    }
                    let (umin, umax) = if b.IsRange {
                        let r = b.Anonymous.Range;
                        (r.UsageMin, r.UsageMax)
                    } else {
                        let r = b.Anonymous.NotRange;
                        (r.Usage, r.Usage)
                    };
                    /* 防御: 个别描述符给出荒谬的 range, 限幅 256 个 */
                    let hi = umax.min(umin.saturating_add(255));
                    for u in umin..=hi {
                        buttons.push((u as u32, crate::hid_layout::standard_button_name(u as u32)));
                    }
                }
            }
        }
        buttons.sort_by_key(|b| b.0);
        buttons.dedup_by_key(|b| b.0);

        let mut axes = Vec::new();
        let nv = caps.NumberInputValueCaps as usize;
        if nv > 0 {
            let mut arr = vec![hid::HIDP_VALUE_CAPS::default(); nv];
            let mut len = nv as u16;
            if hid::HidP_GetValueCaps(hid::HidP_Input, arr.as_mut_ptr(), &mut len, pp).0 >= 0 {
                for v in arr.iter().take(len as usize) {
                    if v.UsagePage != 0x01 {
                        continue;
                    }
                    let usage = if v.IsRange {
                        v.Anonymous.Range.UsageMin
                    } else {
                        v.Anonymous.NotRange.Usage
                    };
                    if let Some(kind) = crate::hid_layout::HidAxisKind::from_usage(usage) {
                        axes.push(kind.label().to_string());
                    }
                }
            }
        }
        axes.dedup();

        Some((buttons, axes, caps.InputReportByteLength))
    }
}

/// 稳定设备指纹低 32 位 (与 [`RawInputHandler::generate_stable_device_id`] 同规则)。
fn stable_id_low32_for(vid: u16, pid: u16, serial: Option<&str>) -> u32 {
    let real = serial
        .map(|s| !s.contains('&') && s.len() > 4)
        .unwrap_or(false);
    let full = if real {
        RawInputHandler::hash_vid_pid_serial(vid, pid, serial.unwrap_or(""))
    } else {
        RawInputHandler::hash_vid_pid(vid, pid)
    };
    full as u32
}

/// 枚举当前连接的 **原始 HID 手柄** (Gamepad/Joystick usage), 尽力解析其标准布局。
///
/// 只读探测; 任何一步失败都不会 panic, 对应字段留空/None。
pub fn enumerate_raw_hid_gamepads() -> Vec<RawHidGamepad> {
    let mut out: Vec<RawHidGamepad> = Vec::new();
    unsafe {
        let mut count: u32 = 0;
        if GetRawInputDeviceList(None, &mut count, std::mem::size_of::<RAWINPUTDEVICELIST>() as u32)
            == u32::MAX
            || count == 0
        {
            return out;
        }
        let mut list = vec![RAWINPUTDEVICELIST::default(); count as usize];
        if GetRawInputDeviceList(
            Some(list.as_mut_ptr()),
            &mut count,
            std::mem::size_of::<RAWINPUTDEVICELIST>() as u32,
        ) == u32::MAX
        {
            return out;
        }

        for dev in list.iter().take(count as usize) {
            if dev.dwType != RIM_TYPEHID {
                continue;
            }
            let handle = dev.hDevice;

            // 设备信息 (usage/VID/PID)
            let mut size = 0u32;
            if GetRawInputDeviceInfoW(Some(handle), RIDI_DEVICEINFO, None, &mut size) != 0 {
                continue;
            }
            let mut info_buf = vec![0u8; size as usize];
            if GetRawInputDeviceInfoW(
                Some(handle),
                RIDI_DEVICEINFO,
                Some(info_buf.as_mut_ptr() as *mut _),
                &mut size,
            ) == u32::MAX
            {
                continue;
            }
            let info = &*(info_buf.as_ptr() as *const RID_DEVICE_INFO);
            let hid = info.Anonymous.hid;
            let is_pad = matches!(
                (hid.usUsagePage, hid.usUsage),
                (0x01, 0x05) | (0x01, 0x04) | (0x01, 0x08) | (0x05, 0x05) | (0x05, 0x04) | (0x05, 0x01)
            );
            if !is_pad {
                continue;
            }

            // 设备路径 (用于打开并读描述符) + 名称
            let mut name_size = 0u32;
            if GetRawInputDeviceInfoW(Some(handle), RIDI_DEVICENAME, None, &mut name_size) != 0 {
                continue;
            }
            let mut name_buf = vec![0u16; name_size as usize];
            if GetRawInputDeviceInfoW(
                Some(handle),
                RIDI_DEVICENAME,
                Some(name_buf.as_mut_ptr() as *mut _),
                &mut name_size,
            ) == u32::MAX
            {
                continue;
            }
            let path = String::from_utf16_lossy(&name_buf);
            let path = path.trim_end_matches('\0').to_string();

            let vid = hid.dwVendorId as u16;
            let pid = hid.dwProductId as u16;
            let serial = RawInputHandler::get_device_serial_number(handle);
            let stable = stable_id_low32_for(vid, pid, serial.as_deref());

            /* HidP 语义能力 (RIDI_PREPARSEDDATA 拷贝 → HidP_GetCaps/GetButtonCaps/GetValueCaps) */
            let (buttons, axes, axis_usages, report_bytes, preparsed_ok) =
                match read_preparsed_blob(handle) {
                    Some(mut blob) => {
                        let axis_usages: Vec<u16> =
                            axis_specs_from_preparsed(&mut blob).iter().map(|(u, _)| *u).collect();
                        match capability_from_preparsed(&mut blob) {
                            Some((b, a, n)) => (b, a, axis_usages, n, true),
                            None => (Vec::new(), Vec::new(), axis_usages, 0, false),
                        }
                    }
                    None => (Vec::new(), Vec::new(), Vec::new(), 0, false),
                };

            /* 同 VID:PID 只保留一台 (多接口设备会重复出现) */
            if out.iter().any(|d| d.vid == vid && d.pid == pid) {
                continue;
            }
            out.push(RawHidGamepad {
                vid,
                pid,
                device_name: extract_device_name(&path),
                serial,
                stable_id_low32: stable,
                buttons,
                axes,
                axis_usages,
                report_bytes,
                preparsed_ok,
            });
        }
    }
    out
}

/// 按标准布局约定, 为一个已解析能力的手柄生成按钮映射触发键 (语义名 `..._H<usage>`)。
/// 只生成能被标准约定命名的按钮 (A/B/X/Y/LB/RB/LT/RT/Back/Start/L3/R3/Guide)。
pub fn build_standard_layout_entries(pad: &RawHidGamepad) -> Vec<StandardLayoutEntry> {
    use crate::hid_layout::{device_prefix, format_semantic_button_name};
    let prefix = device_prefix(&crate::state::DeviceType::Gamepad(pad.vid));
    let mut entries = Vec::new();
    for (usage, std_name) in &pad.buttons {
        let Some(std_name) = std_name else {
            continue;
        };
        entries.push(StandardLayoutEntry {
            std_name: std_name.to_string(),
            trigger_name: format_semantic_button_name(
                prefix,
                pad.vid,
                pad.pid,
                pad.serial.as_deref(),
                pad.stable_id_low32,
                *usage,
            ),
        });
    }
    entries
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vid_pid_serial_hash() {
        let vendor_id: u16 = 0x045E;
        let product_id: u16 = 0x0B05;
        let serial = "ABC123";

        let hash1 = RawInputHandler::hash_vid_pid_serial(vendor_id, product_id, serial);
        let hash2 = RawInputHandler::hash_vid_pid_serial(vendor_id, product_id, serial);
        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_vid_pid_serial_hash_different() {
        let vendor_id: u16 = 0x045E;
        let product_id: u16 = 0x0B05;

        let hash1 = RawInputHandler::hash_vid_pid_serial(vendor_id, product_id, "ABC123");
        let hash2 = RawInputHandler::hash_vid_pid_serial(vendor_id, product_id, "ABC124");
        assert_ne!(hash1, hash2);
    }

    #[test]
    fn test_device_capture_state_initialization() {
        let state = DeviceCaptureState::new();
        assert_eq!(state.frame_count, 0);
    }

    #[test]
    fn test_device_capture_state_add_frame() {
        let mut state = DeviceCaptureState::new();
        let data = vec![0, 0, 0, 0, 0, 0x01, 0x02, 0x03];

        state.add_frame(&data, 1000);
        assert_eq!(state.frame_count, 1);
        assert!(state.get_most_sustained_frame(1000).is_some());
    }

    #[test]
    fn test_device_capture_state_sustained_duration() {
        let mut state = DeviceCaptureState::new();
        let mut time = 1000u64;

        // Pattern A: 100ms duration (3 frames, 50ms apart)
        let pattern_a = vec![0, 0, 0, 0, 0, 0x01, 0x00];
        state.add_frame(&pattern_a, time);
        time += 50;
        state.add_frame(&pattern_a, time);
        time += 50;
        state.add_frame(&pattern_a, time);
        time += 50;

        // Pattern B: 200ms duration (3 frames, 100ms apart) - longest
        let pattern_b = vec![0, 0, 0, 0, 0, 0x03, 0x00];
        state.add_frame(&pattern_b, time);
        time += 100;
        state.add_frame(&pattern_b, time);
        time += 100;
        state.add_frame(&pattern_b, time);
        time += 100;

        // Pattern C: 30ms duration (2 frames, 30ms apart)
        let pattern_c = vec![0, 0, 0, 0, 0, 0x02, 0x00];
        state.add_frame(&pattern_c, time);
        time += 30;
        state.add_frame(&pattern_c, time);

        // Total unique patterns: pattern_a, pattern_b, pattern_c = 3 patterns
        assert_eq!(state.frame_count, 3);

        let sustained = state.get_most_sustained_frame(time).unwrap();
        assert_eq!(&sustained[5], &0x03); // Pattern B has longest duration
    }

    #[test]
    fn test_device_capture_state_capacity_limit() {
        let mut state = DeviceCaptureState::new();
        let data = vec![0, 0, 0, 0, 0, 0x01];

        // Add 32 frames
        for i in 0..32u8 {
            let mut pattern = data.clone();
            pattern[5] = i;
            state.add_frame(&pattern, i as u64 * 10);
        }
        assert_eq!(state.frame_count, 32);

        // Try to add 33rd frame - should be ignored
        let mut new_pattern = data.clone();
        new_pattern[5] = 0xFF;
        state.add_frame(&new_pattern, 1000);
        assert_eq!(state.frame_count, 32);
    }

    #[test]
    fn test_device_capture_state_joystick_scenario() {
        let mut state = DeviceCaptureState::new();
        let mut time = 1000u64;

        // Simulate joystick: Right (0x0C) for 50ms, then Right-Up (0x08) for 150ms

        // Right for 50ms (2 frames)
        let right = vec![0, 0, 0, 0, 0, 0x0C, 0x00];
        state.add_frame(&right, time);
        time += 25;
        state.add_frame(&right, time);
        time += 25;

        // Right-Up for 150ms (3 frames) - should be selected
        let right_up = vec![0, 0, 0, 0, 0, 0x08, 0x00];
        state.add_frame(&right_up, time);
        time += 50;
        state.add_frame(&right_up, time);
        time += 50;
        state.add_frame(&right_up, time);
        time += 50;

        // Total unique patterns: right and right_up = 2 patterns
        assert_eq!(state.frame_count, 2);

        let sustained = state.get_most_sustained_frame(time).unwrap();
        assert_eq!(&sustained[5], &0x08); // Right-Up is selected
    }

    #[test]
    fn test_device_hid_state_with_baseline() {
        let baseline = vec![0x00, 0xFF, 0x7F, 0xFF, 0x7F, 0x00, 0x80, 0x00];
        let state = DeviceHidState::with_baseline(baseline.clone());

        assert!(state.baseline_ready);
        assert_eq!(state.baseline_data, baseline);
        assert_eq!(state.last_data, baseline);
    }

    #[test]
    fn test_parse_device_id_with_serial() {
        let device_id = "045E:0B05:ABC123";
        let result = RawInputHandler::parse_device_id(device_id);

        assert!(result.is_some());
        let (vid, pid, serial) = result.unwrap();
        assert_eq!(vid, 0x045E);
        assert_eq!(pid, 0x0B05);
        assert_eq!(serial, Some("ABC123".to_string()));
    }

    #[test]
    fn test_parse_device_id_without_serial() {
        let device_id = "045E:0B05";
        let result = RawInputHandler::parse_device_id(device_id);

        assert!(result.is_some());
        let (vid, pid, serial) = result.unwrap();
        assert_eq!(vid, 0x045E);
        assert_eq!(pid, 0x0B05);
        assert!(serial.is_none());
    }

    #[test]
    fn test_parse_device_id_invalid() {
        assert!(RawInputHandler::parse_device_id("invalid").is_none());
        assert!(RawInputHandler::parse_device_id("").is_none());
        assert!(RawInputHandler::parse_device_id("ZZZZ:0B05").is_none());
    }

    /* ── ★v21.7 方案A: 语义标准布局生成 ── */

    /// 真实环境冒烟: 枚举 + HidP 语义能力解析全链路不得 panic (设备依硬件)。
    /// `cargo test -- --nocapture` 可看到本机实际结果, 用于现场核对。
    #[test]
    fn smoke_enumerate_and_parse_raw_hid_gamepads() {
        let pads = enumerate_raw_hid_gamepads();
        println!("[smoke] 检测到 {} 台原始 HID 手柄", pads.len());
        for p in &pads {
            let std_names: Vec<&str> = p.buttons.iter().filter_map(|(_, n)| *n).collect();
            println!(
                "[smoke] {:04X}:{:04X} {} -> preparsed={} 按钮={} 标准名={:?} 轴={:?} 报文={}字节",
                p.vid,
                p.pid,
                p.device_name,
                p.preparsed_ok,
                p.buttons.len(),
                std_names,
                p.axes,
                p.report_bytes
            );
        }
        // 无断言 (依硬件); 只要不 panic/OOB 即通过
    }

    /// 语义触发键名必须能被 `input_name_to_device` 解析回同一 button_id (生成的映射真能命中)。
    #[test]
    fn semantic_entries_round_trip_through_name_parser() {
        let low = 0x1234_5678u32;
        let pad = RawHidGamepad {
            vid: 0x20BC,
            pid: 0x5159,
            device_name: "Test Pad".into(),
            serial: None,
            stable_id_low32: low,
            buttons: vec![(1, Some("A")), (2, Some("B")), (3, Some("X")), (4, Some("Y"))],
            axes: vec!["左摇杆·X".into(), "左摇杆·Y".into()],
            axis_usages: vec![crate::hid_layout::USAGE_X, crate::hid_layout::USAGE_Y],
            report_bytes: 15,
            preparsed_ok: true,
        };
        let entries = build_standard_layout_entries(&pad);
        assert_eq!(entries.len(), 4);
        assert_eq!(entries[0].std_name, "A");
        let dev = crate::state::AppState::input_name_to_device(&entries[0].trigger_name).unwrap();
        match dev {
            crate::state::InputDevice::GenericDevice { button_id, .. } => {
                assert_eq!((button_id >> 32) as u32, low);
                let pos = (button_id & 0xFFFF_FFFF) as u32;
                assert_eq!(crate::hid_layout::semantic_button_usage(pos), Some(1));
            }
            other => panic!("期望 GenericDevice, 得到 {other:?}"),
        }
        // 非标准按钮 (usage 20 无标准名) 不生成
        let pad2 = RawHidGamepad {
            buttons: vec![(20, None)],
            ..pad
        };
        assert!(build_standard_layout_entries(&pad2).is_empty());
    }

    /* ── ★v21.7b 摇杆方向判定 (纯函数) ── */

    #[test]
    fn stick_dirs_from_delta_ignores_center_and_detects_four_ways() {
        let base = 32768u32;
        let thr = 32768i64 * 3 / 10; // 位宽 16 → 30% 半量程 ≈ 9830
        // 居中不动
        assert_eq!(stick_dirs_from_delta(Some(base), Some(base), Some(base), Some(base), 16), 0);
        // 右 (X 正向偏移)
        assert_eq!(
            stick_dirs_from_delta(Some((base as i64 + thr) as u32), Some(base), Some(base), Some(base), 16),
            0b0010
        );
        // 左
        assert_eq!(
            stick_dirs_from_delta(Some((base as i64 - thr) as u32), Some(base), Some(base), Some(base), 16),
            0b0001
        );
        // 下 (HID 惯例 Y 正 = 下)
        assert_eq!(
            stick_dirs_from_delta(Some(base), Some(base), Some((base as i64 + thr) as u32), Some(base), 16),
            0b1000
        );
        // 上
        assert_eq!(
            stick_dirs_from_delta(Some(base), Some(base), Some((base as i64 - thr) as u32), Some(base), 16),
            0b0100
        );
        // 左下同时亮
        assert_eq!(
            stick_dirs_from_delta(
                Some((base as i64 - thr * 2) as u32),
                Some(base),
                Some((base as i64 + thr * 2) as u32),
                Some(base),
                16
            ),
            0b1001
        );
        // 死区内不亮
        assert_eq!(
            stick_dirs_from_delta(Some(base + 100), Some(base), Some(base - 100), Some(base), 16),
            0
        );
        // 缺少基线 → 不判方向
        assert_eq!(stick_dirs_from_delta(Some(base + 20000), None, Some(base), None, 16), 0);
    }

    #[test]
    fn dpad_hat_maps_to_four_direction_bits() {
        use crate::hid_layout::USAGE_HAT_SWITCH;
        assert_eq!(dpad_bits_from_hat(0), 0); // 中
        assert_eq!(dpad_bits_from_hat(1), 0b0001); // 上
        assert_eq!(dpad_bits_from_hat(3), 0b0010); // 右
        assert_eq!(dpad_bits_from_hat(5), 0b0100); // 下
        assert_eq!(dpad_bits_from_hat(7), 0b1000); // 左
        assert_eq!(dpad_bits_from_hat(2), 0b0011); // 右上
        let t = axis_dir_transitions((0, 0, 0), (0b0001, 0, 0));
        assert_eq!(t.len(), 1);
        assert_eq!(t[0], (USAGE_HAT_SWITCH, 2, true)); // 十字·上 按下
    }

    #[test]
    fn axis_dir_transitions_reports_press_and_release() {
        use crate::hid_layout::{USAGE_RX, USAGE_RY, USAGE_X, USAGE_Y};
        // 左摇杆向右按下: bit1
        let t = axis_dir_transitions((0, 0, 0), (0, 0b0010, 0));
        assert_eq!(t.as_slice(), &[(USAGE_X, 1, true)]);
        // 再松开
        let t = axis_dir_transitions((0, 0b0010, 0), (0, 0, 0));
        assert_eq!(t.as_slice(), &[(USAGE_X, 1, false)]);
        // 同时左下
        let t = axis_dir_transitions((0, 0, 0), (0, 0b1001, 0));
        assert_eq!(t.len(), 2);
        assert!(t.contains(&(USAGE_X, 0, true))); // 左
        assert!(t.contains(&(USAGE_Y, 3, true))); // 下
        // 右摇杆: bit2=上 → Ry 上
        let t = axis_dir_transitions((0, 0, 0), (0, 0, 0b0100));
        assert_eq!(t.as_slice(), &[(USAGE_RY, 2, true)]);
    }
}
