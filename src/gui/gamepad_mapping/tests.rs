//! 手柄页单元测试 (原 2747-3178), C3 归位。
use super::*;
use crate::config::{AppConfig, KeyMapping};

#[cfg(test)]
mod live_highlight_tests {
    use super::*;
    use crate::state::LiveHidState;

    /// 构造一个"原始位组合"实时状态 (v23.0 起: 高亮只看 raw_position)。
    fn live_raw(pos: u32) -> LiveHidState {
        LiveHidState {
            vid: 0x20BC,
            pid: 0x5159,
            raw_position: pos,
            ..Default::default()
        }
    }

    #[test]
    fn slot_note_is_system_format() {
        assert_eq!(slot_note("A 键"), "手柄A 键（系统）");
        assert_eq!(slot_note("十字键·上"), "手柄十字键·上（系统）");
    }

    /// 只按系统备注关联映射 —— **不能**按 default_trigger / 旧备注串味 (那是双触发根源)。
    #[test]
    fn find_slot_mapping_matches_only_system_note() {
        use crate::config::KeyMapping;
        let mk = |trigger: &str, note: &str| KeyMapping {
            sequence_text: String::new(),
            release_targets: Default::default(),
            trigger_key: trigger.to_string(),
            target_keys: Default::default(),
            interval: None,
            event_duration: None,
            turbo_enabled: true,
            move_speed: 5,
            double_tap_enabled: false,
            double_tap_gap_ms: 50,
            run_enabled: false,
            run_threshold: 80,
            run_recheck: true,
            lock_enabled: false,
            note: note.to_string(),
        };
        let slot = get_slot(19).unwrap(); // A 键
        let mut cfg = AppConfig::default();
        // 只有旧 XInput 默认名 + 旧备注 → 不认
        cfg.mappings.push(mk("GAMEPAD_045E_A", "手柄·A 键"));
        assert_eq!(find_slot_mapping_index(&cfg, slot), None);
        // 有系统备注 → 认
        cfg.mappings.push(mk("GAMEPAD_20BC_5159_X_B1.2", "手柄A 键（系统）"));
        let expect = cfg.mappings.len() - 1;
        assert_eq!(find_slot_mapping_index(&cfg, slot), Some(expect));
    }

    /// ★v24.0: 旧备注一次性迁移为系统备注 (触发键/目标键保留); 重复条目去重。
    #[test]
    fn migrate_slot_notes_renames_and_dedupes() {
        use crate::config::KeyMapping;
        let mk = |trigger: &str, note: &str| KeyMapping {
            sequence_text: String::new(),
            release_targets: Default::default(),
            trigger_key: trigger.to_string(),
            target_keys: vec!["Q".to_string()].into(),
            interval: None,
            event_duration: None,
            turbo_enabled: true,
            move_speed: 5,
            double_tap_enabled: false,
            double_tap_gap_ms: 50,
            run_enabled: false,
            run_threshold: 80,
            run_recheck: true,
            lock_enabled: false,
            note: note.to_string(),
        };
        let mut cfg = AppConfig::default();
        cfg.mappings.push(mk("GAMEPAD_20BC_5159_X_B1.2", "手柄·A 键"));
        cfg.mappings.push(mk("GAMEPAD_20BC_5159_X_B1.2", "手柄·A 键")); // 重复
        assert!(migrate_slot_notes(&mut cfg));
        let a: Vec<&KeyMapping> = cfg
            .mappings
            .iter()
            .filter(|m| m.note == "手柄A 键（系统）")
            .collect();
        assert_eq!(a.len(), 1, "同槽位只保留一条");
        assert_eq!(a[0].trigger_key, "GAMEPAD_20BC_5159_X_B1.2", "触发键保留");
        assert_eq!(a[0].target_keys.len(), 1, "目标键保留");
        // 幂等
        assert!(!migrate_slot_notes(&mut cfg));
    }

    /// 高亮判定: 必须"同设备前缀 + 同位组合"才匹配; 0 位置(未按)不匹配。
    #[test]
    fn trigger_matches_raw_requires_same_device_and_position() {
        let name = "GAMEPAD_20BC_5159_DEVDEADBEEF_B1.7";
        assert!(trigger_matches_raw(name, 0x20BC, 0x5159, 0x0001_0007));
        // 位置不同
        assert!(!trigger_matches_raw(name, 0x20BC, 0x5159, 0x0002_0007));
        // 未按下
        assert!(!trigger_matches_raw(name, 0x20BC, 0x5159, 0));
        // 设备不同
        assert!(!trigger_matches_raw(name, 0x045E, 0x028E, 0x0001_0007));
        // 完全不是原始命名
        assert!(!trigger_matches_raw("GAMEPAD_045E_A", 0x20BC, 0x5159, 0x0001_0007));
    }

    #[test]
    fn calibration_order_covers_all_hotspots_without_duplicates() {
        assert_eq!(CALIBRATION_ORDER.len(), SLOTS.len());
        assert_eq!(calibration_slot(0), Some(19)); // A 起
        assert_eq!(calibration_slot(23), Some(15)); // 右摇杆·右 止
        assert_eq!(calibration_slot(CALIBRATION_ORDER.len()), None);
        let mut seen = std::collections::HashSet::new();
        for id in CALIBRATION_ORDER {
            assert!(get_slot(*id).is_some(), "无效槽位 {id}");
            assert!(seen.insert(*id), "重复槽位 {id}");
        }
        for slot in SLOTS {
            assert!(seen.contains(&slot.id), "漏了槽位 {} ({})", slot.id, slot.label);
        }
    }

    #[test]
    fn mouse_vks_are_filtered() {
        for vk in [0x01u32, 0x02, 0x04, 0x05, 0x06] {
            assert!(is_mouse_vk(vk));
        }
        assert!(!is_mouse_vk(0x41)); // A
        assert!(!is_mouse_vk(0x20)); // SPACE
    }
}

/// ★v24.2: 按压窗口合并 + 按槽位挑选 —— 校准向导回归测试。
#[cfg(test)]
mod pad_capture_reconcile_tests {
    use super::*;
    use crate::state::{DeviceType, InputDevice};
    use std::time::{Duration, Instant};

    /// 原始 HID 通道事件 (第三方手柄, 走 RawInput)。
    fn raw(pos: u64) -> InputDevice {
        InputDevice::GenericDevice {
            device_type: DeviceType::Gamepad(0x20BC),
            button_id: (1u64 << 32) | pos,
        }
    }

    /// XInput 通道事件 (命名恒为 `GAMEPAD_045E_*`)。
    fn xinput(ids: &[u32]) -> InputDevice {
        InputDevice::XInputCombo {
            device_type: DeviceType::Gamepad(0x045E),
            button_ids: ids.to_vec(),
        }
    }

    fn xinput1(id: u32) -> InputDevice {
        xinput(&[id])
    }

    /// 模拟校准向导: 按时间轴喂入事件, 用 [`pick_calibration_device`] 挑输入。
    /// 返回 (每步写入的 (槽位标签, 触发键), 被拒绝时的提示)。
    fn run_wizard(
        events: &[(InputDevice, Duration)],
        hold: Duration,
    ) -> (Vec<(String, String)>, Vec<String>) {
        let mut rec = PadCaptureReconciler::new(hold);
        let mut cfg = AppConfig::default();
        let mut step = 0usize;
        let mut out = Vec::new();
        let mut errs = Vec::new();
        let base = Instant::now();
        let mut handle = |cands: PadCaptureCandidates,
                          cfg: &mut AppConfig,
                          step: &mut usize,
                          out: &mut Vec<(String, String)>,
                          errs: &mut Vec<String>| {
            let Some(slot_id) = calibration_slot(*step) else {
                return;
            };
            let Some(slot) = get_slot(slot_id) else {
                return;
            };
            match pick_calibration_device(slot, &cands) {
                Ok(dev) => {
                    let name = dev.to_string();
                    set_slot_trigger(cfg, slot, name.clone());
                    out.push((slot.label.to_string(), name));
                    *step += 1;
                }
                Err(e) => errs.push(e),
            }
        };
        for (dev, offset) in events {
            let now = base + *offset;
            if let Some(c) = rec.feed(dev.clone(), now) {
                handle(c, &mut cfg, &mut step, &mut out, &mut errs);
                continue;
            }
            if let Some(c) = rec.tick(now) {
                handle(c, &mut cfg, &mut step, &mut out, &mut errs);
            }
        }
        // 收尾: 让仍挂起的窗口结算 (正常 UI 每帧都会 tick)。
        let end = base + hold * 2 + Duration::from_secs(1);
        if let Some(c) = rec.tick(end) {
            handle(c, &mut cfg, &mut step, &mut out, &mut errs);
        }
        (out, errs)
    }

    /// 实机 bug: 按一个键, A 槽 (原始命名) + B 槽 (`GAMEPAD_045E_A`) 同时被填。
    #[test]
    fn one_press_with_dual_channel_writes_one_slot() {
        let events = vec![
            (raw(0x0001_0007), Duration::from_millis(0)),
            (xinput1(0x0B), Duration::from_millis(6)),
        ];
        let (steps, errs) = run_wizard(&events, Duration::from_millis(PAD_CAPTURE_HOLD_MS));
        assert_eq!(steps.len(), 1, "一次按压只能记一步, 实际: {steps:?} {errs:?}");
        assert_eq!(steps[0].0, "A 键", "应记入当前步 (A 键), 实际: {steps:?}");
        assert!(
            steps[0].1.starts_with("GAMEPAD_20BC_"),
            "按钮槽位应优先原始命名, 实际: {}",
            steps[0].1
        );
    }

    /// XInput 事件先到时也要折叠成一步, 并仍优先原始命名。
    #[test]
    fn xinput_first_still_prefers_raw_name() {
        let events = vec![
            (xinput1(0x0B), Duration::from_millis(0)),
            (raw(0x0001_0007), Duration::from_millis(8)),
        ];
        let (steps, errs) = run_wizard(&events, Duration::from_millis(PAD_CAPTURE_HOLD_MS));
        assert_eq!(steps.len(), 1, "反序到达也要折叠成一步: {steps:?} {errs:?}");
        assert!(steps[0].1.starts_with("GAMEPAD_20BC_"), "实际: {}", steps[0].1);
    }

    /// 官方 Xbox 手柄被 RawInput 忽略 → 只会收到 XInput 事件, 不能丢。
    #[test]
    fn lone_xinput_event_commits_after_hold() {
        let events = vec![(xinput1(0x0B), Duration::from_millis(0))];
        let (steps, errs) = run_wizard(&events, Duration::from_millis(PAD_CAPTURE_HOLD_MS));
        assert_eq!(steps.len(), 1, "单通道事件不能丢: {steps:?} {errs:?}");
        assert_eq!(steps[0].1, "GAMEPAD_045E_A");
    }

    /// 两次按压间隔超过窗口 → 正常记两步 (窗口不能无限大)。
    #[test]
    fn two_presses_beyond_window_are_recorded_separately() {
        let events = vec![
            (raw(0x0001_0007), Duration::from_millis(0)),
            (raw(0x0002_0007), Duration::from_millis(900)),
        ];
        let (steps, errs) = run_wizard(&events, Duration::from_millis(PAD_CAPTURE_HOLD_MS));
        assert_eq!(steps.len(), 2, "超出窗口的两次按压应各记一步: {steps:?} {errs:?}");
        assert_ne!(steps[0].1, steps[1].1);
    }

    /// 窗口按用户实测定为 300ms。
    #[test]
    fn default_window_is_300ms() {
        assert_eq!(PAD_CAPTURE_HOLD_MS, 300);
        let hold = Duration::from_millis(PAD_CAPTURE_HOLD_MS);
        let base = Instant::now();
        // 迟到 200ms 的同一次按压 → 仍在窗口内, 合并
        let mut rec = PadCaptureReconciler::new(hold);
        assert!(rec.feed(raw(0x0001_0007), base).is_none());
        assert!(rec.feed(xinput1(0x0B), base + Duration::from_millis(200)).is_none());
        assert!(rec.tick(base + hold + Duration::from_millis(1)).is_some());
        // 迟到 400ms → 超出窗口, 上一次先结算
        let mut rec = PadCaptureReconciler::new(hold);
        assert!(rec.feed(raw(0x0001_0007), base).is_none());
        assert!(
            rec.feed(xinput1(0x0B), base + Duration::from_millis(400)).is_some(),
            "超出窗口应先结算上一次"
        );
    }

    /// 实机症状: 右摇杆按下后回弹 (`RS_Click` → `RS_Right`) 都在同一次按压里,
    /// 合并后按"摇杆按下"槽位过滤 → 只留 click, 回弹被丢掉。
    #[test]
    fn stick_click_rebound_is_filtered_to_the_click() {
        let hold = Duration::from_millis(PAD_CAPTURE_HOLD_MS);
        let base = Instant::now();
        let mut rec = PadCaptureReconciler::new(hold);
        assert!(rec.feed(xinput1(0x08), base).is_none()); // RS_Click
        assert!(rec.feed(xinput1(0x14), base + Duration::from_millis(200)).is_none()); // RS_Right 回弹
        let c = rec.tick(base + hold + Duration::from_millis(1)).expect("窗口应结算");
        let slot = get_slot(23).unwrap(); // 右摇杆·按下
        let got = pick_calibration_device(slot, &c).expect("回弹应被过滤, 只留 click");
        assert_eq!(got.to_string(), "GAMEPAD_045E_RS_Click");
    }

    /// 斜推摇杆 (两个方向) → 判为不干净, 让用户重按 (不能把斜推记成一个方向)。
    #[test]
    fn diagonal_stick_push_is_rejected() {
        let slot = get_slot(10).unwrap(); // 左摇杆·左
        let mut c = PadCaptureCandidates::default();
        c.xinput = Some(xinput(&[0x11, 0x13])); // LS_Left + LS_Down
        assert!(pick_calibration_device(slot, &c).is_err(), "斜推应被判为不干净");

        let slot_click = get_slot(22).unwrap(); // 左摇杆·按下
        let mut c2 = PadCaptureCandidates::default();
        c2.xinput = Some(xinput(&[0x14])); // 只有方向、没有 click
        assert!(pick_calibration_device(slot_click, &c2).is_err(), "缺 click 应重按");
    }

    /// 摇杆槽位优先 XInput (摇杆语义只有 XInput 可靠), 按钮槽位优先原始命名。
    #[test]
    fn stick_slots_prefer_xinput_button_slots_prefer_raw() {
        let stick = get_slot(10).unwrap(); // 左摇杆·左
        let mut c = PadCaptureCandidates::default();
        c.raw = Some(raw(0x0002_0007));
        c.xinput = Some(xinput(&[0x11]));
        assert_eq!(
            pick_calibration_device(stick, &c).unwrap().to_string(),
            "GAMEPAD_045E_LS_Left"
        );

        let button = get_slot(19).unwrap(); // A 键
        let mut c2 = PadCaptureCandidates::default();
        c2.raw = Some(raw(0x0001_0007));
        assert!(pick_calibration_device(button, &c2).unwrap().to_string().starts_with("GAMEPAD_20BC_"));

        let mut c3 = PadCaptureCandidates::default();
        c3.xinput = Some(xinput(&[0x0B, 0x0C])); // A+B 同时按
        assert!(pick_calibration_device(button, &c3).is_err(), "多键应被拒绝");
    }

    /// 旧数据串位自愈: 该键已被别的槽位占用时, 移到当前槽位而不是拒绝
    /// (实机症状: A 键的位组合被旧配置记在 X 槽 → 重新校对按 A 被拦下)。
    #[test]
    /// ★v24.18: 用户报的 bug —— 手柄映射页把键位从「X+A」重设为「X」必须生效。
    /// 根因: 确认捕获走了"追加"(add_target_key), X 已在集合里 → 集合不变。
    #[test]
    fn set_slot_targets_replaces_instead_of_appending() {
        let mut cfg = AppConfig::default();
        let s = get_slot(0).unwrap(); /* A 键 */
        set_slot_trigger(&mut cfg, s, "GAMEPAD_045E_ABXY_A".to_string());
        assert!(add_slot_target(&mut cfg, s, "X".to_string()));
        assert!(add_slot_target(&mut cfg, s, "A".to_string()));
        assert_eq!(slot_targets(&cfg, s).len(), 2, "先造出 X+A");

        /* 重设为单个 X —— 必须只剩 X (旧行为: 追加后仍是 X+A, 看着"没生效") */
        assert!(
            set_slot_targets(&mut cfg, s, "X".to_string()),
            "从 X+A 改成 X 应判定为发生了修改"
        );
        assert_eq!(slot_targets(&cfg, s), vec!["X".to_string()], "应替换而不是追加");

        /* 再设成组合键 (第 2 步「＋ 再加一个键」累加后的形态) */
        assert!(set_slot_targets(&mut cfg, s, "X+A".to_string()));
        assert_eq!(slot_targets(&cfg, s).len(), 2);
        /* 设成与当前完全相同的内容 → 报告"没变化" */
        assert!(!set_slot_targets(&mut cfg, s, "X+A".to_string()));
        /* 未校对的槽位不动 */
        let s2 = get_slot(1).unwrap();
        assert!(!set_slot_targets(&mut cfg, s2, "Y".to_string()));
    }

    /// ★v24.26: 用户需求 —— 目标键允许重复: 「↓+Z」之后再补一个 Z → 「↓+Z+Z」
    /// (序列语义: 连发时依次按 ↓+Z、再按 Z)。当初的去重 (v21.7d) 是为追加语义时代的
    /// "看不出来生效"兜底; v24.18 换成替换语义后, 重复不再有歧义, 按用户要求放开。
    #[test]
    fn set_slot_targets_allows_duplicate_keys() {
        let mut cfg = AppConfig::default();
        let s = get_slot(0).unwrap(); /* A 键 */
        set_slot_trigger(&mut cfg, s, "GAMEPAD_045E_ABXY_A".to_string());
        assert!(set_slot_targets(&mut cfg, s, "DOWN+Z".to_string()));
        /* ↓+Z 已存在时再设 ↓+Z+Z —— 当前被去重 → 判定"没变化"拒绝 (本测试要修的红) */
        assert!(
            set_slot_targets(&mut cfg, s, "DOWN+Z+Z".to_string()),
            "↓+Z+Z 应可设置 (重复键保留)"
        );
        assert_eq!(
            slot_targets(&cfg, s),
            vec!["DOWN".to_string(), "Z".to_string(), "Z".to_string()],
            "重复键应保留 (序列语义)"
        );
        /* 大小写不同的同名键 → 规范成大写后与当前完全相同 → 报告"没变化" (内容仍 ↓+Z+Z) */
        assert!(!set_slot_targets(&mut cfg, s, "DOWN+z+z".to_string()));
        assert_eq!(
            slot_targets(&cfg, s),
            vec!["DOWN".to_string(), "Z".to_string(), "Z".to_string()]
        );
        /* 旧修复不回退: X+A 改 X 依然生效 (v24.18) */
        assert!(set_slot_targets(&mut cfg, s, "X".to_string()));
        assert_eq!(slot_targets(&cfg, s), vec!["X".to_string()]);
    }

    fn set_slot_trigger_moves_conflicting_key() {
        let mut cfg = AppConfig::default();
        let a = get_slot(19).unwrap();
        let x = get_slot(16).unwrap();
        set_slot_trigger(&mut cfg, x, "GAMEPAD_20BC_5159_DEVAA_H123".to_string());
        let moved =
            set_slot_trigger_moving(&mut cfg, a, "GAMEPAD_20BC_5159_DEVAA_H123".to_string());
        assert_eq!(moved.as_deref(), Some("X 键"), "应报告从哪个槽位移走");
        assert!(find_slot_mapping_index(&cfg, a).is_some(), "当前槽位应写入");
        assert!(find_slot_mapping_index(&cfg, x).is_none(), "旧槽位应被清掉");
        assert_eq!(
            slot_trigger_display(&cfg, a),
            "GAMEPAD_20BC_5159_DEVAA_H123"
        );
    }

    /// ★v24.6: XInput 实时位图匹配 (快速映射识别 XInput 命名槽位, 不依赖捕获模式)。
    #[test]
    fn xinput_live_match() {
        let ls_left = 1u32 << 0x11;
        assert!(trigger_matches_xinput("GAMEPAD_045E_LS_Left", 0x045E, ls_left));
        assert!(!trigger_matches_xinput("GAMEPAD_045E_LS_Left", 0x045E, 1u32 << 0x10));
        assert!(!trigger_matches_xinput("GAMEPAD_045E_LS_Left", 0x20BC, ls_left));
        assert!(!trigger_matches_xinput("GAMEPAD_20BC_5159_DEVAA_H1", 0x045E, ls_left));
        assert!(!trigger_matches_xinput("GAMEPAD_045E_LS_Left", 0x045E, 0));
        // 组合键: 必须所有 id 都在位图里
        let ab = (1u32 << 0x0B) | (1u32 << 0x0C);
        assert!(trigger_matches_xinput("GAMEPAD_045E_A+B", 0x045E, ab));
        assert!(!trigger_matches_xinput("GAMEPAD_045E_A+B", 0x045E, 1u32 << 0x0B));
    }

    /// 触发键短名: 原始命名去掉设备前缀, XInput 命名去掉 GAMEPAD_。
    #[test]
    fn short_trigger_strips_device_prefix() {
        assert_eq!(
            short_trigger_name("GAMEPAD_20BC_5159_DEV5A27EA46_H680322833"),
            "H680322833"
        );
        assert_eq!(short_trigger_name("GAMEPAD_045E_RS_Click"), "045E_RS_Click");
    }
}
