//! The small helper the browser itself starts for the Ezoo Download add-in
//! ("native messaging"). The add-in talks to it through the browser, never
//! over the network, so nothing the browser does about web pages reaching
//! this computer (and no port another program or Windows keeps) can stop it.
//!
//! Each message of the add-in is one request for the program; this helper
//! hands it on to the program on this computer (signed with the program's
//! key, read from the program's own files) and gives the answer back. When
//! the program is closed and the user asked for a download, it opens it.
//!
//! Messages: 4 bytes of length (this computer's order) and then JSON, both ways.
//! A console program, as the browsers expect (they start it out of sight).

use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// The browser takes at most 1 MB from a helper in one message.
const MAX_OUT: usize = 1000 * 1024;
/// ...and gives at most this much (Chrome allows 64 MB).
const MAX_IN: usize = 64 << 20;

fn main() {
    let place = Arc::new(Place::find());
    let out = Arc::new(Mutex::new(std::io::stdout()));
    let mut input = std::io::stdin().lock();
    // the browser closes our input when the add-in lets go: then we end
    while let Some(msg) = read_message(&mut input) {
        let (out, place) = (out.clone(), place.clone());
        // a video look-up takes seconds: the other requests don't wait for it
        std::thread::spawn(move || {
            let answer = answer(&place, &msg);
            send(&out, &answer);
        });
    }
}

fn read_message(input: &mut impl Read) -> Option<Value> {
    let mut len = [0u8; 4];
    input.read_exact(&mut len).ok()?;
    let len = u32::from_ne_bytes(len) as usize;
    if len > MAX_IN {
        return None;
    }
    let mut buf = vec![0u8; len];
    input.read_exact(&mut buf).ok()?;
    Some(serde_json::from_slice(&buf).unwrap_or(Value::Null))
}

fn send(out: &Mutex<std::io::Stdout>, v: &Value) {
    let data = v.to_string();
    let mut o = out.lock().unwrap_or_else(|e| e.into_inner());
    let _ = o.write_all(&(data.len() as u32).to_ne_bytes());
    let _ = o.write_all(data.as_bytes());
    let _ = o.flush();
}

/// Where the program and its settings are (the program writes app.json next
/// to this helper when it puts it there).
struct Place {
    app: Option<PathBuf>,
    settings: Option<PathBuf>,
}

impl Place {
    fn find() -> Place {
        let here = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf));
        let told = here
            .as_ref()
            .and_then(|d| std::fs::read(d.join("app.json")).ok())
            .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
            .unwrap_or(Value::Null);
        let path = |k: &str| told[k].as_str().filter(|s| !s.is_empty()).map(PathBuf::from);
        let settings = path("settings").or_else(|| {
            std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join(r"Ezoo\EzooDownload\data\settings.json"))
        });
        Place { app: path("app"), settings }
    }

    /// The program's own port and key, as it wrote them last.
    fn link(&self) -> Option<(u16, String)> {
        let b = std::fs::read(self.settings.as_ref()?).ok()?;
        let v: Value = serde_json::from_slice(&b).ok()?;
        let port = v["api_port"].as_u64().filter(|p| *p > 0 && *p <= 65535)? as u16;
        let token = open_key(v["api_token"].as_str()?)?;
        Some((port, token))
    }
}

/// The program keeps its key encrypted for this Windows user ("dpapi:<hex>");
/// this helper runs as the same user, so it can read it (no other can).
fn open_key(stored: &str) -> Option<String> {
    let key = match stored.strip_prefix("dpapi:") {
        Some(hex) => String::from_utf8(unprotect(&from_hex(hex)?)?).ok()?,
        None => stored.to_string(),
    };
    (key.len() >= 32).then_some(key)
}

fn from_hex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok()).collect()
}

#[cfg(windows)]
fn unprotect(data: &[u8]) -> Option<Vec<u8>> {
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB};
    let input = CRYPT_INTEGER_BLOB { cbData: u32::try_from(data.len()).ok()?, pbData: data.as_ptr() as *mut u8 };
    let mut out = CRYPT_INTEGER_BLOB { cbData: 0, pbData: std::ptr::null_mut() };
    // SAFETY: `input` points at `data`, only read during the call; on success
    // Windows allocates `out`, which is copied and freed once
    unsafe {
        let ok = CryptUnprotectData(&input, std::ptr::null_mut(), std::ptr::null(), std::ptr::null(), std::ptr::null(), CRYPTPROTECT_UI_FORBIDDEN, &mut out);
        if ok == 0 || out.pbData.is_null() {
            return None;
        }
        let bytes = std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec();
        LocalFree(out.pbData as _);
        Some(bytes)
    }
}

#[cfg(not(windows))]
fn unprotect(_data: &[u8]) -> Option<Vec<u8>> {
    None
}

fn answer(place: &Place, msg: &Value) -> Value {
    let id = msg["id"].clone();
    let method = msg["method"].as_str().unwrap_or("GET").to_ascii_uppercase();
    let path = msg["path"].as_str().unwrap_or("");
    if path == "/hello" {
        return json!({ "id": id, "hello": true, "version": env!("CARGO_PKG_VERSION") });
    }
    if !path.starts_with('/') || !path.bytes().all(|b| b.is_ascii_graphic()) || !matches!(method.as_str(), "GET" | "POST") {
        return json!({ "id": id, "error": "bad request" });
    }
    let ms = msg["ms"].as_u64().unwrap_or(1500).clamp(300, 130_000);
    let body = msg["body"].as_str().unwrap_or("");
    let headers: Vec<(String, String)> = msg["headers"]
        .as_object()
        .map(|o| o.iter().filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_string()))).collect())
        .unwrap_or_default();
    let start = msg["start"].as_bool().unwrap_or(false);

    let mut tried_start = false;
    loop {
        let outcome = match place.link() {
            Some((port, token)) => call(port, &token, &method, path, &headers, body, Duration::from_millis(ms)),
            None => Err(Failure::Down),
        };
        match outcome {
            Ok((status, text)) if text.len() <= MAX_OUT => return json!({ "id": id, "status": status, "body": text }),
            Ok(_) => return json!({ "id": id, "error": "big" }),
            Err(Failure::Down) if start && !tried_start => {
                tried_start = true;
                if !open_program(place) {
                    return json!({ "id": id, "error": "down" });
                }
            }
            Err(Failure::Down) => return json!({ "id": id, "error": "down" }),
            Err(Failure::Slow) => return json!({ "id": id, "error": "timeout" }),
            Err(Failure::Other(e)) => return json!({ "id": id, "error": e }),
        }
    }
}

/// Opens Ezoo Download (out of sight: the window of the download comes) and
/// waits until it answers.
fn open_program(place: &Place) -> bool {
    // two requests at once: the program is opened once
    static OPENING: Mutex<()> = Mutex::new(());
    let _one = OPENING.lock().unwrap_or_else(|e| e.into_inner());
    let answers = || place.link().is_some_and(|(port, token)| call(port, &token, "GET", "/ping", &[], "", Duration::from_millis(800)).is_ok());
    if answers() {
        return true;
    }
    let Some(app) = place.app.as_ref().filter(|a| a.is_file()) else { return false };
    if !spawn_program(app) {
        return false;
    }
    let until = Instant::now() + Duration::from_secs(25);
    while Instant::now() < until {
        std::thread::sleep(Duration::from_millis(300));
        if answers() {
            return true;
        }
    }
    false
}

/// The program on its own: the browser ends its helpers (and, on Windows,
/// what they started inside the browser's job), never the program.
fn spawn_program(app: &Path) -> bool {
    let make = || {
        let mut cmd = std::process::Command::new(app);
        cmd.arg("--from-browser")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        cmd
    };
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;
        let flags = DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP;
        // out of the browser's job when it lets us; else as it allows
        if make().creation_flags(flags | CREATE_BREAKAWAY_FROM_JOB).spawn().is_ok() {
            return true;
        }
        make().creation_flags(flags).spawn().is_ok()
    }
    #[cfg(not(windows))]
    {
        make().spawn().is_ok()
    }
}

#[derive(Debug)]
enum Failure {
    /// nothing answers: the program is closed
    Down,
    /// it did not answer in time
    Slow,
    Other(String),
}

/// One request to the program on this computer.
fn call(port: u16, token: &str, method: &str, path: &str, headers: &[(String, String)], body: &str, wait: Duration) -> Result<(u16, String), Failure> {
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    let mut s = TcpStream::connect_timeout(&addr, Duration::from_millis(1500)).map_err(|_| Failure::Down)?;
    let _ = s.set_read_timeout(Some(wait));
    let _ = s.set_write_timeout(Some(Duration::from_secs(10)));
    let sign_path = path.split(['?', '#']).next().unwrap_or(path);
    // X-Ezoo-Helper: the program's link test can say the helper works
    let mut head = format!("{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\nX-SwiftGet: 1\r\nX-Ezoo-Helper: 1\r\n");
    for (k, v) in headers {
        let name_ok = !k.is_empty() && k.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-');
        let ours = ["host", "connection", "content-length", "transfer-encoding", "origin", "x-swiftget", "x-ezoo-time", "x-ezoo-nonce", "x-ezoo-sig"];
        if name_ok && !ours.contains(&k.to_ascii_lowercase().as_str()) && !v.contains(['\r', '\n']) {
            head.push_str(&format!("{k}: {v}\r\n"));
        }
    }
    head.push_str(&sign(token, method, sign_path));
    head.push_str(&format!("Content-Length: {}\r\n\r\n", body.len()));
    s.write_all(head.as_bytes()).and_then(|_| s.write_all(body.as_bytes())).map_err(|e| Failure::Other(e.to_string()))?;
    let mut raw = Vec::new();
    if let Err(e) = s.read_to_end(&mut raw) {
        if raw.is_empty() {
            return Err(if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) { Failure::Slow } else { Failure::Other(e.to_string()) });
        }
    }
    parse_response(&raw).ok_or_else(|| Failure::Other("bad answer".into()))
}

fn parse_response(raw: &[u8]) -> Option<(u16, String)> {
    let split = raw.windows(4).position(|w| w == b"\r\n\r\n")?;
    let head = std::str::from_utf8(&raw[..split]).ok()?;
    let mut rest = &raw[split + 4..];
    let status: u16 = head.split_whitespace().nth(1)?.parse().ok()?;
    let lower = head.to_ascii_lowercase();
    let mut body = Vec::new();
    if lower.contains("transfer-encoding: chunked") {
        loop {
            let line_end = rest.windows(2).position(|w| w == b"\r\n")?;
            let size = usize::from_str_radix(std::str::from_utf8(&rest[..line_end]).ok()?.split(';').next()?.trim(), 16).ok()?;
            rest = &rest[line_end + 2..];
            if size == 0 {
                break;
            }
            body.extend_from_slice(rest.get(..size)?);
            rest = rest.get(size + 2..).unwrap_or(&[]);
        }
    } else if let Some(len) = lower.lines().find_map(|l| l.strip_prefix("content-length:")).and_then(|v| v.trim().parse::<usize>().ok()) {
        body.extend_from_slice(rest.get(..len.min(rest.len()))?);
    } else {
        body.extend_from_slice(rest);
    }
    Some((status, String::from_utf8_lossy(&body).to_string()))
}

/// The headers that sign a request with the program's key (as the add-in does).
fn sign(token: &str, method: &str, path: &str) -> String {
    use ring::rand::SecureRandom;
    let time = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0).to_string();
    let mut n = [0u8; 16];
    if ring::rand::SystemRandom::new().fill(&mut n).is_err() {
        let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
        n.copy_from_slice(&(t ^ u128::from(std::process::id())).to_le_bytes());
    }
    let nonce = hex(&n);
    let key = ring::hmac::Key::new(ring::hmac::HMAC_SHA256, token.as_bytes());
    let sig = hex(ring::hmac::sign(&key, format!("{method}\n{path}\n{time}\n{nonce}").as_bytes()).as_ref());
    format!("X-Ezoo-Time: {time}\r\nX-Ezoo-Nonce: {nonce}\r\nX-Ezoo-Sig: {sig}\r\n")
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn answers_are_read() {
        let plain = b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 11\r\n\r\n{\"ok\":true}";
        assert_eq!(parse_response(plain), Some((200, "{\"ok\":true}".to_string())));
        let chunked = b"HTTP/1.1 401 Unauthorized\r\nTransfer-Encoding: chunked\r\n\r\n4\r\n{\"a\"\r\n3\r\n:1}\r\n0\r\n\r\n";
        assert_eq!(parse_response(chunked), Some((401, "{\"a\":1}".to_string())));
    }

    #[test]
    fn the_key_is_read_as_the_program_keeps_it() {
        assert_eq!(open_key(&"k".repeat(64)).as_deref(), Some("k".repeat(64).as_str()));
        assert_eq!(open_key("short"), None);
        // encrypted for another user (or not at all on this system): no key
        assert_eq!(open_key("dpapi:0102"), None);
        assert_eq!(open_key("dpapi:zz"), None);
    }

    #[test]
    fn messages_go_both_ways() {
        let v = json!({ "id": 3, "path": "/hello" });
        let data = v.to_string();
        let mut buf = (data.len() as u32).to_ne_bytes().to_vec();
        buf.extend_from_slice(data.as_bytes());
        let got = read_message(&mut buf.as_slice()).unwrap();
        assert_eq!(got["id"], 3);
        let a = answer(&Place { app: None, settings: None }, &got);
        assert_eq!(a["hello"], true);
        // nothing to talk to: "down" (not opened, nothing was asked to start)
        let a = answer(&Place { app: None, settings: None }, &json!({ "id": 4, "path": "/ping" }));
        assert_eq!(a["error"], "down");
    }
}
