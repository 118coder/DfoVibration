//! Config 持久化 roundtrip 回归测试 (save → load → 逐字段相等)。
//!
//! 背景: 写端是手工 format! 模板、读端是 serde——模板每漏一行,
//! 对应字段就在每次保存时静默丢失 (已实锤两例: `rawinput_capture_mode`
//! 与预设快照的 `double_tap_*`)。本文件是这类 bug 的反馈回路:
//! 夹具里每个字段的值都刻意取成「非默认」, 任何字段保存后读不回来,
//! 这里变红并指名该字段。

use smallvec::SmallVec;
use sorahk::config::{
    AppConfig, DeviceApiPreference, HidDeviceBaseline, KeyMapping, Preset,
    VibrationConfig, VibrationPreset,
};
use sorahk::i18n::Language;
use std::collections::HashMap;
use std::path::PathBuf;

/// 逐字段断言, 失败时指名字段 (比整结构体 assert_eq 的输出可读得多)
macro_rules! assert_fields {
    ($loaded:expr, $orig:expr, $($field:ident),+ $(,)?) => {
        $(
            assert_eq!(
                $loaded.$field, $orig.$field,
                concat!("字段在 save→load 后不一致: ", stringify!($field))
            );
        )+
    };
}

/// 每个测试独立子目录: save_to_file 会在配置同目录写 Vibration.toml,
/// 若共用 temp 根目录, 并行测试会互相践踏同名的 Vibration.toml。
fn temp_path(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("sorahk_rt_{}_{}", tag, std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join("Config.toml")
}

fn cleanup(cfg_path: &PathBuf) {
    if let Some(dir) = cfg_path.parent() {
        let _ = std::fs::remove_dir_all(dir);
    }
}

/// 所有字段均取非默认值的映射 (Option 字段取 Some 路径)
fn distinct_mapping(trigger: &str, gap: u64) -> KeyMapping {
    KeyMapping {
        trigger_key: trigger.to_string(),
        target_keys: SmallVec::from_vec(vec!["F6".to_string(), "F7".to_string()]),
        interval: Some(37),
        event_duration: Some(41),
        turbo_enabled: false,
        move_speed: 13,
        double_tap_enabled: true,
        double_tap_gap_ms: gap,
        note: format!("备注-{trigger}"),
    }
}

/// Option 字段取 None 路径的映射 (覆盖模板的条件行分支)
fn none_options_mapping() -> KeyMapping {
    KeyMapping {
        trigger_key: "LALT+9".to_string(),
        target_keys: SmallVec::from_vec(vec!["NUMPAD0".to_string()]),
        interval: None,
        event_duration: None,
        turbo_enabled: true,
        move_speed: 5,
        double_tap_enabled: false,
        double_tap_gap_ms: 50,
        note: String::new(),
    }
}

/// 全字段非默认 (bool 一律取 serde 默认的反向, 数值避开各自默认值)
fn full_vibration() -> VibrationConfig {
    VibrationConfig {
        enabled: false,
        advanced_enabled: true,
        motor_l_gain: 91,
        motor_r_gain: 92,
        item_lr: std::array::from_fn(|i| (i as u32) * 3 + 1),
        rank_lr: std::array::from_fn(|i| (i as u32) * 7 + 2),
        rank_type_gain: std::array::from_fn(|i| (i as u32) * 5 + 3),
        rank_level_gain: 93,
        rank_duration: 297,
        random_gain: 66,
        out_smooth: 56,
        independent_test: true,
        move_independent: false,
        rank_decay_enabled: true,
        out_threshold: 27,
        remap_enabled: false,
        remap_min: 26,
        split_enabled: false,
        split_thr: 41,
        rank_decay_delay_ms: 1999,
        rank_decay_speed: 3,
        rank_decay_min_mul: 61,
        rank_decay_max_mul: 121,
        abs_freq_enabled: true,
        abs_freq_window_ms: 2999,
        abs_freq_max: 7,
        throttle_window_ms: 67,
        throttle_max_hits: 68,
        throttle_dense_ratio: 99,
        merge_keep: 91,
        merge_cap: 92,
        merge_hold: 93,
        hitcap_max: 94,
        hitcap_win_ms: 995,
        hitmerge_ms: 996,
        sustain_secs: 9,
        sustain_reduce: 45,
        tail_land_pct: 96,
        monster_abnormal_gain: 97,
        storm_thr: 11,
        storm_keep_pct: 46,
        storm_win_ms: 997,
        storm_pause_ms: 998,
        storm_mute_abnormal: true,
        storm_mute_rank: true,
        merge_enabled: false,
        hitcap_enabled: false,
        storm_enabled: false,
        sustain_enabled: false,
        tail_land_enabled: false,
        density_enabled: false,
        adapt_enabled: false,
        move_charge_enabled: false,
        decay_enabled: false,
        algo_windows_enabled: false,
        pulse_enabled: false,
        storm_unified_enabled: true,
        storm_unified_ms: 137,
        attack_gain: 82,
        damage_gain: 83,
        shake_gain: 84,
        move_gain: 85,
        max_strength: 86,
        decay_ms: 87,
        hit_boost: 88,
        start_pulse: 89,
        kill_pulse: 90,
        master_gain: 91,
        font_strength: 77,
        font_interval: 78,
        rhythm: 79,
        font_hp: 71,
        font_special: 72,
        font_state: 73,
        font_effect: 74,
        font_attack: 75,
        font_hit: 76,
        advanced: std::array::from_fn(|i| (i as u32) * 11 + 4),
    }
}

fn full_vib_preset() -> VibrationPreset {
    VibrationPreset {
        name: "自定A".to_string(),
        params: std::array::from_fn(|i| (i as u32) * 2 + 1),
        item_lr: std::array::from_fn(|i| (i as u32) * 3 + 2),
        rank_lr: std::array::from_fn(|i| (i as u32) * 7 + 3),
        rank_type_gain: std::array::from_fn(|i| (i as u32) * 5 + 4),
        rank_level_gain: 77,
        rank_duration: 321,
        out_smooth: 44,
        user_modified: true,
    }
}

/// 全字段非默认的完整配置。
/// 注意避开 load_from_file 的两类归一化: input_timeout/interval/event_duration
/// 的下限钳制、process_whitelist 排序去重、LS_Left/LS_Right 强制双击。
fn full_config() -> AppConfig {
    let mut c = AppConfig::default();
    c.show_tray_icon = false;
    c.show_notifications = false;
    c.always_on_top = true;
    c.dark_mode = true;
    c.language = Language::English;
    c.guide_seen = true;
    c.window_rect_normal = Some([100.5, 200.5, 800.0, 600.0]);
    c.window_rect_minimal = Some([310.5, 420.5, 260.0, 150.0]);
    c.minimal_mode = true;
    c.minimal_vib_preset_job = true;
    c.dfo_player = false;
    c.whitelist_enabled = false;
    c.switch_key = "F9".to_string();
    c.mappings = vec![distinct_mapping("F6", 77), none_options_mapping()];
    c.input_timeout = 9;
    c.interval = 17;
    c.event_duration = 23;
    c.worker_count = 4;
    c.process_whitelist = vec!["aaa.exe".to_string(), "dnf.exe".to_string()];
    c.hid_baselines = vec![HidDeviceBaseline {
        device_id: "VID_045E:PID_0B12".to_string(),
        baseline_data: vec![0, 1, 2, 254],
    }];
    c.rawinput_capture_mode = "TestModeA".to_string();
    c.xinput_capture_mode = "LastStable".to_string();
    c.device_api_preferences = HashMap::from([(
        "045E:0B12".to_string(),
        DeviceApiPreference::XInput,
    )]);
    c.presets = vec![Preset {
        name: "快照A".to_string(),
        mappings: vec![distinct_mapping("F8", 88)],
    }];
    c.current_preset = "快照A".to_string();
    c.vibration = full_vibration();
    c.vibration_presets = vec![full_vib_preset()];
    c
}

#[test]
fn app_config_roundtrip_preserves_every_field() {
    let cfg_path = temp_path("config");
    let vib_path = AppConfig::vibration_path_for(&cfg_path);
    let orig = full_config();

    orig.save_to_file(&cfg_path).expect("save_to_file 失败");
    let mut loaded = AppConfig::load_from_file(&cfg_path).expect("load_from_file 失败");
    loaded
        .load_vibration_from_file(&vib_path)
        .expect("load_vibration_from_file 失败");

    assert_fields!(
        loaded, orig,
        show_tray_icon, show_notifications, always_on_top, dark_mode,
        language, guide_seen, minimal_mode, minimal_vib_preset_job,
        switch_key, input_timeout, interval, event_duration,
        worker_count, rawinput_capture_mode, xinput_capture_mode,
        current_preset, dfo_player, whitelist_enabled,
    );
    assert_eq!(
        loaded.window_rect_normal, orig.window_rect_normal,
        "window_rect_normal"
    );
    assert_eq!(
        loaded.window_rect_minimal, orig.window_rect_minimal,
        "window_rect_minimal"
    );
    assert_eq!(
        loaded.process_whitelist, orig.process_whitelist,
        "process_whitelist"
    );
    assert_eq!(loaded.mappings, orig.mappings, "mappings");
    assert_eq!(loaded.presets, orig.presets, "presets");
    assert_eq!(loaded.hid_baselines, orig.hid_baselines, "hid_baselines");
    assert_eq!(
        loaded.device_api_preferences, orig.device_api_preferences,
        "device_api_preferences"
    );
    assert_eq!(loaded.vibration, orig.vibration, "vibration");
    assert_eq!(
        loaded.vibration_presets, orig.vibration_presets,
        "vibration_presets"
    );

    cleanup(&cfg_path);
}

/// 实锤 bug #1: rawinput_capture_mode 从未出现在保存模板中。
#[test]
fn rawinput_capture_mode_survives_save() {
    let p = temp_path("rawinput_mode");
    let mut c = AppConfig::default();
    c.rawinput_capture_mode = "TestModeA".to_string();

    c.save_to_file(&p).unwrap();
    let loaded = AppConfig::load_from_file(&p).unwrap();

    assert_eq!(
        loaded.rawinput_capture_mode, "TestModeA",
        "rawinput_capture_mode 在保存模板中缺失, 每次保存后静默回退默认值"
    );
    cleanup(&p);
}

/// 实锤 bug: load 路径的摇杆双击迁移会触发全量保存, 用内存默认值
/// 覆盖磁盘上的 Vibration.toml (用户自建预设/调参被清空)。
/// 修复后: load_or_create 不得有任何写副作用。
#[test]
fn load_or_create_has_no_write_side_effect() {
    let cfg_path = temp_path("ls_mig");
    let vib_path = AppConfig::vibration_path_for(&cfg_path);

    // 旧版格式: LS 映射无 double_tap 字段 (serde default false → 旧代码必触发迁移保存)
    std::fs::write(
        &cfg_path,
        "show_tray_icon = true\n\
         show_notifications = true\n\
         switch_key = \"DELETE\"\n\
         \n\
         [[mappings]]\n\
         trigger_key = \"GAMEPAD_045E_LS_Left\"\n\
         target_keys = [\"W\"]\n\
         turbo_enabled = true\n\
         move_speed = 10\n",
    )
    .unwrap();
    // 用户自建震动预设
    std::fs::write(
        &vib_path,
        "[vibration]\n\
         enabled = true\n\
         master_gain = 91\n\
         \n\
         [[vibration_presets]]\n\
         name = \"我的自定\"\n\
         params = [1, 2, 3]\n",
    )
    .unwrap();

    let before = std::fs::read_to_string(&vib_path).unwrap();
    let cfg = AppConfig::load_or_create(&cfg_path).unwrap();
    let after = std::fs::read_to_string(&vib_path).unwrap();

    assert_eq!(
        after, before,
        "load_or_create 改写了 Vibration.toml (load 路径不得有写副作用)"
    );
    // 用户的震动参数也必须完整读回
    assert_eq!(cfg.vibration.master_gain, 91);

    cleanup(&cfg_path);
}

/// 实锤 bug #2: 预设快照的映射循环漏写 double_tap_*(主 mappings 循环有写)。
#[test]
fn preset_double_tap_survives_save() {
    let p = temp_path("preset_double_tap");
    let mut c = AppConfig::default();
    c.presets = vec![Preset {
        name: "快照A".to_string(),
        mappings: vec![distinct_mapping("F8", 88)],
    }];

    c.save_to_file(&p).unwrap();
    let loaded = AppConfig::load_from_file(&p).unwrap();

    let m = &loaded.presets[0].mappings[0];
    assert!(
        m.double_tap_enabled,
        "预设快照丢失 double_tap_enabled (保存后静默归 false)"
    );
    assert_eq!(
        m.double_tap_gap_ms, 88,
        "预设快照丢失 double_tap_gap_ms (保存后静默归默认 50)"
    );
    cleanup(&p);
}

/// ★v16.8: ACT1 特供预设的附加默认 (高级调校开 + 两个风暴静音默认勾选)
#[test]
fn act1_preset_extras_defaults() {
    let mut vib = VibrationConfig::default();
    vib.advanced_enabled = false;
    vib.storm_mute_abnormal = false;
    vib.storm_mute_rank = false;
    sorahk::config::act1_preset_extras(&mut vib);
    assert!(vib.advanced_enabled, "ACT1 特供应默认开启高级调校");
    assert!(vib.storm_mute_abnormal, "ACT1 特供应默认勾选怪物异常静音");
    assert!(vib.storm_mute_rank, "ACT1 特供应默认勾选评分点系统静音");
}
