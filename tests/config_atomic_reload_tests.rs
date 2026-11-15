//! ★R2.12 (优化 2) 配置整体切换的回归测试 —— 外部 crate 视角。
//!
//! 这条用例锁的是「改设置改坏不许半残」这个语义:
//! 以前 `reload_config` 边算边写, 中途遇到坏配置会返回 Err, 但前面几个 store
//! 已经生效了 —— 共享状态停在「一半新、一半旧」, 输入线程正好在那一瞬读到就会
//! 触发**已经改掉**的映射 (表现为"按下去偶尔失灵 / 触发旧功能", 极难复现)。
//!
//! 现在两阶段: 先把所有可能失败的构建做完, 失败则一个字节都不动;
//! 成功才整体切换, 并把配置代次 +1。

use sorahk::config::{AppConfig, KeyMapping};
use sorahk::state::{AppState, CaptureMode, InputDevice};

const VK_Q: u32 = 0x51;
const VK_E: u32 = 0x45;

fn mk(trigger: &str, target: &str) -> KeyMapping {
    let mut m = KeyMapping {
        release_targets: Default::default(),
        sequence_text: String::new(),
        trigger_key: trigger.to_string(),
        target_keys: Default::default(),
        interval: None,
        event_duration: None,
        turbo_enabled: false,
        move_speed: 5,
        double_tap_enabled: false,
        double_tap_gap_ms: 50,
        run_enabled: false,
        run_threshold: 80,
        run_recheck: true,
        lock_enabled: false,
        note: String::new(),
    };
    m.add_target_key(target.to_string());
    m
}

fn has_key(state: &AppState, vk: u32) -> bool {
    state.get_input_mapping(&InputDevice::Keyboard(vk)).is_some()
}

/// 坏配置必须「整份不生效」: 捕获模式、映射、代次都不能被改动。
#[test]
fn bad_config_leaves_shared_state_untouched() {
    let mut base = AppConfig::default();
    base.rawinput_capture_mode = "MostSustained".to_string();
    base.switch_key = "F12".to_string();
    base.mappings = vec![mk("Q", "W")];

    let state = AppState::new(base.clone()).expect("基线配置应能建起来");
    let epoch_before = state.config_epoch();
    let mode_before = state.get_rawinput_capture_mode();
    assert_eq!(mode_before, CaptureMode::MostSustained, "前提: 基线捕获模式");
    assert!(has_key(&state, VK_Q), "前提: 旧映射 Q 已生效");

    /* 构造一份"前面几项合法、后面一项非法"的配置:
     * 改捕获模式 + 换映射 + 换一个解析不了的切换键 */
    let mut bad = base.clone();
    bad.rawinput_capture_mode = "LastStable".to_string();
    bad.mappings = vec![mk("E", "R")];
    bad.switch_key = "这不是一个按键".to_string();

    assert!(state.reload_config(bad).is_err(), "非法切换键必须报错");

    /* 关键断言: 失败的 reload 不许留下任何副作用 */
    assert_eq!(
        state.config_epoch(),
        epoch_before,
        "失败的 reload 不该推进代次"
    );
    assert_eq!(
        state.get_rawinput_capture_mode(),
        mode_before,
        "失败的 reload 不该改掉捕获模式 (原来这里会先写进一半)"
    );
    assert!(has_key(&state, VK_Q), "失败的 reload 不该把旧映射清空");
    assert!(!has_key(&state, VK_E), "失败的新映射不该部分生效");
}

/// 合法配置: 整体生效 + 代次 +1 (下游缓存据此判断"该重建了")。
#[test]
fn good_config_applies_whole_and_bumps_epoch() {
    let mut base = AppConfig::default();
    base.rawinput_capture_mode = "MostSustained".to_string();
    base.mappings = vec![mk("Q", "W")];

    let state = AppState::new(base.clone()).expect("基线配置应能建起来");
    let epoch_before = state.config_epoch();
    assert_eq!(
        state.get_rawinput_capture_mode(),
        CaptureMode::MostSustained
    );

    let mut next = base.clone();
    next.rawinput_capture_mode = "LastStable".to_string();
    next.mappings = vec![mk("E", "R")];

    state.reload_config(next).expect("合法配置应切换成功");

    assert_eq!(
        state.config_epoch(),
        epoch_before + 1,
        "成功的 reload 必须推进一代"
    );
    assert_eq!(
        state.get_rawinput_capture_mode(),
        CaptureMode::LastStable,
        "新模式应已生效"
    );
    assert!(has_key(&state, VK_E), "新映射应已生效");
    assert!(!has_key(&state, VK_Q), "旧映射应已清除");
}

/// 连续多次失败不该把代次推乱 (排查日志时靠代次判断"新配置到底生效没有")。
#[test]
fn repeated_failures_keep_epoch_stable() {
    let base = AppConfig::default();
    let state = AppState::new(base.clone()).expect("基线配置应能建起来");
    let epoch0 = state.config_epoch();

    for _ in 0..3 {
        let mut bad = base.clone();
        bad.switch_key = "不存在".to_string();
        assert!(state.reload_config(bad).is_err());
    }
    assert_eq!(state.config_epoch(), epoch0, "只有成功切换才推进代次");
}
