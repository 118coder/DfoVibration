//! 震动参数启动恢复回归测试。
//!
//! 背景: AppState::new 曾把 vibration_params[60] 全部初始化为 0, 不从
//! config.vibration 恢复——极简模式启动 (不渲染震动页) 的会话里,
//! 引擎每帧吃全 0 参数 (master_gain=0), 手柄全程无震动。

use sorahk::config::AppConfig;
use sorahk::state::AppState;

#[test]
fn vibration_params_restored_from_config_on_startup() {
    let mut config = AppConfig::default();
    // 模拟用户上一次会话"保存当前"落盘的参数 (sync_vib_config_from_params 的镜像)
    config.vibration.master_gain = 91;
    config.vibration.attack_gain = 82;
    config.vibration.max_strength = 86;
    config.vibration.rhythm = 79;
    config.vibration.decay_ms = 87;
    config.vibration.advanced[0] = 44;
    config.vibration.advanced[40] = 444;

    let state = AppState::new(config).expect("Failed to create state");

    // 索引映射: 0=attack 4=max_strength 5=decay 9=master 18=rhythm 19+i=advanced[i]
    assert_eq!(
        state.vibration_params[9].load(std::sync::atomic::Ordering::Relaxed),
        91,
        "master_gain(params[9]) 启动时必须从 config 恢复"
    );
    assert_eq!(
        state.vibration_params[0].load(std::sync::atomic::Ordering::Relaxed),
        82
    );
    assert_eq!(
        state.vibration_params[4].load(std::sync::atomic::Ordering::Relaxed),
        86
    );
    assert_eq!(
        state.vibration_params[5].load(std::sync::atomic::Ordering::Relaxed),
        87
    );
    assert_eq!(
        state.vibration_params[18].load(std::sync::atomic::Ordering::Relaxed),
        79
    );
    assert_eq!(
        state.vibration_params[19].load(std::sync::atomic::Ordering::Relaxed),
        44,
        "advanced[0] → params[19]"
    );
    assert_eq!(
        state.vibration_params[59].load(std::sync::atomic::Ordering::Relaxed),
        444,
        "advanced[40] → params[59]"
    );
}

#[test]
fn vibration_params_default_config_starts_zeroed_but_valid() {
    // 默认配置 (全新安装): 结构体默认 master/attack 全 0 → params 全 0,
    // 由 GUI 首渲染的 INIT 块套用内置"默认"预设 (pristine 判定依据)
    let config = AppConfig::default();
    let state = AppState::new(config).expect("Failed to create state");

    assert_eq!(
        state.vibration_params[9].load(std::sync::atomic::Ordering::Relaxed),
        0
    );
    assert_eq!(
        state.vibration_params[0].load(std::sync::atomic::Ordering::Relaxed),
        0
    );
}
