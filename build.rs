fn main() {
    // ★v24.60 音频震动引擎已改为纯算法 (无 ONNX 模型), 旧 yamnet/mrx 内嵌机制随
    // AI 层一并移除; 此处只保留 Windows 资源编译。

    // Only compile resources on Windows
    #[cfg(windows)]
    {
        // ★R2.12: 普通发行版一律 asInvoker (默认不嵌 manifest, 或显式 =1 时嵌 asInvoker 清单),
        // 双击不弹 UAC, headless 渲染 / CI 也能在非提权 shell 直接跑 (旧 highestAvailable
        // 曾让实机普通权限启动报 os error 740)。仅显式 SORAHK_MANIFEST=admin 才嵌入
        // highestAvailable 变体, 供 90CN 这类已提权客户端使用。
        println!("cargo:rerun-if-env-changed=SORAHK_MANIFEST");
        let _ = embed_resource::compile("resources/sorahk.rc", embed_resource::NONE);
        match std::env::var("SORAHK_MANIFEST").as_deref() {
            Ok("admin") => {
                let _ = embed_resource::compile(
                    "resources/sorahk_manifest_admin.rc",
                    embed_resource::NONE,
                );
            }
            Ok("1") => {
                // 兼容旧脚本: 旧 =1 期望提权, 现在只嵌 asInvoker 清单 (等价普通权限)。
                let _ = embed_resource::compile("resources/sorahk_manifest.rc", embed_resource::NONE);
            }
            _ => {}
        }
    }
}
