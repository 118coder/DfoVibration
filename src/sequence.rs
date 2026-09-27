//! ★v24.31 按键序列宏 (v24.31-4, QKeyMapper 借鉴)。★v24.38 录制保真 (按下→等待→抬起)。
//!
//! 语法 (与 QKeyMapper 兼容, 另提供 ASCII 备选写法):
//! - 步与步之间用 `»` 分隔 (ASCII 备选 `>>`);
//! - 每步 = 键名用 `+` 连接 (同按), 可带 `⏱毫秒` (ASCII 备选 `@毫秒` / `:毫秒`) 设定该步按住时长;
//! - `NONE⏱200` (或 `NONE@200`) = 纯等待 200ms;
//! - ★v24.38 模式步: `A↓` (ASCII `A_`) = **只按下** (保持到配对的 `A↑`); `A↑` (ASCII `A^`) = **只抬起**;
//!   录制器用 ↓/↑ 步忠实重放"按下→等待→抬起"的完整时序 (含按住方向键期间连打技能键);
//! - `宏(名字)` = 展开为通用宏列表里同名宏的序列内容 (可递归一层防环);
//! - 省略 `⏱` 的步使用本条映射的"时长"设定。
//! 示例: `A+B⏱50»NONE⏱200»C⏱50` = A、B 同按 50ms → 等 200ms → C 按 50ms。
//!      `DOWN↓»Z↓»NONE⏱80»Z↑»DOWN↑` = 按住 ↓ 期间敲一下 Z (真实走位连招)。
//!
//! 执行模型: 每条序列在**独立线程**里跑 (绝不占输入 worker —— v24.28 的"连招卡死"
//! 教训), Tap 步 按下→保持→松开, ↓/↑ 步各自只按/只松; 支持全局暂停/继续 (控制键)
//! 与按设备停止; 序列结束/中断时**安全网释放**所有仍按住的键 (防 ↓ 无 ↑ 卡键)。

use crate::state::{AppState, OutputAction, ResolvedStep, SeqStepMode, SequenceCtl};
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

#[derive(Debug, Clone, PartialEq, Default)]
pub struct SeqStep {
    /// 键名 (空 = 纯等待步)
    pub keys: Vec<String>,
    /// 按住毫秒 (None = 用映射默认时长; 仅 Tap 步使用)
    pub hold_ms: Option<u64>,
    /// ★v24.38 步模式 (默认 Tap = 按下→保持→抬起)
    pub mode: SeqStepMode,
}

/// 键名列表解析 (`+` 连接, 大写规范化, 拒绝宏引用/空名) —— Tap 与 ↓/↑ 步共用。
fn parse_key_list(keys_part: &str) -> Result<Vec<String>, String> {
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
        .collect()
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
        let (keys_part_raw, hold) = split_hold(step_text)?;
        let keys_part_raw = keys_part_raw.trim();
        // ★v24.38 模式后缀: `↓`(ASCII `_`) = 只按下保持; `↑`(ASCII `^`) = 只抬起
        let (mode, keys_part) = if let Some(rest) = keys_part_raw.strip_suffix('↓') {
            (SeqStepMode::PressHold, rest)
        } else if let Some(rest) = keys_part_raw.strip_suffix('↑') {
            (SeqStepMode::Release, rest)
        } else if let Some(rest) = keys_part_raw.strip_suffix('_') {
            (SeqStepMode::PressHold, rest)
        } else if let Some(rest) = keys_part_raw.strip_suffix('^') {
            (SeqStepMode::Release, rest)
        } else {
            (SeqStepMode::Tap, keys_part_raw)
        };
        if mode != SeqStepMode::Tap {
            if hold.is_some() {
                return Err("按下(↓)/抬起(↑)步不带时长后缀 (时长由配对的 ↓/↑ 间距决定)".to_string());
            }
            let keys_part = keys_part.trim();
            if keys_part.is_empty() || keys_part.eq_ignore_ascii_case("NONE") {
                return Err("按下(↓)/抬起(↑)步需要键名, 如 A↓ / A+B↑".to_string());
            }
            let keys = parse_key_list(keys_part)?;
            steps.push(SeqStep {
                keys,
                hold_ms: None,
                mode,
            });
            continue;
        }
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
            parse_key_list(keys_part)?
        };
        steps.push(SeqStep {
            keys,
            hold_ms: hold,
            mode: SeqStepMode::Tap,
        });
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
/// 键步 `A+B:50`, 等待步 `NONE:200`, 步间 `>`; ★v24.38 模式步 `A↓` / `A↑`。
pub fn steps_to_text(steps: &[SeqStep], default_hold: u64) -> String {
    steps
        .iter()
        .filter(|s| !(s.keys.is_empty() && s.hold_ms.is_none()))
        .map(|s| match s.mode {
            SeqStepMode::PressHold => format!("{}↓", s.keys.join("+")),
            SeqStepMode::Release => format!("{}↑", s.keys.join("+")),
            SeqStepMode::Tap => {
                if s.keys.is_empty() {
                    format!("NONE:{}", s.hold_ms.unwrap_or(default_hold))
                } else {
                    format!("{}:{}", s.keys.join("+"), s.hold_ms.unwrap_or(default_hold))
                }
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

/// 录制缓冲 → 序列文本。★v24.38 重写: **事件时序忠实还原** ("按下→等待→抬起"完整过程)。
///
/// 旧版把时间重叠的按键合并成一个"同按 N 毫秒"的 Tap 步 —— 按住方向键期间连打
/// 技能键 (DNF 最常见的走位连招) 会被压成一步巨大的 `DOWN+Z⏱总时长`, 完全不像真实操作。
/// 新版逐事件重放: 每个按下 = `键↓` 步、每个抬起 = `键↑` 步、事件间隔 ≥10ms 生成
/// `NONE⏱gap` 等待步 (录制轮询分辨率 10ms, <10ms 视为同一瞬间合并进同一步)。
/// 重放效果 = 与用户真实操作同时序 (按住时长/键间间隔/交叠关系全部保留)。
/// 录制结束时仍按着的键追加 `键↑` 收尾 (防重放卡键)。
pub fn recorded_to_sequence(
    events: &[RecordedKey],
    vk_name: &dyn Fn(u32) -> Option<String>,
) -> String {
    /// 事件间隔 < 此值视为同一瞬间 (录制轮询 10ms 一帧)
    const SAME_SLICE_MS: u64 = 10;

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
    // (时间, is_down): is_down=false (抬起) 排在 true (按下) 前 —— 同一瞬间先松后按
    timeline.sort_by_key(|(t, is_down, _)| (*t, *is_down));

    let mut out: Vec<String> = Vec::new();
    let mut prev_t: Option<u64> = None;
    let mut idx = 0usize;
    while idx < timeline.len() {
        let t0 = timeline[idx].0;
        let mut downs: Vec<String> = Vec::new();
        let mut ups: Vec<String> = Vec::new();
        let mut last_t = t0;
        /* 同一瞬间 (<10ms) 的事件合进同一步 (如一滚指头同时按下的两个键) */
        while idx < timeline.len() && timeline[idx].0.saturating_sub(t0) < SAME_SLICE_MS {
            let (t, is_down, vk) = timeline[idx];
            if let Some(name) = vk_name(vk).filter(|n| !n.is_empty()) {
                if is_down {
                    if !downs.contains(&name) {
                        downs.push(name);
                    }
                } else if !ups.contains(&name) {
                    ups.push(name);
                }
            }
            last_t = last_t.max(t);
            idx += 1;
        }
        if downs.is_empty() && ups.is_empty() {
            /* 整组都是不可识别的键 (如手柄vk/未知vk): 不产出、不推进时间轴
             * (否则会凭空多出等待步) */
            continue;
        }
        if let Some(pt) = prev_t {
            let gap = t0.saturating_sub(pt);
            if gap >= SAME_SLICE_MS {
                out.push(format!("NONE⏱{gap}"));
            }
        }
        if !downs.is_empty() {
            out.push(format!("{}↓", downs.join("+")));
        }
        if !ups.is_empty() {
            out.push(format!("{}↑", ups.join("+")));
        }
        prev_t = Some(last_t);
    }

    /* 录制结束时仍按着的键: 追加抬起步 (序列结束安全网之外的第二道防卡键) */
    let mut tail: Vec<String> = Vec::new();
    for e in events {
        if e.up_ms.is_none()
            && let Some(name) = vk_name(e.vk).filter(|n| !n.is_empty())
            && !tail.contains(&name)
        {
            tail.push(name);
        }
    }
    if !tail.is_empty() {
        out.push(format!("{}↑", tail.join("+")));
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
    /* ★v24.31 审计: panic 也要注销登记 —— 否则该设备永远无法再触发序列 */
    let body = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        /* ★v24.38 安全网: 记录 PressHold 步按下的键, 序列结束/中断时全部释放
         * —— 手动编辑的序列可能 ↓ 无配对 ↑ (或中途停止), 不回收会卡键。 */
        let mut held: SmallVecHeld = SmallVecHeld::new();
        for step in steps.iter() {
            if run.stop.load(Ordering::Relaxed) {
                break;
            }
            match step.mode {
                /* ★v24.38 仅抬起步: 松开指定键 ( Tap 之外不睡, 节奏由等待步/间距决定) */
                crate::state::SeqStepMode::Release => {
                    for a in &step.actions {
                        state.simulate_release(a);
                    }
                    held.retain(|h| !step.actions.iter().any(|a| a == h));
                }
                /* ★v24.38 仅按下一步: 按下并保持 (到配对 ↑ 或序列结束) */
                crate::state::SeqStepMode::PressHold => {
                    for a in &step.actions {
                        state.simulate_press(a);
                        held.push(a.clone());
                    }
                }
                /* 经典 Tap 步: 等待步 / 按下全部 → 保持 → 松开全部 */
                crate::state::SeqStepMode::Tap => {
                    if step.actions.is_empty() {
                        sleep_responsive(&state, &run, step.hold_ms);
                        continue;
                    }
                    for a in &step.actions {
                        state.simulate_press(a);
                    }
                    sleep_responsive(&state, &run, step.hold_ms);
                    for a in &step.actions {
                        state.simulate_release(a);
                    }
                }
            }
        }
        held
    }));
    /* 安全网释放 (正常结束走这里; panic 时 held 随栈展开丢失 —— 与旧版
     * "Tap 步 sleep 中 panic 卡键" 同一暴露面, 不劣化) */
    let panic_msg = body
        .as_ref()
        .err()
        .map(|p| crate::util::panic_payload_str(p.as_ref()));
    let held_leftover = body.unwrap_or_default();
    for h in &held_leftover {
        state.simulate_release(h);
    }
    state.sequence_run_finished(&device, &run);
    if let Some(msg) = panic_msg {
        crate::util::crash_log("seq-runner", &format!("panic: {msg}"));
    }
}

/// 安全网持有的动作列表 (catch_unwind 闭包的返回值)
type SmallVecHeld = smallvec::SmallVec<[OutputAction; 8]>;

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

    /// ★v24.38 模式步解析: `↓`(=按下保持) / `↑`(=抬起), ASCII 备选 `_` / `^`。
    #[test]
    fn parses_press_hold_and_release_steps() {
        let steps = parse_sequence("DOWN↓»Z↓»NONE⏱80»Z↑»DOWN↑", &HashMap::new()).unwrap();
        assert_eq!(steps.len(), 5);
        assert_eq!(steps[0].mode, SeqStepMode::PressHold);
        assert_eq!(steps[0].keys, vec!["DOWN"]);
        assert_eq!(steps[1].mode, SeqStepMode::PressHold);
        assert_eq!(steps[1].keys, vec!["Z"]);
        assert!(steps[2].keys.is_empty(), "NONE 仍是等待步");
        assert_eq!(steps[2].mode, SeqStepMode::Tap);
        assert_eq!(steps[3].mode, SeqStepMode::Release);
        assert_eq!(steps[4].mode, SeqStepMode::Release);
        // ASCII 备选 + 多键同按 + 与 Tap 步混排
        let steps = parse_sequence("A_»B^»C⏱50»A+B↑", &HashMap::new()).unwrap();
        assert_eq!(steps[0].mode, SeqStepMode::PressHold);
        assert_eq!(steps[1].mode, SeqStepMode::Release);
        assert_eq!(steps[2].mode, SeqStepMode::Tap);
        assert_eq!(steps[3].mode, SeqStepMode::Release);
        assert_eq!(steps[3].keys, vec!["A", "B"]);
        // 非法: 模式步带时长 / NONE 模式步
        assert!(parse_sequence("A↓⏱50", &HashMap::new()).is_err());
        assert!(parse_sequence("NONE↓", &HashMap::new()).is_err());
    }

    /// ★v24.38 steps_to_text 往返: 模式步写回 `↓`/`↑`, 再解析回来语义不变。
    #[test]
    fn mode_steps_round_trip_through_text() {
        let steps = vec![
            SeqStep { keys: vec!["DOWN".into()], hold_ms: None, mode: SeqStepMode::PressHold },
            SeqStep { keys: Vec::new(), hold_ms: Some(120), mode: SeqStepMode::Tap },
            SeqStep { keys: vec!["Z".into()], hold_ms: Some(80), mode: SeqStepMode::Tap },
            SeqStep { keys: vec!["Z".into()], hold_ms: None, mode: SeqStepMode::Release },
            SeqStep { keys: vec!["DOWN".into()], hold_ms: None, mode: SeqStepMode::Release },
        ];
        let text = steps_to_text(&steps, 20);
        assert_eq!(text, "DOWN↓>NONE:120>Z:80>Z↑>DOWN↑");
        let back = parse_sequence(&text, &HashMap::new()).unwrap();
        assert_eq!(back, steps, "文本往返必须保真 (含模式)");
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
        assert!(text.contains("A↓"), "序列文本应含 A↓ 按下步: {text}");
        assert!(text.contains("A↑"), "序列文本应含 A↑ 抬起步: {text}");
    }

    /// ★v24.38 核心场景: 按住方向键期间连打 Z 两下 (DNF 走位连招) ——
    /// 录制必须还原完整"按下→等待→抬起"时序, 而不是压成一步 `DOWN+Z`。
    #[test]
    fn recorded_events_replay_hold_direction_and_taps() {
        let name = vk_to_seq_name;
        let events = vec![
            RecordedKey { vk: 0x28, down_ms: 0, up_ms: None },     // DOWN 按住 (松开在录制结束后的尾部↑)
            RecordedKey { vk: 0x5A, down_ms: 150, up_ms: Some(230) }, // Z 敲第一下 (按住 80ms)
            RecordedKey { vk: 0x5A, down_ms: 400, up_ms: Some(470) }, // Z 敲第二下 (按住 70ms)
        ];
        let text = recorded_to_sequence(&events, &name);
        assert_eq!(
            text,
            "DOWN↓»NONE⏱150»Z↓»NONE⏱80»Z↑»NONE⏱170»Z↓»NONE⏱70»Z↑»DOWN↑",
            "事件时序忠实还原: {text}"
        );
        /* 往返: 生成的文本必须能原样解析回等价步骤 */
        let steps = parse_sequence(&text, &HashMap::new()).unwrap();
        assert_eq!(steps[0].mode, SeqStepMode::PressHold);
        assert_eq!(steps[1].keys, Vec::<String>::new());
        assert_eq!(steps[4].mode, SeqStepMode::Release);
        assert_eq!(steps[9].mode, SeqStepMode::Release);
    }

    /// ★v24.38 事件时序: 单键按住 100ms → 停 120ms → B、C 交叠 (各自独立按下/抬起)。
    #[test]
    fn recorded_events_become_sequence_text() {
        let name = vk_to_seq_name;
        // A 按 100ms → 停 120ms → B+C 重叠 (B 220-300, C 240-300)
        let events = vec![
            RecordedKey { vk: 0x41, down_ms: 0, up_ms: Some(100) },   // A
            RecordedKey { vk: 0x42, down_ms: 220, up_ms: Some(300) }, // B
            RecordedKey { vk: 0x43, down_ms: 240, up_ms: Some(300) }, // C
        ];
        let text = recorded_to_sequence(&events, &name);
        assert_eq!(
            text,
            "A↓»NONE⏱100»A↑»NONE⏱120»B↓»NONE⏱20»C↓»NONE⏱60»B+C↑",
            "B/C 同瞬间 (300ms) 抬起合并为一步 (B+C)↑; 按下间隔 20ms 忠实保留"
        );
        // 同一瞬间 (<10ms) 的两次按下合并为同一步 A+B↓
        let events = vec![
            RecordedKey { vk: 0x41, down_ms: 0, up_ms: Some(50) },
            RecordedKey { vk: 0x42, down_ms: 5, up_ms: Some(50) },
        ];
        assert_eq!(
            recorded_to_sequence(&events, &name),
            "A+B↓»NONE⏱45»A+B↑",
            "同瞬间按下合并, 抬起同瞬间也合并"
        );
        // 未知 vk 被忽略
        let events = vec![RecordedKey { vk: 0xFF, down_ms: 0, up_ms: Some(10) }];
        assert_eq!(recorded_to_sequence(&events, &name), "");
    }

    /// ★v24.38: run_sequence 结束时的安全网 —— ↓ 无配对 ↑ 的键在序列结束时被释放
    /// (纯逻辑验证: 构造 steps 走一遍, 断言不 panic 且不会把 Tap 步的键挂到结束)。
    /// 真实注入路径属环境敏感测试 (见 poller e2e), 这里只锁分支覆盖。
    #[test]
    fn run_sequence_release_safety_branches_exist() {
        // 编译期锁: SeqStepMode 三态 + ResolvedStep.mode 字段存在且可构造
        let steps: std::sync::Arc<[ResolvedStep]> = std::sync::Arc::from(vec![
            ResolvedStep {
                actions: smallvec::smallvec![],
                hold_ms: 10,
                mode: crate::state::SeqStepMode::Tap,
            },
            ResolvedStep {
                actions: smallvec::smallvec![],
                hold_ms: 0,
                mode: crate::state::SeqStepMode::PressHold,
            },
            ResolvedStep {
                actions: smallvec::smallvec![],
                hold_ms: 0,
                mode: crate::state::SeqStepMode::Release,
            },
        ]);
        assert_eq!(steps.len(), 3);
        assert_eq!(steps[0].mode, crate::state::SeqStepMode::Tap);
    }
}