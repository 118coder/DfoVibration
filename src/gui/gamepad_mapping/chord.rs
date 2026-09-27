//! ★v24.37 手柄组合键 (chord) —— 多键组合 (如 RB+X) 映射为键盘/鼠标目标。
//!
//! 数据层**零新概念**: 组合键 = 一条普通 `KeyMapping`, 触发键为多键 XInput 名
//! (`GAMEPAD_045E_RB+X`)。运行时侧 XInput 通道本来就支持多键位图匹配 +
//! 大组合独占仲裁 (见 `xinput.rs::handle_normal_mode_xinput`), v24.37 又加了
//! 「成员键单独按下的延迟抑制」—— 组合完成时不带出成员的单独功能。
//!
//! 本模块只做三件事: ① 组合捕获 (`pick_chord_device`, 要求 ≥2 键同按);
//! ② 建键时冲突校验 (重复 / 组合间部分重叠 / 与切换键相交);
//! ③ 面板渲染 (列表 + 增删改目标)。

use crate::config::{AppConfig, KeyMapping};
use crate::gui::SorahkGui;
use crate::gui::theme::Theme;
use crate::gui::types::{GpEvent, GpFlow};
use crate::state::{AppState, DeviceType, InputDevice};
use crate::xinput::XInputHandler;
use eframe::egui;
use super::*;

/* ============================ 纯函数助手 (可单测) ============================ */

/// 触发键的 XInput 组合部分 (任意键数, 含单键): 返回 `(vid, 排序后的按钮 id)`。
/// 用于切换键交集校验 (切换键通常是单键); 组合键自身判定用 [`chord_ids_of`]。
fn xinput_ids_of_any(trigger: &str) -> Option<(u16, Vec<u32>)> {
    match AppState::input_name_to_device(trigger) {
        Some(InputDevice::XInputCombo {
            device_type: DeviceType::Gamepad(vid),
            button_ids,
        })
        | Some(InputDevice::XInputCombo {
            device_type: DeviceType::Joystick(vid),
            button_ids,
        }) => Some((vid, button_ids)),
        _ => None,
    }
}

/// 触发键的 XInput **多键**组合部分: 返回 `(vid, 排序后的按钮 id)`; 非多键组合 → None。
fn chord_ids_of(trigger: &str) -> Option<(u16, Vec<u32>)> {
    match xinput_ids_of_any(trigger) {
        Some((vid, ids)) if ids.len() >= 2 => Some((vid, ids)),
        _ => None,
    }
}

/// 组合键触发名 (规范形态): `GAMEPAD_{vid:04X}_RB+X` (ids 升序去重)。
/// `normalize_key_combo` 与 `parse_xinput_combo` 对该形态双向兼容。
pub fn chord_trigger_name(vid: u16, ids: &[u32]) -> String {
    let mut ids = ids.to_vec();
    ids.sort_unstable();
    ids.dedup();
    let names: Vec<&str> = ids
        .iter()
        .map(|&id| XInputHandler::input_id_to_name(id))
        .collect();
    format!("GAMEPAD_{vid:04X}_{}", names.join("+"))
}

/// 组合键短显示 (不带设备前缀): `RB+X`。
pub fn chord_display(ids: &[u32]) -> String {
    let mut ids = ids.to_vec();
    ids.sort_unstable();
    ids.dedup();
    ids.iter()
        .map(|&id| XInputHandler::input_id_to_name(id))
        .collect::<Vec<&str>>()
        .join("+")
}

/// 列出组合键映射 (触发键解析为 ≥2 键 XInput 组合) 在 config.mappings 里的下标。
pub fn chord_mapping_indexes(config: &AppConfig) -> Vec<usize> {
    config
        .mappings
        .iter()
        .enumerate()
        .filter(|(_, m)| chord_ids_of(&m.trigger_key).is_some())
        .map(|(i, _)| i)
        .collect()
}

/// 组合键冲突校验 (建键时护栏, "无冲突"的静态一半; 动态抑制在运行时):
/// ① 与既有组合键**完全相同** → 拒绝 (重复);
/// ② 与既有组合键**部分重叠** (共享 ≥1 键且互不包含) → 拒绝 —— 同按两组件的按键时
///    两条组合同时命中且互不遮蔽, 行为不可预测 (完整包含/被包含是安全的: 大组合独占);
/// ③ 与「连发切换键」/预设切换键**共享任一键** → 拒绝 (按组合会误触切换)。
/// 成员键与槽位单键映射的关系**无需**在此校验 —— 运行时延迟抑制已保证组合完成时
/// 成员的单独功能不先触发。
pub fn validate_chord_trigger(config: &AppConfig, vid: u16, ids: &[u32]) -> Result<(), String> {
    let norm = chord_trigger_name(vid, ids);
    for i in chord_mapping_indexes(config) {
        let m = &config.mappings[i];
        if m.trigger_key.eq_ignore_ascii_case(&norm) {
            return Err(format!("组合键 {} 已存在", chord_display(ids)));
        }
        if let Some((vid2, ids2)) = chord_ids_of(&m.trigger_key)
            && vid2 == vid
        {
            let shared = ids2.iter().filter(|id| ids.contains(id)).count();
            if shared == 0 {
                continue;
            }
            let subset = shared == ids.len().min(ids2.len());
            if !subset {
                return Err(format!(
                    "与既有组合键 {} 重叠 ({shared} 个共用按键) —— 组合键之间不能部分共用按键, 请换一组",
                    chord_display(&ids2)
                ));
            }
        }
    }
    /* 连发切换键: 手柄命名 (任意键数) 时按共享键拦截 */
    if let Some((vid2, ids2)) = xinput_ids_of_any(&config.switch_key)
        && vid2 == vid
        && ids2.iter().any(|id| ids.contains(id))
    {
        return Err("与「连发切换键」共用按键 — 按组合键会误触暂停/恢复, 请换一组".to_string());
    }
    /* 预设切换键 */
    for p in &config.presets {
        if let Some((vid2, ids2)) = xinput_ids_of_any(&p.switch_key)
            && vid2 == vid
            && ids2.iter().any(|id| ids.contains(id))
        {
            return Err(format!(
                "与预设「{}」的切换键共用按键 — 按组合键会误触切换, 请换一组",
                p.name
            ));
        }
    }
    Ok(())
}

/// 组合成员中已具单独映射的按键显示名 (保存后提示"延迟触发"语义用)。
pub fn chord_solo_member_names(config: &AppConfig, vid: u16, ids: &[u32]) -> Vec<String> {
    let mut solo_names = Vec::new();
    for m in &config.mappings {
        let Some(InputDevice::XInputCombo {
            device_type: DeviceType::Gamepad(vid2),
            button_ids,
        }) = AppState::input_name_to_device(&m.trigger_key)
        else {
            continue;
        };
        if button_ids.len() != 1 || vid2 != vid || !ids.contains(&button_ids[0]) {
            continue;
        }
        let name = chord_display(&button_ids);
        if !solo_names.contains(&name) {
            solo_names.push(name);
        }
    }
    solo_names
}

/* ================================ 面板渲染 ================================ */

impl SorahkGui {
    /// 开始组合键录入 (面板「＋ 添加组合键」)。
    pub(super) fn start_chord_capture(&mut self) {
        self.gp_flow = self.gp_flow.transition(GpEvent::BeginChord);
        if matches!(self.gp_flow, GpFlow::AwaitChordPad) {
            self.capture_pressed_keys.clear();
            self.gp_pad_capture.reset();
            self.gp_quick_listen = false;
            self.app_state.set_raw_input_capture_mode(true);
            /* ★v24.39: 组合捕获用「输入数最多」帧选择 (LB+LT 才能整帧捕获) */
            self.app_state
                .capture_prefers_max_inputs
                .store(true, std::sync::atomic::Ordering::Relaxed);
        }
    }

    /// 确认写入组合键 (新建或改既有目标)。
    fn confirm_chord(&mut self) {
        let GpFlow::ConfirmChord { vid, ids, value } = self.gp_flow.clone() else {
            return;
        };
        let trigger = chord_trigger_name(vid, &ids);
        let display = chord_display(&ids);
        /* 已有同触发键映射 = 「改目标」语义 → 整份替换; 否则新建 */
        let existing = self
            .config
            .mappings
            .iter()
            .position(|m| m.trigger_key.eq_ignore_ascii_case(&trigger));
        let solo_names;
        match existing {
            Some(i) => {
                self.config.mappings[i].clear_target_keys();
                self.config.mappings[i].add_target_key(value.clone());
                solo_names = Vec::new();
            }
            None => {
                /* 二次校验 (捕获期间配置可能变化) */
                if let Err(msg) = validate_chord_trigger(&self.config, vid, &ids) {
                    self.set_gamepad_warn(msg);
                    return;
                }
                self.config.mappings.push(KeyMapping {
                    release_targets: Default::default(),
                    sequence_text: String::new(),
                    trigger_key: trigger.clone(),
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
                    note: "手柄组合键（系统）".to_string(),
                });
                let last = self.config.mappings.len() - 1;
                self.config.mappings[last].add_target_key(value.clone());
                solo_names = chord_solo_member_names(&self.config, vid, &ids);
            }
        }
        self.gp_save_and_reload();
        self.gp_flow = self.gp_flow.transition(GpEvent::FinishEdit);
        if solo_names.is_empty() {
            self.set_gamepad_toast(format!("组合键 {display} → {value} 已保存"));
        } else {
            self.set_gamepad_toast(format!(
                "组合键 {display} → {value} 已保存。{} 已有单独映射: 单独按住约 0.15 秒后触发其单独功能, 快速点按与组合不受影响",
                solo_names.join("、")
            ));
        }
    }

    /// 删除组合键映射。
    fn delete_chord(&mut self, mapping_idx: usize) {
        if mapping_idx < self.config.mappings.len() {
            let display = chord_display(
                &chord_ids_of(&self.config.mappings[mapping_idx].trigger_key)
                    .map(|(_, ids)| ids)
                    .unwrap_or_default(),
            );
            self.config.mappings.remove(mapping_idx);
            self.gp_save_and_reload();
            self.set_gamepad_toast(format!("组合键 {display} 已删除"));
        }
    }

    /// 对既有组合键「改目标」→ 进入键盘捕获 (沿用组合, 只换目标)。
    fn begin_chord_target_edit(&mut self, mapping_idx: usize) {
        let Some((vid, ids)) = chord_ids_of(&self.config.mappings[mapping_idx].trigger_key) else {
            return;
        };
        self.gp_flow = GpFlow::AwaitChordKb {
            vid,
            ids,
            keys: Vec::new(),
        };
    }

    /// 手柄组合键面板 (页面主体在总览卡之后调用)。
    pub(in crate::gui) fn render_gamepad_chord_panel(&mut self, ui: &mut egui::Ui, th: &Theme) {
        match self.gp_flow.clone() {
            GpFlow::AwaitChordPad => {
                th.card(ui, None, |ui| {
                    ui.horizontal(|ui| {
                        th.badge(ui, "手柄组合键", th.accent_text, th.accent_soft);
                        ui.add_space(6.0);
                        ui.label(th.hint_text("第 1/2 步 · 按组合"));
                    });
                    ui.add_space(8.0);
                    ui.label(
                        egui::RichText::new("请同时按住 2 个及以上手柄按键, 然后一起松开")
                            .size(16.0)
                            .strong()
                            .color(th.accent),
                    );
                    ui.add_space(4.0);
                    ui.label(th.hint_text(
                        "例如按住 RB+X 再松开 = 记录组合 RB+X。\n摇杆方向也能作为组合成员 (如 LT+RS_Up)。",
                    ));
                    ui.add_space(6.0);
                    if ui.add(th.ghost_button("取消")).clicked() {
                        self.cancel_gp();
                    }
                });
            }
            GpFlow::AwaitChordKb { vid, ids, keys } => {
                th.card(ui, None, |ui| {
                    ui.horizontal(|ui| {
                        th.badge(ui, "手柄组合键", th.accent_text, th.accent_soft);
                        ui.add_space(6.0);
                        ui.label(th.hint_text("第 2/2 步 · 设目标键"));
                    });
                    ui.add_space(8.0);
                    ui.label(
                        egui::RichText::new(format!(
                            "组合 {} 已记录。按下要触发的**键盘键** (可组合键)",
                            chord_display(&ids)
                        ))
                        .size(16.0)
                        .strong()
                        .color(th.accent),
                    );
                    if !keys.is_empty() {
                        ui.label(th.hint_text(format!("已捕获: {}", keys.join(" + "))));
                    }
                    ui.add_space(6.0);
                    if ui.add(th.ghost_button("取消")).clicked() {
                        self.cancel_gp();
                    }
                    let _ = vid;
                });
            }
            GpFlow::ConfirmChord { vid, ids, value } => {
                th.card(ui, None, |ui| {
                    ui.horizontal(|ui| {
                        th.badge(ui, "手柄组合键", th.accent_text, th.accent_soft);
                        ui.add_space(6.0);
                        ui.label(th.hint_text("确认写入"));
                    });
                    ui.add_space(8.0);
                    ui.label(
                        egui::RichText::new(format!("{} → {}", chord_display(&ids), value))
                            .size(17.0)
                            .strong()
                            .color(th.title),
                    );
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.add(th.primary_button("确认保存")).clicked() {
                            self.confirm_chord();
                        }
                        if ui.add(th.secondary_button("再加一个键")).clicked() {
                            self.gp_flow = self.gp_flow.transition(GpEvent::AddAnotherKey);
                        }
                        if ui.add(th.ghost_button("取消")).clicked() {
                            self.cancel_gp();
                        }
                    });
                    ui.add_space(4.0);
                    ui.label(th.hint_text(
                        "优先级: 组合 > 单键 —— 按住组合只触发组合, 不会带出成员的单独功能;
                         成员单独按住约 0.15 秒 (或快速点按) 照常触发自己的映射。",
                    ));
                    let _ = vid;
                });
            }
            /* 其余状态: 正常列表 */
            _ => {
                let indexes = chord_mapping_indexes(&self.config);
                let mut delete_idx: Option<usize> = None;
                let mut edit_idx: Option<usize> = None;
                let mut add_clicked = false;
                th.card(ui, None, |ui| {
                    ui.horizontal(|ui| {
                        th.badge(ui, "手柄组合键", th.accent_text, th.accent_soft);
                        ui.add_space(6.0);
                        ui.label(th.hint_text("多键组合 → 任意键盘/鼠标键 (如 RB+X = Space)"));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            add_clicked = ui.add(th.primary_button("＋ 添加组合键")).clicked();
                        });
                    });
                    ui.add_space(6.0);
                    if indexes.is_empty() {
                        ui.label(th.hint_text(
                            "还没有组合键。添加后按住组合 (如 RB+X) 即触发目标键;\n\
                             组合成员的单独映射不受影响 (引擎自动抑制, 组合不会带出成员功能)。",
                        ));
                    } else {
                        for &i in &indexes {
                            /* 先取显示数据再放借用 —— 行内要 &mut self 渲染序列编辑器 */
                            let (trig_display, has_seq, targets) = {
                                let m = &self.config.mappings[i];
                                (
                                    chord_ids_of(&m.trigger_key)
                                        .map(|(_, ids)| chord_display(&ids))
                                        .unwrap_or_else(|| m.trigger_key.clone()),
                                    !m.sequence_text.trim().is_empty(),
                                    m.get_target_keys().join(" + "),
                                )
                            };
                            ui.horizontal(|ui| {
                                ui.label(
                                    egui::RichText::new(format!("{trig_display} →"))
                                        .strong()
                                        .color(th.title),
                                );
                                /* ★v24.39: 已挂序列宏的组合键给 ⚡ 徽章 (一眼可见) */
                                if has_seq {
                                    th.badge(ui, "⚡宏", th.accent_text, th.accent_soft);
                                }
                                let targets_display = if targets.is_empty() {
                                    if has_seq { "(序列宏)".to_string() } else { "(无目标)".to_string() }
                                } else {
                                    targets
                                };
                                ui.label(
                                    egui::RichText::new(targets_display).color(th.accent),
                                );
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if ui.add(th.ghost_button("删除")).clicked() {
                                            delete_idx = Some(i);
                                        }
                                        if ui.add(th.secondary_button("改目标")).clicked() {
                                            edit_idx = Some(i);
                                        }
                                    },
                                );
                            });
                            ui.add_space(2.0);
                            /* ★v24.39: 组合键直接挂序列宏 —— 共享步骤编辑器
                             * (apply_sequence_text 即时落盘+热重载, 手柄页无保存按钮也能生效) */
                            egui::CollapsingHeader::new(
                                egui::RichText::new("☰ 序列宏 (连招)").size(12.0),
                            )
                            .id_salt(("gp_chord_seq", i))
                            .show(ui, |ui| {
                                ui.label(th.hint_text(
                                    "按下组合 = 自动执行这套按键时序 (序列优先于下方目标键)",
                                ));
                                self.render_sequence_steps_editor(ui, i, th);
                            });
                            ui.add_space(4.0);
                        }
                        ui.add_space(2.0);
                        ui.label(th.hint_text(
                            "规则: 组合键之间不能部分共用按键; 不能与切换键共用按键 (保存时校验)。",
                        ));
                    }
                });
                if add_clicked {
                    self.start_chord_capture();
                }
                if let Some(i) = edit_idx {
                    self.begin_chord_target_edit(i);
                }
                if let Some(i) = delete_idx {
                    self.delete_chord(i);
                }
            }
        }
    }
}

/* ================================== 单测 ================================== */
#[cfg(test)]
mod tests {
    use super::*;

    /* XInput 按钮 id (xinput.rs BUTTON_MAP): LB=0x09 RB=0x0A A=0x0B X=0x0D
     * Y=0x0E START=0x05 BACK=0x06 LT=0x18 RT=0x19 */
    const LB: u32 = 0x09;
    const RB: u32 = 0x0A;
    const X: u32 = 0x0D;
    const Y: u32 = 0x0E;
    const START: u32 = 0x05;
    const BACK: u32 = 0x06;

    fn config_with(triggers: &[&str]) -> AppConfig {
        let mut c = AppConfig::default();
        for t in triggers {
            c.mappings.push(KeyMapping {
                release_targets: Default::default(),
                sequence_text: String::new(),
                trigger_key: t.to_string(),
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
            });
        }
        c
    }

    #[test]
    fn test_chord_trigger_name_roundtrip() {
        let name = chord_trigger_name(0x045E, &[RB, LB]);
        assert_eq!(name, "GAMEPAD_045E_LB+RB", "ids 应排序去重");
        /* 规范名必须能被解析回同一组 id (与运行时同一解析路径) */
        let (vid, ids) = chord_ids_of(&name).expect("规范名应可解析");
        assert_eq!(vid, 0x045E);
        assert_eq!(ids, vec![LB, RB]);
        /* 顺序无关: 同一组 id 得到同一个名字 */
        assert_eq!(chord_trigger_name(0x045E, &[LB, RB]), name);
        /* RB+X (用户示例) */
        assert_eq!(chord_display(&[X, RB]), "RB+X");
    }

    #[test]
    fn test_chord_mapping_indexes_only_multi_button() {
        /* Appconfig::default() 自带 1 条 Q→Q 映射 —— 按触发键名定位期望, 不写死下标 */
        let c = config_with(&[
            "GAMEPAD_045E_A",    // 单键槽位映射 → 不算组合键
            "GAMEPAD_045E_RB+X", // 组合键
            "LCTRL+NUMPAD2",     // 键盘组合 → 不算
            "GAMEPAD_045E_Y+LT", // 组合键
        ]);
        let idx = chord_mapping_indexes(&c);
        let idx_of = |t: &str| {
            c.mappings
                .iter()
                .position(|m| m.trigger_key == t)
                .unwrap()
        };
        assert!(idx.contains(&idx_of("GAMEPAD_045E_RB+X")));
        assert!(idx.contains(&idx_of("GAMEPAD_045E_Y+LT")));
        assert!(!idx.contains(&idx_of("GAMEPAD_045E_A")));
        assert!(!idx.contains(&idx_of("LCTRL+NUMPAD2")));
        assert!(!idx.contains(&idx_of("Q")), "默认映射不是组合键");
    }

    #[test]
    fn test_validate_chord_duplicates_and_overlap() {
        let c = config_with(&["GAMEPAD_045E_RB+X", "GAMEPAD_045E_A+LB"]);
        /* 完全重复 (RB+X) → 拒绝 */
        assert!(validate_chord_trigger(&c, 0x045E, &[RB, X]).is_err());
        /* 部分重叠 (RB+Y 与 RB+X 共享 RB) → 拒绝 */
        assert!(
            validate_chord_trigger(&c, 0x045E, &[RB, Y]).is_err(),
            "RB+Y 与 RB+X 部分重叠应拒绝"
        );
        /* 完全包含 (RB+X+Y ⊃ RB+X) → 允许 (大组合独占, 语义确定) */
        assert!(validate_chord_trigger(&c, 0x045E, &[RB, X, Y]).is_ok());
        /* 与另一组合部分重叠 (X+LT 与 RB+X 共享 X) → 拒绝 */
        assert!(validate_chord_trigger(&c, 0x045E, &[X, 0x18]).is_err());
        /* 完全不相交 (LT+RT) → 允许 */
        assert!(validate_chord_trigger(&c, 0x045E, &[0x18, 0x19]).is_ok());
    }

    #[test]
    fn test_validate_chord_rejects_switch_key_overlap() {
        let mut c = config_with(&[]);
        c.switch_key = "GAMEPAD_045E_BACK".to_string();
        /* 与连发切换键共享 BACK → 拒绝 */
        assert!(validate_chord_trigger(&c, 0x045E, &[BACK, RB]).is_err());
        /* 不共享 → 允许 */
        assert!(validate_chord_trigger(&c, 0x045E, &[LB, RB]).is_ok());
        /* 预设切换键 */
        let mut c2 = config_with(&[]);
        c2.presets.push(crate::config::Preset {
            name: "预设1".to_string(),
            mappings: Vec::new(),
            switch_key: "GAMEPAD_045E_START".to_string(),
        });
        assert!(validate_chord_trigger(&c2, 0x045E, &[START, RB]).is_err());
    }

    #[test]
    fn test_chord_solo_member_names() {
        let c = config_with(&["GAMEPAD_045E_X", "GAMEPAD_045E_RB+X", "GAMEPAD_045E_A"]);
        let names = chord_solo_member_names(&c, 0x045E, &[RB, X]);
        assert_eq!(names, vec!["X"], "X 有单独映射, RB 没有");
    }

    /// ★v24.39: LB+LT (肩键+扳机, 纯按键组合) 必须能被组合捕获选中 ——
    /// 捕获帧选择在组合捕获期间强制 max-inputs (xinput 侧), 这里锁挑选器契约。
    #[test]
    fn test_pick_chord_device_accepts_shoulder_and_trigger() {
        use crate::state::{DeviceType, InputDevice};
        let cands = PadCaptureCandidates {
            raw: None,
            xinput: Some(InputDevice::XInputCombo {
                device_type: DeviceType::Gamepad(0x045E),
                button_ids: vec![0x18, 0x09], // LT + LB (乱序也要归一)
            }),
        };
        let (vid, ids) = pick_chord_device(&cands).expect("LB+LT 应可作为组合捕获");
        assert_eq!(vid, 0x045E);
        assert_eq!(ids, vec![LB, 0x18], "ids 排序去重");
        /* 规范名可被运行时同一路径解析回同一组 id */
        assert_eq!(
            chord_trigger_name(vid, &ids),
            "GAMEPAD_045E_LB+LT"
        );
    }

    /// ★v24.39: 槽位捕获遇到多键, 报错必须引导到组合键面板 (而不是只说重按)。
    #[test]
    fn test_slot_capture_multi_key_redirects_to_chord_panel() {
        use crate::state::{DeviceType, InputDevice};
        let cands = PadCaptureCandidates {
            raw: None,
            xinput: Some(InputDevice::XInputCombo {
                device_type: DeviceType::Gamepad(0x045E),
                button_ids: vec![0x09, 0x18],
            }),
        };
        let slot = get_slot(4).expect("LB 槽位存在");
        let err = pick_calibration_device(slot, &cands).unwrap_err();
        assert!(err.contains("手柄组合键"), "报错应引导到组合键面板: {err}");
        assert!(err.contains("优先级"), "报错应说明组合优先级: {err}");
    }
}
