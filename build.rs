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

        // ★双 UI 壳 (契约 docs/双UI桥契约_v1.md): wry/WebView2 在 windows-gnu 工具链下
        // 以动态导入方式链接 WebView2Loader.dll (webview2-com-sys 的 build.rs 只把 DLL
        // 拷进自己 crate 的 OUT_DIR, 不会带到产物旁), 缺它 = 进程启动即 STATUS_DLL_NOT_FOUND
        // (wry spike 实测结论)。这里从 cargo registry 的 crate 源码目录找 x64 版 DLL,
        // 拷到 target/<profile>/ 供运行/交付。找不到时静默跳过 (msvc 静态链接场景不需要)。
        copy_webview2_loader_dll();
    }
}

/// 把 webview2-com-sys 包里的 x64/WebView2Loader.dll 拷到产物 profile 目录 (best-effort)。
#[cfg(windows)]
fn copy_webview2_loader_dll() {
    use std::path::{Path, PathBuf};

    let Ok(out_dir) = std::env::var("OUT_DIR") else { return };
    // OUT_DIR = <target>/<profile>/build/webview2-com-sys-<hash>/out → 上三级 = <profile>
    let Some(profile_dir) = Path::new(&out_dir).ancestors().nth(3).map(Path::to_path_buf) else {
        return;
    };
    let Ok(cargo_home) = std::env::var("CARGO_HOME") else { return };
    let src_root = PathBuf::from(cargo_home).join("registry").join("src");
    let Ok(entries) = std::fs::read_dir(&src_root) else { return };
    for reg in entries.flatten() {
        let Ok(crates) = std::fs::read_dir(reg.path()) else { continue };
        for dir in crates.flatten() {
            let name = dir.file_name();
            let name = name.to_string_lossy();
            if !name.starts_with("webview2-com-sys-") {
                continue;
            }
            let dll = dir.path().join("x64").join("WebView2Loader.dll");
            if dll.is_file() {
                let dest = profile_dir.join("WebView2Loader.dll");
                // 内容相同则不重拷 (避免每次构建都动产物时间戳)
                let same = std::fs::read(&dll).map(|b| {
                    std::fs::read(&dest).map(|d| d == b).unwrap_or(false)
                }).unwrap_or(false);
                if !same {
                    let _ = std::fs::copy(&dll, &dest);
                }
                return;
            }
        }
    }
}
