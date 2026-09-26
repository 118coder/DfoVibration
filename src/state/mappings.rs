//! 连发映射构建 (create_input_mappings) —— 原 state.rs 3232-3459, 2026-09-27 架构重构 B3 归位。

use std::collections::HashMap;

use super::*;

impl AppState {
    pub(super) fn create_input_mappings(
        config: &AppConfig,
    ) -> anyhow::Result<HashMap<InputDevice, InputMappingInfo>> {
        let mut input_mappings = HashMap::new();

        for mapping in &config.mappings {
            /* ★v23.1: 无目标键的映射本就跳过 —— 必须在解析触发键**之前**判断。
             * (此前先解析, 遇到历史遗留的坏触发键会直接报错导致**程序起不来**。) */
            let target_keys = mapping.get_target_keys();
            /* ★v24.31 审计: 纯序列映射允许目标键为空 (序列即输出); 两者都空才跳过 */
            let has_sequence = !mapping.sequence_text.trim().is_empty();
            if target_keys.is_empty() && !has_sequence {
                continue; // Skip mappings without target keys
            }
            /* ★v23.1: 坏触发键降级为"跳过 + 日志", 绝不让单条脏数据阻断启动。 */
            let Some(trigger_device) = Self::input_name_to_device(&mapping.trigger_key) else {
                eprintln!(
                    "[config] 跳过触发键无效的映射: trigger={:?} note={:?} (可在连发页删掉该条)",
                    mapping.trigger_key, mapping.note
                );
                continue;
            };

            let interval = mapping.interval.unwrap_or(config.interval).max(5);
            let event_duration = mapping
                .event_duration
                .unwrap_or(config.event_duration)
                .max(2);
            let move_speed = mapping.move_speed.max(1);

            // Parse target keys into output actions
            let mut actions: SmallVec<[OutputAction; 4]> = SmallVec::new();
            for target_key in target_keys {
                if let Some(action) = Self::input_name_to_output(target_key) {
                    // Update MouseMove and MouseScroll actions with configured speed
                    let action = match action {
                        OutputAction::MouseMove(direction, _) => {
                            OutputAction::MouseMove(direction, move_speed)
                        }
                        OutputAction::MouseScroll(direction, _) => {
                            OutputAction::MouseScroll(direction, move_speed)
                        }
                        other => other,
                    };
                    actions.push(action);
                } else {
                    return Err(anyhow::anyhow!("Invalid target input: {}", target_key));
                }
            }

            if actions.is_empty() && !has_sequence {
                continue; // Skip if no valid actions
            }

            // Create the final target action
            let target_action = if actions.is_empty() {
                /* ★v24.31: 纯序列映射的占位 —— worker 在序列分支拦截, 永不被模拟 */
                OutputAction::KeyboardKey(0)
            } else if actions.len() == 1 {
                actions.into_iter().next().unwrap()
            } else {
                OutputAction::MultipleActions(Arc::new(actions))
            };

            /* ★v24.31 序列控制键 (整条目标都是 KeySequenceToggle/Pause/Continue):
             * 不注入任何键, 按下时切全局序列暂停状态。占位 action 永不被模拟。 */
            let ctls: Vec<Option<SequenceCtl>> = target_keys
                .iter()
                .map(|k| crate::sequence::sequence_control_name(k))
                .collect();
            if !ctls.is_empty() && ctls.iter().all(|c| c.is_some()) {
                input_mappings.insert(
                    trigger_device.clone(),
                    InputMappingInfo {
                        target_action: OutputAction::KeyboardKey(0), // 占位: 永不到达模拟层
                        interval,
                        event_duration,
                        turbo_enabled: false,
                        double_tap_enabled: false,
                        double_tap_gap_ms: mapping.double_tap_gap_ms,
                        run_enabled: false,
                        run_threshold: mapping.run_threshold.clamp(50, 95),
                        run_recheck: mapping.run_recheck,
                        lock_enabled: false,
                        release_action: None,
                        sequence: None,
                        sequence_ctl: ctls[0],
                    },
                );
                continue;
            }

            /* ★v24.35 序列宏: sequence_text 非空时接管输出。序列条目上
             * 连发/锁定/双击/奔跑 全部停用。
             * ★v24.35 冲突修复: 解析失败**不再静默回退目标键** —— 回退会让行为
             * 突变 (连招变单按) 且 GUI 仍显示旧序列; 更糟的是「目标键空 + 序列坏」
             * 时占位 KeyboardKey(0) 会真被 simulate_press 注入。整条跳过 + 日志。 */
            let sequence: Option<Arc<[ResolvedStep]>> =
                if mapping.sequence_text.trim().is_empty() {
                    None
                } else {
                    let macro_map: HashMap<String, String> = config
                        .universal_macros
                        .iter()
                        .map(|m| (m.name.to_lowercase(), m.text.clone()))
                        .collect();
                    match crate::sequence::parse_sequence(&mapping.sequence_text, &macro_map) {
                        Ok(steps) => {
                            let mut resolved: Vec<ResolvedStep> = Vec::with_capacity(steps.len());
                            let mut ok = true;
                            for step in steps {
                                let mut acts: SmallVec<[OutputAction; 2]> = SmallVec::new();
                                for key in &step.keys {
                                    match Self::input_name_to_output(key) {
                                        Some(
                                            a @ (OutputAction::KeyboardKey(_)
                                            | OutputAction::MouseButton(_)
                                            | OutputAction::KeyCombo(_)),
                                        ) => acts.push(a),
                                        _ => {
                                            eprintln!(
                                                "[config] 序列步键无效/不支持: trigger={:?} key={:?}",
                                                mapping.trigger_key, key
                                            );
                                            ok = false;
                                            break;
                                        }
                                    }
                                }
                                if !ok {
                                    break;
                                }
                                resolved.push(ResolvedStep {
                                    actions: acts,
                                    hold_ms: step
                                        .hold_ms
                                        .unwrap_or(event_duration)
                                        .clamp(2, 60_000),
                                });
                            }
                            if ok && !resolved.is_empty() {
                                Some(Arc::from(resolved.into_boxed_slice()))
                            } else {
                                eprintln!(
                                    "[config] 序列解析失败, 本条映射不生效 (不回退目标键): {:?}",
                                    mapping.trigger_key
                                );
                                continue;
                            }
                        }
                        Err(e) => {
                            eprintln!(
                                "[config] 序列语法错误 ({e}), 本条映射不生效 (不回退目标键): {:?}",
                                mapping.trigger_key
                            );
                            continue;
                        }
                    }
                };

            // Create input mapping
            /* ★v21.0 奔跑互斥: 勾选【奔跑】时压制 连发/简易奔跑 —— 奔跑语义 =
             * 按住(轻推走) + 重推补敲(跑), 与连发的循环按压、双击的首按模拟都冲突 */
            input_mappings.insert(
                trigger_device.clone(),
                InputMappingInfo {
                    target_action,
                    interval,
                    event_duration,
                    turbo_enabled: mapping.turbo_enabled
                        && !mapping.run_enabled
                        && sequence.is_none(),
                    double_tap_enabled: mapping.double_tap_enabled
                        && !mapping.run_enabled
                        && sequence.is_none(),
                    double_tap_gap_ms: mapping.double_tap_gap_ms,
                    run_threshold: mapping.run_threshold.clamp(50, 95),
                    run_recheck: mapping.run_recheck,
                    /* ★v24.31 锁定与奔跑互斥: 奔跑有自己的按住语义, 双开语义打架;
                     * 序列条目上锁定也无意义 (序列自己管理按住节奏) */
                    lock_enabled: mapping.lock_enabled
                        && !mapping.run_enabled
                        && !mapping.double_tap_enabled
                        && sequence.is_none(),
                    /* ★v24.35 补漏: 重推奔跑同样与序列互斥 —— 运行时序列分支提前
                     * return 奔跑本就不会执行, 数据层双真只会造成 GUI 误显示 */
                    run_enabled: mapping.run_enabled && sequence.is_none(),
                    sequence,
                    sequence_ctl: None,
                    /* ★v24.31 抬起映射: 解析失败逐条跳过并记日志, 不阻断整表加载 */
                    release_action: {
                        let mut release_actions: SmallVec<[OutputAction; 4]> = SmallVec::new();
                        for release_key in &mapping.release_targets {
                            match Self::input_name_to_output(release_key) {
                                Some(
                                    action @ (OutputAction::KeyboardKey(_)
                                    | OutputAction::MouseButton(_)
                                    | OutputAction::KeyCombo(_)),
                                ) => release_actions.push(action),
                                Some(OutputAction::MouseMove(direction, _)) => {
                                    release_actions.push(OutputAction::MouseMove(direction, move_speed))
                                }
                                Some(OutputAction::MouseScroll(direction, _)) => {
                                    release_actions
                                        .push(OutputAction::MouseScroll(direction, move_speed))
                                }
                                other => {
                                    if other.is_none() {
                                        eprintln!(
                                            "[config] 抬起映射目标无效, 已跳过: trigger={:?} release={:?}",
                                            mapping.trigger_key, release_key
                                        );
                                    }
                                }
                            }
                        }
                        match release_actions.len() {
                            0 => None,
                            1 => Some(release_actions.into_iter().next().unwrap()),
                            _ => Some(OutputAction::MultipleActions(Arc::new(release_actions))),
                        }
                    },
                },
            );
        }

        Ok(input_mappings)
    }
}
