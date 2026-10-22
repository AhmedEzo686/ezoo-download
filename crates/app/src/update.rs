//! New versions of Ezoo Download from GitHub, always on: right after the
//! program opens it looks for a newer release; when there is one it is
//! downloaded, checked with SHA-256 and installed silently, and the program
//! starts again by itself. While the program stays open it looks again every
//! few hours (then it waits until no download is running).
//! Help > Check for updates does the same at once, and says what it found.

use crate::i18n::{t, tf};
use crate::message;
use crate::Ctx;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use swiftget_engine::{selfupdate, Manager, Status};

/// one check / download at a time (automatic or from the menu)
static BUSY: AtomicBool = AtomicBool::new(false);

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
            if !BUSY.swap(true, Ordering::SeqCst) {
                let started = round(&mgr, first).await;
                BUSY.store(false, Ordering::SeqCst);
                if started {
                    return;
                }
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

/// Help > Check for updates: look now instead of waiting for the next check.
pub fn check_now(ctx: &Ctx) {
    if BUSY.swap(true, Ordering::SeqCst) {
        message(&t("A new version is being prepared. It is installed by itself when it is ready."));
        return;
    }
    let client = ctx.mgr.client();
    ctx.rt.spawn(async move {
        let found = selfupdate::check(&client).await;
        let rel = match found {
            Err(e) => {
                BUSY.store(false, Ordering::SeqCst);
                let text = tf("Could not check for updates:\n\n{}", &[&format!("{e:#}")]);
                let _ = slint::invoke_from_event_loop(move || message(&text));
                return;
            }
            Ok(None) => {
                BUSY.store(false, Ordering::SeqCst);
                let text = tf("Ezoo Download is up to date (version {}).", &[&selfupdate::current()]);
                let _ = slint::invoke_from_event_loop(move || message(&text));
                return;
            }
            Ok(Some(rel)) => rel,
        };
        if !selfupdate::installed() {
            BUSY.store(false, Ordering::SeqCst);
            let text = tf(
                "Version {} is available. This copy was not installed with the setup, so it cannot update itself.",
                &[&rel.version],
            );
            let _ = slint::invoke_from_event_loop(move || message(&text));
            return;
        }
        let text = tf(
            "Version {} is available. It is being prepared now and installed by itself; Ezoo Download opens again after that.",
            &[&rel.version],
        );
        let _ = slint::invoke_from_event_loop(move || message(&text));
        let got = selfupdate::download(&client, &rel, &update_dir()).await;
        BUSY.store(false, Ordering::SeqCst);
        match got {
            Ok(setup) => {
                let _ = slint::invoke_from_event_loop(move || apply(setup));
            }
            Err(e) => {
                let text = tf("Could not get the update:\n\n{}", &[&format!("{e:#}")]);
                let _ = slint::invoke_from_event_loop(move || message(&text));
            }
        }
    });
}
