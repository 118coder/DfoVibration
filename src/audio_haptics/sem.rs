//! ★v24.50 音频震动 — YAMNet 语义层 (audio-sem feature)。
//!
//! 推理后端: **tract-onnx** (纯 Rust, 无 C 依赖/无 DLL)。
//! 选型变更记录: 原选 ort 2.0.0-rc.15 (静态链接), 但 ort-sys 预编译二进制不支持
//! x86_64-pc-windows-gnu (本项目 MinGW 工具链铁律) → 改用调研报告 §4.3 备选
//! tract-onnx。YAMNet 是 3.7M 参数小模型, tract 推理速度完全够 0.5-1s 语义窗。
//!
//! 模型来源优先级 (用户决策 D1: 双轨):
//!   1. 编译期内嵌 (`build.rs` 检测到 models/yamnet.onnx → include_bytes, 个人版)
//!   2. exe 同目录 `models\yamnet.onnx` (公开版 mod)
//!   3. `%APPDATA%\SorahkDFO\models\yamnet.onnx` (mod 备选位置)
//! 模型缺失/加载失败 → 语义层自动失效, 纯 DSP 快通道照常 (降级路径, 玩家无感)。
//!
//! 推理线程: 每 ~480ms 取最新 2.0s@16kHz 窗 → tract 推理 → 521 类分数聚合
//! (top3 + Music/Speech/Gun/Explosion/Engine 族) → 写回 AppState 遥测。
//! 注: YAMNet 内部按 0.96s patch 滑 0.48s hop 取帧, 输入 2s 产多个 patch 取 max
//! (15360 样本恰在 patch 边界可能产 0 patch, 用 32000 样本 = 2.0s 稳妥)。

/// 类名族判定 (纯函数; 类名全部小写化后匹配)。
pub fn is_music(name: &str) -> bool {
    name.starts_with("music")
}
pub fn is_speech(name: &str) -> bool {
    name.contains("speech") || name == "conversation" || name == "chatter" || name == "narration"
}
pub fn is_gun(name: &str) -> bool {
    // 类目表实际名字: "Gunshot, gunfire" / "Machine gun" (Cap gun 排除)
    name.contains("gunshot") || name.contains("gunfire") || name == "machine gun"
}
pub fn is_explosion(name: &str) -> bool {
    name == "explosion" || name == "fireworks"
}
pub fn is_engine(name: &str) -> bool {
    name.starts_with("engine") || name == "vehicle" || name == "idling"
}

/// 解析类目表 CSV (`index,mid,display_name`) → 类名表 (纯函数)。
pub fn parse_class_map(csv: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in csv.lines().skip(1) {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        // display_name 可能带引号且含逗号: 取第一个逗号前是 index, 再取 mid 字段,
        // 剩余全部 (去引号) 是 display_name。
        let rest = match line.split_once(',') {
            Some((_, r)) => r,
            None => continue,
        };
        let name = match rest.split_once(',') {
            Some((_, n)) => n,
            None => continue,
        };
        out.push(name.trim().trim_matches('"').to_lowercase());
    }
    out
}

/// 模型来源描述 (GUI 显示; 双轨: 内嵌个人版 / mod 文件)。
pub fn model_source_info() -> String {
    #[cfg(feature = "audio-sem")]
    {
        if let Some((_, src)) = backend::resolve_model() {
            return src;
        }
    }
    "未找到 (语义层降级纯 DSP)".to_string()
}

/// mod 目录 (GUI「打开模型文件夹」用)。
pub fn mod_dir() -> std::path::PathBuf {
    #[cfg(feature = "audio-sem")]
    {
        return backend::mod_dir();
    }
    #[cfg(not(feature = "audio-sem"))]
    {
        return std::path::PathBuf::from("models");
    }
}

// ───────────────────────── 精简版 (无 audio-sem) ─────────────────────────
#[cfg(not(feature = "audio-sem"))]
pub mod backend {
    use super::*;
    use crate::state::AppState;
    use std::sync::Arc;

    /// 共享采样槽 (引擎喂; 精简版直接丢弃)。
    #[derive(Default)]
    pub struct SemFeed(pub std::sync::Mutex<Vec<f32>>);

    pub struct SemLayer;

    impl SemLayer {
        pub fn try_start(_state: Arc<AppState>, _feed: Arc<SemFeed>) -> Option<Self> {
            None
        }
        pub fn is_active(&self) -> bool {
            false
        }
    }
}

// ───────────────────────── 完整版 (ort + YAMNet) ─────────────────────────
#[cfg(feature = "audio-sem")]
pub mod backend {
    use super::*;
    use crate::state::{AppState, SemSnapshot};
    use std::path::PathBuf;
    use std::sync::atomic::Ordering;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    /// 语义层共享采样槽 (引擎线程写 16kHz 单声道, 推理线程每 480ms 取最新窗)。
    #[derive(Default)]
    pub struct SemFeed(pub Mutex<Vec<f32>>);

    /// 编译期内嵌模型 (build.rs 检测 models/yamnet.onnx 后注入)。
    #[cfg(yamnet_embed)]
    const EMBED_MODEL: &[u8] = include_bytes!(env!("YAMNET_EMBED_PATH"));
    #[cfg(not(yamnet_embed))]
    const EMBED_MODEL: Option<&[u8]> = None;

    /// 语义窗口长 (16kHz 样本数) = 2.0s —— 保证 YAMNet 内部 patch 划分 ≥1。
    const WINDOW_SAMPLES: usize = 32000;

    /// 类目表 (Apache-2.0, tensorflow/models audioset yamnet assets 逐字拷贝)。
    const CLASS_MAP_CSV: &str = include_str!("yamnet_class_map.csv");

    /// 模型字节解析 (内嵌 → exe 旁 → APPDATA; 返回字节 + 来源描述)。
    pub fn resolve_model() -> Option<(Vec<u8>, String)> {
        #[cfg(yamnet_embed)]
        {
            return Some((EMBED_MODEL.to_vec(), "内嵌 (个人版构建)".to_string()));
        }
        #[cfg(not(yamnet_embed))]
        {
            if let Ok(exe) = std::env::current_exe() {
                let p = exe.parent()?.join("models").join("yamnet.onnx");
                if p.is_file() {
                    if let Ok(b) = std::fs::read(&p) {
                        return Some((b, p.display().to_string()));
                    }
                }
            }
            if let Some(appdata) = std::env::var_os("APPDATA") {
                let p = PathBuf::from(appdata)
                    .join("SorahkDFO")
                    .join("models")
                    .join("yamnet.onnx");
                if p.is_file() {
                    if let Ok(b) = std::fs::read(&p) {
                        return Some((b, p.display().to_string()));
                    }
                }
            }
            None
        }
    }

    /// mod 目录 (GUI"打开模型文件夹"按钮用)。
    pub fn mod_dir() -> PathBuf {
        if let Ok(exe) = std::env::current_exe() {
            if let Some(parent) = exe.parent() {
                return parent.join("models");
            }
        }
        PathBuf::from("models")
    }

    pub struct SemLayer {
        handle: Option<std::thread::JoinHandle<()>>,
        stop: Arc<std::sync::atomic::AtomicBool>,
    }

    impl SemLayer {
        /// 尝试启动推理线程 (模型解析/加载失败 → None, 纯 DSP 降级)。
        pub fn try_start(state: Arc<AppState>, feed: Arc<SemFeed>) -> Option<Self> {
            let Some((bytes, source)) = resolve_model() else {
                crate::vibration::vib_log("[AUDIO-SEM] 未找到 yamnet.onnx (内嵌/mod 目录均无) → 纯 DSP 降级");
                return None;
            };
            let classes = parse_class_map(CLASS_MAP_CSV);
            if classes.len() != 521 {
                crate::vibration::vib_log(&format!(
                    "[AUDIO-SEM] 类目表行数异常: {} (期望 521) → 纯 DSP 降级",
                    classes.len()
                ));
                return None;
            }
            let session = match build_session(&bytes) {
                Ok(s) => s,
                Err(e) => {
                    crate::vibration::vib_log(&format!(
                        "[AUDIO-SEM] YAMNet 加载失败: {e} → 纯 DSP 降级"
                    ));
                    return None;
                }
            };
            crate::vibration::vib_log(&format!(
                "[AUDIO-SEM] YAMNet 已加载 ({source}), 521 类, 窗 {WINDOW_SAMPLES} 样本"
            ));
            let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let stop2 = stop.clone();
            let handle = std::thread::Builder::new()
                .name("audio_sem".to_string())
                .spawn(move || infer_loop(state, feed, session, classes, stop2))
                .ok()?;
            Some(SemLayer { handle: Some(handle), stop })
        }

        pub fn is_active(&self) -> bool {
            true
        }
    }

    impl Drop for SemLayer {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Relaxed);
            if let Some(h) = self.handle.take() {
                let _ = h.join();
            }
        }
    }

    /// "模型会话" = 闭包 (输入 16kHz 窗 → 输出 521 类最高分)。闭包封装 tract
    /// 计划, 规避命名 tract 内部类型 (SimplePlan 泛型签名跨版本不稳)。
    type SemModel = Box<dyn Fn(&[f32]) -> Result<Vec<f32>, String> + Send>;

    pub(crate) fn build_session(bytes: &[u8]) -> Result<SemModel, String> {
        use tract_onnx::prelude::*;
        let fact = f32::fact([WINDOW_SAMPLES as i64]);
        let model = tract_onnx::onnx()
            .model_for_read(&mut std::io::Cursor::new(bytes))
            .map_err(|e| format!("onnx 解析: {e}"))?
            .with_input_fact(0, fact.into())
            .map_err(|e| format!("输入形状: {e}"))?
            .into_optimized()
            .map_err(|e| format!("图优化: {e}"))?
            .into_runnable()
            .map_err(|e| format!("编译计划: {e}"))?;
        Ok(Box::new(move |window: &[f32]| {
            let arr =
                tract_ndarray::Array1::<f32>::from_iter(window.iter().copied()).into_dyn();
            let input: TValue = Tensor::from(arr).into();
            let outputs = model.run(tvec![input]).map_err(|e| e.to_string())?;
            if outputs.is_empty() {
                return Err("模型无输出".to_string());
            }
            let view = outputs[0]
                .to_plain_array_view::<f32>()
                .map_err(|e| format!("输出类型: {e}"))?;
            let shape = view.shape().to_vec();
            if shape.len() != 2 || shape[1] != 521 {
                return Err(format!("输出形状异常 {shape:?} (期望 [frames, 521])"));
            }
            let frames = shape[0];
            if frames == 0 {
                return Err("patch 数为 0 (输入窗过短?)".to_string());
            }
            let data: Vec<f32> = view.as_standard_layout().iter().copied().collect();
            let mut max_scores = vec![0f32; 521];
            for f in 0..frames {
                for c in 0..521 {
                    let v = data[f * 521 + c];
                    if v > max_scores[c] {
                        max_scores[c] = v;
                    }
                }
            }
            Ok(max_scores)
        }))
    }

    /// 推理主循环: 每 480ms 取最新窗 → 分数 → 族聚合 → 写遥测。
    fn infer_loop(
        state: Arc<AppState>,
        feed: Arc<SemFeed>,
        session: SemModel,
        classes: Vec<String>,
        stop: Arc<std::sync::atomic::AtomicBool>,
    ) {
        loop {
            if stop.load(Ordering::Relaxed) || state.should_exit() {
                return;
            }
            if !state.audio_hap.is_enabled() || !state.audio_hap.sem_enabled.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(200));
                continue;
            }
            // 取最新窗 (丢弃旧样本, 保留语义层 ~0.5s 的判断窗即可)
            let window = {
                let mut g = match feed.0.try_lock() {
                    Ok(g) => g,
                    Err(_) => {
                        std::thread::sleep(Duration::from_millis(50));
                        continue;
                    }
                };
                if g.len() < WINDOW_SAMPLES {
                    None
                } else {
                    let start = g.len() - WINDOW_SAMPLES;
                    Some(g.split_off(start))
                }
            };
            let Some(window) = window else {
                std::thread::sleep(Duration::from_millis(80));
                continue;
            };
            match run_inference(&session, &window) {
                Ok(scores) => {
                    let snap = aggregate(&classes, &scores);
                    if let Ok(mut t) = state.audio_hap.telemetry.try_lock() {
                        t.sem = snap;
                        t.sem_active = true;
                    }
                }
                Err(e) => {
                    // 推理失败: 记一条 (秒级节流), 语义层标记失效 → 门控自动旁路
                    if let Ok(mut t) = state.audio_hap.telemetry.try_lock() {
                        t.sem_active = false;
                    }
                    let now = crate::vibration::now_ms_u64();
                    if now.saturating_sub(LAST_ERR_LOG.load(Ordering::Relaxed)) > 5000 {
                        LAST_ERR_LOG.store(now, Ordering::Relaxed);
                        crate::vibration::vib_log(&format!("[AUDIO-SEM] 推理失败: {e}"));
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(480));
        }
    }

    static LAST_ERR_LOG: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

    /// 单窗推理 → 每类最高分 [521]。
    /// 单窗推理 → 每类最高分 [521] (tract: 输出 0 = [frames, 521] 分数矩阵)。
    pub(crate) fn run_inference(session: &SemModel, window: &[f32]) -> Result<Vec<f32>, String> {
        session(window)
    }

    /// 分数 → 语义快照 (族聚合 + top3; 纯函数便于单测)。
    pub fn aggregate(classes: &[String], scores: &[f32]) -> SemSnapshot {
        let mut snap = SemSnapshot {
            top3: Vec::with_capacity(3),
            ..Default::default()
        };
        let mut idx: Vec<usize> = (0..scores.len().min(classes.len())).collect();
        idx.sort_by(|a, b| scores[*b].partial_cmp(&scores[*a]).unwrap_or(std::cmp::Ordering::Equal));
        for &i in idx.iter().take(3) {
            snap.top3.push((classes[i].clone(), scores[i]));
        }
        for (i, name) in classes.iter().enumerate() {
            let s = scores.get(i).copied().unwrap_or(0.0);
            if s <= 0.0 {
                continue;
            }
            if is_music(name) && s > snap.music {
                snap.music = s;
            }
            if is_speech(name) && s > snap.speech {
                snap.speech = s;
            }
            if is_gun(name) && s > snap.gun {
                snap.gun = s;
            }
            if is_explosion(name) && s > snap.explosion {
                snap.explosion = s;
            }
            if is_engine(name) && s > snap.engine {
                snap.engine = s;
            }
        }
        snap.updated_ms = crate::vibration::now_ms_u64();
        snap
    }
}

#[cfg(feature = "audio-sem")]
pub use backend::{aggregate, SemFeed, SemLayer};
#[cfg(not(feature = "audio-sem"))]
pub use backend::{SemFeed, SemLayer};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_map_parses_521_entries() {
        let classes = parse_class_map(include_str!("yamnet_class_map.csv"));
        assert_eq!(classes.len(), 521, "YAMNet 类目必须是 521 类");
        assert_eq!(classes[0], "speech");
        // 关键类存在且名字与匹配器一致
        assert!(classes.iter().any(|n| n == "gunshot, gunfire"), "类名应为 'Gunshot, gunfire'");
        assert!(classes.iter().any(|n| n == "machine gun"));
        assert!(classes.iter().any(|n| n == "explosion"));
        assert!(classes.iter().any(|n| n == "fireworks"));
    }

    #[test]
    fn family_matchers_cover_real_class_names() {
        assert!(is_gun("gunshot, gunfire"));
        assert!(is_gun("machine gun"));
        assert!(!is_gun("cap gun"), "玩具枪不算枪声");
        assert!(!is_gun("engine"));
        assert!(is_music("music"));
        assert!(is_music("music of africa"));
        assert!(!is_music("speech"));
        assert!(is_speech("speech"));
        assert!(is_speech("child speech, kid speaking"));
        assert!(is_speech("male speech, man speaking"));
        assert!(is_explosion("explosion"));
        assert!(is_explosion("fireworks"));
        assert!(is_engine("engine starting"));
        assert!(is_engine("engine idling"));
        assert!(is_engine("vehicle"));
        assert!(!is_engine("car alarm"));
    }

    #[cfg(feature = "audio-sem")]
    #[test]
    fn aggregate_produces_top3_and_family_maxima() {
        let classes = vec![
            "speech".to_string(),
            "gunshot, gunfire".to_string(),
            "music".to_string(),
            "explosion".to_string(),
            "engine idling".to_string(),
        ];
        let scores = vec![0.7, 0.9, 0.65, 0.1, 0.55];
        let snap = aggregate(&classes, &scores);
        assert_eq!(snap.top3[0].0, "gunshot, gunfire");
        assert_eq!(snap.top3[1].0, "speech");
        assert_eq!(snap.top3[2].0, "music");
        assert!((snap.gun - 0.9).abs() < 1e-6);
        assert!((snap.speech - 0.7).abs() < 1e-6);
        assert!((snap.music - 0.65).abs() < 1e-6);
        assert!((snap.engine - 0.55).abs() < 1e-6);
        assert!((snap.explosion - 0.1).abs() < 1e-6);
        assert!(snap.updated_ms > 0);
    }

    #[cfg(feature = "audio-sem")]
    #[test]
    #[ignore = "需要 yamnet.onnx (个人版构建目录); 运行: cargo test sem_model_smoke -- --ignored --nocapture"]
    fn sem_model_smoke_load_and_infer() {
        let (bytes, src) = backend::resolve_model().expect("模型未找到 (构建目录应有 models/yamnet.onnx)");
        let t0 = std::time::Instant::now();
        let session = backend::build_session(&bytes).expect("tract 加载失败");
        let load_ms = t0.elapsed().as_millis();
        let window = vec![0f32; 32000];
        let t1 = std::time::Instant::now();
        let scores = backend::run_inference(&session, &window).expect("推理失败");
        let infer_ms = t1.elapsed().as_millis();
        let top: Vec<(usize, f32)> = scores
            .iter()
            .enumerate()
            .map(|(i, v)| (i, *v))
            .collect::<Vec<_>>()
            .into_iter()
            .collect();
        let mut sorted = top;
        sorted.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        println!(
            "来源={} 加载={}ms 推理={}ms top1=idx{}({:.3})",
            src, load_ms, infer_ms, sorted[0].0, sorted[0].1
        );
        assert_eq!(scores.len(), 521);
        assert!(infer_ms < 2000, "推理过慢: {}ms", infer_ms);
    }

    #[cfg(feature = "audio-sem")]
    #[test]
    fn aggregate_tolerates_missing_scores() {
        let classes = vec!["music".to_string(), "explosion".to_string()];
        let snap = aggregate(&classes, &[]);
        assert!(snap.top3.is_empty());
        assert_eq!(snap.music, 0.0);
    }
}
