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

/// The folder changed in "Download File Info" while it was already
/// downloading (in the first folder), then stopped and resumed: the file in
/// the new folder must be the whole file, not a nearly empty one.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn folder_changed_while_downloading() {
    let data = test_data(3 * 1024 * 1024 + 7);
    let port = serve(data.clone()).await;
    let url = format!("http://127.0.0.1:{port}/files/test.bin");
    let root = std::env::temp_dir().join(format!("ezoo-retarget-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let (first, second) = (root.join("first"), root.join("second"));
    std::fs::create_dir_all(&first).unwrap();
    let mgr = swiftget_engine::Manager::new(tokio::runtime::Handle::current(), root.join("app")).unwrap();
    let id = mgr.add(swiftget_engine::NewDownload { url, save_dir: Some(first.clone()), start: true, ..Default::default() });
    // part of it is there
    for _ in 0..400 {
        if mgr.get(id).map(|v| v.downloaded > 256 * 1024).unwrap_or(false) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    mgr.pause(id);
    tokio::time::sleep(Duration::from_millis(300)).await;
    let name = mgr.get(id).unwrap().file_name;
    mgr.retarget(id, &second, &name, None, None).unwrap();
    mgr.start(id);
    let mut status = swiftget_engine::Status::Queued;
    for _ in 0..1500 {
        status = mgr.get(id).unwrap().status;
        if matches!(status, swiftget_engine::Status::Completed | swiftget_engine::Status::Failed) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(status, swiftget_engine::Status::Completed, "{:?}", mgr.get(id).unwrap().error);
    let v = mgr.get(id).unwrap();
    assert!(v.save_path.starts_with(&second), "{:?}", v.save_path);
    let got = std::fs::read(&v.save_path).unwrap();
    assert_eq!(got.len(), data.len());
    assert!(got == *data, "the file in the new folder is not the downloaded file");
    mgr.remove(id, false);
    let _ = std::fs::remove_dir_all(&root);
}

/// The folder changed in "Download File Info" while it was already
/// downloading (in the first folder), then stopped and resumed: the file in
/// the new folder must be the whole file, not a nearly empty one.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn folder_changed_while_running() {
    let data = test_data(3 * 1024 * 1024 + 7);
    let port = serve(data.clone()).await;
    let url = format!("http://127.0.0.1:{port}/files/test.bin");
    let root = std::env::temp_dir().join(format!("ezoo-retarget2-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let (first, second) = (root.join("first"), root.join("second"));
    std::fs::create_dir_all(&first).unwrap();
    let mgr = swiftget_engine::Manager::new(tokio::runtime::Handle::current(), root.join("app")).unwrap();
    let id = mgr.add(swiftget_engine::NewDownload { url, save_dir: Some(first.clone()), start: true, ..Default::default() });
    // part of it is there
    for _ in 0..400 {
        if mgr.get(id).map(|v| v.downloaded > 256 * 1024).unwrap_or(false) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let name = mgr.get(id).unwrap().file_name;
    mgr.retarget(id, &second, &name, None, None).unwrap();
    let mut status = swiftget_engine::Status::Queued;
    for _ in 0..1500 {
        status = mgr.get(id).unwrap().status;
        if matches!(status, swiftget_engine::Status::Completed | swiftget_engine::Status::Failed) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(status, swiftget_engine::Status::Completed, "{:?}", mgr.get(id).unwrap().error);
    let v = mgr.get(id).unwrap();
    assert!(v.save_path.starts_with(&second), "{:?}", v.save_path);
    let got = std::fs::read(&v.save_path).unwrap();
    assert_eq!(got.len(), data.len());
    assert!(got == *data, "the file in the new folder is not the downloaded file");
    mgr.remove(id, false);
    let _ = std::fs::remove_dir_all(&root);
}

/// A web server with a few files by path (Range supported, a bit slow).
async fn serve_files(files: Vec<(&'static str, Arc<Vec<u8>>)>) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let files = Arc::new(files);
    tokio::spawn(async move {
        loop {
            let (sock, _) = listener.accept().await.unwrap();
            let files = files.clone();
            tokio::spawn(async move {
                let (r, mut w) = sock.into_split();
                let mut r = BufReader::new(r);
                let mut range: Option<(usize, Option<usize>)> = None;
                let mut path = String::new();
                loop {
                    let mut line = String::new();
                    if r.read_line(&mut line).await.unwrap_or(0) == 0 || line.trim().is_empty() {
                        break;
                    }
                    if path.is_empty() {
                        path = line.split_whitespace().nth(1).unwrap_or("").to_string();
                    }
                    let l = line.to_ascii_lowercase();
                    if let Some(v) = l.strip_prefix("range: bytes=") {
                        let (a, b) = v.trim().split_once('-').unwrap();
                        range = Some((a.parse().unwrap(), b.parse().ok()));
                    }
                }
                let Some((_, data)) = files.iter().find(|(p, _)| *p == path) else {
                    let _ = w.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await;
                    return;
                };
                let (a, b, head) = match range {
                    Some((a, b)) => {
                        let b = b.unwrap_or(data.len() - 1).min(data.len() - 1);
                        (a, b, format!("HTTP/1.1 206 Partial Content\r\nContent-Range: bytes {a}-{b}/{}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", data.len(), b - a + 1))
                    }
                    None => (0, data.len() - 1, format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", data.len())),
                };
                if w.write_all(head.as_bytes()).await.is_err() {
                    return;
                }
                for chunk in data[a..=b].chunks(16384) {
                    if w.write_all(chunk).await.is_err() {
                        return;
                    }
                    tokio::time::sleep(Duration::from_millis(15)).await;
                }
            });
        }
    });
    port
}

/// Picture and sound in two files (a video site): the folder changed while
/// it downloads. The joined file in the new folder must have both.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn video_and_sound_folder_changed() {
    let (Ok(v), Ok(a)) = (std::fs::read(std::env::var("EZOO_TEST_VIDEO").unwrap_or_default()), std::fs::read(std::env::var("EZOO_TEST_AUDIO").unwrap_or_default())) else {
        return; // test media not given
    };
    let (vlen, alen) = (v.len(), a.len());
    let port = serve_files(vec![("/v.mp4", Arc::new(v)), ("/a.m4a", Arc::new(a))]).await;
    let root = std::env::temp_dir().join(format!("ezoo-retarget-av-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let (first, second) = (root.join("first"), root.join("second"));
    std::fs::create_dir_all(&first).unwrap();
    let mgr = swiftget_engine::Manager::new(tokio::runtime::Handle::current(), root.join("app")).unwrap();
    let id = mgr.add(swiftget_engine::NewDownload {
        url: format!("http://127.0.0.1:{port}/v.mp4"),
        audio_url: Some(format!("http://127.0.0.1:{port}/a.m4a")),
        file_name: Some("clip.mp4".into()),
        save_dir: Some(first.clone()),
        early: true,
        start: true,
        ..Default::default()
    });
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_ne!(mgr.get(id).unwrap().status, swiftget_engine::Status::Completed, "too fast for the test");
    mgr.pause(id);
    tokio::time::sleep(Duration::from_millis(300)).await;
    mgr.retarget(id, &second, "my clip.mp4", None, None).unwrap();
    mgr.start(id);
    let mut status = swiftget_engine::Status::Queued;
    for _ in 0..1500 {
        status = mgr.get(id).unwrap().status;
        if matches!(status, swiftget_engine::Status::Completed | swiftget_engine::Status::Failed) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(status, swiftget_engine::Status::Completed, "{:?}", mgr.get(id).unwrap().error);
    let saved = mgr.get(id).unwrap().save_path;
    assert!(saved.starts_with(&second), "{saved:?}");
    let size = std::fs::metadata(&saved).unwrap().len() as usize;
    assert!(size > vlen && size > alen, "joined file is {size} bytes (video {vlen}, sound {alen})");
    mgr.remove(id, false);
    let _ = std::fs::remove_dir_all(&root);
}

/// Options > connections changed while a file downloads: fewer, then more,
/// at once and without a restart; the file is still byte for byte right.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn connections_changed_while_downloading() {
    let data = test_data(6 * 1024 * 1024 + 77);
    let port = serve(data.clone()).await;
    let url = format!("http://127.0.0.1:{port}/files/conns.bin");
    let shared = Arc::new(Shared::new(build_client().unwrap(), SpeedLimiter::new(0)));
    let p = probe(&shared.client(), &url, &RequestHeaders::default(), "test").await.unwrap();
    let dir = std::env::temp_dir().join(format!("swiftget-conns-{port}"));
    std::fs::create_dir_all(&dir).unwrap();
    let part = dir.join("out.sgpart");
    let _ = std::fs::remove_file(&part);
    let job = HttpJob {
        url,
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
    let p2 = progress.clone();
    let total = data.len() as u64;
    let changer = tokio::spawn(async move {
        let mut fewest = u32::MAX;
        let mut most = 0;
        // 8 -> 2 at a fifth of the file
        while p2.bytes.load(Ordering::Relaxed) < total / 5 {
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
        p2.want_connections.store(2, Ordering::Relaxed);
        for _ in 0..300 {
            tokio::time::sleep(Duration::from_millis(5)).await;
            fewest = fewest.min(p2.connections.load(Ordering::Relaxed));
            if p2.bytes.load(Ordering::Relaxed) > total / 2 {
                break;
            }
        }
        // then 16
        p2.want_connections.store(16, Ordering::Relaxed);
        for _ in 0..600 {
            tokio::time::sleep(Duration::from_millis(5)).await;
            most = most.max(p2.connections.load(Ordering::Relaxed));
        }
        (fewest, most)
    });
    http_download::run(shared, job, segs, progress.clone(), CancellationToken::new()).await.unwrap();
    let (fewest, most) = changer.await.unwrap();
    assert!(fewest <= 2, "fewer connections at once: {fewest}");
    assert!(most > 8, "more connections at once: {most}");
    assert_eq!(std::fs::read(&part).unwrap(), *data, "the file is right");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A link that expires: "/get" sends the browser on to "/file?t=N"; after
/// about a third of the file every old token answers 403. The download asks
/// "/get" again, checks that the new link is the same file and goes on.
async fn serve_expiring(data: Arc<Vec<u8>>, wrong_size_once: bool) -> u16 {
    use std::sync::atomic::{AtomicU64, AtomicUsize};
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let token = Arc::new(AtomicU64::new(1));
    let sent = Arc::new(AtomicUsize::new(0));
    let wrong = Arc::new(std::sync::atomic::AtomicBool::new(wrong_size_once));
    tokio::spawn(async move {
        loop {
            let (sock, _) = listener.accept().await.unwrap();
            let (data, token, sent, wrong) = (data.clone(), token.clone(), sent.clone(), wrong.clone());
            tokio::spawn(async move {
                let (r, mut w) = sock.into_split();
                let mut r = BufReader::new(r);
                let (mut path, mut range) = (String::new(), None::<(usize, usize)>);
                loop {
                    let mut line = String::new();
                    if r.read_line(&mut line).await.unwrap_or(0) == 0 || line.trim().is_empty() {
                        break;
                    }
                    if path.is_empty() {
                        path = line.split_whitespace().nth(1).unwrap_or("").to_string();
                    }
                    if let Some(v) = line.to_ascii_lowercase().strip_prefix("range: bytes=") {
                        let (a, b) = v.trim().split_once('-').unwrap();
                        let a: usize = a.parse().unwrap();
                        let b: usize = if b.is_empty() { data.len() - 1 } else { b.parse().unwrap() };
                        range = Some((a, b.min(data.len() - 1)));
                    }
                }
                if path == "/get" {
                    // a fresh link each time it is asked
                    let t = token.load(Ordering::SeqCst);
                    let head = format!("HTTP/1.1 302 Found\r\nLocation: /file?t={t}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                    let _ = w.write_all(head.as_bytes()).await;
                    return;
                }
                let t: u64 = path.rsplit('=').next().and_then(|x| x.parse().ok()).unwrap_or(0);
                if t != token.load(Ordering::SeqCst) {
                    let _ = w.write_all(b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await;
                    return;
                }
                // the first new link once gives another (smaller) file
                let len = if t == 2 && wrong.swap(false, Ordering::SeqCst) { data.len() / 2 } else { data.len() };
                let (a, b) = range.unwrap_or((0, len - 1));
                let b = b.min(len - 1);
                let head = format!(
                    "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes {a}-{b}/{len}\r\nContent-Length: {}\r\nContent-Disposition: attachment; filename=\"exp.bin\"\r\nConnection: close\r\n\r\n",
                    b - a + 1
                );
                if w.write_all(head.as_bytes()).await.is_err() {
                    return;
                }
                for chunk in data[a..=b].chunks(8192) {
                    if t != token.load(Ordering::SeqCst) {
                        return; // expired while sending: the connection drops
                    }
                    if w.write_all(chunk).await.is_err() {
                        return;
                    }
                    // expire once, after a third of the file
                    if sent.fetch_add(chunk.len(), Ordering::SeqCst) + chunk.len() > data.len() / 3 && token.load(Ordering::SeqCst) == 1 {
                        token.store(2, Ordering::SeqCst);
                    }
                    tokio::time::sleep(Duration::from_millis(2)).await;
                }
            });
        }
    });
    port
}

async fn run_expiring(wrong_size_once: bool) {
    let data = test_data(3 * 1024 * 1024 + 11);
    let port = serve_expiring(data.clone(), wrong_size_once).await;
    let root = std::env::temp_dir().join(format!("ezoo-expire-{}-{port}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let dir = root.join("out");
    std::fs::create_dir_all(&dir).unwrap();
    let mgr = swiftget_engine::Manager::new(tokio::runtime::Handle::current(), root.join("app")).unwrap();
    let id = mgr.add(swiftget_engine::NewDownload {
        url: format!("http://127.0.0.1:{port}/get"),
        save_dir: Some(dir.clone()),
        start: true,
        ..Default::default()
    });
    let mut status = swiftget_engine::Status::Queued;
    for _ in 0..3000 {
        status = mgr.get(id).unwrap().status;
        if matches!(status, swiftget_engine::Status::Completed | swiftget_engine::Status::Failed) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(status, swiftget_engine::Status::Completed, "{:?}", mgr.get(id).unwrap().error);
    let v = mgr.get(id).unwrap();
    assert!(std::fs::read(&v.save_path).unwrap() == *data, "the file is right after the new link");
    mgr.remove(id, false);
    let _ = std::fs::remove_dir_all(&root);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn expired_link_is_renewed() {
    run_expiring(false).await;
}

/// The first new link gives another file: nothing of it is written, the
/// next one (the right file) is used.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn wrong_new_link_is_not_used() {
    run_expiring(true).await;
}

/// A busy server: after a third of the file every request gets "503" for a
/// while, and many answers break off half way. The download must finish by
/// itself (no "Resume" click), and the file must be right.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn busy_server_needs_no_resume_click() {
    use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize};
    let data = test_data(4 * 1024 * 1024 + 5);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let sent = Arc::new(AtomicUsize::new(0));
    // when the busy time ends (0 = not busy yet)
    let busy_until = Arc::new(AtomicU64::new(0));
    let was_busy = Arc::new(AtomicBool::new(false));
    let served = Arc::new(AtomicUsize::new(0));
    {
        let (data, sent, busy_until, was_busy, served) = (data.clone(), sent.clone(), busy_until.clone(), was_busy.clone(), served.clone());
        tokio::spawn(async move {
            let now = || std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as u64;
            loop {
                let (sock, _) = listener.accept().await.unwrap();
                let (data, sent, busy_until, was_busy, served) = (data.clone(), sent.clone(), busy_until.clone(), was_busy.clone(), served.clone());
                tokio::spawn(async move {
                    let (r, mut w) = sock.into_split();
                    let mut r = BufReader::new(r);
                    let mut range: Option<(usize, usize)> = None;
                    loop {
                        let mut line = String::new();
                        if r.read_line(&mut line).await.unwrap_or(0) == 0 || line.trim().is_empty() {
                            break;
                        }
                        if let Some(v) = line.to_ascii_lowercase().strip_prefix("range: bytes=") {
                            let (a, b) = v.trim().split_once('-').unwrap();
                            let a: usize = a.parse().unwrap();
                            let b: usize = if b.is_empty() { data.len() - 1 } else { b.parse().unwrap() };
                            range = Some((a, b.min(data.len() - 1)));
                        }
                    }
                    let until = busy_until.load(Ordering::SeqCst);
                    if until != 0 && now() < until {
                        let _ = w.write_all(b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await;
                        return;
                    }
                    let n = served.fetch_add(1, Ordering::SeqCst);
                    let (a, b) = range.unwrap_or((0, data.len() - 1));
                    let head = format!(
                        "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes {a}-{b}/{}\r\nContent-Length: {}\r\nContent-Disposition: attachment; filename=\"busy.bin\"\r\nConnection: close\r\n\r\n",
                        data.len(),
                        b - a + 1
                    );
                    if w.write_all(head.as_bytes()).await.is_err() {
                        return;
                    }
                    let body = &data[a..=b];
                    // every third answer breaks off half way
                    let cut = if n % 3 == 2 { body.len() / 2 } else { body.len() };
                    for chunk in body[..cut].chunks(8192) {
                        if busy_until.load(Ordering::SeqCst) > now() {
                            return; // busy now: the line drops
                        }
                        if w.write_all(chunk).await.is_err() {
                            return;
                        }
                        if sent.fetch_add(chunk.len(), Ordering::SeqCst) + chunk.len() > data.len() / 3 && !was_busy.swap(true, Ordering::SeqCst) {
                            busy_until.store(now() + 20_000, Ordering::SeqCst);
                        }
                        tokio::time::sleep(Duration::from_millis(1)).await;
                    }
                });
            }
        });
    }
    let root = std::env::temp_dir().join(format!("ezoo-busy-{}-{port}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let dir = root.join("out");
    std::fs::create_dir_all(&dir).unwrap();
    let mgr = swiftget_engine::Manager::new(tokio::runtime::Handle::current(), root.join("app")).unwrap();
    let id = mgr.add(swiftget_engine::NewDownload {
        url: format!("http://127.0.0.1:{port}/busy.bin"),
        save_dir: Some(dir.clone()),
        start: true,
        ..Default::default()
    });
    let mut status = swiftget_engine::Status::Queued;
    let mut saw_failed = false;
    for _ in 0..6000 {
        status = mgr.get(id).unwrap().status;
        if status == swiftget_engine::Status::Failed {
            saw_failed = true;
        }
        if status == swiftget_engine::Status::Completed {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(was_busy.load(Ordering::SeqCst), "the server was busy for a while");
    assert!(!saw_failed, "it stopped and waited for a click: {:?}", mgr.get(id).unwrap().error);
    assert_eq!(status, swiftget_engine::Status::Completed, "{:?}", mgr.get(id).unwrap().error);
    let v = mgr.get(id).unwrap();
    assert!(std::fs::read(&v.save_path).unwrap() == *data, "the file is right");
    mgr.remove(id, false);
    let _ = std::fs::remove_dir_all(&root);
}
