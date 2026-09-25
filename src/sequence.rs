//! ★v24.31 按键序列宏 (v24.31-4, QKeyMapper 借鉴)。
//!
//! 语法 (与 QKeyMapper 兼容, 另提供 ASCII 备选写法):
//! - 步与步之间用 `»` 分隔 (ASCII 备选 `>>`);
//! - 每步 = 键名用 `+` 连接 (同按), 可带 `⏱毫秒` (ASCII 备选 `@毫秒`) 设定该步按住时长;
//! - `NONE⏱200` (或 `NONE@200`) = 纯等待 200ms;
//! - `宏(名字)` = 展开为通用宏列表里同名宏的序列内容 (可递归一层防环);
//! - 省略 `⏱` 的步使用本条映射的"时长"设定。
//! 示例: `A+B⏱50»NONE⏱200»C⏱50` = A、B 同按 50ms → 等 200ms → C 按 50ms。
//!
//! 执行模型: 每条序列在**独立线程**里跑 (绝不占输入 worker —— v24.28 的"连招卡死"
//! 教训), 每步 按下→保持→松开; 支持全局暂停/继续 (控制键) 与按设备停止。

use crate::state::{AppState, OutputAction, ResolvedStep, SequenceCtl};
use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::Arc;

/// 序列录制支持的键盘 vk 判定 (与 state::key_name_to_vk 词表互逆;
/// 手柄键不在此列 —— 序列的输出是键盘动作, 手柄键无法录)
pub fn is_seq_key_vk(vk: u32) -> bool {
    matches!(vk,
        0x08 | 0x09 | 0x0C | 0x0D | 0x10..=0x14 | 0x1B | 0x20 | 0x21 | 0x22 | 0x23 | 0x24
        | 0x25..=0x28 | 0x2D | 0x2E | 0x2C
        | 0x30..=0x39 | 0x41..=0x5A | 0x5B | 0x5C
        | 0x60..=0x69 | 0x6A..=0x6F
        | 0x70..=0x87
        | 0x90 | 0x91 | 0xA0..=0xA5
        | 0xBA | 0xBB | 0xBC | 0xBD | 0xBE | 0xBF | 0xC0
        | 0xDB | 0xDC | 0xDD | 0xDE | 0xDF | 0xE2)
}

/// ★v24.31 录制轮询线程 (GetAsyncKeyState 10ms 轮询, 按键沿驱动)。
/// **不依赖 LL 键盘钩子** —— 本机实测钩子按启动随机失效 (探针证实),
/// 而 GetAsyncKeyState 是预设切换键已验证可靠的同款机制, 且不受输入法影响
/// (读物理键状态, VK_PROCESSKEY 不会出现)。手柄键不在词表 → 自然被忽略。
pub fn run_key_record_poller(state: Arc<AppState>) {
    const POLL_MS: u64 = 10;
    let mut prev_down = [false; 256usize];
    while state.key_record_active.load(Ordering::Relaxed) {
        for vk in 0u32..=255u32 {
            if !is_seq_key_vk(vk) {
                continue;
            }
            let down = unsafe {
                use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
                (GetAsyncKeyState(vk as i32) as u16 & 0x8000) != 0
            };
            let was = &mut prev_down[vk as usize];
            if down && !*was {
                state.record_key_state(true, vk);
            } else if !down && *was {
                state.record_key_state(false, vk);
            }
            *was = down;
        }
        std::thread::sleep(std::time::Duration::from_millis(POLL_MS));
    }
}

/// 解析序列文本为步骤列表 (键名层, 尚未转 OutputAction)。
/// `macros`: 通用宏名 → 宏文本 (大小写不敏感)。
pub fn parse_sequence(
    text: &str,
    macros: &HashMap<String, String>,
) -> Result<Vec<SeqStep>, String> {
    parse_inner(text, macros, 0)
}

#[derive(Debug, Clone, PartialEq)]
pub struct SeqStep {
    /// 键名 (空 = 纯等待步)
    pub keys: Vec<String>,
    /// 按住毫秒 (None = 用映射默认时长)
    pub hold_ms: Option<u64>,
}

fn parse_inner(
    text: &str,
    macros: &HashMap<String, String>,
    depth: usize,
) -> Result<Vec<SeqStep>, String> {
    if depth > 3 {
        return Err("通用宏嵌套过深 (疑似循环引用)".to_string());
    }
    let text = text.trim();
    if text.is_empty() {
        return Ok(Vec::new());
    }
    // 简化写法: `>>` 和单个 `>` 都当作 `»`; `:N` 当作 `⏱N`; `等N`/`等待N` = 等待步
    let normalized = text.replace(">>", "»").replace('>', "»").replace('：', ":");
    let mut steps: Vec<SeqStep> = Vec::new();
    for raw_step in normalized.split('»') {
        let step_text = raw_step.trim();
        if step_text.is_empty() {
            return Err("存在空的步骤 (检查 »/>> 分隔符)".to_string());
        }
        // 通用宏引用: 宏(名字) — 允许带 ⏱ 覆盖整段宏的每步时长? v1: 不允许, 宏内自带
        let macro_ref = step_text
            .strip_prefix("宏(")
            .and_then(|rest| rest.strip_suffix(')'))
            .map(|s| s.trim().to_string());
        if let Some(name) = macro_ref {
            let expanded = macros
                .get(&name.to_lowercase())
                .ok_or_else(|| format!("通用宏「{name}」不存在"))?;
            let inner = parse_inner(expanded, macros, depth + 1)?;
            if inner.is_empty() {
                return Err(format!("通用宏「{name}」是空的"));
            }
            steps.extend(inner);
            continue;
        }
        // 拆 ⏱ / @ 时长后缀
        let (keys_part, hold) = split_hold(step_text)?;
        let keys_part = keys_part.trim();
        // ★简化写法: 等200 / 等待200 = 纯等待步 (等价 NONE⏱200)
        let keys_part = if keys_part.starts_with("等待") || keys_part.starts_with("等") {
            &keys_part[keys_part.char_indices().nth(if keys_part.starts_with("等待") { 2 } else { 1 }).map(|(i, _)| i).unwrap_or(0)..]
        } else {
            keys_part
        };
        let is_wait = keys_part.is_empty() || keys_part.eq_ignore_ascii_case("NONE");
        let keys: Vec<String> = if is_wait {
            if hold.is_none() {
                return Err("纯等待步必须带时长, 如 NONE⏱200 (或 NONE@200)".to_string());
            }
            Vec::new()
        } else {
            keys_part
                .split('+')
                .map(|k| {
                    let k = k.trim().to_uppercase();
                    // 宏(名) 不允许出现在键名位置 (只支持整步引用), 防歧义
                    if k.starts_with("宏(") {
                        Err(format!("「{k}」: 通用宏引用必须独占一步 (整步写成 宏(名字))"))
                    } else if k.is_empty() {
                        Err("键名为空 (检查 + 连接符)".to_string())
                    } else {
                        Ok(k)
                    }
                })
                .collect::<Result<Vec<String>, String>>()?
        };
        steps.push(SeqStep { keys, hold_ms: hold });
    }
    Ok(steps)
}

/// 拆出 `⏱N` / `@N` 后缀 (ASCII 备选)。
fn split_hold(step_text: &str) -> Result<(String, Option<u64>), String> {
    // 优先找 ⏱, 否则找最后一个 @ (键名里没有 @)
    if let Some(pos) = step_text.find('⏱') {
        let hold = step_text[pos + '⏱'.len_utf8()..]
            .trim()
            .parse::<u64>()
            .map_err(|_| format!("无效的时长: {:?} (⏱ 后必须是毫秒数)", &step_text[pos + 3..]))?;
        return Ok((step_text[..pos].to_string(), Some(hold)));
    }
    if let Some(pos) = step_text.rfind('@') {
        let hold = step_text[pos + 1..]
            .trim()
            .parse::<u64>()
            .map_err(|_| format!("无效的时长: {:?} (@ 后必须是毫秒数)", &step_text[pos + 1..]))?;
        return Ok((step_text[..pos].to_string(), Some(hold)));
    }
    // ★简化写法: `:N` (A+B:N 的冒号形式)
    if let Some(pos) = step_text.rfind(':') {
        let hold = step_text[pos + 1..]
            .trim()
            .parse::<u64>()
            .map_err(|_| format!("无效的时长: {:?} (: 后必须是毫秒数)", &step_text[pos + 1..]))?;
        return Ok((step_text[..pos].to_string(), Some(hold)));
    }
    Ok((step_text.to_string(), None))
}

/// ★v24.32 步骤 → 文本 (可视化编辑器回写; 用最简 ASCII 形态, 用户可直接读改):
/// 键步 `A+B:50`, 等待步 `NONE:200`, 步间 `>`。
pub fn steps_to_text(steps: &[SeqStep], default_hold: u64) -> String {
    steps
        .iter()
        .filter(|s| !(s.keys.is_empty() && s.hold_ms.is_none()))
        .map(|s| {
            if s.keys.is_empty() {
                format!("NONE:{}", s.hold_ms.unwrap_or(default_hold))
            } else {
                format!("{}:{}", s.keys.join("+"), s.hold_ms.unwrap_or(default_hold))
            }
        })
        .collect::<Vec<String>>()
        .join(">")
}

/// 序列控制键名 → 控制类型 (大小写不敏感)。
pub fn sequence_control_name(name: &str) -> Option<SequenceCtl> {
    match name.trim().to_uppercase().as_str() {
        "KEYSEQUENCETOGGLE" => Some(SequenceCtl::Toggle),
        "KEYSEQUENCEPAUSE" => Some(SequenceCtl::Pause),
        "KEYSEQUENCECONTINUE" => Some(SequenceCtl::Continue),
        _ => None,
    }
}

/// 录制缓冲 → 序列文本。
/// 时间上重叠的按键合并为同一步 (A+B); 相邻两步的空档 ≥50ms 生成 NONE 等待步。
/// 同一时刻先处理抬起再处理按下 (避免相邻两步的同名键被并进同一步)。
pub fn recorded_to_sequence(
    events: &[RecordedKey],
    vk_name: &dyn Fn(u32) -> Option<String>,
) -> String {
    let mut timeline: Vec<(u64, bool, u32)> = events
        .iter()
        .flat_map(|e| {
            let mut v = vec![(e.down_ms, true, e.vk)];
            if let Some(up) = e.up_ms {
                v.push((up, false, e.vk));
            }
            v
        })
        .collect();
    // (时间, is_down): is_down=false (抬起) 排在 true (按下) 前
    timeline.sort_by_key(|(t, is_down, _)| (*t, *is_down));

    let mut active: Vec<(u32, u64)> = Vec::new(); // (vk, 按下时刻)
    let mut step_keys: Vec<String> = Vec::new(); // 本步出现过的键名 (按下顺序, 去重)
    let mut step_start: Option<u64> = None;
    let mut steps: Vec<(Vec<String>, u64, u64)> = Vec::new(); // (keys, start, end)

    for &(
        t,
        is_down,
        vk,
    ) in &timeline
    {
        if is_down {
            let name = vk_name(vk).filter(|n| !n.is_empty());
            let Some(name) = name else { continue };
            match step_start {
                None => {
                    step_start = Some(t);
                    step_keys.push(name);
                    active.push((vk, t));
                }
                Some(_) => {
                    if !step_keys.contains(&name) {
                        step_keys.push(name);
                    }
                    active.push((vk, t));
                }
            }
        } else if step_start.is_some() {
            if let Some(pos) = active.iter().position(|(v, _)| *v == vk) {
                active.remove(pos);
                if active.is_empty() {
                    steps.push((
                        std::mem::take(&mut step_keys),
                        step_start.take().unwrap(),
                        t,
                    ));
                }
            }
        }
    }
    // 录制结束时仍按着的键: 以最后一个事件时间闭合
    if let Some(start) = step_start {
        let end = timeline.last().map(|(t, _, _)| *t).unwrap_or(start);
        if !step_keys.is_empty() {
            steps.push((step_keys, start, end.max(start)));
        }
    }

    // 拼文本
    let mut out: Vec<String> = Vec::new();
    let mut prev_end: Option<u64> = None;
    for (keys, start, end) in &steps {
        if let Some(pe) = prev_end {
            let gap = start.saturating_sub(pe);
            if gap >= 50 {
                out.push(format!("NONE⏱{gap}"));
            }
        }
        let hold = end.saturating_sub(*start).max(1);
        out.push(format!("{}⏱{hold}", keys.join("+")));
        prev_end = Some(*end);
    }
    out.join("»")
}

/// 录制事件 (相对录制开始的毫秒)。
#[derive(Debug, Clone, Copy)]
pub struct RecordedKey {
    pub vk: u32,
    pub down_ms: u64,
    pub up_ms: Option<u64>,
}

/// 序列执行线程 (由 AppState::start_sequence_run 派生)。
pub(crate) fn run_sequence(
    state: Arc<AppState>,
    device: crate::state::InputDevice,
    steps: Arc<[ResolvedStep]>,
    run: Arc<crate::state::SequenceRun>,
) {
    const SLICE_MS: u64 = 15;
    /* ★v24.31 审计: panic 也要注销登记 —— 否则该设备永远无法再触发序列 */
    let body = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        for step in steps.iter() {
            if run.stop.load(Ordering::Relaxed) {
                break;
            }
            // 等待步
            if step.actions.is_empty() {
                sleep_responsive(&state, &run, step.hold_ms);
                continue;
            }
            // 按下全部 → 保持 → 松开全部
            for a in &step.actions {
                state.simulate_press(a);
            }
            sleep_responsive(&state, &run, step.hold_ms);
            for a in &step.actions {
                state.simulate_release(a);
            }
        }
    }));
    state.sequence_run_finished(&device, &run);
    if let Err(payload) = body {
        crate::util::crash_log(
            "seq-runner",
            &format!("panic: {}", crate::util::panic_payload_str(payload.as_ref())),
        );
    }
}

/// 可中断睡眠: 响应 全局暂停 (暂停期间原地等待) 与 本条序列的停止。
fn sleep_responsive(state: &AppState, run: &crate::state::SequenceRun, total_ms: u64) {
    const SLICE_MS: u64 = 15;
    let mut waited = 0u64;
    while waited < total_ms {
        if run.stop.load(Ordering::Relaxed) {
            return;
        }
        // ★v24.31 审计: 应用暂停 或 序列暂停 → 原地小睡等恢复 (停止优先)
        while (state.sequence_paused.load(Ordering::Relaxed) || state.is_paused())
            && !run.stop.load(Ordering::Relaxed)
        {
            std::thread::sleep(std::time::Duration::from_millis(30));
        }
        std::thread::sleep(std::time::Duration::from_millis(SLICE_MS));
        waited += SLICE_MS;
    }
}

/// vk → 序列键名 (与 state::key_name_to_vk 词表互逆; 未知 vk 返回 None → 录制时忽略)。
pub fn vk_to_seq_name(vk: u32) -> Option<String> {
    let name = match vk {
        0x41..=0x5A | 0x30..=0x39 => char::from_u32(vk)?.to_string(),
        0x70..=0x87 => format!("F{}", vk - 0x70 + 1),
        0x60..=0x69 => format!("NUMPAD{}", vk - 0x60),
        0x1B => "ESC".into(),
        0x0D => "ENTER".into(),
        0x09 => "TAB".into(),
        0x0C => "CLEAR".into(),
        0x13 => "PAUSE".into(),
        0x14 => "CAPSLOCK".into(),
        0x20 => "SPACE".into(),
        0x08 => "BACKSPACE".into(),
        0x2E => "DELETE".into(),
        0x2D => "INSERT".into(),
        0x24 => "HOME".into(),
        0x23 => "END".into(),
        0x21 => "PAGEUP".into(),
        0x22 => "PAGEDOWN".into(),
        0x26 => "UP".into(),
        0x28 => "DOWN".into(),
        0x25 => "LEFT".into(),
        0x27 => "RIGHT".into(),
        0xA0 => "LSHIFT".into(),
        0xA1 => "RSHIFT".into(),
        0xA2 => "LCTRL".into(),
        0xA3 => "RCTRL".into(),
        0xA4 => "LALT".into(),
        0xA5 => "RALT".into(),
        0x5B => "LWIN".into(),
        0x5C => "RWIN".into(),
        0x90 => "NUMLOCK".into(),
        0x91 => "SCROLL".into(),
        0x2C => "SNAPSHOT".into(),
        0x6A => "MULTIPLY".into(),
        0x6B => "ADD".into(),
        0x6C => "SEPARATOR".into(),
        0x6D => "SUBTRACT".into(),
        0x6E => "DECIMAL".into(),
        0x6F => "DIVIDE".into(),
        0xBA => "OEM_1".into(),
        0xBB => "OEM_PLUS".into(),
        0xBC => "OEM_COMMA".into(),
        0xBD => "OEM_MINUS".into(),
        0xBE => "OEM_PERIOD".into(),
        0xBF => "OEM_2".into(),
        0xC0 => "OEM_3".into(),
        0xDB => "OEM_4".into(),
        0xDC => "OEM_5".into(),
        0xDD => "OEM_6".into(),
        0xDE => "OEM_7".into(),
        0xE2 => "OEM_102".into(),
        0x01 => "LBUTTON".into(),
        0x02 => "RBUTTON".into(),
        0x04 => "MBUTTON".into(),
        0x05 => "XBUTTON1".into(),
        0x06 => "XBUTTON2".into(),
        _ => return None,
    };
    Some(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn macros(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn parses_basic_qkeymapper_syntax() {
        let steps = parse_sequence("A+B⏱50»NONE⏱200»C⏱50", &HashMap::new()).unwrap();
        assert_eq!(steps.len(), 3);
        assert_eq!(steps[0].keys, vec!["A", "B"]);
        assert_eq!(steps[0].hold_ms, Some(50));
        assert!(steps[1].keys.is_empty(), "NONE = 纯等待步");
        assert_eq!(steps[1].hold_ms, Some(200));
        assert_eq!(steps[2].keys, vec!["C"]);
        assert_eq!(steps[2].hold_ms, Some(50));
    }

    #[test]
    fn parses_ascii_fallback_and_default_hold() {
        let steps = parse_sequence("DOWN+Z>>NONE@80>>J", &HashMap::new()).unwrap();
        assert_eq!(steps.len(), 3);
        assert_eq!(steps[0].keys, vec!["DOWN", "Z"]);
        assert!(steps[0].hold_ms.is_none(), "省略 ⏱ → 用映射默认时长");
        assert_eq!(steps[1].keys, Vec::<String>::new(), "NONE = 纯等待");
        assert_eq!(steps[1].hold_ms, Some(80));
        assert_eq!(steps[2].keys, vec!["J"]);
        // 键 + @时长 = 普通按键步 (不是等待)
        let steps = parse_sequence("SPACE@80", &HashMap::new()).unwrap();
        assert_eq!(steps[0].keys, vec!["SPACE"]);
        assert_eq!(steps[0].hold_ms, Some(80));
    }

    #[test]
    fn expands_universal_macros_one_level() {
        let m = macros(&[("觉醒", "LCTRL⏱30>>K⏱30")]);
        let steps = parse_sequence("宏(觉醒)»NONE⏱100»DFO", &m).unwrap();
        assert_eq!(steps.len(), 4, "宏展开的 2 步 + 等待 + DFO");
        assert_eq!(steps[0].keys, vec!["LCTRL"]);
        assert_eq!(steps[1].keys, vec!["K"]);
        assert_eq!(steps[2].keys, Vec::<String>::new());
        assert_eq!(steps[3].keys, vec!["DFO"]);
        // 名字大小写不敏感
        assert!(parse_sequence("宏(觉醒)", &m).is_ok());
    }

    #[test]
    fn rejects_broken_syntax() {
        assert!(parse_sequence("A+B⏱50»»C", &HashMap::new()).is_err(), "空步骤");
        assert!(parse_sequence("NONE", &HashMap::new()).is_err(), "等待步缺时长");
        assert!(parse_sequence("A⏱abc", &HashMap::new()).is_err(), "时长不是数字");
        assert!(parse_sequence("宏(不存在)", &HashMap::new()).is_err(), "宏未定义");
        // 宏循环引用 → 深度保护
        let m = macros(&[("a", "宏(b)"), ("b", "宏(a)")]);
        assert!(parse_sequence("宏(a)", &m).is_err());
    }

    #[test]
    fn control_names_resolve() {
        assert!(matches!(
            sequence_control_name("KeySequenceToggle"),
            Some(SequenceCtl::Toggle)
        ));
        assert!(matches!(
            sequence_control_name("keysequencepause"),
            Some(SequenceCtl::Pause)
        ));
        assert!(sequence_control_name("KeySequenceContinue").is_some());
        assert!(sequence_control_name("A").is_none());
    }

    /// ★v24.31 决定性验证: 轮询录制器在本机真实捕获 keybd_event 合成的按键
    /// (进程内自注入 → GetAsyncKeyState 轮询 → 事件 → 序列文本)。
    #[test]
    fn poller_captures_synthesized_keys_end_to_end() {
        let state = std::sync::Arc::new(crate::state::AppState::new(crate::config::AppConfig::default()).unwrap());
        state.start_key_record();
        let poller_state = state.clone();
        let _t = std::thread::Builder::new()
            .spawn(move || run_key_record_poller(poller_state))
            .unwrap();
        // 等 poller 起来, 然后真实注入 'A' (不带 SIMULATED_EVENT_MARKER → 被当真实按键)
        std::thread::sleep(std::time::Duration::from_millis(300));
        unsafe {
            use windows::Win32::UI::Input::KeyboardAndMouse::keybd_event;
            keybd_event(0x41, 0, windows::Win32::UI::Input::KeyboardAndMouse::KEYBD_EVENT_FLAGS(0), 0usize);
            std::thread::sleep(std::time::Duration::from_millis(120));
            keybd_event(0x41, 0, windows::Win32::UI::Input::KeyboardAndMouse::KEYBD_EVENT_FLAGS(2), 0usize);
        }
        std::thread::sleep(std::time::Duration::from_millis(300));
        let events = state.stop_key_record().expect("应有事件缓冲");
        assert!(
            events.iter().any(|e| e.vk == 0x41),
            "轮询录制器应捕获 A: {:?}",
            events
        );
        let text = recorded_to_sequence(&events, &vk_to_seq_name);
        assert!(text.contains('A'), "序列文本应含 A: {text}");
    }

    #[test]
    fn recorded_events_become_sequence_text() {
        let name = vk_to_seq_name;
        // A 按 100ms → 停 120ms → B+C 重叠 80ms
        let events = vec![
            RecordedKey { vk: 0x41, down_ms: 0, up_ms: Some(100) },   // A
            RecordedKey { vk: 0x42, down_ms: 220, up_ms: Some(300) }, // B
            RecordedKey { vk: 0x43, down_ms: 240, up_ms: Some(300) }, // C
        ];
        let text = recorded_to_sequence(&events, &name);
        assert_eq!(text, "A⏱100»NONE⏱120»B+C⏱80");
        // 录制结束时仍按着: 以最后事件时间闭合
        let events = vec![RecordedKey { vk: 0x41, down_ms: 0, up_ms: None }];
        assert_eq!(recorded_to_sequence(&events, &name), "A⏱1");
        // 未知 vk 被忽略
        let events = vec![RecordedKey { vk: 0xFF, down_ms: 0, up_ms: Some(10) }];
        assert_eq!(recorded_to_sequence(&events, &name), "");
    }
}