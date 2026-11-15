//! ★R2.14 后端服务层 (backend) —— UI 与既有后端模块之间的**唯一**服务门面。
//!
//! 职责：
//! * 持有配置权威 [`ConfigStore`] (AppConfig 唯一活在它的 RwLock 里)；
//! * 聚合运行态快照 [`UiSnapshot`] (只读既有 pub 原子量，热路径零接触)；
//! * 攒发异步事件 [`UiEvent`] (托盘请求/预设切换/捕获结果)；
//! * 执行 [`UiCommand`] 命令 (与将来 HTTP 适配器共用同一入口)；
//! * 检查更新管理器的所有权归位 (旧在 GUI)。
//!
//! 红线 (调研结论，见交接 §13)：
//! * 不依赖 `crate::gui` (架构守卫 tests/architecture_tests.rs)；
//! * 引擎热路径 (原子量/scc 容器) 只读；写入只走既有入口
//!   (`apply_vibration_config` / `reload_config` / `AudioHapState::apply_config`)；
//! * `config.rs` 字段名一个不改 (TOML 契约冻结)。
//!
//! 领域方法分文件：`capture.rs` (捕获会话)、`vibration.rs` (震动/职业预设)、
//! `audio.rs` (音频震动)、`mappings.rs` (映射/连发/预设/宏/白名单)、
//! `devices.rs` (设备/更新/系统)。

mod audio;
mod capture;
mod config_store;
mod devices;
mod mappings;
mod vibration;

use std::collections::VecDeque;
use std::sync::{Arc, RwLock};
use std::time::Instant;

use crate::state::AppState;
use crate::ui_api::{AudioSnap, Commit, CountsSnap, CoreSnap, InputSnap, UiCommand, UiEvent, UiReply, UiSnapshot, UpdateSnap, VibSnap};
use crate::update::UpdateManager;
use crate::util;

use config_store::ConfigStore;

/// UI 持有的后端句柄 (廉价 Clone)。
pub type Backend = Arc<BackendService>;

/// 后端服务。UI 侧只允许通过它访问后端。
pub struct BackendService {
    pub(crate) cfg: Arc<ConfigStore>,
    pub(crate) app: Arc<AppState>,
    pub(crate) update: UpdateManager,
    pub(crate) events: RwLock<VecDeque<UiEvent>>,
    started_at: Instant,
}

impl BackendService {
    /// 装配后端 (配置权威归位 + 保存调度线程启动)。
    ///
    /// `config`：已加载的配置 (main.rs 的 `load_or_create` 结果)；
    /// `config_path`：通常 `"Config.toml"` (相对工作目录，与旧 GUI 完全一致)。
    pub fn new(app_state: Arc<AppState>, config: crate::config::AppConfig, config_path: &str) -> Self {
        let cfg = Arc::new(ConfigStore::new(config, config_path));
        config_store::spawn_scheduler(cfg.clone());
        Self {
            cfg,
            app: app_state,
            update: UpdateManager::new(),
            events: RwLock::new(VecDeque::new()),
            started_at: Instant::now(),
        }
    }

    // ── 配置读写 (UI 唯二入口) ──────────────────────────────────────────

    /// 只读配置。**闭包内禁止调用 Backend 的写方法** (RwLock 不可重入)。
    /// 渲染页推荐模式：帧首用一个 `with_config` 块把需要的值收集进局部变量，
    /// 之后随意 dispatch 命令，避免持读锁跨越写操作。
    pub fn with_config<R>(&self, f: impl FnOnce(&crate::config::AppConfig) -> R) -> R {
        self.cfg.with_config(f)
    }

    /// 写配置 + 按 [`Commit`] 档位落盘/热重载 (UI 写配置唯一入口)。
    pub fn update_config<R>(
        &self,
        level: Commit,
        f: impl FnOnce(&mut crate::config::AppConfig) -> R,
    ) -> anyhow::Result<R> {
        self.cfg.update_config(&self.app, level, f)
    }

    /// 高频滑杆路径：改内存 + 配置文件去抖脏标 (静默 1 秒落盘)。
    pub fn update_config_debounced<R>(&self, f: impl FnOnce(&mut crate::config::AppConfig) -> R) -> R {
        self.cfg.update_config_debounced(f)
    }

    /// 高频滑杆路径：改内存 + 震动路线文件去抖脏标。
    pub fn update_vib_debounced<R>(&self, f: impl FnOnce(&mut crate::config::AppConfig) -> R) -> R {
        self.cfg.update_vib_debounced(f)
    }

    /// 震动滑杆路径专用：改配置内 [vibration] 并把整套参数镜像进引擎原子
    /// (单一写路径：config → apply_vibration_config → 去抖落盘)。
    /// 注意不动 `vibration_algo_id/ap` (JobVibration 职业叠加层)。
    pub fn update_vibration_config<R>(
        &self,
        f: impl FnOnce(&mut crate::config::VibrationConfig) -> R,
    ) -> R {
        let ret = self.cfg.update_vib_debounced(|c| f(&mut c.vibration));
        self.cfg.with_config(|c| self.app.apply_vibration_config(&c.vibration));
        ret
    }

    /// 立即全部落盘 (Ctrl+S)。
    pub fn save_all_now(&self) -> anyhow::Result<()> {
        self.cfg.save_all_now()
    }

    /// 退出兜底落盘 (只补写脏通道，旧 GUI 语义)。
    pub fn flush_all(&self) {
        self.cfg.flush_all()
    }

    /// 退出：落盘 + 通知运行线程收尾 (GUI 收到后关窗口)。
    pub fn quit(&self) {
        self.flush_all();
        self.app.exit();
    }

    // ── 事件 ────────────────────────────────────────────────────────────

    /// 后端攒事件 (内部/线程用)。
    pub(crate) fn push_event(&self, ev: UiEvent) {
        util::write_guard(&self.events).push_back(ev);
    }

    /// UI 每帧取走全部事件。
    pub fn poll_events(&self) -> Vec<UiEvent> {
        let mut q = util::write_guard(&self.events);
        q.drain(..).collect()
    }

    // ── 快照 ────────────────────────────────────────────────────────────

    /// 聚合运行态快照 (每帧一次；全原子量 load，微秒级)。
    pub fn snapshot(&self) -> UiSnapshot {
        use std::sync::atomic::Ordering;
        let app = &self.app;
        let o = Ordering::Relaxed;

        let core = CoreSnap {
            paused: app.is_paused(),
            language: app.language(),
            config_epoch: app.config_epoch(),
            workers_configured: app.get_configured_worker_count(),
            workers_actual: app.get_actual_worker_count(),
            stopped_threads: util::stopped_threads().into_iter().map(String::from).collect(),
            injection_guard_hits: {
                let (h, _, _) = crate::state::injection_guard_stats();
                h
            },
            injection_guard_passed: {
                let (_, p, _) = crate::state::injection_guard_stats();
                p
            },
            injection_guard_whitelisted: {
                let (_, _, w) = crate::state::injection_guard_stats();
                w
            },
            gp_calibrating: app.is_gp_calibrating(),
            sequence_paused: app.sequence_paused.load(o),
            key_record_active: app.key_record_active.load(o),
            vib_legacy_client: app.vib_legacy_client.load(o),
        };

        let input = InputSnap {
            live_hid_pad: app.live_hid_pad(),
            live_hid: app.live_hid_state(),
            live_xinput: app.live_xinput_state(),
            raw_capture_active: app.is_raw_input_capture_active(),
            raw_capture_mode: app.get_rawinput_capture_mode(),
            xinput_capture_mode: app.get_xinput_capture_mode(),
            has_semantic_hid: app.has_semantic_hid_mappings(),
        };

        let arr = |n: usize, f: &dyn Fn(usize) -> u32| -> Vec<u32> { (0..n).map(f).collect() };
        let vibration = VibSnap {
            enabled: app.vibration_enabled.load(o),
            connected: app.vibration_connected.load(o),
            font_hits: app.vibration_font_hits.load(o),
            advanced_enabled: app.vibration_advanced_enabled.load(o),
            independent_test: app.vibration_independent_test.load(o),
            legacy_output_mode: app.vibration_legacy_output_mode.load(o),
            events_received: app.vibration_events_received.load(o),
            rank_events: app.vibration_rank_events.load(o),
            rank_last: app.vibration_rank_last.load(o),
            params: arr(60, &|i| app.vibration_params[i].load(o)),
            item_lr: arr(26, &|i| app.vibration_item_lr[i].load(o)),
            rank_lr: arr(30, &|i| app.vibration_rank_lr[i].load(o)),
            rank_type_gain: arr(15, &|i| app.vibration_rank_type_gain[i].load(o)),
            rank_type_events: (0..15).map(|i| app.vibration_rank_type_events[i].load(o)).collect(),
            rank_level_gain: app.vibration_rank_level_gain.load(o),
            rank_duration: app.vibration_rank_duration.load(o),
            out_l: app.vibration_out_l.load(o),
            out_r: app.vibration_out_r.load(o),
            rank_out: arr(30, &|i| app.vibration_rank_out[i].load(o)),
            item_out: arr(26, &|i| app.vibration_item_out[i].load(o)),
            test_until_ms: app.vibration_test_until.load(o),
            now_ms: crate::vibration::now_ms_u64(),
            algo_id: app.vibration_algo_id.load(o),
            algo_ap: [
                app.vibration_algo_ap[0].load(o),
                app.vibration_algo_ap[1].load(o),
                app.vibration_algo_ap[2].load(o),
                app.vibration_algo_ap[3].load(o),
            ],
        };

        let audio = AudioSnap {
            enabled: app.audio_hap.is_enabled(),
            status: app.audio_hap.status_name().to_string(),
            motor_l: app.audio_hap.output().0,
            motor_r: app.audio_hap.output().1,
            bands: app.audio_hap.telemetry_bands(),
            band_peak: app.audio_hap.telemetry_band_peak(),
            level_l: app.audio_hap.telemetry_level().0,
            level_r: app.audio_hap.telemetry_level().1,
            mode: app.audio_hap.telemetry_mode(),
            noise_floor: app.audio_hap.telemetry_noise_floor(),
            transient: app.audio_hap.telemetry_transient(),
            threshold_eff: app.audio_hap.telemetry_threshold_eff(),
            wl_entries: app.audio_hap.telemetry_wl_entries(),
            wl_source: app.audio_hap.telemetry_wl_source().to_string(),
            wl_foreground: app.audio_hap.telemetry_wl_foreground().to_string(),
            wl_matched: app.audio_hap.telemetry_wl_matched(),
            wl_degraded: app.audio_hap.telemetry_wl_degraded(),
            wl_error: app.audio_hap.telemetry_wl_error().to_string(),
            self_check_active: app.audio_hap.self_check_active(),
            calib_result: app.audio_hap.calib_result.load(o),
        };

        let update = self.update_snapshot();
        let counts = self.counts_snapshot();

        UiSnapshot {
            version: crate::update::APP_VERSION.to_string(),
            core,
            input,
            vibration,
            audio,
            update,
            counts,
        }
    }

    /// 更新快照 (进程内可直接拿原始 [`crate::update::Snapshot`])。
    pub fn update_snapshot(&self) -> UpdateSnap {
        let s = self.update.snapshot();
        UpdateSnap {
            phase: format!("{:?}", s.phase),
            latest_version: s.latest.as_ref().map(|r| r.version.clone()),
            latest_notes: s.latest.as_ref().map(|r| r.notes.clone()),
            error: s.error.clone(),
            done: s.done,
            total: s.total,
            active_channel: s.active.map(|c| c.key().to_string()),
            saved_path: s.saved.as_ref().map(|p| p.display().to_string()),
            source: s.source.clone(),
            last_check_unix: s.last_check_unix,
            next_check_unix: s.next_check_unix,
        }
    }

    /// 原始更新快照 (update_ui 渲染用)。
    pub fn update_snapshot_raw(&self) -> crate::update::Snapshot {
        self.update.snapshot()
    }

    /// 计数聚合 (配置派生)。
    pub fn counts_snapshot(&self) -> CountsSnap {
        self.cfg.with_config(|c| CountsSnap {
            mappings: c.mappings.len(),
            turbo: c.mappings.iter().filter(|m| m.turbo_enabled).count(),
            combos: c
                .mappings
                .iter()
                .filter(|m| {
                    matches!(
                        crate::state::AppState::input_name_to_device(&m.trigger_key),
                        Some(
                            crate::state::InputDevice::KeyCombo(_)
                                | crate::state::InputDevice::XInputCombo { .. }
                        )
                    )
                })
                .count(),
            presets: c.presets.len(),
            whitelist: c.process_whitelist.len(),
            macros: c.universal_macros.len(),
        })
    }

    // ── 命令分派 ────────────────────────────────────────────────────────

    /// 执行 UI 命令。将来 HTTP `/api/action` 也走这里 (UiCommand 已 serde)。
    pub fn dispatch(&self, cmd: UiCommand) -> UiReply {
        use std::sync::atomic::Ordering::Relaxed;
        match cmd {
            UiCommand::TogglePause => {
                let paused = self.app.toggle_paused();
                UiReply::ok_with(format!("paused={paused}"))
            }
            UiCommand::SetPaused(p) => {
                self.app.set_paused(p);
                UiReply::ok()
            }
            UiCommand::SaveAll => match self.save_all_now() {
                Ok(()) => UiReply::ok(),
                Err(e) => UiReply::err(e.to_string()),
            },
            UiCommand::Quit => {
                self.quit();
                UiReply::ok()
            }
            UiCommand::SetLanguage(l) => match self.update_config(Commit::Reload, |c| {
                c.language = l;
            }) {
                Ok(()) => UiReply::ok(),
                Err(e) => UiReply::err(e.to_string()),
            },
            UiCommand::SetDarkMode(b) => self.commit_ok(Commit::Save, |c| c.dark_mode = b),
            UiCommand::SetAlwaysOnTop(b) => self.commit_ok(Commit::Save, |c| c.always_on_top = b),
            UiCommand::SetTrayEnabled(b) => self.commit_ok(Commit::Save, |c| c.show_tray_icon = b),
            UiCommand::SetNotificationsEnabled(b) => {
                self.commit_ok(Commit::Save, |c| c.show_notifications = b)
            }
            UiCommand::SetMode { minimal, classic } => self.commit_ok(Commit::Save, |c| {
                if let Some(m) = minimal {
                    c.minimal_mode = m;
                }
                if let Some(m) = classic {
                    c.classic_mode = m;
                }
            }),
            UiCommand::ShowGuide => self.commit_ok(Commit::Save, |c| c.guide_seen = false),
            UiCommand::SetVibrationEnabled(b) => {
                let r = self.update_vibration_config(|v| v.enabled = b);
                let _ = r;
                match self.cfg.save_vib_now() {
                    Ok(()) => UiReply::ok(),
                    Err(e) => UiReply::err(e.to_string()),
                }
            }
            UiCommand::TestPulse => {
                self.app
                    .vibration_test_until
                    .store(crate::vibration::now_ms_u64() + 300, Relaxed);
                UiReply::ok()
            }
            UiCommand::TestVibration => {
                self.app
                    .vibration_test_until
                    .store(crate::vibration::now_ms_u64() + 1000, Relaxed);
                UiReply::ok()
            }
            UiCommand::TestRank => {
                self.app.vibration_rank_test.store(true, Relaxed);
                UiReply::ok()
            }
            UiCommand::SwitchVibEdition { legacy } => match self.switch_vib_edition(legacy) {
                Ok(had_history) => UiReply::ok_with(format!("had_history={had_history}")),
                Err(e) => UiReply::err(e.to_string()),
            },
            UiCommand::AudioSelfCheck => {
                self.app.audio_hap.request_self_check();
                UiReply::ok()
            }
            UiCommand::AudioCalibrate => {
                self.app.audio_hap.request_calibration();
                UiReply::ok()
            }
            UiCommand::AudioCalibDismiss => {
                self.app
                    .audio_hap
                    .calib_result
                    .store(0, std::sync::atomic::Ordering::Relaxed);
                UiReply::ok()
            }
            UiCommand::RefreshDevices => UiReply::ok(), // 设备对话框打开时自行枚举
            UiCommand::ResetDevices => {
                crate::input_manager::request_xinput_reset();
                UiReply::ok()
            }
            UiCommand::RestartThreads(name) => {
                let ok = util::restart_thread(&name);
                if ok {
                    UiReply::ok()
                } else {
                    UiReply::err(format!("线程不在看护名单或未停止: {name}"))
                }
            }
            UiCommand::CheckUpdate => {
                let manifest = self
                    .cfg
                    .with_config(|c| c.update.manifest_url.clone());
                let _ = self.update.check(manifest);
                UiReply::ok()
            }
            UiCommand::CancelDownload => {
                self.update.cancel_download();
                UiReply::ok()
            }
            UiCommand::DismissUpdateError => {
                self.update.dismiss_error();
                UiReply::ok()
            }
            UiCommand::ApplyMappingPreset { name } => match self.apply_mapping_preset(&name) {
                Ok(()) => UiReply::ok(),
                Err(e) => UiReply::err(e.to_string()),
            },
            UiCommand::SaveMappingPreset { name } => match self.save_mapping_preset(&name) {
                Ok(()) => UiReply::ok(),
                Err(e) => UiReply::err(e.to_string()),
            },
            UiCommand::DeleteMappingPreset { name } => match self.delete_mapping_preset(&name) {
                Ok(()) => UiReply::ok(),
                Err(e) => UiReply::err(e.to_string()),
            },
            UiCommand::UpsertMapping { index, mapping } => {
                match self.upsert_mapping(index, mapping) {
                    Ok(()) => UiReply::ok(),
                    Err(e) => UiReply::err(e.to_string()),
                }
            }
            UiCommand::DeleteMapping(index) => match self.delete_mapping(index) {
                Ok(()) => UiReply::ok(),
                Err(e) => UiReply::err(e.to_string()),
            },
        }
    }

    /// update_config + 统一错误转 Reply 的小助手。
    fn commit_ok(
        &self,
        level: Commit,
        f: impl FnOnce(&mut crate::config::AppConfig),
    ) -> UiReply {
        match self.update_config(level, f) {
            Ok(()) => UiReply::ok(),
            Err(e) => UiReply::err(e.to_string()),
        }
    }

    // ── 通知 (托盘气泡) ─────────────────────────────────────────────────

    pub fn notify_info(&self, msg: impl Into<String>) {
        self.app
            .send_notification(crate::state::NotificationEvent::Info(msg.into()));
    }
    pub fn notify_warning(&self, msg: impl Into<String>) {
        self.app
            .send_notification(crate::state::NotificationEvent::Warning(msg.into()));
    }
    pub fn notify_error(&self, msg: impl Into<String>) {
        self.app
            .send_notification(crate::state::NotificationEvent::Error(msg.into()));
    }

    /// 后端启动时刻 (诊断用)。
    pub fn uptime(&self) -> std::time::Duration {
        self.started_at.elapsed()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AppConfig;

    /// 临时目录里装配一套最小后端 (真实 AppState；不启动钩子/引擎线程)。
    fn temp_backend(tag: &str) -> (BackendService, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "sorahk_backend_test_{tag}_{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("Config.toml");
        let config = AppConfig::load_or_create(&path).unwrap();
        let state = AppState::new(config.clone()).unwrap();
        (BackendService::new(Arc::new(state), config, &path.to_string_lossy()), path)
    }

    #[test]
    fn update_config_save_and_dirty_flush() {
        let (b, path) = temp_backend("save");
        b.update_config(Commit::Save, |c| c.always_on_top = true).unwrap();
        b.cfg.with_config(|c| assert!(c.always_on_top));
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("always_on_top = true"), "文件未落盘");
        assert!(!b.cfg.any_dirty());
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn debounced_mark_and_flush() {
        let (b, path) = temp_backend("deb");
        b.update_config_debounced(|c| c.dark_mode = true);
        assert!(b.cfg.any_dirty(), "脏标未打上");
        b.flush_all();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("dark_mode = true"));
        assert!(!b.cfg.any_dirty());
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn reload_updates_epoch_and_bindings() {
        let (b, path) = temp_backend("reload");
        let before = b.snapshot().core.config_epoch;
        b.update_config(Commit::Reload, |c| c.language = crate::i18n::Language::English)
            .unwrap();
        let after = b.snapshot().core.config_epoch;
        assert!(after > before, "reload 未推进 config_epoch");
        assert_eq!(b.snapshot().core.language, crate::i18n::Language::English);
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn events_poll_and_commands() {
        let (b, path) = temp_backend("events");
        b.push_event(UiEvent::ShowWindow);
        b.push_event(UiEvent::PresetSwitch("测试".into()));
        let evs = b.poll_events();
        assert_eq!(evs.len(), 2);
        assert!(b.poll_events().is_empty(), "事件应取走即空");

        let r = b.dispatch(UiCommand::SetPaused(true));
        assert!(r.ok);
        assert!(b.snapshot().core.paused);
        let before = b.snapshot().counts.mappings;
        let r = b.dispatch(UiCommand::UpsertMapping {
            index: None,
            mapping: crate::config::KeyMapping {
                trigger_key: "A".into(),
                ..Default::default()
            },
        });
        assert!(r.ok, "UpsertMapping 失败: {:?}", r.message);
        assert_eq!(b.snapshot().counts.mappings, before + 1);
        let r = b.dispatch(UiCommand::DeleteMapping(before));
        assert!(r.ok, "DeleteMapping 失败: {:?}", r.message);
        assert_eq!(b.snapshot().counts.mappings, before);
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }
}
