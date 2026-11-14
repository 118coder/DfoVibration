//! `UiBackend` 在 egui 渲染器上的实现。
//! 与 `webui::WebUi::action()` 是同一套命令的两种落地：网页端持有 RwLock<AppConfig>，
//! egui 端独占 AppConfig。契约在 `ui_api`，两边只负责把命令翻译成自己的状态操作。

use std::sync::atomic::Ordering;
use std::time::Instant;

use crate::gui::SorahkGui;
use crate::ui_api::{UiBackend, UiCommand, UiMode, UiReply, UiSnapshot};

/// 切换模式 / 触发动作时给一声轻提示，和极简模式的震动反馈配套。
#[cfg(windows)]
fn blip() {
    use windows::Win32::System::Diagnostics::Debug::MessageBeep;
    use windows::Win32::UI::WindowsAndMessaging::MB_OK;
    unsafe {
        let _ = MessageBeep(MB_OK);
    }
}

#[cfg(not(windows))]
fn blip() {}

fn pulse(state: &std::sync::Arc<crate::state::AppState>, ms: u64) {
    let until = crate::vibration::now_ms_u64() + ms;
    state.vibration_test_until.store(until, Ordering::Relaxed);
}

impl UiBackend for SorahkGui {
    fn snapshot(&self) -> UiSnapshot {
        let cfg = &self.config;
        UiSnapshot {
            version: env!("CARGO_PKG_VERSION").to_string(),
            paused: self.app_state.is_paused(),
            worker_count: self.app_state.get_actual_worker_count() as u32,
            mappings: cfg.mappings.len(),
            turbo: cfg.mappings.iter().filter(|m| m.turbo_enabled).count(),
            presets: cfg.presets.len(),
            whitelist: cfg.process_whitelist.len(),
            vibration_enabled: cfg.vibration.enabled,
            audio_enabled: cfg.audio_haptics.enabled,
        }
    }

    fn dispatch(&mut self, cmd: UiCommand) -> UiReply {
        match cmd {
            UiCommand::TogglePause => {
                let paused = self.app_state.toggle_paused();
                UiReply::ok(if paused { "已暂停全部映射" } else { "已恢复" })
            }
            UiCommand::Quit => {
                self.app_state.exit();
                UiReply::ok("正在退出")
            }
            UiCommand::Save => match self.config.save_to_file("Config.toml") {
                Ok(()) => UiReply::ok("已写入 Config.toml"),
                Err(e) => UiReply::fail(format!("写入失败：{e}")),
            },
            UiCommand::CheckUpdate => {
                let started = self.update.check(String::new());
                UiReply::ok(if started { "正在检查更新…" } else { "已经在检查了" })
            }
            UiCommand::TestPulse => {
                pulse(&self.app_state, 140);
                blip();
                UiReply::ok("轻触反馈")
            }
            UiCommand::TestVibration => {
                pulse(&self.app_state, 1000);
                UiReply::ok("试震 1 秒")
            }
            UiCommand::TestRank => {
                self.app_state.vibration_rank_test.store(true, Ordering::Relaxed);
                UiReply::ok("档位自检已触发")
            }
            UiCommand::AudioSelfCheck => {
                self.app_state.audio_hap.request_self_check();
                UiReply::ok("音频自检已触发")
            }
            UiCommand::AudioCalibrate => {
                self.app_state.audio_hap.request_calibration();
                UiReply::ok("保持安静 3 秒")
            }
            UiCommand::AudioCalibDismiss => {
                self.app_state.audio_hap.calib_result.store(0, Ordering::Relaxed);
                UiReply::ok("已关闭校准结果")
            }
            UiCommand::ResetDevices => {
                crate::input_manager::request_xinput_reset();
                UiReply::ok("已请求重连手柄")
            }
            UiCommand::RefreshDevices => UiReply::ok("设备清单已刷新"),
            UiCommand::RestartThreads => {
                let names = crate::util::stopped_threads();
                let restarted = names
                    .iter()
                    .filter(|n| crate::util::restart_thread(*n))
                    .count();
                UiReply::ok(format!("已重启 {restarted} 个线程"))
            }
            UiCommand::ToggleTheme => {
                self.dark_mode = !self.dark_mode;
                self.config.dark_mode = self.dark_mode;
                let _ = self.config.save_to_file("Config.toml");
                if let Some(temp) = &mut self.temp_config {
                    temp.dark_mode = self.dark_mode;
                }
                blip();
                UiReply::ok(if self.dark_mode { "已切到深色" } else { "已切到浅色" })
            }
            UiCommand::ShowGuide => {
                self.first_run_rerun = true;
                self.dfo_ask_answered = false;
                self.show_edition_ask = false;
                self.show_guide = false;
                self.modal_defer = 0;
                UiReply::ok("开局引导已打开")
            }
            UiCommand::ModeFull | UiCommand::ModeMinimal | UiCommand::ModeTurboOnly => {
                let mode = cmd.mode().unwrap_or(UiMode::Full);
                self.ef_mode = mode;
                blip();
                pulse(&self.app_state, 120);
                self.ef_toast = Some((format!("已切到{}模式", mode.label()), Instant::now()));
                UiReply::ok(format!("已切到{}模式", mode.label()))
            }
        }
    }
}
