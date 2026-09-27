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

/// ★v24.38 序列宏即时生效契约 (用户实测 bug: 录制序列后触发仍是旧目标键 A):
/// 序列编辑器的**一切**文本变更 (录制填入/加步/清空/改文本) 必须立即落盘+热重载,
/// 引擎侧 sequence 立刻接管 —— v24.32 只有「＋按键步」路径做了即时生效,
/// 录制等路径只改 GUI 内存, 手柄页 (无保存按钮) 永远不生效。
#[cfg(test)]
mod sequence_apply_tests {
    use super::SorahkGui;
    use crate::config::{AppConfig, KeyMapping};
    use crate::state::{AppState, InputDevice};
    use std::sync::Arc;

    fn gui_with_target_a() -> (SorahkGui, Arc<AppState>, InputDevice, usize) {
        let mut config = AppConfig::default();
        config.mappings.push(KeyMapping {
            sequence_text: String::new(),
            release_targets: Default::default(),
            trigger_key: "F8".to_string(),
            target_keys: smallvec::SmallVec::from_vec(vec!["A".to_string()]),
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
        });
        /* ⚠ AppConfig::default() 自带 1 条 Q→Q 映射 —— 按触发键定位, 不写死下标 */
        let idx = config
            .mappings
            .iter()
            .position(|m| m.trigger_key == "F8")
            .unwrap();
        let state = Arc::new(AppState::new(config.clone()).unwrap());
        let gui = SorahkGui::new(state.clone(), config);
        let dev = AppState::input_name_to_device("F8").unwrap();
        (gui, state, dev, idx)
    }

    #[test]
    fn sequence_edit_takes_over_engine_immediately() {
        let (mut gui, state, dev, idx) = gui_with_target_a();
        /* 初始: 无序列 → 目标键 A 生效 */
        let m = state.get_input_mapping(&dev).expect("映射应存在");
        assert!(m.sequence.is_none(), "前置: 初始无序列");

        /* 用户录了 J 按住 100ms (完整按下→等待→抬起) 并点「停止并填入」 */
        gui.apply_sequence_text(idx, "J↓»NONE⏱100»J↑".to_string());

        let m = state.get_input_mapping(&dev).expect("映射应存在");
        assert!(
            m.sequence.is_some(),
            "★序列必须立即接管引擎 (旧 bug: 只改内存不落盘, 触发仍直接按 A)"
        );
        /* 注: mapping 层 target_action 保留原目标键数据, 但 worker 的序列分支
         * 优先且登记占位 DevRuntime (KeyboardKey(0) 永不注入) —— 见 keyboard.rs */
    }

    #[test]
    fn sequence_edit_bad_syntax_disables_mapping_not_falls_back() {
        let (mut gui, state, dev, idx) = gui_with_target_a();
        /* 解析失败: 整条跳过 (不回退目标键) —— 与引擎 v24.35 契约一致 */
        gui.apply_sequence_text(idx, "NONE".to_string());
        let m = state.get_input_mapping(&dev);
        assert!(
            m.is_none(),
            "序列解析失败 → 映射整体不生效 (防行为突变), 而不是悄悄按 A"
        );
    }
}
