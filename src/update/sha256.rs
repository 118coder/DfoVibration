//! 纯 Rust SHA-256 —— 更新包完整性校验专用。
//!
//! 为什么自己写 (而不是引 `sha2`): 项目坚持"零新增运行时依赖", 而 SHA-256 只有
//! ~80 行常量表代码, 比引入一个 crate 加一棵依赖树划算得多; 也便于单元测试
//! 对着 FIPS 180-4 的标准向量逐条验证。
//!
//! 用途 (★R2.12 优化 10): 将来自动下载 exe/zip 时, 先对落盘文件算摘要,
//! 与清单里声明的 `sha256` 比对 —— 下载被中间人替换、传输损坏、磁盘写坏
//! 都能在"替换 exe 之前"拦住, 而不是等玩家双击时才发现程序起不来。

use std::io::Read;
use std::path::Path;

/// SHA-256 轮常量 (FIPS 180-4 §4.2.2): 前 64 个素数立方根小数部分前 32 位。
const K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

const H0: [u32; 8] = [
    0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
];

/// 增量式 SHA-256 (支持大文件流式读取, 不必整个读进内存)。
pub struct Sha256 {
    state: [u32; 8],
    buf: [u8; 64],
    buf_len: usize,
    total_len: u64,
}

impl Default for Sha256 {
    fn default() -> Self {
        Self::new()
    }
}

impl Sha256 {
    pub fn new() -> Self {
        Self {
            state: H0,
            buf: [0u8; 64],
            buf_len: 0,
            total_len: 0,
        }
    }

    pub fn update(&mut self, mut data: &[u8]) {
        self.total_len = self.total_len.wrapping_add(data.len() as u64);
        // 先填满上次剩下的半块
        if self.buf_len > 0 {
            let need = 64 - self.buf_len;
            let take = need.min(data.len());
            self.buf[self.buf_len..self.buf_len + take].copy_from_slice(&data[..take]);
            self.buf_len += take;
            data = &data[take..];
            if self.buf_len == 64 {
                let block = self.buf;
                self.compress(&block);
                self.buf_len = 0;
            }
        }
        // 整块直接压缩
        while data.len() >= 64 {
            let mut block = [0u8; 64];
            block.copy_from_slice(&data[..64]);
            self.compress(&block);
            data = &data[64..];
        }
        // 余数留到下次
        if !data.is_empty() {
            self.buf[..data.len()].copy_from_slice(data);
            self.buf_len = data.len();
        }
    }

    /// 收尾并输出 32 字节摘要。调用后 self 不应继续使用。
    pub fn finish(mut self) -> [u8; 32] {
        let bit_len = self.total_len.wrapping_mul(8);
        // padding: 0x80 + 若干 0 + 64 位大端长度
        self.update(&[0x80]);
        while self.buf_len != 56 {
            self.update(&[0x00]);
        }
        self.update(&bit_len.to_be_bytes());
        let mut out = [0u8; 32];
        for (i, v) in self.state.iter().enumerate() {
            out[i * 4..i * 4 + 4].copy_from_slice(&v.to_be_bytes());
        }
        out
    }

    fn compress(&mut self, block: &[u8; 64]) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                block[i * 4],
                block[i * 4 + 1],
                block[i * 4 + 2],
                block[i * 4 + 3],
            ]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = self.state;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let t1 = h
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (s, v) in self.state.iter_mut().zip([a, b, c, d, e, f, g, h]) {
            *s = s.wrapping_add(v);
        }
    }
}

/// 一段数据的 SHA-256 (小写十六进制)。
pub fn sha256_hex(data: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(data);
    hex(&h.finish())
}

/// 文件的 SHA-256 (小写十六进制)。流式读取, 大文件不吃内存。
pub fn sha256_hex_file(path: &Path) -> std::io::Result<String> {
    let mut f = std::fs::File::open(path)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 256 * 1024];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(hex(&h.finish()))
}

fn hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(HEX[(b >> 4) as usize] as char);
        s.push(HEX[(b & 0x0f) as usize] as char);
    }
    s
}

/// 校验文件摘要。`expected` 允许带 `sha256:` 前缀、允许大写、允许前后空白。
/// 也接受 `SHA256SUMS.txt` 那种 `<hash>  <文件名>` 行 (只取第一段)。
pub fn verify_file(path: &Path, expected: &str) -> Result<(), String> {
    let want = normalize_expected(expected)
        .ok_or_else(|| format!("清单里的 sha256 不是 64 位十六进制: {expected}"))?;
    let got = sha256_hex_file(path).map_err(|e| format!("读取文件算校验和失败: {e}"))?;
    if got == want {
        Ok(())
    } else {
        Err(format!("SHA256 不匹配 (期望 {want} / 实际 {got})"))
    }
}

/// 从清单/校验和文件里提取 64 位十六进制摘要。
pub fn normalize_expected(raw: &str) -> Option<String> {
    let s = raw.trim();
    let s = s.strip_prefix("sha256:").unwrap_or(s);
    let first = s.split_whitespace().next().unwrap_or(s);
    let lower = first.trim().to_ascii_lowercase();
    (lower.len() == 64 && lower.chars().all(|c| c.is_ascii_hexdigit())).then_some(lower)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// FIPS 180-4 / NIST 标准测试向量 —— 段错误最容易藏在 padding 与轮函数里。
    #[test]
    fn matches_known_vectors() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            sha256_hex(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
    }

    /// 分块喂入必须与一次性喂入结果一致 (流式读文件的前提)。
    #[test]
    fn streaming_matches_oneshot() {
        let data: Vec<u8> = (0..1000u32).map(|i| (i % 251) as u8).collect();
        let one = sha256_hex(&data);
        for chunk in [1usize, 7, 63, 64, 65, 200] {
            let mut h = Sha256::new();
            for part in data.chunks(chunk) {
                h.update(part);
            }
            assert_eq!(hex(&h.finish()), one, "chunk={chunk} 时结果应与一次性一致");
        }
    }

    /// 清单里的写法五花八门 (大写/带前缀/SHA256SUMS 行), 归一化不能漏。
    #[test]
    fn normalizes_manifest_forms() {
        let want = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        assert_eq!(normalize_expected(want), Some(want.to_string()));
        assert_eq!(normalize_expected(&want.to_uppercase()), Some(want.to_string()));
        assert_eq!(
            normalize_expected(&format!("sha256:{want}")),
            Some(want.to_string())
        );
        assert_eq!(
            normalize_expected(&format!("{want}  DfoVibration.exe")),
            Some(want.to_string())
        );
        assert_eq!(normalize_expected("not-a-hash"), None);
    }

    /// 真实文件校验: 写一个临时文件, 摘要必须与内存计算一致, 改一个字节必须失败。
    #[test]
    fn verify_file_detects_corruption() {
        let dir = std::env::temp_dir().join("sorahk_sha256_test");
        let _ = std::fs::create_dir_all(&dir);
        let p = dir.join("probe.bin");
        std::fs::write(&p, b"hello sorahk").unwrap();
        let good = sha256_hex(b"hello sorahk");
        assert!(verify_file(&p, &good).is_ok());
        assert!(verify_file(&p, &good.to_uppercase()).is_ok(), "大写摘要也应通过");
        std::fs::write(&p, b"hello sorahK").unwrap();
        assert!(verify_file(&p, &good).is_err(), "改一个字节必须校验失败");
        let _ = std::fs::remove_file(&p);
        let _ = std::fs::remove_dir(&dir);
    }
}
