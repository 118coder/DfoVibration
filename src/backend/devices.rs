//! ★R2.14 设备/更新/系统领域 (BackendService impl)。
//!
//! 设备枚举是对系统 API 的只读透传；更新管理器所有权从 GUI 归位到这里。

use super::BackendService;

impl BackendService {
    // ── 设备枚举 (只读探测，对话框打开时自行节流调用) ───────────────────

    pub fn enumerate_xinput_devices(&self) -> Vec<crate::xinput::XInputDeviceInfo> {
        crate::xinput::XInputHandler::enumerate_devices()
    }

    pub fn enumerate_hid_devices(&self) -> Vec<crate::rawinput::HidDeviceInfo> {
        crate::rawinput::enumerate_hid_devices()
    }

    pub fn enumerate_raw_hid_gamepads(&self) -> Vec<crate::rawinput::RawHidGamepad> {
        crate::rawinput::enumerate_raw_hid_gamepads()
    }

    /// 重置 XInput 连接 (旧 device_manager_dialog.rs:290 语义)。
    pub fn request_xinput_reset(&self) {
        crate::input_manager::request_xinput_reset();
    }

    /// 设备 API 偏好写入配置 (旧 device_manager_dialog.rs:694-722 语义)。
    pub fn set_device_api_preference(
        &self,
        name: &str,
        pref: crate::config::DeviceApiPreference,
    ) -> anyhow::Result<()> {
        self.update_config(crate::ui_api::Commit::Save, |c| {
            c.device_api_preferences.insert(name.to_string(), pref);
        })?;
        Ok(())
    }

    // ── 检查更新 (UpdateManager 归位；GUI 经此调用) ─────────────────────

    /// 手动触发检查 (manifest 来自 config.update.manifest_url)。
    pub fn update_check(&self) -> bool {
        let manifest = self.cfg.with_config(|c| c.update.manifest_url.clone());
        self.update.check(manifest)
    }

    /// 启动时自动检查是否到期 (auto_check 语义)。
    pub fn update_auto_check_due(&self) -> bool {
        self.update.auto_check_due()
    }

    pub fn update_cancel_download(&self) {
        self.update.cancel_download();
    }

    pub fn update_dismiss_error(&self) {
        self.update.dismiss_error();
    }

    /// 下载指定版本的指定渠道包 (GUI 从 latest.packages 里选择)。
    pub fn update_download(
        &self,
        version: String,
        pkg: crate::update::Package,
        dir: std::path::PathBuf,
    ) -> bool {
        self.update.download(version, pkg, dir)
    }

    // ── 看护线程 ────────────────────────────────────────────────────────

    /// 当前已停止的看护线程名单 (设备管理页"一键重启"列表)。
    pub fn stopped_threads(&self) -> Vec<&'static str> {
        crate::util::stopped_threads()
    }

    /// 重启某个已停止的看护线程。
    pub fn restart_thread(&self, name: &str) -> bool {
        crate::util::restart_thread(name)
    }

    // ── 注入保险统计 (诊断胶囊) ─────────────────────────────────────────

    pub fn injection_guard_stats(&self) -> (u64, u64, bool) {
        crate::state::injection_guard_stats()
    }
}
