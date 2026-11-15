//! ★R2.14 映射/连发/预设/宏/白名单领域 (BackendService impl)。
//!
//! 旧 GUI 的模式是「改 `config.mappings` → save_to_file → reload_config」，
//! 这里收敛为带校验语义的单一入口 (`Commit::Reload` 失败即映射非法，
//! 运行态保持旧映射，文件已按新值落盘 —— 与旧失败语义一致)。

use super::BackendService;
use crate::config::KeyMapping;
use crate::ui_api::Commit;

impl BackendService {
    // ── 映射 CRUD ───────────────────────────────────────────────────────

    /// 新增/替换一条映射并热重载。`index = Some(i)` 按 i 替换 (越界 = 追加)。
    pub fn upsert_mapping(&self, index: Option<usize>, mapping: KeyMapping) -> anyhow::Result<()> {
        self.update_config(Commit::Reload, |c| match index {
            Some(i) if i < c.mappings.len() => c.mappings[i] = mapping,
            _ => c.mappings.push(mapping),
        })?;
        Ok(())
    }

    /// 删除第 i 条映射并热重载 (越界静默忽略，与旧 GUI 删除防御一致)。
    pub fn delete_mapping(&self, index: usize) -> anyhow::Result<()> {
        self.update_config(Commit::Reload, |c| {
            if index < c.mappings.len() {
                c.mappings.remove(index);
            }
        })?;
        Ok(())
    }

    // ── 映射预设 ────────────────────────────────────────────────────────

    /// 应用同名预设：mappings 整体替换 + current_preset 归位 + 热重载。
    pub fn apply_mapping_preset(&self, name: &str) -> anyhow::Result<()> {
        self.update_config(Commit::Reload, |c| {
            if let Some(p) = c.presets.iter().find(|p| p.name == name) {
                c.mappings = p.mappings.clone();
                c.current_preset = name.to_string();
            }
        })?;
        Ok(())
    }

    /// 把当前 mappings 存为预设 (同名覆盖 mappings，保留原切换键)。
    pub fn save_mapping_preset(&self, name: &str) -> anyhow::Result<()> {
        self.update_config(Commit::Save, |c| {
            match c.presets.iter_mut().find(|p| p.name == name) {
                Some(p) => p.mappings = c.mappings.clone(),
                None => c.presets.push(crate::config::Preset {
                    name: name.to_string(),
                    mappings: c.mappings.clone(),
                    switch_key: String::new(),
                }),
            }
            c.current_preset = name.to_string();
        })?;
        Ok(())
    }

    /// 删除预设 (删的是当前预设时清空 current_preset)。
    pub fn delete_mapping_preset(&self, name: &str) -> anyhow::Result<()> {
        self.update_config(Commit::Save, |c| {
            c.presets.retain(|p| p.name != name);
            if c.current_preset == name {
                c.current_preset.clear();
            }
        })?;
        Ok(())
    }

    /// 重命名预设 (同步 current_preset 引用)。
    pub fn rename_mapping_preset(&self, old: &str, new: &str) -> anyhow::Result<()> {
        self.update_config(Commit::Save, |c| {
            if let Some(p) = c.presets.iter_mut().find(|p| p.name == old) {
                p.name = new.to_string();
            }
            if c.current_preset == old {
                c.current_preset = new.to_string();
            }
        })?;
        Ok(())
    }

    /// 设置某预设的切换键 (空串 = 解绑；冲突校验由 UI 层先做，与旧流程一致)。
    pub fn set_preset_switch_key(&self, name: &str, key: &str) -> anyhow::Result<()> {
        self.update_config(Commit::Save, |c| {
            if let Some(p) = c.presets.iter_mut().find(|p| p.name == name) {
                p.switch_key = key.to_string();
            }
        })?;
        // 切换键改动要重建手柄绑定表
        self.cfg.reload_and_apply(&self.app)?;
        Ok(())
    }

    // ── 通用宏 ──────────────────────────────────────────────────────────

    /// 新增/覆盖通用宏 (同名覆盖)。
    pub fn upsert_macro(&self, name: &str, text: &str) -> anyhow::Result<()> {
        self.update_config(Commit::Save, |c| {
            match c.universal_macros.iter_mut().find(|m| m.name == name) {
                Some(m) => m.text = text.to_string(),
                None => c.universal_macros.push(crate::config::UniversalMacro {
                    name: name.to_string(),
                    text: text.to_string(),
                }),
            }
        })?;
        Ok(())
    }

    /// 删除通用宏。
    pub fn delete_macro(&self, name: &str) -> anyhow::Result<()> {
        self.update_config(Commit::Save, |c| {
            c.universal_macros.retain(|m| m.name != name);
        })?;
        Ok(())
    }

    // ── 连发白名单 (进程过滤；与音频白名单独立) ─────────────────────────

    /// 追加进程 (存在则忽略；保存 + 热重载即时生效，旧白名单页语义)。
    pub fn whitelist_add(&self, process: &str) -> anyhow::Result<()> {
        self.update_config(Commit::Reload, |c| {
            if !c.process_whitelist.iter().any(|p| p == process) {
                c.process_whitelist.push(process.to_string());
            }
        })?;
        Ok(())
    }

    /// 移除进程。
    pub fn whitelist_remove(&self, process: &str) -> anyhow::Result<()> {
        self.update_config(Commit::Reload, |c| {
            c.process_whitelist.retain(|p| p != process);
        })?;
        Ok(())
    }

    /// 清空白名单。
    pub fn whitelist_clear(&self) -> anyhow::Result<()> {
        self.update_config(Commit::Reload, |c| {
            c.process_whitelist.clear();
        })?;
        Ok(())
    }
}
