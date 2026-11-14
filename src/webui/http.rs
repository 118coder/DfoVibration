//! 极简 HTTP/1.1 服务：只做本地界面需要的那点事。
//!
//! 不引第三方依赖：监听回环地址、一次连接一个请求、响应后关连接。
//! 请求行 + 头部 + 可选 body（带 Content-Length 上限），够 SPA + JSON + SSE 用。

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::TcpStream;

/// 单个请求能带的最大 body（防止误传大文件把内存吃满）。
const MAX_BODY: usize = 4 * 1024 * 1024;
/// 头部上限。
const MAX_HEAD: usize = 64 * 1024;

pub struct Request {
    pub method: String,
    pub path: String,
    pub query: Vec<(String, String)>,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
}

impl Request {
    pub fn query(&self, key: &str) -> Option<&str> {
        self.query
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    pub fn header(&self, key: &str) -> Option<&str> {
        self.headers.get(&key.to_ascii_lowercase()).map(String::as_str)
    }

    /// 取口令：POST 走自定义头，SSE 走 query（EventSource 设不了头）。
    pub fn token(&self) -> Option<&str> {
        self.header("x-sorahk-token").or_else(|| self.query("t"))
    }
}

pub struct Response {
    pub status: u16,
    pub content_type: &'static str,
    pub body: Vec<u8>,
    pub headers: Vec<(&'static str, String)>,
}

impl Response {
    pub fn new(status: u16, content_type: &'static str, body: Vec<u8>) -> Self {
        Self {
            status,
            content_type,
            body,
            headers: Vec::new(),
        }
    }

    pub fn json(value: &serde_json::Value) -> Self {
        Self::new(
            200,
            "application/json; charset=utf-8",
            serde_json::to_vec(value).unwrap_or_else(|_| b"{}".to_vec()),
        )
    }

    pub fn text(message: &str) -> Self {
        Self::new(200, "text/plain; charset=utf-8", message.as_bytes().to_vec())
    }

    pub fn error(status: u16, message: &str) -> Self {
        let body = serde_json::json!({ "ok": false, "error": message });
        Self::new(status, "application/json; charset=utf-8", body.to_string().into_bytes())
    }
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("");
                match u8::from_str_radix(hex, 16) {
                    Ok(b) => {
                        out.push(b);
                        i += 3;
                    }
                    Err(_) => {
                        out.push(bytes[i]);
                        i += 1;
                    }
                }
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// 读一个请求；连接过早关闭时返回 None（浏览器预连接会这样）。
pub fn read_request(stream: &mut TcpStream) -> Option<Request> {
    let mut buf = Vec::with_capacity(4096);
    let mut chunk = [0u8; 4096];
    let head_end;
    loop {
        let n = stream.read(&mut chunk).ok()?;
        if n == 0 {
            return None;
        }
        buf.extend_from_slice(&chunk[..n]);
        if let Some(pos) = find_head_end(&buf) {
            head_end = pos;
            break;
        }
        if buf.len() > MAX_HEAD {
            return None;
        }
    }

    let head = String::from_utf8_lossy(&buf[..head_end]).into_owned();
    let mut lines = head.split("\r\n");
    let request_line = lines.next()?;
    let mut parts = request_line.split_whitespace();
    let method = parts.next()?.to_string();
    let target = parts.next()?.to_string();

    let (path, query_str) = match target.split_once('?') {
        Some((p, q)) => (p.to_string(), q.to_string()),
        None => (target, String::new()),
    };
    let query = query_str
        .split('&')
        .filter(|s| !s.is_empty())
        .map(|pair| match pair.split_once('=') {
            Some((k, v)) => (percent_decode(k), percent_decode(v)),
            None => (percent_decode(pair), String::new()),
        })
        .collect();

    let mut headers = HashMap::new();
    for line in lines {
        if let Some((k, v)) = line.split_once(':') {
            headers.insert(k.trim().to_ascii_lowercase(), v.trim().to_string());
        }
    }

    let want = headers
        .get("content-length")
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(0)
        .min(MAX_BODY);

    let already = buf.len().saturating_sub(head_end + 4);
    let mut body = buf[head_end + 4..].to_vec();
    while body.len() < want {
        let n = stream.read(&mut chunk).ok()?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..n]);
    }
    body.truncate(want);

    Some(Request {
        method,
        path,
        query,
        headers,
        body,
    })
}

fn find_head_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n")
}

pub fn write_response(stream: &mut TcpStream, res: &Response) -> std::io::Result<()> {
    let reason = match res.status {
        200 => "OK",
        204 => "No Content",
        400 => "Bad Request",
        401 => "Unauthorized",
        404 => "Not Found",
        405 => "Method Not Allowed",
        500 => "Internal Server Error",
        _ => "OK",
    };
    let mut head = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\n",
        res.status,
        reason,
        res.content_type,
        res.body.len()
    );
    for (k, v) in &res.headers {
        head.push_str(k);
        head.push_str(": ");
        head.push_str(v);
        head.push_str("\r\n");
    }
    head.push_str("\r\n");
    stream.write_all(head.as_bytes())?;
    if !res.body.is_empty() {
        stream.write_all(&res.body)?;
    }
    stream.flush()
}

/// SSE：头部发出去以后按行推，客户端断开时写入会失败，循环随之结束。
pub fn start_sse(stream: &mut TcpStream) -> std::io::Result<()> {
    stream.write_all(
        b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream; charset=utf-8\r\nCache-Control: no-store\r\nConnection: close\r\nX-Accel-Buffering: no\r\n\r\n",
    )?;
    stream.flush()
}
