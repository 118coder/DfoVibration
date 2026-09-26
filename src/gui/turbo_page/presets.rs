//! 连发预设管理卡: 切换/保存/重命名/删除 + 预设切换键捕获 (v24.31) —— 原 30-592, C4 归位。

use crate::gui::SorahkGui;
use crate::gui::theme::{self};
use crate::gui::types::KeyCaptureMode;

use eframe::egui;
impl SorahkGui {
    pub(in crate::gui) fn render_turbo_preset_manager(&mut self, ui: &mut egui::Ui) {
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
                        /* ★v5: 武装态用深红 btn_danger (白字 4.7:1); rose-400 浅底压白字只有 2.4:1 */
                        egui::RichText::new(del_label).size(13.0).color(if self.page_preset_delete_arm {
                            egui::Color32::WHITE
                        } else {
                            th.btn_secondary_text
                        }),
                    )
                    .fill(if self.page_preset_delete_arm { th.btn_danger } else { th.faint })
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
                                /* ★v5: 绿是浅色实底 → 近黑文字 */
                                egui::RichText::new("✓ 确认").size(13.0).color(th.on_emphasis),
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
    pub(in crate::gui) fn save_preset_switch_key(&mut self) {
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
    pub(in crate::gui) fn decombo_switch_key(key: &str) -> Vec<String> {
        crate::util::key_combo_parts(key)
    }

    /// ★v21.7c: 标签列表 → 已存切换键字符串 (正向着色)。
    /// 同一手柄的多个按键合并成 `GAMEPAD_<VID>_A+B`; 修饰键 (CTRL/SHIFT/ALT) 前置。
    pub(in crate::gui) fn compact_switch_key_parts(parts: &[String]) -> String {
        crate::util::compact_key_combo(parts)
    }

    /// ★v21.7: 在全部预设的全部映射里查与 `key` 冲突的触发键。
    /// 判定 = 完全相同 **或** 作为 `+` 组合键的任一组件 (如映射 CTRL+F6 与切换键 F6 也冲突)。
    /// 返回 (预设名, 触发键原文, 是否带连发/组合性质)。
    pub(super) fn find_preset_switch_conflict(&self, key: &str) -> Option<(String, String, bool)> {
        Self::find_preset_switch_conflict_in(&self.config.presets, key)
    }

    /// 纯函数版 (可单测): 冲突查找规则见上。
    pub(in crate::gui) fn find_preset_switch_conflict_in(
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
    pub(super) fn looks_like_gamepad_input(name_upper: &str) -> bool {
        name_upper.starts_with("GAMEPAD")
            || name_upper.starts_with("JOYSTICK")
            || name_upper.starts_with("HID_")
    }

    /// 组合键是否只有修饰键 (那样永远触发不了, 保存时应拦下)。
    pub(super) fn combo_is_modifier_only(key: &str) -> bool {
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
    pub(in crate::gui) fn start_preset_switch_capture(&mut self) {
        self.key_capture_mode = KeyCaptureMode::PresetSwitchKey;
        self.capture_pressed_keys.clear();
        self.capture_initial_pressed = Self::poll_all_pressed_keys();
        self.app_state.set_raw_input_capture_mode(true);
        self.just_captured_input = false;
        self.preset_key_error = None;
    }

    /// ★v21.7: 取消/结束预设切换键捕获。
    pub(in crate::gui) fn cancel_preset_switch_capture(&mut self) {
        self.key_capture_mode = KeyCaptureMode::None;
        self.capture_pressed_keys.clear();
        self.app_state.set_raw_input_capture_mode(false);
        self.just_captured_input = false;
    }

    /// ★v21.7: 预设切换键捕获轮询 (键盘: 松开即取; 手柄: RawInput/XInput 捕获通道)。
    pub(in crate::gui) fn handle_preset_switch_capture(&mut self, ctx: &egui::Context) {
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
    pub(in crate::gui) fn append_captured_switch_key(&mut self, captured: &str) {
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


}
