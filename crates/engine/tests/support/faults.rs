//! A web server for tests that goes wrong on purpose, the ways real servers
//! do: slow, cut off half way, no ranges, a part other than the one asked
//! for, a file that changes, a limit of connections, "busy, come back later",
//! a "not a robot" check. Used by the engine's tests and by
//! `cargo run --example fault_server` (to try the program against it).

#![allow(dead_code)]

use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;

/// What goes wrong (all of them can be on at once).
#[derive(Debug, Clone, Default)]
pub struct Faults {
    /// bytes per second for every answer (0 = as fast as it goes)
    pub rate: u64,
    /// every answer stops after this many bytes (the connection is cut)
    pub cut_after: Option<usize>,
    /// only the first this many answers are cut (then they go on whole)
    pub cuts: Option<u32>,
    /// ranges are ignored: always the whole file (200)
    pub no_ranges: bool,
    /// a range answer starts this many bytes too early (once)
    pub wrong_part_once: Option<usize>,
    /// more connections at once than this: 403 (as some file sites do)
    pub max_connections: Option<usize>,
    /// the first this many requests get "503 busy" with Retry-After: 1
    pub busy_first: u32,
    /// every request gets the "not a robot" check (403 cf-mitigated)
    pub robot_check: bool,
    /// a version name (ETag) for the file
    pub etag: Option<String>,
    /// after this many requests the file is another one (same size)
    pub changes_after: Option<u32>,
}

/// The test server: its port, what it was asked, and the file it serves.
pub struct FaultServer {
    pub port: u16,
    pub requests: Arc<AtomicU32>,
    pub data: Arc<Vec<u8>>,
    pub faults: Arc<Mutex<Faults>>,
}

impl FaultServer {
    pub fn url(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}/{}", self.port, path.trim_start_matches('/'))
    }
}

/// The bytes of the file (seed: another file).
pub fn file_data(len: usize, seed: usize) -> Vec<u8> {
    (0..len).map(|i| ((i * 31 + seed * 17) % 251) as u8).collect()
}

pub async fn start(data: Vec<u8>, faults: Faults) -> FaultServer {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("a port for the test server");
    let port = listener.local_addr().expect("its port").port();
    let data = Arc::new(data);
    let other = Arc::new(data.iter().map(|b| b.wrapping_add(1)).collect::<Vec<u8>>());
    let faults = Arc::new(Mutex::new(faults));
    let requests = Arc::new(AtomicU32::new(0));
    let open = Arc::new(AtomicUsize::new(0));
    let wrong_done = Arc::new(AtomicU32::new(0));
    let cuts_done = Arc::new(AtomicU32::new(0));
    let server = FaultServer { port, requests: requests.clone(), data: data.clone(), faults: faults.clone() };
    tokio::spawn(async move {
        while let Ok((sock, _)) = listener.accept().await {
            let (data, other, faults, requests, open, wrong_done, cuts_done) =
                (data.clone(), other.clone(), faults.clone(), requests.clone(), open.clone(), wrong_done.clone(), cuts_done.clone());
            tokio::spawn(async move {
                let (r, mut w) = sock.into_split();
                let mut r = BufReader::new(r);
                let mut range: Option<(usize, Option<usize>)> = None;
                let mut if_range: Option<String> = None;
                let mut first = true;
                loop {
                    let mut line = String::new();
                    if r.read_line(&mut line).await.unwrap_or(0) == 0 || line.trim().is_empty() {
                        break;
                    }
                    if first {
                        first = false;
                        continue;
                    }
                    let l = line.trim_end().to_ascii_lowercase();
                    if let Some(v) = l.strip_prefix("range: bytes=") {
                        if let Some((a, b)) = v.split_once('-') {
                            range = a.parse().ok().map(|a| (a, b.parse().ok()));
                        }
                    } else if l.starts_with("if-range:") {
                        if_range = Some(line.trim_end()[9..].trim().to_string());
                    }
                }
                let n = requests.fetch_add(1, Ordering::SeqCst) + 1;
                let f = faults.lock().map(|f| f.clone()).unwrap_or_default();
                let send = |status: &str, headers: Vec<String>, body: &[u8]| {
                    let mut head = format!("HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n", body.len());
                    for h in headers {
                        head.push_str(&h);
                        head.push_str("\r\n");
                    }
                    head.push_str("\r\n");
                    head
                };
                if f.robot_check {
                    let head = send("403 Forbidden", vec!["cf-mitigated: challenge".into(), "Content-Type: text/html".into()], b"check");
                    let _ = w.write_all(head.as_bytes()).await;
                    let _ = w.write_all(b"check").await;
                    return;
                }
                if n <= f.busy_first {
                    let head = send("503 Service Unavailable", vec!["Retry-After: 1".into()], b"");
                    let _ = w.write_all(head.as_bytes()).await;
                    return;
                }
                let now_open = open.fetch_add(1, Ordering::SeqCst) + 1;
                struct Out(Arc<AtomicUsize>);
                impl Drop for Out {
                    fn drop(&mut self) {
                        self.0.fetch_sub(1, Ordering::SeqCst);
                    }
                }
                let _out = Out(open.clone());
                if f.max_connections.is_some_and(|m| now_open > m) {
                    let head = send("403 Forbidden", vec![], b"");
                    let _ = w.write_all(head.as_bytes()).await;
                    return;
                }
                let changed = f.changes_after.is_some_and(|c| n > c);
                let body_of: &[u8] = if changed { &other } else { &data };
                let etag = f.etag.clone().map(|e| if changed { format!("{e}-2") } else { e });
                let mut headers = vec!["Accept-Ranges: bytes".to_string(), "Content-Type: application/octet-stream".into()];
                if let Some(e) = &etag {
                    headers.push(format!("ETag: {e}"));
                }
                let stale = matches!((&if_range, &etag), (Some(a), Some(b)) if a != b);
                let (status, slice) = match range {
                    Some((a, b)) if !f.no_ranges && !stale && a < body_of.len() => {
                        let b = b.unwrap_or(body_of.len() - 1).min(body_of.len() - 1);
                        // once: a part that starts somewhere else
                        let a2 = match f.wrong_part_once {
                            Some(off) if a >= off && wrong_done.fetch_add(1, Ordering::SeqCst) == 0 => a - off,
                            _ => a,
                        };
                        headers.push(format!("Content-Range: bytes {a2}-{b}/{}", body_of.len()));
                        ("206 Partial Content", &body_of[a2..=b])
                    }
                    _ => ("200 OK", &body_of[..]),
                };
                let head = send(status, headers, slice);
                if w.write_all(head.as_bytes()).await.is_err() {
                    return;
                }
                let cut_now = f.cut_after.filter(|_| f.cuts.is_none_or(|c| cuts_done.fetch_add(1, Ordering::SeqCst) < c));
                let mut sent = 0usize;
                for chunk in slice.chunks(16 * 1024) {
                    let chunk = match cut_now {
                        Some(cut) if sent + chunk.len() > cut => &chunk[..cut.saturating_sub(sent)],
                        _ => chunk,
                    };
                    if w.write_all(chunk).await.is_err() {
                        return;
                    }
                    sent += chunk.len();
                    if cut_now.is_some_and(|c| sent >= c) {
                        return; // cut off
                    }
                    if f.rate > 0 {
                        tokio::time::sleep(Duration::from_millis((chunk.len() as u64 * 1000 / f.rate).max(1))).await;
                    }
                }
            });
        }
    });
    server
}
