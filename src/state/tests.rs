//! AppState 单元测试 —— 原 state.rs 内嵌 mod tests (原 4099-5678 行), 2026-09-27 架构重构 B1 归位。
//! 经 use super::* 访问 state 模块树内可见项; 被测私有方法随所在文件迁移, 跨文件调用按编译器指引放宽为 pub(super)。

    use super::*;
    use crate::config::KeyMapping;
    use crate::config::default_double_tap_gap_ms;

    #[test]
    fn test_key_name_to_vk_letters() {
        assert_eq!(AppState::key_name_to_vk("A"), Some(0x41));
        assert_eq!(AppState::key_name_to_vk("Z"), Some(0x5A));
        assert_eq!(AppState::key_name_to_vk("a"), Some(0x41)); // Case insensitive
        assert_eq!(AppState::key_name_to_vk("m"), Some(0x4D));
    }

    #[test]
    fn test_key_name_to_vk_numbers() {
        assert_eq!(AppState::key_name_to_vk("0"), Some(0x30));
        assert_eq!(AppState::key_name_to_vk("5"), Some(0x35));
        assert_eq!(AppState::key_name_to_vk("9"), Some(0x39));
    }

    #[test]
    fn test_key_name_to_vk_function_keys() {
        assert_eq!(AppState::key_name_to_vk("F1"), Some(0x70));
        assert_eq!(AppState::key_name_to_vk("F12"), Some(0x7B));
        assert_eq!(AppState::key_name_to_vk("F24"), Some(0x87));
        assert_eq!(AppState::key_name_to_vk("f5"), Some(0x74)); // Case insensitive
    }

    #[test]
    fn test_key_name_to_vk_special_keys() {
        assert_eq!(AppState::key_name_to_vk("ESC"), Some(0x1B));
        assert_eq!(AppState::key_name_to_vk("ENTER"), Some(0x0D));
        assert_eq!(AppState::key_name_to_vk("TAB"), Some(0x09));
        assert_eq!(AppState::key_name_to_vk("SPACE"), Some(0x20));
        assert_eq!(AppState::key_name_to_vk("BACKSPACE"), Some(0x08));
        assert_eq!(AppState::key_name_to_vk("DELETE"), Some(0x2E));
        assert_eq!(AppState::key_name_to_vk("INSERT"), Some(0x2D));
    }

    #[test]
    fn test_key_name_to_vk_arrow_keys() {
        assert_eq!(AppState::key_name_to_vk("UP"), Some(0x26));
        assert_eq!(AppState::key_name_to_vk("DOWN"), Some(0x28));
        assert_eq!(AppState::key_name_to_vk("LEFT"), Some(0x25));
        assert_eq!(AppState::key_name_to_vk("RIGHT"), Some(0x27));
    }

    #[test]
    fn test_key_name_to_vk_modifier_keys() {
        assert_eq!(AppState::key_name_to_vk("LSHIFT"), Some(0xA0));
        assert_eq!(AppState::key_name_to_vk("RSHIFT"), Some(0xA1));
        assert_eq!(AppState::key_name_to_vk("LCTRL"), Some(0xA2));
        assert_eq!(AppState::key_name_to_vk("RCTRL"), Some(0xA3));
        assert_eq!(AppState::key_name_to_vk("LALT"), Some(0xA4));
        assert_eq!(AppState::key_name_to_vk("RALT"), Some(0xA5));
    }

    #[test]
    fn test_key_name_to_vk_navigation_keys() {
        assert_eq!(AppState::key_name_to_vk("HOME"), Some(0x24));
        assert_eq!(AppState::key_name_to_vk("END"), Some(0x23));
        assert_eq!(AppState::key_name_to_vk("PAGEUP"), Some(0x21));
        assert_eq!(AppState::key_name_to_vk("PAGEDOWN"), Some(0x22));
    }

    #[test]
    fn test_key_name_to_vk_invalid() {
        assert_eq!(AppState::key_name_to_vk("INVALID"), None);
        assert_eq!(AppState::key_name_to_vk("F25"), None);
        assert_eq!(AppState::key_name_to_vk("F0"), None);
        assert_eq!(AppState::key_name_to_vk(""), None);
        assert_eq!(AppState::key_name_to_vk("ABC"), None);
    }

    #[test]
    fn test_vk_to_scancode_letters() {
        assert_eq!(AppState::vk_to_scancode(0x41), 0x1E); // A
        assert_eq!(AppState::vk_to_scancode(0x42), 0x30); // B
        assert_eq!(AppState::vk_to_scancode(0x5A), 0x2C); // Z
    }

    #[test]
    fn test_vk_to_scancode_numbers() {
        assert_eq!(AppState::vk_to_scancode(0x30), 0x0B); // 0
        assert_eq!(AppState::vk_to_scancode(0x31), 0x02); // 1
        assert_eq!(AppState::vk_to_scancode(0x39), 0x0A); // 9
    }

    #[test]
    fn test_vk_to_scancode_function_keys() {
        assert_eq!(AppState::vk_to_scancode(0x70), 0x3B); // F1
        assert_eq!(AppState::vk_to_scancode(0x7B), 0x58); // F12
    }

    #[test]
    fn test_vk_to_scancode_special_keys() {
        assert_eq!(AppState::vk_to_scancode(0x1B), 0x01); // ESC
        assert_eq!(AppState::vk_to_scancode(0x0D), 0x1C); // ENTER
        assert_eq!(AppState::vk_to_scancode(0x20), 0x39); // SPACE
    }

    #[test]
    fn test_vk_to_scancode_invalid() {
        assert_eq!(AppState::vk_to_scancode(0xFF), 0); // Invalid VK code
        assert_eq!(AppState::vk_to_scancode(0x00), 0); // No mapping
    }

    /// ★v24.13: `apply_vibration_config` 必须把 VibrationConfig 忠实灌进运行态。
    ///
    /// 注意: 不能拿 `new()` 与 `apply()` 全字段比对 —— `AppState::new` 有两条"启动叠加"
    /// (内置预设 pristine 兜底 / JobVibration 职业应用) 会改写 preset 家族的字段,
    /// 那是刻意行为。这里只比对**职业叠加不拥有**的字段, 并直接回读校验取值。
    #[test]
    fn apply_vibration_config_writes_runtime_fields() {
        use std::sync::atomic::Ordering::Relaxed;
        let mut c = AppConfig::default();
        // 非 pristine (避免 new 的内置预设兜底改写) —— 本测试只关心 apply 的写入
        c.vibration.attack_gain = 100;
        c.vibration.master_gain = 100;
        // 职业叠加不拥有的字段
        c.vibration.split_enabled = true;
        c.vibration.split_thr = 42;
        c.vibration.out_threshold = 17;
        c.vibration.remap_min = 33;
        c.vibration.remap_enabled = true;
        c.vibration.item_lr[3] = 77;

        let s = AppState::new(AppConfig::default()).expect("state");
        s.apply_vibration_config(&c.vibration);

        assert!(s.vibration_split_enabled.load(Relaxed));
        assert_eq!(s.vibration_split_thr.load(Relaxed), 42);
        assert_eq!(s.vibration_out_threshold.load(Relaxed), 17);
        assert_eq!(s.vibration_remap_min.load(Relaxed), 33);
        assert!(s.vibration_remap_enabled.load(Relaxed));
        assert_eq!(s.vibration_item_lr[3].load(Relaxed), 77);
        // 60 槽参数数组也要跟着走 (advanced[10] → params[29])
        c.vibration.advanced[10] = 1234;
        s.apply_vibration_config(&c.vibration);
        assert_eq!(s.vibration_params[29].load(Relaxed), 1234);
    }

    #[test]
    fn test_create_input_mappings_valid() {
        let mut config = AppConfig::default();
        config.mappings = vec![
            
                KeyMapping {
                    sequence_text: String::new(),
                release_targets: Default::default(),
                trigger_key: "A".to_string(),
                target_keys: SmallVec::from_vec(vec!["B".to_string()]),
                interval: Some(10),
                event_duration: Some(5),
                turbo_enabled: true,
            move_speed: 5,
                double_tap_enabled: false,
                double_tap_gap_ms: default_double_tap_gap_ms(),
                run_enabled: false,
                run_threshold: 80,
                run_recheck: true,
                lock_enabled: false,

                note: String::new(),
            },
            
                KeyMapping {
                    sequence_text: String::new(),
                    release_targets: Default::default(),
                trigger_key: "F1".to_string(),
                target_keys: SmallVec::from_vec(vec!["SPACE".to_string()]),
                interval: None,
                event_duration: None,
                turbo_enabled: true,
                move_speed: 10,                double_tap_enabled: false,
                double_tap_gap_ms: default_double_tap_gap_ms(),
                run_enabled: false,
                run_threshold: 80,
                run_recheck: true,
                lock_enabled: false,

                note: String::new(),
            },
        ];

        let result = AppState::create_input_mappings(&config);
        assert!(result.is_ok());

        let input_mappings = result.unwrap();
        assert_eq!(input_mappings.len(), 2);

        let device_a = InputDevice::Keyboard(0x41); // 'A' key
        let a_mapping = input_mappings.get(&device_a).unwrap();
        assert_eq!(a_mapping.interval, 10);
        assert_eq!(a_mapping.event_duration, 5);

        let device_f1 = InputDevice::Keyboard(0x70); // F1 key
        let f1_mapping = input_mappings.get(&device_f1).unwrap();
        assert_eq!(f1_mapping.interval, 5); // Default interval
        assert_eq!(f1_mapping.event_duration, 5); // Default duration
    }

    #[test]
    fn test_create_input_mappings_invalid_trigger() {
        /* ★v23.1: 坏触发键从"致命错误"改为"跳过 + 日志" —— 单条脏数据不得阻断启动
         * (用户实测: 历史遗留坏触发键曾让程序完全起不来)。 */
        let mut config = AppConfig::default();
        config.mappings = vec![
            KeyMapping {
                sequence_text: String::new(),
                trigger_key: "INVALID_KEY".to_string(),
                target_keys: SmallVec::from_vec(vec!["A".to_string()]),
                interval: None,
                event_duration: None,
                turbo_enabled: true,
                move_speed: 10,
                double_tap_enabled: false,
                double_tap_gap_ms: default_double_tap_gap_ms(),
                run_enabled: false,
                run_threshold: 80,
                run_recheck: true,
                lock_enabled: false,
                release_targets: Default::default(),
                note: String::new(),
            },
            KeyMapping {
                sequence_text: String::new(),
                release_targets: Default::default(),
                trigger_key: "B".to_string(),
                target_keys: SmallVec::from_vec(vec!["C".to_string()]),
                interval: None,
                event_duration: None,
                turbo_enabled: true,
                move_speed: 10,
                double_tap_enabled: false,
                double_tap_gap_ms: default_double_tap_gap_ms(),
                run_enabled: false,
                run_threshold: 80,
                run_recheck: true,
                lock_enabled: false,
                note: String::new(),
            },
        ];

        let map = AppState::create_input_mappings(&config)
            .expect("坏触发键不应让映射构建失败");
        // 坏的那条被跳过, 好的那条照常加载
        assert!(map.contains_key(&AppState::input_name_to_device("B").unwrap()));
        assert_eq!(map.len(), 1);
    }

    #[test]
    fn test_create_input_mappings_invalid_target() {
        let mut config = AppConfig::default();
        config.mappings = vec![
            KeyMapping {
                sequence_text: String::new(),
                release_targets: Default::default(),
            trigger_key: "A".to_string(),
            target_keys: SmallVec::from_vec(vec!["INVALID_KEY".to_string()]),
            interval: None,
            event_duration: None,
            turbo_enabled: true,
                move_speed: 10,                double_tap_enabled: false,
                double_tap_gap_ms: default_double_tap_gap_ms(),
                run_enabled: false,
                run_threshold: 80,
                run_recheck: true,
                lock_enabled: false,

                note: String::new(),
        }];

        let result = AppState::create_input_mappings(&config);
        assert!(result.is_err());
    }

    /// ★v24.31 诊断: 键盘录制链路状态层验证 —— handle_key_event (钩子入口)
    /// → record_key_event → stop_key_record 必须拿到事件。
    /// (真实钩子线程在本测试外; 若此测试绿而实机录不到, 问题在钩子外层环境)
    #[test]
    fn test_key_record_captures_keyboard_events() {
        let config = AppConfig::default();
        let state = AppState::new(config).unwrap();
        const WM_KEYDOWN: u32 = 0x0100;
        const WM_KEYUP: u32 = 0x0101;

        state.start_key_record();
        assert!(state.key_record_active.load(std::sync::atomic::Ordering::Relaxed));

        // A 按下 30ms 后抬起, B 按下 … (时间由真实时钟给出, 只验证事件被捕获)
        // ★录制来源 = RawInput (v24.31 审计后不再走 LL 钩子)
        state.record_key_state(true, 0x41); // A down
        std::thread::sleep(std::time::Duration::from_millis(30));
        state.record_key_state(false, 0x41); // A up
        state.record_key_state(true, 0x42); // B down
        state.record_key_state(false, 0x42); // B up

        let events = state.stop_key_record().expect("停止应取走事件缓冲");
        assert!(!state.key_record_active.load(std::sync::atomic::Ordering::Relaxed));
        assert_eq!(events.len(), 2, "应录到 A、B 两个键");
        assert_eq!(events[0].vk, 0x41);
        assert!(events[0].up_ms.is_some(), "抬起时间应被回填");
        // 转序列文本应非空且含 A
        let text = crate::sequence::recorded_to_sequence(&events, &crate::sequence::vk_to_seq_name);
        assert!(text.contains('A'), "序列文本应含 A: {text}");
    }


    /// ★v24.31 审计回归: 纯序列映射 (目标键为空 + 序列文本) 必须能建档,
    /// 且 turbo/双击/锁定全部停用 (序列接管输出)。
    #[test]
    fn test_create_input_mappings_pure_sequence_mapping() {
        let mut config = AppConfig::default();
        config.mappings = vec![KeyMapping {
            trigger_key: "F8".to_string(),
            target_keys: SmallVec::new(), // 目标键留空
            interval: None,
            event_duration: None,
            turbo_enabled: true, // 会被序列压制
            move_speed: 5,
            double_tap_enabled: false,
            double_tap_gap_ms: 50,
            run_enabled: false,
            run_threshold: 80,
            run_recheck: true,
            lock_enabled: false,
            sequence_text: "A⏱50>>B⏱50".to_string(),
            release_targets: SmallVec::new(),
            note: String::new(),
        }];
        let map = AppState::create_input_mappings(&config).expect("纯序列映射应能建档");
        let device = AppState::input_name_to_device("F8").unwrap();
        let info = map.get(&device).expect("映射应存在");
        assert!(info.sequence.is_some(), "序列应已解析");
        assert_eq!(info.sequence.as_ref().unwrap().len(), 2);
        assert!(!info.turbo_enabled, "序列条目连发必须停用");
        assert!(!info.double_tap_enabled);
        assert!(!info.lock_enabled);
        assert!(info.sequence_ctl.is_none());
    }

    /// ★v24.35 冲突修复回归: 序列解析失败 → 整条映射**不生效** (不再静默回退
    /// 目标键模式 —— 回退会造成行为突变 + 占位 KeyboardKey(0) 被真注入)。
    #[test]
    fn test_create_input_mappings_sequence_parse_failure_skips_mapping() {
        let mut config = AppConfig::default();
        config.mappings = vec![KeyMapping {
            trigger_key: "F8".to_string(),
            target_keys: SmallVec::from_vec(vec!["A".to_string()]), // 有目标键也不回退
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
            sequence_text: "宏(不存在的宏)".to_string(), // 引用未定义宏 → 解析失败
            release_targets: SmallVec::new(),
            note: String::new(),
        }];
        let map = AppState::create_input_mappings(&config).expect("单条脏数据不得阻断加载");
        let device = AppState::input_name_to_device("F8").unwrap();
        assert!(
            map.get(&device).is_none(),
            "解析失败的序列映射必须整条跳过, 不回退目标键"
        );
    }

    /// ★v24.35 补漏回归: 重推奔跑与序列互斥 (数据层压制, 防 GUI 双真误显示)。
    #[test]
    fn test_create_input_mappings_sequence_suppresses_run() {
        let mut config = AppConfig::default();
        config.mappings = vec![KeyMapping {
            trigger_key: "F8".to_string(),
            target_keys: SmallVec::from_vec(vec!["A".to_string()]),
            interval: None,
            event_duration: None,
            turbo_enabled: false,
            move_speed: 5,
            double_tap_enabled: false,
            double_tap_gap_ms: 50,
            run_enabled: true, // 会被序列压制
            run_threshold: 80,
            run_recheck: true,
            lock_enabled: false,
            sequence_text: "A⏱50".to_string(),
            release_targets: SmallVec::new(),
            note: String::new(),
        }];
        let map = AppState::create_input_mappings(&config).unwrap();
        let device = AppState::input_name_to_device("F8").unwrap();
        let info = map.get(&device).expect("映射应存在");
        assert!(info.sequence.is_some());
        assert!(!info.run_enabled, "序列条目上重推奔跑必须停用");
    }

    /// ★v24.31 审计回归: 序列控制键建档 (不注入, worker 拦截切全局暂停状态)。
    #[test]
    fn test_create_input_mappings_sequence_control_key() {
        let mut config = AppConfig::default();
        config.mappings = vec![KeyMapping {
            trigger_key: "F9".to_string(),
            target_keys: SmallVec::from_vec(vec!["KeySequenceToggle".to_string()]),
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
            sequence_text: String::new(),
            release_targets: SmallVec::new(),
            note: String::new(),
        }];
        let map = AppState::create_input_mappings(&config).expect("控制键映射应能建档");
        let device = AppState::input_name_to_device("F9").unwrap();
        let info = map.get(&device).expect("映射应存在");
        assert!(matches!(
            info.sequence_ctl,
            Some(crate::state::SequenceCtl::Toggle)
        ));
        assert!(!info.turbo_enabled, "控制键不得连发 (占位 action 永不被模拟)");
    }

    /// ★v24.31 审计回归: 锁定与 奔跑/简易奔跑 互斥 (引擎侧压制)。
    #[test]
    fn test_create_input_mappings_lock_suppressed_by_run() {
        let mut config = AppConfig::default();
        config.mappings = vec![KeyMapping {
            trigger_key: "A".to_string(),
            target_keys: SmallVec::from_vec(vec!["LEFT".to_string()]),
            interval: None,
            event_duration: None,
            turbo_enabled: true,
            move_speed: 5,
            double_tap_enabled: true, // 简易奔跑
            double_tap_gap_ms: 50,
            run_enabled: false,
            run_threshold: 80,
            run_recheck: true,
            lock_enabled: true, // 与简易奔跑互斥 → 被压制
            sequence_text: String::new(),
            release_targets: SmallVec::new(),
            note: String::new(),
        }];
        let map = AppState::create_input_mappings(&config).unwrap();
        let device = AppState::input_name_to_device("A").unwrap();
        let info = map.get(&device).unwrap();
        assert!(!info.lock_enabled, "锁定与简易奔跑互斥, 应被压制");
        assert!(info.double_tap_enabled, "简易奔跑不受影响");
    }

    #[test]
    fn test_create_input_mappings_interval_validation() {
        let mut config = AppConfig::default();
        config.interval = 3; // Below minimum
        config.mappings = vec![
            KeyMapping {
                sequence_text: String::new(),
                release_targets: Default::default(),
            trigger_key: "A".to_string(),
            target_keys: SmallVec::from_vec(vec!["B".to_string()]),
            interval: Some(3), // Below minimum
            event_duration: None,
            turbo_enabled: true,
                move_speed: 10,                double_tap_enabled: false,
                double_tap_gap_ms: default_double_tap_gap_ms(),
                run_enabled: false,
                run_threshold: 80,
                run_recheck: true,
                lock_enabled: false,

                note: String::new(),
        }];

        let result = AppState::create_input_mappings(&config);
        assert!(result.is_ok());

        let input_mappings = result.unwrap();
        let device = InputDevice::Keyboard(0x41); // 'A' key
        let a_mapping = input_mappings.get(&device).unwrap();
        assert!(
            a_mapping.interval >= 5,
            "Interval should be clamped to minimum 5"
        );
    }

    #[test]
    fn test_create_input_mappings_duration_validation() {
        let mut config = AppConfig::default();
        config.event_duration = 2; // Below minimum
        config.mappings = vec![
            KeyMapping {
                sequence_text: String::new(),
                release_targets: Default::default(),
            trigger_key: "A".to_string(),
            target_keys: SmallVec::from_vec(vec!["B".to_string()]),
            interval: None,
            event_duration: Some(3), // Below minimum
            turbo_enabled: true,
                move_speed: 10,                double_tap_enabled: false,
                double_tap_gap_ms: default_double_tap_gap_ms(),
                run_enabled: false,
                run_threshold: 80,
                run_recheck: true,
                lock_enabled: false,

                note: String::new(),
        }];

        let result = AppState::create_input_mappings(&config);
        assert!(result.is_ok());

        let input_mappings = result.unwrap();
        let device = InputDevice::Keyboard(0x41); // 'A' key
        let a_mapping = input_mappings.get(&device).unwrap();
        assert!(
            a_mapping.event_duration >= 2,
            "Duration should be clamped to minimum 2"
        );
    }

    #[test]
    fn test_case_insensitive_key_names() {
        assert_eq!(
            AppState::key_name_to_vk("space"),
            AppState::key_name_to_vk("SPACE")
        );
        assert_eq!(
            AppState::key_name_to_vk("enter"),
            AppState::key_name_to_vk("ENTER")
        );
        assert_eq!(
            AppState::key_name_to_vk("esc"),
            AppState::key_name_to_vk("ESC")
        );
        assert_eq!(
            AppState::key_name_to_vk("delete"),
            AppState::key_name_to_vk("DELETE")
        );
    }

    #[test]
    fn test_multiple_input_mappings() {
        let mut config = AppConfig::default();
        config.mappings = vec![
            
                KeyMapping {
                    sequence_text: String::new(),
                trigger_key: "A".to_string(),
                target_keys: SmallVec::from_vec(vec!["1".to_string()]),
                interval: Some(10),
                event_duration: Some(5),
                turbo_enabled: true,
                move_speed: 10,                double_tap_enabled: false,
                double_tap_gap_ms: default_double_tap_gap_ms(),
                run_enabled: false,
                release_targets: Default::default(),
                run_threshold: 80,
                run_recheck: true,
                lock_enabled: false,

                note: String::new(),
            },
            
                KeyMapping {
                    sequence_text: String::new(),
                trigger_key: "B".to_string(),
                target_keys: SmallVec::from_vec(vec!["2".to_string()]),
                interval: Some(15),
                event_duration: Some(8),
                turbo_enabled: true,
                move_speed: 10,                double_tap_enabled: false,
                double_tap_gap_ms: default_double_tap_gap_ms(),
                release_targets: Default::default(),
                run_enabled: false,
                run_threshold: 80,
                run_recheck: true,
                lock_enabled: false,

                note: String::new(),
            },
            
                KeyMapping {
                    sequence_text: String::new(),
                    release_targets: Default::default(),
                trigger_key: "C".to_string(),
                target_keys: SmallVec::from_vec(vec!["3".to_string()]),
                interval: Some(20),
                event_duration: Some(10),
                turbo_enabled: true,
                move_speed: 10,                double_tap_enabled: false,
                double_tap_gap_ms: default_double_tap_gap_ms(),
                run_enabled: false,
                run_threshold: 80,
                run_recheck: true,
                lock_enabled: false,

                note: String::new(),
            },
        ];

        let input_mappings = AppState::create_input_mappings(&config).unwrap();
        assert_eq!(input_mappings.len(), 3);

        let device_a = InputDevice::Keyboard(0x41);
        let device_b = InputDevice::Keyboard(0x42);
        let device_c = InputDevice::Keyboard(0x43);

        assert_eq!(input_mappings.get(&device_a).unwrap().interval, 10);
        assert_eq!(input_mappings.get(&device_b).unwrap().interval, 15);
        assert_eq!(input_mappings.get(&device_c).unwrap().interval, 20);
    }

    #[test]
    fn test_app_state_reload_config() {
        let config = AppConfig::default();
        let state = AppState::new(config).unwrap();

        // Initial state
        assert!(!state.is_paused());
        assert_eq!(
            state.switch_key_cache.keyboard_vk.load(Ordering::Relaxed),
            0x2E
        ); // DELETE

        // Create new config
        let mut new_config = AppConfig::default();
        new_config.switch_key = "F11".to_string();
        new_config.show_tray_icon = false;
        new_config.input_timeout = 50;

        // Reload config
        state.reload_config(new_config).unwrap();

        // Verify changes
        assert_eq!(
            state.switch_key_cache.keyboard_vk.load(Ordering::Relaxed),
            0x7A
        ); // F11
        assert!(!state.show_tray_icon());
        assert_eq!(state.input_timeout(), 50);
    }

    #[test]
    fn test_key_mapping_with_boundary_values() {
        let mut config = AppConfig::default();

        // Test with minimum interval
        config.mappings = vec![
            KeyMapping {
                sequence_text: String::new(),
                release_targets: Default::default(),
            trigger_key: "A".to_string(),
            target_keys: SmallVec::from_vec(vec!["B".to_string()]),
            interval: Some(5), // Minimum valid value
            event_duration: Some(2),
            turbo_enabled: true,
                move_speed: 10,                double_tap_enabled: false,
                double_tap_gap_ms: default_double_tap_gap_ms(),
                run_enabled: false,
                run_threshold: 80,
                run_recheck: true,
                lock_enabled: false,

                note: String::new(),
        }];

        let state = AppState::new(config);
        assert!(state.is_ok());
    }

    #[test]
    fn test_key_mapping_with_zero_interval() {
        let mut config = AppConfig::default();

        // Test with zero interval (should be auto-adjusted to minimum)
        config.mappings = vec![
            KeyMapping {
                sequence_text: String::new(),
                release_targets: Default::default(),
            trigger_key: "A".to_string(),
            target_keys: SmallVec::from_vec(vec!["B".to_string()]),
            interval: Some(0),
            event_duration: Some(0),
            turbo_enabled: true,
                move_speed: 10,                double_tap_enabled: false,
                double_tap_gap_ms: default_double_tap_gap_ms(),
                run_enabled: false,
                run_threshold: 80,
                run_recheck: true,
                lock_enabled: false,

                note: String::new(),
        }];

        let state = AppState::new(config).unwrap();

        // Values should be adjusted to minimum of 2
        // This test verifies auto-adjustment behavior
        assert!(state.input_mappings.len() > 0);
    }

    #[test]
    fn test_vk_to_scancode_common_keys() {
        // Test commonly mapped VK codes that exist in SCANCODE_MAP
        assert_eq!(AppState::vk_to_scancode(0x08), 0x0E); // Backspace
        assert_eq!(AppState::vk_to_scancode(0x09), 0x0F); // Tab
        assert_eq!(AppState::vk_to_scancode(0x0D), 0x1C); // Enter
        assert_eq!(AppState::vk_to_scancode(0x20), 0x39); // Space

        // Keys not in map return 0
        let unmapped = AppState::vk_to_scancode(0xFF);
        assert_eq!(unmapped, 0);
    }

    #[test]
    fn test_key_name_to_vk_extended_keys() {
        assert_eq!(AppState::key_name_to_vk("LWIN"), Some(0x5B));
        assert_eq!(AppState::key_name_to_vk("RWIN"), Some(0x5C));
        assert_eq!(AppState::key_name_to_vk("PAUSE"), Some(0x13));
        assert_eq!(AppState::key_name_to_vk("CAPSLOCK"), Some(0x14));
        assert_eq!(AppState::key_name_to_vk("CAPITAL"), Some(0x14));
        assert_eq!(AppState::key_name_to_vk("NUMLOCK"), Some(0x90));
        assert_eq!(AppState::key_name_to_vk("SCROLL"), Some(0x91));
        assert_eq!(AppState::key_name_to_vk("SNAPSHOT"), Some(0x2C));
    }

    #[test]
    fn test_key_name_to_vk_numpad_keys() {
        assert_eq!(AppState::key_name_to_vk("NUMPAD0"), Some(0x60));
        assert_eq!(AppState::key_name_to_vk("NUMPAD1"), Some(0x61));
        assert_eq!(AppState::key_name_to_vk("NUMPAD5"), Some(0x65));
        assert_eq!(AppState::key_name_to_vk("NUMPAD9"), Some(0x69));
        assert_eq!(AppState::key_name_to_vk("MULTIPLY"), Some(0x6A));
        assert_eq!(AppState::key_name_to_vk("ADD"), Some(0x6B));
        assert_eq!(AppState::key_name_to_vk("SUBTRACT"), Some(0x6D));
        assert_eq!(AppState::key_name_to_vk("DECIMAL"), Some(0x6E));
        assert_eq!(AppState::key_name_to_vk("DIVIDE"), Some(0x6F));
    }

    #[test]
    fn test_key_name_to_vk_oem_keys() {
        assert_eq!(AppState::key_name_to_vk("OEM_1"), Some(0xBA));
        assert_eq!(AppState::key_name_to_vk("OEM_2"), Some(0xBF));
        assert_eq!(AppState::key_name_to_vk("OEM_3"), Some(0xC0));
        assert_eq!(AppState::key_name_to_vk("OEM_4"), Some(0xDB));
        assert_eq!(AppState::key_name_to_vk("OEM_5"), Some(0xDC));
        assert_eq!(AppState::key_name_to_vk("OEM_6"), Some(0xDD));
        assert_eq!(AppState::key_name_to_vk("OEM_7"), Some(0xDE));
        assert_eq!(AppState::key_name_to_vk("OEM_PLUS"), Some(0xBB));
        assert_eq!(AppState::key_name_to_vk("OEM_COMMA"), Some(0xBC));
        assert_eq!(AppState::key_name_to_vk("OEM_MINUS"), Some(0xBD));
        assert_eq!(AppState::key_name_to_vk("OEM_PERIOD"), Some(0xBE));
    }

    #[test]
    fn test_key_name_to_vk_mouse_buttons() {
        assert_eq!(AppState::key_name_to_vk("LBUTTON"), Some(0x01));
        assert_eq!(AppState::key_name_to_vk("RBUTTON"), Some(0x02));
        assert_eq!(AppState::key_name_to_vk("MBUTTON"), Some(0x04));
        assert_eq!(AppState::key_name_to_vk("XBUTTON1"), Some(0x05));
        assert_eq!(AppState::key_name_to_vk("XBUTTON2"), Some(0x06));
    }

    #[test]
    fn test_key_name_aliases() {
        assert_eq!(AppState::key_name_to_vk("ESC"), Some(0x1B));
        assert_eq!(AppState::key_name_to_vk("ESCAPE"), Some(0x1B));
        assert_eq!(AppState::key_name_to_vk("ENTER"), Some(0x0D));
        assert_eq!(AppState::key_name_to_vk("RETURN"), Some(0x0D));
        assert_eq!(AppState::key_name_to_vk("BACKSPACE"), Some(0x08));
        assert_eq!(AppState::key_name_to_vk("BACK"), Some(0x08));
    }

    #[test]
    fn test_input_name_to_device_backward_compatibility() {
        // Ensure keyboard input still works
        assert!(matches!(
            AppState::input_name_to_device("A"),
            Some(InputDevice::Keyboard(0x41))
        ));

        // Ensure mouse input still works
        assert!(matches!(
            AppState::input_name_to_device("LBUTTON"),
            Some(InputDevice::Mouse(MouseButton::Left))
        ));

        // Ensure key combos still work
        let combo = AppState::input_name_to_device("LALT+A");
        assert!(matches!(combo, Some(InputDevice::KeyCombo(_))));
    }

    #[test]
    fn test_device_type_equality() {
        let gamepad1 = DeviceType::Gamepad(0x045e);
        let gamepad2 = DeviceType::Gamepad(0x045e);
        let gamepad3 = DeviceType::Gamepad(0x046d);

        assert_eq!(gamepad1, gamepad2);
        assert_ne!(gamepad1, gamepad3);

        let hid1 = DeviceType::HidDevice {
            usage_page: 0x01,
            usage: 0x05,
        };
        let hid2 = DeviceType::HidDevice {
            usage_page: 0x01,
            usage: 0x05,
        };
        assert_eq!(hid1, hid2);
    }

    #[test]
    fn test_vk_to_scancode_numpad_keys() {
        assert_eq!(AppState::vk_to_scancode(0x60), 0x52); // NUMPAD0
        assert_eq!(AppState::vk_to_scancode(0x61), 0x4F); // NUMPAD1
        assert_eq!(AppState::vk_to_scancode(0x65), 0x4C); // NUMPAD5
        assert_eq!(AppState::vk_to_scancode(0x69), 0x49); // NUMPAD9
        assert_eq!(AppState::vk_to_scancode(0x6A), 0x37); // MULTIPLY
        assert_eq!(AppState::vk_to_scancode(0x6B), 0x4E); // ADD
        assert_eq!(AppState::vk_to_scancode(0x6D), 0x4A); // SUBTRACT
        assert_eq!(AppState::vk_to_scancode(0x6F), 0x35); // DIVIDE
    }

    #[test]
    fn test_vk_to_scancode_lock_keys() {
        assert_eq!(AppState::vk_to_scancode(0x14), 0x3A); // CAPSLOCK
        assert_eq!(AppState::vk_to_scancode(0x90), 0x45); // NUMLOCK
        assert_eq!(AppState::vk_to_scancode(0x91), 0x46); // SCROLL LOCK
    }

    #[test]
    fn test_vk_to_scancode_oem_keys() {
        assert_eq!(AppState::vk_to_scancode(0xBA), 0x27); // OEM_1 (;:)
        assert_eq!(AppState::vk_to_scancode(0xBB), 0x0D); // OEM_PLUS (=+)
        assert_eq!(AppState::vk_to_scancode(0xBC), 0x33); // OEM_COMMA (,<)
        assert_eq!(AppState::vk_to_scancode(0xBD), 0x0C); // OEM_MINUS (-_)
        assert_eq!(AppState::vk_to_scancode(0xBE), 0x34); // OEM_PERIOD (.>)
        assert_eq!(AppState::vk_to_scancode(0xBF), 0x35); // OEM_2 (/?)
        assert_eq!(AppState::vk_to_scancode(0xC0), 0x29); // OEM_3 (`~)
    }

    #[test]
    fn test_combo_key_with_numpad() {
        let device = AppState::input_name_to_device("LCTRL+NUMPAD0");
        assert!(device.is_some());

        if let Some(InputDevice::KeyCombo(keys)) = device {
            assert_eq!(keys.len(), 2);
            assert_eq!(keys[0], 0xA2); // LCTRL
            assert_eq!(keys[1], 0x60); // NUMPAD0
        } else {
            panic!("Expected KeyCombo device");
        }
    }

    #[test]
    fn test_combo_key_with_oem() {
        let device = AppState::input_name_to_device("LALT+OEM_3");
        assert!(device.is_some());

        if let Some(InputDevice::KeyCombo(keys)) = device {
            assert_eq!(keys.len(), 2);
            assert_eq!(keys[0], 0xA4); // LALT
            assert_eq!(keys[1], 0xC0); // OEM_3 (`~)
        } else {
            panic!("Expected KeyCombo device");
        }
    }

    #[test]
    fn test_output_action_with_numpad() {
        let action = AppState::input_name_to_output("NUMPAD5");
        assert!(action.is_some());

        if let Some(OutputAction::KeyboardKey(scancode)) = action {
            assert_eq!(scancode, 0x4C); // NUMPAD5 scancode
        } else {
            panic!("Expected KeyboardKey action");
        }
    }

    #[test]
    fn test_parse_key_combo_trigger() {
        // Test parsing key combinations
        let device = AppState::input_name_to_device("ALT+A");
        assert!(device.is_some());

        if let Some(InputDevice::KeyCombo(keys)) = device {
            assert_eq!(keys.len(), 2);
            assert_eq!(keys[0], 0x12); // ALT
            assert_eq!(keys[1], 0x41); // A
        } else {
            panic!("Expected KeyCombo device");
        }
    }

    #[test]
    fn test_parse_complex_key_combo() {
        // Test parsing complex key combinations
        let device = AppState::input_name_to_device("CTRL+SHIFT+S");
        assert!(device.is_some());

        if let Some(InputDevice::KeyCombo(keys)) = device {
            assert_eq!(keys.len(), 3);
            assert_eq!(keys[0], 0x11); // CTRL
            assert_eq!(keys[1], 0x10); // SHIFT
            assert_eq!(keys[2], 0x53); // S
        } else {
            panic!("Expected KeyCombo device");
        }
    }

    #[test]
    fn test_parse_key_combo_output() {
        // Test parsing key combination output
        let action = AppState::input_name_to_output("ALT+F4");
        assert!(action.is_some());

        if let Some(OutputAction::KeyCombo(scancodes)) = action {
            assert_eq!(scancodes.len(), 2);
            assert_eq!(scancodes[0], 0x38); // ALT scancode
            assert_eq!(scancodes[1], 0x3E); // F4 scancode
            // Verify Arc reference counting works
            let clone = scancodes.clone();
            assert_eq!(Arc::strong_count(&scancodes), Arc::strong_count(&clone));
        } else {
            panic!("Expected KeyCombo output");
        }
    }

    #[test]
    fn test_parse_invalid_key_combo() {
        // Test parsing invalid key combinations
        let device = AppState::input_name_to_device("INVALID+KEY");
        assert!(device.is_none());

        let device = AppState::input_name_to_device("A+");
        assert!(device.is_none());

        let device = AppState::input_name_to_device("+B");
        assert!(device.is_none());
    }

    #[test]
    fn test_parse_device_with_vid_pid_serial() {
        // Test parsing new format with VID/PID/Serial
        let device = AppState::input_name_to_device("GAMEPAD_045E_0B05_ABC123_B2.0");
        assert!(device.is_some());
        match device.unwrap() {
            InputDevice::GenericDevice {
                device_type: DeviceType::Gamepad(_),
                button_id,
            } => {
                let stable_id = (button_id >> 32) as u32;
                let position = (button_id & 0xFFFFFFFF) as u32;
                let byte_idx = (position >> 16) as u16;
                let bit_idx = (position & 0xFFFF) as u16;

                // Stable ID should be a hash (non-zero)
                assert_ne!(stable_id, 0);
                assert_eq!(byte_idx, 2);
                assert_eq!(bit_idx, 0);
            }
            _ => panic!("Expected GenericDevice"),
        }
    }

    #[test]
    fn test_parse_device_with_vid_pid_no_serial() {
        // Test parsing new format with VID/PID but no serial (DEV fallback)
        let device = AppState::input_name_to_device("GAMEPAD_045E_0B05_DEV12345678_B2.0");
        assert!(device.is_some());
        match device.unwrap() {
            InputDevice::GenericDevice {
                device_type: DeviceType::Gamepad(_),
                button_id,
            } => {
                let stable_id = (button_id >> 32) as u32;
                let position = (button_id & 0xFFFFFFFF) as u32;
                let byte_idx = (position >> 16) as u16;
                let bit_idx = (position & 0xFFFF) as u16;

                assert_eq!(stable_id, 0x12345678); // Should match DEV value
                assert_eq!(byte_idx, 2);
                assert_eq!(bit_idx, 0);
            }
            _ => panic!("Expected GenericDevice"),
        }
    }

    #[test]
    fn test_multiple_device_handles() {
        // Test that different handles produce different button IDs
        let device1 = InputDevice::GenericDevice {
            device_type: DeviceType::Gamepad(0x045e),
            button_id: (0x11111111u64 << 32) | (2u64 << 16) | 0u64,
        };

        let device2 = InputDevice::GenericDevice {
            device_type: DeviceType::Gamepad(0x045e),
            button_id: (0x22222222u64 << 32) | (2u64 << 16) | 0u64,
        };

        assert_ne!(device1, device2);
    }

    #[test]
    fn test_modifier_key_scancodes() {
        // Test that modifier keys have proper scancodes
        assert_eq!(AppState::vk_to_scancode(0xA0), 0x2A); // LSHIFT
        assert_eq!(AppState::vk_to_scancode(0xA1), 0x36); // RSHIFT
        assert_eq!(AppState::vk_to_scancode(0xA2), 0x1D); // LCTRL
        assert_eq!(AppState::vk_to_scancode(0xA4), 0x38); // LALT
        assert_eq!(AppState::vk_to_scancode(0x10), 0x2A); // SHIFT (generic)
        assert_eq!(AppState::vk_to_scancode(0x11), 0x1D); // CTRL (generic)
        assert_eq!(AppState::vk_to_scancode(0x12), 0x38); // ALT (generic)
    }

    #[test]
    fn test_key_combo_mapping_creation() {
        let mut config = AppConfig::default();
        config.mappings = vec![
            
                KeyMapping {
                    sequence_text: String::new(),
                trigger_key: "ALT+A".to_string(),
                target_keys: SmallVec::from_vec(vec!["B".to_string()]),
                interval: Some(10),
                event_duration: Some(5),
                release_targets: Default::default(),
                turbo_enabled: true,
            move_speed: 5,
                double_tap_enabled: false,
                double_tap_gap_ms: default_double_tap_gap_ms(),
                run_enabled: false,
                run_threshold: 80,
                run_recheck: true,
                lock_enabled: false,

                note: String::new(),
            },
            
                KeyMapping {
                    sequence_text: String::new(),
                    release_targets: Default::default(),
                trigger_key: "CTRL+SHIFT+F".to_string(),
                target_keys: SmallVec::from_vec(vec!["ALT+F4".to_string()]),
                interval: None,
                event_duration: None,
                turbo_enabled: true,
                move_speed: 10,                double_tap_enabled: false,
                double_tap_gap_ms: default_double_tap_gap_ms(),
                run_enabled: false,
                run_threshold: 80,
                run_recheck: true,
                lock_enabled: false,

                note: String::new(),
            },
        ];

        let input_mappings = AppState::create_input_mappings(&config).unwrap();
        assert_eq!(input_mappings.len(), 2);

        // Check first mapping
        let alt_a = InputDevice::KeyCombo(vec![0x12, 0x41]); // ALT+A
        let mapping1 = input_mappings.get(&alt_a);
        assert!(mapping1.is_some());

        if let Some(m) = mapping1 {
            assert_eq!(m.interval, 10);
            assert_eq!(m.event_duration, 5);
            if let OutputAction::KeyboardKey(scancode) = m.target_action {
                assert_eq!(scancode, 0x30); // B scancode
            } else {
                panic!("Expected single key output");
            }
        }

        // Check second mapping
        let ctrl_shift_f = InputDevice::KeyCombo(vec![0x11, 0x10, 0x46]); // CTRL+SHIFT+F
        let mapping2 = input_mappings.get(&ctrl_shift_f);
        assert!(mapping2.is_some());

        if let Some(m) = mapping2 {
            if let OutputAction::KeyCombo(scancodes) = &m.target_action {
                assert_eq!(scancodes.len(), 2); // ALT+F4
            } else {
                panic!("Expected combo key output");
            }
        }
    }

    #[test]
    fn test_pressed_keys_tracking() {
        let config = AppConfig::default();
        let state = AppState::new(config).unwrap();

        // Initially, no keys pressed
        assert_eq!(state.pressed_keys.len(), 0);

        // Simulate key press tracking (would be done by handle_key_event)
        let _ = state.pressed_keys.insert_sync(0x11); // CTRL
        let _ = state.pressed_keys.insert_sync(0x41); // A

        assert_eq!(state.pressed_keys.len(), 2);
        assert!(state.pressed_keys.contains_sync(&0x11));
        assert!(state.pressed_keys.contains_sync(&0x41));

        // Release keys
        let _ = state.pressed_keys.remove_sync(&0x41);

        assert_eq!(state.pressed_keys.len(), 1);
        assert!(state.pressed_keys.contains_sync(&0x11));
    }

    #[test]
    fn test_empty_process_whitelist() {
        let mut config = AppConfig::default();
        config.process_whitelist = vec![];

        let state = AppState::new(config).unwrap();

        // With empty whitelist, all processes should be whitelisted
        assert!(state.is_process_whitelisted());
    }

    /// ★白名单结构优化回归: 同名不同版本的两个 exe (A 版/B 版 DFO.exe) 必须能分别命中。
    /// 旧结构只有进程名条目 → 无法区分; 路径条目只匹配登记过的那个版本。
    #[test]
    fn test_whitelist_entry_matches_two_versions_same_name() {
        let path_a = r"E:\games\A\DFO.exe";
        let path_b = r"E:\games\B\DFO.exe";

        // 1) 路径条目精确匹配: A 版登记 → 只放行 A 版, B 版不命中
        assert!(AppState::whitelist_entry_matches(path_a, Some(path_a)));
        assert!(!AppState::whitelist_entry_matches(path_a, Some(path_b)));
        // B 版登记为另一条 → 两个版本各有一条, 互不冲突 (旧结构下第二条加不进去)
        assert!(AppState::whitelist_entry_matches(path_b, Some(path_b)));

        // 2) 大小写不敏感 (手动输入/不同盘符风格)
        assert!(AppState::whitelist_entry_matches(
            r"e:\GAMES\a\dfo.exe",
            Some(path_a)
        ));

        // 3) 纯进程名条目 (老语义): 任意路径的同名进程都放行
        assert!(AppState::whitelist_entry_matches("dfo.exe", Some(path_a)));
        assert!(AppState::whitelist_entry_matches("DFO.EXE", Some(path_b)));

        // 4) 不同名的进程不被纯名条目误放行
        assert!(!AppState::whitelist_entry_matches("dnf.exe", Some(path_a)));
        // 5) 无前台路径 / 空条目 → 不命中 (放行决策由 is_process_whitelisted 兜底)
        assert!(!AppState::whitelist_entry_matches("dfo.exe", None));
        assert!(!AppState::whitelist_entry_matches("", Some(path_a)));
        assert!(!AppState::whitelist_entry_matches("  ", Some(path_a)));
    }

    #[test]
    fn test_process_whitelist_cache() {
        use std::thread;
        use std::time::Duration;

        let mut config = AppConfig::default();
        config.process_whitelist = vec!["explorer.exe".to_string()];

        let state = AppState::new(config).unwrap();

        // First call - cache miss (will query Windows API)
        let _ = state.is_process_whitelisted();

        // Verify cache was populated
        let cache = state.cached_process_info.read().unwrap();
        let (cached_name, _) = &*cache;
        let initial_name = cached_name.clone();
        drop(cache);

        // Second call immediately - cache hit (should use cached value)
        let _ = state.is_process_whitelisted();

        // Verify cache still has same value
        let cache = state.cached_process_info.read().unwrap();
        let (cached_name, _) = &*cache;
        assert_eq!(*cached_name, initial_name);
        drop(cache);

        // Wait for cache to expire (>50ms)
        thread::sleep(Duration::from_millis(60));

        // Third call after expiration - cache miss (will refresh)
        let _ = state.is_process_whitelisted();

        // Cache should be refreshed with new timestamp
        let cache = state.cached_process_info.read().unwrap();
        let (_, timestamp) = &*cache;
        assert!(timestamp.elapsed() < Duration::from_millis(10));
    }

    #[test]
    fn test_x_button_parsing() {
        use windows::Win32::UI::WindowsAndMessaging::*;

        let config = AppConfig::default();
        let state = AppState::new(config).unwrap();

        // Simulate XBUTTON1 down (mouse_data high word = 1)
        let mouse_data_x1: u32 = 1 << 16; // XBUTTON1
        let _result = state.handle_mouse_event(WM_XBUTTONDOWN, mouse_data_x1);
        // Should parse as X1 button

        // Simulate XBUTTON2 up (mouse_data high word = 2)
        let mouse_data_x2: u32 = 2 << 16; // XBUTTON2
        let _result = state.handle_mouse_event(WM_XBUTTONUP, mouse_data_x2);
        // Should parse as X2 button
    }

    #[test]
    fn test_mouse_button_name_parsing() {
        // Test X button name parsing
        assert_eq!(
            AppState::mouse_button_name_to_type("XBUTTON1"),
            Some(MouseButton::X1)
        );
        assert_eq!(
            AppState::mouse_button_name_to_type("XBUTTON2"),
            Some(MouseButton::X2)
        );
        assert_eq!(
            AppState::mouse_button_name_to_type("X1"),
            Some(MouseButton::X1)
        );
        assert_eq!(
            AppState::mouse_button_name_to_type("MB4"),
            Some(MouseButton::X1)
        );
        assert_eq!(
            AppState::mouse_button_name_to_type("MB5"),
            Some(MouseButton::X2)
        );
    }

    #[test]
    fn test_concurrent_window_requests() {
        use std::thread;

        let config = AppConfig::default();
        let state = Arc::new(AppState::new(config).unwrap());

        let handles: Vec<_> = (0..5)
            .map(|_| {
                let state_clone = state.clone();
                thread::spawn(move || {
                    for _ in 0..20 {
                        state_clone.request_show_window();
                        state_clone.check_and_clear_show_window_request();
                        state_clone.request_show_about();
                        state_clone.check_and_clear_show_about_request();
                    }
                })
            })
            .collect();

        for handle in handles {
            handle.join().unwrap();
        }

        // Final state should be consistent
        assert!(!state.check_and_clear_show_window_request());
        assert!(!state.check_and_clear_show_about_request());
    }

    #[test]
    fn test_create_multiple_target_keys_mapping() {
        let mut config = AppConfig::default();
        config.mappings = vec![
            KeyMapping {
                sequence_text: String::new(),
                release_targets: Default::default(),
            trigger_key: "Q".to_string(),
            target_keys: SmallVec::from_vec(vec!["MOUSE_UP".to_string(), "MOUSE_LEFT".to_string()]),
            interval: Some(5),
            event_duration: Some(5),
            turbo_enabled: true,
                move_speed: 10,                double_tap_enabled: false,
                double_tap_gap_ms: default_double_tap_gap_ms(),
                run_enabled: false,
                run_threshold: 80,
                run_recheck: true,
                lock_enabled: false,

                note: String::new(),
        }];

        let result = AppState::create_input_mappings(&config);
        assert!(result.is_ok());

        let input_mappings = result.unwrap();
        assert_eq!(input_mappings.len(), 1);

        let device_q = InputDevice::Keyboard(0x51); // 'Q' key
        let q_mapping = input_mappings.get(&device_q).unwrap();

        // Should create MultipleActions
        assert!(matches!(
            &q_mapping.target_action,
            OutputAction::MultipleActions(_)
        ));
    }

    #[test]
    fn test_multiple_target_keys_creates_multiple_actions() {
        let mut config = AppConfig::default();
        config.mappings = vec![
            KeyMapping {
                sequence_text: String::new(),
                release_targets: Default::default(),
            trigger_key: "A".to_string(),
            target_keys: SmallVec::from_vec(vec![
                "1".to_string(),
                "2".to_string(),
                "3".to_string(),
            ]),
            interval: Some(10),
            event_duration: Some(5),
            turbo_enabled: true,
                move_speed: 10,                double_tap_enabled: false,
                double_tap_gap_ms: default_double_tap_gap_ms(),
                run_enabled: false,
                run_threshold: 80,
                run_recheck: true,
                lock_enabled: false,

                note: String::new(),
        }];

        let result = AppState::create_input_mappings(&config);
        assert!(result.is_ok());

        let input_mappings = result.unwrap();
        let device_a = InputDevice::Keyboard(0x41);
        let a_mapping = input_mappings.get(&device_a).unwrap();

        if let OutputAction::MultipleActions(actions) = &a_mapping.target_action {
            assert_eq!(actions.len(), 3);
        } else {
            panic!("Expected MultipleActions variant");
        }
    }

    #[test]
    fn test_single_target_key_not_wrapped_in_multiple_actions() {
        let mut config = AppConfig::default();
        config.mappings = vec![
            KeyMapping {
                sequence_text: String::new(),
                release_targets: Default::default(),
            trigger_key: "A".to_string(),
            target_keys: SmallVec::from_vec(vec!["B".to_string()]),
            interval: Some(10),
            event_duration: Some(5),
            turbo_enabled: true,
                move_speed: 10,                double_tap_enabled: false,
                double_tap_gap_ms: default_double_tap_gap_ms(),
                run_enabled: false,
                run_threshold: 80,
                run_recheck: true,
                lock_enabled: false,

                note: String::new(),
        }];

        let result = AppState::create_input_mappings(&config);
        assert!(result.is_ok());

        let input_mappings = result.unwrap();
        let device_a = InputDevice::Keyboard(0x41);
        let a_mapping = input_mappings.get(&device_a).unwrap();

        // Single target should NOT be wrapped in MultipleActions
        assert!(matches!(
            &a_mapping.target_action,
            OutputAction::KeyboardKey(_)
        ));
    }

    #[test]
    fn test_empty_target_keys_skipped() {
        let mut config = AppConfig::default();
        config.mappings = vec![
            KeyMapping {
                sequence_text: String::new(),
            trigger_key: "A".to_string(),
            target_keys: SmallVec::new(),
            interval: Some(10),
            event_duration: Some(5),
            turbo_enabled: true,
            move_speed: 5,
                double_tap_enabled: false,
                double_tap_gap_ms: default_double_tap_gap_ms(),
                run_enabled: false,
                run_threshold: 80,
                run_recheck: true,
                release_targets: Default::default(),
                lock_enabled: false,

                note: String::new(),
        }];

        let result = AppState::create_input_mappings(&config);
        assert!(result.is_ok());

        let input_mappings = result.unwrap();
        // Empty target keys should be skipped
        assert_eq!(input_mappings.len(), 0);
    }

    #[test]
    fn test_multiple_target_keys_with_mixed_types() {
        let mut config = AppConfig::default();
        config.mappings = vec![
            KeyMapping {
                sequence_text: String::new(),
                release_targets: Default::default(),
            trigger_key: "Q".to_string(),
            target_keys: SmallVec::from_vec(vec![
                "A".to_string(),
                "B".to_string(),
                "C".to_string(),
            ]),
            interval: Some(10),
            event_duration: Some(5),
            turbo_enabled: true,
                move_speed: 10,                double_tap_enabled: false,
                double_tap_gap_ms: default_double_tap_gap_ms(),
                run_enabled: false,
                run_threshold: 80,
                run_recheck: true,
                lock_enabled: false,

                note: String::new(),
        }];

        let result = AppState::create_input_mappings(&config);
        assert!(result.is_ok());

        let input_mappings = result.unwrap();
        let device_q = InputDevice::Keyboard(0x51);
        let q_mapping = input_mappings.get(&device_q).unwrap();

        if let OutputAction::MultipleActions(actions) = &q_mapping.target_action {
            assert_eq!(actions.len(), 3);
            // All should be KeyboardKey actions
            for action in actions.iter() {
                assert!(matches!(action, OutputAction::KeyboardKey(_)));
            }
        } else {
            panic!("Expected MultipleActions variant");
        }
    }

    #[test]
    fn test_multiple_target_keys_validation() {
        let mut config = AppConfig::default();
        config.mappings = vec![
            KeyMapping {
                sequence_text: String::new(),
            trigger_key: "A".to_string(),
            target_keys: SmallVec::from_vec(vec!["B".to_string(), "INVALID_KEY".to_string()]),
            interval: None,
            event_duration: None,
            turbo_enabled: true,
                release_targets: Default::default(),
                move_speed: 10,                double_tap_enabled: false,
                double_tap_gap_ms: default_double_tap_gap_ms(),
                run_enabled: false,
                run_threshold: 80,
                run_recheck: true,
                lock_enabled: false,

                note: String::new(),
        }];

        let result = AppState::create_input_mappings(&config);
        // Should fail due to invalid target key
        assert!(result.is_err());
    }

    #[test]
    fn test_smallvec_optimization_in_multiple_actions() {
        let mut config = AppConfig::default();
        config.mappings = vec![
            KeyMapping {
                sequence_text: String::new(),
                release_targets: Default::default(),
            trigger_key: "A".to_string(),
            target_keys: SmallVec::from_vec(vec!["1".to_string(), "2".to_string()]),
            interval: Some(10),
            event_duration: Some(5),
            turbo_enabled: true,
                move_speed: 10,                double_tap_enabled: false,
                double_tap_gap_ms: default_double_tap_gap_ms(),
                run_enabled: false,
                run_threshold: 80,
                run_recheck: true,
                lock_enabled: false,

                note: String::new(),
        }];

        let result = AppState::create_input_mappings(&config);
        assert!(result.is_ok());

        let input_mappings = result.unwrap();
        let device_a = InputDevice::Keyboard(0x41);
        let a_mapping = input_mappings.get(&device_a).unwrap();

        // Verify SmallVec is used (inline storage for small collections)
        if let OutputAction::MultipleActions(actions) = &a_mapping.target_action {
            assert_eq!(actions.len(), 2);
            assert!(!actions.spilled()); // Should use inline storage
        } else {
            panic!("Expected MultipleActions variant");
        }
    }

    /// ★v23.1 回归: 历史遗留的**坏触发键**不得阻断启动。
    /// 用户实测: `GAMEPAD_045E_LS_Left+GAMEPAD_045E_LS_LS_Up` (双前缀/无法解析) 曾导致
    /// "Failed to initialize application state" → 程序完全起不来。
    #[test]
    fn invalid_trigger_mapping_is_skipped_not_fatal() {
        let mk = |trigger: &str, targets: &[&str], note: &str| KeyMapping {
            sequence_text: String::new(),
            release_targets: Default::default(),
            trigger_key: trigger.to_string(),
            target_keys: targets.iter().map(|s| s.to_string()).collect(),
            interval: None,
            event_duration: None,
            turbo_enabled: true,
            move_speed: 5,
            double_tap_enabled: false,
            double_tap_gap_ms: default_double_tap_gap_ms(),
            run_enabled: false,
            run_threshold: 80,
            run_recheck: true,
            lock_enabled: false,
            note: note.to_string(),
        };
        let config = AppConfig {
            mappings: vec![
                // 用户实测的坏条目: 空目标键 + 双前缀坏触发键
                mk(
                    "GAMEPAD_045E_LS_Left+GAMEPAD_045E_LS_LS_Up",
                    &[],
                    "手柄·十字键·右",
                ),
                // 坏触发键但**有**目标键 → 也应跳过而非报错
                mk("GAMEPAD_045E_LS_LS_Up", &["Q"], "坏键"),
                // 正常映射 → 必须照常加载
                mk("A", &["B"], "有效"),
            ],
            ..Default::default()
        };

        let map = AppState::create_input_mappings(&config)
            .expect("坏触发键不得让映射构建失败");
        let dev_a = AppState::input_name_to_device("A").expect("A 可解析");
        assert!(map.contains_key(&dev_a), "有效映射应被加载");
    }
