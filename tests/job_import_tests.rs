//! job_export_*.toml 导入校验回归测试。
//!
//! 背景: parse_job_export 此前只校验 params 长度, rank_lr/rank_type_gain
//! 完全不校验——导入 31 项 rank_lr 的文件时, GUI 以裸下标写入
//! [AtomicU32; 30], 越界 panic 且发生在 egui update 调用栈里, 进程直接退出。

use sorahk::job_presets::{parse_job_export, parse_vibration_export};
use std::io::Write;

fn toml_arr(n: usize) -> String {
    let v = vec!["0"; n];
    format!("[{}]", v.join(", "))
}

fn write_export(tag: &str, rank_lr_len: usize, gain_len: usize) -> String {
    let p = std::env::temp_dir().join(format!(
        "sorahk_job_export_{}_{}.toml",
        tag,
        std::process::id()
    ));
    let body = format!(
        "base_job = \"鬼剑士\"\n\
         class_name = \"剑魂\"\n\
         params = {}\n\
         rank_lr = {}\n\
         rank_type_gain = {}\n",
        toml_arr(60),
        toml_arr(rank_lr_len),
        toml_arr(gain_len)
    );
    let mut f = std::fs::File::create(&p).unwrap();
    f.write_all(body.as_bytes()).unwrap();
    p.to_string_lossy().to_string()
}

#[test]
fn job_import_rejects_wrong_length_arrays() {
    // rank_lr 31 项 → 修复前 parse 返回 Some, GUI 裸下标 [30] 越界
    let p = write_export("bad_rank_lr", 31, 15);
    assert!(
        parse_job_export(&p).is_none(),
        "rank_lr 长度≠30 必须拒绝导入"
    );

    let p = write_export("bad_gain", 30, 16);
    assert!(
        parse_job_export(&p).is_none(),
        "rank_type_gain 长度≠15 必须拒绝导入"
    );

    let p = write_export("good", 30, 15);
    assert!(parse_job_export(&p).is_some(), "合法导入不应被拒绝");

    for tag in ["bad_rank_lr", "bad_gain", "good"] {
        let _ = std::fs::remove_file(std::env::temp_dir().join(format!(
            "sorahk_job_export_{}_{}.toml",
            tag,
            std::process::id()
        )));
    }
}

/* ── 导入值上限回归 (2026-09-08): 解析器此前只查长度不查范围,
   手改/损坏的导出文件可把 u32::MAX 级数值直送震动引擎;
   rank_lr 负数经 `as u32` 回绕成天文数字。钳位与震动页滑块范围一致,
   程序生成的合法导出永远在范围内, 不受影响。 ── */

#[test]
fn job_import_clamps_out_of_range_values() {
    let p = std::env::temp_dir().join(format!("sorahk_job_clamp_{}.toml", std::process::id()));
    let mut params = vec!["0"; 60];
    params[0] = "9999999"; // 攻击频率 % (滑块 0..=100)
    params[5] = "4000000000"; // 衰减时间 ms (滑块 0..=500)
    params[19] = "999999"; // 马达曲线 % (滑块 30..=300)
    params[46] = "99999999"; // 空闲判定 ms (滑块 1000..=10000)
    let mut rank_lr = vec!["0"; 30];
    rank_lr[0] = "-5000"; // 负数此前 `as u32` 回绕
    let mut gains = vec!["0"; 15];
    gains[0] = "999";
    let body = format!(
        "base_job = \"鬼剑士\"\nclass_name = \"剑魂\"\n\
         params = [{}]\nrank_lr = [{}]\nrank_type_gain = [{}]\n\
         rank_level_gain = 4000000000\nrank_duration = 99999999\n\
         out_smooth = 99999999\nthrottle_window = 999999999\n\
         throttle_max = 9999\nthrottle_dense_ratio = 9999\n",
        params.join(", "),
        rank_lr.join(", "),
        gains.join(", ")
    );
    std::fs::File::create(&p).unwrap().write_all(body.as_bytes()).unwrap();

    let ij = parse_job_export(p.to_string_lossy().as_ref()).expect("应能解析");
    assert_eq!(ij.params[0], 100, "% 类参数应钳到 100");
    assert_eq!(ij.params[5], 500, "衰减 ms 应钳到 500");
    assert_eq!(ij.params[19], 300, "马达曲线应钳到 300");
    assert_eq!(ij.params[46], 10000, "空闲判定 ms 应钳到 10000");
    assert!(
        (-100..=100).contains(&ij.rank_lr[0]),
        "rank_lr 应钳到 ±100, 实际 {}",
        ij.rank_lr[0]
    );
    assert_eq!(ij.rank_type_gain[0], 100, "rank_type_gain 应钳到 100");
    assert_eq!(ij.rank_level_gain, 100);
    assert_eq!(ij.rank_duration, 1000);
    assert_eq!(ij.out_smooth, 100);
    assert!(ij.throttle_window <= 10000);
    assert!(ij.throttle_max <= 30);
    assert!(ij.throttle_dense_ratio <= 100);
    let _ = std::fs::remove_file(&p);
}

#[test]
fn job_import_keeps_in_range_values_unchanged() {
    let p = std::env::temp_dir().join(format!("sorahk_job_ok_{}.toml", std::process::id()));
    let mut params = vec!["0"; 60];
    params[0] = "80";
    params[5] = "300";
    let mut rank_lr = vec!["0"; 30];
    rank_lr[0] = "-30";
    let mut gains = vec!["0"; 15];
    gains[0] = "85";
    let body = format!(
        "base_job = \"鬼剑士\"\nclass_name = \"剑魂\"\n\
         params = [{}]\nrank_lr = [{}]\nrank_type_gain = [{}]\n\
         rank_level_gain = 60\nrank_duration = 500\nout_smooth = 40\n\
         throttle_window = 2000\nthrottle_max = 12\nthrottle_dense_ratio = 50\n",
        params.join(", "),
        rank_lr.join(", "),
        gains.join(", ")
    );
    std::fs::File::create(&p).unwrap().write_all(body.as_bytes()).unwrap();

    let ij = parse_job_export(p.to_string_lossy().as_ref()).expect("应能解析");
    assert_eq!(ij.params[0], 80, "合法值不得被钳位误伤");
    assert_eq!(ij.params[5], 300);
    assert_eq!(ij.rank_lr[0], -30);
    assert_eq!(ij.rank_type_gain[0], 85);
    assert_eq!(ij.rank_level_gain, 60);
    assert_eq!(ij.rank_duration, 500);
    assert_eq!(ij.out_smooth, 40);
    assert_eq!(ij.throttle_window, 2000);
    assert_eq!(ij.throttle_max, 12);
    assert_eq!(ij.throttle_dense_ratio, 50);
    let _ = std::fs::remove_file(&p);
}

#[test]
fn vibration_import_clamps_out_of_range_values() {
    let p = std::env::temp_dir().join(format!("sorahk_vib_clamp_{}.toml", std::process::id()));
    let mut params = vec!["0"; 60];
    params[0] = "9999999";
    let mut item_lr = vec!["0"; 26];
    item_lr[0] = "4294967266"; // -30 的二补码, 必须原样保留
    item_lr[1] = "9999999"; // 越界正数 → 钳到 100
    let mut rank_lr = vec!["0"; 30];
    rank_lr[0] = "4294967196"; // -100 的二补码, 边界保留
    let mut gains = vec!["0"; 15];
    gains[0] = "999";
    let body = format!(
        "params = [{}]\nitem_lr = [{}]\nrank_lr = [{}]\nrank_type_gain = [{}]\n\
         rank_level_gain = 999999\nrank_duration = 999999\nout_smooth = 999999\n\
         throttle_window_ms = 99999999\nthrottle_max_hits = 9999\n\
         throttle_dense_ratio = 9999\nrandom_gain = 9999\n\
         motor_l_gain = 9999\nmotor_r_gain = 9999\n",
        params.join(", "),
        item_lr.join(", "),
        rank_lr.join(", "),
        gains.join(", ")
    );
    std::fs::File::create(&p).unwrap().write_all(body.as_bytes()).unwrap();

    let v = parse_vibration_export(p.to_string_lossy().as_ref()).expect("应能解析");
    assert_eq!(v.params[0], 100);
    assert_eq!(v.item_lr[0], 4294967266, "合法的 -30 二补码不得被误钳");
    assert_eq!(v.item_lr[1] as i32, 100, "越界权重应钳到 +100");
    assert_eq!(v.rank_lr[0], 4294967196, "合法的 -100 二补码不得被误钳");
    assert_eq!(v.rank_type_gain[0], 100);
    assert_eq!(v.rank_level_gain, 100);
    assert_eq!(v.rank_duration, 1000);
    assert_eq!(v.out_smooth, 100);
    assert!(v.throttle_window_ms <= 10000);
    assert!(v.throttle_max_hits <= 30);
    assert!(v.throttle_dense_ratio <= 100);
    assert_eq!(v.random_gain, 100);
    assert_eq!(v.motor_l_gain, 100);
    assert_eq!(v.motor_r_gain, 100);
    let _ = std::fs::remove_file(&p);
}
