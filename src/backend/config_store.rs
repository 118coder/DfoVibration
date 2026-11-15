//! ★R2.14 配置权威 store —— `AppConfig` 的唯一持有者 (前后端分离的核心改造点)。
//!
//! 旧架构：`SorahkGui.config` 是唯一权威，137 处 `save_to_file` 散在 GUI 里，
//! 震动/音频滑杆各自维护去抖脏标。新架构：
//! * 读：[`ConfigStore::with_config`] (短借只读闭包，**闭包内禁止调用会再进锁的方法**)；
//! * 写：[`ConfigStore::update_config`] 按 [`Commit`] 档位落盘/热重载，保存集中在这里；
//! * 高频滑杆路径：`Commit::Memory` 改内存 + [`ConfigStore::mark_cfg_dirty`] /
//!   [`ConfigStore::mark_vib_dirty`] 打脏标，由看护线程静默 1 秒后统一原子落盘
//!   (与旧 GUI 去抖语义一致，`main_window.rs` 的两段去抖逻辑由此接管)；
//! * 退出：[`ConfigStore::flush_all`] 只补写脏的 (与旧退出落盘语义一致)。

use std::path::{Path, PathBuf};
use std::sync::RwLock;
use std::time::{Duration, Instant};

use anyhow::Result;

use crate::config::AppConfig;
use crate::state::AppState;
use crate::ui_api::Commit;
use crate::util;

/// 单通道去抖状态 (震动文件 / 配置文件各一)。
#[derive(Debug)]
struct Debounce {
    dirty: bool,
    since: Instant,
}

impl Debounce {
    fn new() -> Self {
        Self {
            dirty: false,
            since: Instant::now(),
        }
    }
    fn mark(&mut self) {
        if !self.dirty {
            self.dirty = true;
            self.since = Instant::now();
        }
    }
    fn due(&self, quiet: Duration) -> bool {
        self.dirty && self.since.elapsed() >= quiet
    }
}

/// 配置权威。`AppConfig` 只活在里面的 `RwLock` 里。
pub(crate) struct ConfigStore {
    lock: RwLock<AppConfig>,
    /// Config.toml 路径 (通常是相对路径 "Config.toml"，随工作目录)
    path: PathBuf,
    vib_deb: RwLock<Debounce>,
    cfg_deb: RwLock<Debounce>,
    /// 保存互斥：调度线程与前台提交并发写文件时串行化
    save_lock: RwLock<()>,
}

/// 调度线程的轮询间隔与去抖静默窗 (与旧 GUI 语义一致：静默 1 秒落盘)。
const TICK: Duration = Duration::from_millis(250);
const QUIET: Duration = Duration::from_secs(1);

impl ConfigStore {
    pub(crate) fn new(config: AppConfig, path: impl AsRef<Path>) -> Self {
        Self {
            lock: RwLock::new(config),
            path: path.as_ref().to_path_buf(),
            vib_deb: RwLock::new(Debounce::new()),
            cfg_deb: RwLock::new(Debounce::new()),
            save_lock: RwLock::new(()),
        }
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    /// 只读短借。**闭包内不得调用本 store 的写方法** (RwLock 不可重入)。
    pub(crate) fn with_config<R>(&self, f: impl FnOnce(&AppConfig) -> R) -> R {
        let c = util::read_guard(&self.lock);
        f(&c)
    }

    /// 写入 + 按 [`Commit`] 档位落盘/热重载。`app` 用于 Reload 档的运行态重挂。
    ///
    /// 失败语义与旧 GUI 一致：文件已按新值写出；`reload_config` 失败 (如映射非法)
    /// 时运行态保持旧映射并返回 Err，由 UI 层提示。
    pub(crate) fn update_config<R>(
        &self,
        app: &AppState,
        level: Commit,
        f: impl FnOnce(&mut AppConfig) -> R,
    ) -> Result<R> {
        let ret = {
            let mut c = util::write_guard(&self.lock);
            f(&mut c)
        };
        match level {
            Commit::Memory => Ok(ret),
            Commit::Save => {
                self.save_all_now()?;
                Ok(ret)
            }
            Commit::SaveVib => {
                self.save_vib_now()?;
                Ok(ret)
            }
            Commit::Reload => {
                self.save_all_now()?;
                self.reload_and_apply(app)?;
                Ok(ret)
            }
        }
    }

    /// 高频路径：只改内存并对配置文件打脏标 (调度线程静默 1 秒后落盘)。
    pub(crate) fn update_config_debounced<R>(&self, f: impl FnOnce(&mut AppConfig) -> R) -> R {
        let ret = {
            let mut c = util::write_guard(&self.lock);
            f(&mut c)
        };
        util::write_guard(&self.cfg_deb).mark();
        ret
    }

    /// 高频路径：只改内存并对路线震动文件打脏标 (震动滑杆去抖，旧语义)。
    pub(crate) fn update_vib_debounced<R>(&self, f: impl FnOnce(&mut AppConfig) -> R) -> R {
        let ret = {
            let mut c = util::write_guard(&self.lock);
            f(&mut c)
        };
        util::write_guard(&self.vib_deb).mark();
        ret
    }

    /// 立即写 Config.toml (+ 路线震动文件，`save_to_file` 既有语义)。
    pub(crate) fn save_all_now(&self) -> Result<()> {
        let _g = util::read_guard(&self.save_lock);
        let c = util::read_guard(&self.lock);
        c.save_to_file(&self.path)?;
        util::write_guard(&self.cfg_deb).dirty = false;
        util::write_guard(&self.vib_deb).dirty = false;
        Ok(())
    }

    /// 立即只写路线震动文件 (Vibration.toml / Vibration-ACT.toml)。
    pub(crate) fn save_vib_now(&self) -> Result<()> {
        let _g = util::read_guard(&self.save_lock);
        let c = util::read_guard(&self.lock);
        c.save_vibration_to_file(AppConfig::vibration_path_for(&self.path))?;
        util::write_guard(&self.vib_deb).dirty = false;
        Ok(())
    }

    /// 两阶段热重载 + 运行态重挂 (预设切换键)。
    ///
    /// 语义与旧 GUI 一致：`reload_config` 内部已同步语言/托盘/白名单/连发缓存/音频镜像；
    /// **不**镜像震动参数 (保护 JobVibration 职业叠加层，避免 reload 冲掉职业调校)。
    pub(crate) fn reload_and_apply(&self, app: &AppState) -> Result<()> {
        let snap = util::read_guard(&self.lock).clone();
        app.reload_config(snap.clone())?;
        app.set_preset_switch_bindings(Self::build_preset_bindings(&snap));
        Ok(())
    }

    /// 从配置构建手柄类预设切换键绑定 (旧 main_window::sync_preset_switch_bindings 的后端版)。
    pub(crate) fn build_preset_bindings(
        config: &AppConfig,
    ) -> Vec<crate::state::PresetSwitchBinding> {
        let mut bindings = Vec::new();
        for p in &config.presets {
            let key = &p.switch_key;
            if key.is_empty()
                || crate::state::key_names::key_kind(key) != crate::state::key_names::KeyKind::Gamepad
            {
                continue;
            }
            if let Some(device) = crate::state::AppState::input_name_to_device(key) {
                bindings.push(crate::state::PresetSwitchBinding {
                    preset_name: p.name.clone(),
                    device,
                });
            }
        }
        bindings
    }

    /// 调度线程一次 tick：静默期满的脏通道落盘。
    pub(crate) fn tick(&self) {
        if util::read_guard(&self.vib_deb).due(QUIET) {
            if let Err(e) = self.save_vib_now() {
                crate::vibration::vib_error(&format!("[backend] 震动文件去抖落盘失败: {e}"));
            }
        }
        if util::read_guard(&self.cfg_deb).due(QUIET) {
            if let Err(e) = self.save_all_now() {
                crate::vibration::vib_error(&format!("[backend] 配置去抖落盘失败: {e}"));
            }
        }
    }

    /// 退出兜底：只补写当前脏的通道 (与旧 GUI 退出落盘语义一致)。
    pub(crate) fn flush_all(&self) {
        let vib = util::read_guard(&self.vib_deb).dirty;
        let cfg = util::read_guard(&self.cfg_deb).dirty;
        if vib {
            if let Err(e) = self.save_vib_now() {
                crate::vibration::vib_error(&format!("[backend] 退出落盘(震动)失败: {e}"));
            }
        }
        if cfg {
            if let Err(e) = self.save_all_now() {
                crate::vibration::vib_error(&format!("[backend] 退出落盘(配置)失败: {e}"));
            }
        }
    }

    /// 是否有未落盘的脏数据 (测试用)。
    #[cfg(test)]
    pub(crate) fn any_dirty(&self) -> bool {
        util::read_guard(&self.vib_deb).dirty || util::read_guard(&self.cfg_deb).dirty
    }
}

/// 后台保存调度线程入口 (supervised；`store` 由 BackendService 与本线程共享)。
pub(crate) fn spawn_scheduler(store: std::sync::Arc<ConfigStore>) {
    util::spawn_supervised("backend-save-scheduler", move || loop {
        std::thread::sleep(TICK);
        store.tick();
    });
}
