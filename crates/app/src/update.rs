//! New versions of Ezoo Download from GitHub, always on: right after the
//! program opens it looks for a newer release; when there is one it is
//! downloaded, checked with SHA-256 and installed silently, and the program
//! starts again by itself. While the program stays open it looks again every
//! few hours (then it waits until no download is running).

use crate::i18n::tf;
use crate::message;
use crate::Ctx;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use swiftget_engine::{selfupdate, Manager, Status};

fn update_dir() -> PathBuf {
    swiftget_engine::default_data_dir().join("update")
}

fn downloading(mgr: &Manager) -> bool {
    mgr.snapshot()
        .iter()
        .any(|v| matches!(v.status, Status::Connecting | Status::Downloading | Status::Assembling))
}

/// Start the installer and close the program (downloads resume next time).
fn apply(setup: PathBuf) {
    match selfupdate::run_installer(&setup) {
        Ok(()) => {
            let _ = slint::quit_event_loop();
        }
        Err(e) => message(&tf("Could not start the update:\n\n{}", &[&format!("{e:#}")])),
    }
}

pub fn spawn(ctx: &Ctx) {
    // leftovers of an update that is installed now
    if let Ok(rd) = std::fs::read_dir(update_dir()) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if name.contains(&format!("-{}.", selfupdate::current())) || name.ends_with(".download") {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
    if !selfupdate::installed() {
        return; // a portable copy: there is no installation to update
    }
    let mgr = ctx.mgr.clone();
    ctx.rt.spawn(async move {
        // first check as soon as the program is open
        tokio::time::sleep(Duration::from_secs(3)).await;
        let mut first = true;
        loop {
            if round(&mgr, first).await {
                return;
            }
            first = false;
            tokio::time::sleep(Duration::from_secs(6 * 3600)).await;
        }
    });
}

/// One check. true = the installer was started.
async fn round(mgr: &Arc<Manager>, at_start: bool) -> bool {
    let client = mgr.client();
    let Ok(Some(rel)) = selfupdate::check(&client).await else {
        return false;
    };
    let Ok(setup) = selfupdate::download(&client, &rel, &update_dir()).await else {
        return false;
    };
    // later checks never cut a running download short; at start the
    // downloads simply go on after the update
    if !at_start {
        while downloading(mgr) {
            tokio::time::sleep(Duration::from_secs(30)).await;
        }
    }
    let _ = slint::invoke_from_event_loop(move || apply(setup));
    true
}
