//! ★v19: ACT 专属预设变体回归测试 (通用预设 "-ACT" 与全职业 "-ACT")。
//! 约束: S4 路线不可见 (过滤), 不修改 S4 原表; ACT 变体事件强度/衰减按 ACT 档位。

use sorahk::config::{
    act_build_params, act_general_presets, act_motor_item_lr, act_rank_lr, act_tier_from_max,
    default_vibration_presets, ensure_act_general_variants, is_act_base_name,
    is_act_variant_name, visible_preset_entries, ActSignature, ActTier, VibrationPreset,
};
use sorahk::job_presets::{available_jobs, builtin_jobs, builtin_jobs_act};

fn base() -> [u32; 60] {
    // 任意一个基准 (模拟 S4 预设): 事件强度故意与 ACT 档位不同
    let mut p = [0u32; 60];
    p[0] = 100;
    p[4] = 45;
    p[9] = 100;
    p[11] = 55;
    p[12] = 15;
    p[13] = 24;
    p[14] = 15;
    p[15] = 15;
    p[16] = 10;
    p[17] = 23;
    p[35] = 45;
    p[36] = 35;
    p[37] = 45;
    p[38] = 45;
    p
}

#[test]
fn act_build_params_shapes_and_signatures() {
    /* Heavy + 命中主体 (≈ ACT1 口径) */
    let p = act_build_params(&base(), ActTier::Heavy, ActSignature::Hit, None);
    assert_eq!(p[11], 20, "IVL 压到 20");
    assert_eq!(
        [p[12], p[13], p[14], p[15], p[16], p[17]],
        [19, 51, 24, 10, 61, 37],
        "Heavy 档事件阶梯 (v19.5 ACT1 锚定)"
    );
    assert_eq!([p[35], p[36], p[37], p[38]], [25, 55, 45, 55], "Heavy 衰减");
    assert_eq!([p[0], p[4], p[9]], [100, 68, 90], "三闸: 攻击/档位上限/总调");
    /* 特殊主体 → 特殊 > 命中 */
    let q = act_build_params(&base(), ActTier::Light, ActSignature::Special, None);
    assert!(q[13] > q[16], "特殊主体应高于命中: {} vs {}", q[13], q[16]);
    /* 受击主体 */
    let r = act_build_params(&base(), ActTier::Medium, ActSignature::Taken, None);
    assert!(r[17] > r[16], "受击主体应高于命中");
    /* DOT 主体 */
    let d = act_build_params(&base(), ActTier::Medium, ActSignature::Dot, None);
    assert!(d[12] > d[16], "DOT 主体应高于命中");
    /* 上限覆盖 (极简轻巧) */
    let e = act_build_params(&base(), ActTier::Light, ActSignature::Hit, Some(50));
    assert_eq!(e[4], 50);
    /* 档位由原上限映射 */
    assert!(matches!(act_tier_from_max(30), ActTier::Light));
    assert!(matches!(act_tier_from_max(33), ActTier::Medium));
    assert!(matches!(act_tier_from_max(37), ActTier::Heavy));
    assert!(matches!(act_tier_from_max(41), ActTier::Extreme));
    assert!(matches!(act_tier_from_max(46), ActTier::Max));
}

#[test]
fn act_advanced_rebuild_and_motor_language() {
    /* 高连击 + 走位原型 */
    let mut combo = base();
    combo[29] = 30;
    combo[47] = 160;
    combo[21] = 12;
    let p = act_build_params(&combo, ActTier::Heavy, ActSignature::Hit, None);
    assert_eq!([p[29], p[31], p[33]], [35, 40, 60], "高连击密度配置");
    assert_eq!(p[47], 140, "高连击倍率上限压低");
    assert_eq!([p[21], p[22], p[39]], [12, 40, 1800], "走位型能量积累");
    assert_eq!([p[23], p[26], p[28]], [380, 45, 4], "移动质感统一");
    /* 重击 + 站桩原型 */
    let mut heavy = base();
    heavy[29] = 70;
    heavy[47] = 170;
    heavy[21] = 2;
    let h = act_build_params(&heavy, ActTier::Max, ActSignature::Hit, None);
    assert_eq!([h[29], h[31], h[33]], [65, 20, 75], "重击密度配置");
    assert_eq!([h[21], h[22], h[39]], [2, 10, 1000], "站桩型能量积累");
    /* 马达语言: 命中偏右 / 受击偏左 / 特殊偏右 / DOT 微轻 */
    let lr = act_motor_item_lr(ActTier::Heavy);
    let s = |w: u32| w as i32;
    assert!(s(lr[11 * 2]) < 0 && s(lr[11 * 2 + 1]) > 0, "命中偏右清脆");
    assert!(s(lr[12 * 2]) > 0 && s(lr[12 * 2 + 1]) < 0, "受击偏左低吼");
    assert!(s(lr[10 * 2 + 1]) > 0, "特殊偏右");
    assert!(s(lr[7 * 2]) < 0 && s(lr[7 * 2 + 1]) < 0, "DOT 微轻");
    let r = act_rank_lr();
    assert_eq!((r[22], r[23]), (-15, 10), "评分马达: 移动偏左");
}

#[test]
fn act_general_presets_keep_their_philosophy() {
    let list = act_general_presets();
    assert_eq!(list.len(), 7, "9 个内置 - ACT1 - 测试版(全0) = 7 个 ACT 变体");
    let find = |n: &str| {
        list.iter()
            .find(|p| p.name == n)
            .unwrap_or_else(|| panic!("缺少 {}", n))
            .params
    };
    for p in &list {
        assert!(p.name.ends_with("-ACT"), "命名必须以 -ACT 结尾: {}", p.name);
        assert!(!p.user_modified);
        assert!(p.params[11] <= 20, "{} IVL", p.name);
    }
    /* 高振幅 = 最重档 (上限 75 / 命中 72) */
    let hi = find("高振幅-ACT");
    assert_eq!((hi[4], hi[16]), (75, 68), "高振幅-ACT 应是最重档");
    /* 极简轻巧 = 最轻 (上限压到 50) */
    let light = find("极简轻巧-ACT");
    assert_eq!(light[4], 50, "极简轻巧-ACT 上限压到 50");
    assert!(light[16] < hi[16], "极简轻巧应比高振幅轻");
    /* 高频攻击 = 轻盈档 */
    assert_eq!(find("高频攻击职业-ACT")[4], 58);
    /* 低频攻击 = 特殊主体 (重击型) */
    let low = find("低频攻击职业-ACT");
    assert!(low[13] > low[16], "低频攻击-ACT 特殊应为主体");
    /* 实战竞技 = 受击主体 (生存预警) */
    let comp = find("实战竞技-ACT");
    assert!(comp[17] > comp[16], "实战竞技-ACT 受击应为主体");
    /* 节奏律动 = 轻脆 + 特殊主体 */
    let rhythm = find("节奏律动-ACT");
    assert_eq!(rhythm[35], 20, "节奏律动-ACT 命中衰减应最短");
    assert!(rhythm[13] > rhythm[16], "节奏律动-ACT 特殊应为主体");
    /* 不含 ACT1 特供 / 测试版 */
    assert!(!list.iter().any(|p| p.name == "ACT1 特供-ACT"));
    assert!(!list.iter().any(|p| p.name.starts_with("测试版")));
}

#[test]
fn ensure_act_variants_inserts_after_base_and_heals() {
    let mut presets = default_vibration_presets(); // 含 默认/ACT1 特供/...
    // 先清掉可能已存在的 -ACT (default 表本身没有)
    presets.retain(|p| !p.name.ends_with("-ACT"));
    assert!(!ensure_act_general_variants(&mut presets, false), "S4 不插入");
    assert!(!presets.iter().any(|p| p.name.ends_with("-ACT")));

    assert!(ensure_act_general_variants(&mut presets, true), "S1 应插入");
    let hi = presets.iter().position(|p| p.name == "高振幅").unwrap();
    assert_eq!(
        presets[hi + 1].name,
        "高振幅-ACT",
        "ACT 变体应紧随基准预设"
    );
    assert!(!ensure_act_general_variants(&mut presets, true), "二次调用应幂等");

    // 陈旧值自愈
    let idx = presets.iter().position(|p| p.name == "高振幅-ACT").unwrap();
    presets[idx].params[16] = 1;
    assert!(ensure_act_general_variants(&mut presets, true), "陈旧值触发自愈");
    assert_eq!(presets[idx].params[16], 68, "高振幅-ACT 命中应回到最重档");

    // 用户覆盖过的条目不动
    presets[idx].user_modified = true;
    presets[idx].params[16] = 42;
    assert!(!ensure_act_general_variants(&mut presets, true));
    assert_eq!(presets[idx].params[16], 42);
}

#[test]
fn visible_entries_route_split_and_real_index() {
    let presets: Vec<VibrationPreset> = default_vibration_presets();
    let legacy = visible_preset_entries(&presets, true);
    let s4 = visible_preset_entries(&presets, false);
    /* S4: 隐藏 ACT 专属 (ACT1 特供 + *-ACT) */
    assert!(s4.iter().all(|(_, n)| !is_act_variant_name(n)));
    assert!(s4.iter().any(|(_, n)| n == "默认"));
    /* S1: 隐藏有 ACT 变体的内置原版 (改用 -ACT 版); ACT1 特供/测试版保留。
     * 本夹具是纯内置表 (无 -ACT 条目), 故 S1 只剩 ACT1 特供 + 测试版 */
    assert!(legacy.iter().all(|(_, n)| !is_act_base_name(n)));
    assert_eq!(
        legacy.iter().map(|(_, n)| n.as_str()).collect::<Vec<_>>(),
        vec!["ACT1 特供", "测试版(全0)"]
    );
    /* 真实下标: 逐个核对名称 */
    for (i, n) in s4.iter().chain(legacy.iter()) {
        assert_eq!(&presets[*i].name, n, "真实下标必须指向同名条目");
    }
}

#[test]
fn job_act_variants_shape_and_route_gate() {
    let act = builtin_jobs_act();
    let base_jobs = builtin_jobs();
    assert_eq!(act.len(), 9, "ACT 名册: 14 - 5 个时代外基类");
    /* ACT1 时代不存在的职业必须缺席 */
    for gone in [
        "鬼剑士（女）-ACT",
        "圣职者（女）-ACT",
        "魔枪士（男）-ACT",
        "守护者（女）-ACT",
        "外传-ACT",
    ] {
        assert!(!act.iter().any(|j| j.base_job == gone), "应剔除 {}", gone);
    }
    assert!(
        !act.iter().any(|j| j.classes.iter().any(|c| c.name == "合金战士")),
        "神枪手（男）-ACT 应剔除合金战士"
    );
    for j in &act {
        assert!(j.base_job.ends_with("-ACT"), "命名后缀: {}", j.base_job);
        for c in &j.classes {
            assert!(
                [58u32, 63, 68, 75].contains(&c.params[4]),
                "{} 力度档上限 (实={})",
                c.name,
                c.params[4]
            );
            assert!(
                (46..=72).contains(&c.params[16]),
                "{} ACT 命中区间 (实={})",
                c.name,
                c.params[16]
            );
            assert!(c.params[11] <= 20, "{} ACT IVL", c.name);
            assert!(
                c.params[35] <= 30 && c.params[36] <= 70,
                "{} ACT 衰减区间",
                c.name
            );
        }
    }
    /* 力度档映射抽查: 剑魂 原上限 30 → Light 档 (上限 55 / 命中 50 / 衰减 20) */
    let jh = act
        .iter()
        .find(|j| j.base_job == "鬼剑士（男）-ACT")
        .unwrap()
        .classes
        .iter()
        .find(|c| c.name == "剑魂")
        .unwrap();
    assert_eq!((jh.params[4], jh.params[16], jh.params[35]), (58, 50, 20));
    /* 专属算法保留 */
    let base_jh = base_jobs
        .iter()
        .find(|j| j.base_job == "鬼剑士（男）")
        .unwrap()
        .classes
        .iter()
        .find(|c| c.name == "剑魂")
        .unwrap();
    assert_eq!(jh.algo_id, base_jh.algo_id, "专属算法 id 保留");
    /* 路线门: S4 只给原表, S1 只给 ACT 名册 (互不冲突) */
    assert_eq!(available_jobs(false).len(), base_jobs.len(), "S4 仅原表");
    assert_eq!(available_jobs(true).len(), 9, "S1 仅 ACT 名册");
    assert!(
        available_jobs(true).iter().all(|j| j.base_job.ends_with("-ACT")),
        "S1 职业表全部为 -ACT"
    );
}

/// ★v24.14: 导出文件的路线标识与按路线过滤 —— ACT 导出带 "-ACT", 且导入列表
/// 只列当前路线的文件 (防把 ACT 档位参数导进 S4 路线)。
#[test]
fn act_export_names_and_route_filter() {
    use sorahk::job_presets::{export_prefix, is_act_export_name, list_export_files_for_route};

    /* 命名口径: 通用导出靠前缀后缀, 职业导出靠职业名自带 -ACT */
    assert_eq!(export_prefix("vibration_export", true), "vibration_export-ACT");
    assert_eq!(export_prefix("vibration_export", false), "vibration_export");
    assert!(is_act_export_name("vibration_export-ACT_1757000000.toml"));
    assert!(is_act_export_name("job_export_剑魂-ACT_1757000000.toml"));
    assert!(!is_act_export_name("vibration_export_1757000000.toml"));
    assert!(!is_act_export_name("job_export_剑魂_1757000000.toml"));

    /* 目录级过滤: 同前缀下两路线的文件互不可见 */
    let tag = format!("zz_route_test_{}", std::process::id());
    let s4_name = format!("{tag}_vibration_export_1.toml");
    let s1_name = format!("{tag}_vibration_export-ACT_1.toml");
    for n in [&s4_name, &s1_name] {
        std::fs::write(n, "params = []\n").unwrap();
    }
    let listed_s1 = list_export_files_for_route(&tag, true);
    let listed_s4 = list_export_files_for_route(&tag, false);
    assert_eq!(listed_s1, vec![s1_name.clone()], "S1 只列 -ACT");
    assert_eq!(listed_s4, vec![s4_name.clone()], "S4 只列非 -ACT");
    for n in [&s4_name, &s1_name] {
        let _ = std::fs::remove_file(n);
    }
}
