//! Main window implementation and rendering logic.

use crate::gui::SorahkGui;
use crate::gui::theme::{self, Theme, truncate_chars};
use crate::gui::utils;
use crate::gui::widgets;
use crate::gui::types::KeyCaptureMode;

use eframe::egui;
use super::main_window::FrameState;

impl SorahkGui {
    /// 连发页: 状态 hero + 映射列表 + 全局参数。
    pub(super) fn render_turbo_page(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, frame_state: &FrameState) {
        self.render_turbo_hero(ui, frame_state);
        /* ★v24.4: 经典模式在「预设管理」上方补一条震动快捷条 (用户要求; 完整模式 hero 已含震动开关) */
        if self.classic_mode {
            self.render_classic_vib_quickbar(ui);
        }
        self.render_turbo_preset_manager(ui);
        self.render_turbo_mappings(ui);
        self.render_turbo_params(ui, ctx);
    }


    /// 连发映射页 · 预设管理卡: 切换 / 保存 / 重命名 / 删除 (两次确认防误删)。
    pub(super) fn render_turbo_preset_manager(&mut self, ui: &mut egui::Ui) {
        let th = self.theme();
        th.card(ui, Some("预设管理"), |ui| {
            /* 行1: 切换预设 (选择即应用) */
            ui.horizontal(|ui| {
                ui.label(th.weak("切换预设:"));
                let cur_name = if self.config.current_preset.is_empty() {
                    "(无)".to_owned()
                } else {
                    self.config.current_preset.clone()
                };
                let mut next = self.config.current_preset.clone();
                let mut switched = false;
                egui::ComboBox::from_id_salt("page_preset_switch")
                    .selected_text(cur_name)
                    .width(180.0)
                    .show_ui(ui, |ui| {
                        if ui
                            .selectable_label(self.config.current_preset.is_empty(), "(无)")
                            .clicked()
                        {
                            next = String::new();
                            switched = true;
                        }
                        for pr in &self.config.presets {
                            let is_sel = self.config.current_preset == pr.name;
                            if ui.selectable_label(is_sel, &pr.name).clicked() {
                                next = pr.name.clone();
                                switched = true;
                            }
                        }
                    });
                if switched {
                    self.config.current_preset = next.clone();
                    if let Some(pr) = self.config.presets.iter().find(|p| p.name == next) {
                        if !pr.mappings.is_empty() {
                            self.config.mappings = pr.mappings.clone();
                        }
                    }
                    self.page_preset_delete_arm = false;
                    let _ = self.config.save_to_file("Config.toml");
                    // 切换即热重载: 运行时钩子只消费 AppState.input_mappings,
                    // 不 reload 的话 UI 显示新映射而钩子仍按旧映射连发
                    if let Err(e) = self.app_state.reload_config(self.config.clone()) {
                        eprintln!("Failed to reload config after preset switch: {}", e);
                    }
                }
                ui.label(th.hint_text("选择后立即应用映射"));
            });
            ui.add_space(theme::SP_S);

            /* 行2: 保存预设 (留空名称 = 覆盖当前预设) */
            ui.horizontal(|ui| {
                ui.label(th.weak("保存预设:"));
                /* ★v20.6: 显性输入框 (描边清晰, 一眼可见可输入) */
                th.text_input(
                    ui,
                    &mut self.page_preset_name_input,
                    "名称 (留空 = 覆盖当前预设)",
                    170.0,
                    egui::Id::new("page_preset_name_input"),
                );
                if ui
                    .add(egui::Button::new(
                        egui::RichText::new("保存预设").size(13.0).color(egui::Color32::WHITE),
                    )
                    .fill(th.btn_primary)
                    .corner_radius(egui::CornerRadius::same(theme::RADIUS_CTRL)))
                    .clicked()
                {
                    let typed = self.page_preset_name_input.trim().to_string();
                    let name = if typed.is_empty() {
                        self.config.current_preset.trim().to_string()
                    } else {
                        typed
                    };
                    if !name.is_empty() {
                        /* ★v20.3: 同名覆盖时保留已绑定的切换键 */
                        let switch_key = self
                            .config
                            .presets
                            .iter()
                            .find(|p| p.name == name)
                            .map(|p| p.switch_key.clone())
                            .unwrap_or_default();
                        self.config.presets.retain(|p| p.name != name);
                        self.config.presets.push(crate::config::Preset {
                            name: name.clone(),
                            mappings: self.config.mappings.clone(),
                            switch_key,
                        });
                        self.config.current_preset = name;
                        self.page_preset_name_input.clear();
                        self.page_preset_delete_arm = false;
                        let _ = self.config.save_to_file("Config.toml");
                    }
                }
                ui.label(th.hint_text("保存当前整页映射为预设"));
            });
            ui.add_space(theme::SP_S);

            /* 行3: 重命名 + 删除 (两次确认), 仅当前预设非空时可用 */
            if !self.config.current_preset.is_empty() {
                ui.horizontal(|ui| {
                    ui.label(th.weak("当前预设:"));
                    if ui
                        .add(egui::Button::new(
                            egui::RichText::new("重命名预设").size(13.0).color(th.btn_secondary_text),
                        )
                        .fill(th.faint)
                        .corner_radius(egui::CornerRadius::same(theme::RADIUS_CTRL)))
                        .clicked()
                    {
                        self.page_preset_rename_input = self.config.current_preset.clone();
                        self.page_preset_rename_show = true;
                    }
                    let del_label = if self.page_preset_delete_arm {
                        "⚠ 确认删除? (再次点击)"
                    } else {
                        "删除预设"
                    };
                    let del_btn = egui::Button::new(
                        egui::RichText::new(del_label).size(13.0).color(egui::Color32::WHITE),
                    )
                    .fill(if self.page_preset_delete_arm { th.bad } else { th.faint })
                    .corner_radius(egui::CornerRadius::same(theme::RADIUS_CTRL));
                    if ui.add(del_btn).clicked() {
                        if self.page_preset_delete_arm {
                            let name = self.config.current_preset.clone();
                            self.config.presets.retain(|p| p.name != name);
                            self.config.current_preset.clear();
                            self.page_preset_delete_arm = false;
                            let _ = self.config.save_to_file("Config.toml");
                        } else {
                            self.page_preset_delete_arm = true;
                        }
                    }
                    if self.page_preset_delete_arm && ui.button("取消").clicked() {
                        self.page_preset_delete_arm = false;
                    }
                });

                /* 重命名输入行 */
                if self.page_preset_rename_show {
                    ui.add_space(theme::SP_XS);
                    ui.horizontal(|ui| {
                        ui.label(th.weak("新名称:"));
                        /* ★v20.6: 显性输入框 */
                        th.text_input(
                            ui,
                            &mut self.page_preset_rename_input,
                            "输入新名称",
                            170.0,
                            egui::Id::new("page_preset_rename_input"),
                        );
                        if ui
                            .add(egui::Button::new(
                                egui::RichText::new("✓ 确认").size(13.0).color(egui::Color32::WHITE),
                            )
                            .fill(th.good)
                            .corner_radius(egui::CornerRadius::same(theme::RADIUS_CTRL)))
                            .clicked()
                        {
                            let old = self.config.current_preset.clone();
                            let new_name = self.page_preset_rename_input.trim().to_string();
                            if !new_name.is_empty() && new_name != old {
                                if let Some(pr) =
                                    self.config.presets.iter_mut().find(|p| p.name == old)
                                {
                                    pr.name = new_name.clone();
                                }
                                self.config.current_preset = new_name;
                                let _ = self.config.save_to_file("Config.toml");
                            }
                            self.page_preset_rename_show = false;
                            self.page_preset_rename_input.clear();
                        }
                        if ui.button("✕").clicked() {
                            self.page_preset_rename_show = false;
                            self.page_preset_rename_input.clear();
                        }
                    });
                }
            } else {
                ui.label(th.hint_text("顶栏选择「(无)」时映射未入档: 用上方「保存预设」起名入档后可切换/重命名/删除"));
            }

            /* ★v20.3 行4: 预设切换键 —— 游戏内按组合键直接切预设 (保存时防冲突) */
            ui.add_space(theme::SP_S);
            ui.separator();
            ui.add_space(theme::SP_XS);
            if self.config.presets.is_empty() {
                ui.label(th.hint_text("组合键切换预设: 先用上方「保存预设」入档, 再在这里绑定切换键"));
            } else {
                if self.preset_key_target >= self.config.presets.len() {
                    self.preset_key_target = self.config.presets.len() - 1;
                }
                let cur_key = self.config.presets[self.preset_key_target].switch_key.clone();
                /* ★v21.7c: 标签列表跟随所选预设 (只捕获不手输 → 无法输入汉字等非法内容) */
                if self.preset_key_parts_signature != cur_key {
                    self.preset_key_parts = Self::decombo_switch_key(&cur_key);
                    self.preset_key_parts_signature = cur_key.clone();
                }
                ui.horizontal_wrapped(|ui| {
                    ui.label(th.weak("切换键:"));
                    let target_name = self.config.presets[self.preset_key_target].name.clone();
                    let mut next_idx = self.preset_key_target;
                    egui::ComboBox::from_id_salt("preset_switch_key_target")
                        .selected_text(target_name)
                        .width(130.0)
                        .show_ui(ui, |ui| {
                            for (i, pr) in self.config.presets.iter().enumerate() {
                                if ui
                                    .selectable_label(i == self.preset_key_target, &pr.name)
                                    .clicked()
                                {
                                    next_idx = i;
                                }
                            }
                        });
                    if next_idx != self.preset_key_target {
                        self.preset_key_target = next_idx;
                        let k = self.config.presets[next_idx].switch_key.clone();
                        self.preset_key_parts = Self::decombo_switch_key(&k);
                        self.preset_key_parts_signature = k;
                        self.preset_key_error = None;
                    }

                    /* 键位标签 (点 × 移除该键); 空 = 未设置 */
                    let mut remove_idx: Option<usize> = None;
                    if self.preset_key_parts.is_empty() {
                        ui.label(th.hint_text("(未设置)"));
                    }
                    for (i, part) in self.preset_key_parts.iter().enumerate() {
                        if ui
                            .add(th.secondary_button(&format!("{part} ×")))
                            .on_hover_text("点击移除这个键")
                            .clicked()
                        {
                            remove_idx = Some(i);
                        }
                        if i + 1 < self.preset_key_parts.len() {
                            ui.label(th.weak("+"));
                        }
                    }
                    if let Some(i) = remove_idx {
                        self.preset_key_parts.remove(i);
                        self.preset_key_error = None;
                    }

                    /* 捕获 (连续捕获可追加成组合键) */
                    let capturing = self.key_capture_mode == KeyCaptureMode::PresetSwitchKey;
                    let cap_label = if capturing { "⌨ 请按键…" } else { "🎮 捕获" };
                    if ui
                        .add(th.secondary_button(cap_label))
                        .on_hover_text(
                            "点一下再按一个键 —— 可连续点多次追加, 拼成组合键 (如先 CTRL 再 F6); Esc 取消",
                        )
                        .clicked()
                    {
                        if capturing {
                            self.cancel_preset_switch_capture();
                        } else {
                            self.start_preset_switch_capture();
                        }
                    }
                    if ui
                        .add_enabled(
                            !self.preset_key_parts.is_empty(),
                            egui::Button::new(
                                egui::RichText::new("⌫ 删除末键")
                                    .size(13.0)
                                    .color(th.btn_secondary_text),
                            )
                            .fill(th.faint)
                            .corner_radius(egui::CornerRadius::same(theme::RADIUS_CTRL)),
                        )
                        .clicked()
                    {
                        self.preset_key_parts.pop();
                        self.preset_key_error = None;
                    }
                    if ui.add(th.secondary_button("✓ 保存")).clicked() {
                        self.save_preset_switch_key();
                    }
                    if ui.add(th.secondary_button("清除")).clicked() {
                        self.preset_key_parts.clear();
                        self.preset_key_error = None;
                        self.save_preset_switch_key();
                    }
                    if capturing {
                        ui.label(
                            egui::RichText::new(
                                "● 等待输入: 按键盘或手柄任意键 (可继续点「捕获」追加; Esc 取消)",
                            )
                            .size(11.0)
                            .color(th.accent),
                        );
                    } else if let Some(err) = &self.preset_key_error {
                        ui.label(
                            egui::RichText::new(format!("❌ {}", err))
                                .size(11.0)
                                .color(th.bad),
                        );
                    } else {
                        ui.label(th.hint_text(
                            "只能捕获不能手输; 可连续捕获多个键组成组合键 (如 CTRL + F6)",
                        ));
                    }
                });
            }
        });
    }

    /// ★v20.3: 保存所选预设的切换键。
    /// 校验: ①键名可被解析 (键盘键名 / 手柄组合 / 原始 HID 设备名) ②不与其他预设的
    /// 切换键重复 ③不与「连发切换键」重复 ④**不与任何预设的任何映射触发键重名或同组件**
    /// —— 见下方 `find_preset_switch_conflict` (v21.7 用户提出的"连发/组合键冲突"以此根治)。
    pub(super) fn save_preset_switch_key(&mut self) {
        if self.preset_key_target >= self.config.presets.len() {
            return;
        }
        let target_name = self.config.presets[self.preset_key_target].name.clone();
        /* ★v21.7d: 组合键先去重+规范排序 (用户要求 相同键不重复 / 顺序严格) */
        let key = crate::util::normalize_key_combo(&Self::compact_switch_key_parts(
            &self.preset_key_parts,
        ));
        if key.is_empty() {
            self.config.presets[self.preset_key_target].switch_key.clear();
            self.preset_key_parts_signature.clear();
            self.preset_key_error = None;
        } else if Self::combo_is_modifier_only(&key) {
            self.preset_key_error = Some(
                "组合键需要至少一个非修饰键 (如 CTRL+F6, 不能只有 CTRL+SHIFT)".to_string(),
            );
            return;
        } else if !crate::state::AppState::is_valid_input_name(&key) {
            self.preset_key_error = Some(
                "无法识别的组合 (手柄的多个按键暂不能合成一个切换键; 键盘组合如 CTRL+F6 可以)"
                    .to_string(),
            );
            return;
        } else if let Some(other) = self.config.presets.iter().find(|p| {
            p.name != target_name
                && !p.switch_key.trim().is_empty()
                && p.switch_key.trim().to_uppercase() == key
        }) {
            self.preset_key_error = Some(format!("与预设「{}」的切换键冲突", other.name));
            return;
        } else if !self.config.switch_key.trim().is_empty()
            && self.config.switch_key.trim().to_uppercase() == key
        {
            self.preset_key_error = Some("与「连发切换键」冲突, 请换一个键".to_string());
            return;
        } else if let Some((preset, trigger, turbo_combo)) = self.find_preset_switch_conflict(&key)
        {
            self.preset_key_error = Some(if turbo_combo {
                format!(
                    "与预设「{}」的映射「{}」冲突 (该映射带连发/组合性质, 同时触发会互相干扰); 切换键必须独占, 请换一个键",
                    preset, trigger
                )
            } else {
                format!(
                    "与预设「{}」的映射「{}」重名 (切换键独占: 按下时该映射不会触发); 建议换一个键",
                    preset, trigger
                )
            });
            return;
        } else {
            self.config.presets[self.preset_key_target].switch_key = key.clone();
            self.preset_key_parts_signature = key.clone();
            /* 回显规范顺序 (去重/排序后的标签) */
            self.preset_key_parts = Self::decombo_switch_key(&key);
            self.preset_key_error = None;
        }
        let _ = self.config.save_to_file("Config.toml");
    }

    /// ★v21.7c: 已存切换键 → 标签列表 (反向分解, 供 UI 显示 chips)。
    /// `GAMEPAD_045E_A+B` → ["GAMEPAD_045E_A", "GAMEPAD_045E_B"]; `CTRL+F6` → ["CTRL","F6"]。
    pub(super) fn decombo_switch_key(key: &str) -> Vec<String> {
        crate::util::key_combo_parts(key)
    }

    /// ★v21.7c: 标签列表 → 已存切换键字符串 (正向着色)。
    /// 同一手柄的多个按键合并成 `GAMEPAD_<VID>_A+B`; 修饰键 (CTRL/SHIFT/ALT) 前置。
    pub(super) fn compact_switch_key_parts(parts: &[String]) -> String {
        crate::util::compact_key_combo(parts)
    }

    /// ★v21.7: 在全部预设的全部映射里查与 `key` 冲突的触发键。
    /// 判定 = 完全相同 **或** 作为 `+` 组合键的任一组件 (如映射 CTRL+F6 与切换键 F6 也冲突)。
    /// 返回 (预设名, 触发键原文, 是否带连发/组合性质)。
    fn find_preset_switch_conflict(&self, key: &str) -> Option<(String, String, bool)> {
        Self::find_preset_switch_conflict_in(&self.config.presets, key)
    }

    /// 纯函数版 (可单测): 冲突查找规则见上。
    pub(super) fn find_preset_switch_conflict_in(
        presets: &[crate::config::Preset],
        key: &str,
    ) -> Option<(String, String, bool)> {
        let key_u = key.trim().to_uppercase();
        if key_u.is_empty() {
            return None;
        }
        let key_is_pad = Self::looks_like_gamepad_input(&key_u);
        for p in presets {
            for m in &p.mappings {
                let trig_u = m.trigger_key.trim().to_uppercase();
                if trig_u.is_empty() {
                    continue;
                }
                let exact = trig_u == key_u;
                /* 组件匹配只对 '+' 组合键有意义, 且**必须同类输入**:
                 * 键盘切换键 "B" 不得与手柄组合 "GAMEPAD_x_A+B" 的 "B" 组件相撞
                 * (那是两个完全不同的物理输入)。 */
                let component = trig_u.contains('+')
                    && Self::looks_like_gamepad_input(&trig_u) == key_is_pad
                    && trig_u.split('+').any(|part| part.trim() == key_u);
                if exact || component {
                    let turbo_combo = m.turbo_enabled
                        || m.run_enabled
                        || m.double_tap_enabled
                        || trig_u.contains('+');
                    return Some((p.name.clone(), m.trigger_key.clone(), turbo_combo));
                }
            }
        }
        None
    }

    /// 是否手柄类输入名 (与 `gui::utils::key_kind` 同判定, 供冲突规则区分输入类别)。
    fn looks_like_gamepad_input(name_upper: &str) -> bool {
        name_upper.starts_with("GAMEPAD")
            || name_upper.starts_with("JOYSTICK")
            || name_upper.starts_with("HID_")
    }

    /// 组合键是否只有修饰键 (那样永远触发不了, 保存时应拦下)。
    fn combo_is_modifier_only(key: &str) -> bool {
        if !key.contains('+') {
            return false;
        }
        key.split('+').all(|p| {
            matches!(
                p.trim().to_uppercase().as_str(),
                "CTRL" | "LCTRL" | "RCTRL" | "SHIFT" | "LSHIFT" | "RSHIFT" | "ALT" | "LALT" | "RALT"
            )
        })
    }

    /// ★v21.7: 开始捕获预设切换键 —— 键盘任意键 **或** 手柄任意键 (此功能主要为手柄准备)。
    pub(super) fn start_preset_switch_capture(&mut self) {
        self.key_capture_mode = KeyCaptureMode::PresetSwitchKey;
        self.capture_pressed_keys.clear();
        self.capture_initial_pressed = Self::poll_all_pressed_keys();
        self.app_state.set_raw_input_capture_mode(true);
        self.just_captured_input = false;
        self.preset_key_error = None;
    }

    /// ★v21.7: 取消/结束预设切换键捕获。
    pub(super) fn cancel_preset_switch_capture(&mut self) {
        self.key_capture_mode = KeyCaptureMode::None;
        self.capture_pressed_keys.clear();
        self.app_state.set_raw_input_capture_mode(false);
        self.just_captured_input = false;
    }

    /// ★v21.7: 预设切换键捕获轮询 (键盘: 松开即取; 手柄: RawInput/XInput 捕获通道)。
    pub(super) fn handle_preset_switch_capture(&mut self, ctx: &egui::Context) {
        if self.key_capture_mode != KeyCaptureMode::PresetSwitchKey {
            return;
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.cancel_preset_switch_capture();
            return;
        }

        let mut captured: Option<String> = None;

        /* 键盘: 记录新按下的键, 任一松开即视为完成 (与映射捕获同语义) */
        let current_pressed = Self::poll_all_pressed_keys();
        current_pressed
            .iter()
            .filter(|&&vk| !self.capture_initial_pressed.contains(&vk))
            .for_each(|&vk| {
                self.capture_pressed_keys.insert(vk);
            });
        if self
            .capture_pressed_keys
            .iter()
            .any(|vk| !current_pressed.contains(vk))
        {
            captured = Self::format_captured_keys(&self.capture_pressed_keys);
        }

        /* 手柄: 原始输入捕获通道 (RawInput 原始报文 / XInput 标准按键) */
        if captured.is_none()
            && let Some(device) = self.app_state.try_recv_raw_input_capture()
        {
            captured = Some(device.to_string());
        }

        if let Some(name) = captured {
            self.cancel_preset_switch_capture();
            /* ★v21.7c: 追加到标签列表 (可连续捕获组成组合键), 不自动保存 */
            self.append_captured_switch_key(&name);
        }
    }

    /// ★v21.7c: 把一次捕获结果追加进切换键标签列表。
    /// 支持一次捕获多个键 (同时按住的组合会一次给出 "CTRL+F6" / "GAMEPAD_045E_A+B")。
    /// 已有的重复键不重复添加; 手柄多键合成暂不支持 → 明确报错。
    pub(super) fn append_captured_switch_key(&mut self, captured: &str) {
        let new_parts = Self::decombo_switch_key(captured);
        if new_parts.is_empty() {
            return;
        }
        let semantic = |s: &str| {
            match crate::state::AppState::input_name_to_device(s) {
                Some(crate::state::InputDevice::GenericDevice { button_id, .. }) => {
                    let pos = (button_id & 0xFFFF_FFFF) as u32;
                    crate::hid_layout::semantic_button_usage(pos).is_some()
                        || crate::hid_layout::semantic_axis_decode(pos).is_some()
                }
                _ => false,
            }
        };
        /* 手柄语义键一次只能一个 (多个合成暂不支持) */
        if new_parts.iter().any(|p| semantic(p))
            && self.preset_key_parts.iter().any(|p| semantic(p))
        {
            self.preset_key_error = Some(
                "该手柄的多个按键暂不能合成一个切换键; 请只保留一个手柄键, 或改用键盘组合"
                    .to_string(),
            );
            return;
        }
        let mut changed = false;
        for p in new_parts {
            if !self
                .preset_key_parts
                .iter()
                .any(|e| e.eq_ignore_ascii_case(&p))
            {
                self.preset_key_parts.push(p);
                changed = true;
            }
        }
        if changed {
            self.preset_key_error = None;
        }
    }


    /// 状态 hero: 运行状态 + 震动开关 + 主操作按钮 + 统计。
    pub(super) fn render_turbo_hero(&mut self, ui: &mut egui::Ui, frame_state: &FrameState) {
        // 提前取值, 闭包内 &mut self 与 translations 借用不冲突
        let status_paused = self.translations.paused_status().to_owned();
        let status_running = self.translations.running_status().to_owned();
        let exit_label = self.translations.exit_button().to_owned();
        let start_label = self.translations.start_button().to_owned();
        let pause_label = self.translations.pause_button().to_owned();
        let th = self.theme();
        th.card(ui, None, |ui| {
            ui.horizontal(|ui| {
                // 左: 状态
                ui.vertical(|ui| {
                    ui.add_space(2.0);
                    ui.horizontal(|ui| {
                        let (color, text, pulsing) = if frame_state.is_paused {
                            (th.warn, status_paused.as_str(), false)
                        } else {
                            (th.good, status_running.as_str(), true)
                        };
                        widgets::status_dot(ui, color, pulsing, 6.0);
                        ui.add_space(2.0);
                        ui.label(
                            egui::RichText::new(text)
                                .size(17.0)
                                .strong()
                                .color(if frame_state.is_paused { th.warn } else { th.good }),
                        );
                    });
                    ui.add_space(theme::SP_XS);
                    ui.horizontal(|ui| {
                        let vib_on = self
                            .app_state
                            .vibration_enabled
                            .load(std::sync::atomic::Ordering::Relaxed);
                        let (vtext, vcolor, vbg) = if vib_on {
                            ("震动: 开", th.good, th.good_soft)
                        } else {
                            ("震动: 关", th.bad, th.bad_soft)
                        };
                        if ui.add(th.status_pill(vtext, vcolor, vbg)).clicked() {
                            let v = self
                                .app_state
                                .vibration_enabled
                                .load(std::sync::atomic::Ordering::Relaxed);
                            self.app_state
                                .vibration_enabled
                                .store(!v, std::sync::atomic::Ordering::Relaxed);
                        }
                        ui.label(th.hint_text("由 DfoVibration.dll 战斗事件驱动 (进图后自动)"));
                    });
                });

                // 右: 主操作按钮
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let exit_btn = egui::Button::new(
                        egui::RichText::new(format!("\u{23f9} {}", exit_label))
                            .size(13.0)
                            .strong()
                            .color(egui::Color32::WHITE),
                    )
                    .fill(th.btn_danger)
                    .corner_radius(egui::CornerRadius::same(theme::RADIUS_CTRL))
                    .min_size(egui::vec2(96.0, 34.0));
                    if ui.add(exit_btn).clicked() {
                        // 只走 app_state.exit(): 经 should_exit → ViewportCommand::Close
                        // 优雅退出, 保证托盘/震动线程清理与 on_exit 执行
                        self.app_state.exit();
                    }
                    ui.add_space(theme::SP_S);
                    let (label, color) = if frame_state.is_paused {
                        (start_label.as_str(), th.good)
                    } else {
                        (pause_label.as_str(), th.warn)
                    };
                    let toggle_btn = egui::Button::new(
                        egui::RichText::new(label).size(13.0).color(egui::Color32::WHITE).strong(),
                    )
                    .fill(color)
                    .corner_radius(egui::CornerRadius::same(theme::RADIUS_CTRL))
                    .min_size(egui::vec2(96.0, 34.0));
                    if ui.add(toggle_btn).clicked() {
                        self.toggle_with_notify();
                    }
                });
            });

            // 统计条: 映射数 / 工作线程 / 默认间隔 / 默认时长 / 切换热键
            ui.add_space(theme::SP_M);
            ui.separator();
            ui.add_space(theme::SP_S);
            ui.horizontal(|ui| {
                widgets::stat(
                    ui,
                    &th,
                    &self.config.mappings.len().to_string(),
                    "映射条目",
                    th.accent_text,
                );
                ui.separator();
                if frame_state.worker_count > 0 {
                    widgets::stat(
                        ui,
                        &th,
                        &frame_state.worker_count.to_string(),
                        "工作线程",
                        th.text,
                    );
                    ui.separator();
                }
                widgets::stat(ui, &th, &format!("{} ms", self.config.interval), "默认间隔", th.text);
                ui.separator();
                widgets::stat(
                    ui,
                    &th,
                    &format!("{} ms", self.config.event_duration),
                    "默认时长",
                    th.text,
                );
                ui.separator();
                ui.vertical(|ui| {
                    ui.set_min_width(64.0);
                    widgets::keycap(ui, &th, &self.config.switch_key);
                    ui.label(th.hint_text("切换热键"));
                });
            });
        });
    }


    /// 映射列表: 行式布局 + 行内编辑/新增 (无需再进设置弹窗)。
    pub(super) fn render_turbo_mappings(&mut self, ui: &mut egui::Ui) {
        let th = self.theme();
        let count = self.config.mappings.len();
        th.card(ui, None, |ui| {
            ui.horizontal(|ui| {
                ui.label(th.h2("连发映射"));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        egui::RichText::new(format!("{} 条", count))
                            .size(12.0)
                            .color(th.hint),
                    );
                    ui.add_space(theme::SP_S);
                    if ui.add(th.primary_button("＋ 新增映射")).clicked() {
                        self.add_new_mapping();
                    }
                });
            });
            ui.add_space(theme::SP_M);
            {
                if count == 0 {
                    widgets::empty_state(
                        ui,
                        &th,
                        widgets::Icon::Keyboard,
                        "暂无连发映射",
                        &[
                            "点击右上角「＋ 新增映射」创建第一条",
                            "配置 手柄/键盘/鼠标 触发 → 目标键",
                        ],
                    );
                    return;
                }
                let interval = self.config.interval;
                let duration = self.config.event_duration;
                let mut idx = 0usize;
                while idx < self.config.mappings.len() {
                    if self.edit_mapping_idx == Some(idx) {
                        /* ★v20.3: 新增映射置顶后, 把编辑面板滚到可视区顶部 */
                        if std::mem::take(&mut self.scroll_to_edit_row) {
                            ui.scroll_to_cursor(Some(egui::Align::TOP));
                        }
                        self.render_mapping_edit_row(ui, idx, interval, duration);
                        // 编辑行可能触发删除, 重新检查下标
                        if idx >= self.config.mappings.len() {
                            break;
                        }
                        idx += 1;
                        continue;
                    }
                    // 静态行: 拷贝显示数据避免借用冲突, 行尾挂「编辑」按钮
                    let (trigger, targets, note, turbo) = {
                        let m = &self.config.mappings[idx];
                        (
                            m.trigger_key.clone(),
                            m.target_keys.to_vec(),
                            m.note.clone(),
                            m.turbo_enabled,
                        )
                    };
                    let m_interval = self.config.mappings[idx].interval.unwrap_or(interval);
                    let m_duration = self.config.mappings[idx].event_duration.unwrap_or(duration);
                    render_mapping_row(
                        ui,
                        &th,
                        &trigger,
                        &targets,
                        m_interval,
                        m_duration,
                        turbo,
                        &note,
                        &mut |ui| {
                            /* 方向/滚动/连发/简易奔跑 全部收在「编辑」面板内 (v20.4 用户定稿:
                             * 不往静态行塞按钮); 点编辑即可配齐 */
                            if ui.add(th.secondary_button("编辑")).clicked() {
                                self.begin_mapping_edit(idx);
                            }
                        },
                    );
                    idx += 1;
                }
            }
        });
    }

    /* ═══════════════════ 连发映射内联编辑 (P8) ═══════════════════ */


    /// 新增一条映射并进入编辑态 (取消时自动删除)。
    /// ★v20.3: 插到列表**最前** (用户定稿: 新增映射的编辑面板必须显示在最前面),
    /// 并在下一帧滚动到编辑面板顶部。
    pub(super) fn add_new_mapping(&mut self) {
        self.config.mappings.insert(
            0,
            crate::config::KeyMapping {
                /* ★v24.3: 新建映射初始**无触发键** —— UI 显示「尚未捕获触发键」,
                 * 旧默认 "A" 会让人以为已经捕获了 A 键 (用户实测困惑点)。 */
                trigger_key: String::new(),
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
                note: String::new(),
            },
        );
        self.edit_mapping_is_new = true;
        self.edit_mapping_snapshot = None;
        self.edit_mapping_idx = Some(0);
        self.scroll_to_edit_row = true;
    }


    /// 进入已有映射的编辑态 (保存快照用于取消还原)。
    pub(super) fn begin_mapping_edit(&mut self, idx: usize) {
        if let Some(m) = self.config.mappings.get(idx) {
            self.edit_mapping_snapshot = Some(m.clone());
            self.edit_mapping_is_new = false;
            self.edit_mapping_idx = Some(idx);
        }
    }


    /// ★v20.3: 在连发页内联编辑行打开「鼠标方向」选择 (设置弹窗同款对话框)。
    pub(super) fn open_mouse_direction_dialog(&mut self, idx: usize) {
        self.mouse_direction_mapping_idx = Some(idx);
        self.mouse_direction_dialog = Some(
            crate::gui::mouse_direction_dialog::MouseDirectionDialog::new(),
        );
    }

    /// ★v20.3: 在连发页内联编辑行打开「鼠标滚动」选择 (设置弹窗同款对话框)。
    pub(super) fn open_mouse_scroll_dialog(&mut self, idx: usize) {
        self.mouse_scroll_mapping_idx = Some(idx);
        self.mouse_scroll_dialog = Some(
            crate::gui::mouse_scroll_dialog::MouseScrollDialog::new(),
        );
    }

    /// ★v20.3: 按预设名切换连发预设 (热键/下拉共用; 切换即落盘 + 热重载 + 同步弹窗暂存)。
    pub(super) fn switch_to_turbo_preset(&mut self, name: &str) {
        if self.config.current_preset == name {
            return;
        }
        if let Some(pr) = self.config.presets.iter().find(|p| p.name == name) {
            self.config.current_preset = pr.name.clone();
            /* 空预设不覆盖 (防止把当前映射清空) */
            if !pr.mappings.is_empty() {
                self.config.mappings = pr.mappings.clone();
            }
            self.page_preset_delete_arm = false;
            let _ = self.config.save_to_file("Config.toml");
            if let Err(e) = self.app_state.reload_config(self.config.clone()) {
                eprintln!("Failed to reload config after preset switch: {}", e);
            }
            if let Some(tc) = &mut self.temp_config {
                tc.current_preset = self.config.current_preset.clone();
                tc.mappings = self.config.mappings.clone();
                tc.presets = self.config.presets.clone();
            }
        }
    }


    /// 行内编辑面板: 触发/目标/间隔/时长/连发/备注 + 保存/取消/删除。
    pub(super) fn render_mapping_edit_row(
        &mut self,
        ui: &mut egui::Ui,
        idx: usize,
        default_interval: u64,
        default_duration: u64,
    ) {
        let th = self.theme();
        let Some(mapping) = self.config.mappings.get_mut(idx) else {
            self.edit_mapping_idx = None;
            return;
        };

        // 局部可编辑副本 (变更即回写 config; 取消由快照还原)
        let trigger = mapping.trigger_key.clone();
        let targets: Vec<String> = mapping.target_keys.to_vec();
        let mut interval = mapping.interval.unwrap_or(default_interval) as f64;
        let mut duration = mapping.event_duration.unwrap_or(default_duration) as f64;
        let mut turbo = mapping.turbo_enabled;
        /* ★v21.5 简易奔跑 ↔ 重推奔跑互斥: 数据双真时重推奔跑优先 (引擎侧本就压制
         * 简易奔跑), 局部副本先按互斥取, 避免两项同时勾选后 UI 双双灰死 */
        let mut double_tap = mapping.double_tap_enabled && !mapping.run_enabled;
        /* ★v21.0 奔跑行局部副本 (二次敲击间隔与 简易奔跑共用 double_tap_gap_ms) */
        let mut run_enabled = mapping.run_enabled;
        let mut run_recheck = mapping.run_recheck;
        let mut run_threshold = mapping.run_threshold as f32;
        let mut run_gap = mapping.double_tap_gap_ms as f32;
        let mut move_speed = mapping.move_speed as f32;
        /* 滚动映射的移动速度上限更高 (与设置弹窗一致: 普通 100 / 滚动 1200) */
        let speed_hi = if targets.iter().any(|k| k.starts_with("SCROLL")) {
            1200.0
        } else {
            100.0
        };
        let mut note = mapping.note.clone();
        /* ★v21.5 互斥自愈: 历史数据双真时清掉简易奔跑 (重推奔跑优先), 防灰死锁 */
        if mapping.double_tap_enabled && mapping.run_enabled {
            self.config.mappings[idx].double_tap_enabled = false;
        }
        let mut remove_target: Option<usize> = None;
        let mut request_delete = false;

        egui::Frame::NONE
            .fill(th.card_alt)
            .stroke(egui::Stroke::new(1.0, th.accent_soft))
            .corner_radius(egui::CornerRadius::same(10))
            .inner_margin(egui::Margin::same(14))
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(format!("编辑映射 #{}", idx + 1))
                            .size(13.5)
                            .strong()
                            .color(th.heading),
                    );
                });
                ui.add_space(theme::SP_S);

                // 触发键
                ui.horizontal(|ui| {
                    ui.label(th.weak("触发键"));
                    /* ★v20.9: 键帽按设备类型上色 (紫=手柄 / 橙=鼠标 / 键盘=中性) */
                    /* ★v24.3: 还没捕获时**不画键帽**, 显示「尚未捕获触发键」(旧默认 "A" 会误导) */
                    if trigger.trim().is_empty() {
                        ui.label(th.hint_text("尚未捕获触发键"));
                    } else {
                        widgets::keycap_typed(ui, &th, &trigger, utils::key_kind(&trigger));
                    }
                    let capturing = matches!(
                        self.key_capture_mode,
                        KeyCaptureMode::MappingTrigger(i) if i == idx
                    );
                    let btn_text = if capturing {
                        "正在捕获触发键… (按下并松开)"
                    } else {
                        "捕获触发键"
                    };
                    if ui.add(th.secondary_button(btn_text)).clicked() && !capturing {
                        self.start_mapping_capture(idx, true);
                    }
                });
                ui.add_space(theme::SP_S);

                // 目标键
                ui.horizontal_wrapped(|ui| {
                    ui.label(th.weak("目标键"));
                    for (i, t) in targets.iter().enumerate() {
                        /* ★v20.5: 长名截断 35 字符 (悬停看全名), 行不溢出
                         * ★v20.9: chip 按设备类型上色 (键盘键保持天蓝目标色) */
                        let short = truncate_chars(t, 35);
                        let (fg, bg) = match utils::key_kind(t) {
                            utils::KeyKind::Gamepad => (th.gamepad_fg, th.gamepad_bg),
                            utils::KeyKind::Mouse => (th.mouse_fg, th.mouse_bg),
                            utils::KeyKind::Keyboard => (th.target_fg, th.target_bg),
                        };
                        let chip = th.badge_clickable(ui, &format!("{}  ✕", short), fg, bg);
                        if chip.clicked() {
                            remove_target = Some(i);
                        }
                        chip.on_hover_text(format!("{}\n点击移除该目标键", t));
                    }
                    if targets.is_empty() {
                        ui.label(th.hint_text("尚未设置目标键"));
                    }
                    let capturing = matches!(
                        self.key_capture_mode,
                        KeyCaptureMode::MappingTarget(i) if i == idx
                    );
                    let btn_text = if capturing {
                        "正在捕获目标键… (按下并松开)"
                    } else {
                        "＋ 添加目标"
                    };
                    if ui.add(th.secondary_button(btn_text)).clicked() && !capturing {
                        self.start_mapping_capture(idx, false);
                    }
                });
                ui.add_space(theme::SP_M);

                // 参数行
                ui.horizontal_wrapped(|ui| {
                    ui.label(th.weak("间隔"));
                    if ui
                        .add(egui::DragValue::new(&mut interval).range(1.0..=5000.0).speed(1.0))
                        .changed()
                    {
                        self.config.mappings[idx].interval = Some(interval.max(1.0) as u64);
                    }
                    ui.label(th.weak("ms"));
                    ui.add_space(theme::SP_L);
                    ui.label(th.weak("时长"));
                    if ui
                        .add(egui::DragValue::new(&mut duration).range(1.0..=5000.0).speed(1.0))
                        .changed()
                    {
                        self.config.mappings[idx].event_duration = Some(duration.max(1.0) as u64);
                    }
                    ui.label(th.weak("ms"));
                    ui.add_space(theme::SP_L);
                    if ui.checkbox(&mut turbo, "连发").changed() {
                        self.config.mappings[idx].turbo_enabled = turbo;
                    }
                    ui.add_space(theme::SP_L);
                    /* ★v20.3: 简易奔跑补齐到连发页 (原只在设置弹窗有)
                     * ★v21.5: 与重推奔跑互斥 —— 重推奔跑勾选时此项灰掉 (不允许勾选) */
                    ui.add_enabled_ui(!run_enabled, |ui| {
                        let resp = ui
                            .checkbox(&mut double_tap, "简易奔跑")
                            .on_hover_text(if run_enabled {
                                "已勾选「重推奔跑」, 两者互斥 —— 取消重推奔跑后可勾选"
                            } else {
                                "按一次自动补一次敲击 (DNF 简易双击跑, 键盘/手柄键均可); \
                                 与设置弹窗里的「简易奔跑」是同一开关"
                            });
                        if resp.changed() {
                            self.config.mappings[idx].double_tap_enabled = double_tap;
                        }
                    });
                });
                ui.add_space(theme::SP_S);

                /* ★v20.3: 鼠标方向/滚动/移动速度补齐到连发页 (原只在设置弹窗有) */
                ui.horizontal_wrapped(|ui| {
                    ui.label(th.weak("鼠标"));
                    if ui
                        .add(th.secondary_button("⌖ 方向"))
                        .on_hover_text("把鼠标方向 (如 MOUSE_UP_LEFT) 加入目标键")
                        .clicked()
                    {
                        self.open_mouse_direction_dialog(idx);
                    }
                    if ui
                        .add(th.secondary_button("🎡 滚动"))
                        .on_hover_text("把鼠标滚动 (SCROLL_UP/DOWN/MBUTTON) 加入目标键")
                        .clicked()
                    {
                        self.open_mouse_scroll_dialog(idx);
                    }
                    ui.add_space(theme::SP_L);
                    ui.label(th.weak("移动速度"));
                    if ui
                        .add(
                            egui::DragValue::new(&mut move_speed)
                                .range(1.0..=speed_hi)
                                .speed(1.0),
                        )
                        .on_hover_text(format!(
                            "方向/滚动映射的每步移动量 (px/步)\n当前映射上限 {}",
                            speed_hi as i32
                        ))
                        .changed()
                    {
                        self.config.mappings[idx].move_speed = move_speed.round().max(1.0) as i32;
                    }
                });
                ui.add_space(theme::SP_S);

                /* ★v21.0 重推奔跑: 轻推=走(方向键按住) / 推过重推阈值=自动补一次
                 * 松开再按下 (游戏判定双击→奔跑)。勾选后本条的 连发/简易奔跑 不生效。 */
                ui.horizontal_wrapped(|ui| {
                    ui.label(th.weak("重推奔跑"));
                    /* ★v21.5: 与简易奔跑互斥 —— 简易奔跑勾选时此项灰掉 (不允许勾选) */
                    ui.add_enabled_ui(!double_tap, |ui| {
                        let resp = ui
                            .checkbox(&mut run_enabled, "")
                            .on_hover_text(if double_tap {
                                "已勾选「简易奔跑」, 两者互斥 —— 取消简易奔跑后可勾选"
                            } else {
                                "重推奔跑 (仅摇杆方向映射有效):\n\
                                 轻推摇杆 = 方向键按住 (走路)\n\
                                 推过「重推阈值」= 自动补一次松开再按下 → 游戏判定双击 → 奔跑\n\
                                 勾选后本条映射的 连发/简易奔跑 不生效 (奔跑改写按键节奏)"
                            });
                        if resp.changed() {
                            self.config.mappings[idx].run_enabled = run_enabled;
                        }
                    });
                    if run_enabled {
                        ui.add_space(theme::SP_L);
                        ui.label(th.weak("重推阈值"));
                        if ui
                            .add(
                                egui::DragValue::new(&mut run_threshold)
                                    .range(50.0..=95.0)
                                    .suffix("%")
                                    .speed(1.0),
                            )
                            .on_hover_text(
                                "摇杆推过多大幅度算「重推」(满量的 %)\n\
                                 轻推就误触奔跑 → 调高; 推到底还不跑 → 调低",
                            )
                            .changed()
                        {
                            self.config.mappings[idx].run_threshold =
                                run_threshold.round().clamp(50.0, 95.0) as u8;
                        }
                        ui.add_space(theme::SP_L);
                        ui.label(th.weak("二次敲击间隔"));
                        if ui
                            .add(
                                egui::DragValue::new(&mut run_gap)
                                    .range(20.0..=200.0)
                                    .suffix("ms")
                                    .speed(1.0),
                            )
                            .on_hover_text(
                                "双重作用: ① 急推判定窗口 (在这么短时间内推过重推线才算「瞬间推入」)\n\
                                 ② 走→跑 切换时两次敲击之间的等待 (与 简易奔跑 共用)\n\
                                 缓推被误判成奔跑 → 调小; 游戏判定不出双击 → 调大",
                            )
                            .changed()
                        {
                            self.config.mappings[idx].double_tap_gap_ms =
                                run_gap.round().clamp(20.0, 200.0) as u64;
                        }
                        ui.add_space(theme::SP_L);
                        /* ★v21.1 重推阈值再检测: 完整双击序列 (DNF 实测定案) */
                        if ui
                            .checkbox(&mut run_recheck, "再检测")
                            .on_hover_text(
                                "重推阈值再检测 (推荐开启) —— 只认「瞬间推入」:\n\
                                 缓慢推进越过重推线 = 继续走路 (只是想走深一点, 不触发)\n\
                                 在二次敲击间隔内从轻推区猛冲过线 = 明确的奔跑意图 → 模拟完整双击\n\
                                 (松开 → 敲一下 → 再敲一下并保持) → 游戏判定双击 → 奔跑\n\
                                 判定窗口 = 「二次敲击间隔」; 关闭则退回「推过线就补一次松按」的旧行为",
                            )
                            .changed()
                        {
                            self.config.mappings[idx].run_recheck = run_recheck;
                        }
                    }
                });
                ui.add_space(theme::SP_S);

                // 备注
                ui.horizontal(|ui| {
                    ui.label(th.weak("备注"));
                    ui.add(
                        egui::TextEdit::singleline(&mut note)
                            .desired_width(ui.available_width() - 40.0)
                            .hint_text("给这条映射加个备注 (可选)"),
                    );
                });
                ui.add_space(theme::SP_M);

                // 保存/取消 + 删除 (★v21.4: 删除从标题行右端移到底部最右端, 防误触)
                ui.horizontal(|ui| {
                    /* ★v24.3: 未捕获触发键时不允许保存 (避免落一条永远不触发的空映射) */
                    let can_save = !trigger.trim().is_empty();
                    if ui
                        .add_enabled(can_save, th.primary_button("保存修改"))
                        .on_disabled_hover_text("请先点「捕获触发键」设一个触发键")
                        .clicked()
                    {
                        self.config.mappings[idx].note = note.clone();
                        let _ = self.config.save_to_file("Config.toml");
                        if let Err(e) = self.app_state.reload_config(self.config.clone()) {
                            eprintln!("Failed to reload config after mapping edit: {}", e);
                        }
                        self.edit_mapping_idx = None;
                        self.edit_mapping_snapshot = None;
                        self.edit_mapping_is_new = false;
                    }
                    ui.add_space(theme::SP_S);
                    if ui.add(th.secondary_button("取消")).clicked() {
                        if self.edit_mapping_is_new {
                            self.config.mappings.remove(idx);
                        } else if let Some(snap) = self.edit_mapping_snapshot.take() {
                            if let Some(m) = self.config.mappings.get_mut(idx) {
                                *m = snap;
                            }
                        }
                        self.edit_mapping_idx = None;
                        self.edit_mapping_is_new = false;
                    }
                    /* 删除推到操作行最右端: 与保存/取消拉开距离,
                     * 消除"点完编辑按钮后同位置再点即误删"的隐患 */
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add(th.danger_button("删除该映射"))
                            .on_hover_text("删除这条映射 (不可恢复)")
                            .clicked()
                        {
                            request_delete = true;
                        }
                    });
                });
            });

        // 移除目标键 (需在 Frame 闭包外执行, 避免借用冲突)
        if let Some(i) = remove_target {
            if i < self.config.mappings[idx].target_keys.len() {
                let key = self.config.mappings[idx].target_keys[i].clone();
                self.config.mappings[idx].remove_target_key(&key);
            }
        }
        // 回写文本字段
        if self.edit_mapping_idx == Some(idx) {
            if self.config.mappings[idx].trigger_key != trigger {
                self.config.mappings[idx].trigger_key = trigger;
            }
            if self.config.mappings[idx].note != note {
                self.config.mappings[idx].note = note;
            }
        }
        // 删除请求
        if request_delete && self.edit_mapping_idx == Some(idx) {
            self.config.mappings.remove(idx);
            self.edit_mapping_idx = None;
            self.edit_mapping_snapshot = None;
            self.edit_mapping_is_new = false;
            let _ = self.config.save_to_file("Config.toml");
            if let Err(e) = self.app_state.reload_config(self.config.clone()) {
                eprintln!("Failed to reload config after mapping delete: {}", e);
            }
        }
    }


    /// 开始内联编辑的按键捕获 (触发键含手柄原始输入, 目标键含鼠标)。
    pub(super) fn start_mapping_capture(&mut self, idx: usize, is_trigger: bool) {
        self.key_capture_mode = if is_trigger {
            KeyCaptureMode::MappingTrigger(idx)
        } else {
            KeyCaptureMode::MappingTarget(idx)
        };
        self.capture_pressed_keys.clear();
        self.capture_initial_pressed = Self::poll_all_pressed_keys();
        if is_trigger {
            self.app_state.set_raw_input_capture_mode(true);
        }
        self.just_captured_input = true;
    }


    /// 内联编辑的捕获轮询 (设置弹窗关闭时由 update 调用)。
    pub(super) fn handle_turbo_edit_capture(&mut self, ctx: &egui::Context) {
        let (idx, is_trigger) = match self.key_capture_mode {
            KeyCaptureMode::MappingTrigger(i) => (i, true),
            KeyCaptureMode::MappingTarget(i) => (i, false),
            _ => return,
        };
        if self.edit_mapping_idx != Some(idx) {
            return;
        }

        // Esc 取消捕获
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.key_capture_mode = KeyCaptureMode::None;
            self.capture_pressed_keys.clear();
            self.app_state.set_raw_input_capture_mode(false);
            return;
        }

        let mut captured: Option<String> = None;

        // 键盘 (触发与目标均可)
        let current_pressed = Self::poll_all_pressed_keys();
        current_pressed
            .iter()
            .filter(|&&vk| !self.capture_initial_pressed.contains(&vk))
            .for_each(|&vk| {
                self.capture_pressed_keys.insert(vk);
            });
        let any_released = self
            .capture_pressed_keys
            .iter()
            .any(|vk| !current_pressed.contains(vk));
        if any_released {
            captured = Self::format_captured_keys(&self.capture_pressed_keys);
        }

        // 鼠标 (仅目标键)
        if captured.is_none() && !is_trigger && !self.just_captured_input {
            ctx.input(|i| {
                captured = if i.pointer.button_clicked(egui::PointerButton::Primary) {
                    Some("LBUTTON".to_string())
                } else if i.pointer.button_clicked(egui::PointerButton::Secondary) {
                    Some("RBUTTON".to_string())
                } else if i.pointer.button_clicked(egui::PointerButton::Middle) {
                    Some("MBUTTON".to_string())
                } else if i.pointer.button_clicked(egui::PointerButton::Extra1) {
                    Some("XBUTTON1".to_string())
                } else if i.pointer.button_clicked(egui::PointerButton::Extra2) {
                    Some("XBUTTON2".to_string())
                } else {
                    None
                };
            });
        }

        // 手柄原始输入 (仅触发键)
        if captured.is_none() && is_trigger {
            if let Some(device) = self.app_state.try_recv_raw_input_capture() {
                captured = Some(device.to_string());
            }
        }

        if let Some(name) = captured {
            if is_trigger {
                /* ★v21.7d: 触发键组合去重+规范排序 (上+上+空格 → 上+空格; 下+上+空格 → 上+下+空格) */
                self.config.mappings[idx].trigger_key = crate::util::normalize_key_combo(&name);
            } else {
                self.config.mappings[idx].add_target_key(name);
            }
            self.key_capture_mode = KeyCaptureMode::None;
            self.capture_pressed_keys.clear();
            self.app_state.set_raw_input_capture_mode(false);
            self.just_captured_input = false;
            // 捕获即持久化 (与手柄页快速捕获一致)
            let _ = self.config.save_to_file("Config.toml");
            if let Err(e) = self.app_state.reload_config(self.config.clone()) {
                eprintln!("Failed to reload config after mapping capture: {}", e);
            }
        } else if self.just_captured_input {
            self.just_captured_input = false;
        }
    }

    /* ═══════════════════ 极简模式 (P8) ═══════════════════ */


    /// 全局参数卡。
    /// 全局配置卡 (★v20.5: 从只读改为**可编辑** —— 玩家在映射页直接改, 不用跑设置弹窗)。
    /// 数值改动即保存 + 热重载; 置顶切换实时作用于窗口。
    pub(super) fn render_turbo_params(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let th = self.theme();
        /* 标签先克隆: 避免闭包内 &self.translations 与 &mut self.config 冲突 */
        let lbl_timeout = self.translations.input_timeout_display().to_owned();
        let lbl_interval = self.translations.default_interval_display().to_owned();
        let lbl_duration = self.translations.default_duration_display().to_owned();
        let lbl_tray = self.translations.show_tray_icon_display().to_owned();
        let lbl_notif = self.translations.show_notifications_display().to_owned();
        let lbl_top = self.translations.always_on_top_display().to_owned();
        let mut dirty = false;
        let mut top_changed: Option<bool> = None;
        th.card(ui, Some("全局配置"), |ui| {
            egui::Grid::new("turbo_params_grid")
                .num_columns(2)
                .spacing([theme::SP_L, theme::SP_S])
                .min_col_width(ui.available_width() * 0.42)
                .striped(false)
                .show(ui, |ui| {
                    let mut v = self.config.input_timeout as f64;
                    ui.label(th.weak(&lbl_timeout));
                    if ui
                        .add(
                            egui::DragValue::new(&mut v)
                                .range(1.0..=2000.0)
                                .speed(1.0)
                                .suffix(" ms"),
                        )
                        .on_hover_text("按键输入超时/去抖 (1-2000 ms)")
                        .changed()
                    {
                        self.config.input_timeout = (v.round().max(1.0) as u64).clamp(1, 2000);
                        dirty = true;
                    }
                    ui.end_row();

                    let mut v = self.config.interval as f64;
                    ui.label(th.weak(&lbl_interval));
                    if ui
                        .add(
                            egui::DragValue::new(&mut v)
                                .range(1.0..=5000.0)
                                .speed(1.0)
                                .suffix(" ms"),
                        )
                        .on_hover_text("连发按键的默认重复间隔 (单条映射可在编辑里覆盖)")
                        .changed()
                    {
                        self.config.interval = (v.round().max(1.0) as u64).max(1);
                        dirty = true;
                    }
                    ui.end_row();

                    let mut v = self.config.event_duration as f64;
                    ui.label(th.weak(&lbl_duration));
                    if ui
                        .add(
                            egui::DragValue::new(&mut v)
                                .range(1.0..=5000.0)
                                .speed(1.0)
                                .suffix(" ms"),
                        )
                        .on_hover_text("每次按键的默认按压时长 (单条映射可在编辑里覆盖)")
                        .changed()
                    {
                        self.config.event_duration = (v.round().max(1.0) as u64).max(1);
                        dirty = true;
                    }
                    ui.end_row();

                    ui.label(th.weak(&lbl_tray));
                    let mut flag = self.config.show_tray_icon;
                    if ui
                        .add(egui::Checkbox::new(&mut flag, ""))
                        .on_hover_text("显示系统托盘图标")
                        .changed()
                    {
                        self.config.show_tray_icon = flag;
                        dirty = true;
                    }
                    ui.end_row();

                    ui.label(th.weak(&lbl_notif));
                    let mut flag = self.config.show_notifications;
                    if ui
                        .add(egui::Checkbox::new(&mut flag, ""))
                        .on_hover_text("显示系统通知")
                        .changed()
                    {
                        self.config.show_notifications = flag;
                        dirty = true;
                    }
                    ui.end_row();

                    ui.label(th.weak(&lbl_top));
                    let mut flag = self.config.always_on_top;
                    if ui
                        .add(egui::Checkbox::new(&mut flag, ""))
                        .on_hover_text("窗口置顶 (即时生效)")
                        .changed()
                    {
                        self.config.always_on_top = flag;
                        top_changed = Some(flag);
                        dirty = true;
                    }
                    ui.end_row();
                });
            ui.add_space(theme::SP_XS);
            ui.label(th.hint_text("改动即时生效并自动保存 (拖动数值 / 勾选开关)"));
        });
        if dirty {
            let _ = self.config.save_to_file("Config.toml");
            if let Err(e) = self.app_state.reload_config(self.config.clone()) {
                eprintln!("Failed to reload config after global params edit: {}", e);
            }
            if let Some(top) = top_changed {
                ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(if top {
                    egui::WindowLevel::AlwaysOnTop
                } else {
                    egui::WindowLevel::Normal
                }));
            }
        }
    }


    /// 参数行: 标签 | 值 (网格内)。
    #[allow(dead_code)]
    pub(super) fn param_row(ui: &mut egui::Ui, th: &Theme, label: &str, value: &str) {
        ui.label(th.weak(label));
        ui.label(egui::RichText::new(value).size(13.0).strong().color(th.accent_text));
        ui.end_row();
    }


    /// 布尔参数行: 标签 | 状态徽章 (网格内)。
    #[allow(dead_code)]
    pub(super) fn param_flag(ui: &mut egui::Ui, th: &Theme, label: &str, on: bool) {
        ui.label(th.weak(label));
        let (text, fg, bg) = if on {
            ("开启", th.good, th.good_soft)
        } else {
            ("关闭", th.hint, th.faint)
        };
        th.badge(ui, text, fg, bg);
        ui.end_row();
    }
}


/// 单条映射行: 触发键帽 → 目标键帽 | 间隔/时长 | Turbo | 备注 | 行尾动作。
#[allow(clippy::too_many_arguments)]
fn render_mapping_row(
    ui: &mut egui::Ui,
    th: &Theme,
    trigger: &str,
    targets: &[String],
    interval: u64,
    duration: u64,
    turbo: bool,
    note: &str,
    extra: &mut dyn FnMut(&mut egui::Ui),
) {
    ui.horizontal(|ui| {
        // 行底: hover 高亮由 egui 默认; 圆角行容器
        egui::Frame::NONE
            .fill(if ui.ui_contains_pointer() { th.faint } else { egui::Color32::TRANSPARENT })
            .corner_radius(egui::CornerRadius::same(theme::RADIUS_CTRL))
            .inner_margin(egui::Margin::symmetric(10, 7))
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.horizontal(|ui| {
                    // 触发键帽 (多键拆分; ★v20.5 长名截断 20 字符防溢出, 悬停看全名 —— 老宿主口径)
                    // ★v20.9: 按设备类型上色 (紫=手柄 / 橙=鼠标 / 键盘=中性)
                    /* ★v24.3: 未捕获触发键时不画空键帽 */
                    if trigger.trim().is_empty() {
                        ui.label(th.hint_text("尚未捕获触发键"));
                    } else {
                        for part in trigger.split('+') {
                            let short = truncate_chars(part, 20);
                            widgets::keycap_typed(ui, th, &short, utils::key_kind(part))
                                .on_hover_text(part.to_string());
                        }
                    }
                    // 指向箭头
                    ui.label(egui::RichText::new("→").size(12.0).color(th.hint));
                    // 目标键帽 (★v20.5 长名截断 35 字符, 悬停看全名)
                    if targets.is_empty() {
                        ui.label(th.hint_text("(未设置目标)"));
                    } else {
                        for t in targets {
                            let short = truncate_chars(t, 35);
                            widgets::keycap_typed(ui, th, &short, utils::key_kind(t))
                                .on_hover_text(t.clone());
                        }
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        extra(ui);
                        ui.add_space(theme::SP_M);
                        // 备注
                        if note.is_empty() {
                            ui.label(th.hint_text("-"));
                        } else {
                            ui.label(
                                egui::RichText::new(truncate_chars(note, 14))
                                    .size(11.5)
                                    .color(th.hint),
                            )
                            .on_hover_text(note);
                        }
                        ui.add_space(theme::SP_M);
                        // Turbo 徽章
                        if turbo {
                            th.badge(ui, "TURBO", th.accent, th.accent_soft);
                        } else {
                            th.badge(ui, "单发", th.hint, th.faint);
                        }
                        ui.add_space(theme::SP_M);
                        ui.label(
                            egui::RichText::new(format!("{interval}ms / {duration}ms"))
                                .size(11.5)
                                .color(th.text_weak),
                        )
                        .on_hover_text(format!("间隔 {interval}ms / 时长 {duration}ms"));
                    });
                });
            });
    });
}

#[cfg(test)]
mod preset_switch_conflict_tests {
    use super::SorahkGui;
    use crate::config::{KeyMapping, Preset};

    fn mapping(trigger: &str, turbo: bool) -> KeyMapping {
        KeyMapping {
            trigger_key: trigger.to_string(),
            target_keys: smallvec::SmallVec::from_vec(vec!["A".to_string()]),
            interval: None,
            event_duration: None,
            turbo_enabled: turbo,
            move_speed: 5,
            double_tap_enabled: false,
            double_tap_gap_ms: 80,
            run_enabled: false,
            run_threshold: 80,
            run_recheck: true,
            note: String::new(),
        }
    }

    fn preset(name: &str, mappings: Vec<KeyMapping>) -> Preset {
        Preset {
            name: name.to_string(),
            mappings,
            switch_key: String::new(),
        }
    }

    #[test]
    fn exact_turbo_mapping_conflicts_and_flags_turbo() {
        let presets = vec![preset("P1", vec![mapping("F6", true)])];
        let hit = SorahkGui::find_preset_switch_conflict_in(&presets, "f6");
        assert_eq!(hit, Some(("P1".into(), "F6".into(), true)));
    }

    #[test]
    fn exact_plain_mapping_conflicts_without_turbo_flag() {
        let presets = vec![preset("P1", vec![mapping("F6", false)])];
        let hit = SorahkGui::find_preset_switch_conflict_in(&presets, "F6");
        assert_eq!(hit, Some(("P1".into(), "F6".into(), false)));
    }

    #[test]
    fn component_of_keyboard_combo_conflicts() {
        // 映射 CTRL+F6 与切换键 F6: 单按 F6 也会切预设 → 必须视为冲突
        let presets = vec![preset("P1", vec![mapping("CTRL+F6", false)])];
        let hit = SorahkGui::find_preset_switch_conflict_in(&presets, "F6");
        assert_eq!(hit, Some(("P1".into(), "CTRL+F6".into(), true)));
    }

    #[test]
    fn gamepad_combo_component_conflicts() {
        let presets = vec![preset("P1", vec![mapping("GAMEPAD_045E_A+B", false)])];
        // 组件 = A / B? 触发键整串按 '+' 切成 ["GAMEPAD_045E_A", "B"]
        let hit = SorahkGui::find_preset_switch_conflict_in(&presets, "GAMEPAD_045E_A");
        assert_eq!(hit, Some(("P1".into(), "GAMEPAD_045E_A+B".into(), true)));
    }

    #[test]
    fn gamepad_combo_does_not_falsely_match_plain_letter() {
        // 切换键 "A"/"B" 不应与手柄组合键 "GAMEPAD_045E_A+B" 冲突
        // (键盘 B ≠ 手柄 B; 组件匹配必须同类输入)
        let presets = vec![preset("P1", vec![mapping("GAMEPAD_045E_A+B", false)])];
        assert_eq!(SorahkGui::find_preset_switch_conflict_in(&presets, "A"), None);
        assert_eq!(SorahkGui::find_preset_switch_conflict_in(&presets, "B"), None);
        // 但手柄侧的组件 (GAMEPAD_045E_A) 必须冲突
        assert!(SorahkGui::find_preset_switch_conflict_in(&presets, "GAMEPAD_045E_A").is_some());
    }

    #[test]
    fn no_conflict_returns_none() {
        let presets = vec![preset("P1", vec![mapping("Q", true), mapping("W", false)])];
        assert_eq!(SorahkGui::find_preset_switch_conflict_in(&presets, "F6"), None);
    }

    #[test]
    fn empty_key_never_conflicts() {
        let presets = vec![preset("P1", vec![mapping("F6", true)])];
        assert_eq!(SorahkGui::find_preset_switch_conflict_in(&presets, "   "), None);
    }

    #[test]
    fn searches_all_presets_not_just_first() {
        let presets = vec![
            preset("P1", vec![mapping("Q", true)]),
            preset("P2", vec![mapping("GAMEPAD_20BC_A", true)]),
        ];
        let hit = SorahkGui::find_preset_switch_conflict_in(&presets, "gamepad_20bc_a");
        assert_eq!(hit, Some(("P2".into(), "GAMEPAD_20BC_A".into(), true)));
    }

    /* ── ★v21.7c 切换键标签分解/合成 (组合键构建器) ── */

    #[test]
    fn decombo_splits_keyboard_and_carries_gamepad_prefix() {
        assert_eq!(
            SorahkGui::decombo_switch_key("CTRL+F6"),
            vec!["CTRL".to_string(), "F6".to_string()]
        );
        assert_eq!(
            SorahkGui::decombo_switch_key("GAMEPAD_045E_A+B"),
            vec!["GAMEPAD_045E_A".to_string(), "GAMEPAD_045E_B".to_string()]
        );
        // 语义名 (含 DEV 段) 本身就是一个标签
        assert_eq!(
            SorahkGui::decombo_switch_key("GAMEPAD_20BC_5159_DEV12345678_H1"),
            vec!["GAMEPAD_20BC_5159_DEV12345678_H1".to_string()]
        );
    }

    #[test]
    fn compact_merges_gamepad_buttons_and_puts_modifiers_first() {
        // compact 保持输入顺序 (排序职责在 normalize_*); 同手柄多键就地合并
        assert_eq!(
            SorahkGui::compact_switch_key_parts(&["F6".into(), "CTRL".into()]),
            "F6+CTRL"
        );
        assert_eq!(
            crate::util::normalize_key_combo("F6+CTRL"),
            "CTRL+F6"
        );
        assert_eq!(
            SorahkGui::compact_switch_key_parts(&[
                "GAMEPAD_045E_A".into(),
                "GAMEPAD_045E_B".into()
            ]),
            "GAMEPAD_045E_A+B"
        );
    }

    #[test]
    fn combo_parts_round_trip_through_compact_decombo() {
        for original in [
            "CTRL+F6",
            "GAMEPAD_045E_A+B",
            "GAMEPAD_20BC_5159_DEV12345678_H1",
            "F6",
        ] {
            let parts = SorahkGui::decombo_switch_key(original);
            assert_eq!(SorahkGui::compact_switch_key_parts(&parts), original, "round-trip {original}");
            // 合成的结果必须能被解析器接受 (否则保存会被拒)
            assert!(
                crate::state::AppState::is_valid_input_name(&SorahkGui::compact_switch_key_parts(&parts)),
                "compact 结果必须合法: {original}"
            );
        }
    }

    #[test]
    fn decombo_rejects_garbage_but_never_panics() {
        assert!(SorahkGui::decombo_switch_key("").is_empty());
        assert!(SorahkGui::decombo_switch_key("++").is_empty());
        // 汉字等非法内容不会被 compact 变成合法键 (保存时再校验)
        let parts = SorahkGui::decombo_switch_key("测试");
        assert_eq!(SorahkGui::compact_switch_key_parts(&parts), "测试");
        assert!(!crate::state::AppState::is_valid_input_name("测试"));
    }
}
