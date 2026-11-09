//! ★v24.55 WinHTTP 最小封装 —— 检查更新/下载发行包专用的同步 HTTP 客户端。
//!
//! 为什么用 WinHTTP 而不是 reqwest/ureq:
//! 1. 单 exe 哲学: 不引入 TLS 实现 (ring/rustls 会显著增体积), 也无需打包 CA 证书;
//! 2. 企业/校园网/代理环境: WinHTTP 直接用系统代理配置 (PAC/自动发现), 与浏览器一致;
//! 3. 运行时只依赖系统 winhttp.dll (Win7 起内置), 无额外 DLL。
//!
//! 约定: 本模块全部函数**阻塞**, 只允许在后台线程调用 (UI 线程调用会卡帧)。
//! 出错一律 `anyhow::Result`, 由上层转成界面上的一句人话。

use std::ffi::c_void;
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Context, Result, anyhow, bail};
use windows::Win32::Networking::WinHttp::{
    WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, WINHTTP_ACCESS_TYPE_DEFAULT_PROXY, WINHTTP_FLAG_SECURE,
    WINHTTP_OPEN_REQUEST_FLAGS, WINHTTP_QUERY_CONTENT_LENGTH, WINHTTP_QUERY_FLAG_NUMBER,
    WINHTTP_QUERY_STATUS_CODE, WinHttpCloseHandle, WinHttpConnect, WinHttpOpen, WinHttpOpenRequest,
    WinHttpQueryDataAvailable, WinHttpQueryHeaders, WinHttpReadData, WinHttpReceiveResponse,
    WinHttpSendRequest, WinHttpSetTimeouts,
};
use windows::core::PCWSTR;

const USER_AGENT: &str = "SorahkDFO-Updater/1.0";
/// 清单类文本响应的体积上限 —— 防止误连到大文件时把内存吃满。
const MAX_TEXT_BYTES: usize = 4 * 1024 * 1024;

/* ───────────────────────── 句柄 RAII ───────────────────────── */

/// WinHTTP 句柄包装。`WinHttp*` 系列失败返回 NULL (不设 last_error 的场合) 或返回
/// `Result::Err`, 两种都归一到这里; Drop 时自动 `WinHttpCloseHandle`。
struct Handle(*mut c_void);

impl Handle {
    fn new(raw: *mut c_void, what: &str) -> Result<Self> {
        if raw.is_null() {
            return Err(anyhow!("{what} 失败: {}", std::io::Error::last_os_error()));
        }
        Ok(Self(raw))
    }

    fn raw(&self) -> *mut c_void {
        self.0
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                let _ = WinHttpCloseHandle(self.0);
            }
        }
    }
}

/* ───────────────────────── URL 拆分 ───────────────────────── */

struct UrlParts {
    secure: bool,
    host: String,
    port: u16,
    /// 请求行里的目标 (路径 + 查询串), 形如 `/a/b.json?x=1`
    target: String,
}

/// 只支持 http/https + 主机 + 可选端口 + 路径/查询。够用即可 (清单地址都是这种)。
fn parse_url(raw: &str) -> Result<UrlParts> {
    let raw = raw.trim();
    let (scheme, rest) = raw
        .split_once("://")
        .ok_or_else(|| anyhow!("地址缺少 http(s):// 前缀: {raw}"))?;
    let secure = match scheme.to_ascii_lowercase().as_str() {
        "https" => true,
        "http" => false,
        other => bail!("不支持的协议 {other} (只支持 http/https)"),
    };
    let (hostport, path) = match rest.find(['/', '?']) {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    let (host, port) = match hostport.rsplit_once(':') {
        Some((h, p)) if !h.is_empty() && !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()) => {
            (h.to_string(), p.parse::<u16>().context("端口号不合法")?)
        }
        _ => (
            hostport.to_string(),
            if secure { 443 } else { 80 },
        ),
    };
    if host.is_empty() {
        bail!("地址缺少主机名: {raw}");
    }
    let target = if path.starts_with('?') {
        format!("/{path}")
    } else {
        path.to_string()
    };
    Ok(UrlParts {
        secure,
        host,
        port,
        target,
    })
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/* ───────────────────────── 请求 ───────────────────────── */

/// 已完成的 GET 响应 (连接/请求句柄随结构体一起释放)。
struct Response {
    // 字段顺序 = 释放顺序: 请求 → 连接 → 会话
    request: Handle,
    _connect: Handle,
    _session: Handle,
}

/// 发起 GET 并等到响应头 (不读 body)。
///
/// 自动处理: 系统代理 (AUTOMATIC_PROXY, Win8.1+; 失败回退 DEFAULT_PROXY)、
/// TLS 1.2+、3xx 重定向。HTTP 状态码非 2xx 由此函数报错。
fn request(url: &str) -> Result<Response> {
    let parts = parse_url(url)?;
    let agent = wide(USER_AGENT);

    unsafe {
        let mut session_raw = WinHttpOpen(
            PCWSTR::from_raw(agent.as_ptr()),
            WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
            PCWSTR::null(),
            PCWSTR::null(),
            0,
        );
        if session_raw.is_null() {
            /* Win7 不认 AUTOMATIC_PROXY(4) → 退回"默认代理 + IE 设置" */
            session_raw = WinHttpOpen(
                PCWSTR::from_raw(agent.as_ptr()),
                WINHTTP_ACCESS_TYPE_DEFAULT_PROXY,
                PCWSTR::null(),
                PCWSTR::null(),
                0,
            );
        }
        let session = Handle::new(session_raw, "初始化 HTTP 会话")?;
        /* 解析/连接/发送/接收 四个超时 (毫秒): 全部限死, 绝不让后台线程无限挂着 */
        let _ = WinHttpSetTimeouts(session.raw(), 6000, 9000, 9000, 25000);

        let host_w = wide(&parts.host);
        let connect = Handle::new(
            WinHttpConnect(
                session.raw(),
                PCWSTR::from_raw(host_w.as_ptr()),
                parts.port,
                0,
            ),
            "连接更新服务器",
        )
        .with_context(|| format!("连接 {}:{} 失败", parts.host, parts.port))?;

        let verb_w = wide("GET");
        let target_w = wide(&parts.target);
        let flags = if parts.secure {
            WINHTTP_FLAG_SECURE
        } else {
            WINHTTP_OPEN_REQUEST_FLAGS(0)
        };
        let request = Handle::new(
            WinHttpOpenRequest(
                connect.raw(),
                PCWSTR::from_raw(verb_w.as_ptr()),
                PCWSTR::from_raw(target_w.as_ptr()),
                PCWSTR::null(),
                PCWSTR::null(),
                std::ptr::null(),
                flags,
            ),
            "构造 HTTP 请求",
        )?;

        WinHttpSendRequest(request.raw(), None, None, 0, 0, 0).context("发送请求失败")?;
        WinHttpReceiveResponse(request.raw(), std::ptr::null_mut()).context("等待服务器响应失败")?;

        let code = status_code(request.raw())?;
        if !(200..300).contains(&code) {
            bail!("服务器返回 HTTP {code} (地址: {url})");
        }

        Ok(Response {
            request,
            _connect: connect,
            _session: session,
        })
    }
}

/// 读响应头里的 32 位整数项 (状态码 / 内容长度)。
unsafe fn header_u32(request: *mut c_void, level: u32) -> Result<u32> {
    let mut value: u32 = 0;
    let mut len: u32 = std::mem::size_of::<u32>() as u32;
    unsafe {
        WinHttpQueryHeaders(
            request,
            level | WINHTTP_QUERY_FLAG_NUMBER,
            PCWSTR::null(),
            Some(&mut value as *mut u32 as *mut c_void),
            &mut len,
            std::ptr::null_mut(),
        )?;
    }
    Ok(value)
}

unsafe fn status_code(request: *mut c_void) -> Result<u32> {
    unsafe { header_u32(request, WINHTTP_QUERY_STATUS_CODE) }
}

fn content_length(request: *mut c_void) -> u64 {
    unsafe { header_u32(request, WINHTTP_QUERY_CONTENT_LENGTH) }.unwrap_or(0) as u64
}

/// 逐块拉取 body; `sink` 写盘, `progress(已收字节, 总字节)` 报进度 (总字节 0 = 未知)。
/// 返回本次实际收到的字节数。
fn pump(
    resp: &Response,
    mut sink: impl FnMut(&[u8]) -> Result<()>,
    mut progress: impl FnMut(u64, u64),
) -> Result<u64> {
    let total = content_length(resp.request.raw());
    let mut buf = vec![0u8; 64 * 1024];
    let mut got_total: u64 = 0;
    loop {
        let mut avail: u32 = 0;
        unsafe { WinHttpQueryDataAvailable(resp.request.raw(), &mut avail) }
            .context("读取响应数据失败 (连接可能已中断)")?;
        if avail == 0 {
            break;
        }
        let want = (avail as usize).min(buf.len());
        let mut read: u32 = 0;
        unsafe {
            WinHttpReadData(
                resp.request.raw(),
                buf.as_mut_ptr() as *mut c_void,
                want as u32,
                &mut read,
            )
        }
        .context("读取响应数据出错")?;
        if read == 0 {
            break;
        }
        sink(&buf[..read as usize])?;
        got_total += read as u64;
        progress(got_total, total);
    }
    Ok(got_total)
}

/* ───────────────────────── 对外接口 ───────────────────────── */

/// GET 一段文本 (UTF-8)。超过 4MB 视为异常直接拒绝。
pub fn get_text(url: &str) -> Result<String> {
    let resp = request(url)?;
    let mut body: Vec<u8> = Vec::with_capacity(16 * 1024);
    pump(
        &resp,
        |chunk| {
            if body.len() + chunk.len() > MAX_TEXT_BYTES {
                bail!("响应体超过 {MAX_TEXT_BYTES} 字节, 已中止");
            }
            body.extend_from_slice(chunk);
            Ok(())
        },
        |_, _| {},
    )?;
    /* GitHub API 与自建清单都是 UTF-8; 偶发的 BOM 一并吃掉 */
    if body.starts_with(&[0xEF, 0xBB, 0xBF]) {
        body.drain(..3);
    }
    String::from_utf8(body).map_err(|e| anyhow!("响应不是合法 UTF-8 文本: {e}"))
}

/// 下载到 `dest`。先写 `dest.part`, 全部完成才改名 —— 中断/失败不会留下半截包。
/// `cancel` 置位时尽快返回 (已写的 .part 会被删除)。
pub fn download(
    url: &str,
    dest: &Path,
    cancel: &AtomicBool,
    mut progress: impl FnMut(u64, u64),
) -> Result<u64> {
    let resp = request(url)?;
    /* ★v24.57 记下服务端声明的长度: 80MB 的包走家用宽带, 断流并不罕见,
     * 而 WinHttpQueryDataAvailable 在对端提前关闭时同样返回 0 —— 若不比对,
     * 半截 zip 会被当成下载成功, 玩家解压时才炸。 */
    let declared = content_length(resp.request.raw());
    if let Some(dir) = dest.parent() {
        std::fs::create_dir_all(dir)
            .with_context(|| format!("无法创建目录 {}", dir.display()))?;
    }
    let part = dest.with_extension("part");
    let mut file = std::fs::File::create(&part)
        .with_context(|| format!("无法写入 {}", part.display()))?;
    let mut cancelled = false;

    let result = pump(
        &resp,
        |chunk| {
            if cancel.load(Ordering::Relaxed) {
                cancelled = true;
                bail!("已取消");
            }
            file.write_all(chunk).context("写入文件失败 (磁盘满/无权限?)")
        },
        |done, total| progress(done, total),
    );

    drop(file);
    match result {
        Ok(n) if n > 0 => {
            if declared > 0 && n != declared {
                let _ = std::fs::remove_file(&part);
                bail!("下载不完整: 服务端声明 {declared} 字节, 实际只收到 {n} 字节（网络中断，请重试）");
            }
            std::fs::rename(&part, dest)
                .with_context(|| format!("无法落盘到 {}", dest.display()))?;
            Ok(n)
        }
        Ok(_) => {
            let _ = std::fs::remove_file(&part);
            bail!("服务器返回了空文件")
        }
        Err(e) => {
            let _ = std::fs::remove_file(&part);
            if cancelled {
                Err(anyhow!("已取消下载"))
            } else {
                Err(e)
            }
        }
    }
}
