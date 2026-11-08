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
///    两条组合同时命中且互不遮蔽, 行为不可预测 (完整包含/被包含是安全的: 大组合独占)。
///
/// ★v24.49 放宽: 与切换键共用按键**不再拒绝** —— 运行时已能仲裁
/// (切换键命中若同时补全某组合, 则组合优先、本次不切; 只有单独按切换键才切)。
/// 因此「LB 既当切换键、又作 LB+X 成员」成为合法配置。
///
/// 成员键与槽位单键映射的关系同样**无需**在此校验 —— 运行时延迟抑制保证组合完成时
/// 成员的单独功能不先触发; 若用户显式勾选「独占」(chord_exclusive_keys), 则成员单键
/// 整份让位给组合 (永不补发), 这是"牺牲一个键换多个组合键"的用法。
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
    Ok(())
}

/// ★v24.49 组合键与切换键共用按键时的**提示** (不再拒绝, 只告知运行时的仲裁语义)。
/// 返回空 `Vec` = 无交集; 否则每项一句话, 供保存后 toast 使用。
pub fn chord_switch_overlap_notes(config: &AppConfig, vid: u16, ids: &[u32]) -> Vec<String> {
    let mut notes = Vec::new();
    if let Some((vid2, ids2)) = xinput_ids_of_any(&config.switch_key)
        && vid2 == vid
        && let Some(shared) = ids2.iter().find(|id| ids.contains(id))
    {
        notes.push(format!(
            "与「连发切换键」共用 {} — 单独按它才切换, 按组合只触发组合",
            XInputHandler::input_id_to_name(*shared)
        ));
    }
    for p in &config.presets {
        if let Some((vid2, ids2)) = xinput_ids_of_any(&p.switch_key)
            && vid2 == vid
            && let Some(shared) = ids2.iter().find(|id| ids.contains(id))
        {
            notes.push(format!(
                "与预设「{}」的切换键共用 {} — 单独按它才切换, 按组合只触发组合",
                p.name,
                XInputHandler::input_id_to_name(*shared)
            ));
        }
    }
    notes
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

/// ★v24.49 可勾「独占」的按钮名 (短名, 不带设备前缀, 已排序去重)。
///
/// 判据 = 既出现在某**多键组合**里、又**有自己的单键映射**的按钮。
/// 只有这种按键才谈得上"牺牲单键换组合"; 纯组合成员 (本来就没单键功能) 勾了无意义,
/// 纯单键 (不参与任何组合) 勾了会白丢功能 —— 两者都不列出。
pub fn chord_solo_member_names_by_vid(config: &AppConfig) -> Vec<String> {
    let mut combo_members: Vec<u32> = Vec::new();
    let mut solo_keys: Vec<u32> = Vec::new();
    for m in &config.mappings {
        let Some(InputDevice::XInputCombo { button_ids, .. }) =
            AppState::input_name_to_device(&m.trigger_key)
        else {
            continue;
        };
        if button_ids.len() >= 2 {
            for &id in &button_ids {
                if !combo_members.contains(&id) {
                    combo_members.push(id);
                }
            }
        } else if let Some(&id) = button_ids.first()
            && !solo_keys.contains(&id)
        {
            solo_keys.push(id);
        }
    }
    let mut out: Vec<String> = combo_members
        .iter()
        .filter(|id| solo_keys.contains(id))
        .map(|&id| XInputHandler::input_id_to_name(id).to_string())
        .collect();
    out.sort_unstable();
    out.dedup();
    out
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
        /* ★v24.49: 与切换键共用按键不再是错误, 但要让用户知道运行时怎么仲裁 */
        let switch_notes = chord_switch_overlap_notes(&self.config, vid, &ids);
        let mut extra = String::new();
        if !switch_notes.is_empty() {
            extra.push_str(&format!("。{}", switch_notes.join("; ")));
        }
        if solo_names.is_empty() {
            self.set_gamepad_toast(format!("组合键 {display} → {value} 已保存{extra}"));
        } else {
            self.set_gamepad_toast(format!(
                "组合键 {display} → {value} 已保存。{} 已有单独映射: 单独按住约 0.15 秒后会触发其单独功能、点按也照常; 想让位给组合请勾选「独占」{extra}",
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
                             按组合时不会带出成员的单独功能; 想让某个键彻底让位给组合, \
                             可在下方勾选「独占」。",
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
                        ui.add_space(4.0);
                        /* ★v24.49 独占成员键: 把"组合成员的单独功能"整份让位给组合。
                         * 只列出**确实同时有单键映射与组合映射**的按钮 —— 其它键勾了也没意义。 */
                        self.render_chord_exclusive_section(ui, th);
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
    /// ★v24.49 独占成员键勾选区。
    ///
    /// 只列出「既出现在某多键组合里、又有自己的单键映射」的按钮 —— 勾上 = 该键的单键功能
    /// 整份让位给组合 (单独按住也不触发自己的映射), 用于"牺牲一个键换多个组合键"。
    /// 例: 牺牲 LB 单键 → LB+X / LB+Y / LB+A / LB+B 各映射一个键盘键。
    fn render_chord_exclusive_section(&mut self, ui: &mut egui::Ui, th: &Theme) {
        let mut candidates = chord_solo_member_names_by_vid(&self.config);
        /* ★v24.49: 把"已勾选但已不再是候选"的残留项也列出来 ——
         * 例如勾了 LB 之后删掉了 LB+X 组合, LB 就不再满足"组合成员∩单键"而消失;
         * 若此时不显示, 用户无法取消这项配置 (虽因交集为空而不生效, 但会一直残留)。 */
        let mut stale: Vec<String> = self
            .config
            .chord_exclusive_keys
            .iter()
            .filter(|k| {
                !candidates
                    .iter()
                    .any(|c| c.eq_ignore_ascii_case(k))
            })
            .cloned()
            .collect();
        if candidates.is_empty() && stale.is_empty() {
            return;
        }
        stale.sort_unstable();
        let active = candidates.clone();
        candidates.extend(stale);
        ui.label(th.hint_text(
            "独占: 勾选的键放弃自己的单键功能, 只作为组合成员使用 —— \
             牺牲 1 个键可换来「该键 + 其它键」的多个组合键。",
        ));
        ui.add_space(2.0);
        let mut changed = false;
        for name in &candidates {
            let on = self
                .config
                .chord_exclusive_keys
                .iter()
                .any(|k| k.eq_ignore_ascii_case(name));
            let mut checked = on;
            /* 残留项标注"当前没有用到该键的组合"——运行时因交集为空而不生效 */
            let is_active = active.iter().any(|c| c.eq_ignore_ascii_case(name));
            let label = if is_active {
                format!("{name} 单独按下时让位给组合")
            } else {
                format!("{name} 单独按下时让位给组合 (当前没有用到该键的组合)")
            };
            if ui
                .checkbox(&mut checked, label)
                .on_hover_text(
                    "勾选后该键的单独映射不再触发 (即使按住也不触发); \
                     不勾则单独按住约 0.15 秒后仍会触发其单独功能",
                )
                .changed()
            {
                if checked {
                    if !on {
                        self.config.chord_exclusive_keys.push(name.clone());
                    }
                } else {
                    self.config
                        .chord_exclusive_keys
                        .retain(|k| !k.eq_ignore_ascii_case(name));
                }
                changed = true;
            }
        }
        if changed {
            self.gp_save_and_reload();
            self.set_gamepad_toast("独占设置已保存");
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

    /// ★v24.49: 与切换键共用按键**不再拒绝** (运行时仲裁: 单独按切换键才切,
    /// 按组合只触发组合)。这里锁新语义 + 提示文案。
    #[test]
    fn test_validate_chord_allows_switch_key_overlap() {
        let mut c = config_with(&[]);
        c.switch_key = "GAMEPAD_045E_BACK".to_string();
        /* 与连发切换键共享 BACK → 允许, 但给出提示 */
        assert!(
            validate_chord_trigger(&c, 0x045E, &[BACK, RB]).is_ok(),
            "v24.49 起切换键可与组合共用 (运行时仲裁)"
        );
        let notes = chord_switch_overlap_notes(&c, 0x045E, &[BACK, RB]);
        assert_eq!(notes.len(), 1, "应提示与连发切换键共用: {notes:?}");
        assert!(notes[0].contains("连发切换键"), "{notes:?}");
        /* 不共享 → 无提示 */
        assert!(validate_chord_trigger(&c, 0x045E, &[LB, RB]).is_ok());
        assert!(chord_switch_overlap_notes(&c, 0x045E, &[LB, RB]).is_empty());
        /* 预设切换键: 同样是"允许 + 提示" */
        let mut c2 = config_with(&[]);
        c2.presets.push(crate::config::Preset {
            name: "预设1".to_string(),
            mappings: Vec::new(),
            switch_key: "GAMEPAD_045E_START".to_string(),
        });
        assert!(validate_chord_trigger(&c2, 0x045E, &[START, RB]).is_ok());
        let notes2 = chord_switch_overlap_notes(&c2, 0x045E, &[START, RB]);
        assert_eq!(notes2.len(), 1, "{notes2:?}");
        assert!(notes2[0].contains("预设1"), "{notes2:?}");
    }

    #[test]
    fn test_chord_solo_member_names() {
        let c = config_with(&["GAMEPAD_045E_X", "GAMEPAD_045E_RB+X", "GAMEPAD_045E_A"]);
        let names = chord_solo_member_names(&c, 0x045E, &[RB, X]);
        assert_eq!(names, vec!["X"], "X 有单独映射, RB 没有");
    }

    /// ★v24.49: 可勾「独占」的键 = 「多键组合成员」∩「有自己的单键映射」。
    /// 纯组合成员 (无单键功能) 与纯单键 (不参与组合) 都不列出。
    #[test]
    fn test_chord_solo_member_names_by_vid() {
        let c = config_with(&[
            "GAMEPAD_045E_LB",     // LB 单键
            "GAMEPAD_045E_LB+X",   // LB 也是组合成员 → LB 可勾独占
            "GAMEPAD_045E_LB+Y",   // 同一键的第二个组合
            "GAMEPAD_045E_A",      // A 只有单键, 不参与任何组合 → 不列出
            "GAMEPAD_045E_RB+B",   // RB/B 无单键映射 → 不列出
        ]);
        let names = chord_solo_member_names_by_vid(&c);
        assert_eq!(names, vec!["LB"], "只有 LB 既在组合里又有单键映射: {names:?}");
    }

    /// ★v24.49: 没有"组合成员兼单键"时, 独占区不出现 (避免无意义勾选)
    #[test]
    fn test_chord_solo_member_names_by_vid_empty_when_no_overlap() {
        let c = config_with(&["GAMEPAD_045E_A", "GAMEPAD_045E_LB+X"]);
        assert!(
            chord_solo_member_names_by_vid(&c).is_empty(),
            "A 是纯单键、LB/X 是纯组合成员, 无可独占项"
        );
    }

    /// ★v24.49: 独占配置残留的判定 —— 勾了 LB 之后删掉全部 LB 组合,
    /// LB 就不再是候选, 但配置里仍留着。GUI 必须能把它列出来供取消。
    #[test]
    fn test_chord_exclusive_stale_entries_are_listed() {
        let mut c = config_with(&["GAMEPAD_045E_LB", "GAMEPAD_045E_LB+X"]);
        c.chord_exclusive_keys = vec!["LB".to_string()];
        /* 有 LB+X 时 LB 是有效候选 */
        assert_eq!(chord_solo_member_names_by_vid(&c), vec!["LB"]);

        /* 删掉组合后: 候选空, 但配置仍残留 → GUI 需列出 (由调用方合并 stale) */
        c.mappings.retain(|m| m.trigger_key != "GAMEPAD_045E_LB+X");
        assert!(
            chord_solo_member_names_by_vid(&c).is_empty(),
            "没有组合后 LB 不再是候选"
        );
        assert_eq!(
            c.chord_exclusive_keys,
            vec!["LB".to_string()],
            "配置项仍在 (GUI 需据此显示残留项让用户取消)"
        );
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
