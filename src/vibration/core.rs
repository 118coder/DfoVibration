//! 震动路线共享基础设施 (S1 / S4 两条引擎分离后的公共层)。
//!
//! 这里只放**与路线语义无关**的东西: 共享内存协议、事件常量、时间哨兵、
//! 日志、XInput 输出与写入去重、评分强度映射。
//! 两条引擎 (s1.rs / s4.rs) 各自持有自己的状态机与参数解读逻辑, 互不引用。

use std::sync::atomic::{AtomicU32, Ordering};

use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::UI::Input::XboxController::{XINPUT_STATE, XINPUT_VIBRATION};

pub(super) const VIB_SHM_MAGIC: u32 = 0x564F4656;
/// 必须与 DLL 源码 common/vib_protocol.h 的 VIB_SHM_VERSION 一致;
/// DLL 协议升级 (事件语义/参数槽位变化) 后旧版本直接拒接, 避免按旧语义误读
pub(super) const VIB_SHM_VERSION: u32 = 2;
pub(super) const VIB_RING_SIZE: usize = 256 * 1024;

/* ★S1 老方案 (v13.34/35 语义移植): 保持式标记位 —— 老 DLL stub10 的 CC 保持震
 * 带 0x20|0x40, 据此走 mode1 hold; 纯 0x20 (玩家 DOT 红字跳字) 走 mode3 衰减脉冲;
 * 0x04 (怪物出血/中毒跳字) 走 mode4 脉冲。 */
pub const FONT_OTHER: u32 = 0x40;

pub const FONT_ATTACK: u32 = 0x01;
pub const FONT_HIT: u32 = 0x02;
pub const FONT_EFFECT: u32 = 0x04;
pub const FONT_STATE: u32 = 0x08;
pub const FONT_SPECIAL: u32 = 0x10;
pub const FONT_HP: u32 = 0x20;

/* 高级 B 组六类 L/R 比例已移除(与基础区每项权重重复), 改用内置固定基准 */
const BASE_LR: [(f32, f32); 6] = [
    (0.25, 0.50),  /* 0 DOT */
    (0.45, 0.85),  /* 1 特殊攻击 */
    (0.80, 0.25),  /* 2 受击 */
    (0.35, 0.70),  /* 3 普通命中 */
    (0.30, 0.70),  /* 4 装备特效 */
    (0.60, 0.50),  /* 5 状态 */
];

pub(super) fn base_lr_for(item_idx: usize) -> (f32, f32) {
    let i = match item_idx {
        7 => 0,   /* DOT */
        10 => 1,  /* 特殊 */
        12 => 2,  /* 受击 */
        11 => 3,  /* 命中 */
        8 => 4,   /* 特效 */
        9 => 5,   /* 状态 */
        _ => 3,
    };
    BASE_LR[i]
}

/* 里程碑固定档位 */
pub(super) const MILESTONES: [(u32, u32); 4] = [(50, 0x1), (100, 0x2), (200, 0x4), (400, 0x8)];

/* ---------------- 事件类型 ---------------- */
pub(super) const VEV_FONT: u32 = 10;
pub(super) const VEV_COMBO: u32 = 9;
pub(super) const VEV_RANKING: u32 = 11;
pub(super) const VEV_TARGET_DIE: u32 = 2;  /* 怪物死亡 (OnTargetDie hook 0x7884C0) */
pub(super) const VEV_KILLPOINT: u32 = 12;
pub(super) const VEV_DODGE: u32 = 13;
pub(super) const VEV_CRIT: u32 = 14;
pub(super) const VEV_BREAK: u32 = 15;
pub(super) const VEV_BACK: u32 = 16;
pub(super) const VEV_FINAL_KILL: u32 = 17;
pub(super) const VEV_AERIAL: u32 = 18;
pub(super) const VEV_ARMOR_BREAK: u32 = 19;
pub(super) const VEV_BUFF_STACK: u32 = 20;
pub(super) const VEV_KILL: u32 = 21;          /* 释放技能 (n2500[5747]) */
pub(super) const VEV_SHAKE_SCREEN: u32 = 22;  /* 真镜头震动 [shake screen] */
pub(super) const VEV_MOVE: u32 = 23;          /* 移动持续震动 */
pub(super) const VEV_SKILL_HIT: u32 = 24;     /* 技能震动 */
pub(super) const VEV_CRIT_SHAKE: u32 = 25;    /* 暴击/击杀特写震屏 */

/* 评分震动映射: 测试模式全强度 (0~8 全部满震, 便于感知事件是否生效)
 * 等级: 0=SSS 1=SS 2=S 3=A 4=B 5=C 6=D 7=E 8=F (数字越低评价越高) */
pub(super) fn rank_strength(level: u32) -> f32 {
    match level {
        0..=8 => 1.0,
        _ => 0.0,
    }
}

/* 评分细分事件强度: 测试模式全强度 */
pub(super) fn rank_type_strength(etype: u32) -> f32 {
    match etype {
        VEV_KILLPOINT => 1.0,   /* 击杀点 */
        VEV_DODGE => 1.0,       /* 极限闪避 */
        VEV_CRIT => 1.0,        /* 暴击 */
        VEV_BREAK => 1.0,       /* 破招 */
        VEV_BACK => 1.0,        /* 背击 */
        VEV_FINAL_KILL => 1.0,  /* 最终击杀 */
        VEV_AERIAL => 1.0,      /* 凌空追击 */
        VEV_ARMOR_BREAK => 1.0, /* 破甲 */
        VEV_BUFF_STACK => 1.0,  /* 增益叠加 */
        VEV_KILL => 1.0,        /* 释放技能 */
        VEV_SHAKE_SCREEN => 1.0,/* 镜头震动 */
        VEV_MOVE => 1.0,        /* 移动 */
        VEV_SKILL_HIT => 1.0,   /* 技能震动 */
        VEV_CRIT_SHAKE => 1.0,  /* 暴击/击杀特写 */
        _ => 0.0,
    }
}

/// 事件是否计入「累计事件」显示。
/// 移动洪流 (6ms 续期, 10 分钟 ≈ 6 万条) 两种模式都不计入总数;
/// S1 额外只计真实注入的震动 (老宿主同源口径), S4 计全部到达事件。
pub(super) fn counts_toward_event_total(etype: u32, legacy: bool, last_injected: bool) -> bool {
    etype != VEV_MOVE && (!legacy || last_injected)
}

/* ---------------- 时间 ---------------- */
pub(super) fn now_ms() -> u32 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u32)
        .unwrap_or(0)
}

pub fn now_ms_u64() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// u32 毫秒时间戳的回绕安全比较: now_ms 截断 u32, 49.7 天回绕一次。
/// "未过期"判定 = (deadline - now) 的有符号差 > 0, 在 ±2^31 ms 内恒正确;
/// 裸比较 `now < deadline` 在回绕点后会恒真 → 震动卡死最长 49.7 天。
#[inline]
pub(super) fn before(now: u32, deadline: u32) -> bool {
    deadline.wrapping_sub(now) as i32 > 0
}

/// ★v24.11: 评分脉冲窗口是否激活 —— **必须带 0 哨兵守卫**。
///
/// `rank_full_until` 初始/复位为 0 (= 没有评分族事件)。裸 `before(now, 0)` 在 `now` 的低 31 位
/// 为负时恒为真 (u32 回绕语义), 会让输出每帧都走"评分分支" → **移动分支成为死代码**
/// (实机: 城镇/副本走路都不震, 但战斗/评分照震 —— 因为评分分支把战斗输出 max 了进来)。
/// 该陷阱只在 `now_ms()` 落进"负半区"的约 24.8 天窗口内暴露, 故长期未被发现。
#[inline]
pub(super) fn rank_window_active(now: u32, rank_full_until: u32) -> bool {
    rank_full_until != 0 && before(now, rank_full_until)
}

/// ★v24.11: 计时器"已到期 **或从未设置 (0)**" —— 用于需要"首次使用即武装"的步频/节奏计时器。
/// 裸 `!before(now, 0)` 恒为 false → 计时器永不武装; 而 `0 - now` 还会下溢。
#[inline]
pub(super) fn expired_or_unset(now: u32, deadline: u32) -> bool {
    deadline == 0 || !before(now, deadline)
}

/* ---------------- 日志 ---------------- */

/// ★R2.12 (优化 9) 日志级别。
///
/// 为什么需要: 原来普通状态、调试细节、异常全塞进同一个 `SorahkDFO_vib.log`,
/// 排障要在几万行里翻。现在每行打级别标签, `Debug` 默认不写, 出问题直接筛
/// `[WARN]` / `[ERROR]` 即可; `ERROR` 额外复制一份到 `SorahkDFO_crash.log`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LogLevel {
    Debug = 0,
    Info = 1,
    Warn = 2,
    Error = 3,
}

impl LogLevel {
    #[inline]
    pub fn tag(self) -> &'static str {
        match self {
            LogLevel::Debug => "DEBUG",
            LogLevel::Info => "INFO",
            LogLevel::Warn => "WARN",
            LogLevel::Error => "ERROR",
        }
    }
}

static VIB_LOG_PATH: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();

/* ★v24.31 日志轮转 (QKeyMapper 同款) 保留为"单日体积"兜底:
 * 现在按天切文件, 单日仍可能被高频事件刷爆, 故 32MB × 3 再切一层。
 * 字节数用原子计数器累计 (避免每行日志都 stat 文件); 进程重启时按文件实际大小校准。 */
const VIB_LOG_ROTATE_BYTES: u64 = 32 * 1024 * 1024;
const VIB_LOG_KEEP: usize = 3;
static VIB_LOG_BYTES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
/// 当前 `SorahkDFO_vib.log` 所属的「纪元日」(自 1970-01-01 起的天数)。
/// 0 = 还没初始化; 跨天时把旧文件改名成带日期的归档再重新计数。
static VIB_LOG_DAY: AtomicU32 = AtomicU32::new(0);
/// 写入级别下限 (默认 Info: Debug 不落盘)。可被 [`set_log_level`] 或环境变量覆盖。
static VIB_LOG_MIN: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(1);

/// `SORAHK_LOG_DEBUG=1` 时把 Debug 也写出来 (只读一次环境变量)。
fn debug_log_enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| {
        std::env::var("SORAHK_LOG_DEBUG")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false)
    })
}

/// 设置落盘级别下限 (Info 为默认; 排查时可在设置页/环境变量切到 Debug)。
pub fn set_log_level(level: LogLevel) {
    VIB_LOG_MIN.store(level as u8, Ordering::Relaxed);
}

#[inline]
pub fn log_level() -> LogLevel {
    match VIB_LOG_MIN.load(Ordering::Relaxed) {
        0 => LogLevel::Debug,
        1 => LogLevel::Info,
        2 => LogLevel::Warn,
        _ => LogLevel::Error,
    }
}

/// 自 1970-01-01 起的天数 (按 UTC 切日; 只用日期, 不需要时区精确)。
fn day_index_now() -> u32 {
    (std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
        / 86_400) as u32
}

/// 纪元日 → (年, 月, 日)。Howard Hinnant 的 civil_from_days, 纯整数无分支表。
fn civil_from_days(days: u32) -> (i32, u32, u32) {
    let z = days as i64 + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as i64; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y } as i32, m, d)
}

/// 跨天时把 `SorahkDFO_vib.log` 改名成 `SorahkDFO_vib-YYYY-MM-DD.log` 再开新文件。
/// 当前日志路径始终不变 —— 使用说明里让用户发的那份文件永远是最新的。
fn rotate_by_day_if_needed(path: &std::path::Path) {
    let today = day_index_now();
    let prev = VIB_LOG_DAY.swap(today, Ordering::Relaxed);
    if prev == today {
        return;
    }
    if prev != 0 {
        let (y, m, d) = civil_from_days(prev);
        let stem = path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "SorahkDFO_vib".to_string());
        let dir = path.parent().map(|p| p.to_path_buf()).unwrap_or_default();
        let archived = dir.join(format!("{stem}-{y:04}-{m:02}-{d:02}.log"));
        let _ = std::fs::rename(path, &archived);
    }
    /* 新的一天: 文件已被挪走, 计数器归零; 没有旧文件时按实际大小校准 */
    let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    VIB_LOG_BYTES.store(size, Ordering::Relaxed);
}

/// 按级别写一行日志。低于下限的级别直接丢弃 (Debug 还需 `SORAHK_LOG_DEBUG=1`)。
pub fn vib_log_at(level: LogLevel, msg: &str) {
    if level == LogLevel::Debug && !debug_log_enabled() {
        return;
    }
    if (level as u8) < VIB_LOG_MIN.load(Ordering::Relaxed) {
        return;
    }
    let path = VIB_LOG_PATH.get_or_init(|| {
        let p = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.join("SorahkDFO_vib.log")))
            .unwrap_or_else(|| std::path::PathBuf::from("SorahkDFO_vib.log"));
        // 首次使用按文件真实大小校准计数器 (进程重启不丢轮转节奏)
        let size = std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0);
        VIB_LOG_BYTES.store(size, Ordering::Relaxed);
        VIB_LOG_DAY.store(day_index_now(), Ordering::Relaxed);
        p
    });
    rotate_by_day_if_needed(path);
    // 轮转检查: 常路径只花一次原子读; 真超限才做移位 (罕见)
    let size =
        VIB_LOG_BYTES.fetch_add(msg.len() as u64 + 32, Ordering::Relaxed);
    if size >= VIB_LOG_ROTATE_BYTES {
        crate::util::rotate_log_if_needed(path, VIB_LOG_ROTATE_BYTES, VIB_LOG_KEEP);
        VIB_LOG_BYTES.store(0, Ordering::Relaxed);
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        use std::io::Write;
        let _ = writeln!(f, "[{}] [{}] {}", now_ms(), level.tag(), msg);
    }
    /* 异常另存一份到 crash.log —— 用户报问题只发一个文件也能看到硬错误 */
    if level == LogLevel::Error {
        crate::util::crash_log("ERROR", msg);
    }
}

/// 常规状态日志 (INFO)。
pub fn vib_log(msg: &str) {
    vib_log_at(LogLevel::Info, msg);
}

/// 调试细节 (默认不落盘, `SORAHK_LOG_DEBUG=1` 才写)。
pub fn vib_debug(msg: &str) {
    vib_log_at(LogLevel::Debug, msg);
}

/// 需要用户/开发者注意但不影响运行的事件 (保护触发、事件被丢弃等)。
pub fn vib_warn(msg: &str) {
    vib_log_at(LogLevel::Warn, msg);
}

/// 硬错误 (同时进 crash.log)。
pub fn vib_error(msg: &str) {
    vib_log_at(LogLevel::Error, msg);
}

/* ★v24.43c: **节流日志** —— 高频决策点 (FONT 到达 / 注入 / 丢弃) 每秒最多写一行。
 *
 * 为什么需要: FONT 事件是每秒几十条的洪流 (召唤/精灵骑士可达 100 连击/秒), 逐条打日志
 * 会瞬间把 vib.log 刷成几百 MB 并拖慢注入路径本身。但"完全不打"又让关键决策不可见
 * (开发者反馈: S4 很多东西没日志)。折中 = 每个决策点各保留一个秒级配额通道。 */
pub(super) static LOG_BUDGET_FONT_EVT: AtomicU32 = AtomicU32::new(0);
pub(super) static LOG_BUDGET_FONT_INJ: AtomicU32 = AtomicU32::new(0);
pub(super) static LOG_BUDGET_FONT_DROP: AtomicU32 = AtomicU32::new(0);
pub(super) static LOG_BUDGET_OUT: AtomicU32 = AtomicU32::new(0);

/// 每秒配额: 同 slot 同一秒内只放行第一条。
#[inline]
pub(super) fn log_budget(slot: &AtomicU32, now: u32) -> Option<u32> {
    let sec = now / 1000;
    /* 跨秒才放行: 上一次写入的秒 != 当前秒。同秒内的并发调用可能都看到旧值而
     * 各写一条 (无锁, 不做 CAS 循环) —— 每秒多几条日志对诊断无影响。 */
    (slot.swap(sec, Ordering::Relaxed) != sec).then_some(0)
}

/* ---------------- XInput 输出 (写入去重) ---------------- */
/// ★v24.41 写入去重状态: 最近一次**真正**写进 XInput 的 (slot, L, R) 打包值。
/// u64::MAX = 从未写过 (哨兵)。打包 = slot<<32 | L<<16 | R。
static LAST_XINPUT_WRITE: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(u64::MAX);
/// 最近一次真正写入的时间戳 (now_ms; 0 = 从未)。非零输出的周期刷新用。
static LAST_XINPUT_WRITE_AT: AtomicU32 = AtomicU32::new(0);

/// 非零输出的周期刷新间隔 ms —— 部分国产/蓝牙手柄驱动需要定期重写才保持震动;
/// 值不变也每 250ms 补写一次 (对别的程序的影响可忽略: 250ms vs 旧实现 15ms)。
/// 归零输出**不刷新** (空闲期一个字节都不写, 见 xinput_write_needed)。
const XINPUT_REFRESH_MS: u32 = 250;

/// 写入是否需要发送 (纯函数, 便于单测)。
///
/// 规则 (v24.41 "不抢别人的手柄" 语义):
/// - 目标非零: 与上次写的完全相同且未到刷新间隔 → 跳过; 变化或到刷新间隔 → 写;
/// - 目标全零: **从未写过或上次已是全零 → 跳过** (空闲期一个字节都不写,
///   别的程序 (手柄基准/映射工具的震动测试) 写进马达的值不会被我们踩掉);
///   仅当上次写过非零值时写这一次 —— 那是"释放" (不把马达留在旧震动上)。
#[inline]
fn xinput_write_needed(
    last_packed: u64,
    last_at: u32,
    now: u32,
    slot: u32,
    left: u16,
    right: u16,
) -> bool {
    let packed = pack_xinput_write(slot, left, right);
    if left == 0 && right == 0 {
        last_packed != u64::MAX && last_packed != packed
    } else {
        last_packed != packed || now.wrapping_sub(last_at) >= XINPUT_REFRESH_MS
    }
}

#[inline]
fn pack_xinput_write(slot: u32, left: u16, right: u16) -> u64 {
    ((slot as u64) << 32) | ((left as u64) << 16) | right as u64
}

/// ★v24.41: 外部直写马达后作废去重缓存 (设备管理弹窗的"测试震动"绕过引擎
/// 直接调用 XInputSetState —— 不作废的话引擎缓存的"上次值"与实际马达状态
/// 不一致: 例如引擎缓存 (0,0) 但弹窗写了满幅, 引擎下次想写 (0,0) 会被去重
/// 跳过, 马达永远停在测试值上)。任何引擎之外的写入方都必须调用本函数。
pub fn invalidate_xinput_write_cache() {
    LAST_XINPUT_WRITE.store(u64::MAX, Ordering::Relaxed);
    LAST_XINPUT_WRITE_AT.store(0, Ordering::Relaxed);
}

/// 向"当前已连接"的 XInput 槽位输出震动 (每次输出时扫描 0..4)。
/// 旧实现硬编码 0 号槽 —— 非 0 号槽位的手柄收不到任何游戏震动。
pub(super) fn send_vibration(left: u16, right: u16) {
    for slot in 0..4u32 {
        let mut st = XINPUT_STATE::default();
        if unsafe { crate::xinput::xinput_get_state(slot, &mut st) } == 0 {
            let now = now_ms();
            if !xinput_write_needed(
                LAST_XINPUT_WRITE.load(Ordering::Relaxed),
                LAST_XINPUT_WRITE_AT.load(Ordering::Relaxed),
                now,
                slot,
                left,
                right,
            ) {
                return;
            }
            let vib = XINPUT_VIBRATION {
                wLeftMotorSpeed: left,
                wRightMotorSpeed: right,
            };
            unsafe {
                /* 仅在写入成功时记账 (失败时下次仍会尝试, 不丢输出) */
                if crate::xinput::xinput_set_state(slot, &vib) == 0 {
                    LAST_XINPUT_WRITE.store(pack_xinput_write(slot, left, right), Ordering::Relaxed);
                    LAST_XINPUT_WRITE_AT.store(now, Ordering::Relaxed);
                }
            }
            return;
        }
    }
    /* ★v24.43c 诊断: 4 个槽位都 get_state 失败 = 没有任何 XInput 设备被枚举到
     * (手柄没连/被独占/驱动问题) —— 此时引擎怎么算都没用。 */
    if left > 0 || right > 0 {
        vib_log(&format!(
            "[XW] NO-PAD L={} R={} (0..4 槽全 get_state 失败)",
            left, right
        ));
    }
}

/// ★v24.41: 退出路径的马达归零 —— 仅当"我们自己是最后写方且写的是非零值"时才写。
/// 平时不要用 (会破坏"不抢别人的手柄"语义)。
pub(super) fn send_vibration_force_zero() {
    let last = LAST_XINPUT_WRITE.load(Ordering::Relaxed);
    if last == u64::MAX {
        return; /* 本进程从未写过马达: 不碰别人的输出 */
    }
    let slot = (last >> 32) as u32;
    let l = ((last >> 16) & 0xFFFF) as u16;
    let r = (last & 0xFFFF) as u16;
    if l == 0 && r == 0 {
        return; /* 上次已是全零: 无需补写 */
    }
    let mut st = XINPUT_STATE::default();
    if unsafe { crate::xinput::xinput_get_state(slot, &mut st) } == 0 {
        let vib = XINPUT_VIBRATION {
            wLeftMotorSpeed: 0,
            wRightMotorSpeed: 0,
        };
        unsafe {
            let _ = crate::xinput::xinput_set_state(slot, &vib);
        }
        LAST_XINPUT_WRITE.store(u64::MAX, Ordering::Relaxed);
    }
}

/* ---------------- 进程存活 ---------------- */
pub(super) fn process_alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    unsafe {
        use windows::Win32::System::Threading::{
            GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
        };
        if let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
            if h.is_invalid() {
                return false;
            }
            let mut code: u32 = 0;
            let ok = GetExitCodeProcess(h, &mut code).is_ok();
            let _ = CloseHandle(h);
            ok && code == 259
        } else {
            false
        }
    }
}

/* ---------------- 共享内存结构 ---------------- */
#[repr(C)]
pub(super) struct VibEvent {
    pub(super) etype: u32,
    pub(super) strength: u32,
    pub(super) tick: u32,
    pub(super) reserved: u32,
}

#[repr(C)]
pub(super) struct VibRing {
    pub(super) head: AtomicU32,
    pub(super) tail: AtomicU32,
    pub(super) capacity: u32,
    pub(super) data: [u8; VIB_RING_SIZE],
}

#[repr(C)]
pub(super) struct VibShm {
    pub(super) magic: u32,
    pub(super) version: u32,
    pub(super) game_pid: u32,
    pub(super) flags: u32,
    pub(super) seq: u32,
    pub(super) last_tick: u32,
    pub(super) api_version: [u32; 7],
    pub(super) ring: VibRing,
}

const _: () = assert!(std::mem::size_of::<VibEvent>() == 16);
const _: () = assert!(std::mem::size_of::<VibShm>() == 262208);

/* ---------------- 测试 ---------------- */
#[cfg(test)]
mod event_count_tests {
    use super::*;

    #[test]
    fn move_stream_never_counts_toward_event_total() {
        // 移动流: 两种模式都不计入 (S4 曾漏排除 → 总事件暴增)
        assert!(!counts_toward_event_total(VEV_MOVE, false, true));
        assert!(!counts_toward_event_total(VEV_MOVE, true, true));
        // 普通战斗事件: 两种模式都计入
        assert!(counts_toward_event_total(VEV_FONT, false, true));
        assert!(counts_toward_event_total(VEV_RANKING, false, true));
        // S1 仍保留"只计真实注入"的老口径
        assert!(counts_toward_event_total(VEV_FONT, true, true));
        assert!(!counts_toward_event_total(VEV_FONT, true, false));
    }
}

#[cfg(test)]
mod time_sentinel_tests {
    use super::*;

    /// 用户日志里的真实时间戳 (u32 截断后 bit31=1)。这正是当年没暴露的原因:
    /// 只有 now 落在"负半区"时, 裸 before(now, 0) 才会恒真。
    const NOW: u32 = 2_517_474_289;

    #[test]
    fn bare_before_on_zero_is_the_trap() {
        assert!(
            before(NOW, 0),
            "裸 before(now,0) 在此时间戳下为真 —— 这就是把移动分支挡死的根因"
        );
    }

    #[test]
    fn rank_window_requires_nonzero_deadline() {
        assert!(!rank_window_active(NOW, 0), "0 = 无评分事件, 不得判为激活");
        assert!(!rank_window_active(0, 0));
        assert!(
            rank_window_active(NOW, NOW.wrapping_add(200)),
            "窗口内应激活"
        );
        assert!(
            !rank_window_active(NOW.wrapping_add(300), NOW.wrapping_add(200)),
            "已过期应关闭"
        );
        // 回绕边界仍按有符号差判定
        assert!(rank_window_active(u32::MAX - 10, 5), "回绕后仍在窗口内");
    }

    #[test]
    fn expired_or_unset_arms_on_first_use() {
        assert!(expired_or_unset(NOW, 0), "未设置 → 需要武装");
        assert!(!expired_or_unset(NOW, NOW.wrapping_add(200)), "未到期不武装");
        assert!(
            expired_or_unset(NOW.wrapping_add(300), NOW.wrapping_add(200)),
            "已到期 → 需要武装"
        );
    }
}

#[cfg(test)]
mod xinput_write_dedupe_tests {
    use super::*;

    #[test]
    fn zero_target_is_written_only_as_release() {
        /* 从未写过 → 不写 (不抢别人的手柄) */
        assert!(!xinput_write_needed(u64::MAX, 0, 1000, 0, 0, 0));
        /* 上次已是全零 → 不写 (空闲期零流量) */
        let zero = pack_xinput_write(0, 0, 0);
        assert!(!xinput_write_needed(zero, 900, 1000, 0, 0, 0));
        /* 上次非零 → 写一次释放 */
        let nz = pack_xinput_write(0, 500, 600);
        assert!(xinput_write_needed(nz, 900, 1000, 0, 0, 0));
    }

    #[test]
    fn nonzero_target_refreshes_periodically() {
        let nz = pack_xinput_write(0, 500, 600);
        /* 值变化 → 立即写 */
        assert!(xinput_write_needed(nz, 900, 1000, 0, 400, 600));
        /* 相同非零值、间隔未到 250ms → 不写 */
        assert!(!xinput_write_needed(nz, 900, 1000, 0, 500, 600));
        /* 相同非零值、到 250ms → 补写 (驱动保震) */
        assert!(xinput_write_needed(nz, 700, 1000, 0, 500, 600));
        /* 槽位不同视为不同写 */
        assert!(xinput_write_needed(nz, 900, 1000, 1, 500, 600));
    }
}

#[cfg(test)]
mod log_tests {
    use super::*;

    /// 纪元日换算必须与实际日期一致 —— 日志归档文件名靠它 (优化 9)。
    #[test]
    fn civil_days_round_trip_known_dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(19_723), (2024, 1, 1)); // 2024-01-01
        assert_eq!(civil_from_days(20_336), (2025, 9, 5)); // 2025-09-05 (闰年跨过)
        /* 2026-11-11 = 自 1970 起第 20768 天 */
        assert_eq!(civil_from_days(20_768), (2026, 11, 11));
        /* 闰日必须落在 2 月 29 —— 手写换算最容易在这里错一天 */
        assert_eq!(civil_from_days(19_782), (2024, 2, 29));
    }

    /// 级别标签是排障时的检索关键字, 改了会让「筛 WARN 以上」的用法失效。
    #[test]
    fn level_tags_are_stable() {
        assert_eq!(LogLevel::Debug.tag(), "DEBUG");
        assert_eq!(LogLevel::Info.tag(), "INFO");
        assert_eq!(LogLevel::Warn.tag(), "WARN");
        assert_eq!(LogLevel::Error.tag(), "ERROR");
        /* 顺序即严重度: 过滤靠 `(level as u8) < min` */
        assert!(LogLevel::Debug < LogLevel::Info);
        assert!(LogLevel::Info < LogLevel::Warn);
        assert!(LogLevel::Warn < LogLevel::Error);
    }

    /// 默认下限是 Info: Debug 不该落盘 (优化 9 的「普通调试默认不写」)。
    #[test]
    fn default_min_level_is_info() {
        /* 该用例不写盘, 只验证常量语义 (真实写到文件会污染发行目录) */
        assert_eq!(LogLevel::Info as u8, 1);
        assert!((LogLevel::Debug as u8) < (LogLevel::Info as u8));
    }
}
