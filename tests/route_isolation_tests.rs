//! ★v24.15 路线隔离回归测试 —— 启动路径上的三个串味/致命口子。
//!
//! 全部在一个 #[test] 里跑: 这些场景要 chdir 到各自的临时目录 (load_job_vibration /
//! load_or_create 都按 cwd 找文件), 而 integration test 的多个 #[test] 是同进程多线程
//! 并行的 —— 只有一个测试函数时 chdir 才是安全的。
//!
//! 覆盖:
//!   1. S4 路线 + JobVibration.toml 里是 S1 的 ACT 职业 → 职业预设不得生效
//!      (修前: ACT 职业参数覆盖 S4 文件参数, 并会随保存写进 S4 的震动文件)
//!   2. S1 路线 + 同一个 ACT 职业 → 必须照常生效 (防"修过头")
//!   3. 路线文件缺失 + 老 Config.toml 残留 [vibration] 段 → 按出厂默认 (不继承旧段)
//!   4. 路线文件损坏 → 启动不失败, 坏文件挪成 .bad, 预设表回内置

use sorahk::config::AppConfig;
use sorahk::state::AppState;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;

fn work_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("sorahk_route_{}_{}", tag, std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    std::env::set_current_dir(&d).unwrap();
    d
}

/// 写一份合法 Config.toml (字段多, 手工拼会漏), 返回给调用方改。
fn write_config(legacy: bool) -> String {
    let p = Path::new("Config.toml");
    let mut c = AppConfig::default();
    c.vib_legacy_client = legacy;
    c.save_config_only(p).unwrap();
    assert_eq!(c.vib_legacy_client, legacy);
    std::fs::read_to_string(p).unwrap()
}

/// 写 JobVibration.toml: 某个职业, applied, 参数全 = v。
fn write_job(base: &str, class: &str, v: u32) {
    let params = vec![v.to_string(); 60].join(", ");
    std::fs::write(
        "JobVibration.toml",
        format!(
            "base_job = \"{base}\"\nclass_name = \"{class}\"\napplied = true\n\
             params = [{params}]\nrank_duration = 250\nrank_level_gain = 50\n"
        ),
    )
    .unwrap();
}

fn act_job() -> (String, String) {
    let jobs = sorahk::job_presets::available_jobs(true);
    (jobs[0].base_job.to_string(), jobs[0].classes[0].name.to_string())
}

#[test]
fn route_isolation_startup_paths() {
    let tmp_root = std::env::temp_dir();
    let _ = tmp_root;

    /* ── 场景 1: S4 路线, JobVibration 里却是 S1 的 ACT 职业 ── */
    {
        work_dir("s4_job");
        write_config(false);
        std::fs::write("Vibration.toml", "[vibration]\nattack_gain = 77\nmaster_gain = 77\n")
            .unwrap();
        let (base, class) = act_job();
        write_job(&base, &class, 11);
        let cfg = AppConfig::load_or_create("Config.toml").unwrap();
        assert!(!cfg.vib_legacy_client);
        assert_eq!(cfg.vibration.attack_gain, 77, "S4 路线文件应被载入");
        assert!(
            sorahk::job_presets::find_class_for_route(&base, &class, false).is_none(),
            "ACT 职业本就不属于 S4 名册"
        );
        let st = AppState::new(cfg).unwrap();
        assert_eq!(
            st.vibration_params[0].load(Ordering::Relaxed),
            77,
            "S1 的 ACT 职业预设不得覆盖 S4 路线的启动参数"
        );
    }

    /* ── 场景 2: S1 路线 + 同一个 ACT 职业 (必须照常生效) ── */
    {
        work_dir("s1_job");
        write_config(true);
        std::fs::write(
            "Vibration-ACT.toml",
            "[vibration]\nattack_gain = 77\nmaster_gain = 77\n",
        )
        .unwrap();
        let (base, class) = act_job();
        write_job(&base, &class, 33);
        let cfg = AppConfig::load_or_create("Config.toml").unwrap();
        assert!(cfg.vib_legacy_client);
        let st = AppState::new(cfg).unwrap();
        assert_eq!(
            st.vibration_params[0].load(Ordering::Relaxed),
            33,
            "S1 路线的 ACT 职业预设必须照常生效 (防修过头)"
        );
    }

    /* ── 场景 3: 老 Config.toml 残留 [vibration] 段 + 路线文件缺失 ── */
    {
        work_dir("stale_section");
        let mut s = write_config(false);
        s.push_str("\n[vibration]\nattack_gain = 55\nmaster_gain = 55\n");
        std::fs::write("Config.toml", s).unwrap();
        let cfg = AppConfig::load_or_create("Config.toml").unwrap();
        assert_eq!(
            cfg.vibration.attack_gain, 0,
            "路线文件缺失时应按出厂默认, 不继承 Config.toml 里残留的旧 [vibration] 段"
        );
        assert!(
            !cfg.vibration_presets.is_empty(),
            "预设表必须回内置 (空表会让预设下拉整个空掉)"
        );
    }

    /* ── 场景 4: 路线文件损坏 ── */
    {
        let d = work_dir("corrupt");
        write_config(true);
        std::fs::write("Vibration-ACT.toml", "这不是 TOML {{{").unwrap();
        let cfg = AppConfig::load_or_create("Config.toml")
            .expect("损坏的震动文件不得阻断启动 (v23.1 纪律)");
        assert!(cfg.vib_legacy_client, "路线标记仍按 Config.toml");
        assert!(
            d.join("Vibration-ACT.toml.bad").exists(),
            "坏文件应挪成 .bad 留档, 而不是丢掉"
        );
        assert!(!d.join("Vibration-ACT.toml").exists());
        assert!(!cfg.vibration_presets.is_empty());
    }

    let _ = std::env::set_current_dir(std::env::temp_dir());
}
