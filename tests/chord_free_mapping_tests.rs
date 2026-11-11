//! ★R2.12 自由手柄组合键的端到端回归 (外部 crate 视角)。
//!
//! 对应 交接_组合键彻查与优化清单_2026-11-11.md 第 4 节的验证用例:
//! GUI 校验 → 建键 → 配置往返 → 运行时仲裁 → 面板分组。
//! 全部纯内存, 不落盘、不占 C 盘。

use sorahk::config::{AppConfig, KeyMapping};
use sorahk::gui::gamepad_mapping::{
    chord_group_label, chord_mapping_indexes, chord_trigger_name, validate_all_chords,
    validate_chord_trigger,
};
use sorahk::xinput::XInputHandler;

const VID: u16 = 0x045E;
const LT: u32 = 0x18;
const A: u32 = 0x0B;
const B: u32 = 0x0C;
const X: u32 = 0x0D;
const Y: u32 = 0x0E;

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

/// 用户原话: A+LT / B+LT / X+LT / Y+LT 四条要能一次建出来 (旧版只能建 1 条)。
#[test]
fn shared_modifier_chord_family_builds_and_roundtrips() {
    let mut cfg = AppConfig::default();
    cfg.mappings.clear();
    let cases = [("A", A, "A"), ("B", B, "S"), ("X", X, "D"), ("Y", Y, "F")];
    for (name, id, target) in cases {
        let ids = vec![id, LT];
        assert!(
            validate_chord_trigger(&cfg, VID, &ids).is_ok(),
            "{name}+LT 应能通过 GUI 建键校验"
        );
        cfg.mappings
            .push(mk(&chord_trigger_name(VID, &ids), target));
    }
    assert_eq!(chord_mapping_indexes(&cfg).len(), 4);
    assert!(
        validate_all_chords(&cfg).is_empty(),
        "四条共享 LT 的组合键体检应无问题: {:?}",
        validate_all_chords(&cfg)
    );

    // 配置序列化往返 (纯内存, 等价于落盘后重启, 但不产生任何临时文件)
    let text = toml::to_string_pretty(&cfg).expect("serialize Config.toml");
    let back: AppConfig = toml::from_str(&text).expect("parse Config.toml");
    assert_eq!(chord_mapping_indexes(&back).len(), 4, "重载后四条都还在");
    assert!(validate_all_chords(&back).is_empty());
    for m in &back.mappings {
        assert_eq!(m.get_target_keys().len(), 1, "目标键不能丢: {:?}", m.trigger_key);
    }
}

/// 完全重复仍要拒绝; 加载期体检要能报出重复与坏名字。
#[test]
fn duplicates_are_rejected_and_reported() {
    let mut cfg = AppConfig::default();
    cfg.mappings.clear();
    let ids = vec![A, LT];
    assert!(validate_chord_trigger(&cfg, VID, &ids).is_ok());
    cfg.mappings.push(mk(&chord_trigger_name(VID, &ids), "A"));
    assert!(
        validate_chord_trigger(&cfg, VID, &ids).is_err(),
        "完全重复应拒绝"
    );

    cfg.mappings.push(mk(&chord_trigger_name(VID, &ids), "B"));
    cfg.mappings.push(mk("GAMEPAD_045E_LS_RightUp+A", "Q"));
    let issues = validate_all_chords(&cfg);
    assert!(
        issues.iter().any(|s| s.contains("完全重复")),
        "重复组合键应被报出: {issues:?}"
    );
    assert!(
        issues.iter().any(|s| s.contains("不是可解析")),
        "斜向名无法解析, 应被报出: {issues:?}"
    );
}

/// 同时按住三个成员时只触发一条组合 (放开共享键规则后的安全网)。
#[test]
fn concurrent_chord_hits_are_arbitrated_to_one() {
    let lt_a = vec![A, LT];
    let lt_b = vec![B, LT];
    let lt_x = vec![X, LT];
    let all = vec![lt_a.clone(), lt_b.clone(), lt_x.clone()];
    for (name, cur) in [
        ("A", vec![A, LT]),
        ("B", vec![B, LT]),
        ("X", vec![X, LT]),
    ] {
        let out = XInputHandler::arbitrate_matched_combos(&cur, all.clone());
        assert_eq!(out.len(), 1, "{name}+LT 应只命中一条: {out:?}");
    }
    let triple = XInputHandler::arbitrate_matched_combos(&[A, B, LT], all.clone());
    assert_eq!(
        triple.iter().filter(|c| c.len() >= 2).count(),
        1,
        "三键同按也只触发一条: {triple:?}"
    );
}

/// 面板分组: 共享 LT 的一族组合都归到 LT 组, 40 条时也能定位。
#[test]
fn shared_modifier_chords_group_together() {
    assert_eq!(chord_group_label(&[A, LT]), "LT 组");
    assert_eq!(chord_group_label(&[B, LT]), "LT 组");
    assert_eq!(chord_group_label(&[X, LT]), "LT 组");
    assert_eq!(chord_group_label(&[Y, LT]), "LT 组");
    assert_eq!(chord_group_label(&[0x09, X]), "LB 组");
    assert_eq!(chord_group_label(&[A, B]), "其它组合");
}
