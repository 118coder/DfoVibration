//! 手柄映射配置读写助手 + 名称工具 (set_slot_*/trigger_matches_*/备注格式) —— 原 378-626, C3 归位。

use crate::config::{AppConfig, KeyMapping};
use super::*;

/// 触发键的短名 (校准向导里逐条展示用): 去掉设备前缀, 一眼看出串键。
/// 原始命名 `GAMEPAD_20BC_5159_DEV5A27EA46_H680322833` → `H680322833`;
/// XInput 命名 `GAMEPAD_045E_RS_Click` → `045E_RS_Click`。
pub fn short_trigger_name(name: &str) -> String {
    let Some(rest) = name.strip_prefix("GAMEPAD_") else {
        return name.to_string();
    };
    let parts: Vec<&str> = rest.split('_').collect();
    if parts.len() >= 4 && parts[2].starts_with("DEV") {
        parts[3..].join("_")
    } else {
        rest.to_string()
    }
}

/// ★v24.0: 槽位的**系统备注** —— 校准/映射的唯一关联键 (写入 `mapping.note`)。
///
/// 例: 「手柄A键（系统）」「手柄十字键·上（系统）」。
/// 它随映射一起保存在**当前预设**里 → 有线 / USB 接收器 / 蓝牙 各自哈希不同时,
/// 各用各的预设即可, 互不冲突 (用户要求)。
pub fn slot_note(label: &str) -> String {
    format!("手柄{label}（系统）")
}

/// 旧版备注格式 (`手柄·<标签>`) —— 仅供一次性迁移使用。
pub(super) fn legacy_slot_note(label: &str) -> String {
    format!("手柄·{label}")
}

/// 查找槽位对应的映射 —— **只按系统备注匹配**。
/// (不再回落到 `default_trigger`: 那会把 XInput 命名 (`GAMEPAD_045E_*`) 引进来,
///  与原始通道命名各中一条 = "按一个键触发两个"。)
pub fn find_slot_mapping_index(config: &AppConfig, slot: &GamepadSlot) -> Option<usize> {
    let note = slot_note(slot.label);
    config.mappings.iter().position(|m| m.note == note)
}

/// ★v24.0: 一次性迁移旧备注 (`手柄·X` → `手柄X（系统）`), 触发键/目标键原样保留。
/// 同槽位若已有系统备注条目, 则删掉旧的重复条目。返回是否有改动 (需落盘)。
pub fn migrate_slot_notes(config: &mut AppConfig) -> bool {
    let mut changed = false;
    for slot in SLOTS {
        let new_note = slot_note(slot.label);
        let old_note = legacy_slot_note(slot.label);
        let old_idxs: Vec<usize> = config
            .mappings
            .iter()
            .enumerate()
            .filter(|(_, m)| m.note == old_note)
            .map(|(i, _)| i)
            .collect();
        if old_idxs.is_empty() {
            continue;
        }
        if config.mappings.iter().any(|m| m.note == new_note) {
            /* 已有新备注条目 → 旧的重复条目删掉 */
            for &i in old_idxs.iter().rev() {
                config.mappings.remove(i);
                changed = true;
            }
        } else {
            /* 第一条改名, 其余删掉 */
            let keep = old_idxs[0];
            config.mappings[keep].note = new_note;
            for &i in old_idxs.iter().skip(1).rev() {
                config.mappings.remove(i);
            }
            changed = true;
        }
    }
    changed
}

/// ★v24.0: 触发键是否与当前"原始位组合"匹配 (SVG 按下即亮 / 识别态自动选中)。
/// 只有 RawInput 原始通道的 `GenericDevice` 命名能匹配 —— 与连发映射同一编码。
pub(super) fn trigger_matches_raw(trigger: &str, vid: u16, pid: u16, pos: u32) -> bool {
    if pos == 0 {
        return false;
    }
    let upper = trigger.to_uppercase();
    if !upper.starts_with(&format!("GAMEPAD_{vid:04X}_{pid:04X}_")) {
        return false;
    }
    matches!(
        crate::state::AppState::input_name_to_device(trigger),
        Some(crate::state::InputDevice::GenericDevice { button_id, .. }) if button_id as u32 == pos
    )
}

/// ★v24.6: 触发键是否与**实时 XInput 输入位图**匹配 (XInput 命名的槽位, 如摇杆方向/按下)。
/// `mask` 的 bit[id] = 该 XInput input id 当前按下 (见 `xinput.rs::inputs_to_bitset`)。
/// 走这条实时位图而不启用捕获模式 —— 捕获模式会抑制正常映射派发 (奔跑/方向失效的根因)。
pub fn trigger_matches_xinput(trigger: &str, vid: u16, mask: u32) -> bool {
    if mask == 0 {
        return false;
    }
    let Some(crate::state::InputDevice::XInputCombo {
        device_type,
        button_ids,
    }) = crate::state::AppState::input_name_to_device(trigger)
    else {
        return false;
    };
    let v = match device_type {
        crate::state::DeviceType::Gamepad(v) | crate::state::DeviceType::Joystick(v) => v,
        _ => return false,
    };
    v == vid
        && !button_ids.is_empty()
        && button_ids
            .iter()
            .all(|id| mask & (1u32 << (id & 31)) != 0)
}

/// ★v22.1: 鼠标虚拟键 (LBUTTON/RBUTTON/MBUTTON/XBUTTON1/2) —— 手柄页键盘捕获必须排除,
/// 否则点 SVG 上的键会被记成"鼠标按键"(用户实测的污染路径)。
pub fn is_mouse_vk(vk: u32) -> bool {
    matches!(vk, 0x01 | 0x02 | 0x04 | 0x05 | 0x06)
}

/// 设置槽位的触发键: 已有映射则更新, 没有则新建 (备注写系统备注)。
/// 返回映射在 config.mappings 中的下标。
pub fn set_slot_trigger(config: &mut AppConfig, slot: &GamepadSlot, trigger: String) -> usize {
    /* ★v21.7d: 触发键先去重+规范排序 (手柄多键组合也适用) */
    let trigger = crate::util::normalize_key_combo(&trigger);
    if let Some(idx) = find_slot_mapping_index(config, slot) {
        config.mappings[idx].trigger_key = trigger;
        config.mappings[idx].note = slot_note(slot.label);
        idx
    } else {
        config.mappings.push(KeyMapping {
            release_targets: Default::default(),
            sequence_text: String::new(),
            trigger_key: trigger,
            target_keys: Default::default(),
            interval: None,
            event_duration: None,
            /* ★v24.1: 新键位**默认不连发** —— 用户期望"按一下 = 一个动作"。
             * 旧默认 true 配合全局 interval=5ms, 按一下会立刻补发一次 → 游戏端看起来"按一个键出两个"。
             * 需要连发的键可在「更多设置」里单独勾选。 */
            turbo_enabled: false,
            move_speed: 5,
            double_tap_enabled: false,
            double_tap_gap_ms: 50,
            run_enabled: false,
            run_threshold: 80,
            run_recheck: true,
            lock_enabled: false,
            note: slot_note(slot.label),
        });
        config.mappings.len() - 1
    }
}

/// ★v24.3: 给槽位写触发键, 并**自愈旧数据串位** —— 若该触发键已被别的槽位占用,
/// 先把它从旧槽位移除 (同一个物理键不能属于两个槽位, 否则一个键亮两处 / 触发两条)。
/// 返回被移走的旧槽位标签 (供提示), 没有冲突则为 `None`。
///
/// 为什么是"移动"而不是"拒绝": 实机症状是历史配置把 A 键的位组合记在了 X 槽,
/// 用户重新校对按 A 时被拒绝, 表现为"A 键被识别成了 X 键"。校准时用户正在按的就是答案,
/// 所以以当前槽位为准。
pub fn set_slot_trigger_moving(
    config: &mut AppConfig,
    slot: &GamepadSlot,
    trigger: String,
) -> Option<String> {
    let name = trigger.clone();
    let mut moved_from = None;
    for other in SLOTS {
        if other.id == slot.id {
            continue;
        }
        if let Some(i) = find_slot_mapping_index(config, other)
            && config.mappings[i].trigger_key == name
        {
            config.mappings.remove(i);
            moved_from = Some(other.label.to_string());
            break;
        }
    }
    set_slot_trigger(config, slot, trigger);
    moved_from
}

/// 向槽位添加一个目标键 (★v24.26 重复键保留; 大小写规范化)。返回是否发生修改。
/// ★v24.0: 映射必须已由校对创建; 不存在则不改动 (调用方负责提示先校对)。
/// ⚠ 这是**追加**语义 —— 仅「鼠标映射…」菜单用它 (UI 文案就是"选一个即加入")。
/// 键盘捕获必须用 `set_slot_targets` (替换), 否则"X+A 改成 X"不会生效。
pub fn add_slot_target(config: &mut AppConfig, slot: &GamepadSlot, target: String) -> bool {
    let Some(idx) = find_slot_mapping_index(config, slot) else {
        return false;
    };
    let len_before = config.mappings[idx].target_keys.len();
    config.mappings[idx].add_target_key(target);
    config.mappings[idx].note = slot_note(slot.label);
    config.mappings[idx].target_keys.len() != len_before
}

/// ★v24.18: 用捕获结果**替换**该槽位的目标键 (设置语义)。
///
/// 修的是用户报的 bug: 手柄映射页里把键位从「X+A」重设为「X」不生效、仍显示「X+A」。
/// 根因 = 确认捕获时调用了追加版 `add_slot_target`: X 已在集合里 → 集合没变化。
/// 捕获流程 (第 2 步按键盘 + 「＋ 再加一个键」累加) 得出的本来就是该槽位**完整**的目标键,
/// 所以这里必须整份替换。传入的 `target` 支持用 `+` 连接的多键 (内部会拆分+去重+规范排序)。
/// 返回是否发生修改 (调用方用于决定要不要提示)。
pub fn set_slot_targets(config: &mut AppConfig, slot: &GamepadSlot, target: String) -> bool {
    let Some(idx) = find_slot_mapping_index(config, slot) else {
        return false;
    };
    let before = config.mappings[idx].target_keys.clone();
    config.mappings[idx].clear_target_keys();
    config.mappings[idx].add_target_key(target);
    config.mappings[idx].note = slot_note(slot.label);
    config.mappings[idx].target_keys != before
}

/// 清空槽位的目标键。
pub fn clear_slot_targets(config: &mut AppConfig, slot: &GamepadSlot) {
    if let Some(idx) = find_slot_mapping_index(config, slot) {
        config.mappings[idx].clear_target_keys();
        config.mappings[idx].note = slot_note(slot.label);
    }
}

/// 删除槽位对应的映射。
pub fn remove_slot_mapping(config: &mut AppConfig, slot: &GamepadSlot) {
    if let Some(idx) = find_slot_mapping_index(config, slot) {
        config.mappings.remove(idx);
    }
}

/// 获取槽位当前映射的触发键 (未校对 → 空串)。
pub fn slot_trigger_display(config: &AppConfig, slot: &GamepadSlot) -> String {
    if let Some(idx) = find_slot_mapping_index(config, slot) {
        config.mappings[idx].trigger_key.clone()
    } else {
        String::new()
    }
}

/// 获取槽位当前映射的目标键列表。
pub fn slot_targets(config: &AppConfig, slot: &GamepadSlot) -> Vec<String> {
    if let Some(idx) = find_slot_mapping_index(config, slot) {
        config.mappings[idx].target_keys.to_vec()
    } else {
        Vec::new()
    }
}

