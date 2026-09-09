//! 外传 - 专属高级算法 id=14 (v31)
use super::JobClass;

/// 生成 外传 的职业预设 (逐职业手工细调 + 专属算法)
pub fn jobs() -> super::JobPreset {
    super::JobPreset {
        base_job: "外传",
        classes: vec![
            JobClass {
                name: "黑暗武士",
                params: [
            26, 0, 0, 0, 37, 36, 25, 0, 0, 100, 0, 50, 15, 22, 15, 15, 9, 24, 50,
            100, 100, 12, 40, 380, 40, 8, 45, 30, 12, 25, 500, 55, 1200, 50, 200, 40, 50, 35,
            45, 1800, 150, 90, 250, 20, 600, 2200, 3000, 140, 33, 30, 50, 40, 160, 45, 0, 140, 50,
            90, 25, 100,
                ],
                rank_lr: super::rank_lr_medium(),
                rank_type_gain: super::rank_cfg(100, 310, 60, 25, 90, 90),
                rank_level_gain: 100,
                rank_duration: 310,
                out_smooth: 30,
                throttle_window: 0,
                throttle_max: 0,
                throttle_dense_ratio: 100,
                abs_freq_enabled: false,
                algo_id: 57,
                algo_params: [800, 50, 75, 55],
                desc: "自创连段, 自由度高 (专属算法: 14)",
            },
        ],
    }
}
