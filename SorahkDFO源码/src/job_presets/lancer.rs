//! 魔枪士（男） - 专属高级算法 id=13 (v31)
use super::JobClass;

/// 生成 魔枪士（男） 的职业预设 (逐职业手工细调 + 专属算法)
pub fn jobs() -> super::JobPreset {
    super::JobPreset {
        base_job: "魔枪士（男）",
        classes: vec![
            JobClass {
                name: "征战者",
                params: [
            30, 0, 0, 0, 46, 45, 25, 0, 0, 100, 0, 40, 15, 26, 15, 15, 11, 28, 50,
            110, 110, 2, 10, 380, 40, 8, 45, 30, 12, 38, 500, 35, 1200, 50, 200, 40, 75, 35,
            45, 1000, 200, 90, 250, 20, 600, 1100, 4000, 170, 43, 30, 60, 40, 160, 45, 400, 140, 65,
            55, 25, 100,
                ],
                rank_lr: super::rank_lr_extreme(),
                rank_type_gain: super::rank_cfg(100, 380, 65, 25, 90, 95),
                rank_level_gain: 100,
                rank_duration: 380,
                out_smooth: 30,
                throttle_window: 0,
                throttle_max: 0,
                throttle_dense_ratio: 100,
                abs_freq_enabled: false,
                algo_id: 55,
                algo_params: [12, 75, 115, 95],
                desc: "战戟刚猛碾压, 极重 (专属算法: 13)",
            },
            JobClass {
                name: "决战者",
                params: [
            30, 0, 0, 0, 30, 30, 25, 0, 0, 100, 0, 55, 15, 24, 15, 15, 10, 23, 50,
            95, 95, 12, 40, 380, 40, 8, 45, 30, 12, 22, 500, 55, 1200, 50, 200, 45, 45, 35,
            45, 1800, 150, 90, 230, 20, 600, 2200, 3000, 140, 33, 30, 48, 50, 130, 45, 0, 140, 45,
            90, 25, 100,
                ],
                rank_lr: super::rank_lr_light(),
                rank_type_gain: super::rank_cfg(100, 300, 55, 25, 90, 85),
                rank_level_gain: 100,
                rank_duration: 300,
                out_smooth: 30,
                throttle_window: 0,
                throttle_max: 0,
                throttle_dense_ratio: 100,
                abs_freq_enabled: false,
                algo_id: 56,
                algo_params: [25, 28, 65, 40],
                desc: "枪术高速连刺, 灵动飘逸 (专属算法: 13)",
            },
        ],
    }
}
