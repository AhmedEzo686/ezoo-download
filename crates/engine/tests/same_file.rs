//! A file must come whole and from one version: a file changed on the server
//! during a pause, servers that name each copy their own way, links that
//! work for one request only, pages that lead on to the file, and a site's
//! "not a robot" check.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use swiftget_engine::http::{self, build_client_with, probe, ProxyConfig, Progress, Shared};
use swiftget_engine::http_download::{self, FileCheck, HttpJob};
use swiftget_engine::limiter::SpeedLimiter;
use swiftget_engine::RequestHeaders;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

/// What the test server was asked.
#[derive(Debug, Clone, Default)]
struct Asked {
    path: String,
    range: Option<(usize, Option<usize>)>,
    if_range: Option<String>,
    cookie: String,
}

/// What it answers: status line and headers (without length), body.
type Answer = (String, Vec<String>, Vec<u8>);

/// A server whose answers `handler` makes, sent slowly enough to pause in.
async fn serve(handler: Arc<dyn Fn(&Asked) -> Answer + Send + Sync>) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        while let Ok((sock, _)) = listener.accept().await {
            let handler = handler.clone();
            tokio::spawn(async move {
                let (r, mut w) = sock.into_split();
                let mut r = BufReader::new(r);
                let mut asked = Asked::default();
                let mut first = true;
                loop {
                    let mut line = String::new();
                    if r.read_line(&mut line).await.unwrap_or(0) == 0 || line.trim().is_empty() {
                        break;
                    }
                    if first {
                        asked.path = line.split_whitespace().nth(1).unwrap_or("/").to_string();
                        first = false;
                        continue;
                    }
                    let l = line.trim_end().to_string();
                    let lower = l.to_ascii_lowercase();
                    if let Some(v) = lower.strip_prefix("range: bytes=") {
                        let (a, b) = v.split_once('-').unwrap();
                        asked.range = Some((a.parse().unwrap(), b.parse().ok()));
                    } else if lower.starts_with("if-range:") {
                        asked.if_range = Some(l[9..].trim().to_string());
                    } else if lower.starts_with("cookie:") {
                        asked.cookie = l[7..].trim().to_string();
                    }
                }
                let (status, headers, body) = handler(&asked);
                let mut head = format!("HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n", body.len());
                for h in headers {
                    head.push_str(&h);
                    head.push_str("\r\n");
                }
                head.push_str("\r\n");
                if w.write_all(head.as_bytes()).await.is_err() {
                    return;
                }
                for chunk in body.chunks(16 * 1024) {
                    if w.write_all(chunk).await.is_err() {
                        return;
                    }
                    tokio::time::sleep(Duration::from_millis(2)).await;
                }
            });
        }
    });
    port
}

/// `data` the way a server with ranges sends it (`etag`: its version name).
fn ranged(data: &[u8], asked: &Asked, etag: Option<&str>) -> Answer {
    let mut headers = vec!["Accept-Ranges: bytes".to_string()];
    if let Some(e) = etag {
        headers.push(format!("ETag: {e}"));
    }
    // "If-Range" of another version: the whole (new) file
    let stale = matches!((&asked.if_range, etag), (Some(theirs), Some(ours)) if theirs != ours);
    match asked.range {
        Some((a, b)) if !stale && a < data.len() => {
            let b = b.unwrap_or(data.len() - 1).min(data.len() - 1);
            headers.push(format!("Content-Range: bytes {a}-{b}/{}", data.len()));
            ("206 Partial Content".into(), headers, data[a..=b].to_vec())
        }
        _ => ("200 OK".into(), headers, data.to_vec()),
    }
}

fn bytes(len: usize, seed: usize) -> Vec<u8> {
    (0..len).map(|i| ((i * 31 + seed * 7) % 251) as u8).collect()
}

fn job(url: &str, size: usize, part: &std::path::Path, connections: u32, check: FileCheck) -> HttpJob {
    HttpJob {
        url: url.to_string(),
        headers: RequestHeaders::default(),
        user_agent: "test".into(),
        total_size: Some(size as u64),
        resumable: true,
        part_path: part.to_path_buf(),
        connections,
        min_split: 64 * 1024,
        retries: 2,
        progress_base: 0,
        max_request: None,
        limiter: Arc::new(SpeedLimiter::new(0)),
        check,
    }
}

fn shared() -> Arc<Shared> {
    Arc::new(Shared::new(build_client_with(&ProxyConfig::default()).unwrap(), SpeedLimiter::new(0)))
}

fn temp(name: &str, port: u16) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("ezoo-same-{name}-{port}"));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join("f.part")
}

/// Run `j` until a quarter is there, then stop (a pause).
async fn run_a_quarter(j: &HttpJob, segs: &http_download::SegTable, size: usize) {
    let progress = Arc::new(Progress::default());
    let cancel = CancellationToken::new();
    let (c2, p2) = (cancel.clone(), progress.clone());
    tokio::spawn(async move {
        while p2.bytes.load(Ordering::Relaxed) < size as u64 / 4 {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        c2.cancel();
    });
    assert!(http_download::run(shared(), j.clone(), segs.clone(), progress, cancel).await.is_err());
}

/// The file changed on the server during a pause (same size, other bytes,
/// no version name): the bytes before each part are compared, and the
/// download stops instead of mixing the two.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_file_changed_without_a_version_name_is_caught() {
    let size = 2 * 1024 * 1024;
    let current = Arc::new(Mutex::new(bytes(size, 1)));
    let c = current.clone();
    let port = serve(Arc::new(move |a: &Asked| ranged(&c.lock().unwrap(), a, None))).await;
    let url = format!("http://127.0.0.1:{port}/f.bin");
    let part = temp("plain", port);
    let j = job(&url, size, &part, 4, FileCheck::default());
    let segs: http_download::SegTable = Arc::new(Mutex::new(Vec::new()));
    run_a_quarter(&j, &segs, size).await;
    *current.lock().unwrap() = bytes(size, 2);
    let e = http_download::run(shared(), j, segs, Arc::new(Progress::default()), CancellationToken::new()).await.unwrap_err();
    assert!(format!("{e:#}").contains("changed since"), "{e:#}");
    let _ = std::fs::remove_dir_all(part.parent().unwrap());
}

/// The same with a version name: the server sends the whole new file to
/// "If-Range" of the old one, and the download stops.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_new_version_is_caught_by_its_name() {
    let size = 2 * 1024 * 1024;
    let current = Arc::new(Mutex::new((bytes(size, 1), "\"v1\"")));
    let c = current.clone();
    let port = serve(Arc::new(move |a: &Asked| {
        let (d, e) = c.lock().unwrap().clone();
        ranged(&d, a, Some(e))
    }))
    .await;
    let url = format!("http://127.0.0.1:{port}/f.bin");
    let part = temp("named", port);
    let j = job(&url, size, &part, 4, FileCheck::new(Some("\"v1\"".into()), None));
    let segs: http_download::SegTable = Arc::new(Mutex::new(Vec::new()));
    run_a_quarter(&j, &segs, size).await;
    *current.lock().unwrap() = (bytes(size + 1000, 2), "\"v2\"");
    let e = http_download::run(shared(), j, segs, Arc::new(Progress::default()), CancellationToken::new()).await.unwrap_err();
    assert!(format!("{e:#}").contains("changed since"), "{e:#}");
    let _ = std::fs::remove_dir_all(part.parent().unwrap());
}

/// Servers of one site that name the same file each their own way: the name
/// is let go, and the file comes whole and right.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn each_server_naming_the_file_its_own_way_is_fine() {
    let size = 2 * 1024 * 1024;
    let data = Arc::new(bytes(size, 3));
    let n = Arc::new(AtomicU32::new(0));
    let (d, n2) = (data.clone(), n.clone());
    let port = serve(Arc::new(move |a: &Asked| {
        let tag = format!("\"node{}\"", n2.fetch_add(1, Ordering::Relaxed));
        ranged(&d, a, Some(&tag))
    }))
    .await;
    let url = format!("http://127.0.0.1:{port}/f.bin");
    let part = temp("nodes", port);
    let j = job(&url, size, &part, 4, FileCheck::new(Some("\"node0\"".into()), None));
    let segs: http_download::SegTable = Arc::new(Mutex::new(Vec::new()));
    run_a_quarter(&j, &segs, size).await;
    http_download::run(shared(), j, segs, Arc::new(Progress::default()), CancellationToken::new()).await.unwrap();
    assert!(std::fs::read(&part).unwrap() == *data);
    let _ = std::fs::remove_dir_all(part.parent().unwrap());
}

/// A link that works for one request only: the look keeps its answer, a
/// second look asks nothing, and the download reads the file from it.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_link_for_one_request_is_not_spent_by_the_look() {
    let size = 600 * 1024;
    let data = Arc::new(bytes(size, 4));
    let asked = Arc::new(AtomicU32::new(0));
    let (d, a2) = (data.clone(), asked.clone());
    let port = serve(Arc::new(move |a: &Asked| {
        if a2.fetch_add(1, Ordering::Relaxed) == 0 {
            ranged(&d, a, None)
        } else {
            ("403 Forbidden".into(), vec![], vec![])
        }
    }))
    .await;
    let url = format!("http://127.0.0.1:{port}/once.bin");
    let s = shared();
    let p = probe(&s.client(), &url, &RequestHeaders::default(), "test").await.unwrap();
    assert_eq!(p.total_size, Some(size as u64));
    // the window looks again: the kept answer says it
    let again = probe(&s.client(), &url, &RequestHeaders::default(), "test").await.unwrap();
    assert_eq!(again.total_size, Some(size as u64));
    assert_eq!(asked.load(Ordering::Relaxed), 1, "asked once only");
    let (_, answer) = http::take_kept(&url).expect("kept");
    let part = temp("once", port);
    let j = job(&url, size, &part, 1, FileCheck::new(None, Some(answer)));
    http_download::run(s, j, Arc::new(Mutex::new(Vec::new())), Arc::new(Progress::default()), CancellationToken::new()).await.unwrap();
    assert!(std::fs::read(&part).unwrap() == *data);
    assert_eq!(asked.load(Ordering::Relaxed), 1, "the download asked nothing more");
    let _ = std::fs::remove_dir_all(part.parent().unwrap());
}

/// "Your download starts in a moment": a page that leads on to the file
/// (in a meta tag, or the "Refresh" header) with a cookie it sets.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn pages_that_lead_on_to_the_file_are_followed() {
    let data = Arc::new(bytes(100 * 1024, 5));
    let d = data.clone();
    let port = serve(Arc::new(move |a: &Asked| match a.path.as_str() {
        "/wait" => (
            "200 OK".into(),
            vec!["Content-Type: text/html; charset=utf-8".into(), "Set-Cookie: ticket=42; Path=/".into()],
            b"<html><head><META HTTP-EQUIV='Refresh' CONTENT='1; URL=/second?a=1&amp;b=2'></head><body>wait</body></html>".to_vec(),
        ),
        "/second?a=1&b=2" => ("200 OK".into(), vec!["Content-Type: text/html".into(), "Refresh: 0; url=/file.zip".into()], b"<html></html>".to_vec()),
        "/file.zip" if a.cookie.contains("ticket=42") => ranged(&d, a, None),
        _ => ("403 Forbidden".into(), vec![], vec![]),
    }))
    .await;
    let url = format!("http://127.0.0.1:{port}/wait");
    let p = probe(&shared().client(), &url, &RequestHeaders::default(), "test").await.unwrap();
    assert!(p.final_url.ends_with("/file.zip"), "{}", p.final_url);
    assert_eq!(p.total_size, Some(data.len() as u64));
    assert!(p.headers_used.unwrap().cookies.unwrap().contains("ticket=42"));
    http::drop_kept(&url);
}

/// A site's "not a robot" check: told as such, not asked again and again.
#[tokio::test]
async fn a_robot_check_is_told() {
    let asked = Arc::new(AtomicU32::new(0));
    let a2 = asked.clone();
    let port = serve(Arc::new(move |_: &Asked| {
        a2.fetch_add(1, Ordering::Relaxed);
        ("403 Forbidden".into(), vec!["cf-mitigated: challenge".into(), "Content-Type: text/html".into()], b"<html>check</html>".to_vec())
    }))
    .await;
    let e = probe(&shared().client(), &format!("http://127.0.0.1:{port}/f.zip"), &RequestHeaders::default(), "test").await.unwrap_err();
    assert!(http::wants_captcha(&e), "{e:#}");
    assert_eq!(asked.load(Ordering::Relaxed), 1);
}

#[test]
fn refresh_values() {
    let base = url::Url::parse("https://a.test/dir/page?x=1").unwrap();
    let (w, u) = http::refresh_target("5; url=next.html", &base).unwrap();
    assert_eq!((w.as_secs(), u.as_str()), (5, "https://a.test/dir/next.html"));
    let (w, u) = http::refresh_target("0;URL='https://b.test/f.zip'", &base).unwrap();
    assert_eq!((w.as_secs(), u.as_str()), (0, "https://b.test/f.zip"));
    // a long wait is cut short; a page reloading itself leads nowhere
    assert_eq!(http::refresh_target("90, /f", &base).unwrap().0.as_secs(), 10);
    assert!(http::refresh_target("3", &base).is_none());
    assert!(http::refresh_target("3; url=page?x=1", &base).is_none());
    assert!(http::refresh_target("1; url=javascript:go()", &base).is_none());
    let page = r#"<meta charset="utf-8"><meta content="2;url=/go" http-equiv="refresh">"#;
    assert_eq!(http::meta_refresh(page, &base).unwrap().1.as_str(), "https://a.test/go");
}

async fn wait_end(mgr: &Arc<swiftget_engine::Manager>, id: u64) -> swiftget_engine::DownloadView {
    for _ in 0..1500 {
        let v = mgr.get(id).unwrap();
        if matches!(v.status, swiftget_engine::Status::Completed | swiftget_engine::Status::Failed) {
            return v;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("did not end");
}

/// A checksum the user pasted is checked before the file is done: right, it
/// is done; wrong, the download says the file is damaged. A file of the same
/// name that is already there is never replaced.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_checksum_is_checked_and_nothing_is_replaced() {
    let data = Arc::new(b"abc".repeat(100_000));
    let d = data.clone();
    let port = serve(Arc::new(move |a: &Asked| ranged(&d, a, None))).await;
    let root = std::env::temp_dir().join(format!("ezoo-sum-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let dir = root.join("out");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("f.bin"), b"mine").unwrap();
    let mgr = swiftget_engine::Manager::new(tokio::runtime::Handle::current(), root.join("app")).unwrap();
    let sha = {
        let mut c = ring::digest::Context::new(&ring::digest::SHA256);
        c.update(&data);
        c.finish().as_ref().iter().map(|b| format!("{b:02x}")).collect::<String>()
    };
    let add = |checksum: &str| swiftget_engine::NewDownload {
        url: format!("http://127.0.0.1:{port}/f.bin"),
        save_dir: Some(dir.clone()),
        file_name: Some("f.bin".into()),
        start: true,
        checksum: Some(checksum.to_string()),
        ..Default::default()
    };
    let good = wait_end(&mgr, mgr.add(add(&format!("SHA-256: {sha}")))).await;
    assert_eq!(good.status, swiftget_engine::Status::Completed, "{:?}", good.error);
    assert!(std::fs::read(&good.save_path).unwrap() == *data);
    assert_eq!(std::fs::read(dir.join("f.bin")).unwrap(), b"mine", "the file that was there stays");
    let bad = wait_end(&mgr, mgr.add(add("900150983cd24fb0d6963f7d28e17f72"))).await;
    assert_eq!(bad.status, swiftget_engine::Status::Failed);
    assert!(bad.error.unwrap_or_default().contains("damaged"));
    let _ = std::fs::remove_dir_all(&root);
}
