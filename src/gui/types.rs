//! GUI type definitions.

/// 主窗口页面 (左侧选项卡)。
///
/// 新增页面: 在此添加变体并加入 [`Page::TABS`], 选项卡栏与页面分发自动生效。
/// 顺序即选项卡顺序, 手柄映射放第一位 (用户最常用)。
/// ★经典模式不是页面 —— 它是与极简模式同级的**独立窗口** (见 gui/classic_mode.rs,
/// 标题栏【典】字进入, 布局/尺寸复刻老宿主 1.5 的 820×600 三页签)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    /// 🎮 手柄可视化快速映射
    Gamepad,
    /// 连发映射 (键盘/鼠标触发 → 目标键)
    Turbo,
    /// 白名单 (进程过滤管理)
    Whitelist,
    /// 通用型震动设定
    Vibration,
    /// 全职业预设 (震动)
    JobPresets,
}

impl Page {
    /// 选项卡从左到右的固定顺序。
    pub const TABS: &'static [Page] = &[Page::Gamepad, Page::Turbo, Page::Whitelist, Page::Vibration, Page::JobPresets];

    /// 导航文案 (左侧选项卡; 2026-09-08 用户定稿: 除手柄映射外带「修改」后缀)。
    pub fn label(self) -> &'static str {
        match self {
            Page::Gamepad => "手柄映射",
            Page::Turbo => "连发映射修改",
            Page::Whitelist => "白名单修改",
            Page::Vibration => "通用震动修改",
            Page::JobPresets => "全职业预设修改",
        }
    }

    /// 页面大标题 (内容页头部, 不带「修改」后缀)。
    pub fn title(self) -> &'static str {
        match self {
            Page::Gamepad => "手柄映射",
            Page::Turbo => "连发映射",
            Page::Whitelist => "白名单",
            Page::Vibration => "通用震动",
            Page::JobPresets => "全职业预设",
        }
    }

    /// 导航矢量图标。
    pub fn icon(self) -> crate::gui::widgets::Icon {
        match self {
            Page::Gamepad => crate::gui::widgets::Icon::Gamepad,
            Page::Turbo => crate::gui::widgets::Icon::Bolt,
            Page::Whitelist => crate::gui::widgets::Icon::Shield,
            Page::Vibration => crate::gui::widgets::Icon::Wave,
            Page::JobPresets => crate::gui::widgets::Icon::Wand,
        }
    }

    /// 选项卡下方的页面级提示语。
    pub fn hint(self) -> &'static str {
        match self {
            Page::Gamepad => "点击手柄按键设置映射, 修改后自动保存并立即生效",
            Page::Turbo => "连发映射与全局配置, 由切换键控制启停",
            Page::Whitelist => "只允许列表内进程使用连发, 防止误触其他程序",
            Page::Vibration => "震动参数实时生效, 进图自动驱动",
            Page::JobPresets => "按职业加载震动预设, 滑块修改自动保存到 JobVibration.toml",
        }
    }
}

/// Key capture mode for keyboard input handling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyCaptureMode {
    None,
    ToggleKey,
    MappingTrigger(usize),
    MappingTarget(usize),
    NewMappingTrigger,
    NewMappingTarget,
    /// ★v21.7 预设管理: 捕获「预设切换键」—— 键盘任意键 **或** 手柄任意键 (含原始 HID 报文)
    PresetSwitchKey,
}

/// ★v22.0 手柄映射页交互状态机 —— 手柄页交互的**唯一真相**。
///
/// 设计原则 (用户要求"一步一步来"):
/// - 每一步只呈现一个主操作, 用户不需要在多个入口之间判断;
/// - 「取消」永远只回退一层 (回到 [`GpFlow::Selected`]), **绝不**顺手关掉手柄识别;
/// - 手柄识别开关 (`app_state.live_hid_pad`) 与本状态机**解耦**: 识别中/未识别都不影响状态合法性。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GpFlow {
    /// 未选中槽位: 空状态 (引导先识别 / 点图上的键)。
    Idle,
    /// 已选中一个槽位, 浏览其现状。下一步 = 设键盘键 / 校键。
    Selected { slot: usize },
    /// 第 2 步: 等键盘按键; `keys` 为已捕获的组合键部件 (可「再加一个键」继续追加)。
    AwaitKb { slot: usize, keys: Vec<String> },
    /// 等手柄物理键 (校对该槽位是手柄上的哪个键)。
    AwaitPad { slot: usize },
    /// 第 3 步: 已捕获, 等用户点【确认】。
    ConfirmCapture {
        slot: usize,
        kind: GpCaptureKind,
        value: String,
    },
    /// 删除映射二次确认 (不可逆操作先确认)。
    ConfirmDelete { slot: usize },
    /// ★v24.4: 快速映射完成页 —— 走完「选键 → 设键盘键 → 确认」后的落点。
    /// 提示可继续按其他手柄键, 并提供【编辑本映射】回到本键第 1 步。
    MapDone { slot: usize },
    /// ★v22.4: 自动校准向导 —— 按固定顺序逐个提示用户按下一个键, 自动记录。
    /// `step` = 当前第几个, `total` = 总数 (状态机不依赖具体槽位表)。
    Calibrate { step: usize, total: usize },
}

/// 捕获类型: 触发键 (校键) / 目标键 (键盘映射)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpCaptureKind {
    Trigger,
    Target,
}

/// 手柄页状态机事件 —— UI 只发事件, 由 [`GpFlow::transition`] 决定下一状态。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GpEvent {
    /// 选中某个槽位 (点热区 / 识别态按下手柄键 / 点总览 chip)。
    SelectSlot(usize),
    /// 开始设键盘键 (进入第 2 步)。
    BeginSetKb,
    /// 开始校对手柄键位。
    BeginSetPad,
    /// 捕获到一个键盘/鼠标键名。
    KbCaptured(String),
    /// 捕获到一个手柄键名。
    PadCaptured(String),
    /// 在确认态点「再加一个键」→ 回第 2 步并保留已捕获值。
    AddAnotherKey,
    /// 确认写入 (落盘在 UI 层做, 状态机只记录状态)。
    ConfirmCapture,
    /// 请求删除该槽位映射。
    RequestDelete,
    /// 确认删除。
    ConfirmDelete,
    /// 取消: 只回退一层 (兼容 [`GpFlow::cancel`] 的语义)。
    Cancel,
    /// 停止识别 / 离开页面 → 回 `Idle`。
    Reset,
    /// ★v22.4: 开始引导式一键校准 (total = 需要用户按下的键数)。
    StartCalibration { total: usize },
    /// ★v22.4: 当前校准步完成 (已记录/已跳过) → 进入下一步或结束。
    CalibrationStepDone,
}

impl GpFlow {
    /// 当前槽位 (如有)。校准向导不绑定单一槽位 → None。
    pub fn slot(&self) -> Option<usize> {
        match self {
            GpFlow::Idle | GpFlow::Calibrate { .. } => None,
            GpFlow::Selected { slot }
            | GpFlow::AwaitKb { slot, .. }
            | GpFlow::AwaitPad { slot }
            | GpFlow::ConfirmCapture { slot, .. }
            | GpFlow::ConfirmDelete { slot }
            | GpFlow::MapDone { slot } => Some(*slot),
        }
    }

    /// 是否正在等输入 (捕获态) —— 离开页面时用它决定是否要中止捕获。
    pub fn is_capturing(&self) -> bool {
        matches!(
            self,
            GpFlow::AwaitKb { .. } | GpFlow::AwaitPad { .. } | GpFlow::Calibrate { .. }
        )
    }

    /// 是否处于需要独占输入的状态 (捕获/确认) —— 此时不响应"识别态按下手柄键自动改选"。
    /// ★v24.4: `MapDone` 不独占 —— 完成提示页仍要能"按其他手柄键继续映射"。
    pub fn is_busy(&self) -> bool {
        !matches!(
            self,
            GpFlow::Idle | GpFlow::Selected { .. } | GpFlow::MapDone { .. }
        )
    }

    /// 取消: 只回退一层。捕获/确认态回到 `Selected`; 其余保持不变。
    pub fn cancel(&self) -> GpFlow {
        match self {
            GpFlow::AwaitKb { slot, .. }
            | GpFlow::AwaitPad { slot }
            | GpFlow::ConfirmCapture { slot, .. }
            | GpFlow::ConfirmDelete { slot }
            | GpFlow::MapDone { slot } => GpFlow::Selected { slot: *slot },
            GpFlow::Calibrate { .. } => GpFlow::Idle,
            other => other.clone(),
        }
    }

    /// 纯函数状态转移 (非法事件 = 状态不变, 绝不 panic)。
    pub fn transition(&self, ev: GpEvent) -> GpFlow {
        use GpEvent as E;
        match ev {
            E::Reset => GpFlow::Idle,
            E::StartCalibration { total } => GpFlow::Calibrate { step: 0, total },
            E::CalibrationStepDone => match self {
                GpFlow::Calibrate { step, total } => {
                    if step + 1 < *total {
                        GpFlow::Calibrate {
                            step: step + 1,
                            total: *total,
                        }
                    } else {
                        GpFlow::Idle
                    }
                }
                _ => self.clone(),
            },
            E::SelectSlot(slot) => GpFlow::Selected { slot },
            E::BeginSetKb => match self.slot() {
                Some(slot) => GpFlow::AwaitKb {
                    slot,
                    keys: Vec::new(),
                },
                None => self.clone(),
            },
            E::BeginSetPad => match self.slot() {
                Some(slot) => GpFlow::AwaitPad { slot },
                None => self.clone(),
            },
            E::KbCaptured(k) => match self {
                GpFlow::AwaitKb { slot, keys } => {
                    let mut parts = keys.clone();
                    parts.push(k);
                    GpFlow::ConfirmCapture {
                        slot: *slot,
                        kind: GpCaptureKind::Target,
                        value: crate::util::normalize_key_combo(&parts.join("+")),
                    }
                }
                _ => self.clone(),
            },
            E::PadCaptured(v) => match self {
                GpFlow::AwaitPad { slot } => GpFlow::ConfirmCapture {
                    slot: *slot,
                    kind: GpCaptureKind::Trigger,
                    value: crate::util::normalize_key_combo(&v),
                },
                _ => self.clone(),
            },
            E::AddAnotherKey => match self {
                GpFlow::ConfirmCapture {
                    slot,
                    kind: GpCaptureKind::Target,
                    value,
                } => GpFlow::AwaitKb {
                    slot: *slot,
                    keys: vec![value.clone()],
                },
                _ => self.clone(),
            },
            E::ConfirmCapture => match self {
                /* ★v24.4: 确认后落到"完成页" (提示可继续按其他手柄键 + 【编辑本映射】),
                 * 而不是直接回 Selected —— 用户要求完成前三步后给出明确反馈与回退入口。 */
                GpFlow::ConfirmCapture { slot, .. } => GpFlow::MapDone { slot: *slot },
                _ => self.clone(),
            },
            E::RequestDelete => match self.slot() {
                Some(slot) => GpFlow::ConfirmDelete { slot },
                None => self.clone(),
            },
            E::ConfirmDelete => match self {
                GpFlow::ConfirmDelete { slot } => GpFlow::Selected { slot: *slot },
                _ => self.clone(),
            },
            E::Cancel => self.cancel(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_key_capture_mode_pattern_matching() {
        // Test extracting index from MappingTrigger variant
        let mode = KeyCaptureMode::MappingTrigger(7);

        match mode {
            KeyCaptureMode::MappingTrigger(idx) => {
                assert_eq!(idx, 7);
            }
            _ => panic!("Pattern matching failed"),
        }

        // Test extracting index from MappingTarget variant
        let mode = KeyCaptureMode::MappingTarget(42);

        match mode {
            KeyCaptureMode::MappingTarget(idx) => {
                assert_eq!(idx, 42);
            }
            _ => panic!("Pattern matching failed"),
        }
    }

    #[test]
    fn test_key_capture_mode_discriminant() {
        // Test that variants with different indices are distinguished
        let trigger_0 = KeyCaptureMode::MappingTrigger(0);
        let trigger_1 = KeyCaptureMode::MappingTrigger(1);
        let target_0 = KeyCaptureMode::MappingTarget(0);

        assert_ne!(
            trigger_0, trigger_1,
            "Different indices should not be equal"
        );
        assert_ne!(
            trigger_0, target_0,
            "Different variants should not be equal"
        );
    }

    // ── ★v22.0 手柄页状态机 (一步一步流) ────────────────────────────────

    #[test]
    fn gp_flow_step_by_step_happy_path() {
        let mut s = GpFlow::Idle;
        s = s.transition(GpEvent::SelectSlot(4));
        assert_eq!(s, GpFlow::Selected { slot: 4 });
        s = s.transition(GpEvent::BeginSetKb);
        assert_eq!(s, GpFlow::AwaitKb { slot: 4, keys: vec![] });
        s = s.transition(GpEvent::KbCaptured("Q".into()));
        assert_eq!(
            s,
            GpFlow::ConfirmCapture {
                slot: 4,
                kind: GpCaptureKind::Target,
                value: "Q".into()
            }
        );
        s = s.transition(GpEvent::ConfirmCapture);
        /* ★v24.4: 确认后落到"完成页" (提示 + 【编辑本映射】), 不再直接回 Selected */
        assert_eq!(s, GpFlow::MapDone { slot: 4 });
        // 完成页不独占输入 → 可直接按其他手柄键改选别的槽位
        assert!(!s.is_busy());
        s = s.transition(GpEvent::SelectSlot(7));
        assert_eq!(s, GpFlow::Selected { slot: 7 });
        // 【编辑本映射】= 回到本键第 1 步 (SelectSlot 自身槽位)
        s = GpFlow::MapDone { slot: 4 }.transition(GpEvent::SelectSlot(4));
        assert_eq!(s, GpFlow::Selected { slot: 4 });
        // 完成页取消 → 回该键第 1 步
        assert_eq!(
            GpFlow::MapDone { slot: 4 }.transition(GpEvent::Cancel),
            GpFlow::Selected { slot: 4 }
        );
    }

    #[test]
    fn gp_flow_cancel_only_goes_back_one_level_and_keeps_slot() {
        // 捕获态取消 → 回 Selected (保留槽位, 不退回 Idle)
        let s = GpFlow::AwaitKb { slot: 9, keys: vec![] };
        assert_eq!(s.transition(GpEvent::Cancel), GpFlow::Selected { slot: 9 });
        // 确认态取消 → 同样只回 Selected
        let s = GpFlow::ConfirmCapture {
            slot: 3,
            kind: GpCaptureKind::Trigger,
            value: "GAMEPAD_X".into(),
        };
        assert_eq!(s.transition(GpEvent::Cancel), GpFlow::Selected { slot: 3 });
        // 删除确认取消 → 回 Selected
        assert_eq!(
            GpFlow::ConfirmDelete { slot: 1 }.transition(GpEvent::Cancel),
            GpFlow::Selected { slot: 1 }
        );
    }

    #[test]
    fn gp_flow_trigger_capture_and_normalize() {
        let s = GpFlow::Selected { slot: 0 }.transition(GpEvent::BeginSetPad);
        assert_eq!(s, GpFlow::AwaitPad { slot: 0 });
        // 规范化: F6+CTRL → CTRL+F6
        let s = s.transition(GpEvent::PadCaptured("F6+CTRL".into()));
        assert_eq!(
            s,
            GpFlow::ConfirmCapture {
                slot: 0,
                kind: GpCaptureKind::Trigger,
                value: "CTRL+F6".into()
            }
        );
    }

    #[test]
    fn gp_flow_add_another_key_builds_combo() {
        let s = GpFlow::ConfirmCapture {
            slot: 7,
            kind: GpCaptureKind::Target,
            value: "CTRL".into(),
        };
        let s = s.transition(GpEvent::AddAnotherKey);
        assert_eq!(
            s,
            GpFlow::AwaitKb {
                slot: 7,
                keys: vec!["CTRL".into()]
            }
        );
        let s = s.transition(GpEvent::KbCaptured("F6".into()));
        assert_eq!(
            s,
            GpFlow::ConfirmCapture {
                slot: 7,
                kind: GpCaptureKind::Target,
                value: "CTRL+F6".into()
            }
        );
        // 再加一个键只对目标键成立 (触发键不可组合)
        let trig = GpFlow::ConfirmCapture {
            slot: 7,
            kind: GpCaptureKind::Trigger,
            value: "A".into(),
        };
        assert_eq!(trig.transition(GpEvent::AddAnotherKey), trig);
    }

    #[test]
    fn gp_flow_illegal_events_are_noops() {
        // 未选中时不能开始捕获
        assert_eq!(
            GpFlow::Idle.transition(GpEvent::BeginSetKb),
            GpFlow::Idle
        );
        // 不在捕获态时 KbCaptured 无效
        assert_eq!(
            GpFlow::Selected { slot: 2 }.transition(GpEvent::KbCaptured("Q".into())),
            GpFlow::Selected { slot: 2 }
        );
        // Reset 永远回 Idle
        assert_eq!(
            GpFlow::AwaitKb { slot: 5, keys: vec![] }.transition(GpEvent::Reset),
            GpFlow::Idle
        );
    }

    #[test]
    fn gp_flow_busy_flag_protects_confirmation() {
        assert!(!GpFlow::Idle.is_busy());
        assert!(!GpFlow::Selected { slot: 1 }.is_busy());
        assert!(GpFlow::AwaitKb { slot: 1, keys: vec![] }.is_busy());
        assert!(GpFlow::AwaitPad { slot: 1 }.is_busy());
        assert!(GpFlow::ConfirmDelete { slot: 1 }.is_busy());
    }

    // ── ★v22.4 引导式一键校准 ──────────────────────────────────────────

    #[test]
    fn gp_flow_calibration_wizard_advances_and_finishes() {
        let mut s = GpFlow::Idle;
        s = s.transition(GpEvent::StartCalibration { total: 3 });
        assert_eq!(s, GpFlow::Calibrate { step: 0, total: 3 });
        s = s.transition(GpEvent::CalibrationStepDone);
        assert_eq!(s, GpFlow::Calibrate { step: 1, total: 3 });
        s = s.transition(GpEvent::CalibrationStepDone);
        assert_eq!(s, GpFlow::Calibrate { step: 2, total: 3 });
        // 最后一步完成 → 回 Idle
        s = s.transition(GpEvent::CalibrationStepDone);
        assert_eq!(s, GpFlow::Idle);
    }

    #[test]
    fn gp_flow_calibration_is_busy_capturing_and_cancellable() {
        let c = GpFlow::Calibrate { step: 1, total: 12 };
        assert!(c.is_busy(), "校准中不能被 chip 跳转打断");
        assert!(c.is_capturing(), "离开页面时应中止校准");
        assert_eq!(c.slot(), None, "校准向导不绑定单一槽位");
        assert_eq!(c.transition(GpEvent::Cancel), GpFlow::Idle);
        assert_eq!(c.transition(GpEvent::Reset), GpFlow::Idle);
        // total=0 的退化输入: 完成即回 Idle
        assert_eq!(
            GpFlow::Calibrate { step: 0, total: 0 }.transition(GpEvent::CalibrationStepDone),
            GpFlow::Idle
        );
    }
}
