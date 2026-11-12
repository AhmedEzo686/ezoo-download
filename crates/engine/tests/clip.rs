//! "Download a part of a video": only the part is read from the site, and
//! the part is cut where asked (quick and exact). Needs a full ffmpeg on
//! the computer that runs the tests (skipped without one).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use swiftget_engine::clip::{self, Output};
use swiftget_engine::clipserve::{self, Source};
use swiftget_engine::http::{build_client, Progress, Shared};
use swiftget_engine::limiter::SpeedLimiter;
use swiftget_engine::RequestHeaders;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

fn ffmpeg() -> Option<PathBuf> {
    let ok = std::process::Command::new("ffmpeg").arg("-version").output().map(|o| o.status.success()).unwrap_or(false);
    ok.then(|| PathBuf::from("ffmpeg"))
}

/// A 2 minutes video (a whole picture every 2 s), its index at the end of the file.
fn make_video(dir: &Path) -> PathBuf {
    let out = dir.join("long.mp4");
    let ok = std::process::Command::new("ffmpeg")
        .args(["-hide_banner", "-loglevel", "error", "-y", "-f", "lavfi", "-i", "testsrc2=size=640x360:rate=25:duration=120"])
        .args(["-f", "lavfi", "-i", "sine=frequency=440:duration=120:sample_rate=48000"])
        .args(["-c:v", "libx264", "-g", "50", "-b:v", "1000k", "-c:a", "aac", "-shortest"])
        .arg(&out)
        .status()
        .unwrap();
    assert!(ok.success());
    out
}

/// A site that gives parts of one file (HTTP ranges) and counts what it sent.
async fn site(data: Arc<Vec<u8>>, sent: Arc<AtomicU64>) -> u16 {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = l.local_addr().unwrap().port();
    tokio::spawn(async move {
        loop {
            let (mut s, _) = l.accept().await.unwrap();
            let (data, sent) = (data.clone(), sent.clone());
            tokio::spawn(async move {
                let mut buf = vec![0u8; 8192];
                let mut got = Vec::new();
                loop {
                    let n = s.read(&mut buf).await.unwrap_or(0);
                    if n == 0 {
                        return;
                    }
                    got.extend_from_slice(&buf[..n]);
                    if got.windows(4).any(|w| w == b"\r\n\r\n") {
                        break;
                    }
                }
                let head = String::from_utf8_lossy(&got).to_string();
                let range = head.lines().find_map(|l| l.to_ascii_lowercase().strip_prefix("range: bytes=").map(str::to_string));
                let total = data.len() as u64;
                let (a, b) = match range {
                    Some(r) => {
                        let (a, b) = r.trim().split_once('-').unwrap();
                        let a: u64 = a.parse().unwrap();
                        let b: u64 = if b.is_empty() { total - 1 } else { b.parse::<u64>().unwrap().min(total - 1) };
                        (a, b)
                    }
                    None => (0, total - 1),
                };
                if std::env::var_os("SITE_LOG").is_some() { eprintln!("SITE range {a}-{b}"); }
                let body = &data[a as usize..=b as usize];
                let head = format!(
                    "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes {a}-{b}/{total}\r\nContent-Length: {}\r\nContent-Type: video/mp4\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                if s.write_all(head.as_bytes()).await.is_ok() && s.write_all(body).await.is_ok() {
                    sent.fetch_add(body.len() as u64, Ordering::Relaxed);
                }
            });
        }
    });
    port
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn only_the_part_is_read_and_cut_where_asked() {
    let Some(ff) = ffmpeg() else { return };
    let dir = std::env::temp_dir().join(format!("ezoo-clip-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let video = make_video(&dir);
    let data = Arc::new(std::fs::read(&video).unwrap());
    let sent = Arc::new(AtomicU64::new(0));
    let port = site(data.clone(), sent.clone()).await;

    let shared = Arc::new(Shared::new(build_client().unwrap(), SpeedLimiter::new(0)));
    let progress = Arc::new(Progress::default());
    let src = Source { url: format!("http://127.0.0.1:{port}/v.mp4"), headers: RequestHeaders::default(), user_agent: "test".into() };
    let ticket = clipserve::open(shared.clone(), src, progress.clone()).await.unwrap();

    // what the video is
    let (len, pic) = clip::look(&ff, &ticket.url).await.unwrap();
    assert!((len.unwrap() - 120.0).abs() < 0.2, "{len:?}");
    assert_eq!(pic.unwrap().height, 360);

    // the quick cut: 3 s from 21.0 s
    sent.store(0, Ordering::Relaxed);
    let quick = dir.join("quick.mp4");
    let cancel = CancellationToken::new();
    let last = Arc::new(std::sync::Mutex::new(0.0));
    let l2 = last.clone();
    let done = clip::cut(&ff, std::slice::from_ref(&ticket.url), 21.0, 3.0, false, &Output::Video, &quick, move |f| *l2.lock().unwrap() = f, &cancel)
        .await
        .unwrap();
    assert_eq!(done, clip::Done::AsAsked);
    let (qlen, _) = clip::look(&ff, quick.to_str().unwrap()).await.unwrap();
    let qlen = qlen.unwrap();
    // starts on the whole picture at 20 s: 3 s, or up to 2 s more
    assert!((2.9..=5.2).contains(&qlen), "quick part lasts {qlen}");
    let read = sent.load(Ordering::Relaxed);
    assert!(read < data.len() as u64 / 3, "only the part was read: {read} of {} bytes", data.len());
    assert!(*last.lock().unwrap() > 0.5, "progress was told");

    // the exact cut: exactly 3 s
    let exact = dir.join("exact.mp4");
    let done = clip::cut(&ff, std::slice::from_ref(&ticket.url), 21.0, 3.0, true, &Output::Video, &exact, |_| {}, &cancel).await.unwrap();
    assert_eq!(done, clip::Done::AsAsked);
    let (elen, _) = clip::look(&ff, exact.to_str().unwrap()).await.unwrap();
    assert!((elen.unwrap() - 3.0).abs() < 0.15, "exact part lasts {elen:?}");

    // sound alone, as MP3
    let mp3 = dir.join("part.mp3");
    clip::cut(&ff, std::slice::from_ref(&ticket.url), 5.0, 4.0, false, &Output::Mp3(128, Some("Song".into())), &mp3, |_| {}, &cancel)
        .await
        .unwrap();
    let (mlen, mpic) = clip::look(&ff, mp3.to_str().unwrap()).await.unwrap();
    assert!((mlen.unwrap() - 4.0).abs() < 0.15, "mp3 part lasts {mlen:?}");
    assert!(mpic.is_none());

    // a ticket that went: the address stops working
    let url = ticket.url.clone();
    drop(ticket);
    assert!(clip::look(&ff, &url).await.is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stopping_stops_ffmpeg() {
    let Some(ff) = ffmpeg() else { return };
    let dir = std::env::temp_dir().join(format!("ezoo-clip-stop-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let video = make_video(&dir);
    let cancel = CancellationToken::new();
    let c2 = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        c2.cancel();
    });
    let t = std::time::Instant::now();
    let r = clip::cut(&ff, &[video.to_string_lossy().into_owned()], 0.0, 120.0, true, &Output::Video, &dir.join("x.mp4"), |_| {}, &cancel).await;
    assert!(r.is_err() && r.unwrap_err().is::<swiftget_engine::http_download::Cancelled>());
    assert!(t.elapsed() < std::time::Duration::from_secs(5));
    let _ = std::fs::remove_dir_all(&dir);
}

/// A site with the files of a folder (ranges too).
async fn folder_site(dir: PathBuf) -> u16 {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = l.local_addr().unwrap().port();
    tokio::spawn(async move {
        loop {
            let (mut s, _) = l.accept().await.unwrap();
            let dir = dir.clone();
            tokio::spawn(async move {
                let mut buf = vec![0u8; 8192];
                let mut got = Vec::new();
                loop {
                    let n = s.read(&mut buf).await.unwrap_or(0);
                    if n == 0 {
                        return;
                    }
                    got.extend_from_slice(&buf[..n]);
                    if got.windows(4).any(|w| w == b"\r\n\r\n") {
                        break;
                    }
                }
                let head = String::from_utf8_lossy(&got).to_string();
                let path = head.split_whitespace().nth(1).unwrap_or("/").trim_start_matches('/').to_string();
                let Ok(data) = std::fs::read(dir.join(&path)) else {
                    let _ = s.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await;
                    return;
                };
                let total = data.len();
                let range = head.lines().find_map(|l| l.to_ascii_lowercase().strip_prefix("range: bytes=").map(str::to_string));
                let (a, b, code) = match range {
                    Some(r) => {
                        let (a, b) = r.trim().split_once('-').unwrap();
                        let b = if b.is_empty() { total - 1 } else { b.parse::<usize>().unwrap().min(total - 1) };
                        (a.parse::<usize>().unwrap(), b, "206 Partial Content")
                    }
                    None => (0, total - 1, "200 OK"),
                };
                let head = format!("HTTP/1.1 {code}\r\nContent-Range: bytes {a}-{b}/{total}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", b - a + 1);
                let _ = s.write_all(head.as_bytes()).await;
                let _ = s.write_all(&data[a..=b]).await;
            });
        }
    });
    port
}

async fn wait_done(mgr: &Arc<swiftget_engine::Manager>, id: u64) -> swiftget_engine::DownloadView {
    for _ in 0..9000 {
        let v = mgr.get(id).unwrap();
        if matches!(v.status, swiftget_engine::Status::Completed | swiftget_engine::Status::Failed) {
            return v;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    panic!("the part did not finish");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn parts_of_a_stream_and_of_separate_picture_and_sound() {
    let Some(ff) = ffmpeg() else { return };
    let root = std::env::temp_dir().join(format!("ezoo-clip-mgr-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let site_dir = root.join("site");
    std::fs::create_dir_all(&site_dir).unwrap();
    let video = make_video(&site_dir);
    let run = |args: &[&str]| {
        let ok = std::process::Command::new("ffmpeg").args(["-hide_banner", "-loglevel", "error", "-y"]).args(args).status().unwrap();
        assert!(ok.success());
    };
    let v = video.to_str().unwrap();
    std::fs::create_dir_all(site_dir.join("hls")).unwrap();
    let list = site_dir.join("hls/list.m3u8");
    run(&["-i", v, "-c", "copy", "-f", "hls", "-hls_time", "4", "-hls_playlist_type", "vod", list.to_str().unwrap()]);
    run(&["-i", v, "-an", "-c", "copy", site_dir.join("v.mp4").to_str().unwrap()]);
    run(&["-i", v, "-vn", "-c", "copy", site_dir.join("a.m4a").to_str().unwrap()]);
    let port = folder_site(site_dir.clone()).await;
    let mgr = swiftget_engine::Manager::new(tokio::runtime::Handle::current(), root.join("app")).unwrap();
    let out = root.join("out");

    // a stream: only the parts that play between 50 s and 56 s
    let id = mgr.add(swiftget_engine::NewDownload {
        url: format!("http://127.0.0.1:{port}/hls/list.m3u8"),
        file_name: Some("Stream.mp4".into()),
        save_dir: Some(out.clone()),
        clip: swiftget_engine::Clip::new(50.0, 56.0, true),
        start: true,
        ..Default::default()
    });
    let done = wait_done(&mgr, id).await;
    assert_eq!(done.status, swiftget_engine::Status::Completed, "{:?}", done.error);
    assert_eq!(done.file_name, "Stream [00-50 → 00-56].mp4");
    assert!(done.parts_total <= 3, "only the parts of the clip: {}", done.parts_total);
    let (len, pic) = clip::look(&ff, done.save_path.to_str().unwrap()).await.unwrap();
    assert!((len.unwrap() - 6.0).abs() < 0.2, "{len:?}");
    assert!(pic.is_some());

    // picture and sound apart (video page style), quick cut
    let id = mgr.add(swiftget_engine::NewDownload {
        url: format!("http://127.0.0.1:{port}/v.mp4"),
        audio_url: Some(format!("http://127.0.0.1:{port}/a.m4a")),
        file_name: Some("Video.webm".into()),
        save_dir: Some(out.clone()),
        clip: swiftget_engine::Clip::new(30.0, 40.0, false),
        start: true,
        ..Default::default()
    });
    let done = wait_done(&mgr, id).await;
    assert_eq!(done.status, swiftget_engine::Status::Completed, "{:?}", done.error);
    assert_eq!(done.file_name, "Video [00-30 → 00-40].mp4");
    let said = std::process::Command::new("ffmpeg").arg("-i").arg(&done.save_path).output().unwrap();
    let said = String::from_utf8_lossy(&said.stderr);
    assert!(said.contains("Video:") && said.contains("Audio:"), "{said}");
    let len = clip::parse_duration(&said).unwrap();
    assert!((9.9..=12.1).contains(&len), "{len}");
    let _ = std::fs::remove_dir_all(&root);
}
