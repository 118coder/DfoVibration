//! 卡键类回归测试。
//!
//! 背景: 暂停/白名单/IME/注入冷却闸门曾把"抬起"事件一并吞掉——
//! Released 到不了 worker, 注入键永久悬空 (卡键), 连发映射永不停。
//! 修复语义: 闸门只拦"按下"; "抬起"始终派发且保持透传 (不拦截真实抬起)。

use smallvec::SmallVec;
use sorahk::config::{AppConfig, KeyMapping};
use sorahk::state::{AppState, InputDevice, InputEvent};
use std::sync::{Arc, Mutex};

/// 记录派发事件的 mock dispatcher (不经过 worker/ SendInput, 测试安全)
#[derive(Default)]
struct RecordingDispatcher(Mutex<Vec<String>>);

impl sorahk::state::EventDispatcher for RecordingDispatcher {
    fn dispatch(&self, event: InputEvent) {
        self.0.lock().unwrap().push(format!("{event:?}"));
    }
    fn clear_cache(&self) {}
}

impl RecordingDispatcher {
    fn count(&self, kind: &str, needle: &str) -> usize {
        self.0
            .lock()
            .unwrap()
            .iter()
            .filter(|e| e.starts_with(kind) && e.contains(needle))
            .count()
    }
}

fn mapping(trigger: &str, target: &str) -> KeyMapping {
    KeyMapping {
        trigger_key: trigger.to_string(),
        target_keys: SmallVec::from_vec(vec![target.to_string()]),
        interval: Some(10),
        event_duration: Some(5),
        turbo_enabled: false,
        move_speed: 10,
        double_tap_enabled: false,
        double_tap_gap_ms: 50,
        note: String::new(),
    }
}

#[test]
fn keyup_reaches_worker_while_paused() {
    let mut config = AppConfig::default();
    config.mappings = vec![mapping("A", "B")];
    let state = Arc::new(AppState::new(config).unwrap());
    let rec = Arc::new(RecordingDispatcher::default());
    state.set_worker_pool(rec.clone());

    const WM_KEYDOWN: u32 = 0x0100;
    const WM_KEYUP: u32 = 0x0101;

    // 正常按下 → Pressed 派发且拦截
    let blocked = state.handle_key_event(WM_KEYDOWN, 0x41);
    assert!(blocked, "映射键的按下应被拦截");
    assert_eq!(rec.count("Pressed", "Keyboard"), 1);

    // 暂停后抬起 → Released 必须仍派发 (旧行为: 被闸门吞掉 → 注入键悬空)
    state.toggle_paused();
    let blocked_up = state.handle_key_event(WM_KEYUP, 0x41);
    assert_eq!(
        rec.count("Released", "Keyboard"),
        1,
        "暂停期间的抬起事件必须派发 Released, 否则注入键永久悬空卡键"
    );
    assert!(
        !blocked_up,
        "暂停期间的抬起必须透传: 按下当时是透传的, 拦截抬起会让游戏侧卡键"
    );
}

#[test]
fn mouse_button_up_reaches_worker_while_paused() {
    let mut config = AppConfig::default();
    config.mappings = vec![mapping("LBUTTON", "F1")];
    let state = Arc::new(AppState::new(config).unwrap());
    let rec = Arc::new(RecordingDispatcher::default());
    state.set_worker_pool(rec.clone());

    const WM_LBUTTONDOWN: u32 = 0x0201;
    const WM_LBUTTONUP: u32 = 0x0202;

    let blocked = state.handle_mouse_event(WM_LBUTTONDOWN, 0);
    assert!(blocked, "映射鼠标键的按下应被拦截");
    assert_eq!(rec.count("Pressed", "Mouse"), 1);

    state.toggle_paused();
    let blocked_up = state.handle_mouse_event(WM_LBUTTONUP, 0);
    assert_eq!(
        rec.count("Released", "Mouse"),
        1,
        "暂停期间的鼠标抬起必须派发 Released"
    );
    assert!(!blocked_up, "鼠标抬起按既有设计始终不拦截");
}

#[test]
fn keyup_while_paused_without_mapping_is_transparent() {
    // 未映射键的抬起在任何状态下都不应产生派发 (无映射 → 无注入 → 无需回收)
    let config = AppConfig::default();
    let state = Arc::new(AppState::new(config).unwrap());
    let rec = Arc::new(RecordingDispatcher::default());
    state.set_worker_pool(rec.clone());

    const WM_KEYUP: u32 = 0x0101;
    state.toggle_paused();
    // 注意: AppConfig::default() 自带 Q→Q 映射, 这里用真正未映射的 R (0x52)
    let blocked = state.handle_key_event(WM_KEYUP, 0x52);
    assert_eq!(rec.count("Released", "Keyboard"), 0);
    assert!(!blocked);
}
