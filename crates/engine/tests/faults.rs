//! The download list against a server that goes wrong on purpose
//! (tests/support/faults.rs): every time the file must come whole and right,
//! or the download must say plainly why it can't.

#[path = "support/faults.rs"]
mod faults;

use faults::{file_data, start, Faults};
use std::sync::Arc;
use std::time::Duration;
use swiftget_engine::{Manager, NewDownload, Status};

fn manager(tag: &str) -> (Arc<Manager>, std::path::PathBuf) {
    let root = std::env::temp_dir().join(format!("ezoo-faults-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let mgr = Manager::new(tokio::runtime::Handle::current(), root.join("app")).unwrap();
    (mgr, root)
}

async fn run_to_end(mgr: &Arc<Manager>, url: String, dir: &std::path::Path) -> swiftget_engine::DownloadView {
    let id = mgr.add(NewDownload { url, save_dir: Some(dir.to_path_buf()), file_name: Some("f.bin".into()), start: true, connections: Some(8), ..Default::default() });
    wait(mgr, id).await
}

async fn wait(mgr: &Arc<Manager>, id: u64) -> swiftget_engine::DownloadView {
    for _ in 0..3000 {
        let v = mgr.get(id).unwrap();
        if matches!(v.status, Status::Completed | Status::Failed) {
            return v;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("the download did not end: {:?}", mgr.get(id).map(|v| (v.status, v.error)));
}

async fn whole(tag: &str, faults: Faults) {
    let data = file_data(2 * 1024 * 1024 + 77, 1);
    let srv = start(data.clone(), faults).await;
    let (mgr, root) = manager(tag);
    let v = run_to_end(&mgr, srv.url("f.bin"), &root.join("out")).await;
    assert_eq!(v.status, Status::Completed, "{tag}: {:?}", v.error);
    assert!(std::fs::read(&v.save_path).unwrap() == data, "{tag}: the file differs");
    let _ = std::fs::remove_dir_all(&root);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn connections_cut_half_way() {
    whole("cut", Faults { cut_after: Some(100 * 1024), cuts: Some(8), ..Default::default() }).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_server_without_ranges() {
    whole("noranges", Faults { no_ranges: true, ..Default::default() }).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_part_other_than_the_one_asked_for() {
    whole("wrongpart", Faults { wrong_part_once: Some(4096), ..Default::default() }).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_server_that_allows_two_connections() {
    whole("twoconn", Faults { max_connections: Some(2), rate: 4 << 20, ..Default::default() }).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_busy_server_at_first() {
    whole("busy", Faults { busy_first: 2, ..Default::default() }).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_version_name_kept_through_cuts() {
    whole("etag", Faults { etag: Some("\"v1\"".into()), cut_after: Some(200 * 1024), cuts: Some(4), ..Default::default() }).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_robot_check_is_told() {
    let srv = start(file_data(1000, 2), Faults { robot_check: true, ..Default::default() }).await;
    let (mgr, root) = manager("robot");
    let v = run_to_end(&mgr, srv.url("f.bin"), &root.join("out")).await;
    assert_eq!(v.status, Status::Failed);
    assert!(v.error.unwrap_or_default().contains("not a robot"));
    let _ = std::fs::remove_dir_all(&root);
}

/// Paused, and the file on the server changed meanwhile (no version name):
/// the download says so instead of mixing two files.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_file_that_changed_while_paused() {
    let data = file_data(3 * 1024 * 1024, 3);
    let srv = start(data.clone(), Faults { rate: 3 << 20, ..Default::default() }).await;
    let (mgr, root) = manager("changed");
    let id = mgr.add(NewDownload {
        url: srv.url("f.bin"),
        save_dir: Some(root.join("out")),
        file_name: Some("f.bin".into()),
        start: true,
        connections: Some(4),
        ..Default::default()
    });
    for _ in 0..500 {
        if mgr.get(id).map(|v| v.downloaded > 512 * 1024).unwrap_or(false) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    mgr.pause(id);
    tokio::time::sleep(Duration::from_millis(400)).await;
    {
        let mut f = srv.faults.lock().unwrap();
        f.changes_after = Some(srv.requests.load(std::sync::atomic::Ordering::SeqCst));
        f.rate = 0;
    }
    mgr.start(id);
    let v = wait(&mgr, id).await;
    assert_eq!(v.status, Status::Failed, "a changed file is never finished as if it were the same");
    assert!(v.error.unwrap_or_default().contains("changed since"));
    let _ = std::fs::remove_dir_all(&root);
}

/// Files added together (a playlist, a batch): the group is busy while any of
/// them runs, then says how many finished and failed.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn files_added_together_are_one_group() {
    let data = file_data(400_000, 9);
    let s = start(data.clone(), Faults { rate: 400_000, ..Default::default() }).await;
    let (mgr, root) = manager("group");
    let dir = root.join("get");
    let g = mgr.new_group();
    assert_ne!(g, mgr.new_group());
    let ids: Vec<_> = ["a.bin", "b.bin"]
        .iter()
        .map(|n| {
            mgr.add(NewDownload {
                url: s.url(n),
                save_dir: Some(dir.clone()),
                group: Some(g),
                start: true,
                ..Default::default()
            })
        })
        .collect();
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert!(mgr.group_state(g).3, "busy while they run");
    for _ in 0..300 {
        if !mgr.group_state(g).3 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(mgr.group_state(g), (2, 2, 0, false));
    for id in ids {
        assert_eq!(mgr.get(id).unwrap().group, Some(g));
    }
    assert!(mgr.finishing().is_none());
    let _ = std::fs::remove_dir_all(&root);
}
