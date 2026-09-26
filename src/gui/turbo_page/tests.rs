//! 连发页单元测试 (原 2248-2410), C4 归位。
use crate::gui::SorahkGui;

#[cfg(test)]
mod preset_switch_conflict_tests {
    use super::SorahkGui;
    use crate::config::{KeyMapping, Preset};

    fn mapping(trigger: &str, turbo: bool) -> KeyMapping {
        KeyMapping {
            sequence_text: String::new(),
            release_targets: Default::default(),
            trigger_key: trigger.to_string(),
            target_keys: smallvec::SmallVec::from_vec(vec!["A".to_string()]),
            interval: None,
            event_duration: None,
            turbo_enabled: turbo,
            move_speed: 5,
            double_tap_enabled: false,
            double_tap_gap_ms: 80,
            run_enabled: false,
            run_threshold: 80,
            run_recheck: true,
            lock_enabled: false,
            note: String::new(),
        }
    }

    fn preset(name: &str, mappings: Vec<KeyMapping>) -> Preset {
        Preset {
            name: name.to_string(),
            mappings,
            switch_key: String::new(),
        }
    }

    #[test]
    fn exact_turbo_mapping_conflicts_and_flags_turbo() {
        let presets = vec![preset("P1", vec![mapping("F6", true)])];
        let hit = SorahkGui::find_preset_switch_conflict_in(&presets, "f6");
        assert_eq!(hit, Some(("P1".into(), "F6".into(), true)));
    }

    #[test]
    fn exact_plain_mapping_conflicts_without_turbo_flag() {
        let presets = vec![preset("P1", vec![mapping("F6", false)])];
        let hit = SorahkGui::find_preset_switch_conflict_in(&presets, "F6");
        assert_eq!(hit, Some(("P1".into(), "F6".into(), false)));
    }

    #[test]
    fn component_of_keyboard_combo_conflicts() {
        // 映射 CTRL+F6 与切换键 F6: 单按 F6 也会切预设 → 必须视为冲突
        let presets = vec![preset("P1", vec![mapping("CTRL+F6", false)])];
        let hit = SorahkGui::find_preset_switch_conflict_in(&presets, "F6");
        assert_eq!(hit, Some(("P1".into(), "CTRL+F6".into(), true)));
    }

    #[test]
    fn gamepad_combo_component_conflicts() {
        let presets = vec![preset("P1", vec![mapping("GAMEPAD_045E_A+B", false)])];
        // 组件 = A / B? 触发键整串按 '+' 切成 ["GAMEPAD_045E_A", "B"]
        let hit = SorahkGui::find_preset_switch_conflict_in(&presets, "GAMEPAD_045E_A");
        assert_eq!(hit, Some(("P1".into(), "GAMEPAD_045E_A+B".into(), true)));
    }

    #[test]
    fn gamepad_combo_does_not_falsely_match_plain_letter() {
        // 切换键 "A"/"B" 不应与手柄组合键 "GAMEPAD_045E_A+B" 冲突
        // (键盘 B ≠ 手柄 B; 组件匹配必须同类输入)
        let presets = vec![preset("P1", vec![mapping("GAMEPAD_045E_A+B", false)])];
        assert_eq!(SorahkGui::find_preset_switch_conflict_in(&presets, "A"), None);
        assert_eq!(SorahkGui::find_preset_switch_conflict_in(&presets, "B"), None);
        // 但手柄侧的组件 (GAMEPAD_045E_A) 必须冲突
        assert!(SorahkGui::find_preset_switch_conflict_in(&presets, "GAMEPAD_045E_A").is_some());
    }

    #[test]
    fn no_conflict_returns_none() {
        let presets = vec![preset("P1", vec![mapping("Q", true), mapping("W", false)])];
        assert_eq!(SorahkGui::find_preset_switch_conflict_in(&presets, "F6"), None);
    }

    #[test]
    fn empty_key_never_conflicts() {
        let presets = vec![preset("P1", vec![mapping("F6", true)])];
        assert_eq!(SorahkGui::find_preset_switch_conflict_in(&presets, "   "), None);
    }

    #[test]
    fn searches_all_presets_not_just_first() {
        let presets = vec![
            preset("P1", vec![mapping("Q", true)]),
            preset("P2", vec![mapping("GAMEPAD_20BC_A", true)]),
        ];
        let hit = SorahkGui::find_preset_switch_conflict_in(&presets, "gamepad_20bc_a");
        assert_eq!(hit, Some(("P2".into(), "GAMEPAD_20BC_A".into(), true)));
    }

    /* ── ★v21.7c 切换键标签分解/合成 (组合键构建器) ── */

    #[test]
    fn decombo_splits_keyboard_and_carries_gamepad_prefix() {
        assert_eq!(
            SorahkGui::decombo_switch_key("CTRL+F6"),
            vec!["CTRL".to_string(), "F6".to_string()]
        );
        assert_eq!(
            SorahkGui::decombo_switch_key("GAMEPAD_045E_A+B"),
            vec!["GAMEPAD_045E_A".to_string(), "GAMEPAD_045E_B".to_string()]
        );
        // 语义名 (含 DEV 段) 本身就是一个标签
        assert_eq!(
            SorahkGui::decombo_switch_key("GAMEPAD_20BC_5159_DEV12345678_H1"),
            vec!["GAMEPAD_20BC_5159_DEV12345678_H1".to_string()]
        );
    }

    #[test]
    fn compact_merges_gamepad_buttons_and_puts_modifiers_first() {
        // compact 保持输入顺序 (排序职责在 normalize_*); 同手柄多键就地合并
        assert_eq!(
            SorahkGui::compact_switch_key_parts(&["F6".into(), "CTRL".into()]),
            "F6+CTRL"
        );
        assert_eq!(
            crate::util::normalize_key_combo("F6+CTRL"),
            "CTRL+F6"
        );
        assert_eq!(
            SorahkGui::compact_switch_key_parts(&[
                "GAMEPAD_045E_A".into(),
                "GAMEPAD_045E_B".into()
            ]),
            "GAMEPAD_045E_A+B"
        );
    }

    #[test]
    fn combo_parts_round_trip_through_compact_decombo() {
        for original in [
            "CTRL+F6",
            "GAMEPAD_045E_A+B",
            "GAMEPAD_20BC_5159_DEV12345678_H1",
            "F6",
        ] {
            let parts = SorahkGui::decombo_switch_key(original);
            assert_eq!(SorahkGui::compact_switch_key_parts(&parts), original, "round-trip {original}");
            // 合成的结果必须能被解析器接受 (否则保存会被拒)
            assert!(
                crate::state::AppState::is_valid_input_name(&SorahkGui::compact_switch_key_parts(&parts)),
                "compact 结果必须合法: {original}"
            );
        }
    }

    #[test]
    fn decombo_rejects_garbage_but_never_panics() {
        assert!(SorahkGui::decombo_switch_key("").is_empty());
        assert!(SorahkGui::decombo_switch_key("++").is_empty());
        // 汉字等非法内容不会被 compact 变成合法键 (保存时再校验)
        let parts = SorahkGui::decombo_switch_key("测试");
        assert_eq!(SorahkGui::compact_switch_key_parts(&parts), "测试");
        assert!(!crate::state::AppState::is_valid_input_name("测试"));
    }
}
