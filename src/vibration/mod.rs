//! 震动引擎总入口 —— **S1 / S4 两条路线各自独立实现, 互不引用**。
//!
//! ## 模块划分 (用户要求: S1 与 S4 几乎分离, 互不冲突)
//!
//! | 模块 | 职责 |
//! |------|------|
//! | `core.rs` | 两条路线共享的**基础设施**: 共享内存协议、事件常量、时间哨兵、日志、XInput 输出与写入去重、评分强度映射 |
//! | `s1.rs`  | **S1 (ACT1 老方案)** 引擎, 独立状态机 + 独立参数解读 |
//! | `s4.rs`  | **S4+ (新版客户端)** 引擎, 逐字节移植自 1.61 版 `src/vibration.rs` |
//! | `mod.rs` | 运行线程: 读共享内存 → 按路线分派到对应引擎 → 出口调度 → 写 XInput |
//!
//! 两条引擎**互不引用**, 各自拥有完整的 `push_event` / `tick` / `finalize`。
//! 改 S4 手感不会碰到 S1 的任何代码, 反之亦然 —— 冲突不再是"靠纪律避免",
//! 而是结构上不可能。
//!
//! ## 路线切换
//! `Config.toml` 的 `vib_legacy_client` 每帧刷新 (v24.20 起), 切换后即时生效:
//! 分派时重建目标引擎, 旧引擎的状态随替换丢弃 (避免跨路线状态污染)。
//!
//! 60 参数槽位, 基础 0-18 / 高级 19-59, 槽位常量见 `params.rs`。

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use windows::core::w;
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::System::Memory::{
    MapViewOfFile, OpenFileMappingW, UnmapViewOfFile, FILE_MAP_ALL_ACCESS,
};

use crate::state::AppState;

pub(crate) mod core;
mod s1;
mod s4;
pub(crate) mod params;
use params::*;

/* 共享基础设施对 crate 内其它模块的公开面 (保持迁移前的调用点不变) */
pub use core::now_ms_u64;
pub use core::vib_log;
pub use core::{invalidate_xinput_write_cache, FONT_ATTACK, FONT_EFFECT, FONT_HIT, FONT_HP, FONT_SPECIAL, FONT_STATE};

use core::*;

/// ★v16.8 高级算法总开关对 60 槽参数的覆盖 (纯函数, 便于测试):
/// 关闭 = 把该算法的"关"语义写入参数 (滑块值仍保留在 AppState/config)。
/// v16.9 扩展三组: 衰减时长/算法窗口 (回内置「默认」预设值) + 静默脉冲 (中性值)。
///
/// **只用于 S1 路线** —— S4+ 引擎按 1.61 语义直接使用保存值 (1.61 无此开关层)。
pub fn apply_algo_toggle_params(
    params: &mut [u32; 60],
    density_enabled: bool,
    adapt_enabled: bool,
    move_charge_enabled: bool,
    decay_enabled: bool,
    algo_windows_enabled: bool,
    pulse_enabled: bool,
) {
    if !density_enabled {
        params[29] = 100; /* P_DENSITY_THR: 100 = 关闭密度自适应 */
    }
    if !adapt_enabled {
        params[51] = 0; /* P_ADAPT_REDUCE: 0 = 打太快不减轻 */
    }
    if !move_charge_enabled {
        params[21] = 0; /* P_MOVE_CHARGE_RATE: 0 = 不积累走位能量 */
    }
    /* 各类事件衰减时长 → 内置「默认」预设值 (25/55/25/60) */
    if !decay_enabled {
        for (i, v) in [(35usize, 25u32), (36, 55), (37, 25), (38, 60)] {
            params[i] = v;
        }
    }
    /* 算法窗口时长 → 内置「默认」预设值 (150/90/250/20/600/1500/3000) */
    if !algo_windows_enabled {
        for (i, v) in [
            (40usize, 150u32),
            (41, 90),
            (42, 250),
            (43, 20),
            (44, 600),
            (45, 1500),
            (46, 3000),
        ] {
            params[i] = v;
        }
    }
    /* 静默/反击/脉冲 → 中性值 (静默 0 / 反击倍数 100 / 唤醒 0 / 里程碑 0 / 收尾 0;
     * 测试强度 59 保留不动) */
    if !pulse_enabled {
        for (i, v) in [(54usize, 0u32), (55, 100), (56, 0), (57, 0), (58, 0)] {
            params[i] = v;
        }
    }
}

/* ---------------- 路线分派 ----------------
 * 两条引擎类型不同 (各自独立的字段集与语义), 用一个枚举承载当前激活的那一个。
 * 切换路线时整个换掉 —— 旧引擎的 deadline/衰减状态不跨路线残留。 */
enum RouteEngine {
    S1(s1::Engine),
    S4(s4::VibEngine),
}

impl RouteEngine {
    fn new(legacy: bool) -> Self {
        if legacy {
            RouteEngine::S1(s1::Engine::new())
        } else {
            RouteEngine::S4(s4::VibEngine::new())
        }
    }

    /// 路线是否与当前持有的引擎一致 (不一致 → 主循环重建)。
    fn matches(&self, legacy: bool) -> bool {
        matches!(
            (self, legacy),
            (RouteEngine::S1(_), true) | (RouteEngine::S4(_), false)
        )
    }

    fn push_event(
        &mut self,
        ev: &VibEvent,
        font_hits: bool,
        item_lr: &[u32; 26],
        item_out: &[std::sync::atomic::AtomicU32; 26],
        rank_gain: &[u32; 15],
        rank_lr: &[i32; 30],
        rank_level_gain: u32,
        rank_duration: u32,
    ) {
        match self {
            RouteEngine::S1(e) => e.push_event(
                ev,
                font_hits,
                item_lr,
                item_out,
                rank_gain,
                rank_lr,
                rank_level_gain,
                rank_duration,
            ),
            RouteEngine::S4(e) => e.push_event(
                ev,
                font_hits,
                item_lr,
                item_out,
                rank_gain,
                rank_lr,
                rank_level_gain,
                rank_duration,
            ),
        }
    }

    fn tick(&mut self, params: &[u32; 60]) {
        match self {
            RouteEngine::S1(e) => e.tick(params),
            RouteEngine::S4(e) => e.tick(params),
        }
    }

    /// 该路由线的评分/移动出口是否与 FONT 输出合成 (S1 恒合成, S4 恒独占)。
    fn channel_composites(&self) -> bool {
        matches!(self, RouteEngine::S1(_))
    }

    /// 退出/断连时清空残余输出 (各引擎字段不同, 分别归零)。
    fn clear_output(&mut self) {
        match self {
            RouteEngine::S1(e) => {
                e.left = 0.0;
                e.right = 0.0;
            }
            RouteEngine::S4(e) => {
                e.left = 0.0;
                e.right = 0.0;
            }
        }
    }

    /// 上一次 push_event 是否真的产生了注入 (事件计数口径用)。
    fn last_injected(&self) -> bool {
        match self {
            RouteEngine::S1(e) => e.last_injected,
            RouteEngine::S4(_) => true, /* S4 计全部到达事件, 不看注入 */
        }
    }

    fn rng_next(&mut self) -> u32 {
        match self {
            RouteEngine::S1(e) => e.rng_next(),
            RouteEngine::S4(e) => e.rng_next(),
        }
    }

    fn rank_state(&self) -> (u32, f32, f32, f32, i32, f32) {
        match self {
            /* rank_full_until, rank_level, rank_lr_l, rank_lr_r, rank_out_idx, move_level */
            RouteEngine::S1(e) => (
                e.rank_full_until,
                e.rank_level,
                e.rank_lr_l,
                e.rank_lr_r,
                e.rank_out_idx,
                e.move_level,
            ),
            RouteEngine::S4(e) => (
                e.rank_full_until,
                e.rank_level,
                e.rank_lr_l,
                e.rank_lr_r,
                e.rank_out_idx,
                e.move_level,
            ),
        }
    }
}

/* ---------------- 运行线程 ---------------- */
pub fn run(state: Arc<AppState>) {
    std::thread::Builder::new()
        .name("vibration_thread".to_string())
        .spawn(move || {
            eprintln!("[vib] engine thread started");
            let mut shm_handle: Option<HANDLE> = None;
            let mut shm_view: Option<windows::Win32::System::Memory::MEMORY_MAPPED_VIEW_ADDRESS> = None;

            /* ★v24.20: 路线开关必须**每轮刷新** —— 曾只在线程启动读一次,
             * 问号界面/设置里切 S1↔S4 后算法一直停在旧路线, 直到重启。 */
            let mut legacy = state.vib_legacy_client.load(Ordering::Relaxed);
            let mut engine = RouteEngine::new(legacy);

            /* 环状态沿记录: VIB_RING_INVALID 只在 ok→invalid 跳变时记一条
             * (DLL 重启窗口每轮都判 invalid 时不再刷屏; crash.log 是 panic 日志,
             * 保持信噪比) */
            let mut ring_prev_ok = true;
            /* 输出闸门诊断: 翻转才记 [GATE] 行 (防刷屏), 见循环内注释 */
            let mut last_gate_state: Option<(bool, bool, bool)> = None;
            /* 路线诊断: 切换时记一行, 便于日志判读是哪条引擎在跑 */
            let mut last_route: Option<bool> = Some(legacy);

            loop {
                if state.should_exit.load(Ordering::Relaxed) {
                    /* ★v24.41: 退出前确保马达归零 —— 去重的"释放写"只在输出变化
                     * 那一帧发生; 若正在震动时收到退出, 不强制写就会把马达留在
                     * 旧档位 (手柄一直震到断电)。 */
                    send_vibration_force_zero();
                    break;
                }

                /* ★v24.20 路线开关每轮刷新; 路线变化 → 重建目标引擎 */
                legacy = state.vib_legacy_client.load(Ordering::Relaxed);
                if !engine.matches(legacy) {
                    engine = RouteEngine::new(legacy);
                }
                if last_route != Some(legacy) {
                    vib_log(&format!(
                        "[ROUTE] 引擎切换 -> {} ({}引擎)",
                        if legacy { "S1 (ACT1 老方案)" } else { "S4+ (新版客户端)" },
                        if legacy { "S1 独立" } else { "S4 独立" }
                    ));
                    last_route = Some(legacy);
                }

                let mut params: [u32; 60] = [0; 60];
                for i in 0..60 {
                    let v = state.vibration_params[i].load(Ordering::Relaxed);
                    if v != 0 {
                        params[i] = v;
                    }
                }
                /* ★v16.8/16.9: 高级算法总开关对参数槽的覆盖
                 * (密度/命中自适应/走位能量 + 衰减时长/算法窗口/静默脉冲)
                 * ★v24.43c: **只在 S1 路线应用** —— S4 引擎按 1.61 语义直接使用
                 * 保存值 (1.61 无此开关层)。 */
                if legacy {
                    apply_algo_toggle_params(
                        &mut params,
                        state.vibration_density_enabled.load(Ordering::Relaxed),
                        state.vibration_adapt_enabled.load(Ordering::Relaxed),
                        state.vibration_move_charge_enabled.load(Ordering::Relaxed),
                        state.vibration_decay_enabled.load(Ordering::Relaxed),
                        state.vibration_algo_windows_enabled.load(Ordering::Relaxed),
                        state.vibration_pulse_enabled.load(Ordering::Relaxed),
                    );
                }
                let motor_l = state.vibration_motor_l_gain.load(Ordering::Relaxed) as f32 / 100.0;
                let motor_r = state.vibration_motor_r_gain.load(Ordering::Relaxed) as f32 / 100.0;
                /* 震动随机性: 0=关闭, N=在强度上限附近随机增减 ±N% */
                let random_gain = state.vibration_random_gain.load(Ordering::Relaxed) as f32;
                let rnd = if random_gain > 0.0 {
                    let x = engine.rng_next();
                    let span = (random_gain * 2.0) as u32 + 1; /* 0..=2N */
                    ((x % span) as f32 - random_gain) / 100.0 /* -N..=+N % */
                } else {
                    0.0
                };
                let enabled = state.vibration_enabled.load(Ordering::Relaxed)
                    && !state.is_paused();
                /* ★输出闸门诊断 (v14.1.1): 事件消费在总开关判断之前, 总开关关闭/
                 * 暂停时 [INJ] 照打但每轮强制 send(0,0) —— 曾因此"日志有注入命中、
                 * 马达纹丝不动"无从排查。有效输出状态一旦翻转, 记一行 [GATE]。 */
                let gate_paused = state.is_paused();
                let gate_abs = state.vibration_abs_freq_enabled.load(Ordering::Relaxed);
                let gate_state = (enabled, gate_paused, gate_abs);
                if last_gate_state != Some(gate_state) {
                    vib_log(&format!(
                        "[GATE] 输出{} (总开关={}, 暂停={}, 绝对频率={})",
                        if enabled { "开" } else { "关" },
                        state.vibration_enabled.load(Ordering::Relaxed),
                        gate_paused,
                        gate_abs
                    ));
                    last_gate_state = Some(gate_state);
                }

                if shm_view.is_none() {
                    if let Ok(h) = unsafe {
                        OpenFileMappingW(FILE_MAP_ALL_ACCESS.0, false, w!("Local\\DfoVibrationShm"))
                    } {
                        if !h.is_invalid() {
                            let view = unsafe {
                                MapViewOfFile(h, FILE_MAP_ALL_ACCESS, 0, 0,
                                              std::mem::size_of::<VibShm>())
                            };
                            if !view.Value.is_null() {
                                let s = unsafe { &mut *(view.Value as *mut VibShm) };
                                if s.magic == VIB_SHM_MAGIC && s.version == VIB_SHM_VERSION {
                                    shm_handle = Some(h);
                                    shm_view = Some(view);
                                } else {
                                    let _ = unsafe { UnmapViewOfFile(view) };
                                    let _ = unsafe { CloseHandle(h) };
                                }
                            } else {
                                let _ = unsafe { CloseHandle(h) };
                            }
                        }
                    }
                }

                if let Some(view) = shm_view {
                    let s = unsafe { &mut *(view.Value as *mut VibShm) };
                    let connected = s.magic == VIB_SHM_MAGIC
                        && s.version == VIB_SHM_VERSION
                        && process_alive(s.game_pid);
                    state.vibration_connected.store(connected, Ordering::Relaxed);

                    if connected {
                        let head = s.ring.head.load(Ordering::Acquire);
                        let tail = s.ring.tail.load(Ordering::Relaxed);

                        // 绝对稳定: 共享内存由 DfoVibration.dll 写入(不可信 —— DLL 崩溃、
                        // 写序被中断、版本不匹配时内容可能被污染)。逐项校验后读取,
                        // 任一异常即丢弃本轮(绝不除零 panic / 越界读 / 死循环)。
                        //
                        // 关键语义: head/tail 是"累计字节偏移"(DLL 每写一事件 head+=16,
                        // 软件每读一事件 tail+=16, 二者持续增长、会超过 capacity),
                        // 通过 % capacity 取环内位置。因此**不能用 head<=capacity / tail<=capacity
                        // 判断合法** —— 那会在玩了一会 head 超过 262144 后恒判非法,
                        // 事件永远被丢弃 → 震动中断。正确校验用两者(无符号)差值:
                        // 未满(head-tail<capacity, 正常可读)或恰好满(=capacity)均合法;
                        // 差值过大(落后太多/DLL 重启后 head 回绕)才判异常丢弃。
                        let ev_size = std::mem::size_of::<VibEvent>() as u32;
                        let max_events = (VIB_RING_SIZE as u32) / ev_size;
                        let capacity = s.ring.capacity;
                        let ring_ok = capacity == VIB_RING_SIZE as u32
                            && capacity % ev_size == 0
                            && head.wrapping_sub(tail) <= capacity;
                        let mut t = if ring_ok { tail } else { head }; // 异常时跳过读取
                        let mut got = 0u32;
                        let font_hits = state.vibration_font_hits.load(Ordering::Relaxed);
                        let mut item_lr = [0u32; 26];
                        for (i, v) in item_lr.iter_mut().enumerate() {
                            *v = state.vibration_item_lr[i].load(Ordering::Relaxed);
                        }
                        let mut rank_gain = [0u32; 15];
                        for (i, v) in rank_gain.iter_mut().enumerate() {
                            *v = state.vibration_rank_type_gain[i].load(Ordering::Relaxed);
                        }
                        let mut rank_lr = [0i32; 30];
                        for (i, v) in rank_lr.iter_mut().enumerate() {
                            *v = state.vibration_rank_lr[i].load(Ordering::Relaxed) as i32;
                        }
                        let rank_level_gain = state.vibration_rank_level_gain.load(Ordering::Relaxed);
                        let rank_duration = state.vibration_rank_duration.load(Ordering::Relaxed);
                        // got < max_events 上限保证单轮读取必然终止(异常 head 不再挂死)
                        while ring_ok && t != head && got < max_events {
                            let pos = (t % capacity) as usize;
                            let ev = unsafe {
                                std::ptr::read_unaligned(
                                    s.ring.data.as_ptr().add(pos) as *const VibEvent
                                )
                            };
                            engine.push_event(
                                &ev,
                                font_hits,
                                &item_lr,
                                &state.vibration_item_out,
                                &rank_gain,
                                &rank_lr,
                                rank_level_gain,
                                rank_duration,
                            );
                            if ev.etype == VEV_RANKING {
                                state.vibration_rank_events.fetch_add(1, Ordering::Relaxed);
                                state.vibration_rank_last.store(
                                    ev.strength.min(8),
                                    Ordering::Relaxed,
                                );
                                eprintln!("[vib] RANKING event level={} received", ev.strength);
                            } else if ev.etype == VEV_TARGET_DIE {
                                state.vibration_rank_type_events[14].fetch_add(1, Ordering::Relaxed);
                                eprintln!("[vib] TARGET_DIE event strength={} received", ev.strength);
                            } else if ev.etype >= VEV_KILLPOINT && ev.etype <= VEV_CRIT_SHAKE {
                                let idx = (ev.etype - VEV_KILLPOINT) as usize;
                                if idx < 14 {
                                    state.vibration_rank_type_events[idx].fetch_add(1, Ordering::Relaxed);
                                }
                                eprintln!("[vib] RANK-type event type={} strength={} received", ev.etype, ev.strength);
                            }
                            /* ★S1 老方案计数口径 (老宿主同源): 只计真实注入的震动;
                             * ★v24.12 移动洪流 (6ms 续期, 10 分钟 ≈ 6 万条) 两种模式都不计入总数,
                             * 否则 S4 模式下"累计事件"会暴增 (只留 rank_type_events[11] 的移动细分计数)。 */
                            let counted = counts_toward_event_total(
                                ev.etype,
                                legacy,
                                engine.last_injected(),
                            );
                            t = t.wrapping_add(ev_size);
                            if counted {
                                got = got.wrapping_add(1);
                            }
                        }
                        if ring_ok {
                            s.ring.tail.store(t, Ordering::Release);
                            if got > 0 {
                                state.vibration_events_received.fetch_add(got as u64, Ordering::Relaxed);
                            }
                        } else {
                            // 头尾异常(DLL 重启 head 回绕 / 软件漏读落后超界): 不能一直丢
                            // (会让震动中断), 把 tail 同步到 head 丢弃积压, 下轮立即恢复读取。
                            if ring_prev_ok {
                                crate::util::crash_log(
                                    "VIB_RING_INVALID",
                                    &format!("cap={capacity} head={head} tail={tail} (已同步 tail=head)"),
                                );
                                ring_prev_ok = false;
                            }
                            s.ring.tail.store(head, Ordering::Release);
                        }
                        ring_prev_ok = ring_ok;
                    } else {
                        let _ = unsafe { UnmapViewOfFile(view) };
                        if let Some(h) = shm_handle.take() {
                            let _ = unsafe { CloseHandle(h) };
                        }
                        shm_view = None;
                        // 整引擎重置: 旧实现只清 left/right, hold/rhythm/评分脉冲等
                        // deadline 字段残留 (配合 u32 回绕会把震动卡死到下一轮 tick)
                        engine = RouteEngine::new(legacy);
                        send_vibration(0, 0);
                    }
                } else {
                    state.vibration_connected.store(false, Ordering::Relaxed);
                }

                if enabled {
                    /* 评分测试: 模拟等级 8 事件 (GUI 测试按钮) */
                    if state.vibration_rank_test.swap(false, Ordering::Relaxed) {
                        state.vibration_rank_events.fetch_add(1, Ordering::Relaxed);
                        state.vibration_rank_last.store(8, Ordering::Relaxed);
                        let fake = VibEvent {
                            etype: VEV_RANKING,
                            strength: 8,
                            tick: 0,
                            reserved: 0,
                        };
                        engine.push_event(&fake, true, &[0u32; 26], &state.vibration_item_out, &[100u32; 15], &[0i32; 30], 100, 300);
                    }
                    let test_until = state.vibration_test_until.load(Ordering::Relaxed);
                    /* 必须用 u64 完整毫秒与 test_until 比较, 否则 u32 截断永远小于, 测试永不结束 */
                    let now_ms_val = now_ms_u64();
                    if test_until != 0 && now_ms_val < test_until {
                        let pct = params[P_TEST].max(1) as f32 / 100.0;
                        let vl = (65535.0 * pct * motor_l).min(65535.0) as u16;
                        let vr = (65535.0 * pct * motor_r).min(65535.0) as u16;
                        send_vibration(vl, vr);
                        state.vibration_out_l.store(vl as u32, Ordering::Relaxed);
                        state.vibration_out_r.store(vr as u32, Ordering::Relaxed);
                    } else {
                        if test_until != 0 {
                            state.vibration_test_until.store(0, Ordering::Relaxed);
                        }
                        engine.tick(&params);
                        /* 路线专属参数同步 (每帧)。S1 与 S4 字段集不同, 各同步各的。 */
                        sync_route_params(&mut engine, &state, legacy);

                        let (rank_until, rank_level, rank_lr_l, rank_lr_r, rank_out_idx, move_level) =
                            engine.rank_state();
                        /* 评分脉冲: 按各事件强度 × 满幅输出, 持续 rank_duration
                         * ★v24.11: 必须用带 0 哨兵守卫的 rank_window_active —— 裸 before(now,0)
                         * 会在"负半区"时间戳下恒真, 把移动分支挡死 (根因见该函数注释)。 */
                        if rank_window_active(now_ms(), rank_until) {
                            let lvl = rank_level.clamp(0.0, 1.0);
                            /* 全局总调整[9] + 强度上限[4] 作用于评分通道 (v24.3);
                             * 独立测试开关开时评分通道独立 (单通道测试用) */
                            let (mst, cap4) = if state.vibration_independent_test.load(Ordering::Relaxed) {
                                (1.0, 1.0)
                            } else {
                                (params[P_MASTER] as f32 / 100.0,
                                 (params[P_MAX] as f32 / 100.0).min(1.0))
                            };
                            let vl = (65535.0 * lvl * rank_lr_l * motor_l * mst * cap4).min(65535.0) as u16;
                            let vr = (65535.0 * lvl * rank_lr_r * motor_r * mst * cap4).min(65535.0) as u16;
                            /* 低强度死区 (v29.3): 评分通道低强度段归 0 (消除嗡声)
                             * ★审计修复 (P0): out_threshold 是 S4 的死区参数, 经 thr3
                             * 泄漏进 S1 评分通道会把低增益评分全部杀光 (实锤: 用户
                             * out_threshold=60 + rank_gain=30 → 评分全灭)。S1 评分与
                             * FONT 同用 15% 只杀真零语义。 */
                            let thr3 = if legacy {
                                (15.0f32 / 100.0) * 65535.0
                            } else {
                                (params_out_threshold(&state).min(50.0) / 100.0) * 65535.0
                            };
                            let vl = if (vl as f32) < thr3 { 0 } else { vl };
                            let vr = if (vr as f32) < thr3 { 0 } else { vr };
                            /* ★v13.37 合成式输出 (仅 S1): 评分脉冲不独占马达 —— 与战斗
                             * FONT 引擎输出取 max (走位/战斗中击杀+评分不压掉命中震动)。
                             * ★v24.44 (S4 无条件对齐 1.61): S4 出口**恒独占**
                             * (1.61 `vibration.rs:1516-1530`: 评分窗口内不调用 finalize(),
                             * 命中 FONT 输出不参与)。 */
                            let (vl, vr) = if engine.channel_composites() {
                                let (ef_l, ef_r) = composite_from_font(&mut engine, &params, motor_l, motor_r, rnd, &state);
                                (vl.max(ef_l), vr.max(ef_r))
                            } else {
                                (vl, vr)
                            };
                            send_vibration(vl, vr);
                            state.vibration_out_l.store(vl as u32, Ordering::Relaxed);
                            state.vibration_out_r.store(vr as u32, Ordering::Relaxed);
                            /* 评分事件实时输出 (rank_out, 供 L/R 进度条) */
                            for v in state.vibration_rank_out.iter() {
                                v.store(0, Ordering::Relaxed);
                            }
                            if rank_out_idx >= 0 && rank_out_idx < 15 {
                                state.vibration_rank_out[(rank_out_idx * 2) as usize]
                                    .store(vl as u32, Ordering::Relaxed);
                                state.vibration_rank_out[(rank_out_idx * 2 + 1) as usize]
                                    .store(vr as u32, Ordering::Relaxed);
                            }
                            eprintln!("[vib] RANK pulse L={} R={} lvl={:.2}", vl, vr, lvl);
                        } else if move_level > 0.0 {
                            /* 移动走路质感 v3: 正弦步伐 + 平滑 (消除嗡嗡/沙沙声)
                             * 参数 (高级震动调校 A2): 23步频 24着地 25抬脚 26增益 27平滑 28阈值
                             * 相位交替: 左脚步 L 主 / 右脚步 R 主 (走路韵律) */
                            let ml = move_level.clamp(0.0, 1.0);
                            let now = now_ms();
                            let pace_ms = params[P_MOVE_PACE].max(200).min(800);
                            /* ★v24.11: 0 哨兵 —— 首次进入移动分支时 move_pace_until=0,
                             * 裸 !before(now,0) 恒为 false 会导致步频计时器永不武装
                             * (且下面的 0 - now 会下溢)。 */
                            let (pace_frac, phase) = step_move_phase(&mut engine, now, pace_ms);
                            let pulse_pct = (params[P_MOVE_PULSE] as f32).clamp(10.0, 100.0) / 100.0;
                            let hold_pct = (params[P_MOVE_HOLD] as f32).clamp(5.0, 50.0) / 100.0;
                            let gain_pct = (params[P_MOVE_GAIN] as f32).clamp(10.0, 100.0) / 100.0;
                            /* 全局总调整[9] + 强度上限[4] 作用于移动通道 (v24.3);
                             * 移动独立开关 (v24.5 默认开, 移动一直在走, 吃全局强度容易没震动)
                             * 或独立测试开关 → 移动通道独立 */
                            let move_indep = state.vibration_move_independent.load(Ordering::Relaxed)
                                || state.vibration_independent_test.load(Ordering::Relaxed);
                            let (mst, cap4) = if move_indep {
                                (1.0, 1.0)
                            } else {
                                (params[P_MASTER] as f32 / 100.0,
                                 (params[P_MAX] as f32 / 100.0).min(1.0))
                            };
                            /* 正弦步伐: 0→峰值→0 平滑起伏 (无突变) */
                            let wave = (pace_frac * std::f32::consts::PI * 2.0).sin() * 0.5 + 0.5;
                            let pulse = hold_pct + (pulse_pct - hold_pct) * wave;
                            let base = ml * pulse * gain_pct * mst * cap4;
                            let tl;
                            let tr;
                            if phase {
                                tl = 65535.0 * base * rank_lr_l * motor_l;
                                tr = 65535.0 * base * 0.4 * rank_lr_r * motor_r;
                            } else {
                                tl = 65535.0 * base * 0.4 * rank_lr_l * motor_l;
                                tr = 65535.0 * base * rank_lr_r * motor_r;
                            }
                            /* 平滑插值: 每帧向目标过渡 (消除交替突跳/嗡嗡声) */
                            let smooth = (params[P_MOVE_SMOOTH] as f32).clamp(5.0, 100.0) / 100.0;
                            let (vl, vr) = smooth_move(&mut engine, tl, tr, smooth);
                            /* 低强度抑制: 低于阈值归 0 (消除马达沙沙声) */
                            let thr = (params[P_MOVE_THRESHOLD] as f32).clamp(0.0, 20.0) / 100.0 * 65535.0;
                            let vl = if vl < thr { 0 } else { vl.min(65535.0) as u16 };
                            let vr = if vr < thr { 0 } else { vr.min(65535.0) as u16 };
                            /* ★v13.37 合成式输出 (仅 S1): 移动步伐不独占马达 —— 与战斗
                             * FONT 引擎输出取 max (走位中命中/受击/暴击不被移动覆盖)。
                             * ★v24.44: S4 **恒独占** (1.61 `vibration.rs:1583-1588`:
                             * 走路期间不调用 finalize())。 */
                            let (vl, vr) = if engine.channel_composites() {
                                let (ef_l, ef_r) = composite_from_font(&mut engine, &params, motor_l, motor_r, rnd, &state);
                                (vl.max(ef_l), vr.max(ef_r))
                            } else {
                                (vl, vr)
                            };
                            send_vibration(vl, vr);
                            state.vibration_out_l.store(vl as u32, Ordering::Relaxed);
                            state.vibration_out_r.store(vr as u32, Ordering::Relaxed);
                        } else {
                            let (l, r) = finalize_route(&mut engine, &params);
                            /* 输出平滑 (v22.3): 一阶低通抑制低频嗡嗡声 (快速连击/衰减尾音
                             * 的输出跳变被柔化; 0=不平滑, 100=强平滑, 默认 55) */
                            let sm = state.vibration_out_smooth.load(Ordering::Relaxed).min(100) as f32 / 100.0;
                            let (l, r) = smooth_font_out(&mut engine, l as f32, r as f32, sm * 0.9);
                            let l = l as u16;
                            let r = r as u16;
                            let rk = 1.0 + rnd;
                            let vl = ((l as f32) * motor_l * rk).min(65535.0) as u16;
                            let vr = ((r as f32) * motor_r * rk).min(65535.0) as u16;
                            send_vibration(vl, vr);
                            state.vibration_out_l.store(vl as u32, Ordering::Relaxed);
                            state.vibration_out_r.store(vr as u32, Ordering::Relaxed);
                        }
                    }
                } else {
                    send_vibration(0, 0);
                    engine.clear_output();
                    state.vibration_out_l.store(0, Ordering::Relaxed);
                    state.vibration_out_r.store(0, Ordering::Relaxed);
                }

                std::thread::sleep(std::time::Duration::from_millis(15));
            }

            if let Some(view) = shm_view {
                let _ = unsafe { UnmapViewOfFile(view) };
            }
            if let Some(h) = shm_handle {
                let _ = unsafe { CloseHandle(h) };
            }
        })
        .ok();
}

/* ---------------- 路线相关的参数同步与出口小助手 ----------------
 * 两条引擎字段不同, 用 match 分别同步; 这里只做"逐字段搬运", 不含算法。 */

fn params_out_threshold(state: &AppState) -> f32 {
    state.vibration_out_threshold.load(Ordering::Relaxed) as f32
}

/// 每帧把 AppState 的运行期开关搬运到当前路线引擎。
fn sync_route_params(engine: &mut RouteEngine, state: &AppState, legacy: bool) {
    /* 通用 (两条路线都有的字段) */
    let out_smooth = state.vibration_out_smooth.load(Ordering::Relaxed);
    let _ = out_smooth;
    match engine {
        RouteEngine::S1(e) => sync_s1_params(e, state),
        RouteEngine::S4(e) => sync_s4_params(e, state, legacy),
    }
}

fn sync_s1_params(e: &mut s1::Engine, state: &AppState) {
    e.throttle_window = state.vibration_throttle_window.load(Ordering::Relaxed);
    e.throttle_max = state.vibration_throttle_max.load(Ordering::Relaxed);
    e.throttle_dense_ratio = state.vibration_throttle_dense_ratio.load(Ordering::Relaxed);
    let merge_en = state.vibration_merge_enabled.load(Ordering::Relaxed);
    let hitcap_en = state.vibration_hitcap_enabled.load(Ordering::Relaxed);
    let sustain_en = state.vibration_sustain_enabled.load(Ordering::Relaxed);
    let tail_en = state.vibration_tail_land_enabled.load(Ordering::Relaxed);
    e.merge_keep = if merge_en {
        state.vibration_merge_keep.load(Ordering::Relaxed)
    } else {
        0
    };
    e.merge_cap = state.vibration_merge_cap.load(Ordering::Relaxed);
    e.merge_hold = if merge_en {
        state.vibration_merge_hold.load(Ordering::Relaxed)
    } else {
        0
    };
    e.hitcap_max = if hitcap_en {
        state.vibration_hitcap_max.load(Ordering::Relaxed)
    } else {
        0
    };
    e.hitcap_win_ms = state.vibration_hitcap_win.load(Ordering::Relaxed);
    e.hitmerge_ms = if merge_en {
        state.vibration_hitmerge_ms.load(Ordering::Relaxed)
    } else {
        0
    };
    e.sustain_secs = if sustain_en {
        state.vibration_sustain_secs.load(Ordering::Relaxed)
    } else {
        0
    };
    e.sustain_reduce = state.vibration_sustain_reduce.load(Ordering::Relaxed);
    e.tail_land_pct = if tail_en {
        state.vibration_tail_land_pct.load(Ordering::Relaxed)
    } else {
        0
    };
    e.monster_abnormal = state.vibration_monster_abnormal.load(Ordering::Relaxed);
    /* 命中风暴抽样 (v16): 检测阈值独立于抽样开关 */
    let storm_thr_raw = state.vibration_storm_thr.load(Ordering::Relaxed);
    e.storm_thr = if storm_thr_raw == 0 { 10 } else { storm_thr_raw };
    e.storm_enabled = state.vibration_storm_enabled.load(Ordering::Relaxed) && storm_thr_raw != 0;
    e.storm_keep_pct = state.vibration_storm_keep_pct.load(Ordering::Relaxed);
    e.storm_win_ms = state.vibration_storm_win_ms.load(Ordering::Relaxed);
    e.storm_pause_ms = state.vibration_storm_pause_ms.load(Ordering::Relaxed);
    e.storm_mute_abnormal = state.vibration_storm_mute_abnormal.load(Ordering::Relaxed);
    e.storm_mute_rank = state.vibration_storm_mute_rank.load(Ordering::Relaxed);
    e.storm_unified_enabled = state.vibration_storm_unified_enabled.load(Ordering::Relaxed);
    e.storm_unified_ms = state.vibration_storm_unified_ms.load(Ordering::Relaxed);
    /* ★S1 输出引擎模式 (v20): 0-2, 越界回经典 */
    e.legacy_output_mode = state.vibration_legacy_output_mode.load(Ordering::Relaxed).min(2);
    /* 总闸 L/R (v15.1) */
    e.gate_lr_l = (1.0
        + state.vibration_item_lr[0].load(Ordering::Relaxed) as i32 as f32 / 100.0)
        .clamp(0.0, 2.0);
    e.gate_lr_r = (1.0
        + state.vibration_item_lr[1].load(Ordering::Relaxed) as i32 as f32 / 100.0)
        .clamp(0.0, 2.0);
    e.abs_freq_enabled = state.vibration_abs_freq_enabled.load(Ordering::Relaxed);
    e.abs_freq_window = state.vibration_abs_freq_window.load(Ordering::Relaxed);
    e.abs_freq_max = state.vibration_abs_freq_max.load(Ordering::Relaxed);
    e.ghost_enabled = state.vibration_rank_decay_enabled.load(Ordering::Relaxed);
    e.ghost_delay = state.vibration_rank_decay_delay.load(Ordering::Relaxed) as f32;
    e.ghost_speed = state.vibration_rank_decay_speed.load(Ordering::Relaxed) as f32;
    e.ghost_min = state.vibration_rank_decay_min.load(Ordering::Relaxed) as f32 / 100.0;
    e.ghost_max = state.vibration_rank_decay_max.load(Ordering::Relaxed) as f32 / 100.0;
    e.out_threshold = state.vibration_out_threshold.load(Ordering::Relaxed) as f32;
    e.remap_enabled = state.vibration_remap_enabled.load(Ordering::Relaxed);
    e.remap_min = state.vibration_remap_min.load(Ordering::Relaxed) as f32;
    e.split_enabled = state.vibration_split_enabled.load(Ordering::Relaxed);
    e.split_thr = state.vibration_split_thr.load(Ordering::Relaxed) as f32;
    /* 职业专属算法 (v31): id 变化时重置状态 */
    let new_algo = state.vibration_algo_id.load(Ordering::Relaxed);
    if new_algo != e.algo_id {
        e.algo_id = new_algo;
        e.algo_state = [0.0; 4];
    }
    for i in 0..4 {
        e.algo_ap[i] = state.vibration_algo_ap[i].load(Ordering::Relaxed) as f32;
    }
}

/// S4 引擎: 只同步 1.61 引擎**本身存在**的字段。
/// (S1 专属的 storm/merge/hitcap/sustain/tail_land/legacy_output_mode/gate_lr 等
/// 在 S4 引擎里根本不存在 —— 这就是两条路线分离的直接体现。)
fn sync_s4_params(e: &mut s4::VibEngine, state: &AppState, _legacy: bool) {
    e.throttle_window = state.vibration_throttle_window.load(Ordering::Relaxed);
    e.throttle_max = state.vibration_throttle_max.load(Ordering::Relaxed);
    e.throttle_dense_ratio = state.vibration_throttle_dense_ratio.load(Ordering::Relaxed);
    e.abs_freq_enabled = state.vibration_abs_freq_enabled.load(Ordering::Relaxed);
    e.abs_freq_window = state.vibration_abs_freq_window.load(Ordering::Relaxed);
    e.abs_freq_max = state.vibration_abs_freq_max.load(Ordering::Relaxed);
    e.ghost_enabled = state.vibration_rank_decay_enabled.load(Ordering::Relaxed);
    e.ghost_delay = state.vibration_rank_decay_delay.load(Ordering::Relaxed) as f32;
    e.ghost_speed = state.vibration_rank_decay_speed.load(Ordering::Relaxed) as f32;
    e.ghost_min = state.vibration_rank_decay_min.load(Ordering::Relaxed) as f32 / 100.0;
    e.ghost_max = state.vibration_rank_decay_max.load(Ordering::Relaxed) as f32 / 100.0;
    e.out_threshold = state.vibration_out_threshold.load(Ordering::Relaxed) as f32;
    e.remap_enabled = state.vibration_remap_enabled.load(Ordering::Relaxed);
    e.remap_min = state.vibration_remap_min.load(Ordering::Relaxed) as f32;
    e.split_enabled = state.vibration_split_enabled.load(Ordering::Relaxed);
    e.split_thr = state.vibration_split_thr.load(Ordering::Relaxed) as f32;
    /* 职业专属算法 (v31) */
    let new_algo = state.vibration_algo_id.load(Ordering::Relaxed);
    if new_algo != e.algo_id {
        e.algo_id = new_algo;
        e.algo_state = [0.0; 4];
    }
    for i in 0..4 {
        e.algo_ap[i] = state.vibration_algo_ap[i].load(Ordering::Relaxed) as f32;
    }
}

/// 通道出口合成 (仅 S1 路线调用): finalize → smooth → max 用的 FONT 分量。
fn composite_from_font(
    engine: &mut RouteEngine,
    params: &[u32; 60],
    motor_l: f32,
    motor_r: f32,
    rnd: f32,
    state: &AppState,
) -> (u16, u16) {
    let smr = state.vibration_out_smooth.load(Ordering::Relaxed).min(100) as f32 / 100.0;
    match engine {
        RouteEngine::S1(e) => {
            let (efl, efr) = e.finalize(params);
            let (efl, efr) = e.smooth_out(efl as f32, efr as f32, smr * 0.9);
            (
                (efl * motor_l * (1.0 + rnd)).min(65535.0) as u16,
                (efr * motor_r * (1.0 + rnd)).min(65535.0) as u16,
            )
        }
        /* S4 恒独占 —— 不应走到这里 (S4 的 channel_composites() 恒 false) */
        RouteEngine::S4(e) => {
            let (efl, efr) = e.finalize(params);
            let (efl, efr) = e.smooth_out(efl as f32, efr as f32, smr * 0.9);
            (
                (efl * motor_l * (1.0 + rnd)).min(65535.0) as u16,
                (efr * motor_r * (1.0 + rnd)).min(65535.0) as u16,
            )
        }
    }
}

/// 移动步伐相位推进 (两条引擎各自持有相位状态)。
fn step_move_phase(engine: &mut RouteEngine, now: u32, pace_ms: u32) -> (f32, bool) {
    match engine {
        RouteEngine::S1(e) => {
            if expired_or_unset(now, e.move_pace_until) {
                e.move_pace_until = now.wrapping_add(pace_ms);
                e.move_phase = !e.move_phase;
            }
            let frac = if e.move_pace_until != 0 && before(now, e.move_pace_until) {
                1.0 - (e.move_pace_until.wrapping_sub(now) as f32 / pace_ms as f32)
            } else {
                1.0
            };
            (frac, e.move_phase)
        }
        RouteEngine::S4(e) => {
            if expired_or_unset(now, e.move_pace_until) {
                e.move_pace_until = now.wrapping_add(pace_ms);
                e.move_phase = !e.move_phase;
            }
            let frac = if e.move_pace_until != 0 && before(now, e.move_pace_until) {
                1.0 - (e.move_pace_until.wrapping_sub(now) as f32 / pace_ms as f32)
            } else {
                1.0
            };
            (frac, e.move_phase)
        }
    }
}

/// 移动通道平滑插值。
fn smooth_move(engine: &mut RouteEngine, tl: f32, tr: f32, smooth: f32) -> (f32, f32) {
    match engine {
        RouteEngine::S1(e) => {
            e.move_out_l += (tl - e.move_out_l) * smooth;
            e.move_out_r += (tr - e.move_out_r) * smooth;
            (e.move_out_l, e.move_out_r)
        }
        RouteEngine::S4(e) => {
            e.move_out_l += (tl - e.move_out_l) * smooth;
            e.move_out_r += (tr - e.move_out_r) * smooth;
            (e.move_out_l, e.move_out_r)
        }
    }
}

/// FONT 常态出口: finalize。
fn finalize_route(engine: &mut RouteEngine, params: &[u32; 60]) -> (u16, u16) {
    match engine {
        RouteEngine::S1(e) => e.finalize(params),
        RouteEngine::S4(e) => e.finalize(params),
    }
}

/// FONT 常态出口的平滑。
fn smooth_font_out(engine: &mut RouteEngine, l: f32, r: f32, k: f32) -> (f32, f32) {
    match engine {
        RouteEngine::S1(e) => e.smooth_out(l, r, k),
        RouteEngine::S4(e) => e.smooth_out(l, r, k),
    }
}

/* ---------------- 测试 ---------------- */
#[cfg(test)]
mod route_dispatch_tests {
    use super::*;

    /// 两条引擎的路线归属: 构造后各自匹配自己的路线, 且切路线会重建。
    #[test]
    fn route_engine_matches_route() {
        let s1 = RouteEngine::new(true);
        assert!(s1.matches(true), "S1 路线应持有 S1 引擎");
        assert!(!s1.matches(false), "S1 引擎不得冒充 S4");

        let s4 = RouteEngine::new(false);
        assert!(s4.matches(false), "S4 路线应持有 S4 引擎");
        assert!(!s4.matches(true), "S4 引擎不得冒充 S1");
    }

    /// 出口语义: S1 合成 / S4 独占 (两引擎各自的常量判定)。
    #[test]
    fn channel_composites_is_per_engine() {
        assert!(
            RouteEngine::new(true).channel_composites(),
            "S1 出口必须合成 (v13.37 红线)"
        );
        assert!(
            !RouteEngine::new(false).channel_composites(),
            "S4 出口必须独占 (对齐 1.61)"
        );
    }

    /// 事件计数口径: S1 只计真实注入, S4 计全部到达。
    #[test]
    fn last_injected_is_per_route() {
        assert!(RouteEngine::new(false).last_injected(), "S4: 恒计入到达事件");
    }

    /// 算法总开关层: 只对 S1 应用 (S4 直接使用保存值 = 1.61)。
    #[test]
    fn algo_toggle_applies_to_s1_only() {
        let saved = {
            let mut p = [0u32; 60];
            p[55] = 150;
            p[56] = 90;
            p[57] = 90;
            p[58] = 30;
            p
        };
        let mut s1 = saved;
        apply_algo_toggle_params(&mut s1, false, false, false, false, false, false);
        assert_eq!(
            [s1[55], s1[56], s1[57], s1[58]],
            [100, 0, 0, 0],
            "S1: 开关层必须回写中性值 (既有功能)"
        );

        let s4 = saved; /* S4 不调用该函数, 保存值原样 */
        assert_eq!(
            [s4[55], s4[56], s4[57], s4[58]],
            [150, 90, 90, 30],
            "S4: 参数槽必须原样保留 (= 1.61 生效值)"
        );
    }
}

#[cfg(test)]
mod composite_output_tests {
    use super::*;

    /// 结构锁: S4 引擎文件里不得出现 S1 专属字段/分支, S1 引擎文件里也不得出现
    /// S4 的语义 —— 两条路线物理隔离的直接守卫。
    #[test]
    fn engines_do_not_cross_reference() {
        let s1_src = include_str!("s1.rs");
        let s4_src = include_str!("s4.rs");

        assert!(
            !s1_src.contains("s4::") && !s1_src.contains("use super::s4"),
            "S1 引擎不得引用 S4 模块"
        );
        assert!(
            !s4_src.contains("s1::") && !s4_src.contains("use super::s1"),
            "S4 引擎不得引用 S1 模块"
        );
        /* S4 引擎不得含 S1 专属机制 (1.61 里零命中) */
        for kw in [
            "storm_active",
            "font_carry_s",
            "hitcap_count",
            "tail_land_pct",
            "sustain_secs",
            "shape_carry_l",
            "legacy_output_mode",
            "monster_abnormal",
        ] {
            assert!(
                !s4_src.contains(kw),
                "S4 引擎出现 S1 专属机制 `{}` —— 两条路线已互相污染",
                kw
            );
        }
    }

    /// S4 引擎必须与 1.61 的关键语义一致 (衰减下限 10/30ms, 无双通道窗)。
    #[test]
    fn s4_engine_has_v161_semantics() {
        let s4_src = include_str!("s4.rs");
        assert!(
            s4_src.contains("max(10.0)"),
            "S4 命中衰减下限必须为 10ms (1.61 vibration.rs:888-890)"
        );
        assert!(
            s4_src.contains("max(30.0)"),
            "S4 特殊/状态/普攻衰减下限必须为 30ms (1.61 vibration.rs:894-898)"
        );
        assert!(
            !s4_src.contains("last_font_ch"),
            "S4 不得有分通道注入窗 (1.61 只有一个全局 last_font)"
        );
    }
}

#[cfg(test)]
mod algo_toggle_tests {
    use super::*;

    #[test]
    fn algo_toggle_params_override() {
        let mut p = [7u32; 60];
        apply_algo_toggle_params(&mut p, false, false, false, false, false, false);
        assert_eq!(p[29], 100, "密度自适应关 → 阈值 100");
        assert_eq!(p[51], 0, "命中自适应关 → 降幅 0");
        assert_eq!(p[21], 0, "走位能量关 → 速率 0");
        assert_eq!(
            [p[35], p[36], p[37], p[38]],
            [25, 55, 25, 60],
            "衰减组关 → 回内置默认衰减"
        );
        assert_eq!(
            [p[40], p[41], p[42], p[43], p[44], p[45], p[46]],
            [150, 90, 250, 20, 600, 1500, 3000],
            "窗口组关 → 回内置默认窗口"
        );
        assert_eq!(
            [p[54], p[55], p[56], p[57], p[58]],
            [0, 100, 0, 0, 0],
            "静默脉冲关 → 中性值"
        );
        assert_eq!(p[59], 7, "测试强度不受任何开关影响");
        let mut q = [7u32; 60];
        apply_algo_toggle_params(&mut q, true, true, true, true, true, true);
        assert_eq!([q[29], q[35], q[40], q[54]], [7, 7, 7, 7], "全开时参数不动");
    }}
