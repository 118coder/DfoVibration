//! 震动页渲染小件: 参数提示表(param_hint v24.16)/with_hint/config↔60槽同步/L·R马达与评分族滑块渲染 —— 原 vibration_page.rs 15-287, 2026-09-27 架构重构 C1 归位。

use crate::gui::SorahkGui;
use crate::gui::theme::Theme;

use eframe::egui;

impl SorahkGui {
    /* ★v24.16: 参数说明单一来源 —— 按滑块显示名查"高低后果"。
     * 用户要求: 每个参数都要讲清"调高会怎样 / 调低会怎样"(开关类讲"开/关"),
     * 让高低有明确概念。没有登记的返回空串 (该控件不挂提示, 不会 panic)。
     * 登记在 param_hint 里的都是原本**没有任何说明**的参数 (只有名字的滑块);
     * 原本已有说明的参数, 其说明末尾已直接补上同样的"调高/调低"句子。 */
    pub(in crate::gui) fn param_hint(name: &str) -> &'static str {
        match name {
        "停手衰减延迟 ms" => "调高：停手后更久才开始衰减（余感更长）；调低：停手后立刻开始掉分衰减。",
        "衰减速度 级/秒 (整数)" => "调高：掉分更快（停手后很快冷下来）；调低：掉分更慢（分数更耐留）。",
        "最低反馈倍率 % (评级 0, 保底)" => "调高：低评级时也震得不轻（保底更响）；调低：低评级时更轻，几乎只有高评级才震。",
        "最高反馈倍率 % (满评分, 预设×此值)" => "调高：高分时更猛（更强的高分奖励感）；调低：高分与低分的差别变小。",
        "绝对频率窗口 ms" => "调高：统计窗口更长，判定更宽容；调低：窗口更短，判定更挑剔。",
        "窗口内最大震动次数" => "调高：允许更密（更容易顶满上限）；调低：更容易被限住（更克制）。",
        "满幅脉冲时长 ms (所有评分事件)" => "调高：评分脉冲更长更厚；调低：更短更利落；0 = 只闪一下。",
        "命中聚合窗 ms (群怪一刀只震一下)" => "调高：更多命中被并成一下（群怪更整齐）；调低：只有真正同时的命中才合并。",
        "合成一下时保留多少力度 % (0=关闭)" => "调高：合并那一下更接近原力度（更厚更响）；调低：合并后更轻；0 = 关闭合并。",
        "漏掉的震动多久后补一下 (ms, 0=不补)" => "调高：等得更久才补（更容易补出一记收尾）；调低：很快补上；0 = 不补，漏掉就漏掉。",
        "每秒最多震几下 (0=不限制)" => "调高：允许更密（群怪更热闹，也更吵）；调低：限得更死（更安静）；0 = 完全不限。",
        "限频的计时窗口 ms" => "调高：窗口更长，长时间的平均更严格；调低：窗口更短，只在很短的爆发里限频。",
        "一秒内命中超过几下算风暴" => "调高：更难进入风暴（阈值高，抽样少触发）；调低：更容易进入风暴（阈值低，抽样多触发）。",
        "风暴期保留多少 % 命中震动" => "调高：保留更多震动（更密、接近刀刀震）；调低：保留更少（更稀疏、更安静）。",
        "停手多久恢复刀刀震 ms" => "调高：要停更久才退出风暴（风暴持续更久）；调低：很快退出风暴（更快恢复刀刀震）。",
        "风暴的统计窗口 ms" => "调高：风暴判定更平缓（不容易被瞬时爆发触发）；调低：判定更挑剔/更灵敏（窗口越小越挑）。",
        "统合衰减期 ms (越大越连贯, 40-400)" => "调高：每击延续更久（更连贯，可能糊在一起）；调低：每击更短促（一击是一击，更干脆）。",
        "打群怪几秒后自动降温 (秒, 0=关闭)" => "调高：更晚才降温（更晚才开始变轻）；调低：很快就开始降温（更早变轻）；0 = 不降温。",
        "自动降温幅度 % (变轻多少)" => "调高：降温后更轻（压得更狠）；调低：降温后只是稍微轻一点。",
        "震尾多快切断 % (0=自然衰减)" => "调高：更早切断余震（一击更干脆、更像点状）；调低：更接近自然衰减（余音更长）。",
        "多快算太密: 连击次数 (100=关闭)" => "调高：更难触发降强度（更晚才开始收敛）；调低：更容易触发（更早开始收敛）。",
        "统计时长 ms" => "调高：统计窗口更长，降强度更平缓；调低：窗口更短，只压瞬时太密。",
        "太密时减轻多少 %" => "调高：太密时压得更狠（更轻）；调低：几乎不压。",
        "停手多久恢复力度 ms" => "调高：要停更久才恢复满强度；调低：很快恢复满强度。",
        "最轻不低于原来的 %" => "调高：压到底也保留更多力度（更响）；调低：允许压得更轻。",
        "恢复快慢 ms (0=立刻)" => "调高：恢复更慢更平滑（不突兀）；调低：恢复更快（立刻恢复）。",
        "连击加成的上限 (倍数×100)" => "调高：连击加成能给到更高（后期更猛）；调低：封顶更低（后期更克制）。",
        "连击越长震越强: 每 100 连加强多少 %" => "调高：每 100 连加强更多（涨得更快）；调低：涨得更慢。",
        "连击很久没连上时补一记的门槛" => "调高：门槛更低（更容易补脉冲）；调低：门槛更高（很少补）。",
        "打多快算太快: 间隔低于此开始减弱 ms" => "调高：更容易被判太快（更早开始减弱）；调低：要打得更快才会减弱。",
        "打太快时减轻多少 %" => "调高：打太快时压得更狠；调低：几乎不压。",
        "打多慢算正常: 间隔超过此恢复满强度 ms" => "调高：要更慢才恢复满强度；调低：稍慢就恢复满强度。",
        "减弱后最轻不低于原来的 %" => "调高：减弱后仍保留更多力度；调低：允许减得更轻。",
        "移动持续震动强度" => "调高：走路震动更明显；调低：走路更轻；0 = 走路不震。",
        "移动步频 ms (自然步频 ~380)" => "调高：脚步更慢更沉（步点拉长）；调低：脚步更密更快。",
        "移动着地脉冲 % (柔和, 不宜高)" => "调高：落地那一下更重（过高会震手）；调低：更轻更柔。",
        "移动抬脚保持 % (极轻)" => "调高：抬脚阶段保留更多（更连续）；调低：抬脚更安静。",
        "移动整体增益 % (轻音量)" => "调高：移动整体更响；调低：更轻（更背景）。",
        "移动平滑系数 % (越大过渡越柔, 消除嗡嗡声)" => "调高：过渡更柔（更不容易嗡嗡，但也更钝）；调低：更锐利（反应更快，可能嗡）。",
        "移动最低输出阈值 % (低于归0, 消除沙沙声)" => "调高：更容易归零（更干净，但轻微移动不震）；调低：更灵敏（轻微移动也有震，可能沙沙）。",
        "移动积累速率 %/秒 (0=禁用)" => "调高：走位时涨得更快（更快变强）；调低：涨得更慢；0 = 不走位积累。",
        "攻击增强上限 % (最多 ×(1+上限))" => "调高：走位能涨到的上限更高；调低：上限更低（走位加成更小）。",
        "增强窗口 ms (停手后有效)" => "调高：停手后保留更久（加成更耐留）；调低：很快失效。",
        "左马达曲线 %" => "调高：左马达更早出力（低强度就有感、更厚重）；调低：更线性/更迟出力（低强度几乎不动）。",
        "右马达曲线 %" => "调高：右马达更早出力（更清脆明显）；调低：更线性/更迟出力。",
        "普通命中衰减 ms" => "调高：命中余震更长更连贯；调低：更短更干脆。",
        "特殊攻击衰减 ms" => "调高：特殊攻击余震更长；调低：更短更利落。",
        "受击衰减 ms" => "调高：挨打的余震更长（更沉）；调低：更短更干脆。",
        "状态变化衰减 ms" => "调高：状态提示余震更长；调低：更短。",
        "DOT 持续反馈时长 ms" => "调高：每次掉血延续更久（更黏）；调低：更短促。",
        "装备特效节奏周期 ms" => "调高：节奏更慢更悠长；调低：节奏更快更碎。",
        "爆发窗口 ms (窗口内 6 连触发爆发)" => "调高：更容易凑够 6 连触发爆发（更常爆发）；调低：更难触发（更少爆发）。",
        "爆发模式最低强度 %" => "调高：爆发更响；调低：爆发更轻。",
        "反击窗口 ms (受击后攻击强化)" => "调高：挨打后强化持续更久；调低：强化转瞬即逝。",
        "连击统计窗口 ms" => "调高：连击更不容易断（统计更宽容）；调低：连击更容易断。",
        "空闲判定 ms (回战斗前状态)" => "调高：更久才算空闲（状态更黏）；调低：很快判定空闲（更快回初始）。",
        "特殊攻击后静默 ms (突显暴击破招)" => "调高：静默更久（暴击/破招更突出，但普攻被压更久）；调低：静默更短（更连续）。",
        "反击强化倍数 %" => "调高：反击那一下加成更高；调低：加成更低。",
        "唤醒脉冲强度 %" => "调高：唤醒（开打第一下）更明显；调低：更轻。",
        "里程碑脉冲强度 %" => "调高：里程碑提示更明显；调低：更轻。",
        "连击中断收尾脉冲 %" => "调高：断连收尾那一下更明显；调低：更轻。",
        "测试震动强度 %" => "调高：测试按钮震得更强；调低：更轻（只验证有没有震动）。",
        "绝对震动频率 (3 秒最多 6 次)" => "开：3 秒内最多震 6 次（防连成一片）；关：完全不限次数。",
        "震动随机性 %" => "调高：每次震动的强弱更随机（更自然，但可预测性变差）；调低：每次几乎一样（更稳定）。",
        "移动持续震动独立于全局强度 (默认开)" => "开：走路震动不随「全局总调整」缩放（独立的一条）；关：跟全局总调整一起变大变小。",
        "启用伤害飘字震动 (默认开启)" => "开：伤害数字出现时震（下面的飘字滑块才生效）；关：飘字不震。",
        "通用飘字强度" => "调高：所有飘字类型整体更响；调低：整体更轻；0 = 飘字通道全关。",
        "飘字最小间隔 ms" => "调高：跳字更疏、更省电（每跳至少隔这么久）；调低：跳字越密越容易连着震（容易糊成一片）。",
        "评分等级强度 % (等级 F~SSS)" => "调高：高评分等级（SSS）震得更猛；调低：各等级差别变小。",
        "启用评分动态衰减 (类鬼泣)" => "开：连打得越猛越强、停手跌分变轻（有起伏）；关：一直按固定强度震。",
        "启用命中自适应 (关=回到旧行为, 下方滑块不生效)" => "开：打太快自动减轻、打太慢自动恢复；关：不自动减（下面 4 个滑块不生效）。",
        "输出平滑 % (抑制嗡嗡声, 高=柔)" => "调高：输出更柔（消除嗡嗡声，但更钝）；调低：更锐利（反应快，低档可能嗡）。",
        "分工分界 % (低于为轻反馈)" => "调高：更多能量给左马达（更厚重）；调低：更多给右马达（更清脆）。",
        "输出低强度抑制 % (S4 / S1-S4模式, 转子马达建议 25-35)" => "调高：弱震动更容易被完全压掉（更干净）；调低：弱震动保留更多（更灵敏，可能沙沙）。",
        "重映射最小输出 % (与死区一致, S4 / S1-S4模式)" => "调高：最低档也有明显输出（抬底更高）；调低：最低档几乎没输出（更安静）。",
        "震动节流窗口 ms (0=禁用, 仅 S4)" => "调高：节流窗口更长、更持续（更安静）；调低：只在很短窗口内节流；0 = 关闭节流。",
        "窗口内最大震动次数 (仅 S4)" => "调高：窗口内允许更多次（更密）；调低：更容易被限住（更克制）。",
        "狂震期次数比例 % (自适应收紧, 100=不收紧, 仅 S4)" => "调高：狂震期保留更多震动；调低：狂震期压得更狠（更安静）；100 = 不收紧。",
        "衰减时间 (预留)" => "预留槽位：当前引擎不读，调高调低都不影响震动。",
            _ => "",
        }
    }

    /// 给控件挂上 `param_hint` 的说明 (没有登记就原样返回)。
    pub(in crate::gui) fn with_hint(resp: egui::Response, name: &str) -> egui::Response {
        let h = Self::param_hint(name);
        if h.is_empty() {
            resp
        } else {
            resp.on_hover_text(h)
        }
    }

    /// 将当前参数写入 config.vibration (索引映射, 与预设 params 顺序一致)
    /// ★D 方案: 槽位↔字段映射唯一事实源在 vibration/params.rs, 此处只做原子读取 + 委托。
    pub(in crate::gui) fn sync_vib_config_from_params(
        cfg: &mut crate::config::VibrationConfig,
        p: &[std::sync::atomic::AtomicU32; 60],
    ) {
        use std::sync::atomic::Ordering;
        let mut a = [0u32; 60];
        for (i, s) in p.iter().enumerate() {
            a[i] = s.load(Ordering::Relaxed);
        }
        crate::vibration::params::config_from_params(&a, cfg);
    }


    /// 渲染某基础项的 L/R 马达权重滑块 + 实时输出条
    /// 权重范围 -100..+100: 0 = 保持原样(不动原参数), 负 = 减弱, 正 = 增强
    /// item: 0=总闸 1=攻击 2=上限 3=衰减 4=连击 5=节奏 6=通用 7=DOT 8=特效 9=状态 10=特殊 11=命中 12=受击
    pub(in crate::gui) fn render_item_lr(
        ui: &mut egui::Ui,
        dark: bool,
        state: &crate::state::AppState,
        config_item_lr: &mut [u32; 26],
        item: usize,
    ) {
        use std::sync::atomic::Ordering;
        let il = state.vibration_item_lr[item * 2].load(Ordering::Relaxed) as i32 as f32;
        let ir = state.vibration_item_lr[item * 2 + 1].load(Ordering::Relaxed) as i32 as f32;
        let ol = state.vibration_item_out[item * 2].load(Ordering::Relaxed);
        let or_ = state.vibration_item_out[item * 2 + 1].load(Ordering::Relaxed);
        let mut l = il;
        let mut r = ir;

        ui.vertical(|ui| {
            /* L = 橙色, R = 红色 (清晰区分), 每行一个马达, 宽度自适应 */
            let th = Theme::new(dark);
            let l_orange = th.motor_l;
            let r_red = th.motor_r;
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("L").size(13.0).strong().color(l_orange));
                ui.add(
                    egui::Slider::new(&mut l, -100.0..=100.0)
                        .show_value(false)
                        .trailing_fill(true)
                        .text_color(l_orange),
                );
                ui.add(
                    egui::ProgressBar::new(ol as f32 / 65535.0)
                        .desired_width(40.0)
                        .fill(l_orange),
                );
                ui.label(
                    egui::RichText::new(format!("L{:+}", l as i32))
                        .size(11.0)
                        .weak(),
                );
            });
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("R").size(13.0).strong().color(r_red));
                ui.add(
                    egui::Slider::new(&mut r, -100.0..=100.0)
                        .show_value(false)
                        .trailing_fill(true)
                        .text_color(r_red),
                );
                ui.add(
                    egui::ProgressBar::new(or_ as f32 / 65535.0)
                        .desired_width(40.0)
                        .fill(r_red),
                );
                ui.label(
                    egui::RichText::new(format!("R{:+}", r as i32))
                        .size(11.0)
                        .weak(),
                );
            });
        });
        if (l as i32) != (il as i32) {
            state.vibration_item_lr[item * 2].store((l as i32) as u32, Ordering::Relaxed);
            config_item_lr[item * 2] = l as u32;
        }
        if (r as i32) != (ir as i32) {
            state.vibration_item_lr[item * 2 + 1].store((r as i32) as u32, Ordering::Relaxed);
            config_item_lr[item * 2 + 1] = r as u32;
        }
    }


    /// 渲染评分事件的 L/R 马达权重滑块 (rank_lr, 30 项 = 15 事件 x L/R)
    pub(in crate::gui) fn render_rank_lr(
        ui: &mut egui::Ui,
        dark: bool,
        state: &crate::state::AppState,
        config_rank_lr: &mut [u32; 30],
        item: usize,
    ) {
        use std::sync::atomic::Ordering;
        let il = state.vibration_rank_lr[item * 2].load(Ordering::Relaxed) as i32 as f32;
        let ir = state.vibration_rank_lr[item * 2 + 1].load(Ordering::Relaxed) as i32 as f32;
        let ol = state.vibration_rank_out[item * 2].load(Ordering::Relaxed);
        let or_ = state.vibration_rank_out[item * 2 + 1].load(Ordering::Relaxed);
        let mut l = il;
        let mut r = ir;

        ui.vertical(|ui| {
            let th = Theme::new(dark);
            let l_orange = th.motor_l;
            let r_red = th.motor_r;
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("L").size(13.0).strong().color(l_orange));
                ui.add(
                    egui::Slider::new(&mut l, -100.0..=100.0)
                        .show_value(false)
                        .trailing_fill(true)
                        .text_color(l_orange),
                );
                ui.add(
                    egui::ProgressBar::new(ol as f32 / 65535.0)
                        .desired_width(40.0)
                        .fill(l_orange),
                );
                ui.label(
                    egui::RichText::new(format!("L{:+}", l as i32))
                        .size(11.0)
                        .weak(),
                );
            });
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("R").size(13.0).strong().color(r_red));
                ui.add(
                    egui::Slider::new(&mut r, -100.0..=100.0)
                        .show_value(false)
                        .trailing_fill(true)
                        .text_color(r_red),
                );
                ui.add(
                    egui::ProgressBar::new(or_ as f32 / 65535.0)
                        .desired_width(40.0)
                        .fill(r_red),
                );
                ui.label(
                    egui::RichText::new(format!("R{:+}", r as i32))
                        .size(11.0)
                        .weak(),
                );
            });
        });
        if (l as i32) != (il as i32) {
            state.vibration_rank_lr[item * 2].store((l as i32) as u32, Ordering::Relaxed);
            config_rank_lr[item * 2] = l as u32;
        }
        if (r as i32) != (ir as i32) {
            state.vibration_rank_lr[item * 2 + 1].store((r as i32) as u32, Ordering::Relaxed);
            config_rank_lr[item * 2 + 1] = r as u32;
        }
    }
}
