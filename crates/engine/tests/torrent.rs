//! Torrents end to end: one session of librqbit shares a file, a second one
//! downloads it, a tiny tracker on 127.0.0.1 introduces them to each other.

use std::sync::{Arc, Mutex};
use std::time::Duration;
use swiftget_engine::bt::{Live, Torrents};
use swiftget_engine::torrent::{parse_torrent, AddOptions};
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

/// Wait until the torrent `g` of `t` has all its chosen files.
async fn complete(t: &Torrents, g: &str, secs: u64) -> bool {
    for _ in 0..secs * 10 {
        let st = t.status(g).await.unwrap();
        assert_ne!(st.status, "error", "{:?}", st.error);
        if st.seeding && st.total > 0 && st.done == st.total {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    false
}

/// A folder, a file, its .torrent, and a first session that shares it.
async fn shared_file(tag: &str) -> Option<(std::path::PathBuf, Vec<u8>, std::path::PathBuf, Arc<Torrents>, String, u16)> {
    let root = std::env::temp_dir().join(format!("ezoo-torrent-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
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
    let seeder = Arc::new(Torrents::new(&root.join("a")));
    let opts = AddOptions { dir: seed_dir.clone(), ..Default::default() };
    let sg = seeder.add(file.to_str().unwrap(), &opts).await.unwrap();
    assert!(complete(&seeder, &sg, 30).await, "the first session should share the file");
    Some((root, data, file, seeder, sg, port))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn downloads_a_torrent_and_goes_on_after_a_restart() {
    let Some((root, data, file, seeder, sg, _)) = shared_file("rpc").await else { return };
    let get_dir = root.join("get");
    let getter = Torrents::new(&root.join("b"));
    let opts = AddOptions { dir: get_dir.clone(), ..Default::default() };
    let gg = getter.add(file.to_str().unwrap(), &opts).await.unwrap();
    assert!(complete(&getter, &gg, 60).await, "the torrent should finish");
    assert_eq!(std::fs::read(get_dir.join("song.bin")).unwrap(), data);
    let files = getter.files(&gg).await.unwrap();
    assert_eq!(files[0].path, get_dir.join("song.bin"));
    assert_eq!(files[0].done, data.len() as u64);
    let st = getter.status(&gg).await.unwrap();
    assert!(st.pieces > 0 && swiftget_engine::torrent::piece_runs(&st.bitfield, st.pieces, 100).iter().all(|r| r.2), "{st:?}");
    // the people: the one that shares it
    let _ = getter.peers(&gg).await.unwrap();
    // a new tracker while it runs: started again, nothing checked from nothing
    let live = Live { trackers: vec!["udp://tracker.test:1/announce".into()], ..Default::default() };
    getter.change(&gg, &live).await.unwrap();
    assert!(getter.status(&gg).await.unwrap().trackers.iter().any(|t| t == "udp://tracker.test:1/announce"));
    assert!(complete(&getter, &gg, 30).await);
    // stopped, the program closed and opened again: it goes on from its notes
    getter.remove(&gg).await;
    drop(getter);
    tokio::time::sleep(Duration::from_millis(500)).await;
    let getter = Torrents::new(&root.join("b"));
    let gg2 = getter.add(file.to_str().unwrap(), &opts).await.unwrap();
    assert_eq!(gg, gg2);
    assert!(complete(&getter, &gg2, 30).await);
    // its file deleted while it was stopped: downloaded again, not taken as there
    getter.remove(&gg2).await;
    std::fs::remove_file(get_dir.join("song.bin")).unwrap();
    let gg3 = getter.add(file.to_str().unwrap(), &opts).await.unwrap();
    assert!(complete(&getter, &gg3, 60).await);
    assert_eq!(std::fs::read(get_dir.join("song.bin")).unwrap(), data);
    getter.forget(&gg3).await;
    seeder.forget(&sg).await;
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
    for _ in 0..600 {
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
    for _ in 0..600 {
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
    seeder.forget(&sg).await;
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
    for _ in 0..600 {
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
    seeder.forget(&sg).await;
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

    let seeder = Torrents::new(&root.join("a"));
    let sg = seeder.add(file.to_str().unwrap(), &AddOptions { dir: seed.clone(), ..Default::default() }).await.unwrap();
    assert!(complete(&seeder, &sg, 30).await);
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
    for _ in 0..600 {
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
    seeder.forget(&sg).await;
    let _ = std::fs::remove_dir_all(&root);
}

/// A magnet link: its file list (a .torrent file) comes before the
/// download, from the people who share it, so files can be chosen.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn magnet_link_file_list_first() {
    let Some((root, data, _file, seeder, sg, port)) = shared_file("meta").await else { return };
    let hash: String = ring::digest::digest(&ring::digest::SHA1_FOR_LEGACY_USE_ONLY, &make_info("song.bin", &data))
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let magnet = format!("magnet:?xt=urn:btih:{hash}&tr=http%3A%2F%2F127.0.0.1%3A{port}%2Fannounce");
    let getter = Torrents::new(&root.join("m"));
    let file = getter.metadata(&magnet, &root.join("listdir"), Duration::from_secs(60)).await.unwrap();
    let info = swiftget_engine::torrent::parse_torrent(&std::fs::read(&file).unwrap()).unwrap();
    assert_eq!(info.name, "song.bin");
    assert_eq!(info.total, data.len() as u64);
    // nothing of the file itself was downloaded
    assert!(!root.join("listdir").join("song.bin").exists());
    seeder.forget(&sg).await;
    let _ = std::fs::remove_dir_all(&root);
}

/// A torrent whose file names Windows can't keep (a device name, characters
/// a name can't have) is written under safe names; the files can be changed
/// while it runs; its magnet link can be given.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn unsafe_names_files_changed_while_running_and_its_magnet() {
    let root = std::env::temp_dir().join(format!("ezoo-torrent-safe-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let seed = root.join("seed");
    let a: Vec<u8> = (0..120_000u32).map(|i| (i % 249) as u8).collect();
    let b: Vec<u8> = (0..70_000u32).map(|i| (i % 239) as u8).collect();
    let port = tracker().await;
    // the same as make_multi, with names Windows refuses
    let piece = 16 * 1024;
    let all: Vec<u8> = a.iter().chain(&b).copied().collect();
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
    info.extend(file(a.len(), "CON"));
    info.extend(file(b.len(), "a:b?.bin"));
    info.extend(b"e4:name");
    info.extend(ben(b"pack"));
    info.extend(b"12:piece lengthi16384e6:pieces");
    info.extend(ben(&pieces));
    info.extend(b"e");
    let mut t = b"d8:announce".to_vec();
    t.extend(ben(format!("http://127.0.0.1:{port}/announce").as_bytes()));
    t.extend(b"4:info");
    t.extend(info);
    t.extend(b"e");
    let tfile = root.join("pack.torrent");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(&tfile, &t).unwrap();
    let parsed = parse_torrent(&t).unwrap();
    let safe = swiftget_engine::torrent::safe_paths(&parsed);
    assert_eq!(safe.len(), 2, "{safe:?}");
    // the computer that shares it keeps the files under the safe names too
    for ((_, rel), data) in safe.iter().zip([&a, &b]) {
        let p = seed.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, data).unwrap();
    }
    let seeder = Torrents::new(&root.join("a"));
    let sg = seeder.add(tfile.to_str().unwrap(), &AddOptions { dir: seed.clone(), ..Default::default() }).await.unwrap();
    assert!(complete(&seeder, &sg, 30).await, "the safe names are the ones it reads too");
    // slowly, so the change below comes while it runs
    seeder.set_limits(0, 40 * 1024).await;
    let mgr = swiftget_engine::Manager::new(tokio::runtime::Handle::current(), root.join("app")).unwrap();
    let get = root.join("get");
    let id = mgr.add(swiftget_engine::NewDownload {
        url: tfile.to_string_lossy().into_owned(),
        save_dir: Some(get.clone()),
        start: true,
        torrent_select: Some("1".into()),
        ..Default::default()
    });
    // its magnet link: its id and name
    let m = mgr.magnet_link(id).unwrap();
    assert!(m.contains(&parsed.info_hash.clone().unwrap()) && m.contains("dn=pack"), "{m}");
    // while it runs: both files wanted after all
    tokio::time::sleep(Duration::from_millis(1500)).await;
    mgr.set_torrent_files(id, None);
    mgr.set_torrent_first_last(id, true);
    let mut v = mgr.get(id).unwrap();
    for _ in 0..900 {
        v = mgr.get(id).unwrap();
        if matches!(v.status, swiftget_engine::Status::Completed | swiftget_engine::Status::Failed) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(v.status, swiftget_engine::Status::Completed, "{:?}", v.error);
    for ((_, rel), data) in safe.iter().zip([&a, &b]) {
        assert_eq!(&std::fs::read(get.join(rel)).unwrap(), data, "{rel}");
    }
    assert!(!get.join("pack/CON").exists() && !get.join("pack/a:b?.bin").exists());
    let live = mgr.torrent_live(id).unwrap();
    assert!(!live.pieces.is_empty());
    seeder.forget(&sg).await;
    let _ = std::fs::remove_dir_all(&root);
}

/// Complete with no sharing wanted: it is let go of at once, and the bits
/// of files that were not chosen go with it.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn no_sharing_lets_go_and_cleans_up() {
    let Some((root, _data, file, seeder, sg, _)) = shared_file("share").await else { return };
    let left = root.join("seed").join("left.bin");
    std::fs::write(&left, b"bits").unwrap();
    seeder.share_then_forget(&sg, 0, 0.0, vec![left.clone()]);
    let mut gone = false;
    for _ in 0..50 {
        if seeder.status(&sg).await.is_err() {
            gone = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(gone, "forgotten");
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(!left.exists(), "the leftover is deleted");
    assert!(root.join("seed").join("song.bin").exists(), "the file itself stays");
    // a later start of the same torrent is not ended by an older sharing
    let sg2 = seeder.add(file.to_str().unwrap(), &AddOptions { dir: root.join("seed"), ..Default::default() }).await.unwrap();
    seeder.share_then_forget(&sg2, 30, 0.0, Vec::new());
    let sg3 = seeder.add(file.to_str().unwrap(), &AddOptions { dir: root.join("seed"), first_last: true, ..Default::default() }).await.unwrap();
    assert!(complete(&seeder, &sg3, 30).await);
    seeder.forget(&sg3).await;
    let _ = std::fs::remove_dir_all(&root);
}

/// A torrent whose names lead out of its folder ("..") is refused, with words
/// that say why.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_way_out_of_the_folder_is_refused() {
    let root = std::env::temp_dir().join(format!("ezoo-torrent-out-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let data = vec![7u8; 1000];
    let pieces = ring::digest::digest(&ring::digest::SHA1_FOR_LEGACY_USE_ONLY, &data);
    let mut info = b"d5:filesld6:lengthi1000e4:pathl2:..8:evil.exeeee4:name4:pack12:piece lengthi16384e6:pieces".to_vec();
    info.extend(ben(pieces.as_ref()));
    info.extend(b"e");
    let mut t = b"d4:info".to_vec();
    t.extend(info);
    t.extend(b"e");
    let tfile = root.join("out.torrent");
    std::fs::write(&tfile, &t).unwrap();
    let mgr = swiftget_engine::Manager::new(tokio::runtime::Handle::current(), root.join("app")).unwrap();
    let id = mgr.add(swiftget_engine::NewDownload {
        url: tfile.to_string_lossy().into_owned(),
        save_dir: Some(root.join("get")),
        start: true,
        ..Default::default()
    });
    let mut v = mgr.get(id).unwrap();
    for _ in 0..100 {
        v = mgr.get(id).unwrap();
        if matches!(v.status, swiftget_engine::Status::Completed | swiftget_engine::Status::Failed) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(v.status, swiftget_engine::Status::Failed);
    let e = v.error.unwrap_or_default();
    assert!(e.contains("lead out of its folder"), "{e}");
    assert!(!root.join("evil.exe").exists());
    let _ = std::fs::remove_dir_all(&root);
}
