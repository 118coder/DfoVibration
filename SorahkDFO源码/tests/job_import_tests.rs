//! job_export_*.toml 导入校验回归测试。
//!
//! 背景: parse_job_export 此前只校验 params 长度, rank_lr/rank_type_gain
//! 完全不校验——导入 31 项 rank_lr 的文件时, GUI 以裸下标写入
//! [AtomicU32; 30], 越界 panic 且发生在 egui update 调用栈里, 进程直接退出。

use sorahk::job_presets::parse_job_export;
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
