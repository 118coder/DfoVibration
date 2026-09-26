//! 架构守卫 + 持久化契约快照 (★R1.1, 2026-09-27 — 长期保障层)。
//!
//! 这组测试不测"功能对不对", 测的是**结构契约有没有被无意识地破坏**:
//! 1. `no_gui_dep_outside_gui`: 除 src/gui/** 之外, 任何文件禁止 `crate::gui`
//!    —— GUI 是表现层, 驱动/引擎/装配层反向依赖 GUI 是缠结的开始
//!    (历史病历: XInputDeviceInfo/HidDeviceInfo 曾住在 gui 里被 rawinput/xinput 引用)。
//! 2. `gui_dependency_allowlist`: gui 只允许消费清单内的 crate 模块
//!    —— 钩子线程/托盘/信号/急退等"接线与系统层"禁止从 GUI 直接伸手。
//! 3. `config_stays_leaf`: config.rs 只允许依赖 i18n + util —— 配置是叶子,
//!    不许长成依赖枢纽。
//! 4. `toml_contract_*`: serde 序列化键集合快照 —— **TOML 字段名 = 老配置文件的
//!    兼容契约** (无迁移机制)。新增字段 = 有意扩展 (更新 EXPECTED 清单即可);
//!    改名/删除字段 = 破坏用户已有 TOML = 必须先过 ADR。
//!
//! 新增合法依赖时的操作: 更新对应 ALLOWED 清单 + 在提交信息里写明理由
//! (这是"有意识的决定", 不是随手 import)。

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

fn src_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

/// 递归收集 src 下全部 .rs 文件
fn collect_rs(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = fs::read_dir(dir).unwrap_or_else(|e| panic!("读目录失败 {:?}: {}", dir, e));
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            collect_rs(&p, out);
        } else if p.extension().map(|x| x == "rs").unwrap_or(false) {
            out.push(p);
        }
    }
}

/// 去掉行内 `//` 之后的部分 (注释里出现 crate::xxx 不算依赖)
fn strip_comment(line: &str) -> &str {
    match line.find("//") {
        Some(i) => &line[..i],
        None => line,
    }
}

/// 提取一段源码里引用的全部 crate 模块名
fn crate_modules(text: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for line in text.lines() {
        let code = strip_comment(line);
        let mut rest = code;
        while let Some(i) = rest.find("crate::") {
            let after = &rest[i + 7..];
            let name: String = after.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect();
            if !name.is_empty() {
                out.insert(name);
            }
            rest = after;
        }
    }
    out
}

/// ── 1: GUI 外禁止 crate::gui ─────────────────────────────────────────────
#[test]
fn no_gui_dep_outside_gui() {
    let mut files = Vec::new();
    collect_rs(&src_dir(), &mut files);
    let mut offenders = Vec::new();
    for f in &files {
        let path = f.to_string_lossy().replace('\\', "/");
        if path.contains("/src/gui/") || path.ends_with("/src/main.rs") {
            continue; // GUI 自身与装配根豁免
        }
        let text = fs::read_to_string(f).unwrap();
        for (i, line) in text.lines().enumerate() {
            if strip_comment(line).contains("crate::gui") {
                offenders.push(format!("{}:{}: {}", path, i + 1, line.trim()));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "发现 GUI 外的 crate::gui 反向依赖 (表现层只能被消费, 不能被依赖):\n{}",
        offenders.join("\n")
    );
}

/// ── 2: gui 的模块依赖允许清单 ────────────────────────────────────────────
/// 允许 = 当前真实图 (2026-09-27 R1.1 审计)。不在清单内 = 禁止,
/// 特别是: keyboard/mouse (钩子线程), tray/signal/safety (系统接线),
/// input_ownership (设备仲裁内部), hidhide (冻结资产)。
#[test]
fn gui_dependency_allowlist() {
    const ALLOWED: &[&str] = &[
        "gui",          // gui 内部互引
        "config", "state", "i18n", "util", "job_presets", "sequence",
        "rawinput", "xinput", "hid_layout",   // 输入层公开 API (类型/查询函数)
        "vibration",                           // 引擎公开助手 (now_ms_u64 等)
        "auto_inject",                         // host_dir_dll 等查询
        "input_manager",                       // 设备管理操作 (request_xinput_reset 等 3 处)
    ];
    let mut files = Vec::new();
    collect_rs(&src_dir().join("gui"), &mut files);
    let mut offenders = Vec::new();
    for f in &files {
        let text = fs::read_to_string(f).unwrap();
        for m in crate_modules(&text) {
            if !ALLOWED.contains(&m.as_str()) {
                offenders.push(format!("{}: crate::{}", f.display(), m));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "gui 引用了允许清单之外的模块 (接线/系统层不得被 GUI 直接触碰; 确需新增请更新本清单并在提交里写明理由):\n{}",
        offenders.join("\n")
    );
}

/// ── 3: config 是叶子 (只许依赖 i18n + util) ──────────────────────────────
#[test]
fn config_stays_leaf() {
    const ALLOWED: &[&str] = &["i18n", "util"];
    let text = fs::read_to_string(src_dir().join("config.rs")).unwrap();
    let deps = crate_modules(&text);
    let unexpected: Vec<_> = deps.iter().filter(|m| !ALLOWED.contains(&m.as_str())).collect();
    assert!(
        unexpected.is_empty(),
        "config.rs 长出了新依赖 {:?} —— 配置模块必须保持叶子 (只许 i18n/util); 确需新增请先过 ADR",
        unexpected
    );
}

/// ── 4: TOML 持久化契约快照 ───────────────────────────────────────────────
/// 键集合按字典序固化。差异 = 有人动了字段名/增删字段:
///   * 新增字段 → 把新键加入 EXPECTED (有意的契约扩展, 提交写明理由);
///   * 改名/删除 → 破坏用户已有 TOML 兼容 (无迁移机制) → 必须先过 ADR。
fn toml_keys<T: serde::Serialize>(value: &T) -> Vec<String> {
    let v = toml::Value::try_from(value).expect("序列化为 toml::Value 失败 (存在非字符串键?)");
    let mut keys: Vec<String> = v.as_table().expect("顶层必须是 table").keys().cloned().collect();
    keys.sort();
    keys
}

/// 快照基线 = R1.1 (2026-09-27)。变更流程见本文件头注释。
const EXPECTED_VIBRATION_KEYS: &[&str] = &[
    "abs_freq_enabled", "abs_freq_max", "abs_freq_window_ms", "adapt_enabled",
    "advanced", "advanced_enabled", "algo_windows_enabled", "attack_gain",
    "damage_gain", "decay_enabled", "decay_ms", "density_enabled", "enabled",
    "font_attack", "font_effect", "font_hit", "font_hp", "font_interval",
    "font_special", "font_state", "font_strength", "hit_boost",
    "hitcap_enabled", "hitcap_max", "hitcap_win_ms", "hitmerge_ms",
    "independent_test", "item_lr", "kill_pulse", "legacy_output_mode",
    "master_gain", "max_strength", "merge_cap", "merge_enabled", "merge_hold",
    "merge_keep", "monster_abnormal_gain", "motor_l_gain", "motor_r_gain",
    "move_charge_enabled", "move_gain", "move_independent", "out_smooth",
    "out_threshold", "pulse_enabled", "random_gain", "rank_decay_delay_ms",
    "rank_decay_enabled", "rank_decay_max_mul", "rank_decay_min_mul",
    "rank_decay_speed", "rank_duration", "rank_level_gain", "rank_lr",
    "rank_type_gain", "remap_enabled", "remap_min", "rhythm", "shake_gain",
    "split_enabled", "split_thr", "start_pulse", "storm_enabled",
    "storm_keep_pct", "storm_mute_abnormal", "storm_mute_rank",
    "storm_pause_ms", "storm_thr", "storm_unified_enabled", "storm_unified_ms",
    "storm_win_ms", "sustain_enabled", "sustain_reduce", "sustain_secs",
    "tail_land_enabled", "tail_land_pct", "throttle_dense_ratio",
    "throttle_max_hits", "throttle_window_ms",
];

const EXPECTED_APP_KEYS: &[&str] = &[
    "always_on_top", "classic_mode", "combo_key_gap_ms", "current_preset",
    "dark_mode", "device_api_preferences", "dfo_player", "event_duration",
    "guide_seen", "hid_baselines", "hid_slot_usages", "input_timeout",
    "interval", "language", "mappings", "minimal_mode",
    "minimal_vib_preset_job", "presets", "process_whitelist",
    "rawinput_capture_mode", "show_notifications", "show_tray_icon",
    "switch_key", "universal_macros", "vib_edition_asked", "vib_legacy_client",
    "whitelist_enabled", "worker_count", "xinput_capture_mode",
];

fn check_contract(name: &str, actual: &[String], expected: &[&str]) {
    let expected: BTreeSet<&str> = expected.iter().copied().collect();
    let actual_set: BTreeSet<&str> = actual.iter().map(|s| s.as_str()).collect();
    let missing: Vec<_> = expected.difference(&actual_set).collect();
    let extra: Vec<_> = actual_set.difference(&expected).collect();
    assert!(
        missing.is_empty() && extra.is_empty(),
        "{} 契约快照不一致!\n  丢失键 (有人删了/改名字段): {:?}\n  新增键 (有意扩展请加入对应 EXPECTED 常量): {:?}",
        name, missing, extra
    );
}

#[test]
fn toml_contract_vibration_config() {
    let actual = toml_keys(&sorahk::config::VibrationConfig::default());
    check_contract("VibrationConfig (Vibration.toml / Vibration-ACT.toml)", &actual, EXPECTED_VIBRATION_KEYS);
}

#[test]
fn toml_contract_app_config() {
    let actual = toml_keys(&sorahk::config::AppConfig::default());
    check_contract("AppConfig (Config.toml)", &actual, EXPECTED_APP_KEYS);
}
