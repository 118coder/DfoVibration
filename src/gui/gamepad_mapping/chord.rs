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

/// ★R2.12 组合键分组标签: 优先按 LT / RT / LB / RB 归组 (用户最常用的一族组合
/// 都挂在这些修饰键下), 没有肩键/扳机的组合归「其它组合」。
pub fn chord_group_label(ids: &[u32]) -> &'static str {
    for &id in ids {
        match id {
            0x18 => return "LT 组",
            0x19 => return "RT 组",
            0x09 => return "LB 组",
            0x0A => return "RB 组",
            _ => {}
        }
    }
    "其它组合"
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

/// 组合键冲突校验 (建键时护栏): **只拒绝完全重复**。
///
/// ★R2.12 关键修正: 旧版还拦「与既有组合部分重叠」, 结果
/// `LT+A / LT+B / LT+X / LT+Y` 这种共用同一个修饰键的一族组合只能建成第 1 条 ——
/// 用户反馈的「只能绑定少量组合键」根因就在这里 (见 交接_组合键彻查与优化清单_2026-11-11.md)。
/// 运行时 [`crate::xinput::arbitrate_matched_combos`] 已能裁决多组合同时命中的情况
/// (精确匹配 > 最长组合 > 确定性取一条), 校验层不再需要这条静态禁令。
///
/// 与切换键共用按键 (v24.49) 早就不拒绝; 成员键与单键映射的关系由运行时
/// 延迟抑制 / `chord_exclusive_keys` 独占处理, 也不在此校验。
pub fn validate_chord_trigger(config: &AppConfig, vid: u16, ids: &[u32]) -> Result<(), String> {
    let norm = chord_trigger_name(vid, ids);
    if chord_mapping_indexes(config)
        .iter()
        .any(|&i| config.mappings[i].trigger_key.eq_ignore_ascii_case(&norm))
    {
        let msg = format!("组合键 {} 已存在", chord_display(ids));
        crate::vibration::vib_log(&format!(
            "[chord] reject-duplicate | vid={vid:04X} | trigger={norm}"
        ));
        return Err(msg);
    }
    Ok(())
}

/// ★R2.12 加载期 / 预设切换期统一校验: 手改 Config.toml、导入配置、切预设都会绕过
/// GUI 建键时的 [`validate_chord_trigger`], 这里补一次只读体检。
/// 返回可直接展示给用户的中文问题列表 (空 = 没问题), **不修改也不丢弃任何配置**。
pub fn validate_all_chords(config: &AppConfig) -> Vec<String> {
    use std::collections::{BTreeSet, HashMap};
    let mut issues = Vec::new();
    let mut seen: HashMap<String, usize> = HashMap::new();
    let mut combo_vids: BTreeSet<u16> = BTreeSet::new();

    for (i, m) in config.mappings.iter().enumerate() {
        let t = m.trigger_key.trim();
        let upper = t.to_ascii_uppercase();
        let looks_like_pad_combo = t.contains('+')
            && (upper.starts_with("GAMEPAD_") || upper.starts_with("JOYSTICK_"));
        if !looks_like_pad_combo {
            continue;
        }
        match chord_ids_of(t) {
            Some((vid, ids)) => {
                combo_vids.insert(vid);
                let norm = chord_trigger_name(vid, &ids);
                if let Some(&first) = seen.get(&norm) {
                    issues.push(format!(
                        "第 {} 条与第 {} 条是同一个组合键 {} (完全重复, 后者不会优先生效)",
                        i + 1,
                        first + 1,
                        norm
                    ));
                } else {
                    seen.insert(norm, i);
                }
            }
            None => issues.push(format!(
                "第 {} 条映射的触发键 {} 不是可解析的手柄组合键 (按钮名请用 LB/RB/LT/RT/A/B/X/Y/DPad_Up/LS_Up 等)",
                i + 1,
                t
            )),
        }
    }

    /* 同 VID 不同 PID (用户实测: 有线 20BC:5158 / 无线 20BC:5159) 的组合键
     * 目前按 VID 编码, 无法区分两台设备 —— 这里给出明确诊断而不是静默串键。 */
    let mut pids_by_vid: HashMap<String, BTreeSet<String>> = HashMap::new();
    for m in &config.mappings {
        let p: Vec<&str> = m.trigger_key.split('_').collect();
        if p.len() >= 4
            && p[1].len() == 4
            && p[2].len() == 4
            && p[1].chars().all(|c| c.is_ascii_hexdigit())
            && p[2].chars().all(|c| c.is_ascii_hexdigit())
        {
            pids_by_vid
                .entry(p[1].to_ascii_uppercase())
                .or_default()
                .insert(p[2].to_ascii_uppercase());
        }
    }
    for vid in combo_vids {
        let key = format!("{vid:04X}");
        if let Some(pids) = pids_by_vid.get(&key)
            && pids.len() > 1
        {
            issues.push(format!(
                "VID {key} 下检测到 {} 个不同 PID 的设备 ({}), 组合键当前按 VID 共享, 这些设备上会互相串键",
                pids.len(),
                pids.iter().cloned().collect::<Vec<_>>().join("/")
            ));
        }
    }
    issues
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
        crate::vibration::vib_log(&format!(
            "[chord] capture-start | existing={}",
            chord_mapping_indexes(&self.config).len()
        ));
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
                        /* ★R2.12: 组合键数量上去后要找得到 —— 搜索 / 按修饰键分组 /
                         * 展开全部序列宏 / 带二次确认的批量删除。状态存 egui 临时数据,
                         * 不往 SorahkGui 和 Config.toml 里加字段。 */
                        let filter_id = egui::Id::new("gp_chord_filter_v1");
                        let grouped_id = egui::Id::new("gp_chord_grouped_v1");
                        let expand_id = egui::Id::new("gp_chord_expand_v1");
                        let confirm_id = egui::Id::new("gp_chord_batch_v1");
                        let mut filter: String = ui
                            .ctx()
                            .data_mut(|d| d.get_temp::<String>(filter_id).unwrap_or_default());
                        let mut grouped: bool = ui
                            .ctx()
                            .data_mut(|d| d.get_temp::<bool>(grouped_id).unwrap_or(true));
                        let mut expand_all: bool = ui
                            .ctx()
                            .data_mut(|d| d.get_temp::<bool>(expand_id).unwrap_or(false));
                        let mut confirm_count: usize = ui
                            .ctx()
                            .data_mut(|d| d.get_temp::<usize>(confirm_id).unwrap_or(0));

                        ui.horizontal(|ui| {
                            ui.label(th.hint_text("搜索"));
                            ui.add(
                                egui::TextEdit::singleline(&mut filter)
                                    .desired_width(150.0)
                                    .hint_text("组合 / 目标键"),
                            );
                            let _ = ui.checkbox(&mut grouped, "按修饰键分组");
                            let _ = ui.checkbox(&mut expand_all, "展开全部序列宏");
                        });
                        let needle = filter.trim().to_ascii_lowercase();
                        let visible: Vec<usize> = indexes
                            .iter()
                            .copied()
                            .filter(|&i| {
                                if needle.is_empty() {
                                    return true;
                                }
                                let m = &self.config.mappings[i];
                                m.trigger_key.to_ascii_lowercase().contains(&needle)
                                    || m.get_target_keys()
                                        .join(" + ")
                                        .to_ascii_lowercase()
                                        .contains(&needle)
                            })
                            .collect();
                        /* 分组排序: 同组的行挨在一起, 循环里遇到新组名就插一行标题 */
                        let mut ordered = visible.clone();
                        if grouped {
                            ordered.sort_by_key(|&i| {
                                chord_ids_of(&self.config.mappings[i].trigger_key)
                                    .map(|(_, ids)| chord_group_label(&ids))
                                    .unwrap_or("其它组合")
                            });
                        }
                        ui.horizontal(|ui| {
                            ui.label(th.hint_text(format!("共 {} 条组合键", indexes.len())));
                            if !needle.is_empty() {
                                ui.label(th.hint_text(format!("筛出 {} 条", visible.len())));
                            }
                            let batch_label = format!("删除筛选结果 ({})", visible.len());
                            if !visible.is_empty()
                                && ui.add(th.ghost_button(batch_label.as_str())).clicked()
                            {
                                confirm_count = visible.len();
                            }
                        });
                        if confirm_count > 0 {
                            ui.horizontal(|ui| {
                                ui.label(th.hint_text(format!(
                                    "确认删除这 {} 条组合键？此操作不可撤销",
                                    confirm_count
                                )));
                                if ui.add(th.primary_button("确认删除")).clicked() {
                                    for i in visible.iter().rev() {
                                        if *i < self.config.mappings.len() {
                                            self.config.mappings.remove(*i);
                                        }
                                    }
                                    self.gp_save_and_reload();
                                    self.set_gamepad_toast(format!(
                                        "已删除 {} 条组合键",
                                        confirm_count
                                    ));
                                    crate::vibration::vib_log(&format!(
                                        "[chord] batch-delete | count={confirm_count}"
                                    ));
                                    confirm_count = 0;
                                }
                                if ui.add(th.ghost_button("取消")).clicked() {
                                    confirm_count = 0;
                                }
                            });
                        }
                        ui.ctx().data_mut(|d| {
                            d.insert_temp(filter_id, filter.clone());
                            d.insert_temp(grouped_id, grouped);
                            d.insert_temp(expand_id, expand_all);
                            d.insert_temp(confirm_id, confirm_count);
                        });
                        let mut last_group = String::new();
                        for &i in &ordered {
                            /* ★R2.12 分组标题: 同组只打一次, 40 条组合键也能一眼定位 */
                            if grouped {
                                let group = chord_ids_of(&self.config.mappings[i].trigger_key)
                                    .map(|(_, ids)| chord_group_label(&ids))
                                    .unwrap_or("其它组合");
                                if group != last_group.as_str() {
                                    last_group = group.to_string();
                                    ui.add_space(2.0);
                                    th.badge(ui, group, th.accent_text, th.accent_soft);
                                }
                            }
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
                            .default_open(expand_all)
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

    /// ★R2.12 分组标签: 共享修饰键的一族组合归到同一组, 便于 40 条时定位。
    #[test]
    fn test_chord_group_label() {
        assert_eq!(chord_group_label(&[0x0B, 0x18]), "LT 组");
        assert_eq!(chord_group_label(&[0x09, 0x0D]), "LB 组");
        assert_eq!(chord_group_label(&[0x0A, 0x0D]), "RB 组");
        assert_eq!(chord_group_label(&[0x0B, 0x0C]), "其它组合");
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

    /// ★R2.12: 部分重叠**不再拒绝** —— 共享修饰键的一族组合是核心用法;
    /// 完全仍然重复 (否则同一触发键会有两条映射, 行为不确定)。
    #[test]
    fn test_validate_chord_duplicates_and_overlap() {
        let c = config_with(&["GAMEPAD_045E_RB+X", "GAMEPAD_045E_A+LB"]);
        assert!(
            validate_chord_trigger(&c, 0x045E, &[RB, X]).is_err(),
            "完全重复仍应拒绝"
        );
        assert!(
            validate_chord_trigger(&c, 0x045E, &[RB, Y]).is_ok(),
            "RB+Y 与 RB+X 共享 RB 应允许"
        );
        assert!(validate_chord_trigger(&c, 0x045E, &[RB, X, Y]).is_ok());
        assert!(
            validate_chord_trigger(&c, 0x045E, &[X, 0x18]).is_ok(),
            "X+LT 与 RB+X 共享 X 应允许"
        );
        assert!(validate_chord_trigger(&c, 0x045E, &[0x18, 0x19]).is_ok());
    }

    /// ★R2.12 用户原话: LT+A / LT+B / LT+X / LT+Y 四条要能一次建出来。
    /// 旧版在第 2 条就会被「部分重叠」拦下 (只能建 1/4)。
    #[test]
    fn test_shared_modifier_chord_family_is_buildable() {
        let mut c = config_with(&[]);
        const LT: u32 = 0x18;
        for (name, key) in [("A", 0x0Bu32), ("B", 0x0C), ("X", 0x0D), ("Y", 0x0E)] {
            let ids = vec![key, LT];
            assert!(
                validate_chord_trigger(&c, 0x045E, &ids).is_ok(),
                "{name}+LT 应能建成 (第 {} 条)",
                c.mappings.len() + 1
            );
            c.mappings.push(KeyMapping {
                release_targets: Default::default(),
                sequence_text: String::new(),
                trigger_key: chord_trigger_name(0x045E, &ids),
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
        assert_eq!(chord_mapping_indexes(&c).len(), 4);
        let issues = validate_all_chords(&c);
        assert!(issues.is_empty(), "四条共享 LT 的组合键体检应无问题: {issues:?}");
    }

    /// ★R2.12: 加载期体检要能发现完全重复与解析不了的组合键。
    #[test]
    fn test_validate_all_chords_finds_duplicates_and_bad_names() {
        let c = config_with(&[
            "GAMEPAD_045E_LT+A",
            "GAMEPAD_045E_LT+A",
            "GAMEPAD_045E_LS_RightUp+A",
        ]);
        let issues = validate_all_chords(&c);
        assert!(
            issues.iter().any(|s| s.contains("完全重复")),
            "重复组合键应被报出: {issues:?}"
        );
        assert!(
            issues.iter().any(|s| s.contains("不是可解析")),
            "斜向名 LS_RightUp 无法解析, 应被报出: {issues:?}"
        );
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
