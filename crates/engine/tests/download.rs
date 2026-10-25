//! End-to-end: a local HTTP server with Range support, a slow region to force
//! dynamic splitting, then pause + resume. The result must be byte-identical.

use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use swiftget_engine::http::{build_client, probe, Progress, Shared};
use swiftget_engine::http_download::{self, HttpJob};
use swiftget_engine::limiter::SpeedLimiter;
use swiftget_engine::RequestHeaders;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

fn test_data(len: usize) -> Arc<Vec<u8>> {
    Arc::new((0..len).map(|i| (i * 31 % 251) as u8).collect())
}

async fn serve(data: Arc<Vec<u8>>) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        loop {
            let (sock, _) = listener.accept().await.unwrap();
            let data = data.clone();
            tokio::spawn(async move {
                let (r, mut w) = sock.into_split();
                let mut r = BufReader::new(r);
                let mut range: Option<(usize, usize)> = None;
                loop {
                    let mut line = String::new();
                    if r.read_line(&mut line).await.unwrap_or(0) == 0 || line.trim().is_empty() {
                        break;
                    }
                    let l = line.to_ascii_lowercase();
                    if let Some(v) = l.strip_prefix("range: bytes=") {
                        let (a, b) = v.trim().split_once('-').unwrap();
                        let a: usize = a.parse().unwrap();
                        let b: usize = if b.is_empty() { data.len() - 1 } else { b.parse().unwrap() };
                        range = Some((a, b.min(data.len() - 1)));
                    }
                }
                let (a, b, head) = match range {
                    Some((a, b)) => (a, b, format!(
                        "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes {a}-{b}/{}\r\nContent-Length: {}\r\nContent-Disposition: attachment; filename=\"test file.bin\"\r\nConnection: close\r\n\r\n",
                        data.len(), b - a + 1)),
                    None => (0, data.len() - 1, format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", data.len())),
                };
                if w.write_all(head.as_bytes()).await.is_err() {
                    return;
                }
                for chunk in data[a..=b].chunks(8192) {
                    if w.write_all(chunk).await.is_err() {
                        return;
                    }
                    // the first part of the file is slow -> other connections split it
                    if a < data.len() / 8 {
                        tokio::time::sleep(Duration::from_millis(5)).await;
                    } else {
                        tokio::time::sleep(Duration::from_millis(1)).await;
                    }
                }
            });
        }
    });
    port
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn segmented_download_with_pause_and_resume() {
    let data = test_data(4 * 1024 * 1024 + 123);
    let port = serve(data.clone()).await;
    let url = format!("http://127.0.0.1:{port}/files/test.bin");
    let shared = Arc::new(Shared::new(build_client().unwrap(), SpeedLimiter::new(0)));

    let p = probe(&shared.client(), &url, &RequestHeaders::default(), "test").await.unwrap();
    assert_eq!(p.total_size, Some(data.len() as u64));
    assert!(p.resumable);
    assert_eq!(p.file_name, "test file.bin");

    let dir = std::env::temp_dir().join(format!("swiftget-test-{port}"));
    std::fs::create_dir_all(&dir).unwrap();
    let part = dir.join("out.sgpart");
    let _ = std::fs::remove_file(&part);
    let job = HttpJob {
        url: url.clone(),
        headers: RequestHeaders::default(),
        user_agent: "test".into(),
        total_size: p.total_size,
        resumable: true,
        part_path: part.clone(),
        connections: 8,
        min_split: 64 * 1024,
        retries: 3,
        progress_base: 0,
        max_request: None,
        limiter: Arc::new(SpeedLimiter::new(0)),
    };
    let segs = Arc::new(Mutex::new(Vec::new()));
    let progress = Arc::new(Progress::default());

    // start, then pause once about a quarter is there
    let cancel = CancellationToken::new();
    let c2 = cancel.clone();
    let p2 = progress.clone();
    let quarter = data.len() as u64 / 4;
    tokio::spawn(async move {
        while p2.bytes.load(Ordering::Relaxed) < quarter {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        c2.cancel();
    });
    let first = http_download::run(shared.clone(), job.clone(), segs.clone(), progress.clone(), cancel).await;
    assert!(first.is_err(), "should have been paused");
    let after_pause = progress.bytes.load(Ordering::Relaxed);
    assert!(after_pause > 0 && after_pause < data.len() as u64);

    // resume with the saved segment table
    let saved = segs.lock().unwrap().clone();
    let segs = Arc::new(Mutex::new(saved));
    http_download::run(shared, job, segs.clone(), progress.clone(), CancellationToken::new())
        .await
        .unwrap();

    let out = std::fs::read(&part).unwrap();
    assert_eq!(out.len(), data.len());
    assert!(out == *data, "downloaded bytes differ from the source");
    // how far the file gets split depends on how fast the machine is
    assert!(!segs.lock().unwrap().is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

/// YouTube style: each connection may only ask for a small piece per request.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn chunked_requests_with_32_connections() {
    let data = test_data(3 * 1024 * 1024 + 7);
    let port = serve(data.clone()).await;
    let url = format!("http://127.0.0.1:{port}/v.bin");
    let shared = Arc::new(Shared::new(build_client().unwrap(), SpeedLimiter::new(0)));
    let dir = std::env::temp_dir().join(format!("swiftget-chunk-{port}"));
    std::fs::create_dir_all(&dir).unwrap();
    let part = dir.join("v.sgpart");
    let job = HttpJob {
        url,
        headers: RequestHeaders::default(),
        user_agent: "test".into(),
        total_size: Some(data.len() as u64),
        resumable: true,
        part_path: part.clone(),
        connections: 32,
        min_split: 32 * 1024,
        retries: 3,
        progress_base: 0,
        max_request: Some(40 * 1024),
        limiter: Arc::new(SpeedLimiter::new(0)),
    };
    let segs = Arc::new(Mutex::new(Vec::new()));
    let progress = Arc::new(Progress::default());
    http_download::run(shared, job, segs, progress.clone(), CancellationToken::new()).await.unwrap();
    let out = std::fs::read(&part).unwrap();
    assert!(out == *data, "chunked download differs from the source");
    assert_eq!(progress.bytes.load(Ordering::Relaxed), data.len() as u64);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Like `serve`, but while `down` is set every connection is dropped at once,
/// and the first answers break off half way: a flaky internet line.
async fn serve_flaky(data: Arc<Vec<u8>>, down: Arc<std::sync::atomic::AtomicBool>) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let served = Arc::new(std::sync::atomic::AtomicU32::new(0));
    tokio::spawn(async move {
        loop {
            let (sock, _) = listener.accept().await.unwrap();
            if down.load(Ordering::Relaxed) {
                drop(sock);
                continue;
            }
            let data = data.clone();
            let n = served.fetch_add(1, Ordering::Relaxed);
            tokio::spawn(async move {
                let (r, mut w) = sock.into_split();
                let mut r = BufReader::new(r);
                let mut range: Option<(usize, usize)> = None;
                loop {
                    let mut line = String::new();
                    if r.read_line(&mut line).await.unwrap_or(0) == 0 || line.trim().is_empty() {
                        break;
                    }
                    let l = line.to_ascii_lowercase();
                    if let Some(v) = l.strip_prefix("range: bytes=") {
                        let (a, b) = v.trim().split_once('-').unwrap();
                        let a: usize = a.parse().unwrap();
                        let b: usize = if b.is_empty() { data.len() - 1 } else { b.parse().unwrap() };
                        range = Some((a, b.min(data.len() - 1)));
                    }
                }
                let (a, b) = range.unwrap_or((0, data.len() - 1));
                let head = format!(
                    "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes {a}-{b}/{}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    data.len(),
                    b - a + 1
                );
                if w.write_all(head.as_bytes()).await.is_err() {
                    return;
                }
                let body = &data[a..=b];
                // the first requests lose the line after half of their part
                let cut = if n < 4 { body.len() / 2 } else { body.len() };
                for chunk in body[..cut].chunks(8192) {
                    if w.write_all(chunk).await.is_err() {
                        return;
                    }
                    tokio::time::sleep(Duration::from_millis(1)).await;
                }
            });
        }
    });
    port
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn waits_through_a_long_outage_and_resumes() {
    let data = test_data(2 * 1024 * 1024 + 99);
    let down = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let port = serve_flaky(data.clone(), down.clone()).await;
    let url = format!("http://127.0.0.1:{port}/f.bin");
    let shared = Arc::new(Shared::new(build_client().unwrap(), SpeedLimiter::new(0)));
    let dir = std::env::temp_dir().join(format!("swiftget-flaky-{port}"));
    std::fs::create_dir_all(&dir).unwrap();
    let part = dir.join("out.sgpart");
    let job = HttpJob {
        url,
        headers: RequestHeaders::default(),
        user_agent: "test".into(),
        total_size: Some(data.len() as u64),
        resumable: true,
        part_path: part.clone(),
        connections: 4,
        min_split: 64 * 1024,
        // far fewer than the failures below: a dropped line must not use them up
        retries: 1,
        progress_base: 0,
        max_request: None,
        limiter: Arc::new(SpeedLimiter::new(0)),
    };
    let segs = Arc::new(Mutex::new(Vec::new()));
    let progress = Arc::new(Progress::default());

    // the line goes down once a bit is there, for longer than the old retries
    let (d2, p2) = (down.clone(), progress.clone());
    let watcher = tokio::spawn(async move {
        while p2.bytes.load(Ordering::Relaxed) < 200_000 {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        d2.store(true, Ordering::Relaxed);
        let mut saw_waiting = false;
        for _ in 0..60 {
            tokio::time::sleep(Duration::from_millis(100)).await;
            saw_waiting |= p2.waiting.load(Ordering::Relaxed);
        }
        d2.store(false, Ordering::Relaxed);
        saw_waiting
    });
    http_download::run(shared, job, segs, progress.clone(), CancellationToken::new())
        .await
        .expect("should wait for the connection and finish");
    assert!(watcher.await.unwrap(), "should have shown 'waiting for connection'");
    assert!(!progress.waiting.load(Ordering::Relaxed));
    let out = std::fs::read(&part).unwrap();
    assert!(out == *data, "downloaded bytes differ from the source");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn network_errors_are_told_apart() {
    use swiftget_engine::http::{http_status_of, is_network_error, HttpStatus, NetDrop};
    let drop: anyhow::Error = NetDrop("connection closed early").into();
    assert!(is_network_error(&drop));
    assert!(is_network_error(&drop.context("part 3")));
    let reset: anyhow::Error = std::io::Error::from(std::io::ErrorKind::ConnectionReset).into();
    assert!(is_network_error(&reset));
    let full: anyhow::Error = std::io::Error::from(std::io::ErrorKind::StorageFull).into();
    assert!(!is_network_error(&full));
    let gone: anyhow::Error = HttpStatus(410).into();
    assert!(!is_network_error(&gone));
    assert_eq!(http_status_of(&gone.context("x")), Some(410));
}
