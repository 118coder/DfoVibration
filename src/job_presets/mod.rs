//! ============================================================
//! 全职业预设 (v21) — 独立模块, 不与原震动设置结构冲突
//!
//! v31: 目录化 — 每个基础职业一个 .rs 文件 (逐职业手工细调 +
//!      每职业专属高级算法 algo_id/algo_params)
//!
//! 每个基础职业 → 多个转职, 每个转职有独立:
//!   - params[60] (完整震动参数, 含移动走路质感 23-28)
//!   - rank_lr[30] (评分左右马达)
//!   - rank_type_gain[15] (评分细分强度)
//!   - rank_level_gain / rank_duration
//!   - out_smooth / throttle / abs_freq
//!   - algo_id + algo_params[4] (专属高级算法, 引擎按 id 执行)
//!
//! 专属算法 (v31):
//!   1 连携脉冲(鬼剑男) 2 收刀爆发(鬼剑女) 3 刚拳蓄势(格斗男)
//!   4 连环腿(格斗女) 5 弹幕连点(神枪男) 6 空袭轰炸(神枪女)
//!   7 元素叠层(魔法男) 8 蓄气爆发(魔法女) 9 圣光追加(圣职男)
//!   10 庇护缓冲(圣职女) 11 暗杀标记(暗夜) 12 盾牌格挡(守护者)
//!   13 战意昂扬(魔枪士) 14 自由连段(外传) 15 绝对频率(召唤, 独立开关)
//! ============================================================

mod slayer_male;
mod slayer_female;
mod fighter_male;
mod fighter_female;
mod gunner_male;
mod gunner_female;
mod mage_male;
mod mage_female;
mod priest_male;
mod priest_female;
mod thief;
mod knight;
mod lancer;
mod creator;

/// 单个转职的完整震动预设
#[derive(Debug, Clone)]
pub struct JobClass {
    pub name: &'static str,
    pub params: [u32; 60],
    pub rank_lr: [i32; 30],
    pub rank_type_gain: [u32; 15],
    pub rank_level_gain: u32,
    pub rank_duration: u32,
    /// 输出平滑 % (v22.3: 抑制低频嗡嗡声; 连击型高/重击型低)
    pub out_smooth: u32,
    /// 震动节流窗口 ms (v24: 0=禁用; 召唤师等狂震职业 3000ms)
    pub throttle_window: u32,
    /// 节流窗口内最大注入次数 (v24: 0=禁用; 召唤师 8 次)
    pub throttle_max: u32,
    /// 密度激活时节流次数比例 % (v24.1: 100=不收紧; 召唤师 20 → 1次/s)
    pub throttle_dense_ratio: u32,
    /// 绝对震动频率 (v26: 召唤专属, 一切算法失效, 3s/6 次写死)
    pub abs_freq_enabled: bool,
    /// 专属高级算法 id (v31: 0=无, 1-15 各基础职业专属)
    pub algo_id: u8,
    /// 专属高级算法参数 [u32; 4]
    pub algo_params: [u32; 4],
    pub desc: &'static str,
}

/// 基础职业
pub struct JobPreset {
    pub base_job: &'static str,
    pub classes: Vec<JobClass>,
}

/// 通用评分参数 (等级/时长/细分), 按力度档微调
const fn rank_cfg(level: u32, dur: u32, points: u32, mv: u32, crit: u32, kill: u32) -> [u32; 15] {
    [
        points, 85, 90, 90, 90, 90, 85, 65, 70, 75, 90, mv, 90, crit, kill,
    ]
}

/// 通用评分 L/R (轻档偏右, 重档偏左)
const fn rank_lr_light() -> [i32; 30] {
    [
        0, 10, 0, 10, 0, 0, 0, 0, 0, 0, 0, 10, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, -15, 10, 0, 0, 0, 15, 0, 10,
    ]
}
const fn rank_lr_medium() -> [i32; 30] {
    [
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, -15, 10, 0, 0, 5, 5, 0, 5,
    ]
}
const fn rank_lr_heavy() -> [i32; 30] {
    [
        0, 0, 15, -5, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 5, 0, 0, -20, 5, 0, 0, 15, -5, 10, 0,
    ]
}
const fn rank_lr_extreme() -> [i32; 30] {
    [
        0, 0, 20, -10, 0, 0, 0, 0, 0, 0, 25, -10, 0, 0, 0, 0, 0, 0, 10, 0, 0, 0, -25, 5, 0, 0, 25, -10, 20, 0,
    ]
}

// ---------- 内置职业数据 (14 基础职业, 各自 .rs 文件) ----------
pub fn builtin_jobs() -> Vec<JobPreset> {
    vec![
        slayer_male::jobs(),
        slayer_female::jobs(),
        fighter_male::jobs(),
        fighter_female::jobs(),
        gunner_male::jobs(),
        gunner_female::jobs(),
        mage_male::jobs(),
        mage_female::jobs(),
        priest_male::jobs(),
        priest_female::jobs(),
        thief::jobs(),
        knight::jobs(),
        lancer::jobs(),
        creator::jobs(),
    ]
}

// ---------- 独立存储: JobVibration.toml (全职业预设专用, 不与其他 TOML 混) ----------

/// 已应用的职业预设配置 (含用户微调后的参数快照)
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct JobVibrationConfig {
    /// 基础职业名 (空 = 未应用)
    pub base_job: String,
    /// 转职名
    pub class_name: String,
    /// 是否已应用 (恢复时只应用 applied=true 的)
    pub applied: bool,
    /// 用户微调后的完整参数 (空 = 用内置职业预设)
    pub params: Vec<u32>,
    pub rank_duration: u32,
    pub rank_level_gain: u32,
}

/// JobVibration.toml 路径 (与程序同目录 = 游戏目录旁)
pub fn job_vibration_path() -> std::path::PathBuf {
    std::path::Path::new("JobVibration.toml").to_path_buf()
}

/// 保存职业预设配置到 JobVibration.toml
pub fn save_job_vibration(cfg: &JobVibrationConfig) {
    use std::io::Write;
    let mut result = String::new();
    result.push_str("# ═══════════════════════════════════════════════════════\n");
    result.push_str("#  Sorahk Job Vibration Settings (全职业预设独立文件) \n");
    result.push_str("# ═══════════════════════════════════════════════════════\n");
    result.push_str("# 独立于 Config.toml / Vibration.toml, 与全职业预设模块\n");
    result.push_str("# (src/job_presets/) 对应。params 为用户微调后的快照。\n\n");
    result.push_str(&format!(
        "base_job = {:?}\nclass_name = {:?}\napplied = {}\n",
        cfg.base_job, cfg.class_name, cfg.applied
    ));
    result.push_str("params = [");
    for (i, v) in cfg.params.iter().enumerate() {
        if i > 0 {
            result.push_str(", ");
        }
        result.push_str(&v.to_string());
    }
    result.push_str("]\n");
    result.push_str(&format!(
        "rank_duration = {}\nrank_level_gain = {}\n",
        cfg.rank_duration, cfg.rank_level_gain
    ));
    if let Some(parent) = job_vibration_path().parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    match std::fs::File::create(job_vibration_path()) {
        Ok(mut f) => {
            let _ = f.write_all(result.as_bytes());
            let _ = f.flush();
        }
        Err(_) => {}
    }
}

/// 从 JobVibration.toml 读取职业预设配置
pub fn load_job_vibration() -> Option<JobVibrationConfig> {
    let content = std::fs::read_to_string(job_vibration_path()).ok()?;
    let cfg: JobVibrationConfig = toml::from_str(&content).ok()?;
    if cfg.base_job.is_empty() {
        return None;
    }
    Some(cfg)
}

/// 按名字查找职业, 返回 (基础职业索引, 转职索引, 克隆)
pub fn find_class(base: &str, name: &str) -> Option<(usize, usize, JobClass)> {
    let jobs = builtin_jobs();
    for (bi, j) in jobs.iter().enumerate() {
        if j.base_job == base {
            for (ci, c) in j.classes.iter().enumerate() {
                if c.name == name {
                    return Some((bi, ci, c.clone()));
                }
            }
        }
    }
    None
}

// ---------- 配置导入 (v33.1: 与导出往返一致规范) ----------

/// 导入的职业设置 (job_export_*.toml 规范)
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct ImportedJob {
    pub base_job: String,
    pub class_name: String,
    #[serde(default)]
    pub params: Vec<u32>,
    #[serde(default)]
    pub rank_lr: Vec<i32>,
    #[serde(default)]
    pub rank_type_gain: Vec<u32>,
    #[serde(default)]
    pub rank_level_gain: u32,
    #[serde(default)]
    pub rank_duration: u32,
    #[serde(default)]
    pub out_smooth: u32,
    #[serde(default)]
    pub throttle_window: u32,
    #[serde(default)]
    pub throttle_max: u32,
    #[serde(default)]
    pub throttle_dense_ratio: u32,
    #[serde(default)]
    pub desc: String,
}

/// 扫描程序目录下指定前缀的导出文件
pub fn list_export_files(prefix: &str) -> Vec<String> {
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(".") {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with(prefix) && name.ends_with(".toml") {
                out.push(name);
            }
        }
    }
    out.sort();
    out
}

/* ── 导入值上限 (2026-09-08) ─────────────────────────────────────
 * 导出文件可被手改/损坏: 此前解析只查数组长度, u32::MAX 级数值会
 * 直送震动引擎 (幅度/时长无界 → 手柄狂震/电机长鸣), rank_lr 负数
 * 经 `as u32` 回绕成天文数字。此处按震动页滑块范围统一钳位;
 * 程序生成的合法导出永远在范围内, 不受影响。 */

/// ±100 权重以 u32 二补码存储 (与 item_lr/rank_lr 滑块 -100..=100 一致)
fn clamp_signed_u32(v: u32) -> u32 {
    (v as i32).clamp(-100, 100) as u32
}

fn clamp_params(p: &mut [u32]) {
    /* 上限 = 震动页滑块 hi; 未列出的索引均为 % 类 (0..=100) */
    let hi = |idx: usize| -> u32 {
        match idx {
            5 => 500,        // 衰减时间 ms
            11 => 500,       // 飘字最小间隔 ms
            19 | 20 => 300,  // 马达输出曲线 %
            21 => 20,        // 移动积累速率 %/秒
            23 => 800,       // 移动步频 ms
            28 => 20,        // 移动最低输出阈值 %
            30 => 1500,      // 密度检测窗口 ms
            31 => 80,        // 密度窗口期降幅 %
            32 => 3000,      // 密度恢复判定 ms
            34 => 800,       // 密度恢复平滑 ms
            35..=38 => 150,  // 各类事件衰减 ms
            39 => 200,       // 装备特效衰减 ms (39 槽位另有引擎冲突, 见 HANDOFF)
            40 => 1000,      // DOT 持续反馈 ms
            41 => 600,       // 装备特效节奏周期 ms
            42 => 1000,      // 爆发窗口 ms
            44 => 2000,      // 反击窗口 ms
            45 => 5000,      // 连击统计窗口 ms
            46 => 10000,     // 空闲判定 ms
            47 => 500,       // 连击放大上限 x
            48 => 300,       // 连击增强斜率
            50 => 300,       // 自适应阈值 ms
            52 => 1000,      // 自适应上限间隔 ms
            54 => 500,       // 特殊攻击后静默 ms
            55 => 300,       // 反击强化倍数 %
            _ => 100,
        }
    };
    for (idx, v) in p.iter_mut().enumerate() {
        *v = (*v).min(hi(idx));
    }
}

/// 解析职业导出文件 (规范: base_job/class_name/params/rank_lr/rank_type_gain/...)
pub fn parse_job_export(path: &str) -> Option<ImportedJob> {
    let content = std::fs::read_to_string(path).ok()?;
    let mut job: ImportedJob = toml::from_str(&content).ok()?;
    if job.base_job.is_empty()
        || job.class_name.is_empty()
        || job.params.len() != 60
        || job.rank_lr.len() != 30
        || job.rank_type_gain.len() != 15
    {
        return None;
    }
    clamp_params(&mut job.params);
    for v in &mut job.rank_lr {
        *v = (*v).clamp(-100, 100);
    }
    for v in &mut job.rank_type_gain {
        *v = (*v).min(100);
    }
    job.rank_level_gain = job.rank_level_gain.min(100);
    job.rank_duration = job.rank_duration.min(1000);
    job.out_smooth = job.out_smooth.min(100);
    job.throttle_window = job.throttle_window.min(10000);
    job.throttle_max = job.throttle_max.min(30);
    job.throttle_dense_ratio = job.throttle_dense_ratio.min(100);
    Some(job)
}

/// 解析通用震动导出文件 (规范: params/item_lr/rank_lr/rank_type_gain/...)
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct ImportedVibration {
    #[serde(default)]
    pub params: Vec<u32>,
    #[serde(default)]
    pub item_lr: Vec<u32>,
    #[serde(default)]
    pub rank_lr: Vec<u32>,
    #[serde(default)]
    pub rank_type_gain: Vec<u32>,
    #[serde(default)]
    pub rank_level_gain: u32,
    #[serde(default)]
    pub rank_duration: u32,
    #[serde(default)]
    pub out_smooth: u32,
    #[serde(default)]
    pub throttle_window_ms: u32,
    #[serde(default)]
    pub throttle_max_hits: u32,
    #[serde(default)]
    pub throttle_dense_ratio: u32,
    #[serde(default)]
    pub independent_test: bool,
    #[serde(default)]
    pub move_independent: bool,
    #[serde(default)]
    pub random_gain: u32,
    #[serde(default)]
    pub motor_l_gain: u32,
    #[serde(default)]
    pub motor_r_gain: u32,
}

/// 解析通用震动导出文件
pub fn parse_vibration_export(path: &str) -> Option<ImportedVibration> {
    let content = std::fs::read_to_string(path).ok()?;
    let mut v: ImportedVibration = toml::from_str(&content).ok()?;
    if v.params.len() != 60 || v.item_lr.len() != 26 || v.rank_lr.len() != 30 || v.rank_type_gain.len() != 15 {
        return None;
    }
    clamp_params(&mut v.params);
    for x in &mut v.item_lr {
        *x = clamp_signed_u32(*x);
    }
    for x in &mut v.rank_lr {
        *x = clamp_signed_u32(*x);
    }
    for x in &mut v.rank_type_gain {
        *x = (*x).min(100);
    }
    v.rank_level_gain = v.rank_level_gain.min(100);
    v.rank_duration = v.rank_duration.min(1000);
    v.out_smooth = v.out_smooth.min(100);
    v.throttle_window_ms = v.throttle_window_ms.min(10000);
    v.throttle_max_hits = v.throttle_max_hits.min(30);
    v.throttle_dense_ratio = v.throttle_dense_ratio.min(100);
    v.random_gain = v.random_gain.min(100);
    v.motor_l_gain = v.motor_l_gain.min(100);
    v.motor_r_gain = v.motor_r_gain.min(100);
    Some(v)
}
