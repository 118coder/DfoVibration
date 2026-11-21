//! ★双 UI 壳 —— cfg.write typed mutation 白名单表 (契约 §4, 附录 §A 的实现本体)。
//!
//! 语法: `a.b.3.c` = 点分段 + 数字数组下标; value = JSON 标量或整节点对象。
//!
//! 实现 = **JSON 子节点往返**: 把目标子结构序列化成 Value, 按白名单改路径, 再反序列化
//! 回类型结构 —— serde 做最终类型校验; 整批 mutations 先全部校验, 任一条失败整批拒绝
//! (契约「整体不半提交」)。`#[serde(skip)]` 的 `vibration`/`vibration_presets` 绝不
//! 走整份 AppConfig 往返 (会被 skip 清掉), 所以震动/音频/更新三族按**子结构**往返,
//! 顶层标量走显式 match 赋值。
//!
//! 白名单只收「既有 egui 页面同族参数」; 映射/预设/宏/白名单的增删改走 cmd 词表
//! (UpsertMapping 等), 不在本表。`vibration.algo_*` 是 JobVibration 职业叠加层
//! 会话专用运行态 (backend/vibration.rs「平时勿碰」), v1 不收录。
//!
//! 档位 (照 egui 同页既有 Commit, 详见规格书附录 §A):
//! * 震动族   → `Backend::update_vibration_config` (改 config + 引擎镜像 + 去抖落盘);
//!   消息 commit ∈ {save,vib,reload} 时追加 `update_config(Commit::SaveVib)` 立即落盘。
//! * 音频族   → `Backend::update_audio_config` (改 config + apply_config 镜像 + 去抖);
//!   消息 commit ∈ {save,config-only,reload} 时追加 `update_config(SaveConfigOnly)`。
//! * 更新族   → `update_config(SaveConfigOnly)` (只写 Config.toml)。
//! * 设置族   → 默认 `SaveConfigOnly` (契约 §4); 钉死 Reload 的字段见 TOP_*_RELOAD。
//! * language → `Commit::Reload` (照 egui SetLanguage 语义)。
//! * dfo_player / vib_legacy_client → 专用后端入口 (shell 分流), 不走本表 JSON 往返。

use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::{json, Value};

use crate::config::{AppConfig, AudioHapticsConfig, UpdateConfig, VibrationConfig};
use crate::ui_api::Commit;

// ── 字段清单 (单一事实来源; paths_table() 与校验共用) ─────────────────────

/// 震动族 u32 标量 (VibrationConfig)。
pub const VIB_U32: &[&str] = &[
    "motor_l_gain", "motor_r_gain", "random_gain", "out_smooth", "out_threshold",
    "remap_min", "split_thr", "rank_decay_delay_ms", "rank_decay_speed",
    "rank_decay_min_mul", "rank_decay_max_mul", "abs_freq_window_ms", "abs_freq_max",
    "throttle_window_ms", "throttle_max_hits", "throttle_dense_ratio", "merge_keep",
    "merge_cap", "merge_hold", "hitcap_max", "hitcap_win_ms", "hitmerge_ms",
    "sustain_secs", "sustain_reduce", "tail_land_pct", "monster_abnormal_gain",
    "storm_thr", "storm_keep_pct", "storm_win_ms", "storm_pause_ms", "storm_unified_ms",
    "legacy_output_mode", "attack_gain", "damage_gain", "shake_gain", "move_gain",
    "max_strength", "decay_ms", "hit_boost", "start_pulse", "kill_pulse", "master_gain",
    "font_strength", "font_interval", "rhythm", "font_hp", "font_special", "font_state",
    "font_effect", "font_attack", "font_hit", "rank_level_gain", "rank_duration",
];

/// 震动族 bool 标量。
pub const VIB_BOOL: &[&str] = &[
    "enabled", "advanced_enabled", "independent_test", "move_independent",
    "rank_decay_enabled", "remap_enabled", "split_enabled", "abs_freq_enabled",
    "storm_mute_abnormal", "storm_mute_rank", "merge_enabled", "hitcap_enabled",
    "storm_enabled", "sustain_enabled", "tail_land_enabled", "density_enabled",
    "adapt_enabled", "move_charge_enabled", "decay_enabled", "algo_windows_enabled",
    "pulse_enabled", "storm_unified_enabled",
];

/// 震动族 u32 定长数组 (名字, 长度)。
pub const VIB_ARRAYS: &[(&str, usize)] = &[
    ("item_lr", 26),
    ("rank_lr", 30),
    ("rank_type_gain", 15),
    ("advanced", 41),
];

/// 音频族 u32 标量 (AudioHapticsConfig)。
pub const AUDIO_U32: &[&str] = &[
    "master_gain_pct", "low_gain_pct", "high_gain_pct", "punch_pct", "mask_threshold",
    "music_mask_pct", "rhythm_pct", "texture_pct", "attack_pct", "release_pct",
    "dead_zone_pct", "map_max", "bass_high_hz", "vocal_low_hz", "vocal_high_hz",
    "smart_strength_pct", "smart_duration_pct",
    /* ★I 轮接线 (C 模式, 字段名照 config.rs): 事件层增强 / BGM 节奏抑制 */
    "c_upgrade_pct", "c_sustain_suppress_pct",
];

/// 音频族 bool 标量。
pub const AUDIO_BOOL: &[&str] = &[
    "enabled", "adaptive_threshold", "auto_profile", "quiet_mask", "dual_motor",
    "vocal_only", "whitelist_filter", "whitelist_enabled", "whitelist_include_tree",
    "smart_enabled",
    /* ★I 轮接线 (C 模式): BGM 筛选开关 (默认 true) */
    "c_bgm_filter",
];

/// 音频族字符串标量。
pub const AUDIO_STR: &[&str] = &[
    "preset", "whitelist_source", "whitelist_mode",
    /* ★I 轮接线 (C 模式): None=旧配置推导 A/B; "a"/"b"/"c" 显式模式 (非法值引擎降级 B) */
    "audio_algorithm_mode",
];

/// 音频族字符串数组。
pub const AUDIO_STRVEC: &[&str] = &["whitelist_processes"];

/// 更新族 (UpdateConfig): (字段, kind)。
pub const UPDATE_FIELDS: &[(&str, &str)] = &[
    ("auto_check", "bool"),
    ("manifest_url", "string"),
    ("skipped_version", "string"),
    ("download_dir", "string"),
    ("channel", "string"),
];

/// 顶层设置族 bool —— 默认 SaveConfigOnly (契约 §4 设置族; 消息 commit 可覆盖)。
pub const TOP_BOOL_LOOSE: &[&str] = &[
    "show_tray_icon", "show_notifications", "always_on_top", "dark_mode",
    "guide_seen", "minimal_mode", "classic_mode", "minimal_vib_preset_job",
];

/// 顶层设置族 —— 档位**钉死 Reload** 的字段 (照 egui 同页既有档位; 消息 commit 不覆盖)。
pub const TOP_BOOL_RELOAD: &[&str] = &["whitelist_enabled", "whitelist_simple_mode"];
pub const TOP_U32_RELOAD: &[&str] = &[
    "input_timeout", "interval", "event_duration", "combo_key_gap_ms", "worker_count",
];
pub const TOP_STR_RELOAD: &[&str] =
    &["switch_key", "rawinput_capture_mode", "xinput_capture_mode"];
pub const TOP_STRVEC_RELOAD: &[&str] = &["chord_exclusive_keys", "process_whitelist"];

/// 顶层字符串数组 —— 校准表, 只写 Config.toml。
pub const TOP_STRVEC_SAVE: &[&str] = &["hid_slot_usages"];

/// 顶层窗口矩形 [f32;4] | null。
pub const TOP_RECTS: &[&str] =
    &["window_rect_normal", "window_rect_minimal", "window_rect_classic"];

/// 特殊路径: 专用后端入口, 不走本表 JSON 往返 (shell 分流处理)。
/// dfo_player → BackendService::set_dfo_player (联动 vibration.enabled + 镜像 + Save);
/// vib_legacy_client → BackendService::switch_vib_edition (S1/S4 路线互换落盘)。
pub const SPECIAL_PATHS: &[(&str, &str)] = &[("dfo_player", "bool"), ("vib_legacy_client", "bool")];

// ── cfg.paths 动态发现 (B 只许从这里拿路径, 禁止猜) ──────────────────────

/// 白名单表 JSON (契约 §4: 应答 data = 白名单表)。
/// 行 = {path, kind, commit}; commit 为**默认档位**; fixed=true 表示消息 commit 不可覆盖。
pub fn paths_table() -> Value {
    let mut rows: Vec<Value> = Vec::new();
    for f in VIB_U32 {
        rows.push(json!({"path": format!("vibration.{f}"), "kind": "u32", "commit": "vib"}));
    }
    for f in VIB_BOOL {
        rows.push(json!({"path": format!("vibration.{f}"), "kind": "bool", "commit": "vib"}));
    }
    for (f, n) in VIB_ARRAYS {
        rows.push(json!({
            "path": format!("vibration.{f}"), "kind": format!("u32[{n}]"),
            "commit": "vib",
            "note": format!("整组写或 {f}.<i> 单槽 (i<{n})"),
        }));
    }
    for f in AUDIO_U32 {
        rows.push(json!({"path": format!("audio_haptics.{f}"), "kind": "u32", "commit": "memory"}));
    }
    for f in AUDIO_BOOL {
        rows.push(json!({"path": format!("audio_haptics.{f}"), "kind": "bool", "commit": "memory"}));
    }
    for f in AUDIO_STR {
        rows.push(json!({"path": format!("audio_haptics.{f}"), "kind": "string", "commit": "memory"}));
    }
    for f in AUDIO_STRVEC {
        rows.push(json!({"path": format!("audio_haptics.{f}"), "kind": "string[]", "commit": "memory"}));
    }
    for (f, k) in UPDATE_FIELDS {
        rows.push(json!({"path": format!("update.{f}"), "kind": k, "commit": "config-only"}));
    }
    for f in TOP_BOOL_LOOSE {
        rows.push(json!({"path": f, "kind": "bool", "commit": "config-only"}));
    }
    for f in TOP_BOOL_RELOAD {
        rows.push(json!({"path": f, "kind": "bool", "commit": "reload", "fixed": true}));
    }
    for f in TOP_U32_RELOAD {
        rows.push(json!({"path": f, "kind": "u32", "commit": "reload", "fixed": true}));
    }
    for f in TOP_STR_RELOAD {
        rows.push(json!({"path": f, "kind": "string", "commit": "reload", "fixed": true}));
    }
    for f in TOP_STRVEC_RELOAD {
        rows.push(json!({"path": f, "kind": "string[]", "commit": "reload", "fixed": true}));
    }
    for f in ["rawinput_capture_mode", "xinput_capture_mode"] {
        rows.push(json!({
            "path": format!("input.{f}"), "kind": "string",
            "commit": "reload", "fixed": true,
            "note": "B 侧别名 = 顶层同名字段; 值 = serde 变体名 (MostSustained/LastStable/...)",
        }));
    }
    for f in TOP_STRVEC_SAVE {
        rows.push(json!({"path": f, "kind": "string[]", "commit": "config-only"}));
    }
    for f in TOP_RECTS {
        rows.push(json!({"path": f, "kind": "rect4|null", "commit": "config-only"}));
    }
    rows.push(json!({"path": "language", "kind": "language", "commit": "reload", "fixed": true}));
    for (p, k) in SPECIAL_PATHS {
        rows.push(json!({"path": p, "kind": k, "commit": "save", "special": true}));
    }
    Value::Array(rows)
}

// ── 消息 commit 标签 ─────────────────────────────────────────────────────

/// cfg.write 消息里的 commit 字符串 (契约 §4: memory/save/config-only/vib/reload)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommitTag {
    Memory,
    Save,
    ConfigOnly,
    Vib,
    Reload,
}

impl CommitTag {
    pub fn parse(s: &str) -> Option<CommitTag> {
        match s {
            "memory" => Some(CommitTag::Memory),
            "save" => Some(CommitTag::Save),
            "config-only" => Some(CommitTag::ConfigOnly),
            "vib" => Some(CommitTag::Vib),
            "reload" => Some(CommitTag::Reload),
            _ => None,
        }
    }
}

// ── 单条 mutation 与族划分 ───────────────────────────────────────────────

/// 一条待写 mutation (value 为原始 JSON)。
#[derive(Debug, Clone)]
pub struct Mutation {
    /// 校验后按族存储时为**去前缀相对路径** ("item_lr.3" / "master_gain_pct");
    /// 输入给 validate_split 时为完整路径 ("vibration.item_lr.3")。
    pub path: String,
    pub value: Value,
}

/// 校验后的族划分 (族内 mutation 的 path 已去前缀)。
#[derive(Debug, Default)]
pub struct FamilySplit {
    pub vib: Vec<Mutation>,
    pub audio: Vec<Mutation>,
    pub update: Vec<Mutation>,
    /// 顶层标量/矩形/语言 (path 保持全名)。
    pub top: Vec<Mutation>,
    /// 特殊路径 (dfo_player / vib_legacy_client), shell 分流到专用后端入口。
    pub special: Vec<Mutation>,
    /// 顶层批次里有钉死 Reload 的条目 (whitelist/连发全局参数/language)。
    pub top_force_reload: bool,
}

/// 校验 (纯查表 + 类型检查, 不触碰配置), 全部通过返回族划分; 否则逐条报错。
pub fn validate_split(muts: &[Mutation]) -> Result<FamilySplit, Vec<String>> {
    let mut f = FamilySplit::default();
    let mut errs: Vec<String> = Vec::new();
    for m in muts {
        let path = m.path.trim();
        let segs: Vec<&str> = path.split('.').collect();
        let r = match segs.as_slice() {
            ["vibration", field] => {
                let kind = kind_of_vib(field);
                check_kind(path, kind, &m.value).map(|()| {
                    let value = if kind == Some("u32") { canon_u32(&m.value) } else { m.value.clone() };
                    f.vib.push(Mutation { path: (*field).into(), value });
                })
            }
            ["vibration", field, idx] => arr_slot(VIB_ARRAYS, field, idx, path, &m.value, "u32")
                .map(|()| {
                    f.vib.push(Mutation {
                        path: path_after(path, 1),
                        value: canon_u32(&m.value),
                    })
                }),
            ["audio_haptics", field] => {
                let kind = kind_of_audio(field);
                check_kind(path, kind, &m.value).map(|()| {
                    let value = if kind == Some("u32") { canon_u32(&m.value) } else { m.value.clone() };
                    f.audio.push(Mutation { path: (*field).into(), value });
                })
            }
            ["audio_haptics", field, idx] => arr_slot(
                &[("whitelist_processes", usize::MAX)],
                field,
                idx,
                path,
                &m.value,
                "string",
            )
            .map(|()| {
                f.audio.push(Mutation {
                    path: path_after(path, 1),
                    value: m.value.clone(),
                })
            }),
            ["update", field] => {
                let k = UPDATE_FIELDS.iter().find(|(n, _)| n == field).map(|(_, k)| *k);
                check_kind(path, k, &m.value).map(|()| {
                    f.update
                        .push(Mutation { path: (*field).into(), value: m.value.clone() })
                })
            }
            ["input", cap @ ("rawinput_capture_mode" | "xinput_capture_mode")] => {
                // B 侧 alias (交接 v12 §6): input.* → 顶层同名 String 字段 (值 = serde 变体名)
                let full = format!("{cap}");
                check_kind(&full, Some("string"), &m.value).map(|()| {
                    f.top.push(Mutation { path: full, value: m.value.clone() });
                    f.top_force_reload = true;
                })
            }
            ["language"] => {
                if is_language(&m.value) {
                    f.top.push(Mutation { path: "language".into(), value: m.value.clone() });
                    f.top_force_reload = true;
                    Ok(())
                } else {
                    Err(format!(
                        "{path}: 语言值非法 (SimplifiedChinese/English/TraditionalChinese/Japanese)"
                    ))
                }
            }
            _ => match classify_top(path) {
                Some(k) => check_kind(path, Some(k), &m.value).map(|()| {
                    if SPECIAL_PATHS.iter().any(|(p, _)| *p == path) {
                        f.special
                            .push(Mutation { path: path.to_string(), value: m.value.clone() });
                    } else {
                        if is_fixed_top(path) {
                            f.top_force_reload = true;
                        }
                        f.top
                            .push(Mutation { path: path.to_string(), value: m.value.clone() });
                    }
                }),
                None => Err(format!("{path}: 不在 cfg.write 白名单 (调 cfg.paths 查表)")),
            },
        };
        if let Err(e) = r {
            errs.push(e);
        }
    }
    if errs.is_empty() {
        Ok(f)
    } else {
        Err(errs)
    }
}

/// 顶层 (含特殊) 路径 → kind。
fn classify_top(path: &str) -> Option<&'static str> {
    if SPECIAL_PATHS.iter().any(|(p, _)| *p == path) {
        return Some("bool");
    }
    if TOP_BOOL_LOOSE.contains(&path) || TOP_BOOL_RELOAD.contains(&path) {
        return Some("bool");
    }
    if TOP_U32_RELOAD.contains(&path) {
        return Some("u32");
    }
    if TOP_STR_RELOAD.contains(&path) {
        return Some("string");
    }
    if TOP_STRVEC_RELOAD.contains(&path) || TOP_STRVEC_SAVE.contains(&path) {
        return Some("string[]");
    }
    if TOP_RECTS.contains(&path) {
        return Some("rect4|null");
    }
    None
}

fn is_fixed_top(path: &str) -> bool {
    TOP_BOOL_RELOAD.contains(&path)
        || TOP_U32_RELOAD.contains(&path)
        || TOP_STR_RELOAD.contains(&path)
        || TOP_STRVEC_RELOAD.contains(&path)
        || path == "language"
}

fn kind_of_vib(f: &str) -> Option<&'static str> {
    if VIB_U32.contains(&f) {
        return Some("u32");
    }
    if VIB_BOOL.contains(&f) {
        return Some("bool");
    }
    if VIB_ARRAYS.iter().any(|(n, _)| *n == f) {
        return Some("u32[]");
    }
    None
}

fn kind_of_audio(f: &str) -> Option<&'static str> {
    if AUDIO_U32.contains(&f) {
        return Some("u32");
    }
    if AUDIO_BOOL.contains(&f) {
        return Some("bool");
    }
    if AUDIO_STR.contains(&f) {
        return Some("string");
    }
    if AUDIO_STRVEC.contains(&f) {
        return Some("string[]");
    }
    None
}

/// JSON 数值 → u32。负值按 i32 位型折回 (v<0 → v+2³², B 侧滑杆 i32 语义约定,
/// 交接 v12 §6); 超范围拒绝。
fn as_u32(v: &Value) -> Option<u32> {
    if let Some(n) = v.as_i64() {
        if n < 0 {
            return if n >= i32::MIN as i64 { Some(n as i32 as u32) } else { None };
        }
        return if n <= u32::MAX as i64 { Some(n as u32) } else { None };
    }
    v.as_u64().filter(|n| *n <= u32::MAX as u64).map(|n| n as u32)
}

fn check_kind(path: &str, kind: Option<&str>, v: &Value) -> Result<(), String> {
    let Some(kind) = kind else {
        return Err(format!("{path}: 不在 cfg.write 白名单 (调 cfg.paths 查表)"));
    };
    let ok = match kind {
        "u32" => as_u32(v).is_some(),
        "bool" => v.is_boolean(),
        "string" => v.is_string(),
        "u32[]" => v.is_array() && v.as_array().unwrap().iter().all(|x| as_u32(x).is_some()),
        "string[]" => v.is_array() && v.as_array().unwrap().iter().all(|x| x.is_string()),
        "rect4|null" => {
            v.is_null()
                || (v.is_array()
                    && v.as_array().unwrap().len() == 4
                    && v.as_array().unwrap().iter().all(|x| x.is_number()))
        }
        "language" => is_language(v),
        _ => false,
    };
    if ok {
        Ok(())
    } else {
        Err(format!("{path}: 值类型不符 (期望 {kind})"))
    }
}

/// 数组下标槽校验 ("vibration.item_lr.5" / "audio_haptics.whitelist_processes.2")。
/// `leaf` = 槽内元素期望类型 (震动数组 u32; 音频白名单 string)。
fn arr_slot(
    arrays: &[(&str, usize)],
    field: &str,
    idx: &str,
    path: &str,
    v: &Value,
    leaf: &str,
) -> Result<(), String> {
    let Some((_, len)) = arrays.iter().find(|(n, _)| *n == field) else {
        return Err(format!("{path}: 不在 cfg.write 白名单"));
    };
    let Ok(i) = idx.parse::<usize>() else {
        return Err(format!("{path}: 数组下标必须是数字"));
    };
    if i >= *len {
        return Err(format!("{path}: 下标越界 (长度 {len})"));
    }
    let ok = match leaf {
        "u32" => as_u32(v).is_some(),
        "string" => v.is_string(),
        _ => false,
    };
    if !ok {
        return Err(format!("{path}: 值类型不符 (期望 {leaf})"));
    }
    Ok(())
}

/// u32 类值规范化: 负值按 i32 位型折回正数 (serde u32 反解才放行)。
fn canon_u32(v: &Value) -> Value {
    as_u32(v).map(|n| json!(n)).unwrap_or_else(|| v.clone())
}

fn is_language(v: &Value) -> bool {
    v.as_str()
        .and_then(|s| serde_json::from_value::<crate::i18n::Language>(json!(s)).ok())
        .is_some()
}

/// 取点路径第 skip 段之后的剩余串 ("vibration.item_lr.5", 1 → "item_lr.5")。
fn path_after(path: &str, skip: usize) -> String {
    path.splitn(skip + 1, '.').nth(skip).unwrap_or(path).to_string()
}

/// 在 JSON 节点上按点路径设值; 只许改**已存在**的键/槽 (防拼错路径造出新结构)。
fn set_json_path(node: &mut Value, rel: &str, v: Value) -> Result<(), String> {
    let segs: Vec<&str> = rel.split('.').collect();
    set_segs(node, &segs, rel, v)
}

/// 递归下钻 (借用清晰: 每层只借一个槽)。
fn set_segs(node: &mut Value, segs: &[&str], rel: &str, v: Value) -> Result<(), String> {
    let Some((seg, rest)) = segs.split_first() else {
        return Err(format!("{rel}: 空路径"));
    };
    match node {
        Value::Object(map) => {
            if !map.contains_key(*seg) {
                return Err(format!("{rel}: 字段不存在 ({seg})"));
            }
            let slot = map.get_mut(*seg).unwrap();
            if rest.is_empty() {
                *slot = v;
                Ok(())
            } else {
                set_segs(slot, rest, rel, v)
            }
        }
        Value::Array(arr) => {
            let Ok(idx) = seg.parse::<usize>() else {
                return Err(format!("{rel}: 数组下标必须是数字 ({seg})"));
            };
            if idx >= arr.len() {
                return Err(format!("{rel}: 下标越界 (长度 {})", arr.len()));
            }
            let slot = &mut arr[idx];
            if rest.is_empty() {
                *slot = v;
                Ok(())
            } else {
                set_segs(slot, rest, rel, v)
            }
        }
        _ => Err(format!("{rel}: 路径深入到标量节点 ({seg})")),
    }
}

/// 通用子节点往返: 序列化 → 逐条设值 → 反序列化回类型 (兜底校验)。
/// `current` 按值传入 (调用方先拷出), 新值返回 (调用方再赋回) —— 借用零交叉。
fn apply_node<T: Serialize + DeserializeOwned>(
    current: T,
    rel_muts: &[Mutation],
    fam: &str,
) -> Result<T, Vec<String>> {
    let mut node = serde_json::to_value(&current)
        .map_err(|e| vec![format!("{fam}: 序列化失败: {e}")])?;
    for m in rel_muts {
        if let Err(e) = set_json_path(&mut node, &m.path, m.value.clone()) {
            return Err(vec![format!("{fam}.{}: {e}", m.path)]);
        }
    }
    serde_json::from_value::<T>(node)
        .map_err(|e| vec![format!("{fam}: 反序列化校验失败: {e}")])
}

// ── 各族应用入口 (shell 按族接 backend 单一写路径) ───────────────────────

/// 震动族 → `Backend::update_vibration_config(|v| apply_vib(v, …))`。
pub fn apply_vib(v: &mut VibrationConfig, muts: &[Mutation]) -> Result<(), Vec<String>> {
    match apply_node(*v, muts, "vibration") {
        Ok(nv) => {
            *v = nv;
            Ok(())
        }
        Err(e) => Err(e),
    }
}

/// 音频族 → `Backend::update_audio_config(|a| apply_audio(a, …))`。
pub fn apply_audio(a: &mut AudioHapticsConfig, muts: &[Mutation]) -> Result<(), Vec<String>> {
    match apply_node(a.clone(), muts, "audio_haptics") {
        Ok(na) => {
            *a = na;
            Ok(())
        }
        Err(e) => Err(e),
    }
}

/// 更新族 → `Backend::update_config(档位, |c| apply_update(&mut c.update, …))`。
pub fn apply_update(u: &mut UpdateConfig, muts: &[Mutation]) -> Result<(), Vec<String>> {
    match apply_node(u.clone(), muts, "update") {
        Ok(nu) => {
            *u = nu;
            Ok(())
        }
        Err(e) => Err(e),
    }
}

/// 顶层标量 (含 language) —— 显式 match 赋值 (绝不做整份 AppConfig 往返)。
pub fn apply_top(c: &mut AppConfig, muts: &[Mutation]) -> Result<(), Vec<String>> {
    for m in muts {
        let v = &m.value;
        match m.path.as_str() {
            "show_tray_icon" => c.show_tray_icon = v.as_bool().unwrap_or(c.show_tray_icon),
            "show_notifications" => {
                c.show_notifications = v.as_bool().unwrap_or(c.show_notifications);
            }
            "always_on_top" => c.always_on_top = v.as_bool().unwrap_or(c.always_on_top),
            "dark_mode" => c.dark_mode = v.as_bool().unwrap_or(c.dark_mode),
            "guide_seen" => c.guide_seen = v.as_bool().unwrap_or(c.guide_seen),
            "minimal_mode" => c.minimal_mode = v.as_bool().unwrap_or(c.minimal_mode),
            "classic_mode" => c.classic_mode = v.as_bool().unwrap_or(c.classic_mode),
            "minimal_vib_preset_job" => {
                c.minimal_vib_preset_job = v.as_bool().unwrap_or(c.minimal_vib_preset_job);
            }
            "whitelist_enabled" => c.whitelist_enabled = v.as_bool().unwrap_or(c.whitelist_enabled),
            "whitelist_simple_mode" => {
                c.whitelist_simple_mode = v.as_bool().unwrap_or(c.whitelist_simple_mode);
            }
            "input_timeout" => c.input_timeout = v.as_u64().unwrap_or(c.input_timeout),
            "interval" => c.interval = v.as_u64().unwrap_or(c.interval),
            "event_duration" => c.event_duration = v.as_u64().unwrap_or(c.event_duration),
            "combo_key_gap_ms" => c.combo_key_gap_ms = v.as_u64().unwrap_or(c.combo_key_gap_ms),
            "worker_count" => {
                c.worker_count = as_u32(v).map(|n| n as usize).unwrap_or(c.worker_count);
            }
            "switch_key" => c.switch_key = v.as_str().unwrap_or(&c.switch_key).to_string(),
            "rawinput_capture_mode" => {
                c.rawinput_capture_mode =
                    v.as_str().unwrap_or(&c.rawinput_capture_mode).to_string();
            }
            "xinput_capture_mode" => {
                c.xinput_capture_mode =
                    v.as_str().unwrap_or(&c.xinput_capture_mode).to_string();
            }
            "chord_exclusive_keys" => c.chord_exclusive_keys = str_vec(v),
            "process_whitelist" => c.process_whitelist = str_vec(v),
            "hid_slot_usages" => c.hid_slot_usages = str_vec(v),
            "window_rect_normal" => c.window_rect_normal = if v.is_null() { None } else { rect4(v).or(c.window_rect_normal) },
            "window_rect_minimal" => c.window_rect_minimal = if v.is_null() { None } else { rect4(v).or(c.window_rect_minimal) },
            "window_rect_classic" => c.window_rect_classic = if v.is_null() { None } else { rect4(v).or(c.window_rect_classic) },
            "language" => match serde_json::from_value::<crate::i18n::Language>(v.clone()) {
                Ok(l) => c.language = l,
                Err(e) => return Err(vec![format!("language: {e}")]),
            },
            other => return Err(vec![format!("{other}: 顶层赋值分支缺失 (表/码漂移)")]),
        }
    }
    Ok(())
}

/// 一步到位: 校验 + 全族应用到 `&mut AppConfig` (测试与简单调用方用)。
/// 返回分拣出的 special 路径列表 (dfo_player / vib_legacy_client, 由专用入口写)。
pub fn apply_mutations(
    c: &mut AppConfig,
    muts: &[Mutation],
) -> Result<Vec<Mutation>, Vec<String>> {
    let f = validate_split(muts)?;
    if !f.vib.is_empty() {
        c.vibration = apply_node(c.vibration, &f.vib, "vibration")?;
    }
    if !f.audio.is_empty() {
        c.audio_haptics = apply_node(c.audio_haptics.clone(), &f.audio, "audio_haptics")?;
    }
    if !f.update.is_empty() {
        c.update = apply_node(c.update.clone(), &f.update, "update")?;
    }
    apply_top(c, &f.top)?;
    Ok(f.special)
}

fn str_vec(v: &Value) -> Vec<String> {
    v.as_array()
        .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
        .unwrap_or_default()
}

fn rect4(v: &Value) -> Option<[f32; 4]> {
    v.as_array().and_then(|a| {
        if a.len() != 4 {
            return None;
        }
        let g = |i: usize| a[i].as_f64().map(|x| x as f32);
        Some([g(0)?, g(1)?, g(2)?, g(3)?])
    })
}

/// 顶层批次的生效档位 (有钉死 Reload 的条目 → Reload; 否则用消息 commit, 缺省 ConfigOnly)。
pub fn top_commit(force_reload: bool, msg_commit: Option<CommitTag>) -> Commit {
    if force_reload {
        Commit::Reload
    } else {
        match msg_commit {
            Some(CommitTag::Memory) => Commit::Memory,
            Some(CommitTag::Save) => Commit::Save,
            Some(CommitTag::Reload) => Commit::Reload,
            _ => Commit::SaveConfigOnly,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn default_config() -> AppConfig {
        AppConfig::default()
    }

    /// 表里每条 path 的 apply 往返测试 (契约 §4: 每条 path 写 apply 往返单测)。
    #[test]
    fn every_table_path_roundtrip() {
        let sample = |kind: &str| -> Value {
            match kind {
                "u32" => json!(77),
                "bool" => json!(true),
                "string" => json!("sample"),
                "string[]" => json!(["a", "b.exe"]),
                "rect4|null" => json!([10.0, 20.0, 1180.0, 760.0]),
                "language" => json!("English"),
                other => panic!("未知 kind {other}"),
            }
        };

        let mut c = default_config();
        let mut checked = 0usize;
        for row in paths_table().as_array().unwrap() {
            let path = row["path"].as_str().unwrap();
            let kind = row["kind"].as_str().unwrap();
            if kind.starts_with("u32[") {
                // 定长数组整组写
                let n: usize =
                    kind.trim_start_matches("u32[").trim_end_matches(']').parse().unwrap();
                let v = Value::Array((0..n).map(|i| json!(i as u64 + 1)).collect());
                apply_mutations(&mut c, &[Mutation { path: path.into(), value: v }])
                    .unwrap_or_else(|e| panic!("{path} 整组往返失败: {e:?}"));
                let node = serde_json::to_value(&c.vibration).unwrap();
                let rel = path.strip_prefix("vibration.").unwrap();
                let back = navigate(&node, rel);
                assert_eq!(back.as_array().unwrap().len(), n, "{path} 往返长度");
                checked += 1;
                continue;
            }
            let v = sample(kind);
            let specials =
                apply_mutations(&mut c, &[Mutation { path: path.into(), value: v.clone() }])
                    .unwrap_or_else(|e| panic!("{path} 往返失败: {e:?}"));
            if specials.is_empty() {
                let back = read_back(&c, path);
                assert_eq!(back, Some(v), "{path} apply 往返不一致");
            } else {
                // 特殊路径 apply_mutations 只分拣不落值 (shell 专用入口写), 验证分拣正确
                assert!(path == "dfo_player" || path == "vib_legacy_client");
            }
            checked += 1;
        }
        assert!(checked >= 100, "白名单表条目异常稀少: {checked}");
    }

    /// ★I 轮接线 (C 模式): 白名单表补齐 audio_algorithm_mode / c_upgrade_pct /
    /// c_sustain_suppress_pct / c_bgm_filter 四字段 (字段名照 config.rs),
    /// 模拟 web 音频页 C 选择的 cfg.write 批次 (egui update_audio_config 同款语义)。
    #[test]
    fn c_mode_whitelist_fields_roundtrip() {
        // 四字段必须在动态发现表里 (B 侧 cfg.paths 可见)
        let paths: Vec<String> = paths_table()
            .as_array().unwrap()
            .iter()
            .filter_map(|r| r["path"].as_str().map(String::from))
            .collect();
        for p in [
            "audio_haptics.audio_algorithm_mode",
            "audio_haptics.c_upgrade_pct",
            "audio_haptics.c_sustain_suppress_pct",
            "audio_haptics.c_bgm_filter",
        ] {
            assert!(paths.iter().any(|x| x == p), "{p} 不在 cfg.paths 表");
        }

        // web C 选择的真实写法: smart_enabled=true + mode="c" + 两条滑条 + 筛选开关
        let mut c = default_config();
        apply_mutations(
            &mut c,
            &[
                Mutation { path: "audio_haptics.smart_enabled".into(), value: json!(true) },
                Mutation { path: "audio_haptics.audio_algorithm_mode".into(), value: json!("c") },
                Mutation { path: "audio_haptics.c_upgrade_pct".into(), value: json!(70) },
                Mutation { path: "audio_haptics.c_sustain_suppress_pct".into(), value: json!(90) },
                Mutation { path: "audio_haptics.c_bgm_filter".into(), value: json!(true) },
            ],
        )
        .unwrap_or_else(|e| panic!("C 模式批次被拒: {e:?}"));
        assert_eq!(c.audio_haptics.smart_enabled, true);
        assert_eq!(c.audio_haptics.audio_algorithm_mode.as_deref(), Some("c"));
        assert_eq!(c.audio_haptics.c_upgrade_pct, 70);
        assert_eq!(c.audio_haptics.c_sustain_suppress_pct, 90);
        assert_eq!(c.audio_haptics.c_bgm_filter, true);

        // 超界 u32 由 serde 往返兜底校验 (audio_state apply_config 再 clamp 0..=100)
        let mut c2 = default_config();
        assert!(apply_mutations(
            &mut c2,
            &[Mutation { path: "audio_haptics.c_upgrade_pct".into(), value: json!(99999) }],
        )
        .is_ok());
        assert_eq!(c2.audio_haptics.c_upgrade_pct, 99999);
    }

    /// 单槽写: vibration.item_lr.3 / rank_type_gain.14 / audio_haptics.whitelist_processes.0
    #[test]
    fn array_slot_paths() {
        let mut c = default_config();
        apply_mutations(
            &mut c,
            &[Mutation { path: "vibration.item_lr.3".into(), value: json!(55) }],
        )
        .unwrap();
        assert_eq!(c.vibration.item_lr[3], 55);
        assert_eq!(c.vibration.item_lr[0], 0, "其他槽不受影响");

        apply_mutations(
            &mut c,
            &[Mutation { path: "vibration.rank_type_gain.14".into(), value: json!(90) }],
        )
        .unwrap();
        assert_eq!(c.vibration.rank_type_gain[14], 90);

        // 单槽写的前提是槽已存在 (空列表 → 先整组写)
        apply_mutations(
            &mut c,
            &[Mutation {
                path: "audio_haptics.whitelist_processes".into(),
                value: json!(["old.exe"]),
            }],
        )
        .unwrap();
        apply_mutations(
            &mut c,
            &[Mutation {
                path: "audio_haptics.whitelist_processes.0".into(),
                value: json!("dnf.exe"),
            }],
        )
        .unwrap();
        assert_eq!(c.audio_haptics.whitelist_processes[0], "dnf.exe");
        // 空列表上单槽写必须被拒
        let mut c2 = default_config();
        let errs = apply_mutations(
            &mut c2,
            &[Mutation {
                path: "audio_haptics.whitelist_processes.0".into(),
                value: json!("dnf.exe"),
            }],
        )
        .unwrap_err();
        assert!(errs[0].contains("越界"), "{errs:?}");

        // 越界
        for bad in ["vibration.item_lr.26", "vibration.advanced.41", "vibration.rank_lr.30"] {
            let errs = apply_mutations(
                &mut c,
                &[Mutation { path: bad.into(), value: json!(1) }],
            )
            .unwrap_err();
            assert!(errs[0].contains("越界"), "{bad}: {errs:?}");
        }
    }

    /// 整体不半提交: 一条坏 mutation → 整批拒绝, 任何字段都没动。
    #[test]
    fn batch_is_atomic() {
        let mut c = default_config();
        let before = c.vibration.master_gain;
        let errs = apply_mutations(
            &mut c,
            &[
                Mutation { path: "vibration.master_gain".into(), value: json!(88) },
                Mutation { path: "vibration.not_a_field".into(), value: json!(1) },
            ],
        )
        .unwrap_err();
        assert_eq!(errs.len(), 1);
        assert!(errs[0].contains("不在 cfg.write 白名单"));
        assert_eq!(c.vibration.master_gain, before, "半提交发生了!");

        // 类型错同样整批拒绝
        assert!(apply_mutations(
            &mut c,
            &[
                Mutation { path: "vibration.enabled".into(), value: json!("yes") },
                Mutation { path: "dark_mode".into(), value: json!(true) },
            ],
        )
        .is_err());
        assert!(!c.dark_mode, "类型错批次的合法条目不该生效");

        // 越界下标同样 (enabled 默认 true, 样本写 false —— 整批被拒后应保持 true)
        assert!(apply_mutations(
            &mut c,
            &[
                Mutation { path: "vibration.rank_lr.30".into(), value: json!(1) },
                Mutation { path: "vibration.enabled".into(), value: json!(false) },
            ],
        )
        .is_err());
        assert!(c.vibration.enabled, "越界批次的合法条目也不该生效 (默认值被动了)");
    }

    /// 负值按 i32 位型写 u32 (B 约定: v<0 → v+2³², 交接 v12 §6)。
    #[test]
    fn negative_i32_bitpattern() {
        let mut c = default_config();
        apply_mutations(
            &mut c,
            &[Mutation { path: "vibration.master_gain".into(), value: json!(-1) }],
        )
        .unwrap();
        assert_eq!(c.vibration.master_gain, u32::MAX);
        apply_mutations(
            &mut c,
            &[Mutation { path: "vibration.item_lr.0".into(), value: json!(-2) }],
        )
        .unwrap();
        assert_eq!(c.vibration.item_lr[0], u32::MAX - 1);
        // 超出 i32 位型的负值必须拒绝
        assert!(apply_mutations(
            &mut c,
            &[Mutation { path: "vibration.master_gain".into(), value: json!(-4294967296i64) }],
        )
        .is_err());
    }

    /// B 侧 input.* 捕获判定别名。
    #[test]
    fn input_capture_alias() {
        let mut c = default_config();
        apply_mutations(
            &mut c,
            &[Mutation {
                path: "input.rawinput_capture_mode".into(),
                value: json!("MostSustained"),
            }],
        )
        .unwrap();
        assert_eq!(c.rawinput_capture_mode, "MostSustained");
        apply_mutations(
            &mut c,
            &[Mutation { path: "input.xinput_capture_mode".into(), value: json!("LastStable") }],
        )
        .unwrap();
        assert_eq!(c.xinput_capture_mode, "LastStable");
    }

    /// process_whitelist 整节点。
    #[test]
    fn process_whitelist_whole_node() {
        let mut c = default_config();
        apply_mutations(
            &mut c,
            &[Mutation {
                path: "process_whitelist".into(),
                value: json!(["dnf.exe", "E:/games/DFO.exe"]),
            }],
        )
        .unwrap();
        assert_eq!(c.process_whitelist.len(), 2);
        assert_eq!(f_top_reload_flag("process_whitelist"), true);
    }

    fn f_top_reload_flag(path: &str) -> bool {
        let f = validate_split(&[Mutation { path: path.into(), value: json!([]) }]).unwrap();
        f.top_force_reload
    }

    /// 语言枚举值校验。
    #[test]
    fn language_path() {
        let mut c = default_config();
        apply_mutations(
            &mut c,
            &[Mutation { path: "language".into(), value: json!("Japanese") }],
        )
        .unwrap();
        assert!(matches!(c.language, crate::i18n::Language::Japanese));
        let errs = apply_mutations(
            &mut c,
            &[Mutation { path: "language".into(), value: json!("Klingon") }],
        )
        .unwrap_err();
        assert!(errs[0].contains("语言值非法"));
    }

    /// rect4 null 回退。
    #[test]
    fn window_rect_null_and_value() {
        let mut c = default_config();
        apply_mutations(
            &mut c,
            &[Mutation { path: "window_rect_normal".into(), value: json!([1.0, 2.0, 3.0, 4.0]) }],
        )
        .unwrap();
        assert_eq!(c.window_rect_normal, Some([1.0, 2.0, 3.0, 4.0]));
        apply_mutations(
            &mut c,
            &[Mutation { path: "window_rect_normal".into(), value: json!(null) }],
        )
        .unwrap();
        assert_eq!(c.window_rect_normal, None);
    }

    /// paths_table 与校验实现互检: 表内每条 path 必须能过校验, 表外路径必须被拒。
    #[test]
    fn table_matches_validator() {
        let mut c = default_config();
        for row in paths_table().as_array().unwrap() {
            let path = row["path"].as_str().unwrap();
            let kind = row["kind"].as_str().unwrap();
            let v = if kind.starts_with("u32[") {
                let n: usize =
                    kind.trim_start_matches("u32[").trim_end_matches(']').parse().unwrap();
                Value::Array((0..n).map(|i| json!(i as u64)).collect())
            } else {
                match kind {
                    "u32" => json!(5),
                    "bool" => json!(false),
                    "string" => json!("s"),
                    "string[]" => json!([]),
                    "rect4|null" => json!(null),
                    "language" => json!("English"),
                    _ => panic!("{path} 未知 kind"),
                }
            };
            let r = apply_mutations(&mut c, &[Mutation { path: path.into(), value: v }]);
            assert!(r.is_ok(), "表内路径校验失败: {path} → {:?}", r.err());
        }
        for bad in [
            "vibration.algo_id",
            "vibration.algo_ap",
            "mappings.0.trigger_key",
            "current_preset",
            "secret_flag",
            "",
        ] {
            let r = apply_mutations(&mut c, &[Mutation { path: bad.into(), value: json!(1) }]);
            assert!(r.is_err(), "表外路径竟通过: {bad}");
        }
    }

    /// validate_split 的族分拣 + 混族批次各写各的。
    #[test]
    fn family_split() {
        let f = validate_split(&[
            Mutation { path: "vibration.master_gain".into(), value: json!(80) },
            Mutation { path: "audio_haptics.master_gain_pct".into(), value: json!(90) },
            Mutation { path: "update.channel".into(), value: json!("full") },
            Mutation { path: "dark_mode".into(), value: json!(true) },
            Mutation { path: "dfo_player".into(), value: json!(false) },
        ])
        .unwrap();
        assert_eq!(f.vib.len(), 1);
        assert_eq!(f.audio.len(), 1);
        assert_eq!(f.update.len(), 1);
        assert_eq!(f.top.len(), 1);
        assert_eq!(f.special.len(), 1);
        assert_eq!(f.vib[0].path, "master_gain");
        assert_eq!(f.audio[0].path, "master_gain_pct");
        assert!(!f.top_force_reload);
        // 混族 apply
        let mut c = default_config();
        apply_mutations(
            &mut c,
            &[
                Mutation { path: "vibration.master_gain".into(), value: json!(80) },
                Mutation { path: "dark_mode".into(), value: json!(true) },
            ],
        )
        .unwrap();
        assert_eq!(c.vibration.master_gain, 80);
        assert!(c.dark_mode);
    }

    fn navigate(node: &Value, rel: &str) -> Value {
        let mut cur = node;
        for seg in rel.split('.') {
            if let Ok(i) = seg.parse::<usize>() {
                cur = &cur.as_array().unwrap()[i];
            } else {
                cur = &cur.as_object().unwrap()[seg];
            }
        }
        cur.clone()
    }

    fn read_back(c: &AppConfig, path: &str) -> Option<Value> {
        // B 侧 input.* 别名落到顶层同名字段
        let path: &str = &path.strip_prefix("input.").map(String::from).unwrap_or_else(|| path.to_string());
        let (fam, rel) = match path.split_once('.') {
            Some(("vibration", r)) => ("vibration", r),
            Some(("audio_haptics", r)) => ("audio_haptics", r),
            Some(("update", r)) => ("update", r),
            _ => return read_back_top(c, path),
        };
        let node = match fam {
            "vibration" => serde_json::to_value(&c.vibration).unwrap(),
            "audio_haptics" => serde_json::to_value(&c.audio_haptics).unwrap(),
            _ => serde_json::to_value(&c.update).unwrap(),
        };
        Some(navigate(&node, rel))
    }

    fn read_back_top(c: &AppConfig, path: &str) -> Option<Value> {
        Some(match path {
            "show_tray_icon" => json!(c.show_tray_icon),
            "show_notifications" => json!(c.show_notifications),
            "always_on_top" => json!(c.always_on_top),
            "dark_mode" => json!(c.dark_mode),
            "guide_seen" => json!(c.guide_seen),
            "minimal_mode" => json!(c.minimal_mode),
            "classic_mode" => json!(c.classic_mode),
            "minimal_vib_preset_job" => json!(c.minimal_vib_preset_job),
            "whitelist_enabled" => json!(c.whitelist_enabled),
            "whitelist_simple_mode" => json!(c.whitelist_simple_mode),
            "input_timeout" => json!(c.input_timeout),
            "interval" => json!(c.interval),
            "event_duration" => json!(c.event_duration),
            "combo_key_gap_ms" => json!(c.combo_key_gap_ms),
            "worker_count" => json!(c.worker_count),
            "switch_key" => json!(c.switch_key),
            "rawinput_capture_mode" => json!(c.rawinput_capture_mode),
            "xinput_capture_mode" => json!(c.xinput_capture_mode),
            "chord_exclusive_keys" => json!(c.chord_exclusive_keys),
            "process_whitelist" => json!(c.process_whitelist),
            "hid_slot_usages" => json!(c.hid_slot_usages),
            "window_rect_normal" => json!(c.window_rect_normal),
            "window_rect_minimal" => json!(c.window_rect_minimal),
            "window_rect_classic" => json!(c.window_rect_classic),
            "language" => json!(c.language),
            _ => return None,
        })
    }
}
