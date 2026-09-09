//! 震动参数启动恢复回归测试。
//!
//! 背景: AppState::new 曾把 vibration_params[60] 全部初始化为 0, 不从
//! config.vibration 恢复——极简模式启动 (不渲染震动页) 的会话里,
//! 引擎每帧吃全 0 参数 (master_gain=0), 手柄全程无震动。
//!
//! ★2026-09-08 起启动兜底完整镜像震动页 INIT_DONE 的初始化顺序:
//!   config.vibration 恢复 → pristine 时套内置"默认"预设 → applied 的
//!   JobVibration.toml 职业参数覆盖 (含 algo/throttle)。
//! 下方 expected_startup_params 与 state.rs 的实现互为镜像, 是维护契约。

use sorahk::config::AppConfig;
use sorahk::state::AppState;

/* 镜像 state.rs AppState::new 的启动参数兜底 (pristine→内置默认预设→JobVibration 覆盖),
 * 供测试计算期望值。与实现保持同步是本测试的维护契约。 */
fn expected_startup_params(mut config: AppConfig) -> [u32; 60] {
    let mut out = [0u32; 60];
    out[0] = config.vibration.attack_gain;
    out[1] = config.vibration.damage_gain;
    out[2] = config.vibration.shake_gain;
    out[3] = config.vibration.move_gain;
    out[4] = config.vibration.max_strength;
    out[5] = config.vibration.decay_ms;
    out[6] = config.vibration.hit_boost;
    out[7] = config.vibration.start_pulse;
    out[8] = config.vibration.kill_pulse;
    out[9] = config.vibration.master_gain;
    out[10] = config.vibration.font_strength;
    out[11] = config.vibration.font_interval;
    out[12] = config.vibration.font_hp;
    out[13] = config.vibration.font_special;
    out[14] = config.vibration.font_state;
    out[15] = config.vibration.font_effect;
    out[16] = config.vibration.font_attack;
    out[17] = config.vibration.font_hit;
    out[18] = config.vibration.rhythm;
    for i in 0..41 {
        out[19 + i] = config.vibration.advanced[i];
    }
    if config.vibration.master_gain == 0 && config.vibration.attack_gain == 0 {
        if let Some(pr) = sorahk::config::default_vibration_presets().first() {
            out = pr.params;
        }
    }
    if let Some(jcfg) = sorahk::job_presets::load_job_vibration() {
        if jcfg.applied {
            if let Some((_b, _c, cls)) =
                sorahk::job_presets::find_class(&jcfg.base_job, &jcfg.class_name)
            {
                let p = if jcfg.params.len() == 60 {
                    jcfg.params.clone()
                } else {
                    cls.params.to_vec()
                };
                out.copy_from_slice(&p);
            }
        }
    }
    out
}

fn read_param(state: &AppState, i: usize) -> u32 {
    state.vibration_params[i].load(std::sync::atomic::Ordering::Relaxed)
}

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

    let expected = expected_startup_params(config.clone());
    let state = AppState::new(config).expect("Failed to create state");

    // 索引映射: 0=attack 4=max_strength 5=decay 9=master 18=rhythm 19+i=advanced[i]
    // 注: 若测试环境存在 applied 的 JobVibration.toml, 职业参数覆盖优先
    //     (镜像实现, expected_startup_params 已计入)。
    for i in [0usize, 4, 5, 9, 18, 19, 59] {
        assert_eq!(
            read_param(&state, i),
            expected[i],
            "params[{}] 启动时必须与启动兜底结果一致",
            i
        );
    }
}

#[test]
fn vibration_params_default_config_gets_builtin_preset_at_startup() {
    // 默认配置 (全新安装): master/attack 全 0 = pristine → 启动兜底套内置
    // "默认"预设; 若测试环境存在 applied 的 JobVibration 则其覆盖优先。
    let config = AppConfig::default();
    let state = AppState::new(config).expect("Failed to create state");
    let expected = expected_startup_params(AppConfig::default());
    for i in [0usize, 9, 18, 30, 59] {
        assert_eq!(
            read_param(&state, i),
            expected[i],
            "params[{}] pristine 启动应与兜底结果一致",
            i
        );
    }
    // pristine 兜底后不应再是全 0 (否则等于修回了"不进页面就静音"的老 bug)
    assert_ne!(expected[9], 0, "内置默认预设的 master_gain 不应为 0");
}
