//! ★R2.14 震动领域 (BackendService impl)。
//!
//! 单一写路径纪律：**任何震动参数改动都先改 `config.vibration`，再整体镜像进
//! 引擎原子** (`apply_vibration_config`)，落盘走路线文件。职业叠加层
//! (`vibration_algo_id/ap`，JobVibration 路径) 不受镜像影响，只由显式方法复位。
//!
//! 槽位常量唯一出处 = `vibration/params.rs` (P_* / 双向映射函数，GUI 渲染共用)。
//! 预设应用语义 = 旧 `gui/vibration_page/presets.rs::apply_general_vibration_preset_in`
//! 的 config/原子部分逐字移植 (job 会话状态仍归 GUI 管理)。

use std::sync::atomic::Ordering::Relaxed;

use super::BackendService;
use crate::config::AppConfig;
use crate::ui_api::Commit;
use crate::vibration::params::{config_from_params, params_from_config};

impl BackendService {
    // ── 60 槽参数 (滑杆热路径：改 config → 镜像 → 去抖落盘) ─────────────

    /// 设置单个震动参数槽 (params.rs 常量定槽位；0-18 具名字段，19-59 advanced)。
    pub fn set_vib_param(&self, slot: usize, value: u32) {
        self.update_vibration_config(|v| {
            if slot < 60 {
                let mut a = params_from_config(v);
                a[slot] = value;
                config_from_params(&a, v);
            }
        });
    }

    /// 设置基础项 L/R 权重 (flat 26 = 13 项 × [L,R])。
    pub fn set_vib_item_lr(&self, index: usize, value: u32) {
        self.update_vibration_config(|v| {
            if index < 26 {
                v.item_lr[index] = value;
            }
        });
    }

    /// 设置评分事件 L/R 权重 (flat 30 = 15 项 × [L,R])。
    pub fn set_vib_rank_lr(&self, index: usize, value: u32) {
        self.update_vibration_config(|v| {
            if index < 30 {
                v.rank_lr[index] = value;
            }
        });
    }

    /// 设置评分细分强度 (15 项)。
    pub fn set_vib_rank_type_gain(&self, index: usize, value: u32) {
        self.update_vibration_config(|v| {
            if index < 15 {
                v.rank_type_gain[index] = value;
            }
        });
    }

    // ── 预设应用 (旧 apply_general_vibration_preset_in 的后端版) ────────

    /// 按名应用震动预设 (内置/自定义；用户主动覆盖过的同名自定义预设优先 ——
    /// `pick_preset_for_apply` 既有语义)。
    ///
    /// 行为：60 槽参数 + item_lr + rank_lr + rank_type_gain + 等级强度/时长 +
    /// 输出平滑 写入 config 并镜像；**职业专属通道复位** (节流清零、算法 id/ap
    /// 归零，与旧「切回通用预设后不残留上一职业」语义一致)；ACT1 特供套附加默认。
    /// 返回 false = 找不到该预设。
    pub fn apply_vibration_preset_by_name(&self, name: &str) -> anyhow::Result<bool> {
        let applied = self.cfg.update_config(&self.app, Commit::SaveVib, |c| {
            let user_entry = c
                .vibration_presets
                .iter()
                .find(|x| x.name == name)
                .cloned();
            let Some(pr) = crate::config::pick_preset_for_apply(user_entry.as_ref(), name) else {
                return false;
            };
            let v = &mut c.vibration;
            config_from_params(&pr.params, v);
            v.item_lr = pr.item_lr;
            v.rank_lr = pr.rank_lr;
            v.rank_type_gain = pr.rank_type_gain;
            v.rank_level_gain = pr.rank_level_gain;
            v.rank_duration = pr.rank_duration;
            v.out_smooth = pr.out_smooth;
            /* 职业专属通道复位 (config 侧；与旧 reset 集合一致) */
            v.throttle_window_ms = 0;
            v.throttle_max_hits = 0;
            v.throttle_dense_ratio = 100;
            /* ★v16.8 ACT1 特供附加默认 + ★v24.16 群怪治理标量 */
            if name == "ACT1 特供" {
                crate::config::act1_preset_extras(v);
            }
            true
        })?;
        if !applied {
            return Ok(false);
        }
        /* 原子镜像 (覆盖 rank_type_gain/等级/时长/平滑/节流/算法开关等全部字段) */
        self.cfg
            .with_config(|c| self.app.apply_vibration_config(&c.vibration));
        /* 职业叠加层复位 (apply_vibration_config 有意不动它们) */
        self.app.vibration_algo_id.store(0, Relaxed);
        for ap in &self.app.vibration_algo_ap {
            ap.store(0, Relaxed);
        }
        Ok(true)
    }

    /// 还原默认 = 应用内置默认预设第一档 (旧「还原默认」语义)。
    pub fn reset_vibration_defaults(&self) -> anyhow::Result<()> {
        let name = crate::config::default_vibration_presets()
            .first()
            .map(|p| p.name.clone())
            .unwrap_or_else(|| "默认".to_string());
        self.apply_vibration_preset_by_name(&name)?;
        Ok(())
    }

    // ── S1/S4 路线切换 (旧 switch_vibration_edition 的后端版；job 会话归 GUI) ──

    /// 切换震动客户端路线。返回目标路线是否有历史文件
    /// (`false` = 首次使用已自动套该路线内置默认预设：S1→「ACT1 特供」，S4→「默认」)。
    ///
    /// GUI 调用后须自行重算职业预设会话状态 (旧 switch 的最后一段)。
    pub fn switch_vib_edition(&self, to_legacy: bool) -> anyhow::Result<bool> {
        let from = self.cfg.with_config(|c| c.vib_legacy_client);
        if from == to_legacy {
            return Ok(true);
        }
        let base = AppConfig::vibration_path_for(self.cfg.path());
        let had_saved = self.cfg.update_config(&self.app, Commit::Memory, |c| {
            c.switch_vibration_edition_to(&base, from, to_legacy)
        })?;
        if had_saved {
            /* 目标路线有历史参数 → 全量同步到运行态 */
            self.cfg
                .with_config(|c| self.app.apply_vibration_config(&c.vibration));
        } else {
            /* 目标路线首次使用 → 套该路线内置默认预设 (会写 config + 镜像 + 落盘) */
            let want = if to_legacy { "ACT1 特供" } else { "默认" };
            self.apply_vibration_preset_by_name(&want)?;
        }
        /* 路线标记落盘 (Config.toml 根字段；旧流程等下次保存才持久化，这里即时化) */
        self.cfg
            .update_config(&self.app, Commit::Save, |c| c.vib_legacy_client = to_legacy)?;
        self.app.vib_legacy_client.store(to_legacy, Relaxed);
        Ok(had_saved)
    }

    // ── 职业叠加层 (JobVibration 路径专用；平时勿碰) ────────────────────

    /// 写职业算法 id/ap 原子 (仅 JobVibration 应用流程使用)。
    pub fn set_vib_algo(&self, id: u32, ap: [u32; 4]) {
        self.app.vibration_algo_id.store(id, Relaxed);
        for (i, v) in ap.iter().enumerate() {
            self.app.vibration_algo_ap[i].store(*v, Relaxed);
        }
    }
}
