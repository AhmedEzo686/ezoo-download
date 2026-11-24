//! Making sure a finished file is the one the site meant: its checksum (a
//! hash the user pasted, or one the server sent with the file) is worked out
//! again from the bytes on the disk and compared.

use std::io::Read;
use std::path::Path;

/// The kinds of hash a checksum can be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Algo {
    Md5,
    Sha1,
    Sha256,
    Sha384,
    Sha512,
}

impl Algo {
    pub fn name(self) -> &'static str {
        match self {
            Algo::Md5 => "MD5",
            Algo::Sha1 => "SHA-1",
            Algo::Sha256 => "SHA-256",
            Algo::Sha384 => "SHA-384",
            Algo::Sha512 => "SHA-512",
        }
    }
    fn hex_len(self) -> usize {
        match self {
            Algo::Md5 => 32,
            Algo::Sha1 => 40,
            Algo::Sha256 => 64,
            Algo::Sha384 => 96,
            Algo::Sha512 => 128,
        }
    }
    fn from_name(n: &str) -> Option<Algo> {
        let n: String = n.trim().to_ascii_lowercase().chars().filter(|c| c.is_ascii_alphanumeric()).collect();
        Some(match n.as_str() {
            "md5" => Algo::Md5,
            "sha1" => Algo::Sha1,
            "sha256" => Algo::Sha256,
            "sha384" => Algo::Sha384,
            "sha512" => Algo::Sha512,
            _ => return None,
        })
    }
    fn from_len(n: usize) -> Option<Algo> {
        [Algo::Md5, Algo::Sha1, Algo::Sha256, Algo::Sha384, Algo::Sha512].into_iter().find(|a| a.hex_len() == n)
    }
}

/// A checksum to compare with: its kind and its value (lower-case hex).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Expected {
    pub algo: Algo,
    pub hex: String,
}

impl Expected {
    /// What the user pasted: "sha256:ab12...", "SHA-256 ab12...", "ab12..."
    /// (the kind told by its length), or a line of a checksum file
    /// ("ab12...  file.zip"). None when it is no checksum.
    pub fn parse(text: &str) -> Option<Expected> {
        let text = text.trim();
        if text.is_empty() {
            return None;
        }
        let (named, rest) = match text.split_once([':', '=', ' ']) {
            Some((a, b)) if Algo::from_name(a).is_some() => (Algo::from_name(a), b.trim()),
            _ => (None, text),
        };
        let hex: String = rest.split_whitespace().next().unwrap_or("").trim_start_matches('*').to_ascii_lowercase();
        if hex.is_empty() || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
        let algo = named.or_else(|| Algo::from_len(hex.len()))?;
        (hex.len() == algo.hex_len()).then_some(Expected { algo, hex })
    }

    /// As it is kept: "sha256:ab12...".
    pub fn text(&self) -> String {
        let name = match self.algo {
            Algo::Md5 => "md5",
            Algo::Sha1 => "sha1",
            Algo::Sha256 => "sha256",
            Algo::Sha384 => "sha384",
            Algo::Sha512 => "sha512",
        };
        format!("{name}:{}", self.hex)
    }

    /// The checksum a server sends with the whole file: "Repr-Digest" /
    /// "Digest" (any answer), "x-goog-hash" (md5), "Content-MD5" (only with
    /// a whole answer: with a part it is the part's).
    pub fn from_headers(h: &reqwest::header::HeaderMap, whole: bool) -> Option<Expected> {
        let get = |n: &str| h.get(n).and_then(|v| v.to_str().ok());
        for name in ["repr-digest", "digest"] {
            if let Some(v) = get(name) {
                // "sha-256=:base64:, sha-512=:base64:" or "SHA-256=base64"
                let mut best: Option<Expected> = None;
                for item in v.split(',') {
                    let Some((a, b)) = item.split_once('=') else { continue };
                    let Some(algo) = Algo::from_name(a) else { continue };
                    let Some(raw) = base64(b.trim().trim_matches(':')) else { continue };
                    let e = Expected { algo, hex: to_hex(&raw) };
                    if e.hex.len() == algo.hex_len() && best.as_ref().is_none_or(|x| x.algo.hex_len() < e.algo.hex_len()) {
                        best = Some(e);
                    }
                }
                if best.is_some() {
                    return best;
                }
            }
        }
        if let Some(v) = get("x-goog-hash") {
            for item in v.split(',') {
                if let Some(b) = item.trim().strip_prefix("md5=") {
                    if let Some(raw) = base64(b.trim()).filter(|r| r.len() == 16) {
                        return Some(Expected { algo: Algo::Md5, hex: to_hex(&raw) });
                    }
                }
            }
        }
        if whole {
            if let Some(raw) = get("content-md5").and_then(|v| base64(v.trim())).filter(|r| r.len() == 16) {
                return Some(Expected { algo: Algo::Md5, hex: to_hex(&raw) });
            }
        }
        None
    }
}

fn to_hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// Base64 (standard or for addresses) to bytes.
pub fn base64(s: &str) -> Option<Vec<u8>> {
    let val = |c: u8| -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            _ => return None,
        } as u32)
    };
    let s = s.trim_end_matches('=');
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0);
    for c in s.bytes() {
        acc = (acc << 6) | val(c)?;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}

/// Works out the hash of `path` and compares it. `at` is told how far it
/// is (0..1). Ok(true): the same.
pub fn matches(path: &Path, want: &Expected, at: &dyn Fn(f64), stop: &dyn Fn() -> bool) -> std::io::Result<bool> {
    let mut f = std::fs::File::open(path)?;
    let size = f.metadata()?.len().max(1);
    let mut buf = vec![0u8; 1 << 20];
    let mut done = 0u64;
    let mut md5 = Md5::new();
    let mut ctx = match want.algo {
        Algo::Md5 => None,
        Algo::Sha1 => Some(ring::digest::Context::new(&ring::digest::SHA1_FOR_LEGACY_USE_ONLY)),
        Algo::Sha256 => Some(ring::digest::Context::new(&ring::digest::SHA256)),
        Algo::Sha384 => Some(ring::digest::Context::new(&ring::digest::SHA384)),
        Algo::Sha512 => Some(ring::digest::Context::new(&ring::digest::SHA512)),
    };
    loop {
        if stop() {
            return Err(std::io::Error::other("stopped"));
        }
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        match ctx.as_mut() {
            Some(c) => c.update(&buf[..n]),
            None => md5.update(&buf[..n]),
        }
        done += n as u64;
        at(done as f64 / size as f64);
    }
    let got = match ctx {
        Some(c) => to_hex(c.finish().as_ref()),
        None => to_hex(&md5.finish()),
    };
    Ok(got == want.hex)
}

/// MD5 (RFC 1321): still what many sites publish for their files.
struct Md5 {
    state: [u32; 4],
    len: u64,
    buf: Vec<u8>,
}

impl Md5 {
    fn new() -> Md5 {
        Md5 { state: [0x67452301, 0xefcdab89, 0x98badcfe, 0x10325476], len: 0, buf: Vec::with_capacity(64) }
    }

    fn update(&mut self, mut data: &[u8]) {
        self.len += data.len() as u64;
        if !self.buf.is_empty() {
            let take = (64 - self.buf.len()).min(data.len());
            self.buf.extend_from_slice(&data[..take]);
            data = &data[take..];
            if self.buf.len() == 64 {
                let block: [u8; 64] = self.buf[..].try_into().unwrap_or([0; 64]);
                self.block(&block);
                self.buf.clear();
            }
        }
        let mut chunks = data.chunks_exact(64);
        for c in &mut chunks {
            let block: [u8; 64] = c.try_into().unwrap_or([0; 64]);
            self.block(&block);
        }
        self.buf.extend_from_slice(chunks.remainder());
    }

    fn finish(mut self) -> [u8; 16] {
        let bits = self.len.wrapping_mul(8);
        let mut pad = vec![0x80u8];
        while (self.buf.len() + pad.len()) % 64 != 56 {
            pad.push(0);
        }
        pad.extend_from_slice(&bits.to_le_bytes());
        let len = self.len;
        self.update(&pad);
        self.len = len;
        let mut out = [0u8; 16];
        for (i, w) in self.state.iter().enumerate() {
            out[i * 4..i * 4 + 4].copy_from_slice(&w.to_le_bytes());
        }
        out
    }

    fn block(&mut self, b: &[u8; 64]) {
        const S: [u32; 64] = [
            7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9, 14, 20, 4, 11, 16, 23,
            4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
        ];
        let k = |i: usize| ((i as f64 + 1.0).sin().abs() * 4294967296.0) as u32;
        let m: Vec<u32> = (0..16).map(|i| u32::from_le_bytes([b[i * 4], b[i * 4 + 1], b[i * 4 + 2], b[i * 4 + 3]])).collect();
        let [mut a, mut bb, mut c, mut d] = self.state;
        for (i, shift) in S.iter().enumerate() {
            let (f, g) = match i / 16 {
                0 => ((bb & c) | (!bb & d), i),
                1 => ((d & bb) | (!d & c), (5 * i + 1) % 16),
                2 => (bb ^ c ^ d, (3 * i + 5) % 16),
                _ => (c ^ (bb | !d), (7 * i) % 16),
            };
            let f = f.wrapping_add(a).wrapping_add(k(i)).wrapping_add(m[g]);
            a = d;
            d = c;
            c = bb;
            bb = bb.wrapping_add(f.rotate_left(*shift));
        }
        self.state[0] = self.state[0].wrapping_add(a);
        self.state[1] = self.state[1].wrapping_add(bb);
        self.state[2] = self.state[2].wrapping_add(c);
        self.state[3] = self.state[3].wrapping_add(d);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn md5_hex(data: &[u8]) -> String {
        let mut m = Md5::new();
        m.update(data);
        to_hex(&m.finish())
    }

    #[test]
    fn md5_is_right() {
        assert_eq!(md5_hex(b""), "d41d8cd98f00b204e9800998ecf8427e");
        assert_eq!(md5_hex(b"abc"), "900150983cd24fb0d6963f7d28e17f72");
        assert_eq!(md5_hex(b"The quick brown fox jumps over the lazy dog"), "9e107d9d372bb6826bd81d3542a419d6");
        // in pieces of every size: the same
        let long: Vec<u8> = (0..10_000u32).map(|i| (i % 253) as u8).collect();
        let whole = md5_hex(&long);
        for step in [1, 7, 63, 64, 65, 1000] {
            let mut m = Md5::new();
            for c in long.chunks(step) {
                m.update(c);
            }
            assert_eq!(to_hex(&m.finish()), whole);
        }
    }

    #[test]
    fn what_users_paste() {
        let sha = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
        assert_eq!(Expected::parse(sha).unwrap().algo, Algo::Sha256);
        assert_eq!(Expected::parse(&format!("SHA-256: {}", sha.to_uppercase())).unwrap().hex, sha);
        assert_eq!(Expected::parse(&format!("{sha} *ubuntu.iso")).unwrap().algo, Algo::Sha256);
        assert_eq!(Expected::parse("md5=d41d8cd98f00b204e9800998ecf8427e").unwrap().algo, Algo::Md5);
        assert!(Expected::parse("sha1:d41d8cd98f00b204e9800998ecf8427e").is_none(), "an MD5 called SHA-1");
        assert!(Expected::parse("hello").is_none());
        assert!(Expected::parse("").is_none());
    }

    #[test]
    fn what_servers_send() {
        let mut h = reqwest::header::HeaderMap::new();
        h.insert("repr-digest", "sha-256=:47DEQpj8HBSa+/TImW+5JCeuQeRkm5NMpJWZG3hSuFU=:".parse().unwrap());
        let e = Expected::from_headers(&h, false).unwrap();
        assert_eq!(e.text(), "sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
        let mut h = reqwest::header::HeaderMap::new();
        h.insert("content-md5", "1B2M2Y8AsgTpgAmY7PhCfg==".parse().unwrap());
        assert!(Expected::from_headers(&h, false).is_none(), "a part's own");
        assert_eq!(Expected::from_headers(&h, true).unwrap().hex, "d41d8cd98f00b204e9800998ecf8427e");
        let mut h = reqwest::header::HeaderMap::new();
        h.insert("x-goog-hash", "crc32c=AAAAAA==, md5=1B2M2Y8AsgTpgAmY7PhCfg==".parse().unwrap());
        assert_eq!(Expected::from_headers(&h, false).unwrap().algo, Algo::Md5);
    }

    #[test]
    fn a_file_is_checked() {
        let p = std::env::temp_dir().join(format!("ezoo-sum-{}", std::process::id()));
        std::fs::write(&p, b"abc").unwrap();
        let right = Expected::parse("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad").unwrap();
        assert!(matches(&p, &right, &|_| {}, &|| false).unwrap());
        let md5 = Expected::parse("900150983cd24fb0d6963f7d28e17f72").unwrap();
        assert!(matches(&p, &md5, &|_| {}, &|| false).unwrap());
        let wrong = Expected::parse("0000000000000000000000000000000000000000").unwrap();
        assert!(!matches(&p, &wrong, &|_| {}, &|| false).unwrap());
        let _ = std::fs::remove_file(&p);
    }
}
