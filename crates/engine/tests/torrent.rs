//! Torrents end to end with the real aria2c (skipped when it isn't
//! installed): one aria2 shares a file, a second one downloads it, a tiny
//! tracker on 127.0.0.1 introduces them to each other.

use std::sync::{Arc, Mutex};
use std::time::Duration;
use swiftget_engine::torrent::{parse_torrent, AddOptions, Aria2};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

fn ben(s: &[u8]) -> Vec<u8> {
    let mut v = format!("{}:", s.len()).into_bytes();
    v.extend_from_slice(s);
    v
}

/// The info part of a .torrent of `data` (single file `name`).
fn make_info(name: &str, data: &[u8]) -> Vec<u8> {
    let piece = 16 * 1024;
    let mut pieces = Vec::new();
    for c in data.chunks(piece) {
        pieces.extend_from_slice(ring::digest::digest(&ring::digest::SHA1_FOR_LEGACY_USE_ONLY, c).as_ref());
    }
    let mut info = b"d".to_vec();
    info.extend(b"6:length");
    info.extend(format!("i{}e", data.len()).into_bytes());
    info.extend(b"4:name");
    info.extend(ben(name.as_bytes()));
    info.extend(b"12:piece length");
    info.extend(format!("i{piece}e").into_bytes());
    info.extend(b"6:pieces");
    info.extend(ben(&pieces));
    info.extend(b"e");
    info
}

/// A real .torrent of `data` (single file `name`) with this tracker.
fn make_torrent(name: &str, data: &[u8], tracker: &str) -> Vec<u8> {
    let info = make_info(name, data);
    let mut t = b"d8:announce".to_vec();
    t.extend(ben(tracker.as_bytes()));
    t.extend(b"4:info");
    t.extend(info);
    t.extend(b"e");
    t
}

/// The smallest HTTP tracker: everyone who announces gets everyone else.
async fn tracker() -> u16 {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = l.local_addr().unwrap().port();
    let peers: Arc<Mutex<Vec<u16>>> = Arc::default();
    tokio::spawn(async move {
        loop {
            let (mut s, _) = l.accept().await.unwrap();
            let peers = peers.clone();
            tokio::spawn(async move {
                let mut buf = vec![0u8; 4096];
                let n = s.read(&mut buf).await.unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]).to_string();
                let p: u16 = req
                    .split(['?', '&', ' '])
                    .find_map(|kv| kv.strip_prefix("port=").and_then(|v| v.parse().ok()))
                    .unwrap_or(0);
                let list = {
                    let mut ps = peers.lock().unwrap();
                    if p > 0 && !ps.contains(&p) {
                        ps.push(p);
                    }
                    ps.clone()
                };
                let mut compact = Vec::new();
                for q in list.iter().filter(|q| **q != p) {
                    compact.extend_from_slice(&[127, 0, 0, 1]);
                    compact.extend_from_slice(&q.to_be_bytes());
                }
                let mut body = b"d8:intervali2e5:peers".to_vec();
                body.extend(ben(&compact));
                body.extend(b"e");
                let head = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
                let _ = s.write_all(head.as_bytes()).await;
                let _ = s.write_all(&body).await;
            });
        }
    });
    port
}

/// A folder, a file, its .torrent, and a first aria2 that shares it.
async fn shared_file(tag: &str) -> Option<(std::path::PathBuf, Vec<u8>, std::path::PathBuf, Aria2, String, u16)> {
    if swiftget_engine::mux::find_tool("aria2c").is_none() {
        eprintln!("aria2c not installed: skipped");
        return None;
    }
    let root = std::env::temp_dir().join(format!("ezoo-torrent-{tag}-{}", std::process::id()));
    let seed_dir = root.join("seed");
    std::fs::create_dir_all(&seed_dir).unwrap();
    // different bytes for every test: different torrents, no mix-ups
    let salt = tag.bytes().map(u32::from).sum::<u32>();
    let data: Vec<u8> = (0..300_000u32).map(|i| ((i * 7 + salt) % 253) as u8).collect();
    std::fs::write(seed_dir.join("song.bin"), &data).unwrap();
    let port = tracker().await;
    let file = root.join("song.torrent");
    std::fs::write(&file, make_torrent("song.bin", &data, &format!("http://127.0.0.1:{port}/announce"))).unwrap();
    let info = parse_torrent(&std::fs::read(&file).unwrap()).unwrap();
    assert_eq!(info.name, "song.bin");
    assert_eq!(info.total, data.len() as u64);

    // the computer that has it: checks its copy, then shares
    let seeder = Aria2::new(&root.join("a"));
    let opts = AddOptions { dir: seed_dir.clone(), seed_minutes: 30, check: true, ..Default::default() };
    let sg = seeder.add(file.to_str().unwrap(), &opts).await.unwrap();
    for _ in 0..100 {
        let st = seeder.status(&sg).await.unwrap();
        if st.seeding && st.done == data.len() as u64 {
            return Some((root, data, file, seeder, sg, port));
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("the first aria2 should share the file");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn downloads_a_torrent_with_aria2() {
    let Some((root, data, file, seeder, sg, _)) = shared_file("rpc").await else { return };
    let get_dir = root.join("get");
    let getter = Aria2::new(&root.join("b"));
    let opts = AddOptions { dir: get_dir.clone(), seed_minutes: 0, ..Default::default() };
    let gg = getter.add(file.to_str().unwrap(), &opts).await.unwrap();
    let mut done = false;
    for _ in 0..300 {
        let st = getter.status(&gg).await.unwrap();
        assert_ne!(st.status, "error", "{:?}", st.error);
        if st.status == "complete" {
            done = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(done, "the torrent should finish");
    assert_eq!(std::fs::read(get_dir.join("song.bin")).unwrap(), data);
    let files = getter.files(&gg).await.unwrap();
    assert_eq!(files[0].path, get_dir.join("song.bin"));
    assert_eq!(files[0].done, data.len() as u64);
    getter.remove(&gg).await;
    seeder.remove(&sg).await;
    let _ = std::fs::remove_dir_all(&root);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn torrent_in_the_download_list() {
    let Some((root, data, file, seeder, sg, _)) = shared_file("list").await else { return };
    let mgr = swiftget_engine::Manager::new(tokio::runtime::Handle::current(), root.join("app")).unwrap();
    let list_dir = root.join("list");
    let id = mgr.add(swiftget_engine::NewDownload {
        url: file.to_string_lossy().into_owned(),
        save_dir: Some(list_dir.clone()),
        start: true,
        ..Default::default()
    });
    let v = mgr.get(id).unwrap();
    assert_eq!(v.kind, swiftget_engine::DownloadKind::Torrent);
    assert_eq!(v.file_name, "song.bin");
    assert_eq!(v.total_size, Some(data.len() as u64));
    let mut status = v.status;
    for _ in 0..300 {
        status = mgr.get(id).unwrap().status;
        if matches!(status, swiftget_engine::Status::Completed | swiftget_engine::Status::Failed) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let v = mgr.get(id).unwrap();
    assert_eq!(status, swiftget_engine::Status::Completed, "{:?}", v.error);
    assert_eq!(std::fs::read(list_dir.join("song.bin")).unwrap(), data);
    // the same torrent again into the same folder: the file there is checked, not refused
    let again = mgr.add(swiftget_engine::NewDownload {
        url: file.to_string_lossy().into_owned(),
        save_dir: Some(list_dir.clone()),
        start: true,
        ..Default::default()
    });
    let mut st2 = swiftget_engine::Status::Queued;
    for _ in 0..300 {
        st2 = mgr.get(again).unwrap().status;
        if matches!(st2, swiftget_engine::Status::Completed | swiftget_engine::Status::Failed) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(st2, swiftget_engine::Status::Completed, "{:?}", mgr.get(again).unwrap().error);
    mgr.remove(again, false);
    // delete with the file: exactly the torrent's file goes
    std::fs::write(list_dir.join("mine.txt"), b"keep").unwrap();
    mgr.remove(id, true);
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert!(!list_dir.join("song.bin").exists());
    assert!(list_dir.join("mine.txt").exists());
    seeder.remove(&sg).await;
    let _ = std::fs::remove_dir_all(&root);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn magnet_link_learns_its_name() {
    let Some((root, data, _file, seeder, sg, port)) = shared_file("magnet").await else { return };
    let hash: String = ring::digest::digest(&ring::digest::SHA1_FOR_LEGACY_USE_ONLY, &make_info("song.bin", &data))
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let magnet = format!("magnet:?xt=urn:btih:{hash}&tr=http%3A%2F%2F127.0.0.1%3A{port}%2Fannounce");
    let mgr = swiftget_engine::Manager::new(tokio::runtime::Handle::current(), root.join("app")).unwrap();
    let list_dir = root.join("list");
    let id = mgr.add(swiftget_engine::NewDownload { url: magnet, save_dir: Some(list_dir.clone()), start: true, ..Default::default() });
    assert_eq!(mgr.get(id).unwrap().file_name, "torrent");
    let mut status = swiftget_engine::Status::Queued;
    for _ in 0..300 {
        status = mgr.get(id).unwrap().status;
        if matches!(status, swiftget_engine::Status::Completed | swiftget_engine::Status::Failed) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let v = mgr.get(id).unwrap();
    assert_eq!(status, swiftget_engine::Status::Completed, "{:?}", v.error);
    assert_eq!(v.file_name, "song.bin", "the real name once the file list came");
    assert_eq!(std::fs::read(list_dir.join("song.bin")).unwrap(), data);
    seeder.remove(&sg).await;
    let _ = std::fs::remove_dir_all(&root);
}

/// A torrent of two files ("pack/a.bin", "pack/b.bin").
fn make_multi(a: &[u8], b: &[u8], tracker: &str) -> (Vec<u8>, Vec<u8>) {
    let piece = 16 * 1024;
    let all: Vec<u8> = a.iter().chain(b).copied().collect();
    let mut pieces = Vec::new();
    for c in all.chunks(piece) {
        pieces.extend_from_slice(ring::digest::digest(&ring::digest::SHA1_FOR_LEGACY_USE_ONLY, c).as_ref());
    }
    let file = |len: usize, name: &str| {
        let mut f = b"d6:length".to_vec();
        f.extend(format!("i{len}e").into_bytes());
        f.extend(b"4:pathl");
        f.extend(ben(name.as_bytes()));
        f.extend(b"ee");
        f
    };
    let mut info = b"d5:filesl".to_vec();
    info.extend(file(a.len(), "a.bin"));
    info.extend(file(b.len(), "b.bin"));
    info.extend(b"e4:name");
    info.extend(ben(b"pack"));
    info.extend(b"12:piece lengthi16384e6:pieces");
    info.extend(ben(&pieces));
    info.extend(b"e");
    let mut t = b"d8:announce".to_vec();
    t.extend(ben(tracker.as_bytes()));
    t.extend(b"4:info");
    t.extend(info.clone());
    t.extend(b"e");
    (t, info)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn only_the_checked_files_of_a_torrent() {
    if swiftget_engine::mux::find_tool("aria2c").is_none() {
        return;
    }
    let root = std::env::temp_dir().join(format!("ezoo-torrent-multi-{}", std::process::id()));
    let seed = root.join("seed");
    std::fs::create_dir_all(seed.join("pack")).unwrap();
    let a: Vec<u8> = (0..200_000u32).map(|i| (i % 251) as u8).collect();
    let b: Vec<u8> = (0..90_000u32).map(|i| (i % 241) as u8).collect();
    std::fs::write(seed.join("pack/a.bin"), &a).unwrap();
    std::fs::write(seed.join("pack/b.bin"), &b).unwrap();
    let port = tracker().await;
    let (t, _) = make_multi(&a, &b, &format!("http://127.0.0.1:{port}/announce"));
    let file = root.join("pack.torrent");
    std::fs::write(&file, t).unwrap();
    let info = parse_torrent(&std::fs::read(&file).unwrap()).unwrap();
    assert_eq!(info.files.iter().map(|f| f.index).collect::<Vec<_>>(), [1, 2]);

    let seeder = Aria2::new(&root.join("a"));
    let sg = seeder.add(file.to_str().unwrap(), &AddOptions { dir: seed.clone(), seed_minutes: 30, check: true, ..Default::default() }).await.unwrap();
    let mgr = swiftget_engine::Manager::new(tokio::runtime::Handle::current(), root.join("app")).unwrap();
    let get = root.join("get");
    let id = mgr.add(swiftget_engine::NewDownload {
        url: file.to_string_lossy().into_owned(),
        save_dir: Some(get.clone()),
        start: true,
        torrent_select: Some("2".into()),
        ..Default::default()
    });
    let mut status = swiftget_engine::Status::Queued;
    let mut total = None;
    for _ in 0..300 {
        let v = mgr.get(id).unwrap();
        status = v.status;
        total = v.total_size;
        if matches!(status, swiftget_engine::Status::Completed | swiftget_engine::Status::Failed) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(status, swiftget_engine::Status::Completed, "{:?}", mgr.get(id).unwrap().error);
    // its own size, not the pieces it shares with the other file
    assert_eq!(total, Some(b.len() as u64));
    assert_eq!(std::fs::read(get.join("pack/b.bin")).unwrap(), b);
    assert!(!get.join("pack/a.bin").exists(), "the unchecked file is not kept");
    seeder.remove(&sg).await;
    let _ = std::fs::remove_dir_all(&root);
}
