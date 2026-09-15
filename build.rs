fn main() {
    // Only compile resources on Windows
    #[cfg(windows)]
    {
        // ★v24.20a: RT_MANIFEST(highestAvailable) 默认**不**嵌入 (用户决策:
        // 不默认管理员运行, 90CN 靠使用说明引导右键管理员) 。
        // 如需恢复静默提权构建: SORAHK_MANIFEST=1 。注意带 manifest 的
        // 可执行体 (cargo test 产物) 非提权 shell 启动会 os error 740。
        println!("cargo:rerun-if-env-changed=SORAHK_MANIFEST");
        let _ = embed_resource::compile("resources/sorahk.rc", embed_resource::NONE);
        if std::env::var("SORAHK_MANIFEST").as_deref() == Ok("1") {
            let _ = embed_resource::compile("resources/sorahk_manifest.rc", embed_resource::NONE);
        }
    }
}
